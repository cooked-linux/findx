//! Volume and storage device monitoring via `org.freedesktop.UDisks2`.

use std::collections::HashMap;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use std::time::Duration;
use zbus::Connection;
use zbus::export::ordered_stream::OrderedStreamExt;
use zbus::proxy;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

use crate::errors::{IpcError, ModelError, Result, SharedError};
use crate::ipc::connection::{DEFAULT_TIMEOUT, get_system_connection};

/// Drive connection interface bus type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DriveConnectionType {
    /// USB bus.
    Usb,
    /// SATA / ATA / SCSI bus.
    Sata,
    /// `NVMe` / `PCIe` bus.
    Nvme,
    /// SD card / eMMC bus.
    Sdio,
    /// Virtual or unknown bus.
    Other,
}

impl DriveConnectionType {
    /// Parses connection bus string into a known enum variant.
    #[must_use]
    pub fn from_bus_str(bus: &str) -> Self {
        match bus.trim().to_ascii_lowercase().as_str() {
            "usb" => Self::Usb,
            "sata" | "ata" | "scsi" => Self::Sata,
            "nvme" | "pcie" => Self::Nvme,
            "sdio" | "mmc" => Self::Sdio,
            _ => Self::Other,
        }
    }
}

/// A mounted filesystem volume discovered by `UDisks2`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MountedVolume {
    /// D-Bus object path of the block device.
    pub id: String,
    /// Human-readable filesystem or partition label.
    pub label: String,
    /// Primary filesystem mount path.
    pub mount_path: PathBuf,
    /// Total volume capacity in bytes.
    pub size_bytes: u64,
    /// Physical drive connection bus type.
    pub connection_type: DriveConnectionType,
    /// Whether the volume or media is removable / ejectable.
    pub is_removable: bool,
    /// D-Bus object path of the parent drive, if available.
    pub drive_path: Option<String>,
}

/// Dynamic device change event emitted when block devices or drives are added/removed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DeviceEvent {
    /// Device interface or storage object added to the bus.
    Added(String),
    /// Device interface or storage object removed from the bus.
    Removed(String),
}

/// D-Bus `ObjectManager` proxy for `org.freedesktop.UDisks2`.
#[proxy(
    interface = "org.freedesktop.DBus.ObjectManager",
    default_service = "org.freedesktop.UDisks2",
    default_path = "/org/freedesktop/UDisks2"
)]
pub trait UDisks2ObjectManager {
    /// Returns all managed objects, interfaces, and properties from `UDisks2`.
    #[allow(clippy::type_complexity)]
    fn get_managed_objects(
        &self,
    ) -> zbus::Result<HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>>;

    /// Emitted when objects or interfaces are added.
    #[zbus(signal)]
    fn interfaces_added(
        &self,
        object_path: OwnedObjectPath,
        interfaces_and_properties: HashMap<String, HashMap<String, OwnedValue>>,
    ) -> zbus::Result<()>;

    /// Emitted when objects or interfaces are removed.
    #[zbus(signal)]
    fn interfaces_removed(
        &self,
        object_path: OwnedObjectPath,
        interfaces: Vec<String>,
    ) -> zbus::Result<()>;
}

/// D-Bus proxy for `UDisks2` filesystem interface on a block device.
#[proxy(
    interface = "org.freedesktop.UDisks2.Filesystem",
    default_service = "org.freedesktop.UDisks2"
)]
pub trait UDisks2Filesystem {
    /// Unmounts the filesystem from all active mount points.
    fn unmount(&self, options: &HashMap<&str, Value<'_>>) -> zbus::Result<()>;
}

/// D-Bus proxy for `UDisks2` drive interface.
#[proxy(
    interface = "org.freedesktop.UDisks2.Drive",
    default_service = "org.freedesktop.UDisks2"
)]
pub trait UDisks2Drive {
    /// Ejects removable media from the drive.
    fn eject(&self, options: &HashMap<&str, Value<'_>>) -> zbus::Result<()>;

    /// Powers off the drive safely for removal.
    fn power_off(&self, options: &HashMap<&str, Value<'_>>) -> zbus::Result<()>;
}

/// D-Bus proxy for `UDisks2` block device interface.
#[proxy(
    interface = "org.freedesktop.UDisks2.Block",
    default_service = "org.freedesktop.UDisks2"
)]
pub trait UDisks2Block {
    /// The parent drive object path for this block device.
    #[zbus(property)]
    fn drive(&self) -> zbus::Result<OwnedObjectPath>;
}

/// High-level volume and device monitor communicating with `UDisks2`.
#[derive(Debug, Clone)]
pub struct UDisks2Monitor {
    connection: Option<Connection>,
    timeout: Duration,
}

impl Default for UDisks2Monitor {
    fn default() -> Self {
        Self::new()
    }
}

impl UDisks2Monitor {
    /// Creates a new `UDisks2Monitor` using the lazy singleton system connection.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            connection: None,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Creates a `UDisks2Monitor` with an explicit D-Bus connection.
    #[must_use]
    pub const fn with_connection(conn: Connection) -> Self {
        Self {
            connection: Some(conn),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Configures a custom timeout for `UDisks2` queries and calls.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    async fn get_conn(&self) -> Result<Connection> {
        if let Some(conn) = &self.connection {
            Ok(conn.clone())
        } else {
            get_system_connection().await
        }
    }

    /// Queries all currently mounted filesystem block devices and their metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if `UDisks2` communication fails or times out.
    pub async fn query_mounted_volumes(&self) -> Result<Vec<MountedVolume>> {
        let conn = self.get_conn().await?;
        let manager = UDisks2ObjectManagerProxy::new(&conn).await?;
        let timeout_ms = u64::try_from(self.timeout.as_millis()).unwrap_or(u64::MAX);

        let managed_objects = match tokio::time::timeout(self.timeout, manager.get_managed_objects()).await {
            Ok(Ok(objs)) => objs,
            Ok(Err(err)) => return Err(SharedError::Ipc(IpcError::Zbus(err))),
            Err(_) => {
                return Err(SharedError::Ipc(IpcError::timeout(
                    "UDisks2.GetManagedObjects",
                    timeout_ms,
                )));
            }
        };

        // 1. Index all drives to correlate connection bus and removability
        let mut drives_info: HashMap<String, (DriveConnectionType, bool)> = HashMap::new();
        for (path, ifaces) in &managed_objects {
            if let Some(drive_props) = ifaces.get("org.freedesktop.UDisks2.Drive") {
                let connection_bus = get_str_prop(drive_props, "ConnectionBus").unwrap_or_default();
                let removable = get_bool_prop(drive_props, "Removable");
                let ejectable = get_bool_prop(drive_props, "Ejectable");

                let conn_type = DriveConnectionType::from_bus_str(&connection_bus);
                drives_info.insert(path.as_str().to_string(), (conn_type, removable || ejectable));
            }
        }

        // 2. Discover mounted filesystems
        let mut volumes = Vec::new();
        for (path, ifaces) in &managed_objects {
            let Some(fs_props) = ifaces.get("org.freedesktop.UDisks2.Filesystem") else {
                continue;
            };
            let Some(block_props) = ifaces.get("org.freedesktop.UDisks2.Block") else {
                continue;
            };

            let mount_points = fs_props
                .get("MountPoints")
                .map(parse_mount_points)
                .unwrap_or_default();

            if mount_points.is_empty() {
                continue;
            }

            let primary_mount = mount_points.into_iter().next().unwrap_or_default();

            let label = get_str_prop(block_props, "IdLabel")
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    primary_mount
                        .file_name()
                        .map_or_else(|| path.as_str().to_string(), |n| n.to_string_lossy().into_owned())
                });

            let size_bytes = get_u64_prop(block_props, "Size");

            let drive_path = get_object_path_prop(block_props, "Drive")
                .filter(|s| s != "/");

            let (connection_type, is_removable) = drive_path.as_ref().map_or(
                (DriveConnectionType::Other, false),
                |dp| drives_info.get(dp).copied().unwrap_or((DriveConnectionType::Other, false)),
            );

            volumes.push(MountedVolume {
                id: path.as_str().to_string(),
                label,
                mount_path: primary_mount,
                size_bytes,
                connection_type,
                is_removable,
                drive_path,
            });
        }

        Ok(volumes)
    }

    /// Unmounts the filesystem at the specified block device object path.
    ///
    /// # Errors
    ///
    /// Returns an error if the unmount call fails or times out.
    pub async fn unmount(&self, block_device_path: &str) -> Result<()> {
        let conn = self.get_conn().await?;
        let path = ObjectPath::try_from(block_device_path)
            .map_err(|_| ModelError::validation("Invalid D-Bus object path"))?;
        let proxy = UDisks2FilesystemProxy::builder(&conn)
            .path(path)
            .map_err(|e| SharedError::Ipc(IpcError::Zbus(e)))?
            .build()
            .await?;

        let options: HashMap<&str, Value<'_>> = HashMap::new();
        let timeout_ms = u64::try_from(self.timeout.as_millis()).unwrap_or(u64::MAX);

        match tokio::time::timeout(self.timeout, proxy.unmount(&options)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(SharedError::Ipc(IpcError::Zbus(err))),
            Err(_) => Err(SharedError::Ipc(IpcError::timeout(
                "UDisks2.Filesystem.Unmount",
                timeout_ms,
            ))),
        }
    }

    /// Ejects or safely powers off the drive corresponding to `drive_or_block_path`.
    ///
    /// # Errors
    ///
    /// Returns an error if the eject operation fails or times out.
    pub async fn eject(&self, drive_or_block_path: &str) -> Result<()> {
        let conn = self.get_conn().await?;
        let path_str = if drive_or_block_path.contains("/block_devices/") {
            // Find corresponding drive path from block device
            let block_obj = ObjectPath::try_from(drive_or_block_path)
                .map_err(|_| ModelError::validation("Invalid block device object path"))?;
            let block_proxy = UDisks2BlockProxy::builder(&conn)
                .path(block_obj)
                .map_err(|e| SharedError::Ipc(IpcError::Zbus(e)))?
                .build()
                .await?;
            let drive_path = block_proxy.drive().await?;
            if drive_path.as_str() == "/" {
                return Err(SharedError::Ipc(IpcError::invalid_response(
                    "Block device does not have an associated drive for eject",
                )));
            }
            drive_path.as_str().to_string()
        } else {
            drive_or_block_path.to_string()
        };

        let drive_obj = ObjectPath::try_from(path_str.as_str())
            .map_err(|_| ModelError::validation("Invalid drive object path"))?;
        let drive_proxy = UDisks2DriveProxy::builder(&conn)
            .path(drive_obj)
            .map_err(|e| SharedError::Ipc(IpcError::Zbus(e)))?
            .build()
            .await?;

        let options: HashMap<&str, Value<'_>> = HashMap::new();
        let timeout_ms = u64::try_from(self.timeout.as_millis()).unwrap_or(u64::MAX);

        // Try Eject first; if unsupported, try PowerOff
        match tokio::time::timeout(self.timeout, drive_proxy.eject(&options)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => {
                // Fallback to power_off
                match tokio::time::timeout(self.timeout, drive_proxy.power_off(&options)).await {
                    Ok(Ok(())) => Ok(()),
                    Ok(Err(err)) => Err(SharedError::Ipc(IpcError::Zbus(err))),
                    Err(_) => Err(SharedError::Ipc(IpcError::timeout(
                        "UDisks2.Drive.PowerOff",
                        timeout_ms,
                    ))),
                }
            }
            Err(_) => Err(SharedError::Ipc(IpcError::timeout(
                "UDisks2.Drive.Eject",
                timeout_ms,
            ))),
        }
    }

    /// Listens for `InterfacesAdded` and `InterfacesRemoved` signals from `UDisks2`.
    ///
    /// # Errors
    ///
    /// Returns an error if the signal stream cannot be initiated.
    pub async fn listen_device_changes(&self) -> Result<tokio::sync::mpsc::Receiver<DeviceEvent>> {
        let conn = self.get_conn().await?;
        let manager = UDisks2ObjectManagerProxy::new(&conn).await?;
        let mut added_stream = manager.receive_interfaces_added().await?;
        let mut removed_stream = manager.receive_interfaces_removed().await?;

        let (tx, rx) = tokio::sync::mpsc::channel(64);

        let tx_added = tx.clone();
        tokio::spawn(async move {
            while let Some(signal) = added_stream.next().await {
                if let Ok(args) = signal.args() {
                    let ev = DeviceEvent::Added(args.object_path.as_str().to_string());
                    if tx_added.send(ev).await.is_err() {
                        break;
                    }
                }
            }
        });

        tokio::spawn(async move {
            while let Some(signal) = removed_stream.next().await {
                if let Ok(args) = signal.args() {
                    let ev = DeviceEvent::Removed(args.object_path.as_str().to_string());
                    if tx.send(ev).await.is_err() {
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }
}

fn get_str_prop(props: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    props.get(key).and_then(|v| match &**v {
        Value::Str(s) => Some(s.as_str().to_string()),
        _ => None,
    })
}

fn get_bool_prop(props: &HashMap<String, OwnedValue>, key: &str) -> bool {
    props.get(key).and_then(|v| match &**v {
        Value::Bool(b) => Some(*b),
        _ => None,
    }).unwrap_or(false)
}

fn get_u64_prop(props: &HashMap<String, OwnedValue>, key: &str) -> u64 {
    props.get(key).and_then(|v| match &**v {
        Value::U64(n) => Some(*n),
        _ => None,
    }).unwrap_or(0)
}

fn get_object_path_prop(props: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    props.get(key).and_then(|v| match &**v {
        Value::ObjectPath(p) => Some(p.as_str().to_string()),
        _ => None,
    })
}

/// Helper extracting filesystem mount paths from a `UDisks2` `MountPoints` variant (`aay` or `as`).
fn parse_mount_points(val: &OwnedValue) -> Vec<PathBuf> {
    let mut results = Vec::new();
    if let Value::Array(outer_arr) = &**val {
        for elem in outer_arr.iter() {
            match elem {
                Value::Array(inner_arr) => {
                    let mut bytes = Vec::with_capacity(inner_arr.len());
                    for b in inner_arr.iter() {
                        if let Value::U8(byte_val) = b {
                            bytes.push(*byte_val);
                        }
                    }
                    if bytes.last() == Some(&0) {
                        bytes.pop();
                    }
                    if !bytes.is_empty() {
                        results.push(PathBuf::from(std::ffi::OsString::from_vec(bytes)));
                    }
                }
                Value::Str(s) => {
                    let path_str = s.as_str();
                    if !path_str.is_empty() {
                        results.push(PathBuf::from(path_str));
                    }
                }
                _ => {}
            }
        }
    }
    results
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_drive_connection_type_parsing() {
        assert_eq!(DriveConnectionType::from_bus_str("usb"), DriveConnectionType::Usb);
        assert_eq!(DriveConnectionType::from_bus_str("USB"), DriveConnectionType::Usb);
        assert_eq!(DriveConnectionType::from_bus_str("sata"), DriveConnectionType::Sata);
        assert_eq!(DriveConnectionType::from_bus_str("ata"), DriveConnectionType::Sata);
        assert_eq!(DriveConnectionType::from_bus_str("scsi"), DriveConnectionType::Sata);
        assert_eq!(DriveConnectionType::from_bus_str("nvme"), DriveConnectionType::Nvme);
        assert_eq!(DriveConnectionType::from_bus_str("sdio"), DriveConnectionType::Sdio);
        assert_eq!(DriveConnectionType::from_bus_str("virtual"), DriveConnectionType::Other);
    }

    #[test]
    fn test_parse_mount_points_byte_arrays() {
        let mount1 = b"/mnt/usb\0";
        let mount2 = b"/media/external\0";
        let raw = vec![mount1.to_vec(), mount2.to_vec()];
        let val = OwnedValue::try_from(Value::from(raw)).unwrap();

        let parsed = parse_mount_points(&val);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0], PathBuf::from("/mnt/usb"));
        assert_eq!(parsed[1], PathBuf::from("/media/external"));
    }

    #[test]
    fn test_mounted_volume_serde() {
        let vol = MountedVolume {
            id: "/org/freedesktop/UDisks2/block_devices/sdb1".to_string(),
            label: "BackupDrive".to_string(),
            mount_path: PathBuf::from("/media/backup"),
            size_bytes: 1_000_000_000,
            connection_type: DriveConnectionType::Usb,
            is_removable: true,
            drive_path: Some("/org/freedesktop/UDisks2/drives/SANDISK_123".to_string()),
        };

        let json = serde_json::to_string(&vol).unwrap();
        let deserialized: MountedVolume = serde_json::from_str(&json).unwrap();
        assert_eq!(vol, deserialized);
    }

    #[tokio::test]
    async fn test_udisks2_monitor_query_or_graceful_missing() {
        let monitor = UDisks2Monitor::new().with_timeout(Duration::from_millis(500));
        let res = monitor.query_mounted_volumes().await;
        // On real Linux system with UDisks2 running, this returns mounted devices!
        // In headless environment without UDisks2, it returns a typed IpcError gracefully.
        match res {
            Ok(vols) => {
                for v in vols {
                    assert_ne!(v.id, "");
                    assert!(v.mount_path.is_absolute());
                }
            }
            Err(SharedError::Ipc(err)) => {
                assert!(matches!(
                    err,
                    IpcError::Zbus(_) | IpcError::ServiceUnavailable(_) | IpcError::Timeout { .. }
                ));
            }
            Err(other) => panic!("Unexpected error: {other:?}"),
        }
    }
}
