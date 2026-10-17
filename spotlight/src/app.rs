//! Application lifecycle coordinator, database initialization, and presentation wireup.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gdk4::prelude::*;
use gtk4::prelude::*;
use shared::db::{configure_connection, default_db_path};
use shared::frecency::FrecencyTracker;
use shared::models::{AggregatedSearchResults, SearchCategory, SearchPayload, SearchRecord};
use tokio::sync::mpsc;
use tracing::warn;

use crate::arbiter::dispatcher::{QueryDispatcher, StreamingUpdate};
use crate::arbiter::normalizer::{SelectionState, flatten_records};
use crate::arbiter::provider::SearchProvider;
use crate::providers::{
    ApplicationProvider, CalculatorProvider, DictionaryProvider, FileSearchProvider,
    SystemActionsProvider, WebSearchProvider, compute_ghost_completion, execute_system_action,
    launch_desktop_app_with_token, open_browser_url, open_file, show_in_file_manager,
};
use crate::ui::FileRowWidget;
use crate::ui::empty_state::EmptyStateWidget;
use crate::ui::no_results::NoResultsWidget;
use crate::ui::search_bar::KeyAction;
use crate::ui::window::{OverlayState, SpotlightWindow};

/// Initializes SQLite database connection and shared frecency tracker.
/// Paired SQLite connection and frecency tracker handles.
pub type DbAndFrecency = (Arc<Mutex<rusqlite::Connection>>, Arc<FrecencyTracker>);

/// Paired list of all search providers and the application provider handle.
pub type ProvidersAndApp = (Vec<Arc<dyn SearchProvider>>, Arc<ApplicationProvider>);

/// Generic error type returned by lifecycle initialization.
pub type AppResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Initializes SQLite database connection and shared frecency tracker.
///
/// Configures standard WAL pragmas and applies schema migrations if needed.
///
/// # Errors
/// Returns an error if the database path cannot be created or connection fails.
pub fn init_database_and_frecency() -> AppResult<DbAndFrecency> {
    let db_path = default_db_path()?;
    let mut conn = rusqlite::Connection::open(&db_path)?;
    configure_connection(&conn)?;
    let _ = shared::db::apply_migrations(&mut conn);
    let shared_conn = Arc::new(Mutex::new(conn));
    let frecency_tracker = Arc::new(FrecencyTracker::new(Arc::clone(&shared_conn)));
    Ok((shared_conn, frecency_tracker))
}

/// Creates in-memory SQLite connection and frecency tracker for testing.
///
/// # Errors
/// Returns an error if in-memory database creation fails.
pub fn init_test_database_and_frecency() -> AppResult<DbAndFrecency> {
    let mut conn = rusqlite::Connection::open_in_memory()?;
    configure_connection(&conn)?;
    let _ = shared::db::apply_migrations(&mut conn);
    let shared_conn = Arc::new(Mutex::new(conn));
    let frecency_tracker = Arc::new(FrecencyTracker::new(Arc::clone(&shared_conn)));
    Ok((shared_conn, frecency_tracker))
}

/// Registers and instantiates all 6 standard search providers into a vector.
///
/// # Errors
/// Returns an error if application scanning fails critically.
pub fn init_search_providers(
    shared_conn: &Arc<Mutex<rusqlite::Connection>>,
    frecency_tracker: &Arc<FrecencyTracker>,
) -> AppResult<ProvidersAndApp> {
    let app_provider = Arc::new(ApplicationProvider::new(Arc::clone(frecency_tracker))?);
    let calc_provider = Arc::new(CalculatorProvider::new());
    let system_provider = Arc::new(SystemActionsProvider::new());
    let web_provider = Arc::new(WebSearchProvider::new());
    let dict_provider = Arc::new(DictionaryProvider::new());
    let file_provider = Arc::new(FileSearchProvider::new(Arc::clone(shared_conn), 20));

    let providers: Vec<Arc<dyn SearchProvider>> = vec![
        calc_provider,
        system_provider,
        Arc::clone(&app_provider) as Arc<dyn SearchProvider>,
        dict_provider,
        file_provider,
        web_provider,
    ];

    Ok((providers, app_provider))
}

/// Executes primary action corresponding to a selected [`SearchRecord`].
pub fn execute_primary_action(
    record: &SearchRecord,
    query: &str,
    app_provider: &ApplicationProvider,
    frecency: &FrecencyTracker,
    window: &SpotlightWindow,
) {
    match &record.payload {
        SearchPayload::Application { desktop_id, exec } => {
            if let Some(app) = app_provider
                .apps()
                .iter()
                .find(|a| a.desktop_id == *desktop_id)
            {
                let token = window.activation_token();
                if let Err(err) =
                    launch_desktop_app_with_token(app, frecency, query, token.as_deref())
                {
                    warn!("Failed to launch application {}: {err}", app.name);
                }
            } else {
                let parts: Vec<&str> = exec.split_whitespace().collect();
                if let Some((prog, args)) = parts.split_first() {
                    let _ = std::process::Command::new(prog).args(args).spawn();
                }
            }
            window.dismiss();
        }
        SearchPayload::File { path } => {
            let _ = open_file(path);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let _ = frecency.record_event(
                &path.to_string_lossy(),
                "file",
                shared::frecency::EventKind::Open,
                Some(query),
                now,
            );
            window.dismiss();
        }
        SearchPayload::Calculation { result, .. } => {
            if let Some(display) = gdk4::Display::default() {
                display.clipboard().set_text(result);
            }
            window.dismiss();
        }
        SearchPayload::Dictionary { definition, .. } => {
            if let Some(display) = gdk4::Display::default() {
                display.clipboard().set_text(definition);
            }
            window.dismiss();
        }
        SearchPayload::SystemAction { action } => {
            let action_str = action.clone();
            tokio::spawn(async move {
                if let Err(err) = execute_system_action(&action_str).await {
                    warn!("Failed to execute system action '{action_str}': {err}");
                }
            });
            window.dismiss();
        }
        SearchPayload::WebSearch { url } => {
            let _ = open_browser_url(url, false);
            window.dismiss();
        }
        SearchPayload::Custom(_) => {
            window.dismiss();
        }
    }
}

/// Executes secondary action (e.g. reveal in file manager) for a selected item.
pub fn execute_secondary_action(record: &SearchRecord, window: &SpotlightWindow) {
    if let SearchPayload::File { path } = &record.payload {
        let p = path.clone();
        tokio::spawn(async move {
            let _ = show_in_file_manager(&p).await;
        });
        window.dismiss();
    }
}

/// Connects all GTK4 UI presentation events, query dispatcher updates, and keyboard interaction.
#[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
pub fn wire_spotlight_application(
    window: SpotlightWindow,
    dispatcher: QueryDispatcher,
    app_provider: Arc<ApplicationProvider>,
    frecency: Arc<FrecencyTracker>,
    mut update_rx: mpsc::UnboundedReceiver<StreamingUpdate>,
) {
    let current_records: Rc<RefCell<Vec<SearchRecord>>> = Rc::new(RefCell::new(Vec::new()));
    let selected_index: Rc<Cell<usize>> = Rc::new(Cell::new(0));
    let row_boxes: Rc<RefCell<Vec<gtk4::Box>>> = Rc::new(RefCell::new(Vec::new()));
    let selection_tracker = Rc::new(RefCell::new(SelectionState::new()));

    // Initial empty state presentation
    let suggested = app_provider.get_suggested_apps(4);
    let empty_widget = EmptyStateWidget::new(suggested);
    let empty_widget_clone = empty_widget.clone();

    let win_for_empty = window.clone();
    let frec_empty = Arc::clone(&frecency);
    empty_widget.connect_activate(move |app| {
        let token = win_for_empty.activation_token();
        let _ = launch_desktop_app_with_token(&app, &frec_empty, "", token.as_deref());
        win_for_empty.dismiss();
    });

    window.results_container.append(&empty_widget.container);

    // Search bar text changes -> dispatch query
    let win_query = window.clone();
    let records_query = Rc::clone(&current_records);
    let sel_query = Rc::clone(&selected_index);
    let rows_query = Rc::clone(&row_boxes);
    let empty_box = empty_widget_clone.container.clone();

    window.search_bar_widget.connect_query_changed(move |text| {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            // Show empty state suggested dock
            empty_box.set_visible(true);
            while let Some(child) = win_query.results_container.first_child() {
                if child != empty_box {
                    win_query.results_container.remove(&child);
                }
            }
            records_query.borrow_mut().clear();
            rows_query.borrow_mut().clear();
            sel_query.set(0);
            win_query.search_bar_widget.clear_ghost();
            win_query.animate_to_empty();
            win_query.animate_to_results_width();
            let _ = win_query.set_state(OverlayState::Empty);
        } else {
            empty_box.set_visible(false);
            let _ = dispatcher.on_query_changed(trimmed);
        }
    });

    // Handle StreamingUpdate events on glib main loop
    let win_stream = window.clone();
    let records_stream = Rc::clone(&current_records);
    let sel_stream = Rc::clone(&selected_index);
    let rows_stream = Rc::clone(&row_boxes);
    let sel_tracker = Rc::clone(&selection_tracker);
    let app_prov_stream = Arc::clone(&app_provider);
    let frec_stream = Arc::clone(&frecency);
    let empty_box_stream = empty_widget_clone.container;

    glib::MainContext::default().spawn_local(async move {
        while let Some(update) = update_rx.recv().await {
            let active_qid = win_stream.search_bar_widget.text();
            if active_qid.trim().is_empty() {
                continue;
            }

            let results = update.results();
            render_search_results(
                &win_stream,
                results,
                &records_stream,
                &sel_stream,
                &rows_stream,
                &sel_tracker,
                &app_prov_stream,
                &frec_stream,
                &empty_box_stream,
            );
        }
    });

    // Keyboard navigation and activation actions
    let win_keys = window.clone();
    let records_keys = Rc::clone(&current_records);
    let sel_keys = Rc::clone(&selected_index);
    let rows_keys = Rc::clone(&row_boxes);
    let app_prov_keys = Arc::clone(&app_provider);
    let frec_keys = Arc::clone(&frecency);

    window.search_bar_widget.connect_key_action(move |action| {
        let recs = records_keys.borrow().clone();
        if recs.is_empty() {
            match action {
                KeyAction::Escape => {
                    if win_keys.search_bar_widget.text().trim().is_empty() {
                        win_keys.dismiss();
                    } else {
                        win_keys.search_bar_widget.clear();
                    }
                }
                KeyAction::Toggle => win_keys.toggle(),
                _ => {}
            }
            return;
        }

        let cur = sel_keys.get();
        match action {
            KeyAction::Down => {
                if cur + 1 < recs.len() {
                    let next = cur + 1;
                    sel_keys.set(next);
                    update_row_selection_visuals(&rows_keys.borrow(), cur, next);
                    update_preview_for_selection(&win_keys, &recs[next]);
                }
            }
            KeyAction::Up => {
                if cur > 0 {
                    let prev = cur - 1;
                    sel_keys.set(prev);
                    update_row_selection_visuals(&rows_keys.borrow(), cur, prev);
                    update_preview_for_selection(&win_keys, &recs[prev]);
                }
            }
            KeyAction::Enter => {
                if let Some(record) = recs.get(cur) {
                    let query = win_keys.search_bar_widget.text();
                    execute_primary_action(record, &query, &app_prov_keys, &frec_keys, &win_keys);
                }
            }
            KeyAction::Escape => {
                if win_keys.search_bar_widget.text().trim().is_empty() {
                    win_keys.dismiss();
                } else {
                    win_keys.search_bar_widget.clear();
                }
            }
            KeyAction::Tab => {
                let _ = win_keys.search_bar_widget.accept_ghost_completion();
            }
            KeyAction::Toggle => {
                win_keys.toggle();
            }
        }
    });
}

/// Renders the empty results fallback card when search returns no matches.
fn render_empty_search_results(window: &SpotlightWindow, trimmed_query: &str) {
    if !trimmed_query.is_empty() {
        let no_res = NoResultsWidget::new(trimmed_query);
        let win_click = window.clone();
        no_res.connect_activate(move |url| {
            let _ = open_browser_url(url, false);
            win_click.dismiss();
        });
        window.results_container.append(&no_res.container);
        let _ = window.set_state(OverlayState::ResultsList);
        window.animate_to_results(2, 0);
        window.animate_to_results_width();
    }
}

/// Updates ghost completion in search bar based on top hit application.
fn update_ghost_text(
    window: &SpotlightWindow,
    results: &AggregatedSearchResults,
    trimmed_query: &str,
) {
    if let Some(top) = &results.top_hit
        && top.category == SearchCategory::Applications
        && let Some(suffix) = compute_ghost_completion(trimmed_query, &top.title)
    {
        window
            .search_bar_widget
            .set_ghost_completion(trimmed_query, suffix);
        return;
    }
    window.search_bar_widget.clear_ghost();
}

/// Renders synthesized search results into the window results container.
#[allow(clippy::too_many_arguments)]
fn render_search_results(
    window: &SpotlightWindow,
    results: &AggregatedSearchResults,
    records_holder: &Rc<RefCell<Vec<SearchRecord>>>,
    sel_holder: &Rc<Cell<usize>>,
    rows_holder: &Rc<RefCell<Vec<gtk4::Box>>>,
    sel_tracker: &Rc<RefCell<SelectionState>>,
    app_provider: &Arc<ApplicationProvider>,
    frecency: &Arc<FrecencyTracker>,
    empty_dock: &gtk4::Box,
) {
    empty_dock.set_visible(false);

    // Clear previous children except empty_dock
    while let Some(child) = window.results_container.first_child() {
        if child != *empty_dock {
            window.results_container.remove(&child);
        }
    }

    let flat: Vec<SearchRecord> = flatten_records(results).into_iter().cloned().collect();
    (*records_holder.borrow_mut()).clone_from(&flat);
    rows_holder.borrow_mut().clear();

    let query_str = window.search_bar_widget.text();
    let trimmed_query = query_str.trim();

    if flat.is_empty() {
        render_empty_search_results(window, trimmed_query);
        return;
    }

    let headers_count = results.sections.len();
    let rows_count = flat.len();
    let _ = window.set_state(OverlayState::ResultsList);
    window.animate_to_results(rows_count, headers_count);

    // Ghost text for top hit application
    update_ghost_text(window, results, trimmed_query);

    // Determine restored selection index using SelectionState
    let chosen_index = if let Some(prev_id) = sel_tracker.borrow().selected_id()
        && let Some(found_idx) = flat.iter().position(|r| r.id == prev_id)
    {
        found_idx
    } else {
        0
    };
    sel_holder.set(chosen_index);
    if let Some(record) = flat.get(chosen_index) {
        sel_tracker.borrow_mut().select_id(Some(record.id.clone()));
    }

    let mut row_counter = 0;

    // Render top hit if present
    if let Some(top) = &results.top_hit {
        let is_selected = row_counter == chosen_index;
        let row_box = build_result_row(
            top,
            is_selected,
            row_counter,
            window,
            sel_holder,
            rows_holder,
            sel_tracker,
            app_provider,
            frecency,
        );
        window.results_container.append(&row_box);
        rows_holder.borrow_mut().push(row_box);
        row_counter += 1;
    }

    // Render sections
    for section in &results.sections {
        let sec_hdr = gtk4::Label::new(Some(section.category.display_name()));
        sec_hdr.add_css_class("sec-hdr");
        sec_hdr.set_xalign(0.0);
        sec_hdr.set_height_request(28);
        window.results_container.append(&sec_hdr);

        for item in &section.items {
            let is_selected = row_counter == chosen_index;
            let row_box = build_result_row(
                item,
                is_selected,
                row_counter,
                window,
                sel_holder,
                rows_holder,
                sel_tracker,
                app_provider,
                frecency,
            );
            window.results_container.append(&row_box);
            rows_holder.borrow_mut().push(row_box);
            row_counter += 1;
        }
    }

    // Initial preview update for chosen selection
    if let Some(sel_rec) = flat.get(chosen_index) {
        update_preview_for_selection(window, sel_rec);
    }
}

/// Builds a 44px result row box representing a single [`SearchRecord`].
#[allow(clippy::too_many_arguments)]
fn build_result_row(
    record: &SearchRecord,
    is_selected: bool,
    index: usize,
    window: &SpotlightWindow,
    sel_holder: &Rc<Cell<usize>>,
    rows_holder: &Rc<RefCell<Vec<gtk4::Box>>>,
    sel_tracker: &Rc<RefCell<SelectionState>>,
    app_provider: &Arc<ApplicationProvider>,
    frecency: &Arc<FrecencyTracker>,
) -> gtk4::Box {
    if let SearchPayload::File { .. } = &record.payload {
        let file_row = FileRowWidget::new(record);
        file_row.set_selected(is_selected);

        let win_act = window.clone();
        let app_prov_act = Arc::clone(app_provider);
        let frec_act = Arc::clone(frecency);
        let q_act = window.search_bar_widget.text();
        file_row.connect_activate(move |rec| {
            execute_primary_action(rec, &q_act, &app_prov_act, &frec_act, &win_act);
        });

        let win_rev = window.clone();
        file_row.connect_reveal(move |rec| {
            execute_secondary_action(rec, &win_rev);
        });

        connect_row_hover_and_click(
            &file_row.container,
            index,
            record,
            window,
            sel_holder,
            rows_holder,
            sel_tracker,
        );

        file_row.container
    } else {
        let container = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        container.add_css_class("row");
        container.set_height_request(44);
        if is_selected {
            container.add_css_class("selected");
            container.add_css_class("sel");
        }

        let icon_chip = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        icon_chip.add_css_class("ic");
        icon_chip.set_size_request(28, 28);
        icon_chip.set_halign(gtk4::Align::Center);
        icon_chip.set_valign(gtk4::Align::Center);

        let icon_name = record
            .icon_name
            .as_deref()
            .unwrap_or_else(|| record.category.default_icon());
        let icon_img = gtk4::Image::from_icon_name(icon_name);
        icon_img.set_pixel_size(18);
        icon_chip.append(&icon_img);
        container.append(&icon_chip);

        let title_lbl = gtk4::Label::new(Some(&record.title));
        title_lbl.add_css_class("n");
        title_lbl.set_hexpand(true);
        title_lbl.set_xalign(0.0);
        title_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        container.append(&title_lbl);

        let cat_lbl = gtk4::Label::new(Some(record.category.display_name()));
        cat_lbl.add_css_class("t");
        cat_lbl.set_xalign(1.0);
        container.append(&cat_lbl);

        let win_act = window.clone();
        let app_prov_act = Arc::clone(app_provider);
        let frec_act = Arc::clone(frecency);
        let rec_clone = record.clone();
        let click = gtk4::GestureClick::new();
        click.connect_pressed(move |_, _, _, _| {
            let q = win_act.search_bar_widget.text();
            execute_primary_action(&rec_clone, &q, &app_prov_act, &frec_act, &win_act);
        });
        container.add_controller(click);

        connect_row_hover_and_click(
            &container,
            index,
            record,
            window,
            sel_holder,
            rows_holder,
            sel_tracker,
        );

        container
    }
}

/// Connects mouse hover and selection tracking on a row widget.
#[allow(clippy::too_many_arguments)]
fn connect_row_hover_and_click(
    container: &gtk4::Box,
    index: usize,
    record: &SearchRecord,
    window: &SpotlightWindow,
    sel_holder: &Rc<Cell<usize>>,
    rows_holder: &Rc<RefCell<Vec<gtk4::Box>>>,
    sel_tracker: &Rc<RefCell<SelectionState>>,
) {
    let motion = gtk4::EventControllerMotion::new();
    let rows_clone = Rc::clone(rows_holder);
    let sel_clone = Rc::clone(sel_holder);
    let tracker_clone = Rc::clone(sel_tracker);
    let rec_id = record.id.clone();
    let win_clone = window.clone();
    let rec_item = record.clone();

    motion.connect_enter(move |_, _, _| {
        let prev = sel_clone.get();
        if prev != index {
            sel_clone.set(index);
            tracker_clone.borrow_mut().select_id(Some(rec_id.clone()));
            update_row_selection_visuals(&rows_clone.borrow(), prev, index);
            update_preview_for_selection(&win_clone, &rec_item);
        }
    });
    container.add_controller(motion);
}

/// Updates CSS selection styling between previously and newly selected rows.
fn update_row_selection_visuals(rows: &[gtk4::Box], prev_idx: usize, new_idx: usize) {
    if let Some(prev) = rows.get(prev_idx) {
        prev.remove_css_class("selected");
        prev.remove_css_class("sel");
    }
    if let Some(new_row) = rows.get(new_idx) {
        new_row.add_css_class("selected");
        new_row.add_css_class("sel");
    }
}

/// Updates preview pane and window expansion width based on selected record.
fn update_preview_for_selection(window: &SpotlightWindow, record: &SearchRecord) {
    if let SearchPayload::File { path } = &record.payload {
        window.set_preview_item(path, None);
        window.animate_to_split_preview();
    } else {
        window.animate_to_results_width();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_init_test_database_and_frecency() {
        let res = init_test_database_and_frecency();
        assert!(res.is_ok());
        let (conn, frecency) = res.unwrap();
        assert_eq!(frecency.count_all_events().unwrap(), 0);
        drop(conn);
    }

    #[test]
    fn test_init_search_providers_count() {
        let (conn, frecency) = init_test_database_and_frecency().unwrap();
        let (providers, app_prov) = init_search_providers(&conn, &frecency).unwrap();
        assert_eq!(providers.len(), 6);
        assert!(!app_prov.apps().is_empty() || app_prov.apps().is_empty());
    }
}
