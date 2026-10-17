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

/* Sidebar Container (210px default width) */
.finder-sidebar {
    min-width: 160px;
    background-color: var(--sb);
    border-right: 1px solid var(--ln);
}

.sd {
    width: 210px;
    background-color: var(--sb);
    border-right: 1px solid var(--ln);
}

/* Sidebar Sections & Headers */
.sh-box {
    padding: 10px 8px 4px 8px;
}
.sh-title {
    font-size: 11px;
    font-weight: 700;
    color: var(--mu);
    letter-spacing: 0.6px;
    text-transform: uppercase;
}
.sh-toggle {
    font-size: 11px;
    color: var(--mu);
    padding: 2px 6px;
    border-radius: 4px;
    opacity: 0;
    transition: opacity 120ms ease;
    background-color: transparent;
}
.sh-box:hover .sh-toggle {
    opacity: 1;
}
.sh-toggle:hover {
    color: var(--tx);
    background-color: var(--hv);
}

/* Sidebar Item Rows */
.si {
    padding: 5px 8px;
    border-radius: 7px;
    color: var(--tx);
    font-size: 13px;
    margin: 1px 4px;
    transition: background-color 100ms ease;
}
.si:hover {
    background-color: var(--hv);
}
.si.on {
    background-color: var(--ac);
    color: #ffffff;
}
.si.on label {
    color: #ffffff;
}

/* Item Glyphs / Icons */
.gl {
    min-width: 13px;
    min-height: 13px;
    width: 13px;
    height: 13px;
    opacity: 0.75;
}
.si.on .gl {
    color: #ffffff;
}

/* Inline Eject Button */
.si-eject-btn {
    padding: 0 4px;
    min-width: 18px;
    min-height: 18px;
    border-radius: 4px;
    color: var(--mu);
    font-size: 12px;
    background-color: transparent;
}
.si-eject-btn:hover {
    background-color: var(--hv);
    color: var(--tx);
}
.si.on .si-eject-btn {
    color: #ffffff;
}

/* macOS Tag Color Dots */
.tg {
    min-width: 9px;
    min-height: 9px;
    width: 9px;
    height: 9px;
    border-radius: 50%;
    border: none;
    padding: 0;
    margin: 0;
    transition: transform 100ms ease;
}
.tg:hover {
    transform: scale(1.25);
}
.tg.red { background-color: #ff5f57; }
.tg.orange { background-color: #ff9f0a; }
.tg.yellow { background-color: #ffd60a; }
.tg.green { background-color: #30d158; }
.tg.blue { background-color: #0a84ff; }
.tg.purple { background-color: #bf5af2; }
.tg.gray { background-color: #98989d; }
.tg.default { background-color: #98989d; }
.tags-dots-row {
    padding: 4px 10px 8px 10px;
    gap: 7px;
}

/* Drag and Drop Hover Indicators */
.si.drop-target {
    border-top: 2px solid var(--ac);
}

/* Sticky Section Group Headers */
.group-header {
    padding: 6px 14px;
    min-height: 28px;
    background-color: var(--sb);
    border-bottom: 1px solid var(--ln);
    color: var(--tx);
    cursor: pointer;
    transition: background-color 100ms ease;
}
.group-header:hover {
    background-color: var(--hv);
}
.group-chevron {
    font-size: 11px;
    color: var(--mu);
    min-width: 14px;
}
.group-title {
    font-size: 12px;
    font-weight: 700;
    color: var(--tx);
}
/* Hierarchical List View (Nested list) */
.hierarchical-list-view {
    background-color: var(--g);
}
.hd {
    min-height: 28px;
    height: 28px;
    background-color: var(--sb);
    border-bottom: 1px solid var(--ln);
    padding: 0 8px;
}
.hd-cell {
    font-size: 11px;
    font-weight: 600;
    color: var(--mu);
    padding: 4px 6px;
    border-radius: 4px;
    background-color: transparent;
    transition: background-color 100ms ease, color 100ms ease;
}
.hd-cell:hover {
    background-color: var(--hv);
    color: var(--tx);
}
.hd-sort-arrow {
    font-size: 9px;
    margin-left: 4px;
    color: var(--ac);
}
.list-scrolled-window {
    background-color: var(--g);
}
.r {
    min-height: 26px;
    height: 26px;
    padding: 0 8px;
    border-radius: 6px;
    color: var(--tx);
    font-size: 13px;
    margin: 1px 2px;
    transition: background-color 100ms ease;
}
.r:hover {
    background-color: var(--hv);
}
.r.on {
    background-color: var(--ac);
    color: #ffffff;
}
.r.on label {
    color: #ffffff;
}
.r.on .disclosure-chevron {
    color: #ffffff;
}
.r.on .column-icon {
    color: #ffffff;
}
.r.on .dc-date,
.r.on .dc-size,
.r.on .dc-kind {
    color: rgba(255, 255, 255, 0.85);
}
.dc {
    padding: 0 4px;
}
.dc-name {
    min-width: 200px;
    width: 320px;
}
.dc-date {
    width: 150px;
    color: var(--mu);
    font-size: 12px;
}
.dc-size {
    width: 140px;
    color: var(--mu);
    font-size: 12px;
}
.dc-kind {
    width: 120px;
    color: var(--mu);
    font-size: 12px;
}
.dc-tags {
    width: 80px;
}
.disclosure-chevron {
    font-size: 11px;
    color: var(--mu);
    min-width: 16px;
    padding: 2px;
    border-radius: 4px;
    cursor: pointer;
    background-color: transparent;
}
.disclosure-chevron:hover {
    background-color: var(--hv);
    color: var(--tx);
}
.disclosure-spinner {
    min-width: 16px;
    min-height: 16px;
}

/* Miller Columns Navigation View */
.column-scrolled-window {
    background-color: var(--g);
    border-right: 1px solid var(--ln);
}
.cl {
    min-width: 160px;
    background-color: var(--g);
    padding: 4px;
}
.rw {
    min-height: 26px;
    height: 26px;
    padding: 0 8px;
    border-radius: 6px;
    color: var(--tx);
    font-size: 13px;
    margin: 1px 2px;
    transition: background-color 100ms ease;
}
.rw:hover {
    background-color: var(--hv);
}
.rw.on {
    background-color: var(--ac);
    color: #ffffff;
}
.rw.on label {
    color: #ffffff;
}
.rw.on .chevron {
    color: #ffffff;
}
.rw.ia {
    background-color: var(--ib);
}
.column-icon {
    min-width: 16px;
    min-height: 16px;
}
.column-icon.fi {
    color: #4ea1f0;
}
.column-icon.fl {
    color: var(--mu);
}
.rw.on .column-icon {
    color: #ffffff;
}
.chevron {
    font-size: 13px;
    color: var(--mu);
    margin-left: 4px;
}
.column-divider {
    min-width: 5px;
    width: 5px;
    background-color: transparent;
    cursor: col-resize;
    transition: background-color 150ms ease;
}
.column-divider:hover {
    background-color: var(--ac);
}
.column-divider.dragging {
    background-color: var(--ac);
}

/* Terminal Leaf Preview Column */
.preview-column {
    min-width: 240px;
    padding: 22px 20px;
    background-color: var(--g);
}
.preview-card {
    margin-top: 16px;
    padding: 10px 12px;
    border-radius: 12px;
    background-color: var(--cd);
    border: 1px solid var(--ln);
    line-height: 1.9;
}
.preview-title {
    font-weight: 600;
    font-size: 15px;
    color: var(--tx);
    margin-top: 12px;
}
.preview-subtitle {
    color: var(--mu);
    font-size: 13px;
}
.preview-btn {
    padding: 5px 14px;
    border-radius: 8px;
    background-color: var(--ib);
    color: var(--tx);
    font-size: 13px;
}
.preview-btn:hover {
    background-color: var(--hv);
}
.preview-btn.primary {
    background-color: var(--ac);
    color: #ffffff;
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
        assert!(FINDER_CSS.contains(".hd"));
        assert!(FINDER_CSS.contains(".r"));
        assert!(FINDER_CSS.contains(".dc"));
    }
}
