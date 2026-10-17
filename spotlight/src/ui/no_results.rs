//! Zero-results fallback view presenting web search action.
//!
//! Matches design layout from `design/spotlight/No results@1x.png` with `.empty` class styling.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;

use crate::providers::web_provider::SearchEngine;

/// Pure headless state representation for the No Results view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoResultsState {
    /// Active search query string that yielded zero results.
    pub query: String,
    /// Primary header text (always `"No Results"`).
    pub title: String,
    /// Subtitle text explaining that nothing matched locally.
    pub subtitle: String,
    /// Action label for launching web search.
    pub web_action_label: String,
    /// Target web URL for the default web search engine.
    pub web_url: String,
}

impl NoResultsState {
    /// Creates a new `NoResultsState` for `query` using default `DuckDuckGo` search.
    #[must_use]
    pub fn new(query: &str) -> Self {
        Self::with_engine(query, SearchEngine::DuckDuckGo)
    }

    /// Creates a new `NoResultsState` for `query` with explicit `engine`.
    #[must_use]
    pub fn with_engine(query: &str, engine: SearchEngine) -> Self {
        let trimmed = query.trim();
        let subtitle = format!("Nothing on this computer matches “{trimmed}”.");
        let web_action_label = format!("Search the web for “{trimmed}”");
        let web_url = engine.format_url(trimmed);

        Self {
            query: trimmed.to_string(),
            title: "No Results".to_string(),
            subtitle,
            web_action_label,
            web_url,
        }
    }
}

type ActivateCallback = Box<dyn Fn(&str)>;

/// GTK4 Widget displaying the zero-result fallback view matching design specifications.
#[derive(Clone)]
pub struct NoResultsWidget {
    /// Outer layout container with `.empty` CSS class.
    pub container: gtk4::Box,
    /// Header label ("No Results").
    pub title_label: gtk4::Label,
    /// Subtitle label ("Nothing on this computer matches “{query}”.").
    pub subtitle_label: gtk4::Label,
    /// Action button / card for web search fallback.
    pub web_card_button: gtk4::Button,
    /// Label inside web card button.
    pub web_card_label: gtk4::Label,
    state: Rc<RefCell<NoResultsState>>,
    on_activate: Rc<RefCell<Option<ActivateCallback>>>,
}

impl std::fmt::Debug for NoResultsWidget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoResultsWidget")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl NoResultsWidget {
    /// Creates a new `NoResultsWidget` for `query`.
    #[must_use]
    pub fn new(query: &str) -> Self {
        let initial_state = NoResultsState::new(query);

        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        container.add_css_class("empty");

        let title_label = gtk4::Label::new(Some(&initial_state.title));
        title_label.add_css_class("empty-title");

        let subtitle_label = gtk4::Label::new(Some(&initial_state.subtitle));
        subtitle_label.add_css_class("empty-subtitle");

        // Web search action item card
        let web_card_button = gtk4::Button::new();
        web_card_button.add_css_class("card");
        web_card_button.add_css_class("no-results-web-card");
        web_card_button.set_margin_top(16);

        let card_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        let icon = gtk4::Image::from_icon_name("applications-internet");
        icon.set_pixel_size(24);

        let web_card_label = gtk4::Label::new(Some(&initial_state.web_action_label));
        web_card_label.add_css_class("no-results-web-label");

        card_box.append(&icon);
        card_box.append(&web_card_label);
        web_card_button.set_child(Some(&card_box));

        container.append(&title_label);
        container.append(&subtitle_label);
        container.append(&web_card_button);

        let state = Rc::new(RefCell::new(initial_state));
        let on_activate: Rc<RefCell<Option<ActivateCallback>>> = Rc::new(RefCell::new(None));

        let widget = Self {
            container,
            title_label,
            subtitle_label,
            web_card_button,
            web_card_label,
            state,
            on_activate,
        };

        let state_clone = Rc::clone(&widget.state);
        let activate_clone = Rc::clone(&widget.on_activate);
        widget.web_card_button.connect_clicked(move |_| {
            let url = state_clone.borrow().web_url.clone();
            if let Some(cb) = activate_clone.borrow().as_ref() {
                cb(&url);
            }
        });

        widget
    }

    /// Updates the query and updates all rendered labels and web URLs.
    pub fn update_query(&self, query: &str) {
        let new_state = NoResultsState::new(query);
        self.subtitle_label.set_text(&new_state.subtitle);
        self.web_card_label.set_text(&new_state.web_action_label);
        *self.state.borrow_mut() = new_state;
    }

    /// Connects a callback invoked when the user clicks or activates the web search fallback card.
    pub fn connect_activate<F: Fn(&str) + 'static>(&self, callback: F) {
        *self.on_activate.borrow_mut() = Some(Box::new(callback));
    }

    /// Returns the underlying GTK container box.
    #[must_use]
    pub const fn container(&self) -> &gtk4::Box {
        &self.container
    }

    /// Returns the active web search target URL.
    #[must_use]
    pub fn web_url(&self) -> String {
        self.state.borrow().web_url.clone()
    }
}
