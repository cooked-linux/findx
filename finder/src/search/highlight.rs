//! Pango markup formatting and matched text highlighting.

use super::parser::parse_query;
use super::types::ParsedQuery;

const PANGO_HIGHLIGHT_OPEN: &str =
    "<span background=\"#f5c211\" foreground=\"#000000\" weight=\"bold\">";
const PANGO_HIGHLIGHT_CLOSE: &str = "</span>";
const MAX_SNIPPET_LENGTH: usize = 100;

/// Escapes XML and Pango reserved control characters.
#[must_use]
pub fn escape_pango(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

/// Highlights matched terms and exact phrases from [`ParsedQuery`] in the given text using Pango markup.
#[must_use]
pub fn highlight_matched_text(text: &str, query: &ParsedQuery) -> String {
    let ranges = find_all_match_ranges(text, query);
    apply_highlight_spans(text, &ranges)
}

/// Convenience wrapper parsing raw query and highlighting matches in `text`.
#[must_use]
pub fn highlight_query(text: &str, raw_query: &str) -> String {
    let query = parse_query(raw_query);
    highlight_matched_text(text, &query)
}

/// Formats and highlights a content search snippet (max 100 chars window) with Pango markup.
#[must_use]
pub fn format_highlighted_snippet(content: &str, query: &ParsedQuery) -> String {
    if content.is_empty() {
        return String::new();
    }

    let ranges = find_all_match_ranges(content, query);
    if ranges.is_empty() {
        // No match found, return leading snippet escaped
        let end_idx = truncate_char_boundary(content, MAX_SNIPPET_LENGTH);
        let prefix = &content[..end_idx];
        let suffix = if end_idx < content.len() { "..." } else { "" };
        return format!("{}{}", escape_pango(prefix), suffix);
    }

    // Anchor around the first match
    let (first_match_start, first_match_end) = ranges[0];
    let match_len = first_match_end - first_match_start;
    let budget = MAX_SNIPPET_LENGTH.saturating_sub(match_len);
    let half_budget = budget / 2;

    let mut start_idx = first_match_start.saturating_sub(half_budget);
    while start_idx > 0 && !content.is_char_boundary(start_idx) {
        start_idx -= 1;
    }

    let mut end_idx = (first_match_end + half_budget).min(content.len());
    while end_idx < content.len() && !content.is_char_boundary(end_idx) {
        end_idx += 1;
    }

    let prefix_ellipsis = if start_idx > 0 { "..." } else { "" };
    let suffix_ellipsis = if end_idx < content.len() { "..." } else { "" };

    let window_slice = &content[start_idx..end_idx];
    let window_ranges: Vec<(usize, usize)> = ranges
        .iter()
        .filter_map(|&(s, e)| {
            if e <= start_idx || s >= end_idx {
                None
            } else {
                let clamped_s = s.max(start_idx) - start_idx;
                let clamped_e = e.min(end_idx) - start_idx;
                Some((clamped_s, clamped_e))
            }
        })
        .collect();

    let highlighted_window = apply_highlight_spans(window_slice, &window_ranges);
    format!("{prefix_ellipsis}{highlighted_window}{suffix_ellipsis}")
}

fn truncate_char_boundary(s: &str, max_bytes: usize) -> usize {
    let mut idx = max_bytes.min(s.len());
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// Collects and merges all byte ranges in `text` matching query terms and phrases.
fn find_all_match_ranges(text: &str, query: &ParsedQuery) -> Vec<(usize, usize)> {
    let mut raw_ranges = Vec::new();
    let text_lower = text.to_ascii_lowercase();

    // Exact phrases
    for phrase in &query.exact_phrases {
        collect_substring_ranges(&text_lower, &phrase.to_ascii_lowercase(), &mut raw_ranges);
    }

    // Free terms
    for term in &query.terms {
        collect_substring_ranges(&text_lower, &term.to_ascii_lowercase(), &mut raw_ranges);
    }

    // Wildcards: extract alphanumeric word tokens from pattern
    for wc in &query.wildcards {
        let clean_tokens: Vec<String> = wc
            .split(['*', '?'])
            .filter(|s| s.len() >= 2)
            .map(str::to_ascii_lowercase)
            .collect();

        for token in clean_tokens {
            collect_substring_ranges(&text_lower, &token, &mut raw_ranges);
        }
    }

    // Sort by start offset, then end offset
    raw_ranges.sort_unstable_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    // Reconcile and merge overlapping or contiguous spans
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in raw_ranges {
        if start >= end || end > text.len() {
            continue;
        }

        if let Some(last) = merged.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
            continue;
        }
        merged.push((start, end));
    }

    merged
}

fn collect_substring_ranges(
    text_lower: &str,
    needle_lower: &str,
    ranges: &mut Vec<(usize, usize)>,
) {
    if needle_lower.is_empty() {
        return;
    }

    let mut start_search = 0;
    while let Some(rel_pos) = text_lower[start_search..].find(needle_lower) {
        let abs_start = start_search + rel_pos;
        let abs_end = abs_start + needle_lower.len();
        ranges.push((abs_start, abs_end));
        start_search = abs_start + 1;
        while start_search < text_lower.len() && !text_lower.is_char_boundary(start_search) {
            start_search += 1;
        }
    }
}

fn apply_highlight_spans(text: &str, ranges: &[(usize, usize)]) -> String {
    let mut result = String::with_capacity(text.len() + ranges.len() * 80);
    let mut last_idx = 0;

    for &(start, end) in ranges {
        if start > last_idx && start <= text.len() {
            result.push_str(&escape_pango(&text[last_idx..start]));
        }

        if start < text.len() && end <= text.len() && start < end {
            let matched_segment = &text[start..end];
            result.push_str(PANGO_HIGHLIGHT_OPEN);
            result.push_str(&escape_pango(matched_segment));
            result.push_str(PANGO_HIGHLIGHT_CLOSE);
            last_idx = end;
        }
    }

    if last_idx < text.len() {
        result.push_str(&escape_pango(&text[last_idx..]));
    }

    result
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_pango() {
        assert_eq!(
            escape_pango("a & b < c > d \"e\" 'f'"),
            "a &amp; b &lt; c &gt; d &quot;e&quot; &apos;f&apos;"
        );
    }

    #[test]
    fn test_highlight_with_xml_entities() {
        let filename = "report <Q1 & Q2>.pdf";
        let query = parse_query("Q1");
        let highlighted = highlight_matched_text(filename, &query);

        assert_eq!(
            highlighted,
            "report &lt;<span background=\"#f5c211\" foreground=\"#000000\" weight=\"bold\">Q1</span> &amp; Q2&gt;.pdf"
        );
    }

    #[test]
    fn test_overlapping_keywords_merge() {
        let text = "catchy";
        let query = parse_query("cat catch");
        let highlighted = highlight_matched_text(text, &query);

        assert_eq!(
            highlighted,
            "<span background=\"#f5c211\" foreground=\"#000000\" weight=\"bold\">catch</span>y"
        );
    }

    #[test]
    fn test_case_insensitive_casing_preservation() {
        let text = "FoObAr";
        let query = parse_query("foo");
        let highlighted = highlight_matched_text(text, &query);

        assert_eq!(
            highlighted,
            "<span background=\"#f5c211\" foreground=\"#000000\" weight=\"bold\">FoO</span>bAr"
        );
    }

    #[test]
    fn test_snippet_highlighting() {
        let content =
            "The beginning text here is followed by secret project details and trailing sentences.";
        let query = parse_query("\"secret project\"");
        let snippet = format_highlighted_snippet(content, &query);

        assert!(snippet.contains("<span background=\"#f5c211\" foreground=\"#000000\" weight=\"bold\">secret project</span>"));
    }
}
