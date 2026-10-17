//! Multi-tier dictionary lookup, trigger parsing, and diacritic normalization.

use unicode_normalization::UnicodeNormalization;

use crate::dict::DictError;
use crate::dict::models::DictionaryCard;
use crate::dict::stemmer::Lemmatizer;
use crate::dict::storage::DictionaryStorage;

/// Normalizes a headword or query term by decomposing Unicode diacritics (NFD),
/// stripping combining marks (accents, umlauts, tildes), converting to lowercase,
/// and trimming leading/trailing whitespace.
#[must_use]
pub fn normalize_key(input: &str) -> String {
    input
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .collect::<String>()
        .trim()
        .to_lowercase()
}

/// Returns `true` if `c` is a Unicode combining diacritical mark.
const fn is_combining_mark(c: char) -> bool {
    matches!(
        c,
        '\u{0300}'..='\u{036f}'
            | '\u{1ab0}'..='\u{1aff}'
            | '\u{1dc0}'..='\u{1dff}'
            | '\u{20d0}'..='\u{20ff}'
            | '\u{fe20}'..='\u{fe2f}'
    )
}

/// Determines whether a query contains an explicit dictionary trigger.
///
/// Supported prefix triggers: `define `, `dict `, `meaning of `.
/// Supported suffix triggers: ` definition`.
///
/// Returns a tuple `(is_explicit, target_word)`.
#[must_use]
pub fn parse_trigger(query: &str) -> (bool, String) {
    let trimmed = query.trim();

    for prefix in ["define ", "dict ", "meaning of "] {
        if let Some(rest) = strip_prefix_ignore_case(trimmed, prefix) {
            return (true, rest.trim().to_string());
        }
    }

    if let Some(head) = strip_suffix_ignore_case(trimmed, " definition") {
        return (true, head.trim().to_string());
    }

    (false, trimmed.to_string())
}

/// Strips an ASCII prefix case-insensitively.
const fn strip_prefix_ignore_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() < prefix.len() {
        return None;
    }
    let (head, tail) = s.split_at(prefix.len());
    if head.eq_ignore_ascii_case(prefix) {
        Some(tail)
    } else {
        None
    }
}

/// Strips an ASCII suffix case-insensitively.
const fn strip_suffix_ignore_case<'a>(s: &'a str, suffix: &str) -> Option<&'a str> {
    if s.len() < suffix.len() {
        return None;
    }
    let (head, tail) = s.split_at(s.len() - suffix.len());
    if tail.eq_ignore_ascii_case(suffix) {
        Some(head)
    } else {
        None
    }
}

/// Sets the appropriate arbiter relevance score on a dictionary card.
const fn apply_score(mut card: DictionaryCard, is_explicit: bool) -> DictionaryCard {
    card.score = if is_explicit { 900.0 } else { 450.0 };
    card
}

/// Resolves a user query against the dictionary storage engine using a multi-tier fallback strategy:
///
/// 1. **Tier 1 (Exact Match):** Look up normalized key directly.
/// 2. **Tier 2 (Morphological Stemming):** Fall back to Snowball root lemma (e.g. `running` -> `run`, `geese` -> `goose`).
/// 3. **Tier 3 (Prefix Completion):** If explicit (`define <word>`), match highest-ranked prefix candidate.
///
/// Returns `Ok(None)` if no dictionary entries match or if the search term is shorter than 2 characters.
///
/// # Errors
///
/// Returns [`DictError`] if underlying SQLite query or deserialization fails.
pub fn resolve_query(
    query: &str,
    storage: &DictionaryStorage,
    lemmatizer: &Lemmatizer,
) -> Result<Option<DictionaryCard>, DictError> {
    let (is_explicit, target_word) = parse_trigger(query);
    if target_word.chars().count() < 2 {
        return Ok(None);
    }

    let normalized = normalize_key(&target_word);
    if normalized.is_empty() {
        return Ok(None);
    }

    // Tier 1: Exact match
    if let Some(card) = storage.find_exact(&normalized)? {
        return Ok(Some(apply_score(card, is_explicit)));
    }

    // Tier 2: Stemmed fallback
    let stem = lemmatizer.stem(&normalized);
    if stem != normalized
        && !stem.is_empty()
        && let Some(card) = storage.find_exact(&stem)?
    {
        return Ok(Some(apply_score(card, is_explicit)));
    }

    // Tier 3: Prefix search (only if query was explicitly "define <word>")
    if is_explicit {
        let candidates = storage.find_prefix(&normalized, 1)?;
        if let Some(card) = candidates.into_iter().next() {
            return Ok(Some(apply_score(card, true)));
        }
    }

    Ok(None)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp
)]
mod tests {
    use super::*;
    use crate::dict::models::DefinitionSense;

    #[test]
    fn test_diacritic_normalization() {
        assert_eq!(normalize_key("café"), "cafe");
        assert_eq!(normalize_key("über"), "uber");
        assert_eq!(normalize_key("résumé"), "resume");
        assert_eq!(normalize_key("  SERENDIPITY  "), "serendipity");
        assert_eq!(normalize_key("façade"), "facade");
        assert_eq!(normalize_key("piñata"), "pinata");
    }

    #[test]
    fn test_trigger_parsing() {
        assert_eq!(
            parse_trigger("define serendipity"),
            (true, "serendipity".to_string())
        );
        assert_eq!(parse_trigger("Define  Apple "), (true, "Apple".to_string()));
        assert_eq!(parse_trigger("dict rust"), (true, "rust".to_string()));
        assert_eq!(parse_trigger("meaning of life"), (true, "life".to_string()));
        assert_eq!(
            parse_trigger("computer definition"),
            (true, "computer".to_string())
        );
        assert_eq!(
            parse_trigger("simple word"),
            (false, "simple word".to_string())
        );
    }

    #[test]
    fn test_multi_tier_lookup_resolution() {
        let storage = DictionaryStorage::open_in_memory().expect("open storage");
        let lemmatizer = Lemmatizer::new();

        let card_run = DictionaryCard::new(
            "run",
            Some("/rʌn/".to_string()),
            vec![DefinitionSense::new("verb", "Move fast on foot.", None)],
            vec!["sprint".to_string()],
            None,
            450.0,
        );
        let card_cafe = DictionaryCard::new(
            "café",
            Some("/kæˈfeɪ/".to_string()),
            vec![DefinitionSense::new(
                "noun",
                "A coffee shop or small restaurant.",
                None,
            )],
            vec!["bistro".to_string()],
            None,
            450.0,
        );

        storage.insert_entry(&card_run).expect("insert run");
        storage.insert_entry(&card_cafe).expect("insert cafe");

        // Tier 1: Exact match with diacritics
        let res = resolve_query("cafe", &storage, &lemmatizer).expect("resolve cafe");
        assert!(res.is_some());
        let res_card = res.unwrap();
        assert_eq!(res_card.headword, "café");
        assert_eq!(res_card.score, 450.0);

        // Tier 1: Explicit trigger
        let res = resolve_query("define café", &storage, &lemmatizer).expect("resolve define cafe");
        assert!(res.is_some());
        assert_eq!(res.unwrap().score, 900.0);

        // Tier 2: Stemming fallback
        let res = resolve_query("running", &storage, &lemmatizer).expect("resolve running");
        assert!(res.is_some());
        assert_eq!(res.unwrap().headword, "run");

        // Tier 3: Prefix fallback on explicit query
        let res = resolve_query("define ca", &storage, &lemmatizer).expect("resolve define ca");
        assert!(res.is_some());
        assert_eq!(res.unwrap().headword, "café");

        // Implicit prefix should NOT match Tier 3
        let res = resolve_query("ca", &storage, &lemmatizer).expect("resolve implicit ca");
        assert!(res.is_none());

        // Single character query returns None
        let res = resolve_query("a", &storage, &lemmatizer).expect("resolve single char");
        assert!(res.is_none());
    }
}
