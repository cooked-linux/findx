//! Unique name generation algorithms for conflict resolution ("Keep Both").

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Known compound archive extensions that should not be split at the last dot.
const COMPOUND_EXTENSIONS: &[&str] = &[
    ".tar.gz",
    ".tar.bz2",
    ".tar.xz",
    ".tar.zst",
    ".tar.lz",
    ".tar.lzma",
    ".tar.lzo",
];

/// Splits a filename into its stem and extension component.
///
/// Preserves compound archive extensions (e.g. `".tar.gz"`) as well as leading dots
/// for hidden Unix files (e.g. `".gitignore"`).
#[must_use]
pub fn split_stem_and_extension(filename: &str) -> (&str, &str) {
    let lower = filename.to_lowercase();
    for compound in COMPOUND_EXTENSIONS {
        if lower.ends_with(compound) {
            let split_pos = filename.len().saturating_sub(compound.len());
            return (&filename[..split_pos], &filename[split_pos..]);
        }
    }

    if let Some(dot_idx) = filename.rfind('.')
        && dot_idx > 0
    {
        return (&filename[..dot_idx], &filename[dot_idx..]);
    }

    (filename, "")
}

/// Parses the base name and optional counter from a filename stem.
///
/// Recognizes:
/// - Parenthesized index: `"Document (1)"` -> base: `"Document"`, counter: `1`
/// - macOS copy suffixes: `"Document copy"` -> base: `"Document"`, counter: `1`
/// - macOS numbered copy: `"Document copy 2"` -> base: `"Document"`, counter: `2`
#[must_use]
fn parse_stem_counter(stem: &str) -> (&str, Option<u64>) {
    // Check for `^(.+)\s*\((\d+)\)$`
    if stem.ends_with(')')
        && let Some(open_paren) = stem.rfind('(')
    {
        let digits_slice = &stem[open_paren + 1..stem.len() - 1];
        if !digits_slice.is_empty()
            && digits_slice.chars().all(|c| c.is_ascii_digit())
            && let Ok(counter) = digits_slice.parse::<u64>()
        {
            let base = stem[..open_paren].trim_end();
            if !base.is_empty() {
                return (base, Some(counter));
            }
        }
    }

    // Check for `^(.+)\s+copy(?:\s+(\d+))?$`
    if let Some(copy_idx) = stem.rfind(" copy") {
        let remainder = &stem[copy_idx + 5..];
        let base = stem[..copy_idx].trim_end();
        if !base.is_empty() {
            if remainder.is_empty() {
                return (base, Some(1));
            }
            if let Some(digits_str) = remainder.strip_prefix(' ') {
                let digits_slice = digits_str.trim();
                if !digits_slice.is_empty()
                    && digits_slice.chars().all(|c| c.is_ascii_digit())
                    && let Ok(counter) = digits_slice.parse::<u64>()
                {
                    return (base, Some(counter));
                }
            }
        }
    }

    (stem, None)
}

/// Detects whether the filesystem hosting `dir` is case-insensitive (e.g. FAT, NTFS, CIFS).
#[must_use]
pub fn is_case_insensitive_filesystem(dir: &Path) -> bool {
    let probe_dir = if dir.exists() {
        dir.to_path_buf()
    } else {
        dir.ancestors()
            .find(|p| p.exists())
            .map_or_else(|| PathBuf::from("/"), Path::to_path_buf)
    };

    let Ok(stat) = nix::sys::statfs::statfs(&probe_dir) else {
        return false;
    };

    let f_type = stat.filesystem_type();

    // Check known case-insensitive filesystem types:
    // MSDOS (0x4d44), NTFS (0x5346544e), SMB (0x517b), CIFS (0xff534d42), SMB2 (0xfe534d42)
    #[allow(clippy::cast_sign_loss)]
    let f_type_val = f_type.0 as u64;
    f_type == nix::sys::statfs::MSDOS_SUPER_MAGIC
        || f_type == nix::sys::statfs::SMB_SUPER_MAGIC
        || f_type_val == 0x5346_544e
        || f_type_val == 0xff53_4d42
        || f_type_val == 0xfe53_4d42
}

/// Checks whether `candidate_name` exists in `destination_dir`, optionally case-insensitively.
#[must_use]
pub fn candidate_exists_in_dir(
    destination_dir: &Path,
    candidate_name: &str,
    case_insensitive: bool,
) -> bool {
    let target = destination_dir.join(candidate_name);
    if std::fs::symlink_metadata(&target).is_ok() {
        return true;
    }

    if case_insensitive && let Ok(entries) = std::fs::read_dir(destination_dir) {
        let candidate_lower = candidate_name.to_lowercase();
        for entry in entries.flatten() {
            if let Ok(name) = entry.file_name().into_string()
                && name.to_lowercase() == candidate_lower
            {
                return true;
            }
        }
    }

    false
}

/// Generates a unique collision-free filename in `destination_dir` for "Keep Both" resolution.
#[must_use]
pub fn generate_unique_name(destination_dir: &Path, original_name: &str) -> String {
    let case_insensitive = is_case_insensitive_filesystem(destination_dir);
    generate_unique_name_with_case_sensitivity(destination_dir, original_name, case_insensitive)
}

/// Generates a unique collision-free filename, allowing explicit case-sensitivity control.
#[must_use]
pub fn generate_unique_name_with_case_sensitivity(
    destination_dir: &Path,
    original_name: &str,
    case_insensitive: bool,
) -> String {
    let (stem, ext) = split_stem_and_extension(original_name);
    let (base, existing_counter) = parse_stem_counter(stem);

    let mut counter = existing_counter.map_or(1, |c| c.saturating_add(1));

    // Pre-cache lowercase directory entries if case-insensitive to avoid repeated directory scans
    let cached_names: Option<HashSet<String>> = if case_insensitive {
        std::fs::read_dir(destination_dir).ok().map(|entries| {
            entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .map(|s| s.to_lowercase())
                .collect()
        })
    } else {
        None
    };

    loop {
        let candidate_name = if ext.is_empty() {
            format!("{base} ({counter})")
        } else {
            format!("{base} ({counter}){ext}")
        };

        let exists = cached_names.as_ref().map_or_else(
            || std::fs::symlink_metadata(destination_dir.join(&candidate_name)).is_ok(),
            |cache| {
                cache.contains(&candidate_name.to_lowercase())
                    || std::fs::symlink_metadata(destination_dir.join(&candidate_name)).is_ok()
            },
        );

        if !exists {
            return candidate_name;
        }

        counter = counter.saturating_add(1);
    }
}

/// Generates a full unique [`PathBuf`] candidate in the destination folder.
#[must_use]
pub fn generate_unique_path(target_path: &Path) -> PathBuf {
    let parent = target_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = target_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("item");
    let unique_name = generate_unique_name(parent, file_name);
    parent.join(unique_name)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::redundant_clone
)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_keep_both_simple() {
        let dir = tempdir().expect("tempdir");
        let name = "photo.jpg";
        File::create(dir.path().join(name)).expect("create file");

        let unique = generate_unique_name(dir.path(), name);
        assert_eq!(unique, "photo (1).jpg");
    }

    #[test]
    fn test_keep_both_existing_counter() {
        let dir = tempdir().expect("tempdir");
        let name = "notes (3).txt";
        File::create(dir.path().join(name)).expect("create file");

        let unique = generate_unique_name(dir.path(), name);
        assert_eq!(unique, "notes (4).txt");
    }

    #[test]
    fn test_keep_both_no_extension() {
        let dir = tempdir().expect("tempdir");
        let name = "MyFolder";
        std::fs::create_dir(dir.path().join(name)).expect("create dir");

        let unique = generate_unique_name(dir.path(), name);
        assert_eq!(unique, "MyFolder (1)");
    }

    #[test]
    fn test_keep_both_compound_extension() {
        let dir = tempdir().expect("tempdir");
        let name = "archive.tar.gz";
        File::create(dir.path().join(name)).expect("create file");

        let unique = generate_unique_name(dir.path(), name);
        assert_eq!(unique, "archive (1).tar.gz");
    }

    #[test]
    fn test_keep_both_increments_past_collisions() {
        let dir = tempdir().expect("tempdir");
        File::create(dir.path().join("doc.pdf")).expect("create doc");
        File::create(dir.path().join("doc (1).pdf")).expect("create doc 1");
        File::create(dir.path().join("doc (2).pdf")).expect("create doc 2");

        let unique = generate_unique_name(dir.path(), "doc.pdf");
        assert_eq!(unique, "doc (3).pdf");
    }

    #[test]
    fn test_keep_both_case_insensitive() {
        let dir = tempdir().expect("tempdir");
        // Create uppercase collision
        File::create(dir.path().join("DOC (1).PDF")).expect("create file");

        let unique = generate_unique_name_with_case_sensitivity(dir.path(), "doc.pdf", true);
        assert_eq!(unique, "doc (2).pdf");
    }
}
