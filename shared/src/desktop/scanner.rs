//! Desktop entry directory scanner and precedence masking.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use super::{DesktopApp, DesktopError};

/// Global in-memory cache for scanned desktop applications.
static APP_CACHE: RwLock<Option<Vec<DesktopApp>>> = RwLock::new(None);

/// Returns the standard XDG application search directories in strict order of precedence:
/// 1. User local: `~/.local/share/applications/`
/// 2. Flatpak user exports: `~/.local/share/flatpak/exports/share/applications/`
/// 3. Flatpak system exports: `/var/lib/flatpak/exports/share/applications/`
/// 4. Snap applications: `/var/lib/snapd/desktop/applications/`
/// 5. Local administrator: `/usr/local/share/applications/`
/// 6. System distribution: `/usr/share/applications/`
#[must_use]
pub fn default_application_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::with_capacity(6);

    let data_home = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(|| {
            std::env::var("HOME")
                .map(|h| PathBuf::from(h).join(".local/share"))
                .ok()
        });

    if let Some(ref home_share) = data_home {
        // 1. User local
        dirs.push(home_share.join("applications"));
        // 2. Flatpak user exports
        dirs.push(
            home_share
                .parent()
                .unwrap_or(home_share)
                .join("flatpak/exports/share/applications"),
        );
    }

    // 3. Flatpak system exports
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    // 4. Snap applications
    dirs.push(PathBuf::from("/var/lib/snapd/desktop/applications"));
    // 5. Local administrator
    dirs.push(PathBuf::from("/usr/local/share/applications"));
    // 6. System distribution
    dirs.push(PathBuf::from("/usr/share/applications"));

    dirs
}

/// Clears the global desktop application cache, forcing the next scan to re-read from disk.
pub fn clear_app_cache() {
    let mut guard = match APP_CACHE.write() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = None;
}

/// Scans standard XDG application directories and returns indexed applications.
/// Results are cached in memory for subsequent calls.
///
/// # Errors
///
/// Returns [`DesktopError::Io`] if an unrecoverable I/O error occurs.
pub fn scan_installed_apps() -> Result<Vec<DesktopApp>, DesktopError> {
    {
        let guard = match APP_CACHE.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(ref cached) = *guard {
            return Ok(cached.clone());
        }
    }

    let dirs = default_application_dirs();
    let current_desktop = std::env::var("XDG_CURRENT_DESKTOP").ok();
    let locale = super::entry::get_system_locale();

    let apps = scan_application_dirs(&dirs, current_desktop.as_deref(), locale.as_deref())?;

    let mut guard = match APP_CACHE.write() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = Some(apps.clone());
    drop(guard);

    Ok(apps)
}

/// Scans specified directories in precedence order with desktop ID masking and visibility filtering.
///
/// # Errors
///
/// Returns [`DesktopError::Io`] if directory traversal fails critically.
pub fn scan_application_dirs(
    dirs: &[PathBuf],
    current_desktop: Option<&str>,
    locale: Option<&str>,
) -> Result<Vec<DesktopApp>, DesktopError> {
    let mut apps = Vec::new();
    let mut seen_ids = HashSet::new();

    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }

        let mut entries = Vec::new();
        collect_desktop_files_recursive(dir, dir, &mut entries);

        for (desktop_id, file_path) in entries {
            // Precedence masking: first occurrence of a desktop ID completely shadows any later ones
            if !seen_ids.insert(desktop_id.clone()) {
                continue;
            }

            let content = match std::fs::read_to_string(&file_path) {
                Ok(c) => c,
                Err(err) => {
                    tracing::debug!(
                        path = %file_path.display(),
                        error = %err,
                        "Skipping unreadable desktop entry file"
                    );
                    continue;
                }
            };

            match DesktopApp::parse_content(&content, &file_path, &desktop_id, locale) {
                Ok(app) => {
                    if is_visible_in_desktop(&app, current_desktop) {
                        apps.push(app);
                    }
                }
                Err(DesktopError::Filtered) => {
                    // Filtered by non-application Type or similar
                }
                Err(err) => {
                    tracing::debug!(
                        path = %file_path.display(),
                        error = %err,
                        "Skipping invalid desktop entry file"
                    );
                }
            }
        }
    }

    Ok(apps)
}

/// Checks whether an application is visible in the specified desktop environment session.
#[must_use]
pub fn is_visible_in_desktop(app: &DesktopApp, current_desktop: Option<&str>) -> bool {
    if app.hidden || app.no_display {
        return false;
    }

    let active_desktops: Vec<&str> = current_desktop
        .unwrap_or("")
        .split(':')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    // If OnlyShowIn is set, the active desktop session must match at least one entry
    if !app.only_show_in.is_empty() {
        if active_desktops.is_empty() {
            return false;
        }
        let matches = app.only_show_in.iter().any(|req| {
            active_desktops
                .iter()
                .any(|cur| cur.eq_ignore_ascii_case(req))
        });
        if !matches {
            return false;
        }
    }

    // If NotShowIn is set, the active desktop session must not match any entry
    if !app.not_show_in.is_empty() && !active_desktops.is_empty() {
        let matches = app.not_show_in.iter().any(|denied| {
            active_desktops
                .iter()
                .any(|cur| cur.eq_ignore_ascii_case(denied))
        });
        if matches {
            return false;
        }
    }

    true
}

/// Recursively collects `.desktop` files in `dir` and derives their desktop ID relative to `base_dir`.
fn collect_desktop_files_recursive(
    dir: &Path,
    base_dir: &Path,
    entries: &mut Vec<(String, PathBuf)>,
) {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };

    for entry_res in read_dir {
        let Ok(entry) = entry_res else {
            continue;
        };

        let path = entry.path();
        if path.is_dir() {
            // Avoid symlink loops by not following directory symlinks
            if let Ok(meta) = entry.metadata()
                && meta.is_dir()
            {
                collect_desktop_files_recursive(&path, base_dir, entries);
            }
        } else if path.is_file()
            && let Some(ext) = path.extension()
            && ext == "desktop"
        {
            let desktop_id = path.strip_prefix(base_dir).map_or_else(
                |_| {
                    path.file_name().map_or_else(
                        || "unknown.desktop".to_string(),
                        |n| n.to_string_lossy().into_owned(),
                    )
                },
                |rel| {
                    let mut parts = Vec::new();
                    for comp in rel.components() {
                        parts.push(comp.as_os_str().to_string_lossy());
                    }
                    parts.join("-")
                },
            );

            entries.push((desktop_id, path));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_precedence_masking_higher_priority_wins() {
        let dir_user = tempdir().expect("tempdir");
        let dir_sys = tempdir().expect("tempdir");

        let app_user = dir_user.path().join("calculator.desktop");
        let mut f1 = File::create(&app_user).expect("create");
        writeln!(
            f1,
            "[Desktop Entry]\nType=Application\nName=User Calculator\nExec=calc-user"
        )
        .expect("write");

        let app_sys = dir_sys.path().join("calculator.desktop");
        let mut f2 = File::create(&app_sys).expect("create");
        writeln!(
            f2,
            "[Desktop Entry]\nType=Application\nName=System Calculator\nExec=calc-sys"
        )
        .expect("write");

        let dirs = vec![dir_user.path().to_path_buf(), dir_sys.path().to_path_buf()];
        let apps = scan_application_dirs(&dirs, None, None).expect("scan");

        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "User Calculator");
        assert_eq!(apps[0].desktop_id, "calculator.desktop");
    }

    #[test]
    fn test_precedence_masking_hidden_masks_lower() {
        let dir_user = tempdir().expect("tempdir");
        let dir_sys = tempdir().expect("tempdir");

        let app_user = dir_user.path().join("calculator.desktop");
        let mut f1 = File::create(&app_user).expect("create");
        writeln!(
            f1,
            "[Desktop Entry]\nType=Application\nName=User Calc\nExec=calc\nHidden=true"
        )
        .expect("write");

        let app_sys = dir_sys.path().join("calculator.desktop");
        let mut f2 = File::create(&app_sys).expect("create");
        writeln!(
            f2,
            "[Desktop Entry]\nType=Application\nName=System Calc\nExec=calc"
        )
        .expect("write");

        let dirs = vec![dir_user.path().to_path_buf(), dir_sys.path().to_path_buf()];
        let apps = scan_application_dirs(&dirs, None, None).expect("scan");

        // The hidden entry in user directory completely masks the system entry, so 0 apps returned
        assert_eq!(apps.len(), 0);
    }

    #[test]
    fn test_visibility_filtering() {
        let dir = tempdir().expect("tempdir");

        let p1 = dir.path().join("gnome-only.desktop");
        let mut f1 = File::create(&p1).expect("create");
        writeln!(
            f1,
            "[Desktop Entry]\nType=Application\nName=GNOME App\nExec=gnome-app\nOnlyShowIn=GNOME;"
        )
        .expect("write");

        let p2 = dir.path().join("no-kde.desktop");
        let mut f2 = File::create(&p2).expect("create");
        writeln!(
            f2,
            "[Desktop Entry]\nType=Application\nName=Non-KDE App\nExec=app\nNotShowIn=KDE;"
        )
        .expect("write");

        let p3 = dir.path().join("nodisplay.desktop");
        let mut f3 = File::create(&p3).expect("create");
        writeln!(
            f3,
            "[Desktop Entry]\nType=Application\nName=Hidden App\nExec=app\nNoDisplay=true"
        )
        .expect("write");

        let dirs = vec![dir.path().to_path_buf()];

        // Test in GNOME session: gnome-only and no-kde should show
        let apps_gnome = scan_application_dirs(&dirs, Some("ubuntu:GNOME"), None).expect("scan");
        let names_gnome: Vec<&str> = apps_gnome.iter().map(|a| a.name.as_str()).collect();
        assert!(names_gnome.contains(&"GNOME App"));
        assert!(names_gnome.contains(&"Non-KDE App"));
        assert!(!names_gnome.contains(&"Hidden App"));

        // Test in KDE session: gnome-only and no-kde should both be excluded
        let apps_kde = scan_application_dirs(&dirs, Some("KDE"), None).expect("scan");
        assert_eq!(apps_kde.len(), 0);
    }

    #[test]
    fn test_subdirectory_desktop_id() {
        let dir = tempdir().expect("tempdir");
        let sub = dir.path().join("kde4");
        std::fs::create_dir(&sub).expect("create sub");

        let app_path = sub.join("konsole.desktop");
        let mut f = File::create(&app_path).expect("create");
        writeln!(
            f,
            "[Desktop Entry]\nType=Application\nName=Konsole\nExec=konsole"
        )
        .expect("write");

        let dirs = vec![dir.path().to_path_buf()];
        let apps = scan_application_dirs(&dirs, None, None).expect("scan");

        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].desktop_id, "kde4-konsole.desktop");
    }
}
