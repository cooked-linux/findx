//! Desktop application data models and `.desktop` file parser.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::DesktopError;

/// Metadata model representing an installed XDG desktop application.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DesktopApp {
    /// Desktop entry ID (e.g. `org.gnome.Calculator.desktop`).
    pub desktop_id: String,
    /// Absolute path to the `.desktop` file.
    pub path: PathBuf,
    /// Localized or default application name.
    pub name: String,
    /// Generic application name (e.g. "Calculator").
    pub generic_name: Option<String>,
    /// Tooltip or description.
    pub comment: Option<String>,
    /// Original `Exec` field with all arguments and field codes intact.
    pub exec_raw: String,
    /// Executable path/command with Freedesktop field codes stripped.
    pub exec_sanitized: String,
    /// Icon name or absolute path specified in the entry.
    pub icon_name: Option<String>,
    /// Normalized search keywords.
    pub keywords: Vec<String>,
    /// Categories associated with the application.
    pub categories: Vec<String>,
    /// Supported MIME types.
    pub mime_types: Vec<String>,
    /// Whether the application must run in a terminal window.
    pub is_terminal: bool,
    /// Whether the application should be hidden from application menus.
    pub no_display: bool,
    /// Equivalent to deleted; entry should be completely ignored.
    pub hidden: bool,
    /// List of desktop environments this entry should exclusively show in.
    pub only_show_in: Vec<String>,
    /// List of desktop environments this entry should never show in.
    pub not_show_in: Vec<String>,
}

impl DesktopApp {
    /// Parses a `.desktop` file from disk using system locale.
    ///
    /// # Errors
    ///
    /// Returns [`DesktopError::Io`] if file cannot be read, or [`DesktopError::InvalidFormat`]
    /// if the file is not a valid desktop entry.
    pub fn from_file(path: &Path, desktop_id: &str) -> Result<Self, DesktopError> {
        let content = std::fs::read_to_string(path)?;
        let locale = get_system_locale();
        Self::parse_content(&content, path, desktop_id, locale.as_deref())
    }

    /// Parses a desktop entry from string content and specified desktop ID.
    ///
    /// # Errors
    ///
    /// Returns [`DesktopError::InvalidFormat`] if required fields (`Name`, `Exec`) are missing
    /// or if the file contains no `[Desktop Entry]` section.
    pub fn from_content(
        content: &str,
        path: impl Into<PathBuf>,
        desktop_id: impl Into<String>,
    ) -> Result<Self, DesktopError> {
        let path = path.into();
        let desktop_id = desktop_id.into();
        let locale = get_system_locale();
        Self::parse_content(content, &path, &desktop_id, locale.as_deref())
    }

    /// Parses a desktop entry with an explicit locale for localized field lookup.
    ///
    /// # Errors
    ///
    /// Returns [`DesktopError::InvalidFormat`] if required fields (`Name`, `Exec`) are missing
    /// or if the file contains no `[Desktop Entry]` section.
    pub fn parse_content(
        content: &str,
        path: &Path,
        desktop_id: &str,
        locale: Option<&str>,
    ) -> Result<Self, DesktopError> {
        let entries = parse_desktop_entry_group(content, path)?;

        // If Type is specified and not "Application", filter out or error
        if let Some(entry_type) = entries.get("Type")
            && !entry_type.eq_ignore_ascii_case("Application")
        {
            return Err(DesktopError::Filtered);
        }

        let name = get_localized_value(&entries, "Name", locale).ok_or_else(|| {
            DesktopError::InvalidFormat(path.to_path_buf(), "Missing required 'Name' field".into())
        })?;

        let exec_raw = entries.get("Exec").cloned().ok_or_else(|| {
            DesktopError::InvalidFormat(path.to_path_buf(), "Missing required 'Exec' field".into())
        })?;

        let exec_sanitized = sanitize_exec(&exec_raw);

        let generic_name = get_localized_value(&entries, "GenericName", locale);
        let comment = get_localized_value(&entries, "Comment", locale);
        let icon_name = entries.get("Icon").cloned();

        let keywords = get_localized_value(&entries, "Keywords", locale)
            .or_else(|| entries.get("Keywords").cloned())
            .map(|kw| split_semicolon_list(&kw))
            .unwrap_or_default();

        let categories = entries
            .get("Categories")
            .map(|c| split_semicolon_list(c))
            .unwrap_or_default();

        let mime_types = entries
            .get("MimeType")
            .map(|m| split_semicolon_list(m))
            .unwrap_or_default();

        let is_terminal = parse_bool(entries.get("Terminal"));
        let no_display = parse_bool(entries.get("NoDisplay"));
        let hidden = parse_bool(entries.get("Hidden"));

        let only_show_in = entries
            .get("OnlyShowIn")
            .map(|s| split_semicolon_list(s))
            .unwrap_or_default();

        let not_show_in = entries
            .get("NotShowIn")
            .map(|s| split_semicolon_list(s))
            .unwrap_or_default();

        Ok(Self {
            desktop_id: desktop_id.to_string(),
            path: path.to_path_buf(),
            name,
            generic_name,
            comment,
            exec_raw,
            exec_sanitized,
            icon_name,
            keywords,
            categories,
            mime_types,
            is_terminal,
            no_display,
            hidden,
            only_show_in,
            not_show_in,
        })
    }
}

/// Retrieves the current system locale from environment variables (`LC_ALL`, `LC_MESSAGES`, `LANG`).
#[must_use]
pub fn get_system_locale() -> Option<String> {
    std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .ok()
}

/// Splits a semicolon-delimited list into clean, trimmed string tokens.
#[must_use]
pub fn split_semicolon_list(value: &str) -> Vec<String> {
    value
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

/// Strips Freedesktop field codes (`%f`, `%F`, `%u`, `%U`, `%d`, `%D`, `%n`, `%N`, `%i`, `%c`, `%k`, `%v`, `%m`)
/// and Flatpak parameter delimiters (`@@`, `@@u`) from `Exec` command strings.
#[must_use]
pub fn sanitize_exec(exec: &str) -> String {
    let mut tokens = Vec::new();
    let mut chars = exec.chars().peekable();
    let mut current = String::new();
    let mut in_single_quotes = false;
    let mut in_double_quotes = false;

    while let Some(ch) = chars.next() {
        match ch {
            '\'' if !in_double_quotes => {
                in_single_quotes = !in_single_quotes;
            }
            '"' if !in_single_quotes => {
                in_double_quotes = !in_double_quotes;
            }
            ' ' | '\t' if !in_single_quotes && !in_double_quotes => {
                if !current.is_empty() {
                    tokens.push(current);
                    current = String::new();
                }
            }
            '\\' if in_double_quotes => {
                if let Some(&next) = chars.peek() {
                    if next == '"' || next == '\\' || next == '`' || next == '$' {
                        chars.next();
                        current.push(next);
                    } else {
                        current.push('\\');
                    }
                } else {
                    current.push('\\');
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    // Filter out field codes and Flatpak forwarding delimiters
    let mut sanitized_tokens = Vec::new();
    for token in tokens {
        if is_field_code(&token) || token == "@@" || token == "@@u" {
            continue;
        }
        // Handle options like --file=%f or --url=%u
        if let Some((_prefix, code)) = token.split_once('=')
            && is_field_code(code)
        {
            // If the entire value was a field code and prefix is option-like, skip option
            continue;
        }
        sanitized_tokens.push(token);
    }

    sanitized_tokens.join(" ")
}

fn is_field_code(token: &str) -> bool {
    let unquoted = token.trim_matches(['"', '\'']);
    matches!(
        unquoted,
        "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%i" | "%c" | "%k" | "%v" | "%m"
    )
}

fn parse_bool(val: Option<&String>) -> bool {
    val.is_some_and(|s| {
        let trimmed = s.trim();
        trimmed.eq_ignore_ascii_case("true") || trimmed == "1"
    })
}

/// Parses the `[Desktop Entry]` group from a `.desktop` file into key-value pairs.
fn parse_desktop_entry_group(
    content: &str,
    path: &Path,
) -> Result<HashMap<String, String>, DesktopError> {
    let mut in_desktop_entry = false;
    let mut map = HashMap::new();
    let mut found_header = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let section = &trimmed[1..trimmed.len() - 1];
            if section == "Desktop Entry" {
                in_desktop_entry = true;
                found_header = true;
            } else {
                in_desktop_entry = false;
            }
            continue;
        }

        if in_desktop_entry && let Some((key, val)) = line.split_once('=') {
            let key = key.trim();
            let val = unescape_desktop_value(val.trim());
            map.insert(key.to_string(), val);
        }
    }

    if !found_header {
        return Err(DesktopError::InvalidFormat(
            path.to_path_buf(),
            "Missing [Desktop Entry] group header".into(),
        ));
    }

    Ok(map)
}

/// Unescapes standard Desktop Entry escape sequences (`\s`, `\n`, `\t`, `\r`, `\\`, `\;`).
fn unescape_desktop_value(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len());
    let mut chars = raw.chars();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                match next {
                    's' => result.push(' '),
                    'n' => result.push('\n'),
                    't' => result.push('\t'),
                    'r' => result.push('\r'),
                    '\\' => result.push('\\'),
                    ';' => result.push(';'),
                    other => {
                        result.push('\\');
                        result.push(other);
                    }
                }
            } else {
                result.push('\\');
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Finds the best matching localized value for a key based on the locale.
///
/// Lookup order following Freedesktop spec:
/// 1. `Key[lang_COUNTRY@MODIFIER]`
/// 2. `Key[lang_COUNTRY]`
/// 3. `Key[lang@MODIFIER]`
/// 4. `Key[lang]`
/// 5. `Key` (default)
fn get_localized_value(
    entries: &HashMap<String, String>,
    key_name: &str,
    locale: Option<&str>,
) -> Option<String> {
    if let Some(loc) = locale {
        let (lang, country, modifier) = parse_locale_string(loc);

        if let (Some(l), Some(c), Some(m)) = (lang, country, modifier) {
            let k = format!("{key_name}[{l}_{c}@{m}]");
            if let Some(val) = entries.get(&k) {
                return Some(val.clone());
            }
        }

        if let (Some(l), Some(c)) = (lang, country) {
            let k = format!("{key_name}[{l}_{c}]");
            if let Some(val) = entries.get(&k) {
                return Some(val.clone());
            }
        }

        if let (Some(l), Some(m)) = (lang, modifier) {
            let k = format!("{key_name}[{l}@{m}]");
            if let Some(val) = entries.get(&k) {
                return Some(val.clone());
            }
        }

        if let Some(l) = lang {
            let k = format!("{key_name}[{l}]");
            if let Some(val) = entries.get(&k) {
                return Some(val.clone());
            }
        }
    }

    entries.get(key_name).cloned()
}

/// Parses a locale string (e.g., `es_ES.UTF-8@euro`) into `(lang, country, modifier)`.
fn parse_locale_string(locale: &str) -> (Option<&str>, Option<&str>, Option<&str>) {
    // 1. Extract modifier after '@'
    let (base_and_enc, modifier) = match locale.split_once('@') {
        Some((b, m)) => (b, Some(m)),
        None => (locale, None),
    };

    // 2. Extract base before encoding '.'
    let base = match base_and_enc.split_once('.') {
        Some((b, _enc)) => b,
        None => base_and_enc,
    };

    // 3. Extract language and country before/after '_'
    let (lang, country) = match base.split_once('_') {
        Some((l, c)) => (Some(l), Some(c)),
        None => {
            if base.is_empty() {
                (None, None)
            } else {
                (Some(base), None)
            }
        }
    };

    (lang, country, modifier)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_basic_desktop_entry() {
        let content = r#"
[Desktop Entry]
Type=Application
Name=Calculator
GenericName=Math Tool
Comment=Perform calculations
Exec=gnome-calculator %u
Icon=org.gnome.Calculator
Keywords=calc;math;arithmetic;
Categories=GNOME;GTK;Utility;Calculator;
MimeType=text/plain;
Terminal=false
NoDisplay=false
Hidden=false
"#;
        let app = DesktopApp::from_content(
            content,
            PathBuf::from("/usr/share/applications/calculator.desktop"),
            "calculator.desktop",
        )
        .expect("Valid entry");

        assert_eq!(app.desktop_id, "calculator.desktop");
        assert_eq!(app.name, "Calculator");
        assert_eq!(app.generic_name.as_deref(), Some("Math Tool"));
        assert_eq!(app.comment.as_deref(), Some("Perform calculations"));
        assert_eq!(app.exec_raw, "gnome-calculator %u");
        assert_eq!(app.exec_sanitized, "gnome-calculator");
        assert_eq!(app.icon_name.as_deref(), Some("org.gnome.Calculator"));
        assert_eq!(app.keywords, vec!["calc", "math", "arithmetic"]);
        assert_eq!(
            app.categories,
            vec!["GNOME", "GTK", "Utility", "Calculator"]
        );
        assert_eq!(app.mime_types, vec!["text/plain"]);
        assert!(!app.is_terminal);
        assert!(!app.no_display);
        assert!(!app.hidden);
    }

    #[test]
    fn test_localized_name_lookup() {
        let content = r#"
[Desktop Entry]
Type=Application
Name=Calculator
Name[es]=Calculadora
Name[fr]=Calculatrice
Exec=calculator
"#;
        let app_es = DesktopApp::parse_content(
            content,
            &PathBuf::from("/test.desktop"),
            "test.desktop",
            Some("es_ES.UTF-8"),
        )
        .expect("valid");
        assert_eq!(app_es.name, "Calculadora");

        let app_fr = DesktopApp::parse_content(
            content,
            &PathBuf::from("/test.desktop"),
            "test.desktop",
            Some("fr_FR"),
        )
        .expect("valid");
        assert_eq!(app_fr.name, "Calculatrice");

        let app_en = DesktopApp::parse_content(
            content,
            &PathBuf::from("/test.desktop"),
            "test.desktop",
            Some("en_US"),
        )
        .expect("valid");
        assert_eq!(app_en.name, "Calculator");
    }

    #[test]
    fn test_sanitize_exec_field_codes() {
        assert_eq!(
            sanitize_exec("gedit %U --encoding=UTF-8 %F"),
            "gedit --encoding=UTF-8"
        );
        assert_eq!(
            sanitize_exec("vlc --started-from-file %U"),
            "vlc --started-from-file"
        );
        assert_eq!(
            sanitize_exec("/usr/bin/flatpak run org.test.App @@ %u @@"),
            "/usr/bin/flatpak run org.test.App"
        );
        assert_eq!(sanitize_exec("app \"%f\""), "app");
        assert_eq!(sanitize_exec("myapp --file=%f"), "myapp");
    }

    #[test]
    fn test_missing_header_fails() {
        let content = "Name=Foo\nExec=bar\n";
        let res = DesktopApp::from_content(content, PathBuf::from("/test"), "test");
        assert!(res.is_err());
    }

    #[test]
    fn test_actions_group_ignored() {
        let content = r#"
[Desktop Entry]
Type=Application
Name=Main
Exec=main_exec

[Desktop Action New]
Name=Action
Exec=action_exec
"#;
        let app = DesktopApp::from_content(content, PathBuf::from("/test"), "test")
            .expect("should parse");
        assert_eq!(app.name, "Main");
        assert_eq!(app.exec_sanitized, "main_exec");
    }
}
