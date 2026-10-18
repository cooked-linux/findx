//! Collision detection and item metadata comparator.

use std::path::Path;
use std::time::{Duration, SystemTime};

use super::models::{ConflictContext, ConflictItemMeta, RelativeTimeStatus};
use crate::operations::error::OperationError;

/// Timestamp difference threshold (1 second) for treating modification times as identical.
const TIMESTAMP_TOLERANCE: Duration = Duration::from_secs(1);

/// Checks whether an operation target collides with an existing destination item.
///
/// # Errors
/// Returns [`OperationError::Invalid`] if a cycle is detected (e.g. source is an ancestor of destination),
/// or [`OperationError::Io`] if inspecting metadata fails for reasons other than non-existence.
pub fn check_collision(
    operation_id: &str,
    source_path: &Path,
    destination_path: &Path,
) -> Result<Option<ConflictContext>, OperationError> {
    let dst_meta = match std::fs::symlink_metadata(destination_path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(OperationError::Io {
                path: destination_path.to_path_buf(),
                source: err,
            });
        }
    };

    let src_meta = std::fs::symlink_metadata(source_path).map_err(|err| OperationError::Io {
        path: source_path.to_path_buf(),
        source: err,
    })?;

    // Prevent cyclic parent-into-child operations
    let canon_src = source_path
        .canonicalize()
        .unwrap_or_else(|_| source_path.to_path_buf());
    let canon_dst = destination_path
        .canonicalize()
        .unwrap_or_else(|_| destination_path.to_path_buf());

    if canon_src == canon_dst {
        return Err(OperationError::Invalid(
            "Cannot copy or move item onto itself".into(),
        ));
    }

    if canon_dst.starts_with(&canon_src) {
        return Err(OperationError::Invalid(
            "Cannot copy or move folder into its own subfolder".into(),
        ));
    }

    let dst_is_dir = dst_meta.is_dir();
    let src_is_dir = src_meta.is_dir();

    if canon_src.starts_with(&canon_dst) && dst_is_dir {
        return Err(OperationError::Invalid(
            "Cannot copy or move folder into its own subfolder".into(),
        ));
    }

    let src_mtime = src_meta.modified().ok();
    let dst_mtime = dst_meta.modified().ok();

    let relative_time = compare_modification_times(src_mtime, dst_mtime);
    let is_directory_collision = src_is_dir && dst_is_dir;

    let src_size = if src_is_dir { 0 } else { src_meta.len() };
    let dst_size = if dst_is_dir { 0 } else { dst_meta.len() };

    let context = ConflictContext::new(
        operation_id.to_string(),
        ConflictItemMeta::new(
            source_path.to_path_buf(),
            src_is_dir,
            src_size,
            src_mtime,
            None,
        ),
        ConflictItemMeta::new(
            destination_path.to_path_buf(),
            dst_is_dir,
            dst_size,
            dst_mtime,
            None,
        ),
        is_directory_collision,
        relative_time,
    );

    Ok(Some(context))
}

/// Compares two modification timestamps against a 1.0-second tolerance window.
#[allow(clippy::option_if_let_else)]
fn compare_modification_times(
    src_mtime: Option<SystemTime>,
    dst_mtime: Option<SystemTime>,
) -> RelativeTimeStatus {
    let (Some(src), Some(dst)) = (src_mtime, dst_mtime) else {
        return RelativeTimeStatus::IdenticalTime;
    };

    if let Ok(duration) = src.duration_since(dst) {
        if duration > TIMESTAMP_TOLERANCE {
            RelativeTimeStatus::IncomingNewer
        } else {
            RelativeTimeStatus::IdenticalTime
        }
    } else if let Ok(duration) = dst.duration_since(src) {
        if duration > TIMESTAMP_TOLERANCE {
            RelativeTimeStatus::IncomingOlder
        } else {
            RelativeTimeStatus::IdenticalTime
        }
    } else {
        RelativeTimeStatus::IdenticalTime
    }
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
    fn test_collision_detection_no_collision() {
        let dir = tempdir().expect("tempdir");
        let src = dir.path().join("source.txt");
        let dst = dir.path().join("dest.txt");
        File::create(&src).expect("create src");

        let res = check_collision("op-1", &src, &dst).expect("check_collision");
        assert!(
            res.is_none(),
            "should report no collision for absent destination"
        );
    }

    #[test]
    fn test_collision_detection_relative_time_newer_older_identical() {
        let dir = tempdir().expect("tempdir");
        let src = dir.path().join("source.txt");
        let dst = dir.path().join("dest.txt");

        File::create(&src).expect("create src");
        File::create(&dst).expect("create dst");

        let base_time = SystemTime::now();

        // 1. Incoming is newer by 5 seconds
        let older_time = base_time - Duration::from_secs(10);
        let newer_time = base_time;

        filetime::set_file_mtime(&src, filetime::FileTime::from_system_time(newer_time))
            .expect("set mtime src");
        filetime::set_file_mtime(&dst, filetime::FileTime::from_system_time(older_time))
            .expect("set mtime dst");

        let ctx = check_collision("op-1", &src, &dst)
            .expect("check_collision")
            .expect("collision exists");
        assert_eq!(ctx.relative_time, RelativeTimeStatus::IncomingNewer);
        assert!(!ctx.is_directory_collision);

        // 2. Incoming is older by 5 seconds
        filetime::set_file_mtime(&src, filetime::FileTime::from_system_time(older_time))
            .expect("set mtime src");
        filetime::set_file_mtime(&dst, filetime::FileTime::from_system_time(newer_time))
            .expect("set mtime dst");

        let ctx = check_collision("op-1", &src, &dst)
            .expect("check_collision")
            .expect("collision exists");
        assert_eq!(ctx.relative_time, RelativeTimeStatus::IncomingOlder);

        // 3. Timestamps within 0.5s tolerance -> IdenticalTime
        let close_time = base_time + Duration::from_millis(500);
        filetime::set_file_mtime(&src, filetime::FileTime::from_system_time(close_time))
            .expect("set mtime src");
        filetime::set_file_mtime(&dst, filetime::FileTime::from_system_time(base_time))
            .expect("set mtime dst");

        let ctx = check_collision("op-1", &src, &dst)
            .expect("check_collision")
            .expect("collision exists");
        assert_eq!(ctx.relative_time, RelativeTimeStatus::IdenticalTime);
    }

    #[test]
    fn test_collision_detection_directory_collision() {
        let dir = tempdir().expect("tempdir");
        let src = dir.path().join("src_dir");
        let dst = dir.path().join("dst_dir");

        std::fs::create_dir(&src).expect("create src dir");
        std::fs::create_dir(&dst).expect("create dst dir");

        let ctx = check_collision("op-dir", &src, &dst)
            .expect("check_collision")
            .expect("collision exists");
        assert!(ctx.is_directory_collision);
        assert!(ctx.source.is_dir);
        assert!(ctx.destination.is_dir);
    }

    #[test]
    fn test_collision_detection_cycle_prevention() {
        let dir = tempdir().expect("tempdir");
        let parent = dir.path().join("parent_folder");
        let child = parent.join("child_folder");

        std::fs::create_dir_all(&child).expect("create child");

        let err = check_collision("op-cycle", &parent, &child).unwrap_err();
        match err {
            OperationError::Invalid(msg) => {
                assert!(msg.contains("subfolder"));
            }
            other => panic!("expected Invalid error, got {other:?}"),
        }
    }
}
