//! Definition card presentation widget for lexical dictionary lookup results.
//!
//! Replicates the 30px typographic definition card styling from the design system,
//! mapping [`DictionaryCard`] into `.card.def`, `h3`, `.m`, and `p` layouts,
//! with one-key definition copying (`Enter`) and phonetic transcription copying (`Ctrl+C`).

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use gdk4::prelude::*;
use gtk4::prelude::*;
use shared::dict::DictionaryCard;

type DismissCallback = Box<dyn Fn()>;
type CopyCallback = Box<dyn Fn(&str)>;

/// Formats the grammatical metadata string combining part of speech and phonetic IPA transcription.
#[must_use]
pub fn format_meta(part_of_speech: &str, phonetic_ipa: Option<&str>) -> String {
    let pos = part_of_speech.trim();
    let ipa = phonetic_ipa.map(str::trim).filter(|s| !s.is_empty());

    match (pos.is_empty(), ipa) {
        (false, Some(ipa_str)) => format!("{pos} · {ipa_str}"),
        (false, None) => pos.to_string(),
        (true, Some(ipa_str)) => ipa_str.to_string(),
        (true, None) => String::new(),
    }
}

/// Formats the headword and phonetic IPA transcription for clipboard copying.
#[must_use]
pub fn format_headword_and_ipa(headword: &str, phonetic_ipa: Option<&str>) -> String {
    let hw = headword.trim();
    phonetic_ipa
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map_or_else(|| hw.to_string(), |ipa| format!("{hw} {ipa}"))
}

/// Definition quick answer card widget presenting headword, phonetics, and primary definition.
#[derive(Clone)]
pub struct DictCardWidget {
    /// Root vertical card container with `.card` and `.def` CSS classes.
    pub container: gtk4::Box,
    /// Prominent 30px headword title label with `h3` and `.def-headword` CSS classes.
    pub headword_label: gtk4::Label,
    /// Subtle metadata label with `.m` and `.def-meta` CSS classes.
    pub meta_label: gtk4::Label,
    /// Main definition body label with `p` and `.def-body` CSS classes.
    pub body_label: gtk4::Label,
    /// Category footer badge label with `.m` and `.def-footer` CSS classes ("Dictionary").
    pub footer_label: gtk4::Label,
    card: Rc<RefCell<DictionaryCard>>,
    dismiss_cbs: Rc<RefCell<Vec<DismissCallback>>>,
    copy_cbs: Rc<RefCell<Vec<CopyCallback>>>,
}

impl fmt::Debug for DictCardWidget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DictCardWidget")
            .field("container", &self.container)
            .field("headword_label", &self.headword_label)
            .field("meta_label", &self.meta_label)
            .field("body_label", &self.body_label)
            .field("footer_label", &self.footer_label)
            .field("headword", &self.card.borrow().headword)
            .finish_non_exhaustive()
    }
}

impl DictCardWidget {
    /// Constructs a new [`DictCardWidget`] populated from a [`DictionaryCard`].
    #[must_use]
    pub fn new(card: &DictionaryCard) -> Self {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.add_css_class("card");
        container.add_css_class("def");
        container.set_focusable(true);

        let headword_label = gtk4::Label::new(None);
        headword_label.add_css_class("def-headword");
        headword_label.add_css_class("h3");
        headword_label.set_xalign(0.0);

        let meta_label = gtk4::Label::new(None);
        meta_label.add_css_class("def-meta");
        meta_label.add_css_class("m");
        meta_label.set_xalign(0.0);

        let body_label = gtk4::Label::new(None);
        body_label.add_css_class("def-body");
        body_label.add_css_class("p");
        body_label.set_xalign(0.0);
        body_label.set_wrap(true);

        let footer_label = gtk4::Label::new(Some("Dictionary"));
        footer_label.add_css_class("def-footer");
        footer_label.add_css_class("m");
        footer_label.set_xalign(0.0);

        container.append(&headword_label);
        container.append(&meta_label);
        container.append(&body_label);
        container.append(&footer_label);

        let card_rc = Rc::new(RefCell::new(card.clone()));
        let dismiss_cbs = Rc::new(RefCell::new(Vec::new()));
        let copy_cbs = Rc::new(RefCell::new(Vec::new()));

        let widget = Self {
            container,
            headword_label,
            meta_label,
            body_label,
            footer_label,
            card: card_rc,
            dismiss_cbs,
            copy_cbs,
        };

        widget.refresh_display();
        widget.setup_key_controller();

        widget
    }

    /// Sets up the keyboard event controller for Enter and Ctrl+C shortcuts.
    fn setup_key_controller(&self) {
        let key_controller = gtk4::EventControllerKey::new();
        let widget_clone = self.clone();

        key_controller.connect_key_pressed(move |_, key, _, modifiers| {
            if widget_clone.handle_key_press(key, modifiers) {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });

        self.container.add_controller(key_controller);
    }

    /// Handles keyboard shortcuts for dictionary actions:
    /// - `Enter`: Copies clean definition text to clipboard and dismisses overlay.
    /// - `Ctrl+C` / `Cmd+C`: Copies headword and phonetic IPA transcription.
    ///
    /// Returns `true` if the key press was handled.
    #[must_use]
    pub fn handle_key_press(&self, key: gdk4::Key, modifiers: gdk4::ModifierType) -> bool {
        // Enter / KP_Enter: Copy clean definition text and dismiss
        if key == gdk4::Key::Return || key == gdk4::Key::KP_Enter {
            self.copy_clean_definition_and_dismiss();
            return true;
        }

        // Ctrl+C / Cmd+C / Super+C: Copy headword and phonetic IPA transcription
        let ctrl_or_cmd = modifiers.contains(gdk4::ModifierType::CONTROL_MASK)
            || modifiers.contains(gdk4::ModifierType::SUPER_MASK)
            || modifiers.contains(gdk4::ModifierType::META_MASK);

        if (key == gdk4::Key::c || key == gdk4::Key::C) && ctrl_or_cmd {
            self.copy_headword_and_ipa_and_dismiss();
            return true;
        }

        false
    }

    /// Updates the card to display a new [`DictionaryCard`].
    pub fn set_card(&self, card: &DictionaryCard) {
        *self.card.borrow_mut() = card.clone();
        self.refresh_display();
    }

    /// Returns a clone of the currently active [`DictionaryCard`].
    #[must_use]
    pub fn card(&self) -> DictionaryCard {
        self.card.borrow().clone()
    }

    /// Re-renders all labels based on the active [`DictionaryCard`].
    pub fn refresh_display(&self) {
        let c = self.card.borrow();

        // 1. Headword
        self.headword_label.set_text(&c.headword);

        // 2. Metadata (part of speech and phonetic IPA transcription)
        let pos = c
            .senses
            .first()
            .map(|s| s.part_of_speech.as_str())
            .unwrap_or_default();
        let meta_text = format_meta(pos, c.phonetic_ipa.as_deref());
        self.meta_label.set_text(&meta_text);

        // 3. Definition text
        let def_text = c
            .senses
            .first()
            .map(|s| s.definition.as_str())
            .unwrap_or_default();
        self.body_label.set_text(def_text);

        // 4. Footer badge
        self.footer_label.set_text("Dictionary");
    }

    /// Returns the primary clean definition text from the current card.
    #[must_use]
    pub fn clean_definition(&self) -> String {
        self.card
            .borrow()
            .senses
            .first()
            .map(|s| s.definition.clone())
            .unwrap_or_default()
    }

    /// Returns the formatted headword and phonetic IPA transcription.
    #[must_use]
    pub fn headword_and_ipa(&self) -> String {
        let c = self.card.borrow();
        format_headword_and_ipa(&c.headword, c.phonetic_ipa.as_deref())
    }

    /// Copies arbitrary text to the system clipboard and notifies copy listeners.
    ///
    /// Returns `true` if a clipboard surface was available.
    #[must_use]
    pub fn copy_to_clipboard(&self, text: &str) -> bool {
        for cb in &*self.copy_cbs.borrow() {
            cb(text);
        }

        gdk4::Display::default().is_some_and(|display| {
            display.clipboard().set_text(text);
            true
        })
    }

    /// Copies the clean definition text to the clipboard and invokes all dismissal callbacks.
    pub fn copy_clean_definition_and_dismiss(&self) {
        let text = self.clean_definition();
        let _ = self.copy_to_clipboard(&text);
        for cb in &*self.dismiss_cbs.borrow() {
            cb();
        }
    }

    /// Copies the headword and phonetic IPA transcription to the clipboard and invokes dismissal callbacks.
    pub fn copy_headword_and_ipa_and_dismiss(&self) {
        let text = self.headword_and_ipa();
        let _ = self.copy_to_clipboard(&text);
        for cb in &*self.dismiss_cbs.borrow() {
            cb();
        }
    }

    /// Registers a listener callback invoked when overlay dismissal is requested.
    pub fn connect_dismiss<F: Fn() + 'static>(&self, callback: F) {
        self.dismiss_cbs.borrow_mut().push(Box::new(callback));
    }

    /// Registers a listener callback invoked when text is copied to the clipboard.
    pub fn connect_copied<F: Fn(&str) + 'static>(&self, callback: F) {
        self.copy_cbs.borrow_mut().push(Box::new(callback));
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
    use shared::dict::DefinitionSense;
    use std::cell::Cell;

    #[test]
    fn test_format_meta_variants() {
        assert_eq!(
            format_meta("noun", Some("/ˌsɛr.ənˈdɪp.ɪ.ti/")),
            "noun · /ˌsɛr.ənˈdɪp.ɪ.ti/"
        );
        assert_eq!(format_meta("noun", None), "noun");
        assert_eq!(format_meta("noun", Some("")), "noun");
        assert_eq!(format_meta("", Some("/kæˈfeɪ/")), "/kæˈfeɪ/");
        assert_eq!(format_meta("", None), "");
    }

    #[test]
    fn test_format_headword_and_ipa() {
        assert_eq!(
            format_headword_and_ipa("serendipity", Some("/ˌsɛr.ənˈdɪp.ɪ.ti/")),
            "serendipity /ˌsɛr.ənˈdɪp.ɪ.ti/"
        );
        assert_eq!(format_headword_and_ipa("serendipity", None), "serendipity");
        assert_eq!(
            format_headword_and_ipa("serendipity", Some("")),
            "serendipity"
        );
    }

    #[test]
    fn test_dict_card_widget_lifecycle() {
        if !gtk4::is_initialized_main_thread() && (gtk4::is_initialized() || gtk4::init().is_err())
        {
            return;
        }
        let card = DictionaryCard::new(
            "serendipity",
            Some("/ˌsɛr.ənˈdɪp.ɪ.ti/".to_string()),
            vec![DefinitionSense::new(
                "noun",
                "The happy chance of finding something good while looking for something else.",
                None,
            )],
            vec!["chance".to_string()],
            None,
            900.0,
        );

        let widget = DictCardWidget::new(&card);

        assert_eq!(widget.headword_label.text(), "serendipity");
        assert_eq!(widget.meta_label.text(), "noun · /ˌsɛr.ənˈdɪp.ɪ.ti/");
        assert_eq!(
            widget.body_label.text(),
            "The happy chance of finding something good while looking for something else."
        );
        assert_eq!(widget.footer_label.text(), "Dictionary");
        assert_eq!(
            widget.clean_definition(),
            "The happy chance of finding something good while looking for something else."
        );
        assert_eq!(widget.headword_and_ipa(), "serendipity /ˌsɛr.ənˈdɪp.ɪ.ti/");

        // Test connect_dismiss and connect_copied with Enter
        let dismissed = Rc::new(Cell::new(false));
        let dismissed_clone = Rc::clone(&dismissed);
        widget.connect_dismiss(move || {
            dismissed_clone.set(true);
        });

        let copied_text = Rc::new(RefCell::new(String::new()));
        let copied_clone = Rc::clone(&copied_text);
        widget.connect_copied(move |text| {
            *copied_clone.borrow_mut() = text.to_string();
        });

        // Enter copies clean definition
        assert!(widget.handle_key_press(gdk4::Key::Return, gdk4::ModifierType::empty()));
        assert!(dismissed.get());
        assert_eq!(
            *copied_text.borrow(),
            "The happy chance of finding something good while looking for something else."
        );

        // Reset and test Ctrl+C copies headword + IPA
        dismissed.set(false);
        *copied_text.borrow_mut() = String::new();

        assert!(widget.handle_key_press(gdk4::Key::c, gdk4::ModifierType::CONTROL_MASK));
        assert!(dismissed.get());
        assert_eq!(*copied_text.borrow(), "serendipity /ˌsɛr.ənˈdɪp.ɪ.ti/");

        // Test set_card update
        let card_run = DictionaryCard::new(
            "run",
            Some("/rʌn/".to_string()),
            vec![DefinitionSense::new("verb", "Move fast on foot.", None)],
            vec![],
            None,
            450.0,
        );
        widget.set_card(&card_run);
        assert_eq!(widget.headword_label.text(), "run");
        assert_eq!(widget.meta_label.text(), "verb · /rʌn/");
        assert_eq!(widget.body_label.text(), "Move fast on foot.");
    }
}
