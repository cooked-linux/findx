//! Quick Look CSS Styling and Theme Definitions matching macOS Finder aesthetics.

/// CSS stylesheet for Quick Look floating preview window, viewports, and controls.
pub const QUICKLOOK_CSS: &str = r"
/* Quick Look Floating Window */
.quicklook-window {
    background: alpha(var(--window-bg-color, #1e1e24), 0.88);
    border: 1px solid alpha(var(--border-color, #ffffff), 0.12);
    border-radius: 14px;
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.45);
}

.quicklook-window.quicklook-fullscreen {
    border-radius: 0;
    border: none;
    box-shadow: none;
    background: #000000;
}

.quicklook-header {
    background: transparent;
    border-bottom: 1px solid alpha(var(--border-color, #ffffff), 0.08);
    padding: 8px 12px;
}

.quicklook-title {
    font-weight: 600;
    font-size: 13px;
    color: var(--window-fg-color, #f0f0f4);
}

.quicklook-counter {
    font-size: 12px;
    color: alpha(var(--window-fg-color, #f0f0f4), 0.60);
    font-variant-numeric: tabular-nums;
}

.quicklook-viewport-container {
    padding: 0;
    border-radius: 0 0 14px 14px;
}

.quicklook-code-view {
    font-family: monospace;
    font-size: 12px;
    line-height: 1.45;
    background: transparent;
}

.quicklook-mosaic-grid {
    padding: 16px;
    background: var(--popover-bg-color, #24242c);
}

/* Floating Pill Overlays for PDF & Media controls */
.quicklook-pill-overlay {
    background: alpha(#1c1c22, 0.78);
    border: 1px solid alpha(#ffffff, 0.12);
    border-radius: 20px;
    padding: 6px 14px;
    color: #f0f0f4;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.35);
}

/* Code gutter & line numbers */
.quicklook-line-numbers {
    font-family: monospace;
    font-size: 12px;
    line-height: 1.45;
    color: alpha(var(--window-fg-color, #f0f0f4), 0.4);
    padding: 8px 12px 8px 8px;
}

/* Media transport controls bar */
.quicklook-media-transport {
    background: alpha(#1c1c22, 0.85);
    border-radius: 12px;
    padding: 8px 16px;
    margin: 12px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
}

/* Fallback metadata card */
.quicklook-fallback-card {
    padding: 32px;
}

.quicklook-fallback-title {
    font-size: 16px;
    font-weight: 700;
    color: var(--window-fg-color, #f0f0f4);
}

.quicklook-fallback-meta {
    font-size: 13px;
    color: alpha(var(--window-fg-color, #f0f0f4), 0.7);
}

/* Index Sheet Mosaic thumbnails */
.quicklook-mosaic-item {
    border-radius: 8px;
    padding: 6px;
    transition: background-color 150ms ease-in-out;
}

.quicklook-mosaic-item:hover {
    background: alpha(var(--accent-color, #2f6df6), 0.25);
}

.quicklook-mosaic-item.selected {
    background: var(--accent-color, #2f6df6);
    color: #ffffff;
}
";

/// Installs the Quick Look CSS stylesheet onto the default display.
pub fn install_quicklook_theme() {
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(QUICKLOOK_CSS);
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
    fn test_quicklook_css_contains_all_classes() {
        assert!(QUICKLOOK_CSS.contains(".quicklook-window"));
        assert!(QUICKLOOK_CSS.contains(".quicklook-header"));
        assert!(QUICKLOOK_CSS.contains(".quicklook-title"));
        assert!(QUICKLOOK_CSS.contains(".quicklook-counter"));
        assert!(QUICKLOOK_CSS.contains(".quicklook-viewport-container"));
        assert!(QUICKLOOK_CSS.contains(".quicklook-code-view"));
        assert!(QUICKLOOK_CSS.contains(".quicklook-mosaic-grid"));
        assert!(QUICKLOOK_CSS.contains(".quicklook-pill-overlay"));
        assert!(QUICKLOOK_CSS.contains(".quicklook-line-numbers"));
        assert!(QUICKLOOK_CSS.contains(".quicklook-media-transport"));
        assert!(QUICKLOOK_CSS.contains(".quicklook-fallback-card"));
        assert!(QUICKLOOK_CSS.contains(".quicklook-fallback-title"));
        assert!(QUICKLOOK_CSS.contains(".quicklook-fallback-meta"));
        assert!(QUICKLOOK_CSS.contains(".quicklook-mosaic-item"));
    }
}
