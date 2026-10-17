//! Integration tests for the offline lexical dictionary and quick answers engine.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp,
    clippy::assert_is_empty
)]

use shared::dict::{
    DefinitionSense, DictionaryCard, DictionaryStorage, Lemmatizer, normalize_key, parse_trigger,
    resolve_query,
};
use shared::models::{SearchCategory, SearchPayload};

fn setup_seeded_storage() -> DictionaryStorage {
    let storage = DictionaryStorage::open_in_memory().expect("open in-memory storage");

    let card_serendipity = DictionaryCard::new(
        "serendipity",
        Some("/ˌserənˈdɪpədē/".to_string()),
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

#[test]
fn test_diacritic_normalization() {
    assert_eq!(normalize_key("café"), "cafe");
    assert_eq!(normalize_key("über"), "uber");
    assert_eq!(normalize_key("résumé"), "resume");
    assert_eq!(normalize_key("piñata"), "pinata");
    assert_eq!(normalize_key("façade"), "facade");
    assert_eq!(normalize_key("  NAÏVE  "), "naive");
}

#[test]
fn test_exact_lookup_seeded_database() {
    let storage = setup_seeded_storage();
    let lemmatizer = Lemmatizer::new();

    // Plain ASCII query matching accented headword
    let result = resolve_query("cafe", &storage, &lemmatizer).expect("resolve cafe");
    assert!(result.is_some());
    let card = result.unwrap();
    assert_eq!(card.headword, "café");
    assert_eq!(card.phonetic_ipa.as_deref(), Some("/kæˈfeɪ/"));
    assert_eq!(card.senses.len(), 1);
    assert_eq!(card.senses[0].part_of_speech, "noun");
    assert_eq!(card.synonyms, vec!["bistro", "coffeehouse"]);
    assert_eq!(card.score, 450.0);

    // Accented query matching accented headword
    let result_accented = resolve_query("café", &storage, &lemmatizer).expect("resolve café");
    assert!(result_accented.is_some());
    assert_eq!(result_accented.unwrap().headword, "café");

    // Exact lookup for serendipity
    let result_serendipity =
        resolve_query("serendipity", &storage, &lemmatizer).expect("resolve serendipity");
    assert!(result_serendipity.is_some());
    let card_serendipity = result_serendipity.unwrap();
    assert_eq!(card_serendipity.headword, "serendipity");
    assert_eq!(card_serendipity.score, 450.0);
}

#[test]
fn test_stemming_fallback() {
    let storage = setup_seeded_storage();
    let lemmatizer = Lemmatizer::new();

    // "running" -> stems to "run"
    let result_running = resolve_query("running", &storage, &lemmatizer).expect("resolve running");
    assert!(result_running.is_some());
    let card = result_running.unwrap();
    assert_eq!(card.headword, "run");
    assert_eq!(card.score, 450.0);

    // "geese" -> irregular lemmatization to "goose"
    let result_geese = resolve_query("geese", &storage, &lemmatizer).expect("resolve geese");
    assert!(result_geese.is_some());
    let card_goose = result_geese.unwrap();
    assert_eq!(card_goose.headword, "goose");
}

#[test]
fn test_explicit_trigger_parser_and_scoring() {
    let storage = setup_seeded_storage();
    let lemmatizer = Lemmatizer::new();

    // Trigger prefix: "define "
    let (is_exp, word) = parse_trigger("define serendipity");
    assert!(is_exp);
    assert_eq!(word, "serendipity");

    let res = resolve_query("define serendipity", &storage, &lemmatizer).expect("query define");
    assert!(res.is_some());
    assert_eq!(res.unwrap().score, 900.0);

    // Trigger prefix: "dict "
    let (is_exp, word) = parse_trigger("dict run");
    assert!(is_exp);
    assert_eq!(word, "run");

    let res = resolve_query("dict run", &storage, &lemmatizer).expect("query dict");
    assert!(res.is_some());
    assert_eq!(res.unwrap().score, 900.0);

    // Trigger prefix: "meaning of "
    let (is_exp, word) = parse_trigger("meaning of café");
    assert!(is_exp);
    assert_eq!(word, "café");

    let res = resolve_query("meaning of café", &storage, &lemmatizer).expect("query meaning of");
    assert!(res.is_some());
    assert_eq!(res.unwrap().score, 900.0);

    // Trigger suffix: " definition"
    let (is_exp, word) = parse_trigger("serendipity definition");
    assert!(is_exp);
    assert_eq!(word, "serendipity");

    let res =
        resolve_query("serendipity definition", &storage, &lemmatizer).expect("query definition");
    assert!(res.is_some());
    assert_eq!(res.unwrap().score, 900.0);

    // Implicit query yields default score 450.0
    let (is_exp, _) = parse_trigger("serendipity");
    assert!(!is_exp);

    let res = resolve_query("serendipity", &storage, &lemmatizer).expect("query implicit");
    assert!(res.is_some());
    assert_eq!(res.unwrap().score, 450.0);
}

#[test]
fn test_prefix_fallback_only_on_explicit() {
    let storage = setup_seeded_storage();
    let lemmatizer = Lemmatizer::new();

    // Explicit prefix match: "define seren" -> matches "serendipity"
    let res = resolve_query("define seren", &storage, &lemmatizer).expect("query prefix explicit");
    assert!(res.is_some());
    let card = res.unwrap();
    assert_eq!(card.headword, "serendipity");
    assert_eq!(card.score, 900.0);

    // Implicit prefix match: "seren" -> must return None
    let res_implicit =
        resolve_query("seren", &storage, &lemmatizer).expect("query prefix implicit");
    assert!(res_implicit.is_none());
}

#[test]
fn test_nonexistent_word_returns_ok_none() {
    let storage = setup_seeded_storage();
    let lemmatizer = Lemmatizer::new();

    // Completely unknown word
    let res = resolve_query("nonexistentwordxyz", &storage, &lemmatizer)
        .expect("query unknown nonexistent");
    assert!(res.is_none());

    // Single character query
    let res_single = resolve_query("a", &storage, &lemmatizer).expect("query single char");
    assert!(res_single.is_none());

    // Empty query
    let res_empty = resolve_query("", &storage, &lemmatizer).expect("query empty");
    assert!(res_empty.is_none());

    let res_spaces = resolve_query("   ", &storage, &lemmatizer).expect("query spaces");
    assert!(res_spaces.is_none());
}

#[test]
fn test_search_record_and_payload_integration() {
    let storage = setup_seeded_storage();
    let lemmatizer = Lemmatizer::new();

    let card = resolve_query("define serendipity", &storage, &lemmatizer)
        .expect("resolve serendipity")
        .expect("card must exist");

    let record = card.to_search_record();
    assert_eq!(record.id, "dict:serendipity");
    assert_eq!(record.title, "serendipity");
    assert_eq!(record.category, SearchCategory::Dictionary);
    assert_eq!(record.icon_name.as_deref(), Some("accessories-dictionary"));
    assert_eq!(record.score, 900.0);
    assert!(record.subtitle.is_some());
    let subtitle = record.subtitle.unwrap();
    assert!(subtitle.contains("/ˌserənˈdɪpədē/"));
    assert!(subtitle.contains("(noun) The occurrence and development"));

    match record.payload {
        SearchPayload::Dictionary { word, definition } => {
            assert_eq!(word, "serendipity");
            assert!(definition.contains("The occurrence and development of events by chance"));
        }
        _ => panic!("Expected SearchPayload::Dictionary"),
    }
}
