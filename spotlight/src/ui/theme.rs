//! Frosted-glass CSS styling and theme synchronization.

use gtk4::prelude::*;

/// Complete CSS design system stylesheet adhering to macOS Spotlight styling.
pub const SPOTLIGHT_CSS: &str = r"
/* Light Theme Variables */
.light, .light.win, .light .win {
    --bg: rgba(250, 250, 252, 0.72);
    --bd: rgba(255, 255, 255, 0.75);
    --tx: #1d1d1f;
    --mu: #86868b;
    --dv: rgba(0, 0, 0, 0.09);
    --hv: rgba(0, 0, 0, 0.055);
    --card: rgba(255, 255, 255, 0.6);
    --sh: 0 28px 70px rgba(30, 40, 80, 0.30), 0 2px 10px rgba(0, 0, 0, 0.10);
    color: #1d1d1f;
}

/* Dark Theme Variables */
.dark, .dark.win, .dark .win {
    --bg: rgba(42, 42, 50, 0.66);
    --bd: rgba(255, 255, 255, 0.15);
    --tx: #f5f5f7;
    --mu: #9a9aa3;
    --dv: rgba(255, 255, 255, 0.10);
    --hv: rgba(255, 255, 255, 0.08);
    --card: rgba(255, 255, 255, 0.07);
    --sh: 0 28px 80px rgba(0, 0, 0, 0.60), 0 2px 10px rgba(0, 0, 0, 0.40);
    color: #f5f5f7;
}

/* Base Window Container */
.win {
    border-radius: 20px;
    border: 1px solid var(--bd);
    background-color: var(--bg);
    box-shadow: var(--sh);
    color: var(--tx);
}

/* Search Bar Header */
.sb {
    min-height: 68px;
    padding: 0 24px;
}

.sb.dv {
    border-bottom: 1px solid var(--dv);
}

.sb-text {
    font-size: 24px;
    font-weight: 300;
    letter-spacing: -0.2px;
    caret-color: #0a6cff;
    background: transparent;
    border: none;
    box-shadow: none;
    outline: none;
}

.ghost-text {
    font-size: 24px;
    font-weight: 300;
    letter-spacing: -0.2px;
    color: var(--mu);
    opacity: 0.55;
    background: transparent;
}

/* Search Icon */
.sb-icon {
    color: var(--mu);
    margin-right: 16px;
}

/* Footer Hints */
.hint {
    padding: 8px 18px;
    font-size: 12px;
    color: var(--mu);
    border-top: 1px solid var(--dv);
}

.hint b {
    font-weight: 600;
    color: var(--tx);
    margin-right: 4px;
}

/* Empty State Suggested Apps */
.empty-state {
    padding: 16px 20px 20px 20px;
    border-top: 1px solid var(--dv);
}

.suggested-dock {
    padding: 4px 0;
}


.suggested-app-item {
    border-radius: 14px;
    padding: 10px 14px;
    min-width: 80px;
    transition: background 120ms ease;
}

.suggested-app-item:hover, .suggested-app-item.selected {
    background-color: var(--hv);
}

.suggested-app-label {
    font-size: 11px;
    font-weight: 500;
    color: var(--tx);
}

/* Calculator & Unit/Currency Conversion Hero Card */
.card {
    margin: 12px 14px;
    padding: 22px 28px;
    border-radius: 14px;
    background: var(--card);
    border: 1px solid var(--dv);
}

.cx {
    font-size: 16px;
    color: var(--mu);
    margin-bottom: 2px;
}

.cv {
    font-size: 68px;
    font-weight: 300;
    letter-spacing: -1.5px;
    line-height: 1.05;
}

.ck {
    font-size: 12px;
    font-weight: 500;
    color: var(--mu);
}
";

/// Initializes the global CSS styling provider and connects `StyleManager` theme switching.
pub fn init_theme(window: &gtk4::ApplicationWindow) {
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(SPOTLIGHT_CSS);

    if let Some(display) = gdk4::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let manager = libadwaita::StyleManager::default();
    let is_dark = manager.is_dark();

    apply_theme_classes(window, is_dark);

    let window_weak = window.downgrade();
    manager.connect_dark_notify(move |mgr| {
        if let Some(win) = window_weak.upgrade() {
            apply_theme_classes(&win, mgr.is_dark());
        }
    });
}

/// Applies `.dark` or `.light` classes to the given widget based on dark mode status.
pub fn apply_theme_classes<W: IsA<gtk4::Widget>>(widget: &W, is_dark: bool) {
    if is_dark {
        widget.remove_css_class("light");
        widget.add_css_class("dark");
    } else {
        widget.remove_css_class("dark");
        widget.add_css_class("light");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spotlight_css_constants_defined() {
        assert!(SPOTLIGHT_CSS.contains(".win"));
        assert!(SPOTLIGHT_CSS.contains(".light"));
        assert!(SPOTLIGHT_CSS.contains(".dark"));
        assert!(SPOTLIGHT_CSS.contains(".sb"));
        assert!(SPOTLIGHT_CSS.contains(".hint"));
        assert!(SPOTLIGHT_CSS.contains(".card"));
        assert!(SPOTLIGHT_CSS.contains(".cx"));
        assert!(SPOTLIGHT_CSS.contains(".cv"));
        assert!(SPOTLIGHT_CSS.contains(".ck"));
    }
}
