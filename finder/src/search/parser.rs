//! Search query parser, operator validation, and filtering evaluation.

use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};
use shared::models::mime::MimeCategory;

use super::types::{
    AttributeFilter, DateConstraint, ParsedQuery, SearchMode, SearchToken, SizeFilter, SizeOperator,
};

/// Parses a raw user query string into a [`ParsedQuery`].
#[must_use]
pub fn parse_query(input: &str) -> ParsedQuery {
    let mut parsed = ParsedQuery {
        raw: input.to_string(),
        terms: Vec::new(),
        exact_phrases: Vec::new(),
        wildcards: Vec::new(),
        attributes: Vec::new(),
        negated_terms: Vec::new(),
        mode: SearchMode::NameMatches,
        include_system_files: false,
    };

    let tokens = tokenize(input);
    let mut iter = tokens.into_iter();

    while let Some(token) = iter.next() {
        match token {
            SearchToken::Word(w) => {
                if w.eq_ignore_ascii_case("NOT") {
                    if let Some(
                        SearchToken::Word(nw)
                        | SearchToken::Wildcard(nw)
                        | SearchToken::ExactPhrase(nw),
                    ) = iter.next()
                    {
                        parsed.negated_terms.push(nw);
                    }
                } else if let Some(stripped) = w.strip_prefix('-') {
                    if !stripped.is_empty() {
                        parsed.negated_terms.push(stripped.to_string());
                    }
                } else if is_attribute_syntax(&w) {
                    if let Some(attr) = parse_attribute_token(&w) {
                        parsed.attributes.push(attr);
                    } else {
                        parsed.terms.push(w);
                    }
                } else {
                    parsed.terms.push(w);
                }
            }
            SearchToken::ExactPhrase(phrase) => {
                if let Some(stripped) = phrase.strip_prefix('-') {
                    if stripped.is_empty() {
                        parsed.exact_phrases.push(phrase);
                    } else {
                        parsed.negated_terms.push(stripped.to_string());
                    }
                } else {
                    parsed.exact_phrases.push(phrase);
                }
            }
            SearchToken::Wildcard(pat) => {
                if let Some(stripped) = pat.strip_prefix('-') {
                    if stripped.is_empty() {
                        parsed.wildcards.push(pat);
                    } else {
                        parsed.negated_terms.push(stripped.to_string());
                    }
                } else {
                    parsed.wildcards.push(pat);
                }
            }
            SearchToken::Attribute(attr) => {
                parsed.attributes.push(attr);
            }
            SearchToken::Negated(neg) => {
                parsed.negated_terms.push(neg);
            }
            SearchToken::And | SearchToken::Or => {
                // Preserved for boolean grammar, do not treat as free text term
            }
        }
    }

    parsed
}

fn is_attribute_syntax(w: &str) -> bool {
    let lower = w.to_ascii_lowercase();
    lower.starts_with("kind:")
        || lower.starts_with("size:")
        || lower.starts_with("tag:")
        || lower.starts_with("date:")
}

fn tokenize(input: &str) -> Vec<SearchToken> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Skip whitespace
        while i < len && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= len {
            break;
        }

        // Quoted phrase (balanced or unclosed at end of input)
        if chars[i] == '"' {
            i += 1;
            let start = i;
            while i < len && chars[i] != '"' {
                i += 1;
            }
            let phrase: String = chars[start..i].iter().collect();
            if i < len && chars[i] == '"' {
                i += 1;
            }
            let trimmed = phrase.trim();
            if !trimmed.is_empty() {
                tokens.push(SearchToken::ExactPhrase(trimmed.to_string()));
            }
            continue;
        }

        // Word or attribute
        let start = i;
        let mut in_quote = false;
        while i < len {
            let c = chars[i];
            if c == '"' {
                in_quote = !in_quote;
                i += 1;
            } else if c.is_whitespace() && !in_quote {
                break;
            } else {
                i += 1;
            }
        }

        let raw_token: String = chars[start..i].iter().collect();
        if raw_token.is_empty() {
            continue;
        }

        // Check for attribute prefix with potential internal quotes (e.g. tag:"Project Alpha")
        if let Some(attr) = parse_attribute_token(&raw_token) {
            tokens.push(SearchToken::Attribute(attr));
        } else if raw_token.eq_ignore_ascii_case("AND") {
            tokens.push(SearchToken::And);
        } else if raw_token.eq_ignore_ascii_case("OR") {
            tokens.push(SearchToken::Or);
        } else if raw_token.contains('*') || raw_token.contains('?') {
            tokens.push(SearchToken::Wildcard(raw_token));
        } else {
            tokens.push(SearchToken::Word(raw_token));
        }
    }

    tokens
}

fn parse_attribute_token(token: &str) -> Option<AttributeFilter> {
    let colon_idx = token.find(':')?;
    let prefix = &token[..colon_idx].to_ascii_lowercase();
    let mut value = &token[colon_idx + 1..];

    // Strip wrapping quotes if present
    if value.starts_with('"') {
        value = value.strip_prefix('"').unwrap_or(value);
        if value.ends_with('"') {
            value = value.strip_suffix('"').unwrap_or(value);
        }
    }

    match prefix.as_str() {
        "kind" => parse_kind_attribute(value),
        "size" => parse_size_attribute(value),
        "tag" => {
            if value.trim().is_empty() {
                None
            } else {
                Some(AttributeFilter::Tag(value.trim().to_string()))
            }
        }
        "date" => parse_date_attribute(value),
        _ => None,
    }
}

fn parse_kind_attribute(val: &str) -> Option<AttributeFilter> {
    let lower = val.trim().to_ascii_lowercase();
    match lower.as_str() {
        "image" | "photo" | "picture" => Some(AttributeFilter::Kind(MimeCategory::Image)),
        "doc" | "document" | "pdf" | "text" => Some(AttributeFilter::Kind(MimeCategory::Document)),
        "code" | "source" | "rust" => Some(AttributeFilter::Kind(MimeCategory::SourceCode)),
        "audio" | "music" | "song" => Some(AttributeFilter::Kind(MimeCategory::Audio)),
        "video" | "movie" => Some(AttributeFilter::Kind(MimeCategory::Video)),
        "archive" | "zip" | "tar" => Some(AttributeFilter::Kind(MimeCategory::Archive)),
        "folder" | "dir" | "directory" => Some(AttributeFilter::Kind(MimeCategory::Folder)),
        _ => None,
    }
}

#[allow(clippy::option_if_let_else)]
fn parse_size_attribute(val: &str) -> Option<AttributeFilter> {
    let val = val.trim();
    if val.is_empty() {
        return None;
    }

    // Check for range syntax "min..max"
    if let Some((min_str, max_str)) = val.split_once("..") {
        let min_bytes = parse_size_bytes(min_str.trim())?;
        let max_bytes = parse_size_bytes(max_str.trim())?;
        return Some(AttributeFilter::Size(SizeFilter {
            op: SizeOperator::Between(min_bytes, max_bytes),
            bytes: min_bytes,
        }));
    }

    let (op, num_str) = if let Some(rest) = val.strip_prefix(">=") {
        (SizeOperator::GreaterThanOrEqual, rest)
    } else if let Some(rest) = val.strip_prefix('>') {
        (SizeOperator::GreaterThan, rest)
    } else if let Some(rest) = val.strip_prefix("<=") {
        (SizeOperator::LessThanOrEqual, rest)
    } else if let Some(rest) = val.strip_prefix('<') {
        (SizeOperator::LessThan, rest)
    } else if let Some(rest) = val.strip_prefix('=') {
        (SizeOperator::Equal, rest)
    } else {
        (SizeOperator::Equal, val)
    };

    let bytes = parse_size_bytes(num_str.trim())?;
    Some(AttributeFilter::Size(SizeFilter { op, bytes }))
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn parse_size_bytes(input: &str) -> Option<u64> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let num_end = trimmed
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(trimmed.len());

    let num_part = &trimmed[..num_end];
    let unit_part = trimmed[num_end..].trim().to_ascii_uppercase();

    let num: f64 = num_part.parse().ok()?;
    if num < 0.0 {
        return None;
    }

    let multiplier: f64 = match unit_part.as_str() {
        "" | "B" => 1.0,
        "K" | "KB" | "KIB" => 1024.0,
        "M" | "MB" | "MIB" => 1024.0 * 1024.0,
        "G" | "GB" | "GIB" => 1024.0 * 1024.0 * 1024.0,
        "T" | "TB" | "TIB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };

    Some((num * multiplier) as u64)
}

#[allow(clippy::option_if_let_else)]
fn parse_date_attribute(val: &str) -> Option<AttributeFilter> {
    let trimmed = val.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    match lower.as_str() {
        "today" => return Some(AttributeFilter::Date(DateConstraint::Today)),
        "yesterday" => return Some(AttributeFilter::Date(DateConstraint::Yesterday)),
        "thisweek" => return Some(AttributeFilter::Date(DateConstraint::ThisWeek)),
        "thismonth" => return Some(AttributeFilter::Date(DateConstraint::ThisMonth)),
        "thisyear" => return Some(AttributeFilter::Date(DateConstraint::ThisYear)),
        _ => {}
    }

    let (is_before, date_str) = if let Some(rest) = trimmed.strip_prefix('>') {
        (false, rest.trim_start_matches('=').trim())
    } else if let Some(rest) = trimmed.strip_prefix('<') {
        (true, rest.trim_start_matches('=').trim())
    } else {
        (true, trimmed)
    };

    let dt = parse_iso_datetime(date_str)?;
    if is_before {
        Some(AttributeFilter::Date(DateConstraint::Before(dt)))
    } else {
        Some(AttributeFilter::Date(DateConstraint::After(dt)))
    }
}

fn parse_iso_datetime(s: &str) -> Option<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }

    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let naive = date.and_hms_opt(0, 0, 0)?;
        return Some(Utc.from_utc_datetime(&naive));
    }

    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(Utc.from_utc_datetime(&naive));
    }

    None
}

impl ParsedQuery {
    /// Translates the parsed query into a sanitized SQLite FTS5 MATCH expression.
    #[must_use]
    pub fn to_fts5_match(&self) -> String {
        let mut parts = Vec::new();

        // Exact phrases: wrapped in double quotes
        for phrase in &self.exact_phrases {
            let sanitized: String = phrase.chars().filter(|&c| c != '"' && c != '*').collect();
            let trimmed = sanitized.trim();
            if !trimmed.is_empty() {
                parts.push(format!("\"{trimmed}\""));
            }
        }

        // Terms: sanitized alphanumeric prefix tokens
        for term in &self.terms {
            let sanitized: String = term
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            let trimmed = sanitized.trim_matches(|c| c == '-' || c == '_');
            if !trimmed.is_empty() {
                let upper = trimmed.to_ascii_uppercase();
                if upper == "AND" || upper == "OR" || upper == "NOT" || upper == "NEAR" {
                    parts.push(format!("\"{trimmed}\"*"));
                } else {
                    parts.push(format!("{trimmed}*"));
                }
            }
        }

        // Wildcards: strip wildcard chars and prefix match
        for wc in &self.wildcards {
            let cleaned: String = wc.chars().filter(|c| c.is_alphanumeric()).collect();
            if !cleaned.is_empty() {
                parts.push(format!("{cleaned}*"));
            }
        }

        // Negated terms: NOT <term>*
        for neg in &self.negated_terms {
            let cleaned: String = neg
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            let trimmed = cleaned.trim_matches(|c| c == '-' || c == '_');
            if !trimmed.is_empty() {
                parts.push(format!("NOT {trimmed}*"));
            }
        }

        parts.join(" ")
    }

    /// Evaluates whether a filename satisfies all terms, wildcards, exact phrases, and negations.
    #[must_use]
    pub fn matches_filename(&self, filename: &str) -> bool {
        let lower_filename = filename.to_ascii_lowercase();

        // Negations must not appear in filename
        for neg in &self.negated_terms {
            if lower_filename.contains(&neg.to_ascii_lowercase()) {
                return false;
            }
        }

        // Terms must all be substrings
        for term in &self.terms {
            if !lower_filename.contains(&term.to_ascii_lowercase()) {
                return false;
            }
        }

        // Exact phrases must all be substrings
        for phrase in &self.exact_phrases {
            if !lower_filename.contains(&phrase.to_ascii_lowercase()) {
                return false;
            }
        }

        // Wildcards must match
        for pattern in &self.wildcards {
            if !glob_match(pattern, filename) {
                return false;
            }
        }

        true
    }

    /// Evaluates whether an item's metadata and tags satisfy attribute filters.
    #[must_use]
    #[allow(clippy::similar_names)]
    pub fn matches_attributes(
        &self,
        size_bytes: u64,
        mtime: i64,
        mime: &str,
        tags: &[String],
    ) -> bool {
        for attr in &self.attributes {
            match attr {
                AttributeFilter::Kind(expected_cat) => {
                    let actual_cat = MimeCategory::from_mime_str(mime);
                    if actual_cat != *expected_cat {
                        return false;
                    }
                }
                AttributeFilter::Size(size_filter) => {
                    if !size_filter.matches(size_bytes) {
                        return false;
                    }
                }
                AttributeFilter::Tag(expected_tag) => {
                    let found = tags.iter().any(|t| t.eq_ignore_ascii_case(expected_tag));
                    if !found {
                        return false;
                    }
                }
                AttributeFilter::Date(date_constraint) => {
                    let item_dt = Utc
                        .timestamp_opt(mtime, 0)
                        .single()
                        .unwrap_or_else(Utc::now);
                    let now = Utc::now();

                    match date_constraint {
                        DateConstraint::Today => {
                            if item_dt.date_naive() != now.date_naive() {
                                return false;
                            }
                        }
                        DateConstraint::Yesterday => {
                            let yesterday = (now - Duration::days(1)).date_naive();
                            if item_dt.date_naive() != yesterday {
                                return false;
                            }
                        }
                        DateConstraint::ThisWeek => {
                            if item_dt.iso_week() != now.iso_week() || item_dt.year() != now.year()
                            {
                                return false;
                            }
                        }
                        DateConstraint::ThisMonth => {
                            if item_dt.year() != now.year() || item_dt.month() != now.month() {
                                return false;
                            }
                        }
                        DateConstraint::ThisYear => {
                            if item_dt.year() != now.year() {
                                return false;
                            }
                        }
                        DateConstraint::Before(cutoff) => {
                            if item_dt >= *cutoff {
                                return false;
                            }
                        }
                        DateConstraint::After(cutoff) => {
                            if item_dt <= *cutoff {
                                return false;
                            }
                        }
                    }
                }
            }
        }

        true
    }
}

/// Case-insensitive glob matching for `*` and `?`.
fn glob_match(pattern: &str, text: &str) -> bool {
    let p_chars: Vec<char> = pattern.to_ascii_lowercase().chars().collect();
    let t_chars: Vec<char> = text.to_ascii_lowercase().chars().collect();

    let p_len = p_chars.len();
    let t_len = t_chars.len();

    let mut p_idx = 0;
    let mut t_idx = 0;
    let mut star_idx = None;
    let mut match_idx = 0;

    while t_idx < t_len {
        if p_idx < p_len && (p_chars[p_idx] == '?' || p_chars[p_idx] == t_chars[t_idx]) {
            p_idx += 1;
            t_idx += 1;
        } else if p_idx < p_len && p_chars[p_idx] == '*' {
            star_idx = Some(p_idx);
            match_idx = t_idx;
            p_idx += 1;
        } else if let Some(star) = star_idx {
            p_idx = star + 1;
            match_idx += 1;
            t_idx = match_idx;
        } else {
            return false;
        }
    }

    while p_idx < p_len && p_chars[p_idx] == '*' {
        p_idx += 1;
    }

    p_idx == p_len
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_token_parsing() {
        let q = parse_query("budget \"Q3 2026\" kind:pdf size:>1MB tag:Finance -draft");
        assert_eq!(q.terms, vec!["budget"]);
        assert_eq!(q.exact_phrases, vec!["Q3 2026"]);
        assert_eq!(q.negated_terms, vec!["draft"]);
        assert_eq!(q.attributes.len(), 3);
        assert_eq!(
            q.attributes[0],
            AttributeFilter::Kind(MimeCategory::Document)
        );
        assert_eq!(
            q.attributes[1],
            AttributeFilter::Size(SizeFilter {
                op: SizeOperator::GreaterThan,
                bytes: 1024 * 1024,
            })
        );
        assert_eq!(q.attributes[2], AttributeFilter::Tag("Finance".to_string()));
    }

    #[test]
    fn test_unclosed_quotes() {
        let q = parse_query("find \"unclosed phrase at end");
        assert_eq!(q.terms, vec!["find"]);
        assert_eq!(q.exact_phrases, vec!["unclosed phrase at end"]);
    }

    #[test]
    fn test_malformed_syntax_resilience() {
        let q = parse_query("kind: size:abc \"\" \"\"\"\" tag: date:");
        // Should not panic, invalid size falls back or is ignored
        assert!(q.exact_phrases.is_empty());
    }

    #[test]
    fn test_wildcard_parsing() {
        let q = parse_query("*.png release-v*.tar.gz");
        assert_eq!(q.wildcards, vec!["*.png", "release-v*.tar.gz"]);

        let q1 = parse_query("*.png");
        assert!(q1.matches_filename("image.png"));
        assert!(q1.matches_filename("IMAGE.PNG"));
        assert!(!q1.matches_filename("image.jpg"));

        let q2 = parse_query("release-v*.tar.gz");
        assert!(q2.matches_filename("release-v1.2.tar.gz"));
        assert!(!q2.matches_filename("debug-v1.2.tar.gz"));
    }

    #[test]
    fn test_not_keyword_negation() {
        let q = parse_query("project NOT archive");
        assert_eq!(q.terms, vec!["project"]);
        assert_eq!(q.negated_terms, vec!["archive"]);
        assert!(q.matches_filename("project_notes.txt"));
        assert!(!q.matches_filename("project_archive.zip"));
    }

    #[test]
    fn test_fts5_match_expression() {
        let q = parse_query("budget \"Q3 2026\" -draft");
        let fts5 = q.to_fts5_match();
        assert!(fts5.contains("\"Q3 2026\""));
        assert!(fts5.contains("budget*"));
        assert!(fts5.contains("NOT draft*"));
    }

    #[test]
    fn test_matches_attributes() {
        let q = parse_query("kind:rust size:>100 tag:Work");
        let tags = vec!["Work".to_string(), "Rust".to_string()];

        assert!(q.matches_attributes(500, 1_700_000_000, "text/rust", &tags));
        // Fails size
        assert!(!q.matches_attributes(50, 1_700_000_000, "text/rust", &tags));
        // Fails kind
        assert!(!q.matches_attributes(500, 1_700_000_000, "image/png", &tags));
        // Fails tag
        let other_tags = vec!["Personal".to_string()];
        assert!(!q.matches_attributes(500, 1_700_000_000, "text/rust", &other_tags));
    }
}
