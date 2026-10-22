//! Criteria compiler translating boolean smart folder ASTs into in-memory predicates
//! and sanitized SQLite FTS5 query strings.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use shared::models::{FileItem, MimeCategory};

use super::types::{
    BooleanMode, CriteriaAttribute, CriteriaGroup, CriteriaOperator, CriteriaSlice,
    SmartFolderDefinition,
};

/// Result of compiling a [`SmartFolderDefinition`] into an executable query.
pub struct CompiledQuery {
    /// Sanitized SQLite FTS5 query string for indexed text matching.
    pub fts5_query: Option<String>,
    /// In-memory filter predicate evaluating full [`FileItem`] records.
    pub filter_fn: Box<dyn Fn(&FileItem) -> bool + Send + Sync>,
    /// List of active, readable scopes (missing or unmounted scopes excluded).
    pub active_scopes: Vec<PathBuf>,
    /// Warnings encountered during compilation (unknown MIME, malformed values).
    pub warnings: Vec<String>,
}

impl std::fmt::Debug for CompiledQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledQuery")
            .field("fts5_query", &self.fts5_query)
            .field("active_scopes", &self.active_scopes)
            .field("warnings", &self.warnings)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
enum KindMatcher {
    Any,
    Category(MimeCategory),
    Mime(&'static str),
    Prefixes(&'static [&'static str]),
}

impl KindMatcher {
    fn matches(&self, mime_type: &str) -> bool {
        let normalized = mime_type.trim().to_ascii_lowercase();
        match self {
            Self::Any => true,
            Self::Category(category) => MimeCategory::from_mime_str(&normalized) == *category,
            Self::Mime(expected) => normalized == *expected,
            Self::Prefixes(prefixes) => prefixes.iter().any(|p| normalized.starts_with(p)),
        }
    }
}

const PRESENTATION_PREFIXES: &[&str] = &[
    "application/vnd.openxmlformats-officedocument.presentationml",
    "application/vnd.oasis.opendocument.presentation",
    "application/vnd.ms-powerpoint",
];

const SPREADSHEET_PREFIXES: &[&str] = &[
    "application/vnd.openxmlformats-officedocument.spreadsheetml",
    "application/vnd.oasis.opendocument.spreadsheet",
    "application/vnd.ms-excel",
];

fn resolve_kind_matcher(raw: &str) -> Option<KindMatcher> {
    let normalized = raw.trim().to_ascii_lowercase();
    let matcher = match normalized.as_str() {
        "" | "any" => KindMatcher::Any,
        "pdf" => KindMatcher::Mime("application/pdf"),
        "documents" | "document" => KindMatcher::Category(MimeCategory::Document),
        "images" | "image" | "pictures" => KindMatcher::Category(MimeCategory::Image),
        "audio" | "music" => KindMatcher::Category(MimeCategory::Audio),
        "video" | "videos" | "movies" => KindMatcher::Category(MimeCategory::Video),
        "source code" | "sourcecode" | "source" | "code" | "developer" => {
            KindMatcher::Category(MimeCategory::SourceCode)
        }
        "archives" | "archive" | "compressed" => KindMatcher::Category(MimeCategory::Archive),
        "presentations" | "presentation" => KindMatcher::Prefixes(PRESENTATION_PREFIXES),
        "spreadsheets" | "spreadsheet" => KindMatcher::Prefixes(SPREADSHEET_PREFIXES),
        "folders" | "folder" | "directories" => KindMatcher::Mime("inode/directory"),
        "applications" | "application" | "executables" | "executable" => {
            KindMatcher::Category(MimeCategory::Executable)
        }
        "fonts" | "font" => KindMatcher::Category(MimeCategory::Font),
        _ => return None,
    };
    Some(matcher)
}

#[derive(Debug, Clone)]
enum Matcher {
    Kind(KindMatcher),
    Name {
        operator: CriteriaOperator,
        needle: String,
    },
    Date {
        attribute: CriteriaAttribute,
        days: u32,
    },
    Tag {
        operator: CriteriaOperator,
        needle: String,
    },
    Size {
        operator: CriteriaOperator,
        bytes: i64,
    },
}

#[derive(Debug, Clone)]
struct CompiledSlice {
    matcher: Matcher,
}

impl CompiledSlice {
    fn matches(&self, item: &FileItem) -> bool {
        match &self.matcher {
            Matcher::Kind(kind) => kind.matches(&item.metadata.mime_type),
            Matcher::Name { operator, needle } => {
                let candidate = item.file_name.to_lowercase();
                match operator {
                    CriteriaOperator::Equals => candidate == *needle,
                    CriteriaOperator::StartsWith => candidate.starts_with(needle),
                    CriteriaOperator::EndsWith => candidate.ends_with(needle),
                    _ => candidate.contains(needle),
                }
            }
            Matcher::Date { attribute, days } => {
                let timestamp = match attribute {
                    CriteriaAttribute::DateCreated => item.metadata.created,
                    _ => item.metadata.modified,
                };
                timestamp.is_some_and(|time| within_days(time, *days))
            }
            Matcher::Tag { operator, needle } => item.tags.iter().any(|tag| {
                let name = tag.name.to_lowercase();
                let color = tag.color.map(|c| c.display_name().to_lowercase());
                let name_hit = match operator {
                    CriteriaOperator::Equals => name == *needle,
                    _ => name.contains(needle),
                };
                let color_hit = color.is_some_and(|c| {
                    c == *needle
                        || (matches!(operator, CriteriaOperator::Contains) && c.contains(needle))
                });
                name_hit || color_hit
            }),
            Matcher::Size { operator, bytes } => {
                let size = i64::try_from(item.metadata.size_bytes).unwrap_or(i64::MAX);
                match operator {
                    CriteriaOperator::GreaterThan => size > *bytes,
                    CriteriaOperator::LessThan => size < *bytes,
                    _ => size == *bytes,
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
enum CompiledNode {
    Constant(bool),
    Slice(CompiledSlice),
    Group {
        mode: BooleanMode,
        children: Vec<Self>,
    },
}

impl CompiledNode {
    fn matches(&self, item: &FileItem) -> bool {
        match self {
            Self::Constant(value) => *value,
            Self::Slice(slice) => slice.matches(item),
            Self::Group { mode, children } => match mode {
                BooleanMode::All => children.iter().all(|child| child.matches(item)),
                BooleanMode::Any => children.iter().any(|child| child.matches(item)),
                BooleanMode::None => !children.iter().any(|child| child.matches(item)),
            },
        }
    }
}

fn within_days(timestamp: SystemTime, days: u32) -> bool {
    let window = Duration::from_secs(u64::from(days).saturating_mul(86_400));
    let now = SystemTime::now();
    let cutoff = now.checked_sub(window).unwrap_or(UNIX_EPOCH);
    timestamp >= cutoff
}

/// Validates a single criteria slice, returning a human-readable reason on failure.
///
/// # Errors
///
/// Returns an explanation when the attribute, operator, or value combination is invalid.
pub fn validate_slice(slice: &CriteriaSlice) -> std::result::Result<(), String> {
    match slice.attribute {
        CriteriaAttribute::Kind => {
            let Some(text) = slice.value.as_text() else {
                return Err("kind criteria require a textual MIME category".to_string());
            };
            if resolve_kind_matcher(text).is_none() {
                return Err(format!("unknown kind '{text}'"));
            }
        }
        CriteriaAttribute::Name => {
            let text = slice.value.as_text().unwrap_or_default();
            if text.trim().is_empty() {
                return Err("name criteria require a non-empty pattern".to_string());
            }
            if !matches!(
                slice.operator,
                CriteriaOperator::Equals
                    | CriteriaOperator::Contains
                    | CriteriaOperator::StartsWith
                    | CriteriaOperator::EndsWith
            ) {
                return Err(format!(
                    "operator '{}' is not valid for names",
                    slice.operator.label()
                ));
            }
        }
        CriteriaAttribute::DateModified | CriteriaAttribute::DateCreated => {
            if slice.operator != CriteriaOperator::WithinLastDays {
                return Err("date criteria support 'within last days' only".to_string());
            }
            if slice.value.as_days().is_none() {
                return Err("date criteria require a non-negative day count".to_string());
            }
        }
        CriteriaAttribute::Tag => {
            let text = slice.value.as_text().unwrap_or_default();
            if text.trim().is_empty() {
                return Err("tag criteria require a tag name or color".to_string());
            }
        }
        CriteriaAttribute::Size => {
            if !matches!(
                slice.operator,
                CriteriaOperator::Equals
                    | CriteriaOperator::GreaterThan
                    | CriteriaOperator::LessThan
            ) {
                return Err(format!(
                    "operator '{}' is not valid for sizes",
                    slice.operator.label()
                ));
            }
            if slice.value.as_i64().is_none_or(|value| value < 0) {
                return Err("size criteria require a non-negative byte count".to_string());
            }
        }
    }
    Ok(())
}

fn compile_slice(slice: &CriteriaSlice) -> std::result::Result<CompiledSlice, String> {
    validate_slice(slice)?;
    let matcher = match slice.attribute {
        CriteriaAttribute::Kind => {
            let text = slice.value.as_text().unwrap_or_default();
            let kind =
                resolve_kind_matcher(text).ok_or_else(|| format!("unknown kind '{text}'"))?;
            Matcher::Kind(kind)
        }
        CriteriaAttribute::Name => Matcher::Name {
            operator: slice.operator,
            needle: slice.value.as_text().unwrap_or_default().to_lowercase(),
        },
        CriteriaAttribute::DateModified | CriteriaAttribute::DateCreated => Matcher::Date {
            attribute: slice.attribute,
            days: slice.value.as_days().unwrap_or(0),
        },
        CriteriaAttribute::Tag => Matcher::Tag {
            operator: slice.operator,
            needle: slice
                .value
                .as_text()
                .unwrap_or_default()
                .trim()
                .to_lowercase(),
        },
        CriteriaAttribute::Size => Matcher::Size {
            operator: slice.operator,
            bytes: slice.value.as_i64().unwrap_or(0),
        },
    };
    Ok(CompiledSlice { matcher })
}

fn compile_group(group: &CriteriaGroup, warnings: &mut Vec<String>) -> Option<CompiledNode> {
    let mut children = Vec::with_capacity(group.slices.len() + group.subgroups.len());

    for slice in &group.slices {
        if !slice.is_valid {
            let reason = slice
                .warning_message
                .clone()
                .unwrap_or_else(|| "criteria slice marked invalid".to_string());
            warnings.push(format!("{}: {reason}", slice.attribute.label()));
            continue;
        }
        match compile_slice(slice) {
            Ok(compiled) => children.push(CompiledNode::Slice(compiled)),
            Err(reason) => {
                warnings.push(format!("{}: {reason}", slice.attribute.label()));
            }
        }
    }

    for subgroup in &group.subgroups {
        if let Some(node) = compile_group(subgroup, warnings) {
            children.push(node);
        }
    }

    if children.is_empty() {
        if group.is_empty() {
            // An explicitly empty group is treated as an unconditional match.
            Some(CompiledNode::Constant(true))
        } else {
            // Every branch was invalid; skip the group rather than swallowing siblings.
            None
        }
    } else {
        Some(CompiledNode::Group {
            mode: group.mode,
            children,
        })
    }
}

fn collect_name_terms(group: &CriteriaGroup, terms: &mut Vec<String>) {
    for slice in &group.slices {
        if slice.attribute != CriteriaAttribute::Name || !slice.is_valid {
            continue;
        }
        if matches!(
            slice.operator,
            CriteriaOperator::Equals | CriteriaOperator::Contains | CriteriaOperator::StartsWith
        ) && let Some(text) = slice.value.as_text()
            && !text.trim().is_empty()
        {
            terms.push(text.trim().to_string());
        }
    }
    for subgroup in &group.subgroups {
        collect_name_terms(subgroup, terms);
    }
}

fn build_fts_query(group: &CriteriaGroup) -> Option<String> {
    let mut terms = Vec::new();
    collect_name_terms(group, &mut terms);
    if terms.is_empty() {
        return None;
    }
    let raw = terms.join(" ");
    let sanitized = shared::indexer::query::sanitize_fts5_query(&raw);
    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized)
    }
}

fn is_readable_scope(path: &Path) -> bool {
    path.is_dir() && std::fs::read_dir(path).is_ok()
}

/// Resolves configured scopes into readable directories, defaulting to `$HOME`.
#[must_use]
pub fn resolve_active_scopes(scopes: &[PathBuf]) -> Vec<PathBuf> {
    if scopes.is_empty() {
        let home = std::env::var("HOME").map_or_else(|_| PathBuf::from("/"), PathBuf::from);
        return vec![home];
    }
    scopes
        .iter()
        .filter(|path| is_readable_scope(path))
        .cloned()
        .collect()
}

/// Compiles a smart folder definition into an executable query.
///
/// Invalid criteria slices are skipped and reported as warnings, while valid branches
/// continue to participate in evaluation.
#[must_use]
pub fn compile_definition(definition: &SmartFolderDefinition) -> CompiledQuery {
    let mut warnings = Vec::new();
    let root = compile_group(&definition.root_criteria, &mut warnings)
        .unwrap_or(CompiledNode::Constant(true));

    CompiledQuery {
        fts5_query: build_fts_query(&definition.root_criteria),
        filter_fn: Box::new(move |item: &FileItem| root.matches(item)),
        active_scopes: resolve_active_scopes(&definition.scopes),
        warnings,
    }
}

/// Builder helper exposing the resolved kind matcher name for UI validation.
#[must_use]
pub fn is_known_kind(raw: &str) -> bool {
    resolve_kind_matcher(raw).is_some()
}

/// Converts a criteria value into a byte count using the supplied unit multiplier.
#[must_use]
pub const fn size_value_to_bytes(value: i64, multiplier: i64) -> i64 {
    value.saturating_mul(multiplier)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty
)]
mod tests {
    use std::path::PathBuf;
    use std::time::SystemTime;

    use shared::models::{FileMetadata, FileTag, TagColor};

    use super::*;
    use crate::smart_folders::types::{CriteriaGroup, CriteriaSlice};

    fn make_item(name: &str, mime: &str, size: u64, days_ago: u64, tags: &[&str]) -> FileItem {
        let modified = SystemTime::now() - Duration::from_secs(days_ago * 86_400);
        FileItem::new(
            PathBuf::from(format!("/tmp/{name}")),
            name,
            false,
            false,
            None,
            FileMetadata {
                size_bytes: size,
                modified: Some(modified),
                created: Some(modified),
                mime_type: mime.to_string(),
                ..FileMetadata::default()
            },
            tags.iter()
                .map(|tag| FileTag::new(*tag, Some(TagColor::Blue)))
                .collect(),
        )
    }

    fn definition(group: CriteriaGroup) -> SmartFolderDefinition {
        let mut def = SmartFolderDefinition::new("Test", group);
        def.scopes = vec![PathBuf::from("/tmp")];
        def
    }

    #[test]
    fn test_kind_and_recent_date_compilation() {
        let def = definition(CriteriaGroup::all(vec![
            CriteriaSlice::new(CriteriaAttribute::Kind, CriteriaOperator::Equals, "PDF"),
            CriteriaSlice::new(
                CriteriaAttribute::DateModified,
                CriteriaOperator::WithinLastDays,
                7_u32,
            ),
        ]));
        let compiled = compile_definition(&def);
        assert!(compiled.warnings.is_empty());
        assert!(
            compiled
                .active_scopes
                .iter()
                .any(|p| p == Path::new("/tmp"))
        );

        let pdf = make_item("report.pdf", "application/pdf", 10, 2, &[]);
        let old_pdf = make_item("old.pdf", "application/pdf", 10, 30, &[]);
        let txt = make_item("notes.txt", "text/plain", 10, 1, &[]);

        assert!((compiled.filter_fn)(&pdf));
        assert!(!(compiled.filter_fn)(&old_pdf));
        assert!(!(compiled.filter_fn)(&txt));
    }

    #[test]
    fn test_any_name_or_tag_compilation() {
        let def = definition(CriteriaGroup::any(vec![
            CriteriaSlice::new(
                CriteriaAttribute::Name,
                CriteriaOperator::Contains,
                "report",
            ),
            CriteriaSlice::new(CriteriaAttribute::Tag, CriteriaOperator::Equals, "Work"),
        ]));
        let compiled = compile_definition(&def);
        assert!(compiled.warnings.is_empty());
        assert_eq!(compiled.fts5_query.as_deref(), Some("report*"));

        assert!((compiled.filter_fn)(&make_item(
            "report.pdf",
            "application/pdf",
            1,
            0,
            &[]
        )));
        assert!((compiled.filter_fn)(&make_item(
            "invoice.pdf",
            "application/pdf",
            1,
            0,
            &["Work"]
        )));
        assert!(!(compiled.filter_fn)(&make_item(
            "notes.txt",
            "text/plain",
            1,
            0,
            &[]
        )));
    }

    #[test]
    fn test_none_size_compilation() {
        let def = definition(CriteriaGroup {
            mode: BooleanMode::None,
            slices: vec![CriteriaSlice::new(
                CriteriaAttribute::Size,
                CriteriaOperator::GreaterThan,
                100_i64 * 1024 * 1024,
            )],
            subgroups: Vec::new(),
        });
        let compiled = compile_definition(&def);

        assert!((compiled.filter_fn)(&make_item(
            "small.txt",
            "text/plain",
            1024,
            0,
            &[]
        )));
        assert!(!(compiled.filter_fn)(&make_item(
            "huge.bin",
            "application/octet-stream",
            200 * 1024 * 1024,
            0,
            &[]
        )));
    }

    #[test]
    fn test_invalid_kind_is_skipped_with_warning() {
        let def = definition(CriteriaGroup::all(vec![
            CriteriaSlice::new(
                CriteriaAttribute::Kind,
                CriteriaOperator::Equals,
                "NotAKind",
            ),
            CriteriaSlice::new(
                CriteriaAttribute::Name,
                CriteriaOperator::Equals,
                "keep.txt",
            ),
        ]));
        let compiled = compile_definition(&def);
        assert_eq!(compiled.warnings.len(), 1);
        assert!(compiled.warnings[0].contains("NotAKind"));
        assert!((compiled.filter_fn)(&make_item(
            "keep.txt",
            "text/plain",
            1,
            0,
            &[]
        )));
        assert!(!(compiled.filter_fn)(&make_item(
            "drop.txt",
            "text/plain",
            1,
            0,
            &[]
        )));
    }

    #[test]
    fn test_missing_scope_is_excluded() {
        let mut def = definition(CriteriaGroup::match_all());
        def.scopes = vec![
            PathBuf::from("/tmp"),
            PathBuf::from("/definitely/not/a/real/path/findx"),
        ];
        let compiled = compile_definition(&def);
        assert_eq!(compiled.active_scopes, vec![PathBuf::from("/tmp")]);
    }
}
