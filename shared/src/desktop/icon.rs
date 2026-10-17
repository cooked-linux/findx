//! Freedesktop icon theme resolution.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// Global in-memory cache for resolved icon paths.
static ICON_CACHE: RwLock<Option<HashMap<String, Option<PathBuf>>>> = RwLock::new(None);

/// Preferred raster and vector icon sizes in search order.
const PREFERRED_SIZES: &[&str] = &[
    "256x256", "128x128", "64x64", "scalable", "512x512", "48x48", "32x32",
];

/// Supported icon file extensions in preference order.
const ICON_EXTENSIONS: &[&str] = &[".png", ".svg", ".xpm"];

/// Standard context subdirectories within an icon theme.
const THEME_CONTEXTS: &[&str] = &[
    "apps",
    "applications",
    "categories",
    "devices",
    "mimetypes",
    "places",
    "status",
];

/// Clears the global icon cache.
pub fn clear_icon_cache() {
    let mut guard = match ICON_CACHE.write() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = None;
}

/// Returns the standard icon search directories according to XDG specifications.
#[must_use]
pub fn default_icon_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    let data_home = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(|| {
            std::env::var("HOME")
                .map(|h| PathBuf::from(h).join(".local/share"))
                .ok()
        });

    if let Some(ref home_share) = data_home {
        dirs.push(home_share.join("icons"));
    }

    dirs.push(PathBuf::from("/usr/local/share/icons"));
    dirs.push(PathBuf::from("/usr/share/icons"));
    dirs.push(PathBuf::from("/usr/share/pixmaps"));

    dirs
}

/// Detects the active desktop icon theme name from environment variables (`GTK_THEME`, `ICON_THEME`).
#[must_use]
pub fn get_active_theme_name() -> Option<String> {
    if let Ok(theme) = std::env::var("ICON_THEME") {
        let trimmed = theme.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    if let Ok(gtk_theme) = std::env::var("GTK_THEME") {
        let base = gtk_theme
            .split(':')
            .next()
            .unwrap_or(&gtk_theme)
            .trim_end_matches("-dark")
            .trim_end_matches("-light")
            .trim();
        if !base.is_empty() {
            return Some(base.to_string());
        }
    }

    None
}

/// Resolves an icon name or path to an absolute image path according to Freedesktop specifications.
///
/// Results are cached in memory for sub-millisecond repeated queries.
#[must_use]
pub fn resolve_icon_path(icon_name: &str) -> Option<PathBuf> {
    let trimmed = icon_name.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Check in-memory cache
    {
        let guard = match ICON_CACHE.read() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(ref cache) = *guard
            && let Some(hit) = cache.get(trimmed)
        {
            return hit.clone();
        }
    }

    let dirs = default_icon_dirs();
    let theme = get_active_theme_name();
    let resolved = resolve_icon_path_with_dirs(trimmed, theme.as_deref(), &dirs);

    let mut guard = match ICON_CACHE.write() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let cache = guard.get_or_insert_with(HashMap::new);
    cache.insert(trimmed.to_string(), resolved.clone());
    drop(guard);

    resolved
}

/// Resolves an icon with explicit icon directories and theme name.
#[must_use]
pub fn resolve_icon_path_with_dirs(
    icon_name: &str,
    user_theme: Option<&str>,
    icon_dirs: &[PathBuf],
) -> Option<PathBuf> {
    let trimmed = icon_name.trim();
    if trimmed.is_empty() {
        return None;
    }

    // 1. Direct absolute path resolution
    let path = Path::new(trimmed);
    if path.is_absolute() {
        if path.is_file() {
            return Some(path.to_path_buf());
        }
        for ext in ICON_EXTENSIONS {
            let candidate = path.with_extension(&ext[1..]);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        return None;
    }

    // Strip existing extension if provided
    let clean_name = strip_icon_extension(trimmed);

    // 2. Search in user theme and fallback themes ("hicolor") across icon dirs
    let mut themes = Vec::with_capacity(2);
    if let Some(t) = user_theme {
        let t = t.trim();
        if !t.is_empty() && !t.eq_ignore_ascii_case("hicolor") {
            themes.push(t);
        }
    }
    themes.push("hicolor");

    for theme in themes {
        for dir in icon_dirs {
            let theme_dir = dir.join(theme);
            if !theme_dir.is_dir() {
                continue;
            }

            for size in PREFERRED_SIZES {
                let size_dir = theme_dir.join(size);
                if !size_dir.is_dir() {
                    continue;
                }

                for context in THEME_CONTEXTS {
                    let context_dir = size_dir.join(context);
                    if let Some(found) = find_icon_file(&context_dir, clean_name, trimmed) {
                        return Some(found);
                    }
                }

                // Also check directly under size_dir
                if let Some(found) = find_icon_file(&size_dir, clean_name, trimmed) {
                    return Some(found);
                }
            }
        }
    }

    // 3. Fallback to /usr/share/pixmaps or root of icon directories
    for dir in icon_dirs {
        if let Some(found) = find_icon_file(dir, clean_name, trimmed) {
            return Some(found);
        }
    }

    None
}

/// Checks if an icon exists in `dir` matching `clean_name` (with extensions) or exact `raw_name`.
fn find_icon_file(dir: &Path, clean_name: &str, raw_name: &str) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }

    // Try exact raw name first
    let exact = dir.join(raw_name);
    if exact.is_file() {
        return Some(exact);
    }

    // Try preferred extensions
    for ext in ICON_EXTENSIONS {
        let candidate = dir.join(format!("{clean_name}{ext}"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

fn strip_icon_extension(name: &str) -> &str {
    for ext in ICON_EXTENSIONS {
        if let Some(stripped) = name.strip_suffix(ext) {
            return stripped;
        }
    }
    name
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::fs::{File, create_dir_all};
    use tempfile::tempdir;

    #[test]
    fn test_absolute_path_resolution() {
        let dir = tempdir().expect("tempdir");
        let icon_file = dir.path().join("custom-icon.png");
        File::create(&icon_file).expect("create file");

        let resolved = resolve_icon_path_with_dirs(icon_file.to_str().expect("str"), None, &[]);
        assert_eq!(resolved, Some(icon_file));
    }

    #[test]
    fn test_theme_hierarchy_and_size_precedence() {
        let dir = tempdir().expect("tempdir");

        // Create hicolor 64x64 and 256x256 icons
        let icon_64 = dir.path().join("hicolor/64x64/apps/calculator.png");
        let icon_256 = dir.path().join("hicolor/256x256/apps/calculator.png");

        create_dir_all(icon_64.parent().expect("parent")).expect("dir");
        create_dir_all(icon_256.parent().expect("parent")).expect("dir");

        File::create(&icon_64).expect("create");
        File::create(&icon_256).expect("create");

        let dirs = vec![dir.path().to_path_buf()];

        // 256x256 has higher priority than 64x64
        let resolved = resolve_icon_path_with_dirs("calculator", None, &dirs);
        assert_eq!(resolved, Some(icon_256));
    }

    #[test]
    fn test_user_theme_overrides_hicolor() {
        let dir = tempdir().expect("tempdir");

        let user_icon = dir.path().join("Yaru/256x256/apps/gimp.png");
        let hicolor_icon = dir.path().join("hicolor/256x256/apps/gimp.png");

        create_dir_all(user_icon.parent().expect("parent")).expect("dir");
        create_dir_all(hicolor_icon.parent().expect("parent")).expect("dir");

        File::create(&user_icon).expect("create");
        File::create(&hicolor_icon).expect("create");

        let dirs = vec![dir.path().to_path_buf()];

        let resolved = resolve_icon_path_with_dirs("gimp", Some("Yaru"), &dirs);
        assert_eq!(resolved, Some(user_icon));
    }

    #[test]
    fn test_pixmaps_fallback() {
        let dir = tempdir().expect("tempdir");
        let pixmaps = dir.path().join("pixmaps");
        create_dir_all(&pixmaps).expect("dir");

        let icon = pixmaps.join("legacy-app.xpm");
        File::create(&icon).expect("create");

        let dirs = vec![pixmaps];
        let resolved = resolve_icon_path_with_dirs("legacy-app", None, &dirs);
        assert_eq!(resolved, Some(icon));
    }

    #[test]
    fn test_nonexistent_icon_returns_none() {
        let dirs = vec![PathBuf::from("/tmp/nonexistent-icons")];
        let resolved = resolve_icon_path_with_dirs("no-such-icon-12345", None, &dirs);
        assert!(resolved.is_none());
    }
}
