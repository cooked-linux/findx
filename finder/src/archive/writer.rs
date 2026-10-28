//! Low-level compression writer engine wrapping `libarchive` (`-larchive`).

#![allow(unsafe_code)]

use std::path::Path;
use std::sync::atomic::Ordering;

use crate::archive::error::{ArchiveError, Result};
use crate::archive::types::{ArchiveCompressionJob, ArchiveFormat, ArchiveManifest};

/// Low-level C FFI bindings to system `libarchive`.
pub(crate) mod ffi {
    use libc::{c_int, c_void, mode_t, size_t, ssize_t, time_t};
    use std::ffi::c_char;

    #[repr(C)]
    pub struct Archive {
        _private: [u8; 0],
    }

    #[repr(C)]
    pub struct ArchiveEntry {
        _private: [u8; 0],
    }

    pub const ARCHIVE_OK: c_int = 0;
    pub const ARCHIVE_WARN: c_int = -20;

    #[link(name = "archive")]
    unsafe extern "C" {
        pub fn archive_write_new() -> *mut Archive;
        pub fn archive_write_set_format_zip(a: *mut Archive) -> c_int;
        pub fn archive_write_set_format_pax_restricted(a: *mut Archive) -> c_int;
        pub fn archive_write_add_filter_gzip(a: *mut Archive) -> c_int;
        pub fn archive_write_add_filter_xz(a: *mut Archive) -> c_int;
        pub fn archive_write_add_filter_zstd(a: *mut Archive) -> c_int;
        pub fn archive_write_set_options(a: *mut Archive, opts: *const c_char) -> c_int;
        pub fn archive_write_open_filename(a: *mut Archive, filename: *const c_char) -> c_int;
        pub fn archive_write_header(a: *mut Archive, entry: *mut ArchiveEntry) -> c_int;
        pub fn archive_write_data(a: *mut Archive, buff: *const c_void, len: size_t) -> ssize_t;
        pub fn archive_write_finish_entry(a: *mut Archive) -> c_int;
        pub fn archive_write_close(a: *mut Archive) -> c_int;
        pub fn archive_write_free(a: *mut Archive) -> c_int;
        pub fn archive_error_string(a: *mut Archive) -> *const c_char;
        pub fn archive_errno(a: *mut Archive) -> c_int;

        pub fn archive_entry_new() -> *mut ArchiveEntry;
        pub fn archive_entry_free(entry: *mut ArchiveEntry);
        pub fn archive_entry_set_pathname_utf8(entry: *mut ArchiveEntry, path: *const c_char);
        pub fn archive_entry_set_size(entry: *mut ArchiveEntry, size: i64);
        pub fn archive_entry_set_filetype(entry: *mut ArchiveEntry, ftype: libc::c_uint);
        pub fn archive_entry_set_perm(entry: *mut ArchiveEntry, perm: mode_t);
        pub fn archive_entry_set_mtime(entry: *mut ArchiveEntry, sec: time_t, nsec: libc::c_long);
        pub fn archive_entry_set_symlink_utf8(entry: *mut ArchiveEntry, link: *const c_char);
    }
}

/// RAII wrapper around a `*mut ffi::Archive` writer handle.
struct ArchiveHandle {
    raw: *mut ffi::Archive,
}

impl ArchiveHandle {
    fn new() -> Result<Self> {
        // SAFETY: archive_write_new allocates an archive writer struct.
        let raw = unsafe { ffi::archive_write_new() };
        if raw.is_null() {
            return Err(ArchiveError::Libarchive {
                code: -1,
                message: "Failed to allocate libarchive writer".to_string(),
            });
        }
        Ok(Self { raw })
    }

    fn set_format(&self, format: ArchiveFormat, requires_zip64: bool) -> Result<()> {
        match format {
            ArchiveFormat::Zip => {
                // SAFETY: self.raw is guaranteed non-null.
                let ret = unsafe { ffi::archive_write_set_format_zip(self.raw) };
                self.check_res(ret, "Failed to set ZIP format")?;

                if requires_zip64 {
                    let opt = std::ffi::CString::new("zip:zip64=1")
                        .map_err(|_| ArchiveError::InvalidFormat("zip64".into()))?;
                    // SAFETY: self.raw and opt pointer are valid.
                    let ret = unsafe { ffi::archive_write_set_options(self.raw, opt.as_ptr()) };
                    self.check_res_warn(ret, "Failed to set ZIP64 option")?;
                }

                // Force UTF-8 filename encoding flag for cross-platform extraction
                let opt = std::ffi::CString::new("hdrcharset=UTF-8")
                    .map_err(|_| ArchiveError::InvalidFormat("hdrcharset".into()))?;
                // SAFETY: self.raw and opt pointer are valid.
                let ret = unsafe { ffi::archive_write_set_options(self.raw, opt.as_ptr()) };
                self.check_res_warn(ret, "Failed to set UTF-8 hdrcharset option")?;
            }
            ArchiveFormat::TarGz => {
                // SAFETY: self.raw is guaranteed non-null.
                let ret = unsafe { ffi::archive_write_set_format_pax_restricted(self.raw) };
                self.check_res(ret, "Failed to set pax restricted format")?;
                // SAFETY: self.raw is guaranteed non-null.
                let ret = unsafe { ffi::archive_write_add_filter_gzip(self.raw) };
                self.check_res(ret, "Failed to add gzip filter")?;
            }
            ArchiveFormat::TarXz => {
                // SAFETY: self.raw is guaranteed non-null.
                let ret = unsafe { ffi::archive_write_set_format_pax_restricted(self.raw) };
                self.check_res(ret, "Failed to set pax restricted format")?;
                // SAFETY: self.raw is guaranteed non-null.
                let ret = unsafe { ffi::archive_write_add_filter_xz(self.raw) };
                self.check_res(ret, "Failed to add xz filter")?;
            }
            ArchiveFormat::TarZst => {
                // SAFETY: self.raw is guaranteed non-null.
                let ret = unsafe { ffi::archive_write_set_format_pax_restricted(self.raw) };
                self.check_res(ret, "Failed to set pax restricted format")?;
                // SAFETY: self.raw is guaranteed non-null.
                let ret = unsafe { ffi::archive_write_add_filter_zstd(self.raw) };
                self.check_res(ret, "Failed to add zstd filter")?;
            }
        }
        Ok(())
    }

    fn check_res(&self, code: libc::c_int, default_msg: &str) -> Result<()> {
        if code != ffi::ARCHIVE_OK {
            let (errno, msg) = self.last_error();
            return Err(ArchiveError::Libarchive {
                code: if errno != 0 { errno } else { code },
                message: if msg.is_empty() {
                    default_msg.to_string()
                } else {
                    msg
                },
            });
        }
        Ok(())
    }

    fn check_res_warn(&self, code: libc::c_int, default_msg: &str) -> Result<()> {
        if code != ffi::ARCHIVE_OK && code != ffi::ARCHIVE_WARN {
            let (errno, msg) = self.last_error();
            return Err(ArchiveError::Libarchive {
                code: if errno != 0 { errno } else { code },
                message: if msg.is_empty() {
                    default_msg.to_string()
                } else {
                    msg
                },
            });
        }
        Ok(())
    }

    fn last_error(&self) -> (i32, String) {
        // SAFETY: self.raw is non-null.
        let code = unsafe { ffi::archive_errno(self.raw) };
        // SAFETY: self.raw is non-null.
        let msg_ptr = unsafe { ffi::archive_error_string(self.raw) };
        let msg = if msg_ptr.is_null() {
            String::new()
        } else {
            // SAFETY: msg_ptr is null-terminated C string provided by libarchive.
            unsafe { std::ffi::CStr::from_ptr(msg_ptr) }
                .to_string_lossy()
                .into_owned()
        };
        (code, msg)
    }

    fn open_filename(&self, path: &Path) -> Result<()> {
        let path_str = path.to_string_lossy();
        let c_path = std::ffi::CString::new(path_str.as_bytes())
            .map_err(|_| ArchiveError::InvalidDestination(path.to_path_buf()))?;
        // SAFETY: self.raw is non-null, c_path is a valid null-terminated C string.
        let ret = unsafe { ffi::archive_write_open_filename(self.raw, c_path.as_ptr()) };
        self.check_res(ret, "Failed to open archive destination file")
    }

    fn close(&mut self) -> Result<()> {
        if !self.raw.is_null() {
            // SAFETY: self.raw is non-null.
            let ret = unsafe { ffi::archive_write_close(self.raw) };
            self.check_res(ret, "Failed to close archive write stream")?;
        }
        Ok(())
    }
}

impl Drop for ArchiveHandle {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            // SAFETY: self.raw is valid and freed once.
            unsafe {
                ffi::archive_write_free(self.raw);
            }
        }
    }
}

/// RAII wrapper around a `*mut ffi::ArchiveEntry` item descriptor.
struct ArchiveEntryHandle {
    raw: *mut ffi::ArchiveEntry,
}

impl ArchiveEntryHandle {
    fn new() -> Result<Self> {
        // SAFETY: archive_entry_new allocates entry descriptor.
        let raw = unsafe { ffi::archive_entry_new() };
        if raw.is_null() {
            return Err(ArchiveError::Libarchive {
                code: -1,
                message: "Failed to allocate ArchiveEntry".to_string(),
            });
        }
        Ok(Self { raw })
    }

    fn set_pathname(&self, path: &str) -> Result<()> {
        let c_path = std::ffi::CString::new(path)
            .map_err(|_| ArchiveError::InvalidFormat(format!("Invalid path '{path}'")))?;
        // SAFETY: self.raw and c_path are valid.
        unsafe {
            ffi::archive_entry_set_pathname_utf8(self.raw, c_path.as_ptr());
        }
        Ok(())
    }

    fn set_filetype(&self, filetype: libc::c_uint) {
        // SAFETY: self.raw is non-null.
        unsafe {
            ffi::archive_entry_set_filetype(self.raw, filetype);
        }
    }

    fn set_perm(&self, perm: u32) {
        // Strip setuid (0o4000) and setgid (0o2000) bits for unprivileged extraction safety
        let sanitized = (perm & 0o7777) & !0o6000;
        // SAFETY: self.raw is non-null.
        unsafe {
            ffi::archive_entry_set_perm(self.raw, sanitized as libc::mode_t);
        }
    }

    fn set_size(&self, size: u64) {
        // SAFETY: self.raw is non-null.
        #[allow(clippy::cast_possible_wrap)]
        unsafe {
            ffi::archive_entry_set_size(self.raw, size as i64);
        }
    }

    fn set_mtime(&self, sec: i64, nsec: i64) {
        // SAFETY: self.raw is non-null.
        #[allow(clippy::cast_possible_truncation)]
        unsafe {
            ffi::archive_entry_set_mtime(self.raw, sec as libc::time_t, nsec as libc::c_long);
        }
    }

    fn set_symlink(&self, target: &Path) -> Result<()> {
        let target_str = target.to_string_lossy();
        let c_target = std::ffi::CString::new(target_str.as_bytes()).map_err(|_| {
            ArchiveError::InvalidFormat(format!("Invalid symlink target '{target_str}'"))
        })?;
        // SAFETY: self.raw and c_target are valid.
        unsafe {
            ffi::archive_entry_set_symlink_utf8(self.raw, c_target.as_ptr());
        }
        Ok(())
    }
}

impl Drop for ArchiveEntryHandle {
    fn drop(&mut self) {
        if !self.raw.is_null() {
            // SAFETY: self.raw is valid and freed once.
            unsafe {
                ffi::archive_entry_free(self.raw);
            }
        }
    }
}

/// Compression writer engine streaming file entries to disk using `libarchive`.
pub struct ArchiveWriter;

impl ArchiveWriter {
    /// 256 KB chunk size for streaming file reads and writes.
    pub const CHUNK_SIZE: usize = 256 * 1024;

    /// Writes all manifest items into `job.temp_archive_path`.
    ///
    /// Periodically calls `on_progress(bytes_written, files_processed, current_file)`
    /// after each chunk and upon completing each entry header.
    ///
    /// # Errors
    /// Returns [`ArchiveError::Cancelled`] if cancellation is signaled,
    /// [`ArchiveError::Io`] on disk I/O failure, or [`ArchiveError::Libarchive`] on compression error.
    #[allow(clippy::too_many_lines)]
    pub fn write_archive<F>(
        job: &ArchiveCompressionJob,
        manifest: &ArchiveManifest,
        mut on_progress: F,
    ) -> Result<()>
    where
        F: FnMut(u64, u64, &str),
    {
        if job.is_cancelled.load(Ordering::Relaxed) {
            return Err(ArchiveError::Cancelled);
        }

        // SAFETY: Initialize C locale to user environment to allow UTF-8 conversions
        unsafe {
            libc::setlocale(libc::LC_ALL, c"".as_ptr());
        }

        let mut archive = ArchiveHandle::new()?;
        archive.set_format(job.format, manifest.requires_zip64)?;
        archive.open_filename(&job.temp_archive_path)?;

        let mut files_processed: u64 = 0;
        let mut bytes_written: u64 = 0;

        for entry_item in &manifest.entries {
            if job.is_cancelled.load(Ordering::Relaxed) {
                return Err(ArchiveError::Cancelled);
            }

            let entry = ArchiveEntryHandle::new()?;

            if entry_item.is_directory {
                let mut path = entry_item.archive_rel_path.clone();
                if !path.ends_with('/') {
                    path.push('/');
                }
                entry.set_pathname(&path)?;
                entry.set_filetype(libc::S_IFDIR as libc::c_uint);
                entry.set_perm(entry_item.mode);
                entry.set_size(0);
                entry.set_mtime(entry_item.mtime_sec, entry_item.mtime_nsec);

                // SAFETY: raw pointers are valid.
                let ret = unsafe { ffi::archive_write_header(archive.raw, entry.raw) };
                archive.check_res(ret, "Failed to write directory header")?;
            } else if entry_item.is_symlink {
                entry.set_pathname(&entry_item.archive_rel_path)?;
                entry.set_filetype(libc::S_IFLNK as libc::c_uint);
                entry.set_perm(entry_item.mode);
                entry.set_size(0);
                entry.set_mtime(entry_item.mtime_sec, entry_item.mtime_nsec);

                if let Some(ref target) = entry_item.symlink_target {
                    entry.set_symlink(target)?;
                }

                // SAFETY: raw pointers are valid.
                let ret = unsafe { ffi::archive_write_header(archive.raw, entry.raw) };
                archive.check_res(ret, "Failed to write symlink header")?;
            } else {
                entry.set_pathname(&entry_item.archive_rel_path)?;
                entry.set_filetype(libc::S_IFREG as libc::c_uint);
                entry.set_perm(entry_item.mode);
                entry.set_size(entry_item.file_size);
                entry.set_mtime(entry_item.mtime_sec, entry_item.mtime_nsec);

                // SAFETY: raw pointers are valid.
                let ret = unsafe { ffi::archive_write_header(archive.raw, entry.raw) };
                archive.check_res(ret, "Failed to write file header")?;

                if entry_item.file_size > 0 {
                    let file = std::fs::File::open(&entry_item.disk_path).map_err(|e| {
                        ArchiveError::Io {
                            path: entry_item.disk_path.clone(),
                            source: e,
                        }
                    })?;
                    let mut reader = std::io::BufReader::with_capacity(Self::CHUNK_SIZE, file);
                    let mut buf = vec![0u8; Self::CHUNK_SIZE];

                    loop {
                        if job.is_cancelled.load(Ordering::Relaxed) {
                            return Err(ArchiveError::Cancelled);
                        }

                        let n = std::io::Read::read(&mut reader, &mut buf).map_err(|e| {
                            ArchiveError::Io {
                                path: entry_item.disk_path.clone(),
                                source: e,
                            }
                        })?;

                        if n == 0 {
                            break;
                        }

                        // SAFETY: archive.raw is valid and buf contains n bytes.
                        let written = unsafe {
                            ffi::archive_write_data(
                                archive.raw,
                                buf.as_ptr().cast::<libc::c_void>(),
                                n as libc::size_t,
                            )
                        };

                        if written < 0 {
                            let (code, msg) = archive.last_error();
                            return Err(ArchiveError::Libarchive {
                                code,
                                message: if msg.is_empty() {
                                    "Failed to write file data chunk".to_string()
                                } else {
                                    msg
                                },
                            });
                        }

                        bytes_written = bytes_written.saturating_add(n as u64);
                        on_progress(bytes_written, files_processed, &entry_item.archive_rel_path);
                    }
                }
            }

            // SAFETY: raw pointers are valid.
            unsafe {
                ffi::archive_write_finish_entry(archive.raw);
            }

            files_processed = files_processed.saturating_add(1);
            on_progress(bytes_written, files_processed, &entry_item.archive_rel_path);
        }

        archive.close()?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::archive::types::ArchiveEntryItem;
    use std::fs::File;
    use std::io::Write;
    use std::os::unix::fs::symlink;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use tempfile::tempdir;

    fn make_test_job(
        temp_path: std::path::PathBuf,
        final_path: std::path::PathBuf,
        format: ArchiveFormat,
    ) -> ArchiveCompressionJob {
        ArchiveCompressionJob {
            job_id: "test-job".to_string(),
            source_paths: vec![],
            destination_dir: temp_path.parent().unwrap().to_path_buf(),
            format,
            final_archive_path: final_path,
            temp_archive_path: temp_path,
            is_cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn test_create_valid_archives_all_formats() {
        let tmp = tempdir().unwrap();

        // Prepare test source file
        let src_file = tmp.path().join("hello.txt");
        let mut f = File::create(&src_file).unwrap();
        f.write_all(b"Hello from FindX Archive Engine!").unwrap();

        let entry = ArchiveEntryItem {
            disk_path: src_file,
            archive_rel_path: "hello.txt".to_string(),
            is_directory: false,
            is_symlink: false,
            symlink_target: None,
            file_size: 32,
            mode: 0o644,
            mtime_sec: 1_700_000_000,
            mtime_nsec: 0,
        };

        let manifest = ArchiveManifest {
            entries: vec![entry],
            total_uncompressed_bytes: 32,
            total_files: 1,
            requires_zip64: false,
        };

        for format in [
            ArchiveFormat::Zip,
            ArchiveFormat::TarGz,
            ArchiveFormat::TarXz,
            ArchiveFormat::TarZst,
        ] {
            let out_file = tmp.path().join(format!("test{}", format.extension()));
            let job = make_test_job(out_file.clone(), out_file.clone(), format);

            let mut progress_count = 0;
            ArchiveWriter::write_archive(&job, &manifest, |_bytes, _files, _cur| {
                progress_count += 1;
            })
            .unwrap();

            assert!(out_file.exists());
            assert!(out_file.metadata().unwrap().len() > 0);
            assert!(progress_count > 0);

            // Test readable by standard cli
            if format == ArchiveFormat::Zip {
                let status = std::process::Command::new("unzip")
                    .arg("-t")
                    .arg(&out_file)
                    .status()
                    .unwrap();
                assert!(status.success());
            } else {
                let status = std::process::Command::new("tar")
                    .arg("-tf")
                    .arg(&out_file)
                    .status()
                    .unwrap();
                assert!(status.success());
            }
        }
    }

    #[test]
    fn test_non_ascii_filename_preservation() {
        let tmp = tempdir().unwrap();

        let de_file = tmp.path().join("München.txt");
        File::create(&de_file)
            .unwrap()
            .write_all(b"Munich")
            .unwrap();

        let jp_file = tmp.path().join("日本語.txt");
        File::create(&jp_file)
            .unwrap()
            .write_all(b"Japanese")
            .unwrap();

        let manifest = ArchiveManifest {
            entries: vec![
                ArchiveEntryItem {
                    disk_path: de_file,
                    archive_rel_path: "München.txt".to_string(),
                    is_directory: false,
                    is_symlink: false,
                    symlink_target: None,
                    file_size: 6,
                    mode: 0o644,
                    mtime_sec: 1_700_000_000,
                    mtime_nsec: 0,
                },
                ArchiveEntryItem {
                    disk_path: jp_file,
                    archive_rel_path: "日本語.txt".to_string(),
                    is_directory: false,
                    is_symlink: false,
                    symlink_target: None,
                    file_size: 8,
                    mode: 0o644,
                    mtime_sec: 1_700_000_000,
                    mtime_nsec: 0,
                },
            ],
            total_uncompressed_bytes: 14,
            total_files: 2,
            requires_zip64: false,
        };

        let out_zip = tmp.path().join("unicode.zip");
        let job = make_test_job(out_zip.clone(), out_zip.clone(), ArchiveFormat::Zip);
        ArchiveWriter::write_archive(&job, &manifest, |_, _, _| {}).unwrap();

        let output = std::process::Command::new("unzip")
            .arg("-l")
            .arg(&out_zip)
            .output()
            .unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("München.txt"));
        assert!(stdout.contains("日本語.txt"));
    }

    #[test]
    fn test_preservation_of_empty_subdirectories() {
        let tmp = tempdir().unwrap();
        let empty_dir = tmp.path().join("empty_folder");
        std::fs::create_dir(&empty_dir).unwrap();

        let manifest = ArchiveManifest {
            entries: vec![ArchiveEntryItem {
                disk_path: empty_dir,
                archive_rel_path: "empty_folder".to_string(),
                is_directory: true,
                is_symlink: false,
                symlink_target: None,
                file_size: 0,
                mode: 0o755,
                mtime_sec: 1_700_000_000,
                mtime_nsec: 0,
            }],
            total_uncompressed_bytes: 0,
            total_files: 1,
            requires_zip64: false,
        };

        let out_zip = tmp.path().join("empty.zip");
        let job = make_test_job(out_zip.clone(), out_zip.clone(), ArchiveFormat::Zip);
        ArchiveWriter::write_archive(&job, &manifest, |_, _, _| {}).unwrap();

        let output = std::process::Command::new("unzip")
            .arg("-l")
            .arg(&out_zip)
            .output()
            .unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("empty_folder/"));
    }

    #[test]
    fn test_preservation_of_symlink_records_without_cyclic_traversal() {
        let tmp = tempdir().unwrap();
        let target_file = tmp.path().join("target.txt");
        File::create(&target_file)
            .unwrap()
            .write_all(b"Target content")
            .unwrap();

        let symlink_path = tmp.path().join("link_to_target");
        symlink(&target_file, &symlink_path).unwrap();

        let manifest = ArchiveManifest {
            entries: vec![
                ArchiveEntryItem {
                    disk_path: target_file,
                    archive_rel_path: "target.txt".to_string(),
                    is_directory: false,
                    is_symlink: false,
                    symlink_target: None,
                    file_size: 14,
                    mode: 0o644,
                    mtime_sec: 1_700_000_000,
                    mtime_nsec: 0,
                },
                ArchiveEntryItem {
                    disk_path: symlink_path,
                    archive_rel_path: "link_to_target".to_string(),
                    is_directory: false,
                    is_symlink: true,
                    symlink_target: Some(std::path::PathBuf::from("target.txt")),
                    file_size: 0,
                    mode: 0o777,
                    mtime_sec: 1_700_000_000,
                    mtime_nsec: 0,
                },
            ],
            total_uncompressed_bytes: 14,
            total_files: 2,
            requires_zip64: false,
        };

        let out_tar = tmp.path().join("symlinks.tar.gz");
        let job = make_test_job(out_tar.clone(), out_tar.clone(), ArchiveFormat::TarGz);
        ArchiveWriter::write_archive(&job, &manifest, |_, _, _| {}).unwrap();

        let output = std::process::Command::new("tar")
            .arg("-tvf")
            .arg(&out_tar)
            .output()
            .unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("link_to_target -> target.txt"));
    }

    #[test]
    fn test_cancellation_during_write() {
        let tmp = tempdir().unwrap();
        let file = tmp.path().join("data.bin");
        File::create(&file)
            .unwrap()
            .write_all(&[0u8; 1024])
            .unwrap();

        let cancelled_flag = Arc::new(AtomicBool::new(true)); // already cancelled
        let out_zip = tmp.path().join("cancel.zip");
        let mut job = make_test_job(out_zip.clone(), out_zip, ArchiveFormat::Zip);
        job.is_cancelled = cancelled_flag;

        let manifest = ArchiveManifest {
            entries: vec![ArchiveEntryItem {
                disk_path: file,
                archive_rel_path: "data.bin".to_string(),
                is_directory: false,
                is_symlink: false,
                symlink_target: None,
                file_size: 1024,
                mode: 0o644,
                mtime_sec: 1_700_000_000,
                mtime_nsec: 0,
            }],
            total_uncompressed_bytes: 1024,
            total_files: 1,
            requires_zip64: false,
        };

        let res = ArchiveWriter::write_archive(&job, &manifest, |_, _, _| {});
        assert!(matches!(res, Err(ArchiveError::Cancelled)));
    }
}
