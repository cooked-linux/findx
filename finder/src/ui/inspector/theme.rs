//! CSS styling and theme definitions for the Get Info Inspector window.

/// CSS stylesheet for the Get Info property inspector window and components.
pub const INSPECTOR_CSS: &str = r"
/* Get Info Dialog */
.inspector-window {
    min-width: 380px;
}

.inspector-header-card {
    padding: 16px;
    background: transparent;
    border-bottom: 1px solid alpha(var(--border-color, #ffffff), 0.08);
}

.inspector-icon-button {
    min-width: 64px;
    min-height: 64px;
    border-radius: 12px;
    padding: 0;
    border: 2px solid transparent;
    transition: border-color 150ms ease;
}

.inspector-icon-button:focus {
    border-color: var(--accent-color, #2f6df6);
    box-shadow: 0 0 0 3px rgba(47, 109, 246, 0.3);
}

.inspector-title {
    font-weight: 700;
    font-size: 15px;
}

.inspector-subtitle {
    font-size: 12px;
    color: alpha(var(--window-fg-color, #f0f0f4), 0.65);
}

.inspector-padlock {
    opacity: 0.7;
}
";

/// Installs the Inspector CSS stylesheet onto the default display.
pub fn install_inspector_theme() {
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(INSPECTOR_CSS);
    if let Some(display) = gdk4::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inspector_css_contains_required_classes() {
        assert!(INSPECTOR_CSS.contains(".inspector-window"));
        assert!(INSPECTOR_CSS.contains(".inspector-header-card"));
        assert!(INSPECTOR_CSS.contains(".inspector-icon-button"));
        assert!(INSPECTOR_CSS.contains(".inspector-title"));
        assert!(INSPECTOR_CSS.contains(".inspector-subtitle"));
        assert!(INSPECTOR_CSS.contains(".inspector-padlock"));
    }
}
