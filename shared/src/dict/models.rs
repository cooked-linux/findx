//! Domain data structures for lexical dictionary entries and quick answers.

use crate::models::{SearchCategory, SearchPayload, SearchRecord};

/// Individual lexical sense or definition for a dictionary entry.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DefinitionSense {
    /// Grammatical part of speech (e.g. `"noun"`, `"verb"`, `"adjective"`).
    pub part_of_speech: String,
    /// Detailed definition text.
    pub definition: String,
    /// Optional italicized example usage sentence.
    pub example: Option<String>,
}

impl DefinitionSense {
    /// Constructs a new [`DefinitionSense`].
    #[must_use]
    pub fn new(
        part_of_speech: impl Into<String>,
        definition: impl Into<String>,
        example: Option<String>,
    ) -> Self {
        Self {
            part_of_speech: part_of_speech.into(),
            definition: definition.into(),
            example,
        }
    }
}

/// Composite dictionary result card representing a headword, phonetics, senses, and metadata.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DictionaryCard {
    /// Capitalized or canonical headword (e.g. `"serendipity"`).
    pub headword: String,
    /// International Phonetic Alphabet pronunciation transcription (e.g. `"/ˌserənˈdɪpədē/"`).
    pub phonetic_ipa: Option<String>,
    /// Ordered list of definition senses categorized by part of speech.
    pub senses: Vec<DefinitionSense>,
    /// List of thesaurus synonyms and related terms.
    pub synonyms: Vec<String>,
    /// Word origin and historical etymology notes.
    pub etymology: Option<String>,
    /// Arbiter relevance score (typically `900.0` for explicit queries, `450.0` for implicit).
    pub score: f64,
}

impl DictionaryCard {
    /// Constructs a new [`DictionaryCard`].
    #[must_use]
    pub fn new(
        headword: impl Into<String>,
        phonetic_ipa: Option<String>,
        senses: Vec<DefinitionSense>,
        synonyms: Vec<String>,
        etymology: Option<String>,
        score: f64,
    ) -> Self {
        Self {
            headword: headword.into(),
            phonetic_ipa,
            senses,
            synonyms,
            etymology,
            score,
        }
    }

    /// Maps this dictionary entry into a [`SearchPayload::Dictionary`].
    #[must_use]
    pub fn to_search_payload(&self) -> SearchPayload {
        let definition = self
            .senses
            .first()
            .map(|s| s.definition.clone())
            .unwrap_or_default();

        SearchPayload::Dictionary {
            word: self.headword.clone(),
            definition,
        }
    }

    /// Converts this dictionary card into a unified [`SearchRecord`].
    #[must_use]
    pub fn to_search_record(&self) -> SearchRecord {
        let first_def = self
            .senses
            .first()
            .map(|s| {
                if s.part_of_speech.is_empty() {
                    s.definition.clone()
                } else {
                    format!("({}) {}", s.part_of_speech, s.definition)
                }
            })
            .unwrap_or_default();

        let subtitle = if let Some(ipa) = &self.phonetic_ipa {
            if first_def.is_empty() {
                ipa.clone()
            } else {
                format!("{ipa} • {first_def}")
            }
        } else {
            first_def
        };

        SearchRecord::new(
            format!("dict:{}", self.headword.to_ascii_lowercase()),
            self.headword.clone(),
            Some(subtitle),
            SearchCategory::Dictionary,
            Some("accessories-dictionary".to_string()),
            self.score,
            self.to_search_payload(),
        )
    }
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

    #[test]
    fn test_dictionary_card_serde_roundtrip() {
        let card = DictionaryCard::new(
            "serendipity",
            Some("/ˌserənˈdɪpədē/".to_string()),
            vec![DefinitionSense::new(
                "noun",
                "The occurrence and development of events by chance in a happy way.",
                Some("A fortunate stroke of serendipity.".to_string()),
            )],
            vec!["chance".to_string(), "fortune".to_string()],
            Some("Coined by Horace Walpole in 1754.".to_string()),
            900.0,
        );

        let json = serde_json::to_string(&card).expect("serialize card");
        let deserialized: DictionaryCard = serde_json::from_str(&json).expect("deserialize card");
        assert_eq!(card, deserialized);
    }

    #[test]
    fn test_to_search_record_conversion() {
        let card = DictionaryCard::new(
            "run",
            Some("/rʌn/".to_string()),
            vec![DefinitionSense::new(
                "verb",
                "Move at a speed faster than a walk.",
                None,
            )],
            vec!["sprint".to_string()],
            None,
            450.0,
        );

        let record = card.to_search_record();
        assert_eq!(record.id, "dict:run");
        assert_eq!(record.title, "run");
        assert_eq!(record.category, SearchCategory::Dictionary);
        assert_eq!(record.icon_name.as_deref(), Some("accessories-dictionary"));
        assert_eq!(record.score, 450.0);
        assert_eq!(
            record.subtitle.as_deref(),
            Some("/rʌn/ • (verb) Move at a speed faster than a walk.")
        );

        match record.payload {
            SearchPayload::Dictionary { word, definition } => {
                assert_eq!(word, "run");
                assert_eq!(definition, "Move at a speed faster than a walk.");
            }
            _ => panic!("unexpected payload variant"),
        }
    }
}
