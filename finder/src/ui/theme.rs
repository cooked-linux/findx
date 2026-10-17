//! CSS Theme and styling system for Finder window and header bar.

use gtk4::prelude::*;

/// Complete CSS design system stylesheet adhering to macOS Finder styling.
pub const FINDER_CSS: &str = r"
/* Light Theme Palette */
.finder-light {
    --wp: linear-gradient(135deg, #7fb2ff, #c9a7ff 50%, #ffd2a1);
    --g: rgba(247, 247, 249, 0.85);
    --sb: rgba(232, 233, 238, 0.65);
    --tx: #1c1c20;
    --mu: #7a7a85;
    --ln: rgba(0, 0, 0, 0.09);
    --ac: #2f6df6;
    --hv: rgba(0, 0, 0, 0.05);
    --cd: rgba(255, 255, 255, 0.7);
    --ib: rgba(0, 0, 0, 0.06);
    --sp: rgba(255, 255, 255, 0.7);
}

/* Dark Theme Palette */
.finder-dark {
    --wp: linear-gradient(135deg, #0f1b3d, #3b1d6b 55%, #7a3b2e);
    --g: rgba(34, 35, 41, 0.88);
    --sb: rgba(22, 23, 28, 0.6);
    --tx: #ececf1;
    --mu: #9a9aa8;
    --ln: rgba(255, 255, 255, 0.09);
    --ac: #4d86ff;
    --hv: rgba(255, 255, 255, 0.07);
    --cd: rgba(255, 255, 255, 0.06);
    --ib: rgba(255, 255, 255, 0.08);
    --sp: rgba(255, 255, 255, 0.16);
}

/* Finder Window Container */
.finder-window {
    border-radius: 16px;
    background-color: var(--g);
    border: 1px solid var(--sp);
    box-shadow: 0 2px 4px rgba(0,0,0,0.12), 0 12px 28px rgba(0,0,0,0.2), 0 40px 80px rgba(0,0,0,0.3);
    color: var(--tx);
    font-family: system-ui, -apple-system, sans-serif;
}

/* Header Bar (52px height) */
.finder-headerbar {
    min-height: 52px;
    height: 52px;
    background-color: var(--sb);
    border-bottom: 1px solid var(--ln);
    padding: 0 14px;
}

/* Navigation Back/Forward Buttons */
.nav-group {
    border-radius: 7px;
    background-color: var(--ib);
    padding: 2px;
}
.nav-btn {
    padding: 4px 10px;
    font-weight: 500;
    color: var(--mu);
    font-size: 13px;
}
.nav-btn:hover {
    background-color: var(--hv);
    color: var(--tx);
}
.nav-btn:disabled {
    opacity: 0.35;
}

/* View Switcher Segmented Control */
.segmented-control {
    border-radius: 7px;
    background-color: var(--ib);
    padding: 2px;
}
.segmented-btn {
    padding: 4px 10px;
    font-weight: 500;
    color: var(--mu);
    font-size: 12px;
    border-radius: 5px;
}
.segmented-btn.active {
    background-color: var(--ac);
    color: #ffffff;
}

/* Action Buttons */
.action-btn {
    padding: 4px 9px;
    border-radius: 7px;
    background-color: var(--ib);
    color: var(--mu);
    font-size: 12px;
}
.action-btn:hover {
    background-color: var(--hv);
    color: var(--tx);
}
.action-btn.active {
    background-color: var(--hv);
    color: var(--ac);
}

/* Search Box */
.finder-search {
    min-width: 150px;
    padding: 5px 10px;
    border-radius: 8px;
    background-color: var(--ib);
    color: var(--tx);
    border: 1px solid transparent;
    transition: min-width 150ms ease-in-out, border-color 150ms;
}
.finder-search:focus {
    min-width: 230px;
    border-color: var(--ac);
}

/* Path bar / Breadcrumbs Dock (28px height) */
.finder-breadcrumbs {
    min-height: 28px;
    height: 28px;
    background-color: var(--sb);
    border-top: 1px solid var(--ln);
    padding: 0 14px;
    color: var(--mu);
    font-size: 12px;
}
";

/// Installs the Finder CSS stylesheet onto the default display.
pub fn install_finder_theme() {
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(FINDER_CSS);
    if let Some(display) = gdk4::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// Applies `.finder-dark` or `.finder-light` classes to the widget based on `is_dark`.
pub fn apply_theme_classes<W: IsA<gtk4::Widget>>(widget: &W, is_dark: bool) {
    if is_dark {
        widget.remove_css_class("finder-light");
        widget.add_css_class("finder-dark");
    } else {
        widget.remove_css_class("finder-dark");
        widget.add_css_class("finder-light");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_finder_css_contains_core_classes() {
        assert!(FINDER_CSS.contains(".finder-light"));
        assert!(FINDER_CSS.contains(".finder-dark"));
        assert!(FINDER_CSS.contains(".finder-window"));
        assert!(FINDER_CSS.contains(".finder-headerbar"));
        assert!(FINDER_CSS.contains(".nav-group"));
        assert!(FINDER_CSS.contains(".nav-btn"));
        assert!(FINDER_CSS.contains(".segmented-control"));
        assert!(FINDER_CSS.contains(".segmented-btn"));
        assert!(FINDER_CSS.contains(".action-btn"));
        assert!(FINDER_CSS.contains(".finder-search"));
        assert!(FINDER_CSS.contains(".finder-breadcrumbs"));
    }
}
