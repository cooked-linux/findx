//! Criteria AST, domain models, and persistence schema for smart folders.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Operator applied to criteria evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriteriaOperator {
    /// Exact value equality.
    Equals,
    /// Substring containment.
    Contains,
    /// Prefix match.
    StartsWith,
    /// Suffix match.
    EndsWith,
    /// Numeric or temporal greater-than comparison.
    GreaterThan,
    /// Numeric or temporal less-than comparison.
    LessThan,
    /// Rolling date window expressed in days.
    WithinLastDays,
}

impl CriteriaOperator {
    /// Returns the user-facing label rendered in criteria operator drop-downs.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Equals => "is",
            Self::Contains => "contains",
            Self::StartsWith => "starts with",
            Self::EndsWith => "ends with",
            Self::GreaterThan => "greater than",
            Self::LessThan => "less than",
            Self::WithinLastDays => "within last days",
        }
    }
}

/// Evaluated file attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriteriaAttribute {
    /// MIME category or concrete MIME type.
    Kind,
    /// File or directory name.
    Name,
    /// Last modification timestamp.
    DateModified,
    /// Creation timestamp.
    DateCreated,
    /// User tag name or color label.
    Tag,
    /// File size in bytes.
    Size,
}

impl CriteriaAttribute {
    /// Returns the user-facing label rendered in criteria attribute drop-downs.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Kind => "Kind",
            Self::Name => "Name",
            Self::DateModified => "Last Modified",
            Self::DateCreated => "Date Created",
            Self::Tag => "Tags",
            Self::Size => "File Size",
        }
    }
}

/// Value representation across different attribute data types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CriteriaValue {
    /// Free-form text value.
    Text(String),
    /// Signed numeric value, such as byte counts.
    Numeric(i64),
    /// Non-negative rolling day count.
    Days(u32),
}

impl CriteriaValue {
    /// Returns the textual representation, if this is a text value.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Numeric(_) | Self::Days(_) => None,
        }
    }

    /// Returns the numeric representation, accepting both numeric and day variants.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Text(_) => None,
            Self::Numeric(value) => Some(*value),
            Self::Days(days) => Some(i64::from(*days)),
        }
    }

    /// Returns a non-negative day count, accepting numeric variants when in range.
    #[must_use]
    pub fn as_days(&self) -> Option<u32> {
        match self {
            Self::Text(_) => None,
            Self::Numeric(value) => u32::try_from(*value).ok(),
            Self::Days(days) => Some(*days),
        }
    }
}

impl From<String> for CriteriaValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for CriteriaValue {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

impl From<i64> for CriteriaValue {
    fn from(value: i64) -> Self {
        Self::Numeric(value)
    }
}

impl From<u32> for CriteriaValue {
    fn from(value: u32) -> Self {
        Self::Days(value)
    }
}

/// Single rule slice in the criteria builder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CriteriaSlice {
    /// Attribute being evaluated.
    pub attribute: CriteriaAttribute,
    /// Comparison operator applied to the attribute.
    pub operator: CriteriaOperator,
    /// Comparison value.
    pub value: CriteriaValue,
    /// Whether the slice passed validation and participates in evaluation.
    #[serde(default = "default_true")]
    pub is_valid: bool,
    /// Validation warning rendered as a badge in the criteria builder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning_message: Option<String>,
}

const fn default_true() -> bool {
    true
}

impl CriteriaSlice {
    /// Constructs a valid slice with no warning attached.
    #[must_use]
    pub fn new(
        attribute: CriteriaAttribute,
        operator: CriteriaOperator,
        value: impl Into<CriteriaValue>,
    ) -> Self {
        Self {
            attribute,
            operator,
            value: value.into(),
            is_valid: true,
            warning_message: None,
        }
    }

    /// Marks this slice as invalid with an explanatory warning message.
    #[must_use]
    pub fn invalidated(mut self, message: impl Into<String>) -> Self {
        self.is_valid = false;
        self.warning_message = Some(message.into());
        self
    }
}

/// Boolean combinator mode for compound rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BooleanMode {
    /// All rules must match (logical AND).
    All,
    /// Any rule may match (logical OR).
    Any,
    /// No rule may match (logical NOT / NAND).
    None,
}

impl BooleanMode {
    /// Returns the user-facing label rendered in boolean mode selectors.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Any => "any",
            Self::None => "none",
        }
    }
}

/// Compound group of criteria slices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::module_name_repetitions)]
pub struct CriteriaGroup {
    /// Boolean combinator applied across direct slices and subgroups.
    pub mode: BooleanMode,
    /// Leaf criteria slices in this group.
    #[serde(default)]
    pub slices: Vec<CriteriaSlice>,
    /// Nested compound groups.
    #[serde(default)]
    pub subgroups: Vec<Self>,
}

impl CriteriaGroup {
    /// Constructs an empty criteria group with the supplied boolean mode.
    #[must_use]
    pub const fn new(mode: BooleanMode) -> Self {
        Self {
            mode,
            slices: Vec::new(),
            subgroups: Vec::new(),
        }
    }

    /// Constructs a group matching all supplied slices (logical AND).
    #[must_use]
    pub const fn all(slices: Vec<CriteriaSlice>) -> Self {
        Self {
            mode: BooleanMode::All,
            slices,
            subgroups: Vec::new(),
        }
    }

    /// Constructs a group matching any supplied slice (logical OR).
    #[must_use]
    pub const fn any(slices: Vec<CriteriaSlice>) -> Self {
        Self {
            mode: BooleanMode::Any,
            slices,
            subgroups: Vec::new(),
        }
    }

    /// Constructs an empty group matching everything, suitable for new smart folders.
    #[must_use]
    pub fn match_all() -> Self {
        Self::all(vec![CriteriaSlice::new(
            CriteriaAttribute::Kind,
            CriteriaOperator::Equals,
            "Any",
        )])
    }

    /// Returns `true` if this group has no slices and no nested subgroups.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.slices.is_empty() && self.subgroups.is_empty()
    }
}

/// Persisted structure of a `.savedSearch` virtual directory file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SmartFolderDefinition {
    /// Persistence schema version.
    pub version: String,
    /// User-visible smart folder name.
    pub name: String,
    /// Filesystem roots evaluated by the query.
    #[serde(default)]
    pub scopes: Vec<PathBuf>,
    /// Whether content matching is restricted to file names.
    #[serde(default)]
    pub name_only: bool,
    /// Root boolean criteria tree.
    pub root_criteria: CriteriaGroup,
    /// Optional symbolic icon name rendered in the sidebar and header.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_name: Option<String>,
    /// Creation timestamp in epoch seconds.
    pub created_at: i64,
    /// Last modification timestamp in epoch seconds.
    pub modified_at: i64,
}

impl SmartFolderDefinition {
    /// Schema version supported by this build.
    pub const CURRENT_VERSION: &'static str = "1.0";

    /// Constructs a new smart folder definition with current timestamps.
    #[must_use]
    pub fn new(name: impl Into<String>, root_criteria: CriteriaGroup) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            version: Self::CURRENT_VERSION.to_string(),
            name: name.into(),
            scopes: Vec::new(),
            name_only: false,
            root_criteria,
            icon_name: Some("folder-saved-search-symbolic".to_string()),
            created_at: now,
            modified_at: now,
        }
    }

    /// Returns the themed icon name for this smart folder.
    #[must_use]
    pub fn icon_name(&self) -> &str {
        self.icon_name
            .as_deref()
            .unwrap_or("folder-saved-search-symbolic")
    }

    /// Updates the modification timestamp to the current wall clock time.
    pub fn touch(&mut self) {
        self.modified_at = chrono::Utc::now().timestamp();
    }
}
