//! Scoring engine for matching queries against desktop applications.

use super::DesktopApp;

/// Scores an application candidate against a search query using a 7-tier ranking algorithm:
///
/// - **Tier 1 (1000.0):** Exact match on Name or executable basename.
/// - **Tier 2 (800.0 - 850.0):** Exact prefix match on Name.
/// - **Tier 3 (600.0 - 640.0):** Acronym / Initials match (e.g., "gc" -> "Google Chrome").
/// - **Tier 4 (500.0 - 530.0):** Word boundary prefix match (e.g., "calc" -> "`LibreOffice` Calc").
/// - **Tier 5 (300.0 / 400.0):** Executable basename prefix (400.0) or `GenericName` substring (300.0).
/// - **Tier 6 (150.0 / 200.0):** Keyword prefix (200.0) or Category prefix (150.0).
/// - **Tier 7 (10.0 - 100.0):** Substring fallback offset score.
///
/// Returns `0.0` if no tier matches.
#[must_use]
pub fn calculate_text_score(query: &str, app: &DesktopApp) -> f64 {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return 0.0;
    }

    let name = app.name.to_lowercase();
    let exec_binary = app.exec_sanitized.split_whitespace().next().unwrap_or("");
    let exec_base = std::path::Path::new(exec_binary)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Tier 1: Exact match (1000.0)
    if name == q || exec_base == q {
        return 1000.0;
    }

    // Tier 2: Exact prefix match (800.0 + length ratio)
    if name.starts_with(&q) {
        #[expect(clippy::cast_precision_loss)]
        let ratio = q.len() as f64 / (name.len().max(1) as f64);
        return ratio.mul_add(50.0, 800.0);
    }

    // Tier 3: Acronym / Initials match (600.0 + ratio)
    // e.g., "gc" -> "Google Chrome", "ff" -> "Firefox", "vs" -> "Visual Studio"
    let words: Vec<&str> = name
        .split(|c: char| c.is_whitespace() || c == '-' || c == '_')
        .filter(|w| !w.is_empty())
        .collect();
    let acronym: String = words
        .iter()
        .filter_map(|w| w.chars().next())
        .collect::<String>()
        .to_lowercase();

    if !acronym.is_empty() && acronym.starts_with(&q) {
        #[expect(clippy::cast_precision_loss)]
        let ratio = q.len() as f64 / (acronym.len().max(1) as f64);
        return ratio.mul_add(40.0, 600.0);
    }

    // Tier 4: Word boundary prefix match (500.0 + ratio)
    // e.g., "calc" -> "LibreOffice Calc"
    for word in &words {
        if word.starts_with(&q) {
            #[expect(clippy::cast_precision_loss)]
            let ratio = q.len() as f64 / (word.len().max(1) as f64);
            return ratio.mul_add(30.0, 500.0);
        }
    }

    // Tier 5: Executable or GenericName match (400.0 / 300.0)
    if exec_base.starts_with(&q) {
        return 400.0;
    }
    if let Some(ref gn) = app.generic_name
        && gn.to_lowercase().contains(&q)
    {
        return 300.0;
    }

    // Tier 6: Keywords & Category match (200.0 / 150.0)
    for kw in &app.keywords {
        if kw.to_lowercase().starts_with(&q) {
            return 200.0;
        }
    }
    for cat in &app.categories {
        if cat.to_lowercase().starts_with(&q) {
            return 150.0;
        }
    }

    // Tier 7: Substring fallback (100.0 - offset)
    if let Some(idx) = name.find(&q) {
        #[expect(clippy::cast_precision_loss)]
        let offset = idx as f64;
        return (100.0 - offset).max(10.0);
    }

    0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_test_app(
        name: &str,
        exec: &str,
        generic: Option<&str>,
        keywords: Vec<&str>,
        categories: Vec<&str>,
    ) -> DesktopApp {
        DesktopApp {
            desktop_id: format!("{name}.desktop"),
            path: PathBuf::from(format!("/usr/share/applications/{name}.desktop")),
            name: name.to_string(),
            generic_name: generic.map(ToString::to_string),
            comment: None,
            exec_raw: exec.to_string(),
            exec_sanitized: exec.to_string(),
            icon_name: None,
            keywords: keywords.into_iter().map(ToString::to_string).collect(),
            categories: categories.into_iter().map(ToString::to_string).collect(),
            mime_types: Vec::new(),
            is_terminal: false,
            no_display: false,
            hidden: false,
            only_show_in: Vec::new(),
            not_show_in: Vec::new(),
        }
    }

    #[test]
    fn test_tier1_exact_match() {
        let app = make_test_app(
            "Google Chrome",
            "google-chrome-stable",
            None,
            vec![],
            vec![],
        );
        assert_eq!(calculate_text_score("Google Chrome", &app), 1000.0);
        assert_eq!(calculate_text_score("google-chrome-stable", &app), 1000.0);
    }

    #[test]
    fn test_tier2_prefix_match() {
        let app = make_test_app("Chrome", "chrome", None, vec![], vec![]);
        let score = calculate_text_score("chr", &app);
        assert!(score >= 800.0 && score < 1000.0);
    }

    #[test]
    fn test_tier3_acronym_match() {
        let app = make_test_app("Google Chrome", "google-chrome", None, vec![], vec![]);
        let score = calculate_text_score("gc", &app);
        assert!(score >= 600.0 && score < 800.0);
    }

    #[test]
    fn test_tier4_word_boundary_match() {
        let app = make_test_app("LibreOffice Calc", "soffice", None, vec![], vec![]);
        let score = calculate_text_score("calc", &app);
        assert!(score >= 500.0 && score < 600.0);
    }

    #[test]
    fn test_tier5_exec_or_generic_match() {
        let app = make_test_app(
            "Text Editor",
            "/usr/bin/gedit",
            Some("Note Editor"),
            vec![],
            vec![],
        );
        let score_exec = calculate_text_score("ged", &app);
        assert_eq!(score_exec, 400.0);

        let score_gen = calculate_text_score("note", &app);
        assert_eq!(score_gen, 300.0);
    }

    #[test]
    fn test_tier6_keywords_and_categories() {
        let app = make_test_app(
            "GIMP",
            "gimp",
            None,
            vec!["photo", "paint"],
            vec!["Graphics"],
        );
        assert_eq!(calculate_text_score("pho", &app), 200.0);
        assert_eq!(calculate_text_score("grap", &app), 150.0);
    }

    #[test]
    fn test_tier7_substring_fallback() {
        let app = make_test_app("SuperDuperApp", "super", None, vec![], vec![]);
        let score = calculate_text_score("duper", &app);
        assert!(score >= 10.0 && score < 100.0);
    }

    #[test]
    fn test_empty_or_no_match() {
        let app = make_test_app("Firefox", "firefox", None, vec![], vec![]);
        assert_eq!(calculate_text_score("", &app), 0.0);
        assert_eq!(calculate_text_score("xyz123", &app), 0.0);
    }
}
