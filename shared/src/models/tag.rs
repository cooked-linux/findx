//! Tagging and color label models.

use std::fmt;
use std::str::FromStr;

use crate::errors::ModelError;

/// Canonical macOS color labels supported by the tag engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TagColor {
    /// Red color label (index 6, `#ff3b30`).
    Red,
    /// Orange color label (index 7, `#ff9500`).
    Orange,
    /// Yellow color label (index 5, `#ffcc00`).
    Yellow,
    /// Green color label (index 2, `#34c759`).
    Green,
    /// Blue color label (index 4, `#007aff`).
    Blue,
    /// Purple color label (index 3, `#af52de`).
    Purple,
    /// Gray color label (index 1, `#8e8e93`).
    Gray,
}

impl TagColor {
    /// All 7 canonical tag colors in index order (1 through 7).
    pub const ALL: [Self; 7] = [
        Self::Gray,
        Self::Green,
        Self::Purple,
        Self::Blue,
        Self::Yellow,
        Self::Red,
        Self::Orange,
    ];

    /// Returns the legacy numeric index (1 to 7) associated with this color label.
    #[must_use]
    pub const fn color_index(&self) -> u8 {
        match self {
            Self::Gray => 1,
            Self::Green => 2,
            Self::Purple => 3,
            Self::Blue => 4,
            Self::Yellow => 5,
            Self::Red => 6,
            Self::Orange => 7,
        }
    }

    /// Resolves a color label from its legacy numeric index (1 to 7).
    #[must_use]
    pub const fn from_index(index: u8) -> Option<Self> {
        match index {
            1 => Some(Self::Gray),
            2 => Some(Self::Green),
            3 => Some(Self::Purple),
            4 => Some(Self::Blue),
            5 => Some(Self::Yellow),
            6 => Some(Self::Red),
            7 => Some(Self::Orange),
            _ => None,
        }
    }

    /// Returns the standard 6-digit hex color code string (e.g. `"#ff3b30"`).
    #[must_use]
    pub const fn hex_code(&self) -> &'static str {
        match self {
            Self::Gray => "#8e8e93",
            Self::Green => "#34c759",
            Self::Purple => "#af52de",
            Self::Blue => "#007aff",
            Self::Yellow => "#ffcc00",
            Self::Red => "#ff3b30",
            Self::Orange => "#ff9500",
        }
    }

    /// Returns the capitalized display name of the color (e.g. `"Red"`).
    #[must_use]
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::Gray => "Gray",
            Self::Green => "Green",
            Self::Purple => "Purple",
            Self::Blue => "Blue",
            Self::Yellow => "Yellow",
            Self::Red => "Red",
            Self::Orange => "Orange",
        }
    }

    /// Matches a case-insensitive string against known color names.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "gray" | "grey" => Some(Self::Gray),
            "green" => Some(Self::Green),
            "purple" => Some(Self::Purple),
            "blue" => Some(Self::Blue),
            "yellow" => Some(Self::Yellow),
            "red" => Some(Self::Red),
            "orange" => Some(Self::Orange),
            _ => None,
        }
    }
}

impl fmt::Display for TagColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

impl FromStr for TagColor {
    type Err = ModelError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        if let Some(color) = Self::from_name(trimmed) {
            return Ok(color);
        }
        if let Some(color) = trimmed.parse::<u8>().ok().and_then(Self::from_index) {
            return Ok(color);
        }
        for color in &Self::ALL {
            if color.hex_code().eq_ignore_ascii_case(trimmed) {
                return Ok(*color);
            }
        }
        Err(ModelError::invalid_tag_color(s))
    }
}

/// A tag or label applied to a file or folder.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FileTag {
    /// Display name of the tag (e.g. `"Work"`, `"Invoice"`).
    pub name: String,
    /// Optional associated color label.
    pub color: Option<TagColor>,
}

impl FileTag {
    /// Constructs a new [`FileTag`].
    #[must_use]
    pub fn new(name: impl Into<String>, color: Option<TagColor>) -> Self {
        Self {
            name: name.into(),
            color,
        }
    }

    /// Constructs a tag from a name, automatically inferring the color if the name
    /// matches one of the canonical color names (e.g., `"Red"` -> `Some(TagColor::Red)`).
    #[must_use]
    pub fn with_name(name: impl Into<String>) -> Self {
        let name_str = name.into();
        let color = TagColor::from_name(&name_str);
        Self {
            name: name_str,
            color,
        }
    }

    /// Parses an extended attribute string (XDG comma/newline or macOS `"Name\nIndex"` format)
    /// into a list of [`FileTag`] instances.
    ///
    /// # Formats Supported:
    /// - macOS style: `"Work\n6\nImportant\n1"`
    /// - XDG newline style: `"ProjectX\nUrgent\nRed"`
    /// - XDG comma style: `"ProjectX, Urgent, Red"`
    #[must_use]
    pub fn parse_xdg_string(input: &str) -> Vec<Self> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }

        // If input contains newlines, check for macOS style pair (Name followed by single digit 1-7)
        if trimmed.contains('\n') {
            let lines: Vec<&str> = trimmed
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect();

            let mut tags = Vec::new();
            let mut i = 0;
            while i < lines.len() {
                let current = lines[i];

                // Check if the next line is a single-digit color index (macOS format: "Name\n6")
                if let Some(color) = lines
                    .get(i + 1)
                    .and_then(|nl| nl.parse::<u8>().ok())
                    .and_then(TagColor::from_index)
                {
                    tags.push(Self::new(current, Some(color)));
                    i += 2;
                    continue;
                }

                // Otherwise, treat as individual line tag
                tags.push(Self::with_name(current));
                i += 1;
            }
            return tags;
        }

        // Handle comma-separated list
        trimmed
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(Self::with_name)
            .collect()
    }

    /// Formats tags into an XDG standard newline-delimited string for `user.xdg.tags`.
    #[must_use]
    pub fn to_xdg_string(tags: &[Self]) -> String {
        let mut out = String::new();
        for (i, tag) in tags.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&tag.name);
        }
        out
    }

    /// Formats tags into macOS compatibility format `"TagName\nColorIndex\n..."`.
    #[must_use]
    pub fn to_macos_string(tags: &[Self]) -> String {
        let mut out = String::new();
        for (i, tag) in tags.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&tag.name);
            if let Some(color) = tag.color {
                out.push('\n');
                out.push_str(&color.color_index().to_string());
            }
        }
        out
    }
}

impl fmt::Display for FileTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_color_attributes() {
        assert_eq!(TagColor::Red.hex_code(), "#ff3b30");
        assert_eq!(TagColor::Red.display_name(), "Red");
        assert_eq!(TagColor::Red.color_index(), 6);
        assert_eq!(TagColor::from_index(6), Some(TagColor::Red));
        assert_eq!(TagColor::from_index(1), Some(TagColor::Gray));
        assert_eq!(TagColor::from_index(7), Some(TagColor::Orange));
        assert_eq!(TagColor::from_index(0), None);
        assert_eq!(TagColor::from_index(8), None);
    }

    #[test]
    fn test_tag_color_parsing() {
        assert_eq!("Red".parse::<TagColor>().unwrap(), TagColor::Red);
        assert_eq!("red".parse::<TagColor>().unwrap(), TagColor::Red);
        assert_eq!("6".parse::<TagColor>().unwrap(), TagColor::Red);
        assert_eq!("#ff3b30".parse::<TagColor>().unwrap(), TagColor::Red);
        assert_eq!("blue".parse::<TagColor>().unwrap(), TagColor::Blue);
        assert_eq!("Gray".parse::<TagColor>().unwrap(), TagColor::Gray);
        assert_eq!("grey".parse::<TagColor>().unwrap(), TagColor::Gray);

        assert!("InvalidColor".parse::<TagColor>().is_err());
        assert!("99".parse::<TagColor>().is_err());
    }

    #[test]
    fn test_parse_xdg_string_comma() {
        let tags = FileTag::parse_xdg_string("Work, Urgent, Red");
        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0], FileTag::new("Work", None));
        assert_eq!(tags[1], FileTag::new("Urgent", None));
        assert_eq!(tags[2], FileTag::new("Red", Some(TagColor::Red)));
    }

    #[test]
    fn test_parse_xdg_string_macos_paired() {
        let raw = "Work\n6\nInvoices\n2";
        let tags = FileTag::parse_xdg_string(raw);
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0], FileTag::new("Work", Some(TagColor::Red)));
        assert_eq!(tags[1], FileTag::new("Invoices", Some(TagColor::Green)));
    }

    #[test]
    fn test_parse_xdg_string_newline_unpaired() {
        let raw = "Taxes\nReceipts\nBlue";
        let tags = FileTag::parse_xdg_string(raw);
        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0], FileTag::new("Taxes", None));
        assert_eq!(tags[1], FileTag::new("Receipts", None));
        assert_eq!(tags[2], FileTag::new("Blue", Some(TagColor::Blue)));
    }

    #[test]
    fn test_to_xdg_and_macos_string() {
        let tags = vec![
            FileTag::new("Work", Some(TagColor::Red)),
            FileTag::new("Important", None),
        ];

        let xdg = FileTag::to_xdg_string(&tags);
        assert_eq!(xdg, "Work\nImportant");

        let macos = FileTag::to_macos_string(&tags);
        assert_eq!(macos, "Work\n6\nImportant");
    }

    #[test]
    fn test_file_tag_serde() {
        let tag = FileTag::new("Finance", Some(TagColor::Yellow));
        let json = serde_json::to_string(&tag).unwrap();
        let deserialized: FileTag = serde_json::from_str(&json).unwrap();
        assert_eq!(tag, deserialized);
    }
}
