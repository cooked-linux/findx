//! Integration tests for the Spotlight Calculator and Converter Provider.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::assert_is_empty
)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

use shared::calc::try_evaluate;
use shared::models::{SearchCategory, SearchPayload};
use spotlight::arbiter::dispatcher::QueryScope;
use spotlight::arbiter::provider::SearchProvider;
use spotlight::providers::CalculatorProvider;
use spotlight::ui::calc_card::{CalcHeroCardWidget, MathRepresentation};

#[tokio::test]
async fn test_arithmetic_query_returns_calculation_record() {
    let provider = CalculatorProvider::new();

    // Query: 25*4+10
    let results = provider
        .search("25*4+10", None, Instant::now())
        .await
        .expect("search succeeded");

    assert_eq!(results.len(), 1);
    let record = &results[0];
    assert_eq!(record.score, 980.0);
    assert_eq!(record.category, SearchCategory::Calculations);
    assert_eq!(record.title, "110");
    assert_eq!(record.icon_name.as_deref(), Some("accessories-calculator"));

    match &record.payload {
        SearchPayload::Calculation { expression, result } => {
            assert_eq!(expression, "25*4+10");
            assert_eq!(result, "110");
        }
        _ => panic!("Expected SearchPayload::Calculation"),
    }
}

#[tokio::test]
async fn test_unit_conversion_query_returns_converted_record() {
    let provider = CalculatorProvider::new();

    // Query: 10 miles in km
    let results = provider
        .search("10 miles in km", None, Instant::now())
        .await
        .expect("search succeeded");

    assert_eq!(results.len(), 1);
    let record = &results[0];
    assert_eq!(record.score, 980.0);
    assert_eq!(record.category, SearchCategory::Calculations);
    assert!(
        record.title.starts_with("16.09"),
        "Expected title to start with '16.09', got: {}",
        record.title
    );
}

#[tokio::test]
async fn test_dangling_operator_suppression() {
    let provider = CalculatorProvider::new();

    // Query: 25 +
    let results_add = provider
        .search("25 + ", None, Instant::now())
        .await
        .expect("search succeeded");
    assert!(
        results_add.is_empty(),
        "Expected empty result for dangling operator '25 + '"
    );

    // Query: 25 *
    let results_mul = provider
        .search("25 * ", None, Instant::now())
        .await
        .expect("search succeeded");
    assert!(
        results_mul.is_empty(),
        "Expected empty result for dangling operator '25 * '"
    );
}

#[tokio::test]
async fn test_plain_number_avoidance() {
    let provider = CalculatorProvider::new();

    // Query: 2026
    let results_2026 = provider
        .search("2026", None, Instant::now())
        .await
        .expect("search succeeded");
    assert!(
        results_2026.is_empty(),
        "Expected empty result for plain number '2026'"
    );

    // Query: 1984
    let results_1984 = provider
        .search("1984", None, Instant::now())
        .await
        .expect("search succeeded");
    assert!(
        results_1984.is_empty(),
        "Expected empty result for plain number '1984'"
    );
}

#[tokio::test]
async fn test_currency_conversion_query() {
    let provider = CalculatorProvider::new();

    // Query: $100 to eur
    let results = provider
        .search("$100 to eur", None, Instant::now())
        .await
        .expect("search succeeded");

    assert_eq!(results.len(), 1);
    let record = &results[0];
    assert_eq!(record.score, 980.0);
    assert_eq!(record.category, SearchCategory::Calculations);
    assert!(record.title.contains("EUR"));
}

#[test]
fn test_provider_fast_path_and_scope_routing() {
    let provider = CalculatorProvider::new();
    assert_eq!(provider.id(), "calculator");
    assert_eq!(provider.display_name(), "Calculator & Converter");
    assert!(provider.is_fast_path());
    assert!(provider.handles_scope(QueryScope::All));
    assert!(provider.handles_scope(QueryScope::Calculator));
    assert!(!provider.handles_scope(QueryScope::Apps));
    assert!(!provider.handles_scope(QueryScope::Files));
    assert!(!provider.handles_scope(QueryScope::Dictionary));
    assert!(!provider.handles_scope(QueryScope::Web));
}

#[test]
fn test_hero_card_ui_rendering_and_tab_cycling() {
    if gtk4::init().is_ok() {
        let calc = try_evaluate("25 * 4 + 10").expect("evaluation succeeded");
        let card = CalcHeroCardWidget::new(&calc);

        assert_eq!(card.expr_label.text(), "25 × 4 + 10");
        assert_eq!(card.value_label.text(), "110");
        assert_eq!(card.kind_label.text(), "Calculator");
        assert_eq!(card.current_representation(), MathRepresentation::Decimal);

        // Tab 1: Hexadecimal (110 = 0x6E)
        assert!(card.handle_key_press(gdk4::Key::Tab, gdk4::ModifierType::empty()));
        assert_eq!(card.value_label.text(), "0x6E");
        assert_eq!(
            card.current_representation(),
            MathRepresentation::Hexadecimal
        );

        // Tab 2: Binary (110 = 0b1101110)
        assert!(card.handle_key_press(gdk4::Key::Tab, gdk4::ModifierType::empty()));
        assert_eq!(card.value_label.text(), "0b1101110");
        assert_eq!(card.current_representation(), MathRepresentation::Binary);

        // Tab 3: Scientific (110 = 1.1 × 10²)
        assert!(card.handle_key_press(gdk4::Key::Tab, gdk4::ModifierType::empty()));
        assert_eq!(card.value_label.text(), "1.1 × 10²");
        assert_eq!(
            card.current_representation(),
            MathRepresentation::Scientific
        );

        // Tab 4: Wraps back to Decimal
        assert!(card.handle_key_press(gdk4::Key::Tab, gdk4::ModifierType::empty()));
        assert_eq!(card.value_label.text(), "110");
        assert_eq!(card.current_representation(), MathRepresentation::Decimal);
    }
}

#[test]
fn test_hero_card_clipboard_and_dismissal_actions() {
    if gtk4::init().is_ok() {
        let calc = try_evaluate("10 miles in km").expect("evaluation succeeded");
        let card = CalcHeroCardWidget::new(&calc);

        assert_eq!(card.expr_label.text(), "10 miles");
        assert_eq!(card.value_label.text(), "16.09 km");
        assert_eq!(card.kind_label.text(), "Conversion");

        let dismissed = Rc::new(Cell::new(false));
        let dismissed_clone = Rc::clone(&dismissed);
        card.connect_dismiss(move || {
            dismissed_clone.set(true);
        });

        let copied_text = Rc::new(RefCell::new(String::new()));
        let copied_clone = Rc::clone(&copied_text);
        card.connect_copied(move |text| {
            *copied_clone.borrow_mut() = text.to_string();
        });

        // Test Enter key action
        assert!(card.handle_key_press(gdk4::Key::Return, gdk4::ModifierType::empty()));
        assert!(dismissed.get());
        assert_eq!(*copied_text.borrow(), "16.09 km");

        // Reset and test Ctrl+C
        dismissed.set(false);
        *copied_text.borrow_mut() = String::new();

        assert!(card.handle_key_press(gdk4::Key::c, gdk4::ModifierType::CONTROL_MASK));
        assert!(dismissed.get());
        assert_eq!(*copied_text.borrow(), "16.09 km");
    }
}
