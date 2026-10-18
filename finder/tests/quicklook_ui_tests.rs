#![allow(
    clippy::unwrap_used,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::redundant_clone,
    clippy::unreadable_literal
)]
//! Integration and widget tests for Quick Look preview window, viewports, carousel, and index sheet.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use finder::quicklook::QuickLookEngine;
use finder::quicklook::types::{
    CodePreviewData, FallbackCardData, MediaPreviewData, PdfPreviewData,
};
use finder::ui::quicklook::QuickLookWindow;
use finder::ui::quicklook::theme::QUICKLOOK_CSS;
use gtk4::prelude::*;
use tempfile::NamedTempFile;

static GTK_TEST_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn test_quicklook_css_theme_definitions() {
    assert!(QUICKLOOK_CSS.contains(".quicklook-window"));
    assert!(QUICKLOOK_CSS.contains(".quicklook-header"));
    assert!(QUICKLOOK_CSS.contains(".quicklook-title"));
    assert!(QUICKLOOK_CSS.contains(".quicklook-counter"));
    assert!(QUICKLOOK_CSS.contains(".quicklook-viewport-container"));
    assert!(QUICKLOOK_CSS.contains(".quicklook-code-view"));
    assert!(QUICKLOOK_CSS.contains(".quicklook-mosaic-grid"));
    assert!(QUICKLOOK_CSS.contains(".quicklook-pill-overlay"));
    assert!(QUICKLOOK_CSS.contains(".quicklook-line-numbers"));
    assert!(QUICKLOOK_CSS.contains(".quicklook-media-transport"));
}

#[test]
fn test_gtk_quicklook_window_and_viewports() {
    let _lock = GTK_TEST_MUTEX.lock().unwrap();
    if !gtk4::is_initialized_main_thread() && (gtk4::is_initialized() || gtk4::init().is_err()) {
        return;
    }
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();

    let engine = Arc::new(QuickLookEngine::new());
    let ql_window = QuickLookWindow::new(None::<&gtk4::Window>, engine);

    // 1. Initial State Checks
    assert_eq!(ql_window.carousel.borrow().len(), 0);
    assert!(!*ql_window.is_fullscreen.borrow());
    assert!(!ql_window.counter_label.is_visible());
    assert!(!ql_window.btn_prev.is_sensitive());
    assert!(!ql_window.btn_next.is_sensitive());

    // 2. Carousel & Multi-File Preview
    let file1 = PathBuf::from("/tmp/preview_test_1.png");
    let file2 = PathBuf::from("/tmp/preview_test_2.rs");
    let file3 = PathBuf::from("/tmp/preview_test_3.pdf");
    let files = vec![file1.clone(), file2.clone(), file3.clone()];

    ql_window.show_preview(files.clone(), 0);

    assert_eq!(ql_window.carousel.borrow().len(), 3);
    assert_eq!(ql_window.carousel.borrow().current_index(), 0);
    assert!(ql_window.counter_label.is_visible());
    assert_eq!(ql_window.counter_label.text(), "1 of 3");
    assert!(ql_window.btn_prev.is_sensitive());
    assert!(ql_window.btn_next.is_sensitive());
    assert_eq!(ql_window.title_label.text(), "preview_test_1.png");

    // Next item via button click
    ql_window.btn_next.emit_clicked();
    assert_eq!(ql_window.carousel.borrow().current_index(), 1);
    assert_eq!(ql_window.counter_label.text(), "2 of 3");
    assert_eq!(ql_window.title_label.text(), "preview_test_2.rs");

    // Next item again
    ql_window.btn_next.emit_clicked();
    assert_eq!(ql_window.carousel.borrow().current_index(), 2);
    assert_eq!(ql_window.counter_label.text(), "3 of 3");

    // Prev item via button click
    ql_window.btn_prev.emit_clicked();
    assert_eq!(ql_window.carousel.borrow().current_index(), 1);
    assert_eq!(ql_window.counter_label.text(), "2 of 3");

    // 3. Image Viewport Zoom Tests
    {
        let img_vp = &ql_window.image_viewport;
        let temp_png = NamedTempFile::with_suffix(".png").unwrap();
        let img = image::RgbaImage::new(100, 80);
        img.save(temp_png.path()).unwrap();

        img_vp.set_image(temp_png.path(), 100, 80);
        assert!(*img_vp.is_fit_to_window.borrow());

        // Toggle to 100% original size
        img_vp.toggle_fit_or_original();
        assert!(!*img_vp.is_fit_to_window.borrow());
        assert_eq!(*img_vp.zoom_factor.borrow(), 1.0);

        // Toggle back to fit
        img_vp.toggle_fit_or_original();
        assert!(*img_vp.is_fit_to_window.borrow());

        // Explicit zoom factor
        img_vp.set_zoom_factor(2.5);
        assert!(!*img_vp.is_fit_to_window.borrow());
        assert_eq!(*img_vp.zoom_factor.borrow(), 2.5);
    }

    // 4. Code / Text Viewport Tests
    {
        let text_vp = &ql_window.text_viewport;
        let code_data = CodePreviewData {
            highlighted_html: "<span style=\"color:#ff0000;\">let</span> x = 42;".to_string(),
            raw_lines: vec!["let x = 42;".to_string(), "let y = 10;".to_string()],
            language: "Rust".to_string(),
            total_lines: 2,
            is_truncated: false,
        };

        text_vp.load_data(&code_data);
        assert_eq!(
            text_vp.gutter_buffer.text(
                &text_vp.gutter_buffer.start_iter(),
                &text_vp.gutter_buffer.end_iter(),
                false
            ),
            "1\n2\n"
        );
    }

    // 5. PDF Viewport Tests
    {
        let pdf_vp = &ql_window.pdf_viewport;
        let temp_png = NamedTempFile::with_suffix(".png").unwrap();
        let img = image::RgbaImage::new(50, 50);
        img.save(temp_png.path()).unwrap();

        let pdf_data = PdfPreviewData {
            page_count: 5,
            current_page: 2,
            rendered_page_path: temp_png.path().to_path_buf(),
            title: Some("Spec Document".to_string()),
        };

        pdf_vp.load_data(&pdf_data);
        assert_eq!(pdf_vp.current_page(), 2);
        assert_eq!(pdf_vp.page_count(), 5);
        assert_eq!(pdf_vp.page_label.text(), "Page 2 of 5");
        assert!(pdf_vp.btn_prev.is_sensitive());
        assert!(pdf_vp.btn_next.is_sensitive());

        // Advance to page 3
        pdf_vp.next_page();
        assert_eq!(pdf_vp.current_page(), 3);
        assert_eq!(pdf_vp.page_label.text(), "Page 3 of 5");

        // Previous to page 2
        pdf_vp.previous_page();
        assert_eq!(pdf_vp.current_page(), 2);
        assert_eq!(pdf_vp.page_label.text(), "Page 2 of 5");
    }

    // 6. Media Viewport Tests
    {
        let media_vp = &ql_window.media_viewport;
        let media_data = MediaPreviewData {
            duration_secs: 185.0,
            width: Some(1920),
            height: Some(1080),
            codec: Some("h264".to_string()),
            sample_rate: Some(48000),
            is_video: true,
            poster_image: None,
        };

        media_vp.load_data(&media_data);
        assert_eq!(*media_vp.duration_secs.borrow(), 185.0);
        assert_eq!(media_vp.dur_label.text(), "03:05");
        assert_eq!(media_vp.pos_label.text(), "00:00");

        media_vp.toggle_play_pause();
        assert!(*media_vp.is_playing.borrow());
        media_vp.toggle_play_pause();
        assert!(!*media_vp.is_playing.borrow());

        media_vp.set_position(65.0);
        assert_eq!(*media_vp.position_secs.borrow(), 65.0);
        assert_eq!(media_vp.pos_label.text(), "01:05");

        media_vp.set_volume(0.5);
        assert_eq!(*media_vp.volume_level.borrow(), 0.5);
    }

    // 7. Fallback Viewport Tests
    {
        let fallback_vp = &ql_window.fallback_viewport;
        let card = FallbackCardData {
            path: PathBuf::from("/tmp/archive.tar.gz"),
            file_name: "archive.tar.gz".to_string(),
            file_size: 1048576,
            mime_type: "application/gzip".to_string(),
            modified: Some(SystemTime::now()),
            icon_name: "package-x-generic".to_string(),
        };

        fallback_vp.load_data(&card);
        assert_eq!(fallback_vp.title_label.text(), "archive.tar.gz");
        assert_eq!(fallback_vp.kind_val.text(), "application/gzip");
        assert!(fallback_vp.size_val.text().contains("1.0 MB"));
    }

    // 8. Fullscreen Toggle Simulation
    {
        ql_window.btn_fullscreen.emit_clicked();
        assert!(*ql_window.is_fullscreen.borrow());
        assert!(ql_window.window.has_css_class("quicklook-fullscreen"));

        ql_window.btn_fullscreen.emit_clicked();
        assert!(!*ql_window.is_fullscreen.borrow());
        assert!(!ql_window.window.has_css_class("quicklook-fullscreen"));
    }

    // 9. Single File Selection Disables Nav Buttons
    {
        ql_window.show_preview(vec![file1], 0);
        assert_eq!(ql_window.carousel.borrow().len(), 1);
        assert!(!ql_window.counter_label.is_visible());
        assert!(!ql_window.btn_prev.is_sensitive());
        assert!(!ql_window.btn_next.is_sensitive());
        assert!(!ql_window.btn_index_sheet.is_sensitive());
    }

    // 10. Update Active File seamlessly
    {
        let file4 = PathBuf::from("/tmp/preview_test_4.jpg");
        ql_window.update_active_file(&file4);
        assert_eq!(ql_window.carousel.borrow().len(), 1);
        assert_eq!(ql_window.title_label.text(), "preview_test_4.jpg");
    }

    // 11. Dismiss
    ql_window.dismiss();
}
