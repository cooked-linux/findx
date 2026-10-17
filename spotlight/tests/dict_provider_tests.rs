//! Integration tests for the Spotlight Lexical Dictionary and Quick Answers Provider.

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

use shared::dict::{DefinitionSense, DictionaryCard, DictionaryStorage};
use shared::models::{SearchCategory, SearchPayload};
use spotlight::arbiter::dispatcher::QueryScope;
use spotlight::arbiter::provider::SearchProvider;
use spotlight::providers::DictionaryProvider;
use spotlight::ui::dict_card::DictCardWidget;

fn setup_seeded_storage() -> DictionaryStorage {
    let storage = DictionaryStorage::open_in_memory().expect("open storage");

    let card_serendipity = DictionaryCard::new(
        "serendipity",
        Some("/ˌsɛr.ənˈdɪp.ɪ.ti/".to_string()),
        vec![DefinitionSense::new(
            "noun",
            "The occurrence and development of events by chance in a happy or beneficial way.",
            Some("A fortunate stroke of serendipity.".to_string()),
        )],
        vec![
            "chance".to_string(),
            "fortune".to_string(),
            "fluke".to_string(),
        ],
        Some("Coined by Horace Walpole in 1754.".to_string()),
        450.0,
    );

    let card_cafe = DictionaryCard::new(
        "café",
        Some("/kæˈfeɪ/".to_string()),
        vec![DefinitionSense::new(
            "noun",
            "A small restaurant selling light meals and drinks.",
            Some("Let's meet at the local café.".to_string()),
        )],
        vec!["bistro".to_string(), "coffeehouse".to_string()],
        Some("From French café (coffee).".to_string()),
        450.0,
    );

    let card_run = DictionaryCard::new(
        "run",
        Some("/rʌn/".to_string()),
        vec![
            DefinitionSense::new("verb", "Move at a speed faster than a walk.", None),
            DefinitionSense::new("noun", "An act or spell of running.", None),
        ],
        vec!["sprint".to_string(), "dash".to_string()],
        None,
        450.0,
    );

    let card_goose = DictionaryCard::new(
        "goose",
        Some("/ɡuːs/".to_string()),
        vec![DefinitionSense::new(
            "noun",
            "A large waterbird with a long neck, short legs, webbed feet, and a short broad bill.",
            None,
        )],
        vec![],
        None,
        450.0,
    );

    storage
        .insert_entry(&card_serendipity)
        .expect("seed serendipity");
    storage.insert_entry(&card_cafe).expect("seed cafe");
    storage.insert_entry(&card_run).expect("seed run");
    storage.insert_entry(&card_goose).expect("seed goose");

    storage
}

#[tokio::test]
async fn test_explicit_prefix_parsing_and_lookup() {
    let storage = setup_seeded_storage();
    let provider = DictionaryProvider::from_storage(storage);

    // 1. "define serendipity"
    let results_define = provider
        .search("define serendipity", None, Instant::now())
        .await
        .expect("search succeeded");

    assert_eq!(results_define.len(), 3);
    let dict_rec = &results_define[0];
    assert_eq!(dict_rec.title, "serendipity");
    assert_eq!(dict_rec.category, SearchCategory::Dictionary);
    assert_eq!(dict_rec.score, 900.0);
    assert_eq!(
        dict_rec.icon_name.as_deref(),
        Some("accessories-dictionary")
    );
    assert!(
        dict_rec
            .subtitle
            .as_deref()
            .unwrap_or_default()
            .contains("/ˌsɛr.ənˈdɪp.ɪ.ti/")
    );

    match &dict_rec.payload {
        SearchPayload::Dictionary { word, definition } => {
            assert_eq!(word, "serendipity");
            assert!(definition.contains("chance in a happy or beneficial way"));
        }
        _ => panic!("Expected SearchPayload::Dictionary"),
    }

    // 2. "dict run"
    let results_dict = provider
        .search("dict run", None, Instant::now())
        .await
        .expect("search succeeded");
    assert_eq!(results_dict.len(), 3);
    assert_eq!(results_dict[0].title, "run");
    assert_eq!(results_dict[0].score, 900.0);

    // 3. "meaning of café"
    let results_meaning = provider
        .search("meaning of café", None, Instant::now())
        .await
        .expect("search succeeded");
    assert_eq!(results_meaning.len(), 3);
    assert_eq!(results_meaning[0].title, "café");
    assert_eq!(results_meaning[0].score, 900.0);

    // 4. "serendipity definition"
    let results_suffix = provider
        .search("serendipity definition", None, Instant::now())
        .await
        .expect("search succeeded");
    assert_eq!(results_suffix.len(), 3);
    assert_eq!(results_suffix[0].title, "serendipity");
    assert_eq!(results_suffix[0].score, 900.0);

    // 5. "dict:serendipity"
    let results_colon = provider
        .search("dict:serendipity", None, Instant::now())
        .await
        .expect("search succeeded");
    assert_eq!(results_colon.len(), 3);
    assert_eq!(results_colon[0].title, "serendipity");
    assert_eq!(results_colon[0].score, 900.0);
}

#[tokio::test]
async fn test_implicit_vocabulary_lookup() {
    let storage = setup_seeded_storage();
    let provider = DictionaryProvider::from_storage(storage);

    let results = provider
        .search("serendipity", None, Instant::now())
        .await
        .expect("search succeeded");

    assert_eq!(results.len(), 3);
    let dict_rec = &results[0];
    assert_eq!(dict_rec.title, "serendipity");
    assert_eq!(dict_rec.score, 450.0);
    assert_eq!(dict_rec.category, SearchCategory::Dictionary);
}

#[tokio::test]
async fn test_stemming_and_lemmatization() {
    let storage = setup_seeded_storage();
    let provider = DictionaryProvider::from_storage(storage);

    // "running" -> maps to "run"
    let results_run = provider
        .search("running", None, Instant::now())
        .await
        .expect("search succeeded");
    assert_eq!(results_run.len(), 3);
    assert_eq!(results_run[0].title, "run");

    // "geese" -> irregular plural maps to "goose"
    let results_geese = provider
        .search("geese", None, Instant::now())
        .await
        .expect("search succeeded");
    assert_eq!(results_geese.len(), 3);
    assert_eq!(results_geese[0].title, "goose");
}

#[tokio::test]
async fn test_accent_and_diacritic_normalization() {
    let storage = setup_seeded_storage();
    let provider = DictionaryProvider::from_storage(storage);

    // Unaccented "cafe" resolves to accented headword "café"
    let results_plain = provider
        .search("cafe", None, Instant::now())
        .await
        .expect("search succeeded");
    assert_eq!(results_plain.len(), 3);
    assert_eq!(results_plain[0].title, "café");

    // Accented "café" resolves directly
    let results_accent = provider
        .search("café", None, Instant::now())
        .await
        .expect("search succeeded");
    assert_eq!(results_accent.len(), 3);
    assert_eq!(results_accent[0].title, "café");
}

#[tokio::test]
async fn test_web_suggestions_generation() {
    let storage = setup_seeded_storage();
    let provider = DictionaryProvider::from_storage(storage);

    let results = provider
        .search("define serendipity", None, Instant::now())
        .await
        .expect("search succeeded");

    assert_eq!(results.len(), 3);

    // Web Suggestion 1: serendipity meaning
    let sugg_meaning = &results[1];
    assert_eq!(sugg_meaning.title, "serendipity meaning");
    assert_eq!(sugg_meaning.subtitle.as_deref(), Some("Web Suggestion"));
    assert_eq!(sugg_meaning.category, SearchCategory::WebSearch);
    assert_eq!(
        sugg_meaning.icon_name.as_deref(),
        Some("applications-internet")
    );
    match &sugg_meaning.payload {
        SearchPayload::WebSearch { url } => {
            assert!(url.contains("duckduckgo.com"));
            assert!(url.contains("serendipity+meaning") || url.contains("serendipity%20meaning"));
        }
        _ => panic!("Expected SearchPayload::WebSearch"),
    }

    // Web Suggestion 2: serendipity examples
    let sugg_examples = &results[2];
    assert_eq!(sugg_examples.title, "serendipity examples");
    assert_eq!(sugg_examples.subtitle.as_deref(), Some("Web Suggestion"));
    assert_eq!(sugg_examples.category, SearchCategory::WebSearch);
    assert_eq!(
        sugg_examples.icon_name.as_deref(),
        Some("applications-internet")
    );
    match &sugg_examples.payload {
        SearchPayload::WebSearch { url } => {
            assert!(url.contains("duckduckgo.com"));
            assert!(url.contains("serendipity+examples") || url.contains("serendipity%20examples"));
        }
        _ => panic!("Expected SearchPayload::WebSearch"),
    }
}

#[tokio::test]
async fn test_missing_database_graceful_degradation() {
    // When no dictionary databases are installed, search fails silently and returns empty list
    let provider = DictionaryProvider::with_storage(None);
    assert!(!provider.is_available());

    let results = provider
        .search("define serendipity", None, Instant::now())
        .await
        .expect("search succeeded without panic");

    assert!(results.is_empty());

    let results_implicit = provider
        .search("serendipity", None, Instant::now())
        .await
        .expect("search succeeded without panic");

    assert!(results_implicit.is_empty());
}

#[test]
fn test_provider_metadata_and_scope_routing() {
    let provider = DictionaryProvider::new();
    assert_eq!(provider.id(), "dictionary");
    assert_eq!(provider.display_name(), "Dictionary");
    assert!(provider.is_fast_path());
    assert!(provider.handles_scope(QueryScope::All));
    assert!(provider.handles_scope(QueryScope::Dictionary));
    assert!(!provider.handles_scope(QueryScope::Apps));
    assert!(!provider.handles_scope(QueryScope::Files));
    assert!(!provider.handles_scope(QueryScope::Calculator));
    assert!(!provider.handles_scope(QueryScope::Web));
}

#[test]
fn test_dict_card_widget_ui_and_actions() {
    if !gtk4::is_initialized_main_thread() && (gtk4::is_initialized() || gtk4::init().is_err()) {
        return;
    }
    let card = DictionaryCard::new(
        "serendipity",
        Some("/ˌsɛr.ənˈdɪp.ɪ.ti/".to_string()),
        vec![DefinitionSense::new(
            "noun",
            "The happy chance of finding something good while looking for something else.",
            Some("A fortunate stroke of serendipity.".to_string()),
        )],
        vec!["chance".to_string(), "fortune".to_string()],
        Some("Coined by Horace Walpole in 1754.".to_string()),
        900.0,
    );

    let widget = DictCardWidget::new(&card);

    // Verification of styling and text labels
    assert_eq!(widget.headword_label.text(), "serendipity");
    assert_eq!(widget.meta_label.text(), "noun · /ˌsɛr.ənˈdɪp.ɪ.ti/");
    assert_eq!(
        widget.body_label.text(),
        "The happy chance of finding something good while looking for something else."
    );
    assert_eq!(widget.footer_label.text(), "Dictionary");

    let dismissed = Rc::new(Cell::new(false));
    let dismissed_clone = Rc::clone(&dismissed);
    widget.connect_dismiss(move || {
        dismissed_clone.set(true);
    });

    let copied_text = Rc::new(RefCell::new(String::new()));
    let copied_clone = Rc::clone(&copied_text);
    widget.connect_copied(move |text| {
        *copied_clone.borrow_mut() = text.to_string();
    });

    // Test Enter action: copies clean definition text and triggers dismissal
    assert!(widget.handle_key_press(gdk4::Key::Return, gdk4::ModifierType::empty()));
    assert!(dismissed.get());
    assert_eq!(
        *copied_text.borrow(),
        "The happy chance of finding something good while looking for something else."
    );

    // Reset and test KP_Enter
    dismissed.set(false);
    *copied_text.borrow_mut() = String::new();
    assert!(widget.handle_key_press(gdk4::Key::KP_Enter, gdk4::ModifierType::empty()));
    assert!(dismissed.get());
    assert_eq!(
        *copied_text.borrow(),
        "The happy chance of finding something good while looking for something else."
    );

    // Reset and test Ctrl+C: copies headword and phonetic IPA transcription
    dismissed.set(false);
    *copied_text.borrow_mut() = String::new();
    assert!(widget.handle_key_press(gdk4::Key::c, gdk4::ModifierType::CONTROL_MASK));
    assert!(dismissed.get());
    assert_eq!(*copied_text.borrow(), "serendipity /ˌsɛr.ənˈdɪp.ɪ.ti/");

    // Reset and test Super+C (Cmd+C on macOS/Wayland)
    dismissed.set(false);
    *copied_text.borrow_mut() = String::new();
    assert!(widget.handle_key_press(gdk4::Key::C, gdk4::ModifierType::SUPER_MASK));
    assert!(dismissed.get());
    assert_eq!(*copied_text.borrow(), "serendipity /ˌsɛr.ənˈdɪp.ɪ.ti/");
}
