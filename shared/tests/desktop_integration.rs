//! Integration tests for XDG Desktop Application Indexer.
//!
//! Verifies:
//! - Desktop entry INI parsing (field code stripping, localized names, category/keyword lists).
//! - Directory precedence masking and deletion masking (`Hidden=true`).
//! - Desktop environment visibility filtering (`OnlyShowIn`, `NotShowIn`, `NoDisplay`).
//! - 7-tier text matching and acronym scoring.
//! - Icon theme lookup, size precedence, and caching.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::float_cmp
)]

use std::fs::{File, create_dir_all};
use std::io::Write;
use std::path::PathBuf;
use tempfile::tempdir;

use shared::desktop::{
    DesktopApp, calculate_text_score, clear_app_cache, clear_icon_cache,
    resolve_icon_path_with_dirs, scan_application_dirs, scan_installed_apps,
};

#[test]
fn test_parse_synthetic_desktop_entries() {
    let content = r#"
[Desktop Entry]
Type=Application
Version=1.5
Name=Code Editor
Name[es]=Editor de Código
Name[fr]=Éditeur de Code
Name[de]=Code-Editor
GenericName=Text Editor
GenericName[es]=Editor de Texto
Comment=Edit source code and text files
Comment[es]=Edite código fuente y archivos de texto
Exec=/usr/bin/code --new-window %F --unity-launch %u
Icon=com.visualstudio.code
Keywords=text;editor;development;programming;
Categories=Development;IDE;TextEditor;
MimeType=text/plain;text/x-chdr;text/x-csrc;application/json;
Terminal=false
NoDisplay=false
Hidden=false
OnlyShowIn=GNOME;KDE;X-Cinnamon;
"#;

    let path = PathBuf::from("/usr/share/applications/code.desktop");

    // Test default locale parsing
    let app_default = DesktopApp::parse_content(content, &path, "code.desktop", None)
        .expect("should parse synthetic entry");
    assert_eq!(app_default.desktop_id, "code.desktop");
    assert_eq!(app_default.name, "Code Editor");
    assert_eq!(app_default.generic_name.as_deref(), Some("Text Editor"));
    assert_eq!(
        app_default.comment.as_deref(),
        Some("Edit source code and text files")
    );
    assert_eq!(
        app_default.exec_raw,
        "/usr/bin/code --new-window %F --unity-launch %u"
    );
    assert_eq!(
        app_default.exec_sanitized,
        "/usr/bin/code --new-window --unity-launch"
    );
    assert_eq!(
        app_default.icon_name.as_deref(),
        Some("com.visualstudio.code")
    );
    assert_eq!(
        app_default.keywords,
        vec!["text", "editor", "development", "programming"]
    );
    assert_eq!(
        app_default.categories,
        vec!["Development", "IDE", "TextEditor"]
    );
    assert_eq!(
        app_default.mime_types,
        vec![
            "text/plain",
            "text/x-chdr",
            "text/x-csrc",
            "application/json"
        ]
    );
    assert!(!app_default.is_terminal);
    assert!(!app_default.no_display);
    assert!(!app_default.hidden);
    assert_eq!(app_default.only_show_in, vec!["GNOME", "KDE", "X-Cinnamon"]);

    // Test Spanish locale parsing
    let app_es =
        DesktopApp::parse_content(content, &path, "code.desktop", Some("es_ES.UTF-8@euro"))
            .expect("should parse with Spanish locale");
    assert_eq!(app_es.name, "Editor de Código");
    assert_eq!(app_es.generic_name.as_deref(), Some("Editor de Texto"));
    assert_eq!(
        app_es.comment.as_deref(),
        Some("Edite código fuente y archivos de texto")
    );

    // Test French locale parsing
    let app_fr = DesktopApp::parse_content(content, &path, "code.desktop", Some("fr_FR"))
        .expect("should parse with French locale");
    assert_eq!(app_fr.name, "Éditeur de Code");
}

#[test]
fn test_precedence_masking_integration() {
    let user_dir = tempdir().expect("tempdir");
    let sys_dir = tempdir().expect("tempdir");

    // System provides version 1.0 of the app
    let sys_file = sys_dir.path().join("terminal.desktop");
    let mut f_sys = File::create(&sys_file).expect("create sys file");
    writeln!(
        f_sys,
        "[Desktop Entry]\nType=Application\nName=System Terminal\nExec=gnome-terminal"
    )
    .expect("write sys");

    // User overrides with custom version
    let user_file = user_dir.path().join("terminal.desktop");
    let mut f_user = File::create(&user_file).expect("create user file");
    writeln!(
        f_user,
        "[Desktop Entry]\nType=Application\nName=Custom Terminal\nExec=alacritty"
    )
    .expect("write user");

    let dirs = vec![user_dir.path().to_path_buf(), sys_dir.path().to_path_buf()];
    let apps = scan_application_dirs(&dirs, None, None).expect("scan should succeed");

    assert_eq!(apps.len(), 1);
    assert_eq!(apps[0].name, "Custom Terminal");
    assert_eq!(apps[0].exec_sanitized, "alacritty");

    // Now test deletion masking: user creates Hidden=true entry
    let hidden_user_file = user_dir.path().join("browser.desktop");
    let mut f_hidden = File::create(&hidden_user_file).expect("create hidden file");
    writeln!(
        f_hidden,
        "[Desktop Entry]\nType=Application\nName=Browser\nExec=firefox\nHidden=true"
    )
    .expect("write hidden");

    let sys_browser = sys_dir.path().join("browser.desktop");
    let mut f_sys_browser = File::create(&sys_browser).expect("create sys browser");
    writeln!(
        f_sys_browser,
        "[Desktop Entry]\nType=Application\nName=System Browser\nExec=firefox"
    )
    .expect("write sys browser");

    let apps_after_mask = scan_application_dirs(&dirs, None, None).expect("scan should succeed");
    // Only Custom Terminal should be returned; browser is masked as deleted
    assert_eq!(apps_after_mask.len(), 1);
    assert_eq!(apps_after_mask[0].desktop_id, "terminal.desktop");
}

#[test]
fn test_desktop_visibility_integration() {
    let dir = tempdir().expect("tempdir");

    // 1. App that only shows in GNOME
    let p_gnome = dir.path().join("gnome-tool.desktop");
    let mut f1 = File::create(&p_gnome).expect("create");
    writeln!(
        f1,
        "[Desktop Entry]\nType=Application\nName=GNOME Tweaks\nExec=gnome-tweaks\nOnlyShowIn=GNOME;"
    )
    .expect("write");

    // 2. App excluded from KDE
    let p_nokde = dir.path().join("gtk-settings.desktop");
    let mut f2 = File::create(&p_nokde).expect("create");
    writeln!(
        f2,
        "[Desktop Entry]\nType=Application\nName=GTK Settings\nExec=gtk-settings\nNotShowIn=KDE;"
    )
    .expect("write");

    // 3. NoDisplay=true app (e.g. background runner)
    let p_nodisp = dir.path().join("daemon.desktop");
    let mut f3 = File::create(&p_nodisp).expect("create");
    writeln!(
        f3,
        "[Desktop Entry]\nType=Application\nName=Service Daemon\nExec=service\nNoDisplay=true"
    )
    .expect("write");

    let dirs = vec![dir.path().to_path_buf()];

    // GNOME session
    let apps_gnome = scan_application_dirs(&dirs, Some("ubuntu:GNOME"), None).expect("scan GNOME");
    let names_gnome: Vec<&str> = apps_gnome.iter().map(|a| a.name.as_str()).collect();
    assert!(names_gnome.contains(&"GNOME Tweaks"));
    assert!(names_gnome.contains(&"GTK Settings"));
    assert!(!names_gnome.contains(&"Service Daemon"));

    // KDE session
    let apps_kde = scan_application_dirs(&dirs, Some("KDE"), None).expect("scan KDE");
    assert!(apps_kde.is_empty());
}

#[test]
fn test_seven_tier_scoring_integration() {
    let chrome = DesktopApp {
        desktop_id: "google-chrome.desktop".into(),
        path: PathBuf::from("/usr/share/applications/google-chrome.desktop"),
        name: "Google Chrome".into(),
        generic_name: Some("Web Browser".into()),
        comment: Some("Access the Internet".into()),
        exec_raw: "/usr/bin/google-chrome-stable %U".into(),
        exec_sanitized: "/usr/bin/google-chrome-stable".into(),
        icon_name: Some("google-chrome".into()),
        keywords: vec!["internet".into(), "browser".into(), "web".into()],
        categories: vec!["Network".into(), "WebBrowser".into()],
        mime_types: vec!["text/html".into()],
        is_terminal: false,
        no_display: false,
        hidden: false,
        only_show_in: vec![],
        not_show_in: vec![],
    };

    let libre_calc = DesktopApp {
        desktop_id: "libreoffice-calc.desktop".into(),
        path: PathBuf::from("/usr/share/applications/libreoffice-calc.desktop"),
        name: "LibreOffice Calc".into(),
        generic_name: Some("Spreadsheet".into()),
        comment: Some("Perform calculations".into()),
        exec_raw: "libreoffice --calc %U".into(),
        exec_sanitized: "libreoffice --calc".into(),
        icon_name: Some("libreoffice-calc".into()),
        keywords: vec!["accounting".into(), "table".into(), "math".into()],
        categories: vec!["Office".into(), "Spreadsheet".into()],
        mime_types: vec![],
        is_terminal: false,
        no_display: false,
        hidden: false,
        only_show_in: vec![],
        not_show_in: vec![],
    };

    let simple_chrome = DesktopApp {
        desktop_id: "chrome.desktop".into(),
        path: PathBuf::from("/usr/share/applications/chrome.desktop"),
        name: "Chrome".into(),
        generic_name: None,
        comment: None,
        exec_raw: "chrome".into(),
        exec_sanitized: "chrome".into(),
        icon_name: None,
        keywords: vec![],
        categories: vec![],
        mime_types: vec![],
        is_terminal: false,
        no_display: false,
        hidden: false,
        only_show_in: vec![],
        not_show_in: vec![],
    };

    // 1. Tier 3: "gc" matches "Google Chrome" via acronym
    let score_gc = calculate_text_score("gc", &chrome);
    assert!(
        (600.0..800.0).contains(&score_gc),
        "Expected Tier 3 acronym match (600..800), got {score_gc}"
    );

    // 2. Tier 4: "calc" matches "LibreOffice Calc" via word boundary prefix
    let score_calc = calculate_text_score("calc", &libre_calc);
    assert!(
        (500.0..600.0).contains(&score_calc),
        "Expected Tier 4 word-boundary match (500..600), got {score_calc}"
    );

    // 3. Tier 2: "chr" matches "Chrome" via prefix
    let score_chr = calculate_text_score("chr", &simple_chrome);
    assert!(
        (800.0..1000.0).contains(&score_chr),
        "Expected Tier 2 prefix match (800..1000), got {score_chr}"
    );

    // 4. Tier 1: Exact match
    assert_eq!(calculate_text_score("Google Chrome", &chrome), 1000.0);
    assert_eq!(
        calculate_text_score("google-chrome-stable", &chrome),
        1000.0
    );

    // 5. Tier 5: Generic name
    let score_web = calculate_text_score("web", &chrome);
    assert!(score_web >= 300.0);

    // 6. Non-matching query returns 0.0
    assert_eq!(calculate_text_score("xyz999", &chrome), 0.0);
    assert_eq!(calculate_text_score("", &chrome), 0.0);
}

#[test]
fn test_icon_resolver_integration() {
    clear_icon_cache();

    let root = tempdir().expect("tempdir");
    let theme_dir = root.path().join("icons");
    let pixmaps_dir = root.path().join("pixmaps");

    // Structure:
    // icons/
    //   hicolor/
    //     64x64/apps/app.png
    //     256x256/apps/app.png
    //     scalable/apps/scalable-only.svg
    //   Nord/
    //     256x256/apps/app.png
    // pixmaps/
    //   legacy.png

    let hicolor_64 = theme_dir.join("hicolor/64x64/apps/app.png");
    let hicolor_256 = theme_dir.join("hicolor/256x256/apps/app.png");
    let hicolor_svg = theme_dir.join("hicolor/scalable/apps/scalable-only.svg");
    let nord_256 = theme_dir.join("Nord/256x256/apps/app.png");
    let pixmap_legacy = pixmaps_dir.join("legacy.png");

    create_dir_all(hicolor_64.parent().expect("parent")).expect("mkdir");
    create_dir_all(hicolor_256.parent().expect("parent")).expect("mkdir");
    create_dir_all(hicolor_svg.parent().expect("parent")).expect("mkdir");
    create_dir_all(nord_256.parent().expect("parent")).expect("mkdir");
    create_dir_all(&pixmaps_dir).expect("mkdir");

    File::create(&hicolor_64).expect("create");
    File::create(&hicolor_256).expect("create");
    File::create(&hicolor_svg).expect("create");
    File::create(&nord_256).expect("create");
    File::create(&pixmap_legacy).expect("create");

    let dirs = vec![theme_dir, pixmaps_dir];

    // Case 1: In default theme (no user theme), 256x256 in hicolor preferred over 64x64
    let res1 = resolve_icon_path_with_dirs("app", None, &dirs);
    assert_eq!(res1, Some(hicolor_256));

    // Case 2: When "Nord" theme is active, Nord/256x256 overrides hicolor
    let res2 = resolve_icon_path_with_dirs("app", Some("Nord"), &dirs);
    assert_eq!(res2, Some(nord_256));

    // Case 3: Scalable svg fallback
    let res3 = resolve_icon_path_with_dirs("scalable-only", None, &dirs);
    assert_eq!(res3, Some(hicolor_svg));

    // Case 4: Pixmaps fallback
    let res4 = resolve_icon_path_with_dirs("legacy", None, &dirs);
    assert_eq!(res4, Some(pixmap_legacy));

    // Case 5: Direct absolute path
    let res5 = resolve_icon_path_with_dirs(hicolor_64.to_str().expect("str"), None, &dirs);
    assert_eq!(res5, Some(hicolor_64));
}

#[test]
fn test_live_system_scan_and_cache() {
    clear_app_cache();

    // Calling scan_installed_apps on host system must succeed gracefully without unwrap panic
    let result = scan_installed_apps();
    assert!(result.is_ok(), "System desktop scan should not panic");

    let apps = result.expect("must be ok");
    // Second invocation should retrieve from cache
    let cached_apps = scan_installed_apps().expect("cache hit should succeed");
    assert_eq!(apps.len(), cached_apps.len());
}
