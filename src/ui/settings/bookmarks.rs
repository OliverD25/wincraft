//! The Bookmarks page: every bookmark with its phrase and place, and a form
//! to add one. The palette's `'` prefix lists the same bookmarks.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::core::theme::Tokens;
use crate::core::ui_bridge::{HostChannel, HostRequest};
use crate::search::bookmarks::{self, Bookmark, EditError, Kind, Shared, Store};
use crate::search::Action;
use crate::ui::settings::{self as page, SettingsState};
use crate::ui::widgets::button::{self, Kind as ButtonKind};
use crate::ui::widgets::empty_state::empty_state;
use crate::ui::widgets::input::{self, Face};
use crate::ui::widgets::row::{settings_row, RowText};
use crate::ui::widgets::text;

/// How often the places are looked for again while the page is open.
const RECHECK: Duration = Duration::from_secs(5);
const CAPTION_WIDTH: f32 = 64.0;

/// What the page says under a form after the last click.
#[derive(Debug)]
enum Note {
    Warning(String),
    Error(String),
}

/// A bookmark being changed: its phrase as saved, and what the boxes hold.
struct Editing {
    phrase: String,
    new_phrase: String,
    target: String,
}

#[derive(Default)]
pub struct BookmarksView {
    add_phrase: String,
    add_target: String,
    editing: Option<Editing>,
    note: Option<Note>,
    /// Whether each place (by lowercase target) was found on disk. Looked up
    /// on a thread of its own: a share that does not answer can take seconds.
    found: HashMap<String, bool>,
    checked: Option<Instant>,
    pending: Option<Receiver<Vec<(String, bool)>>>,
}

impl BookmarksView {
    /// Takes what a finished look found, and starts another when the last is
    /// old or the list changed.
    fn update(&mut self, ctx: &egui::Context, items: &[Bookmark]) {
        if let Some(receiver) = &self.pending {
            if let Ok(results) = receiver.try_recv() {
                self.found = results.into_iter().collect();
                self.pending = None;
            }
        }
        let old = self.checked.is_none_or(|at| at.elapsed() >= RECHECK);
        if self.pending.is_some() || !old {
            return;
        }
        let places: Vec<String> = items
            .iter()
            .filter(|item| bookmarks::kind(&item.target) == Kind::Path)
            .map(|item| item.target.clone())
            .collect();
        self.checked = Some(Instant::now());
        if places.is_empty() {
            return;
        }
        let (sender, receiver) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let results = places
                .into_iter()
                .map(|place| {
                    let found = Path::new(&place).exists();
                    (place.to_lowercase(), found)
                })
                .collect();
            let _ = sender.send(results);
            ctx.request_repaint_of(page::viewport_id());
        });
        self.pending = Some(receiver);
    }

    /// A place is called missing only once it has been looked for.
    fn missing(&self, target: &str) -> bool {
        bookmarks::kind(target) == Kind::Path
            && self.found.get(&target.to_lowercase()) == Some(&false)
    }

    /// Looks again at once, after the list changed.
    fn recheck(&mut self) {
        self.checked = None;
    }
}

/// Adds a bookmark from the form. Whether the place exists is only a
/// warning: the folder may be on a drive that is not plugged in.
fn add(store: &mut Store, phrase: &str, target: &str) -> Result<Option<Note>, String> {
    let phrase = bookmarks::clean_phrase(phrase);
    let target = bookmarks::clean_target(target);
    if phrase.is_empty() {
        return Err("Type a short phrase first.".to_string());
    }
    if target.is_empty() {
        return Err("Type where it goes: a folder, a file or a web address.".to_string());
    }
    if let Some(existing) = store.find(&phrase) {
        return Err(format!(
            "\u{201c}{}\u{201d} already opens {}. Edit that one, or pick another phrase.",
            existing.phrase, existing.target
        ));
    }
    store.set(&phrase, &target, bookmarks::now());
    Ok(missing_note(&target))
}

fn missing_note(target: &str) -> Option<Note> {
    (bookmarks::kind(target) == Kind::Path && !Path::new(target).exists()).then(|| {
        Note::Warning(
            "Saved, but that folder or file was not found. Check the spelling, or the drive."
                .to_string(),
        )
    })
}

/// Saves the boxes of a bookmark being edited.
fn save_edit(store: &mut Store, editing: &Editing) -> Result<Option<Note>, String> {
    match store.edit(&editing.phrase, &editing.new_phrase, &editing.target) {
        Ok(()) => Ok(missing_note(&bookmarks::clean_target(&editing.target))),
        Err(EditError::PhraseTaken) => Err(format!(
            "\u{201c}{}\u{201d} is another bookmark. Pick a different phrase.",
            bookmarks::clean_phrase(&editing.new_phrase)
        )),
        Err(EditError::Empty) => Err("The phrase and the place cannot be empty.".to_string()),
        Err(EditError::NoSuchBookmark) => {
            Err("That bookmark is gone. It may have been removed in the palette.".to_string())
        }
    }
}

fn open_action(target: &str) -> Action {
    match bookmarks::kind(target) {
        Kind::Url => Action::OpenUrl(target.to_string()),
        Kind::Path => Action::Open(PathBuf::from(target)),
    }
}

pub fn show(ui: &mut egui::Ui, state: &mut SettingsState, to_host: &Arc<HostChannel>) {
    show_with(ui, &bookmarks::shared(), &mut state.bookmarks, to_host);
}

fn show_with(
    ui: &mut egui::Ui,
    shared: &Shared,
    view: &mut BookmarksView,
    to_host: &Arc<HostChannel>,
) {
    let items = shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .sorted();
    view.update(ui.ctx(), &items);
    ui.ctx().request_repaint_after(RECHECK);
    let tokens = Tokens::get(ui.ctx());

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            page::page_header(
                ui,
                "Bookmarks",
                None,
                "Short phrases that open a folder, a file or a web address from the palette. \
                 Type the phrase, or ' to list them all. Ctrl+B on a result adds one.",
                |_| {},
            );
            page::section_header(ui, "Add a bookmark");
            ui.spacing_mut().item_spacing.y = 8.0;
            add_form(ui, shared, view);

            let count = items.len();
            let noun = if count == 1 { "bookmark" } else { "bookmarks" };
            page::section_header(ui, &format!("Your bookmarks \u{b7} {count} {noun}"));
            ui.spacing_mut().item_spacing.y = 0.0;
            if items.is_empty() {
                empty_state(
                    ui,
                    "No bookmarks yet",
                    "Add one above, or press Ctrl+B on a folder in the palette.",
                );
                return;
            }
            for item in &items {
                if view
                    .editing
                    .as_ref()
                    .is_some_and(|editing| editing.phrase == item.phrase)
                {
                    edit_form(ui, shared, view);
                    continue;
                }
                let missing = view.missing(&item.target);
                let (desc, colour) = if missing {
                    (format!("{} \u{b7} not found", item.target), tokens.warning)
                } else {
                    (item.target.clone(), tokens.text_secondary)
                };
                let row = RowText::new(&item.phrase).desc(&desc, colour);
                settings_row(ui, &format!("bookmark/{}", item.phrase), row, false, |ui| {
                    if button::button(ui, "Open", ButtonKind::Secondary).clicked() {
                        to_host.send(HostRequest::RunAction(open_action(&item.target)));
                    }
                    if button::button(ui, "Edit", ButtonKind::Secondary).clicked() {
                        view.editing = Some(Editing {
                            phrase: item.phrase.clone(),
                            new_phrase: item.phrase.clone(),
                            target: item.target.clone(),
                        });
                        view.note = None;
                    }
                    if button::button(ui, "Delete", ButtonKind::Danger).clicked() {
                        shared
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .remove(&item.phrase);
                        view.note = None;
                        view.recheck();
                    }
                });
            }
        });
}

/// One labelled box of a form.
fn field(ui: &mut egui::Ui, caption: &str, id: &str, value: &mut String, face: Face) {
    let tokens = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(CAPTION_WIDTH, 32.0), egui::Sense::hover());
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                text::secondary(ui, caption, tokens.text_secondary);
            },
        );
        let width = (ui.available_width() - 8.0).clamp(160.0, 440.0);
        input::text(ui, id, value, face, width);
    });
}

fn note_line(ui: &mut egui::Ui, note: &Option<Note>) {
    let tokens = Tokens::get(ui.ctx());
    match note {
        Some(Note::Warning(line)) => {
            text::caption(ui, line, tokens.warning);
        }
        Some(Note::Error(line)) => {
            text::caption(ui, line, tokens.danger);
        }
        None => {}
    }
}

fn add_form(ui: &mut egui::Ui, shared: &Shared, view: &mut BookmarksView) {
    field(
        ui,
        "Phrase",
        "bookmark-add-phrase",
        &mut view.add_phrase,
        Face::Text,
    );
    field(
        ui,
        "Place",
        "bookmark-add-target",
        &mut view.add_target,
        Face::Mono,
    );
    ui.horizontal(|ui| {
        ui.add_space(CAPTION_WIDTH + 8.0);
        if button::button(ui, "Add", ButtonKind::Primary).clicked() {
            let result = add(
                &mut shared
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
                &view.add_phrase,
                &view.add_target,
            );
            match result {
                Ok(note) => {
                    view.add_phrase.clear();
                    view.add_target.clear();
                    view.note = note;
                    view.recheck();
                }
                Err(line) => view.note = Some(Note::Error(line)),
            }
        }
    });
    note_line(ui, &view.note);
}

fn edit_form(ui: &mut egui::Ui, shared: &Shared, view: &mut BookmarksView) {
    let Some(editing) = view.editing.as_mut() else {
        return;
    };
    ui.spacing_mut().item_spacing.y = 8.0;
    ui.add_space(10.0);
    field(
        ui,
        "Phrase",
        "bookmark-edit-phrase",
        &mut editing.new_phrase,
        Face::Text,
    );
    field(
        ui,
        "Place",
        "bookmark-edit-target",
        &mut editing.target,
        Face::Mono,
    );
    let mut finished = false;
    let mut saved: Option<Result<Option<Note>, String>> = None;
    ui.horizontal(|ui| {
        ui.add_space(CAPTION_WIDTH + 8.0);
        ui.spacing_mut().item_spacing.x = 8.0;
        if button::button(ui, "Save", ButtonKind::Primary).clicked() {
            saved = Some(save_edit(
                &mut shared
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
                editing,
            ));
        }
        if button::button(ui, "Cancel", ButtonKind::Secondary).clicked() {
            finished = true;
        }
    });
    match saved {
        Some(Ok(note)) => {
            view.note = note;
            finished = true;
            view.recheck();
        }
        Some(Err(line)) => view.note = Some(Note::Error(line)),
        None => {}
    }
    note_line(ui, &view.note);
    ui.add_space(10.0);
    if finished {
        view.editing = None;
    }
    ui.spacing_mut().item_spacing.y = 0.0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::ThemeChoice;
    use crate::core::theme;
    use std::sync::Mutex;

    fn shared_with(entries: &[(&str, &str)]) -> Shared {
        let mut store = Store::in_memory();
        for (phrase, target) in entries {
            store.set(phrase, target, 1);
        }
        Arc::new(Mutex::new(store))
    }

    #[test]
    fn the_add_form_needs_a_phrase_and_a_place() {
        let mut store = Store::in_memory();
        assert!(add(&mut store, "  ", r"C:\x").is_err());
        assert!(add(&mut store, "x", "  ").is_err());
        assert!(store.items().is_empty());
    }

    #[test]
    fn a_new_bookmark_is_added_and_a_taken_phrase_is_refused() {
        let mut store = Store::in_memory();
        let dir = std::env::temp_dir();
        let target = dir.display().to_string();
        assert!(matches!(add(&mut store, " Temp ", &target), Ok(None)));
        assert_eq!(store.find("temp").unwrap().phrase, "Temp");
        let refused = add(&mut store, "TEMP", r"C:\other").unwrap_err();
        assert!(refused.contains("already opens"), "{refused}");
        assert_eq!(store.items().len(), 1);
    }

    #[test]
    fn a_place_that_is_not_there_is_saved_with_a_warning() {
        let mut store = Store::in_memory();
        let note = add(&mut store, "gone", r"Z:\no\such\folder").unwrap();
        assert!(matches!(note, Some(Note::Warning(_))));
        assert_eq!(store.items().len(), 1, "still saved");
        // An address is never looked for on disk.
        assert!(matches!(
            add(&mut store, "web", "https://example.com"),
            Ok(None)
        ));
    }

    #[test]
    fn editing_reports_why_it_cannot_save() {
        let mut store = Store::in_memory();
        store.set("a", r"C:\a", 1);
        store.set("b", r"C:\b", 1);
        let editing = |phrase: &str, new_phrase: &str, target: &str| Editing {
            phrase: phrase.to_string(),
            new_phrase: new_phrase.to_string(),
            target: target.to_string(),
        };
        assert!(save_edit(&mut store, &editing("a", "B", r"C:\a"))
            .unwrap_err()
            .contains("another bookmark"));
        assert!(save_edit(&mut store, &editing("a", "", r"C:\a"))
            .unwrap_err()
            .contains("cannot be empty"));
        assert!(save_edit(&mut store, &editing("zzz", "z", r"C:\z"))
            .unwrap_err()
            .contains("gone"));
        assert!(save_edit(&mut store, &editing("a", "alpha", r"Z:\nowhere")).is_ok());
        assert_eq!(store.find("alpha").unwrap().target, r"Z:\nowhere");
    }

    #[test]
    fn open_uses_the_browser_for_an_address_and_the_shell_for_a_place() {
        assert_eq!(
            open_action("https://example.com"),
            Action::OpenUrl("https://example.com".to_string())
        );
        assert_eq!(open_action(r"D:\x"), Action::Open(PathBuf::from(r"D:\x")));
    }

    #[test]
    fn a_place_is_missing_only_after_it_was_looked_for() {
        let mut view = BookmarksView::default();
        assert!(!view.missing(r"D:\x"), "not yet looked for");
        view.found.insert(r"d:\x".to_string(), false);
        assert!(view.missing(r"D:\x"));
        assert!(!view.missing("https://example.com"));
        view.found.insert(r"d:\y".to_string(), true);
        assert!(!view.missing(r"D:\y"));
    }

    /// Draws the page a few frames, as the settings window does, over a
    /// store of its own so the real bookmarks file is never touched.
    fn draw(shared: &Shared, view: &mut BookmarksView, frames: usize) {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(&ctx, ThemeChoice::Dark);
        let bridge = crate::core::ui_bridge::create();
        for _ in 0..frames {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                show_with(ui, shared, view, &bridge.to_host);
            });
            output.textures_delta.clear();
        }
    }

    #[test]
    fn the_page_draws_with_no_bookmarks_and_with_some() {
        let empty = shared_with(&[]);
        draw(&empty, &mut BookmarksView::default(), 3);
        let some = shared_with(&[
            ("pswds", r"D:\Windows_OneDrive\OneDrive\pswds_OneDrive"),
            ("docs", "https://example.com/docs"),
        ]);
        let mut view = BookmarksView {
            editing: Some(Editing {
                phrase: "docs".to_string(),
                new_phrase: "docs".to_string(),
                target: "https://example.com/docs".to_string(),
            }),
            note: Some(Note::Warning("Saved, but not found.".to_string())),
            ..Default::default()
        };
        draw(&some, &mut view, 3);
        assert_eq!(
            some.lock().unwrap().items().len(),
            2,
            "drawing changes nothing"
        );
    }
}
