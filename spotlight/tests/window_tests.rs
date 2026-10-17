//! Integration tests for Spotlight overlay window, geometry, and state transitions.

use spotlight::ui::{
    ANIMATION_DURATION_MS, FOOTER_HINT_HEIGHT, FOOTER_HINT_SHORTCUTS, HEADER_HEIGHT,
    INITIAL_HEIGHT, INITIAL_WIDTH, KeyAction, MAX_HEIGHT, OverlayState, ROW_HEIGHT,
    SpotlightWindow, calculate_target_height, calculate_top_margin, ease_out_cubic,
    interpolate_height, map_key_to_action,
};

#[test]
fn test_initial_geometry_calculation() {
    assert_eq!(INITIAL_WIDTH, 680);
    assert_eq!(INITIAL_HEIGHT, 68);
    assert_eq!(MAX_HEIGHT, 580);
    assert_eq!(ROW_HEIGHT, 44);
    assert_eq!(HEADER_HEIGHT, 28);
    assert_eq!(FOOTER_HINT_HEIGHT, 36);

    // Initial empty state has no results rows and no headers
    let initial_height = INITIAL_HEIGHT;
    assert_eq!(initial_height, 68);

    // Margin calculation for standard display resolutions
    // 1080p: 1080 * 0.18 = 194.4 -> 194
    assert_eq!(calculate_top_margin(1080), 194);
    // 1440p: 1440 * 0.18 = 259.2 -> 259
    assert_eq!(calculate_top_margin(1440), 259);
    // 4K (2160p): 2160 * 0.18 = 388.8 -> 389 (or 388 via integer div)
    assert_eq!(calculate_top_margin(2160), 388);
    // Fallback on invalid heights
    assert_eq!(calculate_top_margin(0), 194);
    assert_eq!(calculate_top_margin(-100), 194);
}

#[test]
fn test_target_height_calculations_across_results() {
    // Height formula: min(68 + (rows * 44) + (headers * 28) + 36, 580)
    let expected = |rows: usize, headers: usize| -> i32 {
        let r = i32::try_from(rows).unwrap_or(0);
        let h = i32::try_from(headers).unwrap_or(0);
        let base = 68 + (r * 44) + (h * 28) + 36;
        base.min(580)
    };

    // Test across 1 to 10 result items with various category divider counts
    for rows in 1..=10 {
        // 0 category dividers
        assert_eq!(calculate_target_height(rows, 0), expected(rows, 0));

        // 1 category divider
        assert_eq!(calculate_target_height(rows, 1), expected(rows, 1));

        // 2 category dividers
        assert_eq!(calculate_target_height(rows, 2), expected(rows, 2));

        // 3 category dividers
        assert_eq!(calculate_target_height(rows, 3), expected(rows, 3));
    }

    // Explicit checks:
    // 1 item, 0 headers: 68 + 44 + 36 = 148
    assert_eq!(calculate_target_height(1, 0), 148);

    // 2 items, 1 header: 68 + 88 + 28 + 36 = 220
    assert_eq!(calculate_target_height(2, 1), 220);

    // 5 items, 2 headers: 68 + 220 + 56 + 36 = 380
    assert_eq!(calculate_target_height(5, 2), 380);

    // Clamping at MAX_HEIGHT (580)
    // 10 items, 2 headers: 68 + 440 + 56 + 36 = 600 -> clamped to 580
    assert_eq!(calculate_target_height(10, 2), 580);
    // 20 items: must clamp at 580
    assert_eq!(calculate_target_height(20, 5), 580);
}

#[test]
fn test_overlay_state_lifecycle_transitions() {
    // 1. Initial Dormant state
    let state = OverlayState::Dormant;
    assert!(!state.is_visible());

    // 2. Transition Dormant -> Empty (hotkey pressed)
    assert!(state.can_transition_to(OverlayState::Empty));
    assert!(state.can_transition_to(OverlayState::Dormant));
    assert!(!state.can_transition_to(OverlayState::ResultsList));
    assert!(!state.can_transition_to(OverlayState::SplitPreview));

    // 3. In Empty state
    let state = OverlayState::Empty;
    assert!(state.is_visible());
    // Empty -> ResultsList (user typed characters and results returned)
    assert!(state.can_transition_to(OverlayState::ResultsList));
    // Empty -> Dormant (user dismissed via Escape or focus loss)
    assert!(state.can_transition_to(OverlayState::Dormant));
    assert!(state.can_transition_to(OverlayState::Empty));

    // 4. In ResultsList state
    let state = OverlayState::ResultsList;
    assert!(state.is_visible());
    // ResultsList -> Empty (user cleared query)
    assert!(state.can_transition_to(OverlayState::Empty));
    // ResultsList -> SplitPreview (user highlighted rich preview item)
    assert!(state.can_transition_to(OverlayState::SplitPreview));
    // ResultsList -> Dormant (user launched item or cancelled)
    assert!(state.can_transition_to(OverlayState::Dormant));
    assert!(state.can_transition_to(OverlayState::ResultsList));

    // 5. In SplitPreview state
    let state = OverlayState::SplitPreview;
    assert!(state.is_visible());
    assert!(state.can_transition_to(OverlayState::ResultsList));
    assert!(state.can_transition_to(OverlayState::Empty));
    assert!(state.can_transition_to(OverlayState::Dormant));
    assert!(state.can_transition_to(OverlayState::SplitPreview));

    // Complete canonical cycle: Dormant -> Empty -> ResultsList -> Dormant
    let mut current = OverlayState::Dormant;
    assert!(current.can_transition_to(OverlayState::Empty));
    current = OverlayState::Empty;
    assert!(current.can_transition_to(OverlayState::ResultsList));
    current = OverlayState::ResultsList;
    assert!(current.can_transition_to(OverlayState::Dormant));
    current = OverlayState::Dormant;
    assert_eq!(current, OverlayState::Dormant);
}

#[test]
fn test_ease_out_cubic_and_interpolation_math() {
    assert_eq!(ANIMATION_DURATION_MS, 120);

    // Easing boundaries
    assert!((ease_out_cubic(0.0) - 0.0).abs() < 1e-6);
    assert!((ease_out_cubic(1.0) - 1.0).abs() < 1e-6);

    // Monotonically increasing curve
    let mut prev = 0.0;
    for step in 1..=10 {
        let p = f64::from(step) / 10.0;
        let v = ease_out_cubic(p);
        assert!(v >= prev);
        prev = v;
    }

    // Height interpolation
    assert_eq!(interpolate_height(68, 580, 0.0), 68);
    assert_eq!(interpolate_height(68, 580, 1.0), 580);
    let mid = interpolate_height(68, 580, 0.5);
    assert!(mid > 450); // fast start of ease-out
}

#[test]
fn test_navigation_key_mappings() {
    assert_eq!(
        map_key_to_action(gdk4::Key::Up, gdk4::ModifierType::empty()),
        Some(KeyAction::Up)
    );
    assert_eq!(
        map_key_to_action(gdk4::Key::Down, gdk4::ModifierType::empty()),
        Some(KeyAction::Down)
    );
    assert_eq!(
        map_key_to_action(gdk4::Key::k, gdk4::ModifierType::CONTROL_MASK),
        Some(KeyAction::Up)
    );
    assert_eq!(
        map_key_to_action(gdk4::Key::j, gdk4::ModifierType::CONTROL_MASK),
        Some(KeyAction::Down)
    );
    assert_eq!(
        map_key_to_action(gdk4::Key::Return, gdk4::ModifierType::empty()),
        Some(KeyAction::Enter)
    );
    assert_eq!(
        map_key_to_action(gdk4::Key::KP_Enter, gdk4::ModifierType::empty()),
        Some(KeyAction::Enter)
    );
    assert_eq!(
        map_key_to_action(gdk4::Key::Tab, gdk4::ModifierType::empty()),
        Some(KeyAction::Tab)
    );
    assert_eq!(
        map_key_to_action(gdk4::Key::Escape, gdk4::ModifierType::empty()),
        Some(KeyAction::Escape)
    );
    assert_eq!(
        map_key_to_action(gdk4::Key::space, gdk4::ModifierType::SUPER_MASK),
        Some(KeyAction::Toggle)
    );
}

#[test]
fn test_footer_hint_shortcuts_integrity() {
    assert_eq!(FOOTER_HINT_SHORTCUTS.len(), 3);
    assert_eq!(FOOTER_HINT_SHORTCUTS[0], ("↑↓", "Navigate"));
    assert_eq!(FOOTER_HINT_SHORTCUTS[1], ("↵", "Open"));
    assert_eq!(FOOTER_HINT_SHORTCUTS[2], ("Ctrl+↵", "Show in Folder"));
}

#[test]
fn test_spotlight_window_lifecycle_if_gtk_available() {
    if gtk4::init().is_ok() {
        let window = SpotlightWindow::new(None);
        assert_eq!(window.state(), OverlayState::Dormant);
        assert_eq!(window.current_height(), INITIAL_HEIGHT);

        assert!(window.set_state(OverlayState::Empty));
        assert_eq!(window.state(), OverlayState::Empty);

        assert!(window.set_state(OverlayState::ResultsList));
        assert_eq!(window.state(), OverlayState::ResultsList);

        assert!(window.set_state(OverlayState::Dormant));
        assert_eq!(window.state(), OverlayState::Dormant);
    }
}
