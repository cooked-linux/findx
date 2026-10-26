//! Smart contextual archive naming and temporary staging path generation.

use std::path::{Path, PathBuf};

use crate::archive::types::ArchiveFormat;
use crate::operations::naming::{
    candidate_exists_in_dir, is_case_insensitive_filesystem, split_stem_and_extension,
};

/// Computes the contextual base name for an archive derived from selected items.
///
/// - If a single directory is selected, uses the directory's name directly.
/// - If a single file is selected, strips its extension (including compound extensions like `.tar.gz`).
/// - If a single dotfile without extension is selected (e.g. `.bashrc`), preserves the dot name.
/// - If multiple items or an empty slice are selected, defaults to `"Archive"`.
#[must_use]
pub fn determine_archive_base_name(source_paths: &[PathBuf]) -> String {
    if source_paths.len() == 1 {
        let path = &source_paths[0];
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Archive");

        if path.is_dir() {
            file_name.to_string()
        } else {
            let (stem, _) = split_stem_and_extension(file_name);
            if stem.is_empty() {
                "Archive".to_string()
            } else {
                stem.to_string()
            }
        }
    } else {
        "Archive".to_string()
    }
}

/// Resolves a collision-free filename in `destination_dir` following macOS conventions.
///
/// Sequential resolution attempts:
/// 1. `{base_name}{format.extension()}` (e.g. `Archive.zip`)
/// 2. `{base_name} 2{format.extension()}` (e.g. `Archive 2.zip`)
/// 3. `{base_name} {N}{format.extension()}` (e.g. `Archive 3.zip`)
#[must_use]
pub fn determine_archive_name(
    destination_dir: &Path,
    source_paths: &[PathBuf],
    format: ArchiveFormat,
) -> String {
    let base_name = determine_archive_base_name(source_paths);
    let case_insensitive = is_case_insensitive_filesystem(destination_dir);
    let ext = format.extension();

    let initial_candidate = format!("{base_name}{ext}");
    if !candidate_exists_in_dir(destination_dir, &initial_candidate, case_insensitive) {
        return initial_candidate;
    }

    let mut counter: u64 = 2;
    loop {
        let candidate = format!("{base_name} {counter}{ext}");
        if !candidate_exists_in_dir(destination_dir, &candidate, case_insensitive) {
            return candidate;
        }
        counter = counter.saturating_add(1);
    }
}

/// Computes the complete target destination path for the prospective archive.
#[must_use]
pub fn determine_archive_path(
    destination_dir: &Path,
    source_paths: &[PathBuf],
    format: ArchiveFormat,
) -> PathBuf {
    let name = determine_archive_name(destination_dir, source_paths, format);
    destination_dir.join(name)
}

/// Generates a hidden temporary staging file path in the same target directory.
///
/// The hidden prefix (`.`) ensures partial data is invisible in directory listings and
/// file watchers, and co-location in `destination_dir` guarantees POSIX `rename(2)` atomicity.
#[must_use]
pub fn generate_staging_path(destination_dir: &Path, final_path: &Path) -> PathBuf {
    let file_name = final_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Archive.zip");
    let pid = std::process::id();
    let nonce = uuid::Uuid::new_v4().simple();
    destination_dir.join(format!(".{file_name}.tmp_{pid}_{nonce}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::fs::File;
    use tempfile::tempdir;

    #[test]
    fn test_single_directory_naming() {
        let tmp = tempdir().unwrap();
        let folder = tmp.path().join("Photos");
        std::fs::create_dir(&folder).unwrap();

        let name = determine_archive_name(tmp.path(), &[folder], ArchiveFormat::Zip);
        assert_eq!(name, "Photos.zip");
    }

    #[test]
    fn test_single_file_with_extension_stripping() {
        let tmp = tempdir().unwrap();
        let file = tmp.path().join("Document.pdf");
        File::create(&file).unwrap();

        let name = determine_archive_name(tmp.path(), &[file], ArchiveFormat::Zip);
        assert_eq!(name, "Document.zip");
    }

    #[test]
    fn test_single_compound_extension_stripping() {
        let tmp = tempdir().unwrap();
        let file = tmp.path().join("backup.tar.gz");
        File::create(&file).unwrap();

        let name = determine_archive_name(tmp.path(), &[file], ArchiveFormat::Zip);
        assert_eq!(name, "backup.zip");
    }

    #[test]
    fn test_single_hidden_file_naming() {
        let tmp = tempdir().unwrap();
        let file = tmp.path().join(".bashrc");
        File::create(&file).unwrap();

        let name = determine_archive_name(tmp.path(), &[file], ArchiveFormat::Zip);
        assert_eq!(name, ".bashrc.zip");
    }

    #[test]
    fn test_multi_item_default_naming() {
        let tmp = tempdir().unwrap();
        let file1 = tmp.path().join("a.txt");
        let file2 = tmp.path().join("b.txt");
        File::create(&file1).unwrap();
        File::create(&file2).unwrap();

        let name = determine_archive_name(tmp.path(), &[file1, file2], ArchiveFormat::Zip);
        assert_eq!(name, "Archive.zip");
    }

    #[test]
    fn test_collision_incrementing() {
        let tmp = tempdir().unwrap();
        let file1 = tmp.path().join("a.txt");
        let file2 = tmp.path().join("b.txt");
        File::create(&file1).unwrap();
        File::create(&file2).unwrap();

        // No existing archive -> Archive.zip
        let name1 = determine_archive_name(
            tmp.path(),
            &[file1.clone(), file2.clone()],
            ArchiveFormat::Zip,
        );
        assert_eq!(name1, "Archive.zip");

        // Create Archive.zip
        File::create(tmp.path().join("Archive.zip")).unwrap();
        let name2 = determine_archive_name(
            tmp.path(),
            &[file1.clone(), file2.clone()],
            ArchiveFormat::Zip,
        );
        assert_eq!(name2, "Archive 2.zip");

        // Create Archive 2.zip
        File::create(tmp.path().join("Archive 2.zip")).unwrap();
        let name3 = determine_archive_name(tmp.path(), &[file1, file2], ArchiveFormat::Zip);
        assert_eq!(name3, "Archive 3.zip");
    }

    #[test]
    fn test_different_archive_format_extensions() {
        let tmp = tempdir().unwrap();
        let folder = tmp.path().join("Source");
        std::fs::create_dir(&folder).unwrap();

        assert_eq!(
            determine_archive_name(
                tmp.path(),
                std::slice::from_ref(&folder),
                ArchiveFormat::TarGz
            ),
            "Source.tar.gz"
        );
        assert_eq!(
            determine_archive_name(
                tmp.path(),
                std::slice::from_ref(&folder),
                ArchiveFormat::TarXz
            ),
            "Source.tar.xz"
        );
        assert_eq!(
            determine_archive_name(tmp.path(), &[folder], ArchiveFormat::TarZst),
            "Source.tar.zst"
        );
    }

    #[test]
    fn test_staging_path_structure() {
        let tmp = tempdir().unwrap();
        let final_path = tmp.path().join("Archive.zip");
        let staging = generate_staging_path(tmp.path(), &final_path);

        assert_eq!(staging.parent().unwrap(), tmp.path());
        let file_name = staging.file_name().unwrap().to_str().unwrap();
        assert!(file_name.starts_with(".Archive.zip.tmp_"));
        assert_ne!(staging, final_path);
    }
}
