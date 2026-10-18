//! Specialized syntax-highlighted code and text document viewport.

use gtk4::prelude::*;

use crate::quicklook::types::CodePreviewData;

/// Viewport widget rendering syntax-highlighted source code with line number gutter.
#[derive(Debug, Clone)]
pub struct TextViewport {
    /// Outer scrolled window providing dual-axis navigation.
    pub scrolled_window: gtk4::ScrolledWindow,
    /// Line numbers gutter column displayed in left margin.
    pub gutter_view: gtk4::TextView,
    /// Primary text view displaying syntax-highlighted code.
    pub code_view: gtk4::TextView,
    /// Underlying text buffer for the gutter.
    pub gutter_buffer: gtk4::TextBuffer,
    /// Underlying text buffer for the code.
    pub code_buffer: gtk4::TextBuffer,
}

impl Default for TextViewport {
    fn default() -> Self {
        Self::new()
    }
}

impl TextViewport {
    /// Constructs a new [`TextViewport`].
    #[must_use]
    pub fn new() -> Self {
        let gutter_buffer = gtk4::TextBuffer::new(None::<&gtk4::TextTagTable>);
        let gutter_view = gtk4::TextView::builder()
            .buffer(&gutter_buffer)
            .editable(false)
            .cursor_visible(false)
            .can_focus(false)
            .css_classes(["quicklook-line-numbers"])
            .wrap_mode(gtk4::WrapMode::None)
            .valign(gtk4::Align::Fill)
            .build();

        let code_buffer = gtk4::TextBuffer::new(None::<&gtk4::TextTagTable>);
        let code_view = gtk4::TextView::builder()
            .buffer(&code_buffer)
            .editable(false)
            .cursor_visible(true)
            .can_focus(true)
            .css_classes(["quicklook-code-view"])
            .wrap_mode(gtk4::WrapMode::None)
            .hexpand(true)
            .vexpand(true)
            .left_margin(8)
            .right_margin(16)
            .top_margin(8)
            .bottom_margin(16)
            .build();

        let content_box = gtk4::Box::builder()
            .orientation(gtk4::Orientation::Horizontal)
            .hexpand(true)
            .vexpand(true)
            .build();

        content_box.append(&gutter_view);
        content_box.append(&code_view);

        let scrolled_window = gtk4::ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Automatic)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .hexpand(true)
            .vexpand(true)
            .child(&content_box)
            .build();

        Self {
            scrolled_window,
            gutter_view,
            code_view,
            gutter_buffer,
            code_buffer,
        }
    }

    /// Returns the root widget to embed in containers.
    #[must_use]
    pub const fn widget(&self) -> &gtk4::ScrolledWindow {
        &self.scrolled_window
    }

    /// Loads and renders the preview data from [`CodePreviewData`].
    pub fn load_data(&self, data: &CodePreviewData) {
        // 1. Populate line numbers gutter
        let count = data.raw_lines.len();
        let mut gutter_text = String::new();
        for i in 1..=count {
            gutter_text.push_str(&i.to_string());
            gutter_text.push('\n');
        }
        self.gutter_buffer.set_text(&gutter_text);

        // 2. Render syntax-highlighted text
        self.code_buffer.set_text("");
        let pango_markup = html_to_pango_markup(&data.highlighted_html);
        let mut iter = self.code_buffer.start_iter();
        self.code_buffer.insert_markup(&mut iter, &pango_markup);
    }
}

/// Converts syntect-generated HTML syntax output into valid Pango markup.
#[must_use]
pub fn html_to_pango_markup(html: &str) -> String {
    let mut markup = html.to_string();

    // Strip outer <pre ...> and </pre>
    if let Some(start) = markup.find("<pre")
        && let Some(close_tag) = markup[start..].find('>')
    {
        let pre_end = start + close_tag + 1;
        markup.drain(..pre_end);
    }
    if let Some(end) = markup.rfind("</pre>") {
        markup.truncate(end);
    }

    // Convert <p style="...">--- Truncated: ...---</p>
    let trunc_target = "<p style=\"color:#888888; font-style:italic;\">--- Truncated: File exceeds preview limit ---</p>";
    let trunc_replace = "\n<span foreground=\"#888888\" style=\"italic\">--- Truncated: File exceeds preview limit ---</span>";
    markup = markup.replace(trunc_target, trunc_replace);

    // Replace <span style="color:#([0-9a-fA-F]{6});"> with <span foreground="#$1">
    let mut result = String::with_capacity(markup.len());
    let mut rest = markup.as_str();

    while let Some(span_pos) = rest.find("<span style=\"color:#") {
        result.push_str(&rest[..span_pos]);
        let hex_start = span_pos + "<span style=\"color:#".len();
        if hex_start + 6 <= rest.len() && &rest[hex_start + 6..hex_start + 8] == ";\"" {
            let hex_color = &rest[hex_start..hex_start + 6];
            result.push_str("<span foreground=\"#");
            result.push_str(hex_color);
            result.push('\"');
            // Skip past the closing quote and optional '>'
            let remainder = &rest[hex_start + 8..];
            if let Some(tag_end) = remainder.find('>') {
                rest = &remainder[tag_end..]; // includes '>' which will be processed next or copied
            } else {
                rest = remainder;
            }
        } else {
            result.push_str("<span");
            rest = &rest[span_pos + 5..];
        }
    }
    result.push_str(rest);

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_to_pango_markup_conversion() {
        let input = "<pre style=\"background-color:#2b303c;\">\n<span style=\"color:#b48ead;\">fn</span> <span style=\"color:#8fa1b3;\">main</span>() {\n}\n</pre>";
        let markup = html_to_pango_markup(input);
        assert!(markup.contains("<span foreground=\"#b48ead\">fn</span>"));
        assert!(markup.contains("<span foreground=\"#8fa1b3\">main</span>"));
        assert!(!markup.contains("<pre"));
        assert!(!markup.contains("</pre>"));
    }
}
