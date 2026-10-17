//! Contextual "Open With" candidate application aggregation, intersection, and ranking.
//!
//! Evaluates candidate desktop applications for single or multiple selected files.
//! Employs intersection filtering for heterogeneous MIME types and ranks the
//! system/user default application first, followed alphabetically by display name.

use crate::desktop::DesktopApp;

use super::MimeError;
use super::apps::get_default_app_for_mime;

/// Presentation and invocation model for an application registered to handle a MIME type.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AppHandlerInfo {
    /// Desktop entry ID (e.g. `"org.gnome.TextEditor.desktop"`).
    pub desktop_id: String,
    /// Localized or display name of the application (e.g. `"Text Editor"`).
    pub display_name: String,
    /// Command line invocation string.
    pub exec_command: String,
    /// Icon theme name or path, if specified.
    pub icon_name: Option<String>,
    /// Whether this application is the configured default for the target MIME type(s).
    pub is_default: bool,
}

impl AppHandlerInfo {
    /// Constructs an [`AppHandlerInfo`] from a [`DesktopApp`].
    #[must_use]
    pub fn from_desktop_app(app: &DesktopApp, is_default: bool) -> Self {
        Self {
            desktop_id: app.desktop_id.clone(),
            display_name: app.name.clone(),
            exec_command: if app.exec_raw.is_empty() {
                app.exec_sanitized.clone()
            } else {
                app.exec_raw.clone()
            },
            icon_name: app.icon_name.clone(),
            is_default,
        }
    }
}

/// Computes "Open With" candidate applications from a given list of installed applications.
///
/// If `mime_types` has multiple distinct entries, only applications supporting *all*
/// specified MIME types are returned (heterogeneous intersection).
/// The default application is sorted first, followed alphabetically by `display_name`.
#[must_use]
pub fn get_open_with_candidates_from_apps(
    mime_types: &[&str],
    apps: &[DesktopApp],
    default_id: Option<&str>,
) -> Vec<AppHandlerInfo> {
    let mut distinct_mimes: Vec<String> = Vec::new();
    for m in mime_types {
        let trimmed = m.trim().to_ascii_lowercase();
        if !trimmed.is_empty() && !distinct_mimes.contains(&trimmed) {
            distinct_mimes.push(trimmed);
        }
    }

    if distinct_mimes.is_empty() {
        return Vec::new();
    }

    let mut candidates = Vec::new();

    for app in apps {
        if app.hidden || app.no_display {
            continue;
        }

        // Check if app supports all requested MIME types
        let supports_all = distinct_mimes.iter().all(|target_mime| {
            app.mime_types
                .iter()
                .any(|app_mime| app_mime.eq_ignore_ascii_case(target_mime))
        });

        let is_explicit_default =
            default_id.is_some_and(|def| def.eq_ignore_ascii_case(&app.desktop_id));

        // Include if it supports all requested MIME types or is explicitly configured default
        if supports_all || (distinct_mimes.len() == 1 && is_explicit_default) {
            let is_default = is_explicit_default;
            candidates.push(AppHandlerInfo::from_desktop_app(app, is_default));
        }
    }

    // Sort: default application first, then alphabetically by display_name
    candidates.sort_by(|a, b| match (a.is_default, b.is_default) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a
            .display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
            .then_with(|| a.desktop_id.cmp(&b.desktop_id)),
    });

    candidates
}

/// Resolves "Open With" candidate applications for a set of MIME types.
///
/// Automatically inspects installed desktop applications and queries standard
/// XDG default association configurations.
///
/// # Errors
///
/// Returns [`MimeError::Io`] or [`MimeError::InvalidConfig`] if desktop scanning
/// or configuration reading fails.
pub fn get_open_with_candidates(mime_types: &[&str]) -> Result<Vec<AppHandlerInfo>, MimeError> {
    if mime_types.is_empty() {
        return Ok(Vec::new());
    }

    let apps = match crate::desktop::scan_installed_apps() {
        Ok(a) => a,
        Err(crate::desktop::DesktopError::Io(e)) => return Err(MimeError::Io(e)),
        Err(_) => Vec::new(),
    };

    let default_id = if mime_types.len() == 1 {
        get_default_app_for_mime(mime_types[0])?
    } else {
        // Multi-file selection: check if there is a common default across all types
        let mut common_default: Option<String> = None;
        let mut first = true;

        for m in mime_types {
            let def = get_default_app_for_mime(m)?;
            if first {
                common_default = def;
                first = false;
            } else if common_default != def {
                common_default = None;
                break;
            }
        }
        common_default
    };

    Ok(get_open_with_candidates_from_apps(
        mime_types,
        &apps,
        default_id.as_deref(),
    ))
}

/// Resolves "Open With" candidate applications for a single MIME type.
///
/// # Errors
///
/// Returns [`MimeError::Io`] or [`MimeError::InvalidConfig`] if resolution fails.
pub fn get_open_with_candidates_for_mime(
    mime_type: &str,
) -> Result<Vec<AppHandlerInfo>, MimeError> {
    get_open_with_candidates(&[mime_type])
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_mock_app(id: &str, name: &str, exec: &str, mimes: &[&str]) -> DesktopApp {
        DesktopApp {
            desktop_id: id.to_string(),
            path: PathBuf::from(format!("/usr/share/applications/{id}")),
            name: name.to_string(),
            generic_name: None,
            comment: None,
            exec_raw: exec.to_string(),
            exec_sanitized: exec.split_whitespace().next().unwrap_or("").to_string(),
            icon_name: Some(id.to_string()),
            keywords: Vec::new(),
            categories: Vec::new(),
            mime_types: mimes.iter().map(|m| (*m).to_string()).collect(),
            is_terminal: false,
            no_display: false,
            hidden: false,
            only_show_in: Vec::new(),
            not_show_in: Vec::new(),
        }
    }

    #[test]
    fn test_candidate_ranking_default_first_then_alphabetical() {
        let app_a = create_mock_app("a.desktop", "Alpha Editor", "alpha %u", &["text/plain"]);
        let app_b = create_mock_app("b.desktop", "Beta Editor", "beta %u", &["text/plain"]);
        let app_z = create_mock_app("z.desktop", "Zeta Editor", "zeta %u", &["text/plain"]);

        let apps = vec![app_a, app_b, app_z];

        // Case 1: Zeta is default -> should be ranked first, then Alpha, Beta
        let candidates =
            get_open_with_candidates_from_apps(&["text/plain"], &apps, Some("z.desktop"));
        assert_eq!(candidates.len(), 3);
        assert_eq!(candidates[0].desktop_id, "z.desktop");
        assert!(candidates[0].is_default);
        assert_eq!(candidates[1].desktop_id, "a.desktop");
        assert!(!candidates[1].is_default);
        assert_eq!(candidates[2].desktop_id, "b.desktop");
        assert!(!candidates[2].is_default);

        // Case 2: No default -> should be alphabetical Alpha, Beta, Zeta
        let candidates_no_default =
            get_open_with_candidates_from_apps(&["text/plain"], &apps, None);
        assert_eq!(candidates_no_default.len(), 3);
        assert_eq!(candidates_no_default[0].desktop_id, "a.desktop");
        assert_eq!(candidates_no_default[1].desktop_id, "b.desktop");
        assert_eq!(candidates_no_default[2].desktop_id, "z.desktop");
    }

    #[test]
    fn test_multi_mime_heterogeneous_intersection() {
        let text_only = create_mock_app("text.desktop", "Text App", "text", &["text/plain"]);
        let image_only = create_mock_app("img.desktop", "Img App", "img", &["image/png"]);
        let universal = create_mock_app(
            "universal.desktop",
            "Universal Viewer",
            "universal",
            &["text/plain", "image/png"],
        );

        let apps = vec![text_only, image_only, universal];

        // Selection of [text/plain, image/png] -> only universal app should match
        let candidates = get_open_with_candidates_from_apps(
            &["text/plain", "image/png"],
            &apps,
            Some("universal.desktop"),
        );

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].desktop_id, "universal.desktop");
        assert_eq!(candidates[0].display_name, "Universal Viewer");

        // Selection with no intersecting app -> empty candidates list
        let empty_candidates = get_open_with_candidates_from_apps(
            &["text/plain", "image/png", "application/pdf"],
            &apps,
            None,
        );
        assert_eq!(empty_candidates, []);
    }
}
