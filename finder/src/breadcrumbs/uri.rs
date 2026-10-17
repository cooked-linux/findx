//! URI-list parsing for drag-and-drop file ingestion.

use std::path::PathBuf;

/// Decodes percent-encoded character sequences (e.g. `%20` -> ` `, `%C3%A9` -> `é`).
#[must_use]
pub fn percent_decode(input: &str) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let mut iter = input.bytes();

    while let Some(b) = iter.next() {
        if b == b'%' {
            let h1 = iter.next();
            let h2 = iter.next();
            if let (Some(c1), Some(c2)) = (h1, h2) {
                let hex = [c1, c2];
                if let Ok(ascii_str) = std::str::from_utf8(&hex)
                    && let Ok(byte_val) = u8::from_str_radix(ascii_str, 16)
                {
                    bytes.push(byte_val);
                    continue;
                }
                // If invalid hex, push literal bytes
                bytes.push(b'%');
                bytes.push(c1);
                bytes.push(c2);
                continue;
            }
            bytes.push(b'%');
            if let Some(c1) = h1 {
                bytes.push(c1);
            }
            continue;
        }
        bytes.push(b);
    }

    String::from_utf8_lossy(&bytes).to_string()
}

/// Parses a `text/uri-list` payload into resolved local filesystem [`PathBuf`] items.
///
/// Handles `file://` schemes, `localhost` hostnames, percent-decoding, comments (`#`), and CRLF endings.
#[must_use]
pub fn parse_uri_list(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let stripped = line
                .strip_prefix("file://localhost")
                .or_else(|| line.strip_prefix("file://"))
                .unwrap_or(line);
            let decoded = percent_decode(stripped);
            if decoded.is_empty() {
                None
            } else {
                Some(PathBuf::from(decoded))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_uri_list_basic() {
        let raw = "file:///home/user/document.txt\r\nfile:///home/user/photo.png\r\n";
        let paths = parse_uri_list(raw);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], PathBuf::from("/home/user/document.txt"));
        assert_eq!(paths[1], PathBuf::from("/home/user/photo.png"));
    }

    #[test]
    fn test_parse_uri_list_percent_encoded_spaces_and_utf8() {
        let raw = "file:///home/user/My%20Documents/caf%C3%A9.txt\n";
        let paths = parse_uri_list(raw);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], PathBuf::from("/home/user/My Documents/café.txt"));
    }

    #[test]
    fn test_parse_uri_list_comments_and_blank_lines() {
        let raw = "# Freedesktop drop\n\nfile:///tmp/test.rs\n# another comment\nfile:///var/log\n";
        let paths = parse_uri_list(raw);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], PathBuf::from("/tmp/test.rs"));
        assert_eq!(paths[1], PathBuf::from("/var/log"));
    }
}
