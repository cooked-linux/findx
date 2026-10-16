//! Natural ordering (alphanumeric chunked collation) for file paths and names.
//!
//! Implements human-friendly natural sort order where numeric sequences are sorted
//! by numeric value rather than pure ASCII code points (e.g. `file1`, `file2`, `file10`, `file20`),
//! with case-insensitive collation and deterministic tie-breaking.

use std::cmp::Ordering;

/// Splits a string into the next homogeneous chunk (either all ASCII digits or all non-digits)
/// and the remaining string slice.
fn next_chunk(s: &str) -> (&str, &str) {
    if s.is_empty() {
        return ("", "");
    }

    let mut char_indices = s.char_indices();
    let Some((_, first_char)) = char_indices.next() else {
        return ("", "");
    };

    let is_digit = first_char.is_ascii_digit();
    let mut end = s.len();

    for (idx, ch) in char_indices {
        if ch.is_ascii_digit() != is_digit {
            end = idx;
            break;
        }
    }

    (&s[..end], &s[end..])
}

/// Compares two strings using natural numeric collation.
///
/// # Examples
/// ```
/// use std::cmp::Ordering;
/// use shared::fs::natural_cmp;
///
/// assert_eq!(natural_cmp("file2", "file10"), Ordering::Less);
/// assert_eq!(natural_cmp("file10", "file20"), Ordering::Less);
/// assert_eq!(natural_cmp("apple", "Banana"), Ordering::Less);
/// ```
#[must_use]
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut rem_a = a;
    let mut rem_b = b;
    let mut tie_breaker = Ordering::Equal;

    while !rem_a.is_empty() && !rem_b.is_empty() {
        let (ca, next_a) = next_chunk(rem_a);
        let (cb, next_b) = next_chunk(rem_b);
        rem_a = next_a;
        rem_b = next_b;

        let is_digit_a = ca.chars().next().is_some_and(|c| c.is_ascii_digit());
        let is_digit_b = cb.chars().next().is_some_and(|c| c.is_ascii_digit());

        if is_digit_a && is_digit_b {
            let trimmed_a = ca.trim_start_matches('0');
            let trimmed_b = cb.trim_start_matches('0');

            // Compare numerical values without overflow: longer string after trimming is larger
            let ord = trimmed_a
                .len()
                .cmp(&trimmed_b.len())
                .then_with(|| trimmed_a.cmp(trimmed_b));

            if ord != Ordering::Equal {
                return ord;
            }

            // Numerically equivalent (e.g. "01" vs "1"): record leading zeros as tie breaker
            if tie_breaker == Ordering::Equal {
                tie_breaker = ca.len().cmp(&cb.len());
            }
        } else if !is_digit_a && !is_digit_b {
            let mut chars_a = ca.chars();
            let mut chars_b = cb.chars();

            loop {
                match (chars_a.next(), chars_b.next()) {
                    (Some(cha), Some(chb)) => {
                        let low_a = cha.to_lowercase();
                        let low_b = chb.to_lowercase();
                        let ord = low_a.cmp(low_b);

                        if ord != Ordering::Equal {
                            return ord;
                        }

                        if tie_breaker == Ordering::Equal && cha != chb {
                            tie_breaker = cha.cmp(&chb);
                        }
                    }
                    (None, Some(_)) => return Ordering::Less,
                    (Some(_), None) => return Ordering::Greater,
                    (None, None) => break,
                }
            }
        } else {
            // One is digit, one is text: compare their first characters case-insensitively
            let cha = ca.chars().next().unwrap_or('\0');
            let chb = cb.chars().next().unwrap_or('\0');
            let low_a = cha.to_lowercase();
            let low_b = chb.to_lowercase();
            let ord = low_a.cmp(low_b);

            if ord != Ordering::Equal {
                return ord;
            }
            return cha.cmp(&chb);
        }
    }

    match (rem_a.is_empty(), rem_b.is_empty()) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => tie_breaker.then_with(|| a.cmp(b)),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn test_numeric_sequence() {
        let mut files = vec!["file20", "file1", "file10", "file2"];
        files.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(files, vec!["file1", "file2", "file10", "file20"]);
    }

    #[test]
    fn test_case_insensitive_collation() {
        let mut words = vec!["cherry", "apple", "Banana", "date"];
        words.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(words, vec!["apple", "Banana", "cherry", "date"]);
    }

    #[test]
    fn test_case_sensitive_tie_breaking() {
        assert_eq!(natural_cmp("file", "File"), Ordering::Greater);
        assert_eq!(natural_cmp("File", "file"), Ordering::Less);
        assert_eq!(natural_cmp("abc", "abc"), Ordering::Equal);
    }

    #[test]
    fn test_leading_zeros() {
        assert_eq!(natural_cmp("file1", "file01"), Ordering::Less);
        assert_eq!(natural_cmp("007", "7"), Ordering::Greater);
        assert_eq!(natural_cmp("img02.png", "img10.png"), Ordering::Less);
    }

    #[test]
    fn test_complex_versions() {
        let mut versions = vec!["v1.10.0", "v1.2.0", "v1.2.1", "v1.1.9"];
        versions.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(versions, vec!["v1.1.9", "v1.2.0", "v1.2.1", "v1.10.0"]);
    }

    #[test]
    fn test_empty_strings() {
        assert_eq!(natural_cmp("", ""), Ordering::Equal);
        assert_eq!(natural_cmp("", "a"), Ordering::Less);
        assert_eq!(natural_cmp("a", ""), Ordering::Greater);
    }
}
