//! CSS styling and theme definitions for file tagging, color swatches, pills, and badges.

/// Complete CSS stylesheet for macOS-style file tagging UI components.
pub const TAGGING_CSS: &str = r"
/* Color Swatch Bar */
.color-swatch-bar {
    padding: 6px 12px;
    border-bottom: 1px solid alpha(var(--border-color, #ffffff), 0.08);
}

.color-dot-button {
    min-width: 18px;
    min-height: 18px;
    border-radius: 50%;
    border: 2px solid transparent;
    padding: 0;
    margin: 0 4px;
    transition: transform 120ms ease, border-color 120ms ease;
}

.color-dot-button:hover {
    transform: scale(1.15);
}

.color-dot-button.active {
    border-color: #ffffff;
    box-shadow: 0 0 6px rgba(0, 0, 0, 0.4);
}

.color-dot-button.partial {
    border-color: rgba(255, 255, 255, 0.6);
    border-style: dashed;
    box-shadow: 0 0 4px rgba(0, 0, 0, 0.25);
}

/* Tag Color Classes */
.tag-color-gray   { background-color: #8e8e93; }
.tag-color-green  { background-color: #34c759; }
.tag-color-purple { background-color: #af52de; }
.tag-color-blue   { background-color: #007aff; }
.tag-color-yellow { background-color: #ffcc00; }
.tag-color-red    { background-color: #ff3b30; }
.tag-color-orange { background-color: #ff9500; }

/* Tag Capsule Pills */
.tag-pill {
    border-radius: 12px;
    padding: 2px 8px;
    font-size: 11px;
    font-weight: 500;
    background: alpha(var(--accent-color, #2f6df6), 0.15);
    color: var(--window-fg-color, #f0f0f4);
    margin: 2px;
}

.tag-pill-partial {
    border: 1px dashed alpha(var(--window-fg-color, #f0f0f4), 0.4);
    background: transparent;
}

.tag-pill-remove-btn {
    min-width: 12px;
    min-height: 12px;
    padding: 0;
    margin-left: 4px;
    opacity: 0.6;
}

.tag-pill-remove-btn:hover {
    opacity: 1.0;
}

/* Badges in Column View and List View */
.tag-badge-cluster {
    margin-left: 6px;
}

.tag-badge-dot {
    min-width: 8px;
    min-height: 8px;
    border-radius: 50%;
    margin-right: 2px;
}

.tag-badge-count {
    font-size: 10px;
    font-weight: 600;
    color: var(--mu);
    margin-left: 2px;
}

/* Autocomplete Popover */
.tag-autocomplete-popover {
    padding: 4px;
    min-width: 180px;
}

.tag-autocomplete-row {
    padding: 4px 8px;
    border-radius: 4px;
    font-size: 12px;
}

.tag-autocomplete-row:hover,
.tag-autocomplete-row.selected {
    background-color: var(--ac);
    color: #ffffff;
}

.tag-pill-editor-box {
    padding: 4px;
    border-radius: 8px;
    border: 1px solid var(--ln);
    background-color: var(--ib);
}

.tag-entry-field {
    border: none;
    background: transparent;
    box-shadow: none;
    min-width: 80px;
    font-size: 12px;
    padding: 2px 4px;
}
";

/// Installs the tagging CSS styling provider onto the default display.
pub fn install_tagging_theme() {
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(TAGGING_CSS);
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
    fn test_tagging_css_classes() {
        assert!(TAGGING_CSS.contains(".color-swatch-bar"));
        assert!(TAGGING_CSS.contains(".color-dot-button"));
        assert!(TAGGING_CSS.contains(".color-dot-button.active"));
        assert!(TAGGING_CSS.contains(".color-dot-button.partial"));
        assert!(TAGGING_CSS.contains(".tag-color-gray"));
        assert!(TAGGING_CSS.contains(".tag-color-green"));
        assert!(TAGGING_CSS.contains(".tag-color-purple"));
        assert!(TAGGING_CSS.contains(".tag-color-blue"));
        assert!(TAGGING_CSS.contains(".tag-color-yellow"));
        assert!(TAGGING_CSS.contains(".tag-color-red"));
        assert!(TAGGING_CSS.contains(".tag-color-orange"));
        assert!(TAGGING_CSS.contains(".tag-pill"));
        assert!(TAGGING_CSS.contains(".tag-pill-partial"));
        assert!(TAGGING_CSS.contains(".tag-pill-remove-btn"));
        assert!(TAGGING_CSS.contains(".tag-badge-dot"));
    }
}
