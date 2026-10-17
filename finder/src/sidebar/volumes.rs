//! `UDisks2` Removable Volume Discovery, Eject Service, and `/proc/mounts` Fallback.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use shared::ipc::{DriveConnectionType, MountedVolume, UDisks2Monitor};

/// Discovered storage device or filesystem mount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredVolume {
    /// Identifier: D-Bus object path (e.g. `/org/freedesktop/UDisks2/block_devices/sdb1`) or device node (`/dev/sdb1`).
    pub device_path: String,
    /// Primary filesystem mount point.
    pub mount_point: PathBuf,
    /// Human-readable partition or volume label.
    pub label: String,
    /// Recommended Freedesktop symbolic icon name.
    pub icon_name: String,
    /// Whether this volume is removable (USB, SD card, optical, external drive).
    pub is_removable: bool,
    /// Whether this volume can be unmounted or ejected.
    pub is_ejectable: bool,
    /// Total capacity in bytes, if known.
    pub size_bytes: u64,
}

impl DiscoveredVolume {
    /// Creates a [`DiscoveredVolume`] from a [`MountedVolume`] emitted by `shared::ipc::UDisks2Monitor`.
    #[must_use]
    pub fn from_mounted_volume(vol: &MountedVolume) -> Self {
        let is_removable = vol.is_removable
            || matches!(
                vol.connection_type,
                DriveConnectionType::Usb | DriveConnectionType::Sdio
            );
        let icon_name = match vol.connection_type {
            DriveConnectionType::Usb | DriveConnectionType::Sdio => {
                "drive-removable-media-symbolic".to_string()
            }
            _ if is_removable => "drive-removable-media-symbolic".to_string(),
            _ => "drive-harddisk-symbolic".to_string(),
        };

        Self {
            device_path: vol.id.clone(),
            mount_point: vol.mount_path.clone(),
            label: vol.label.clone(),
            icon_name,
            is_removable,
            is_ejectable: is_removable,
            size_bytes: vol.size_bytes,
        }
    }
}

/// Service managing device discovery and volume ejection.
#[derive(Debug, Clone)]
pub struct VolumeService {
    monitor: UDisks2Monitor,
}

impl Default for VolumeService {
    fn default() -> Self {
        Self::new()
    }
}

impl VolumeService {
    /// Constructs a new [`VolumeService`] with standard timeout settings.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            monitor: UDisks2Monitor::new().with_timeout(Duration::from_millis(2500)),
        }
    }

    /// Queries mounted volumes via `UDisks2`, falling back to `/proc/mounts` on failure.
    pub async fn discover_volumes(&self) -> Vec<DiscoveredVolume> {
        match self.monitor.query_mounted_volumes().await {
            Ok(vols) if !vols.is_empty() => vols
                .iter()
                .map(DiscoveredVolume::from_mounted_volume)
                .collect(),
            _ => parse_proc_mounts(),
        }
    }

    /// Unmounts or safely ejects the volume at `device_path`.
    ///
    /// # Errors
    ///
    /// Returns an error string if unmounting fails, including details of any blocking processes.
    pub async fn unmount_volume(
        &self,
        device_path: &str,
        mount_point: Option<&Path>,
    ) -> Result<(), String> {
        unmount_volume_internal(&self.monitor, device_path, mount_point).await
    }
}

/// Standalone function for unmounting a volume.
///
/// # Errors
///
/// Returns an error string if unmounting fails or the target is in use by blocking processes.
pub async fn unmount_volume(device_path: &str) -> Result<(), String> {
    let monitor = UDisks2Monitor::new().with_timeout(Duration::from_millis(3000));
    unmount_volume_internal(&monitor, device_path, None).await
}

async fn unmount_volume_internal(
    monitor: &UDisks2Monitor,
    device_path: &str,
    mount_point: Option<&Path>,
) -> Result<(), String> {
    if device_path.starts_with("/org/freedesktop/UDisks2/") {
        match monitor.unmount(device_path).await {
            Ok(()) => Ok(()),
            Err(err) => {
                let err_str = err.to_string();
                if err_str.contains("busy")
                    || err_str.contains("DeviceBusy")
                    || err_str.contains("EBUSY")
                {
                    let blocking = mount_point.map_or_else(Vec::new, find_blocking_processes);
                    if blocking.is_empty() {
                        Err(format!("Device {device_path} is currently busy"))
                    } else {
                        Err(format!(
                            "Device is in use by applications: {}",
                            blocking.join(", ")
                        ))
                    }
                } else {
                    Err(err_str)
                }
            }
        }
    } else {
        // Fallback for raw /dev nodes or system mounts
        let dev_or_point = mount_point.map_or_else(
            || device_path.to_string(),
            |p| p.to_string_lossy().into_owned(),
        );

        let result = tokio::task::spawn_blocking(move || {
            std::process::Command::new("umount")
                .arg(&dev_or_point)
                .output()
        })
        .await
        .map_err(|e| e.to_string())?;

        match result {
            Ok(output) if output.status.success() => Ok(()),
            Ok(output) => {
                let err_msg = String::from_utf8_lossy(&output.stderr).into_owned();
                let blocking = mount_point.map_or_else(Vec::new, find_blocking_processes);
                if blocking.is_empty() {
                    Err(if err_msg.trim().is_empty() {
                        format!("Failed to unmount {device_path}")
                    } else {
                        err_msg.trim().to_string()
                    })
                } else {
                    Err(format!(
                        "Volume is in use by: {}. {}",
                        blocking.join(", "),
                        err_msg.trim()
                    ))
                }
            }
            Err(e) => Err(e.to_string()),
        }
    }
}

/// Parses active mounts from `/proc/mounts`, filtering pseudo-filesystems and virtual mounts.
#[must_use]
pub fn parse_proc_mounts() -> Vec<DiscoveredVolume> {
    let Ok(content) = fs::read_to_string("/proc/mounts") else {
        return Vec::new();
    };

    parse_mount_lines(&content)
}

/// Parses raw `/proc/mounts` content lines into [`DiscoveredVolume`] entries.
#[must_use]
pub fn parse_mount_lines(content: &str) -> Vec<DiscoveredVolume> {
    let mut volumes = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }

        let spec = parts[0];
        let mount_point_str = unescape_octal(parts[1]);
        let fstype = parts[2];

        // Filter for physical block devices (/dev/...) and skip pseudo filesystems
        if !spec.starts_with("/dev/") {
            continue;
        }
        if spec.starts_with("/dev/loop") || spec.starts_with("/dev/ram") {
            continue;
        }
        if matches!(fstype, "devtmpfs" | "tmpfs" | "squashfs") {
            continue;
        }

        let mount_point = PathBuf::from(&mount_point_str);
        let dev_name = spec.strip_prefix("/dev/").unwrap_or(spec);

        let is_removable = check_device_removable(dev_name)
            || mount_point_str.starts_with("/media")
            || mount_point_str.starts_with("/run/media");

        let label = if mount_point_str == "/" {
            "System Root".to_string()
        } else {
            mount_point
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(dev_name)
                .to_string()
        };

        let icon_name = if is_removable {
            "drive-removable-media-symbolic".to_string()
        } else {
            "drive-harddisk-symbolic".to_string()
        };

        volumes.push(DiscoveredVolume {
            device_path: spec.to_string(),
            mount_point,
            label,
            icon_name,
            is_removable,
            is_ejectable: is_removable,
            size_bytes: 0,
        });
    }

    volumes
}

fn check_device_removable(dev_name: &str) -> bool {
    let trimmed_name = dev_name.trim_end_matches(|c: char| c.is_ascii_digit());
    let sys_path = format!("/sys/block/{trimmed_name}/removable");
    if let Ok(content) = fs::read_to_string(sys_path)
        && content.trim() == "1"
    {
        return true;
    }
    false
}

/// Decodes octal escape sequences in `/proc/mounts` strings (e.g. `\040` -> space).
#[must_use]
pub fn unescape_octal(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            let mut octal = String::new();
            for _ in 0..3 {
                if let Some(&next_c) = chars.peek() {
                    if ('0'..='7').contains(&next_c) {
                        octal.push(next_c);
                        chars.next();
                    } else {
                        break;
                    }
                }
            }

            if octal.len() == 3
                && let Ok(val) = u8::from_str_radix(&octal, 8)
            {
                out.push(val as char);
                continue;
            }
            out.push('\\');
            out.push_str(&octal);
        } else {
            out.push(c);
        }
    }

    out
}

/// Identifies active process names holding open file descriptors under `mount_point`.
#[must_use]
pub fn find_blocking_processes(mount_point: &Path) -> Vec<String> {
    let mut processes = Vec::new();
    let Ok(proc_dir) = fs::read_dir("/proc") else {
        return processes;
    };

    for entry in proc_dir.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }

        let pid_path = entry.path();
        let fd_dir = pid_path.join("fd");
        let mut matched = false;

        if let Ok(fds) = fs::read_dir(&fd_dir) {
            for fd_entry in fds.flatten() {
                if let Ok(target) = fs::read_link(fd_entry.path())
                    && target.starts_with(mount_point)
                {
                    matched = true;
                    break;
                }
            }
        }

        if matched {
            let comm = fs::read_to_string(pid_path.join("comm"))
                .map_or_else(|_| name_str.into_owned(), |s| s.trim().to_string());
            if !processes.contains(&comm) {
                processes.push(comm);
            }
        }
    }

    processes
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_unescape_octal() {
        assert_eq!(unescape_octal("/media/USB\\040Drive"), "/media/USB Drive");
        assert_eq!(unescape_octal("/plain/path"), "/plain/path");
    }

    #[test]
    fn test_parse_mount_lines() {
        let sample = r"
sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0
/dev/sda2 / ext4 rw,relatime 0 0
/dev/sdb1 /run/media/user/Thumb\040Drive vfat rw,nosuid,nodev 0 0
/dev/loop0 /var/lib/snapd/snaps/core squashfs ro 0 0
";
        let volumes = parse_mount_lines(sample);
        assert_eq!(volumes.len(), 2);
        assert_eq!(volumes[0].device_path, "/dev/sda2");
        assert_eq!(volumes[0].mount_point, PathBuf::from("/"));
        assert_eq!(volumes[0].label, "System Root");
        assert!(!volumes[0].is_removable);

        assert_eq!(volumes[1].device_path, "/dev/sdb1");
        assert_eq!(
            volumes[1].mount_point,
            PathBuf::from("/run/media/user/Thumb Drive")
        );
        assert_eq!(volumes[1].label, "Thumb Drive");
        assert!(volumes[1].is_removable);
    }
}
