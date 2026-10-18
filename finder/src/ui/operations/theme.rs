//! CSS Theme definitions for operations progress, queue popover, and conflict dialogs.

/// CSS stylesheet for file operations progress, queue popover, and conflict resolution cards.
pub const OPERATIONS_CSS: &str = r"
/* Operations Transfer Progress Dialog */
.operations-progress-dialog {
    background: var(--g);
    border: 1px solid var(--ln);
    border-radius: 12px;
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.24);
    padding: 16px;
}

.operations-progress-title {
    font-weight: 600;
    font-size: 14px;
    color: var(--tx);
}

.operations-progress-subtitle {
    font-size: 12px;
    color: var(--mu);
}

.operations-progress-metrics {
    font-size: 11px;
    color: var(--mu);
    font-variant-numeric: tabular-nums;
}

/* Conflict Comparison Cards */
.conflict-dialog-card {
    background: var(--cd);
    border: 1px solid var(--ln);
    border-radius: 8px;
    padding: 12px;
}

.conflict-age-badge-newer {
    background: rgba(47, 109, 246, 0.15);
    color: #2f6df6;
    border-radius: 4px;
    padding: 2px 6px;
    font-size: 11px;
    font-weight: 600;
}

.conflict-age-badge-older {
    background: rgba(122, 122, 133, 0.15);
    color: var(--mu);
    border-radius: 4px;
    padding: 2px 6px;
    font-size: 11px;
}

/* Operations Queue Popover */
.operations-queue-popover {
    min-width: 340px;
    padding: 8px;
}

.operations-queue-title {
    font-weight: 600;
    font-size: 13px;
    color: var(--tx);
    padding: 6px 8px;
}

.operations-queue-row {
    padding: 8px;
    border-radius: 6px;
    border-bottom: 1px solid var(--ln);
}

.operations-queue-row:last-child {
    border-bottom: none;
}

.operations-queue-empty {
    color: var(--mu);
    font-size: 12px;
    padding: 16px;
}

.queue-btn-active {
    color: var(--ac);
    font-weight: 600;
}
";

/// Installs the operations CSS styling provider onto the default display.
pub fn install_operations_theme() {
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(OPERATIONS_CSS);
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
    fn test_operations_css_classes() {
        assert!(OPERATIONS_CSS.contains(".operations-progress-dialog"));
        assert!(OPERATIONS_CSS.contains(".operations-progress-title"));
        assert!(OPERATIONS_CSS.contains(".operations-progress-subtitle"));
        assert!(OPERATIONS_CSS.contains(".operations-progress-metrics"));
        assert!(OPERATIONS_CSS.contains(".conflict-dialog-card"));
        assert!(OPERATIONS_CSS.contains(".conflict-age-badge-newer"));
        assert!(OPERATIONS_CSS.contains(".conflict-age-badge-older"));
        assert!(OPERATIONS_CSS.contains(".operations-queue-popover"));
    }
}
