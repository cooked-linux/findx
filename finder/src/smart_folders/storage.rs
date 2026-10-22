//! `.savedSearch` persistence layer with XDG-compliant storage and atomic writes.

use std::fs;
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

use super::error::{Result, SmartFolderError};
use super::types::SmartFolderDefinition;

/// File extension used for persisted smart folder definitions.
pub const SAVED_SEARCH_EXTENSION: &str = "savedSearch";

/// Resolves the XDG-compliant smart folder storage directory.
///
/// Honors `$XDG_DATA_HOME`, defaulting to `$HOME/.local/share/filemanager/saved_searches`.
#[must_use]
pub fn resolve_storage_dir() -> PathBuf {
    if let Ok(data_home) = std::env::var("XDG_DATA_HOME")
        && !data_home.trim().is_empty()
    {
        return PathBuf::from(data_home)
            .join("filemanager")
            .join("saved_searches");
    }
    let home = std::env::var("HOME").map_or_else(|_| PathBuf::from("/"), PathBuf::from);
    home.join(".local")
        .join("share")
        .join("filemanager")
        .join("saved_searches")
}

/// Returns `true` when `path` uses the `.savedSearch` extension.
#[must_use]
pub fn is_saved_search_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(SAVED_SEARCH_EXTENSION))
}

/// Ensures the storage directory exists with owner-only permissions (`0o700`).
///
/// # Errors
///
/// Returns [`SmartFolderError::Io`] if the directory cannot be created.
pub fn ensure_storage_dir(storage_dir: &Path) -> Result<()> {
    if storage_dir.is_dir() {
        return Ok(());
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(storage_dir)
        .map_err(|source| SmartFolderError::io(storage_dir, source))
}

/// Replaces path separators and illegal filename characters with underscores.
#[must_use]
pub fn sanitize_smart_folder_name(name: &str) -> String {
    let sanitized: String = name
        .trim()
        .chars()
        .map(|ch| {
            if ch.is_control() || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
            {
                '_'
            } else {
                ch
            }
        })
        .collect();
    let trimmed = sanitized.trim_matches(['.', ' ']).trim();
    if trimmed.is_empty() {
        "Smart Folder".to_string()
    } else {
        trimmed.to_string()
    }
}

fn validate_definition(definition: &SmartFolderDefinition) -> Result<()> {
    if definition.version == SmartFolderDefinition::CURRENT_VERSION {
        Ok(())
    } else {
        Err(SmartFolderError::InvalidSchemaVersion {
            found: definition.version.clone(),
            expected: SmartFolderDefinition::CURRENT_VERSION.to_string(),
        })
    }
}

fn same_definition(left: &SmartFolderDefinition, right: &SmartFolderDefinition) -> bool {
    left.name == right.name
        && left.scopes == right.scopes
        && left.name_only == right.name_only
        && left.root_criteria == right.root_criteria
        && left.icon_name == right.icon_name
}

fn resolve_target_path(storage_dir: &Path, definition: &SmartFolderDefinition) -> Result<PathBuf> {
    let file_name = format!(
        "{}.{}",
        sanitize_smart_folder_name(&definition.name),
        SAVED_SEARCH_EXTENSION
    );
    let preferred = storage_dir.join(&file_name);

    if !preferred.exists() {
        return Ok(preferred);
    }

    if let Ok(existing) = load_smart_folder(&preferred)
        && same_definition(&existing, definition)
    {
        return Ok(preferred);
    }

    let stem = sanitize_smart_folder_name(&definition.name);
    for suffix in 2..=999_u32 {
        let candidate = storage_dir.join(format!("{stem} {suffix}.{SAVED_SEARCH_EXTENSION}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
        if let Ok(existing) = load_smart_folder(&candidate)
            && same_definition(&existing, definition)
        {
            return Ok(candidate);
        }
    }

    Err(SmartFolderError::InvalidCriteria {
        attribute: "name".to_string(),
        reason: format!("too many saved searches named '{}'", definition.name),
    })
}

/// Persists a smart folder definition atomically as a `.savedSearch` JSON file.
///
/// # Errors
///
/// Returns [`SmartFolderError::InvalidSchemaVersion`] for unsupported schemas,
/// [`SmartFolderError::Serialization`] for JSON failures, or [`SmartFolderError::Io`]
/// when directory creation or atomic write fails.
pub fn save_smart_folder(
    storage_dir: &Path,
    definition: &SmartFolderDefinition,
) -> Result<PathBuf> {
    validate_definition(definition)?;
    ensure_storage_dir(storage_dir)?;

    let target = resolve_target_path(storage_dir, definition)?;
    let serialized = serde_json::to_string_pretty(definition).map_err(|error| {
        SmartFolderError::Serialization {
            path: target.clone(),
            message: error.to_string(),
        }
    })?;

    let Some(file_name) = target.file_name().and_then(|name| name.to_str()) else {
        return Err(SmartFolderError::InvalidCriteria {
            attribute: "name".to_string(),
            reason: "resolved saved search filename is invalid UTF-8".to_string(),
        });
    };

    let temp_path = storage_dir.join(format!(".{file_name}.tmp"));
    let write_result = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(serialized.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp_path, &target)
    })();

    if let Err(source) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(SmartFolderError::io(target, source));
    }

    Ok(target)
}

/// Loads and validates a `.savedSearch` JSON definition.
///
/// # Errors
///
/// Returns [`SmartFolderError::Io`] when the file cannot be read,
/// [`SmartFolderError::Serialization`] for malformed JSON,
/// or [`SmartFolderError::InvalidSchemaVersion`] for unsupported schema versions.
pub fn load_smart_folder(path: &Path) -> Result<SmartFolderDefinition> {
    let content = fs::read_to_string(path).map_err(|source| SmartFolderError::io(path, source))?;
    let definition: SmartFolderDefinition =
        serde_json::from_str(&content).map_err(|error| SmartFolderError::Serialization {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
    validate_definition(&definition)?;
    Ok(definition)
}

/// Enumerates all readable saved searches, skipping corrupt files with a warning.
///
/// # Errors
///
/// Returns [`SmartFolderError::Io`] when the storage directory cannot be read.
pub fn list_saved_searches(storage_dir: &Path) -> Result<Vec<(PathBuf, SmartFolderDefinition)>> {
    if !storage_dir.exists() {
        return Ok(Vec::new());
    }

    let entries =
        fs::read_dir(storage_dir).map_err(|source| SmartFolderError::io(storage_dir, source))?;
    let mut results = Vec::new();

    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if !is_saved_search_path(&path) {
            continue;
        }
        match load_smart_folder(&path) {
            Ok(definition) => results.push((path, definition)),
            Err(error) => {
                tracing::warn!(
                    "Skipping corrupt saved search '{}': {error}",
                    path.display()
                );
            }
        }
    }

    results.sort_by_key(|entry| entry.1.name.to_lowercase());
    Ok(results)
}

/// Deletes a persisted `.savedSearch` file.
///
/// # Errors
///
/// Returns [`SmartFolderError::InvalidCriteria`] when `path` is not a saved search,
/// or [`SmartFolderError::Io`] when removal fails for reasons other than absence.
pub fn delete_saved_search(path: &Path) -> Result<()> {
    if !is_saved_search_path(path) {
        return Err(SmartFolderError::InvalidCriteria {
            attribute: "path".to_string(),
            reason: format!("'{}' is not a .savedSearch file", path.display()),
        });
    }

    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(SmartFolderError::io(path, source)),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::assert_is_empty
)]
mod tests {
    use super::*;
    use crate::smart_folders::types::{
        CriteriaAttribute, CriteriaGroup, CriteriaOperator, CriteriaSlice,
    };

    fn sample_definition(name: &str) -> SmartFolderDefinition {
        SmartFolderDefinition::new(
            name,
            CriteriaGroup::all(vec![CriteriaSlice::new(
                CriteriaAttribute::Kind,
                CriteriaOperator::Equals,
                "PDF",
            )]),
        )
    }

    #[test]
    fn test_atomic_save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let definition = sample_definition("Recent PDFs");
        let path = save_smart_folder(dir.path(), &definition).unwrap();

        assert!(path.ends_with("Recent PDFs.savedSearch"));
        assert!(path.exists());
        assert!(!dir.path().join(".Recent PDFs.savedSearch.tmp").exists());

        let loaded = load_smart_folder(&path).unwrap();
        assert_eq!(loaded, definition);
    }

    #[test]
    fn test_collision_renaming_and_idempotent_resave() {
        let dir = tempfile::tempdir().unwrap();
        let first = sample_definition("Invoices");
        let first_path = save_smart_folder(dir.path(), &first).unwrap();
        assert!(first_path.ends_with("Invoices.savedSearch"));

        let mut second = first.clone();
        second.root_criteria = CriteriaGroup::any(vec![CriteriaSlice::new(
            CriteriaAttribute::Name,
            CriteriaOperator::Contains,
            "invoice",
        )]);
        let second_path = save_smart_folder(dir.path(), &second).unwrap();
        assert!(second_path.ends_with("Invoices 2.savedSearch"));

        let resaved = save_smart_folder(dir.path(), &first).unwrap();
        assert_eq!(resaved, first_path);
    }

    #[test]
    fn test_corrupt_json_rejected_and_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let corrupt = dir.path().join("Broken.savedSearch");
        fs::write(&corrupt, "{not valid json").unwrap();

        assert!(matches!(
            load_smart_folder(&corrupt),
            Err(SmartFolderError::Serialization { .. })
        ));

        let valid = sample_definition("Valid");
        save_smart_folder(dir.path(), &valid).unwrap();
        let listed = list_saved_searches(dir.path()).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].1.name, "Valid");
    }

    #[test]
    fn test_schema_version_validation() {
        let dir = tempfile::tempdir().unwrap();
        let mut definition = sample_definition("Old");
        definition.version = "0.9".to_string();
        assert!(matches!(
            save_smart_folder(dir.path(), &definition),
            Err(SmartFolderError::InvalidSchemaVersion { .. })
        ));

        let path = dir.path().join("Old.savedSearch");
        definition.version = "0.9".to_string();
        fs::write(&path, serde_json::to_string(&definition).unwrap()).unwrap();
        assert!(matches!(
            load_smart_folder(&path),
            Err(SmartFolderError::InvalidSchemaVersion { .. })
        ));
    }

    #[test]
    fn test_delete_saved_search() {
        let dir = tempfile::tempdir().unwrap();
        let path = save_smart_folder(dir.path(), &sample_definition("Delete Me")).unwrap();
        delete_saved_search(&path).unwrap();
        assert!(!path.exists());

        assert!(matches!(
            delete_saved_search(&dir.path().join("notes.txt")),
            Err(SmartFolderError::InvalidCriteria { .. })
        ));
    }

    #[test]
    fn test_name_sanitization() {
        assert_eq!(sanitize_smart_folder_name("a/b\\c:d"), "a_b_c_d");
        assert_eq!(sanitize_smart_folder_name("   "), "Smart Folder");
        assert!(is_saved_search_path(Path::new("x.savedSearch")));
        assert!(!is_saved_search_path(Path::new("x.json")));
    }
}
