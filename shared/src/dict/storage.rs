//! SQLite dictionary storage engine and query manager.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, params};

use crate::db::PRAGMAS;
use crate::dict::DictError;
use crate::dict::lookup::normalize_key;
use crate::dict::models::{DefinitionSense, DictionaryCard};

/// Schema definition and index initialization for dictionary tables.
const SCHEMA_SQL: &str = "\
CREATE TABLE IF NOT EXISTS dictionary_entries (
    id INTEGER PRIMARY KEY,
    normalized_key TEXT NOT NULL,
    word TEXT NOT NULL,
    phonetic_ipa TEXT,
    senses_json TEXT NOT NULL,
    synonyms_json TEXT,
    etymology TEXT
);
CREATE INDEX IF NOT EXISTS idx_dict_key ON dictionary_entries(normalized_key);
CREATE INDEX IF NOT EXISTS idx_dict_prefix ON dictionary_entries(normalized_key COLLATE NOCASE);
";

/// Persistent or in-memory SQLite storage engine for dictionary entries.
#[derive(Debug)]
pub struct DictionaryStorage {
    conn: Mutex<Connection>,
}

impl DictionaryStorage {
    /// Opens an in-memory SQLite dictionary database with configured PRAGMAs and schema.
    ///
    /// Primarily used for testing.
    ///
    /// # Errors
    ///
    /// Returns [`DictError::Db`] if SQLite connection or table initialization fails.
    pub fn open_in_memory() -> Result<Self, DictError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(PRAGMAS)?;
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Opens an embedded dictionary database file at `path`.
    ///
    /// # Errors
    ///
    /// Returns [`DictError::Db`] if opening the SQLite database or configuring PRAGMAs fails.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DictError> {
        let conn = Connection::open(path)?;
        conn.execute_batch(PRAGMAS)?;
        conn.execute_batch(SCHEMA_SQL)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Checks standard filesystem locations for an installed dictionary database.
    ///
    /// Checks:
    /// - `$XDG_DATA_HOME/findx/dictionary.db` (e.g. `~/.local/share/findx/dictionary.db`)
    /// - `/usr/share/findx/dictionary.db`
    /// - `/usr/local/share/findx/dictionary.db`
    ///
    /// Returns `Ok(None)` gracefully if no database is installed on the user system.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] if opening an existing database file fails.
    pub fn open_default() -> Result<Option<Self>, DictError> {
        if let Ok(xdg_dirs) = xdg::BaseDirectories::with_prefix("findx")
            && let Some(user_path) = xdg_dirs.find_data_file("dictionary.db")
            && user_path.exists()
        {
            return Self::open(user_path).map(Some);
        }

        if let Ok(xdg_dirs) = xdg::BaseDirectories::with_prefix("spotlight_linux/dictionaries")
            && let Some(user_path) = xdg_dirs.find_data_file("dictionary.db")
            && user_path.exists()
        {
            return Self::open(user_path).map(Some);
        }

        for sys_path in [
            "/usr/share/findx/dictionary.db",
            "/usr/local/share/findx/dictionary.db",
            "/usr/share/stardict/dic/dictionary.db",
            "/usr/share/spotlight_linux/dictionaries/dictionary.db",
        ] {
            let p = Path::new(sys_path);
            if p.exists() {
                return Self::open(p).map(Some);
            }
        }

        Ok(None)
    }

    /// Inserts a new dictionary card entry into the database.
    ///
    /// # Errors
    ///
    /// Returns [`DictError::Json`] if serialization fails, or [`DictError::Db`] if SQLite execution fails.
    pub fn insert_entry(&self, card: &DictionaryCard) -> Result<(), DictError> {
        let normalized = normalize_key(&card.headword);
        let senses_json = serde_json::to_string(&card.senses)?;
        let synonyms_json = if card.synonyms.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&card.synonyms)?)
        };

        let conn = self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        conn.execute(
            "INSERT INTO dictionary_entries (normalized_key, word, phonetic_ipa, senses_json, synonyms_json, etymology) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
            params![
                normalized,
                card.headword,
                card.phonetic_ipa,
                senses_json,
                synonyms_json,
                card.etymology,
            ],
        )?;
        drop(conn);

        Ok(())
    }

    /// Finds an exact entry matching `normalized_key`.
    ///
    /// # Errors
    ///
    /// Returns [`DictError::Db`] or [`DictError::Json`] if querying or deserialization fails.
    pub fn find_exact(&self, normalized_key: &str) -> Result<Option<DictionaryCard>, DictError> {
        let conn = self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let row_result = conn.query_row(
            "SELECT word, phonetic_ipa, senses_json, synonyms_json, etymology \
             FROM dictionary_entries \
             WHERE normalized_key = ?1 \
             LIMIT 1;",
            params![normalized_key],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        );
        drop(conn);

        match row_result {
            Ok((word, phonetic_ipa, senses_json, synonyms_json, etymology)) => {
                let senses: Vec<DefinitionSense> = serde_json::from_str(&senses_json)?;
                let synonyms: Vec<String> = match synonyms_json {
                    Some(s) if !s.is_empty() => serde_json::from_str(&s)?,
                    _ => Vec::new(),
                };

                Ok(Some(DictionaryCard::new(
                    word,
                    phonetic_ipa,
                    senses,
                    synonyms,
                    etymology,
                    450.0,
                )))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(DictError::from(e)),
        }
    }

    /// Finds candidate dictionary cards matching `prefix`, ordered by key length then lexicographically.
    ///
    /// # Errors
    ///
    /// Returns [`DictError::Db`] or [`DictError::Json`] if querying or deserialization fails.
    pub fn find_prefix(
        &self,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<DictionaryCard>, DictError> {
        if prefix.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let escaped_prefix = escape_like_pattern(prefix);
        let pattern = format!("{escaped_prefix}%");
        let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);

        let conn = self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut stmt = conn.prepare(
            "SELECT word, phonetic_ipa, senses_json, synonyms_json, etymology \
             FROM dictionary_entries \
             WHERE normalized_key LIKE ?1 ESCAPE '\\' \
             ORDER BY LENGTH(normalized_key) ASC, normalized_key ASC \
             LIMIT ?2;",
        )?;

        let rows = stmt.query_map(params![pattern, limit_i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?;

        let mut raw_rows = Vec::new();
        for item in rows {
            raw_rows.push(item?);
        }
        drop(stmt);
        drop(conn);

        let mut results = Vec::with_capacity(raw_rows.len());
        for (word, phonetic_ipa, senses_json, synonyms_json, etymology) in raw_rows {
            let senses: Vec<DefinitionSense> = serde_json::from_str(&senses_json)?;
            let synonyms: Vec<String> = match synonyms_json {
                Some(s) if !s.is_empty() => serde_json::from_str(&s)?,
                _ => Vec::new(),
            };

            results.push(DictionaryCard::new(
                word,
                phonetic_ipa,
                senses,
                synonyms,
                etymology,
                450.0,
            ));
        }

        Ok(results)
    }
}

/// Helper function to escape `%`, `_`, and `\` characters for SQL `LIKE ... ESCAPE '\'`.
fn escape_like_pattern(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for c in input.chars() {
        if matches!(c, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
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
    fn test_storage_lifecycle_in_memory() {
        let storage = DictionaryStorage::open_in_memory().expect("open storage");
        let card = DictionaryCard::new(
            "serendipity",
            Some("/ˌserənˈdɪpədē/".to_string()),
            vec![DefinitionSense::new(
                "noun",
                "Finding good things without looking for them.",
                None,
            )],
            vec!["chance".to_string(), "fortune".to_string()],
            None,
            450.0,
        );

        storage.insert_entry(&card).expect("insert entry");

        // Exact match
        let found = storage.find_exact("serendipity").expect("query");
        assert!(found.is_some());
        let hit = found.unwrap();
        assert_eq!(hit.headword, "serendipity");
        assert_eq!(hit.synonyms.len(), 2);

        // Missing match
        let missing = storage.find_exact("unknown").expect("query");
        assert!(missing.is_none());

        // Prefix match
        let prefix_hits = storage.find_prefix("seren", 5).expect("prefix query");
        assert_eq!(prefix_hits.len(), 1);
        assert_eq!(prefix_hits[0].headword, "serendipity");
    }

    #[test]
    fn test_storage_open_default_graceful() {
        // When not present on standard system path, returns Ok(None)
        let res = DictionaryStorage::open_default();
        assert!(res.is_ok());
    }
}
