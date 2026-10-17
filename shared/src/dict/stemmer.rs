//! Morphological lemmatizer and root resolution using Snowball stemmer.

/// Morphological lemmatizer and stemmer wrapping the Snowball algorithm.
pub struct Lemmatizer {
    stemmer: rust_stemmers::Stemmer,
}

impl Lemmatizer {
    /// Creates a new English Snowball lemmatizer instance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stemmer: rust_stemmers::Stemmer::create(rust_stemmers::Algorithm::English),
        }
    }

    /// Stems or lemmatizes an English word down to its base morphological root.
    ///
    /// Handles common irregular plurals and inflections (e.g. `geese` -> `goose`, `mice` -> `mouse`, `children` -> `child`),
    /// as well as standard suffix stripping via Snowball (e.g. `running` -> `run`, `cats` -> `cat`).
    #[must_use]
    pub fn stem(&self, word: &str) -> String {
        let trimmed = word.trim();
        if trimmed.is_empty() {
            return String::new();
        }

        let lower = trimmed.to_ascii_lowercase();

        // Irregular nouns and verbs normalization
        match lower.as_str() {
            "geese" => return "goose".to_string(),
            "mice" => return "mouse".to_string(),
            "men" => return "man".to_string(),
            "women" => return "woman".to_string(),
            "children" => return "child".to_string(),
            "teeth" => return "tooth".to_string(),
            "feet" => return "foot".to_string(),
            "oxen" => return "ox".to_string(),
            "people" => return "person".to_string(),
            _ => {}
        }

        self.stemmer.stem(&lower).to_string()
    }
}

impl Default for Lemmatizer {
    fn default() -> Self {
        Self::new()
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
    fn test_snowball_stemming_regular() {
        let lemmatizer = Lemmatizer::new();
        assert_eq!(lemmatizer.stem("running"), "run");
        assert_eq!(lemmatizer.stem("cats"), "cat");
        assert_eq!(lemmatizer.stem("happily"), "happili");
    }

    #[test]
    fn test_irregular_lemmas() {
        let lemmatizer = Lemmatizer::default();
        assert_eq!(lemmatizer.stem("geese"), "goose");
        assert_eq!(lemmatizer.stem("mice"), "mouse");
        assert_eq!(lemmatizer.stem("children"), "child");
        assert_eq!(lemmatizer.stem("feet"), "foot");
        assert_eq!(lemmatizer.stem("teeth"), "tooth");
    }

    #[test]
    fn test_empty_and_whitespace() {
        let lemmatizer = Lemmatizer::new();
        assert_eq!(lemmatizer.stem(""), "");
        assert_eq!(lemmatizer.stem("   "), "");
    }
}
