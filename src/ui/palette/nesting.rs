//! The palette's side of the running windows nested under the best-match
//! app: finding them after each search, the keys that reach them, the
//! footer and actions while the cursor is on them, and Ctrl+D's view by
//! desktop. The rules themselves live in `running`.

use super::*;
use crate::core::virtual_desktops;
use crate::search::providers::apps;
use crate::search::providers::windows::{self, OpenWindow};
use crate::search::Nested;
use running::{ByDesktop, DeskSummary, RunningApp, Spot};

/// `searched` holds this while Ctrl+D's view is up, so its rows are built
/// again when the text changes.
/// The app index and the open windows that nesting reads.
type Sources = (Arc<Vec<apps::App>>, Arc<Vec<OpenWindow>>);

const BY_DESKTOP_KEY: &str = "\u{0}by-desktop";

impl Palette {
    /// After a search: nests the best-match app's open windows under it.
    pub(super) fn nest_running(&mut self) {
        self.running.app = None;
        self.running.summary = None;
        if self.sub_list.is_some() {
            return;
        }
        let (index, open) = match self.sources() {
            Some(sources) => sources,
            None => return,
        };
        let Some(found) = running::find(
            &self.results.items,
            &self.query,
            self.prefix.is_none(),
            &index,
            &open,
        ) else {
            return;
        };
        if found.row.is_some() {
            if self.trying {
                self.results.items.clear();
                self.trying = false;
            }
            self.results.best_match = false;
        }
        let expanded = self.running.expanded.as_deref() == Some(found.app.key.as_str());
        let app = running::nest(&mut self.results.items, found, expanded);
        self.running.summary = Some(self.desk_summary(&app));
        self.running.app = Some(app);
    }

    /// The app index and the windows read when the palette opened.
    fn sources(&self) -> Option<Sources> {
        #[cfg(test)]
        if let Some((index, open)) = &self.running.fake {
            return Some((index.clone().into(), open.clone().into()));
        }
        Some((apps::published()?, windows::listed()))
    }

    fn desk_summary(&mut self, app: &RunningApp) -> DeskSummary {
        #[cfg(test)]
        if self.running.fake.is_some() {
            let places = vec![None; app.windows.len()];
            return running::summary(&places, &[], None);
        }
        let (desktops, current) = self
            .running
            .desktops
            .get_or_insert_with(|| (virtual_desktops::list(), virtual_desktops::current()))
            .clone();
        crate::search::start_com();
        let mut reader: Option<Option<virtual_desktops::Reader>> = None;
        let places = running::places(app, &mut self.running.places, |hwnd| {
            reader
                .get_or_insert_with(|| virtual_desktops::Reader::new().ok())
                .as_ref()
                .and_then(|reader| reader.read(hwnd as HWND))
        });
        running::summary(&places, &desktops, current)
    }

    /// Ctrl+D's view in place of the results, while it is up.
    pub(super) fn refresh_by_desktop(&mut self) -> bool {
        let Some(view) = &self.running.by_desktop else {
            return false;
        };
        let wanted = (Some(BY_DESKTOP_KEY.to_string()), self.query.clone());
        if self.searched.as_ref() != Some(&wanted) {
            self.results.items = running::by_desktop_rows(&view.app, &view.summary, &self.query);
            self.results.best_match = false;
            self.results.needle = self.query.clone();
            self.trying = false;
            self.searched = Some(wanted);
        }
        if self.selected >= self.results.items.len() {
            self.selected = self.results.items.len().saturating_sub(1);
        }
        true
    }

    pub(super) fn expanded(&self) -> bool {
        self.running
            .app
            .as_ref()
            .is_some_and(|app| self.running.expanded.as_deref() == Some(app.key.as_str()))
    }

    /// Where the cursor is against the running app, if anywhere.
    pub(super) fn spot(&self) -> Option<Spot> {
        if self.running.by_desktop.is_some() {
            return Some(Spot::ByDesktop);
        }
        self.running.app.as_ref()?;
        match self.selected_item()?.nested {
            None if self.selected == 0 => Some(Spot::App),
            Some(Nested::Window { .. }) => Some(Spot::Window),
            Some(Nested::More { .. }) => Some(Spot::More),
            _ => None,
        }
    }

    /// The app the keys act on: Ctrl+D's, or the nested one.
    fn spot_app(&self) -> Option<&RunningApp> {
        match &self.running.by_desktop {
            Some(view) => Some(&view.app),
            None => self.running.app.as_ref(),
        }
    }

    /// The footer while the cursor is on the running app or its windows.
    pub(super) fn running_hints(&self, escape: &str) -> Option<Vec<Hint>> {
        if !self.marks.is_empty() || self.action_cursor.is_some() || self.overlay {
            return None;
        }
        let spot = self.spot()?;
        Some(running::hints(
            spot,
            self.spot_app()?,
            self.expanded(),
            escape,
        ))
    }

    /// The selected row's actions, worded for the running app.
    pub(super) fn running_actions(&self, actions: Vec<RowAction>) -> Vec<RowAction> {
        match (self.spot(), self.spot_app()) {
            (Some(spot), Some(app)) => running::adjust(actions, spot, app),
            _ => actions,
        }
    }

    /// "N more windows": the whole list, in place.
    pub(super) fn expand(&mut self) {
        if let Some(app) = &self.running.app {
            self.running.expanded = Some(app.key.clone());
            self.searched = None;
            self.follow_selection = true;
        }
    }

    fn collapse(&mut self) {
        let Some(app) = &self.running.app else {
            return;
        };
        let shown = app.rows_shown(false);
        let folded = shown + usize::from(app.shown.len() > shown);
        let unfolded = app.shown.len();
        let on_nested = self
            .selected_item()
            .is_some_and(|item| item.nested.is_some());
        self.running.expanded = None;
        self.searched = None;
        if on_nested && self.selected > shown {
            // Back on the "N more" row the list was opened from.
            self.selected = shown + 1;
        } else if !on_nested && self.selected > unfolded {
            // A result under the list keeps the cursor.
            self.selected -= unfolded - folded;
        }
        self.follow_selection = true;
    }

    fn open_by_desktop(&mut self) {
        let Some(app) = self.running.app.as_ref().map(RunningApp::unfiltered) else {
            return;
        };
        let summary = self.desk_summary(&app);
        let back = (self.prefix.take(), self.query.clone());
        self.running.by_desktop = Some(ByDesktop { app, summary, back });
        self.set_query(String::new());
        self.searched = None;
    }

    fn close_by_desktop(&mut self) {
        if let Some(view) = self.running.by_desktop.take() {
            let (prefix, text) = view.back;
            self.prefix = prefix;
            self.set_query(text);
            self.searched = None;
        }
    }

    /// The keys for the running app, taken before the list and the search
    /// box see them. Returns what to run, for Alt+1 to Alt+9.
    pub(super) fn running_keys(&mut self, ctx: &egui::Context) -> Option<Run> {
        if self.action_cursor.is_some() || !self.marks.is_empty() {
            return None;
        }
        let take = |modifiers, key| ctx.input_mut(|input| input.consume_key(modifiers, key));
        let Some(spot) = self.spot() else {
            // The cursor is on a result under the expanded list: Esc still
            // folds the list first.
            if self.expanded() && take(egui::Modifiers::NONE, egui::Key::Escape) {
                self.collapse();
            }
            return None;
        };
        if take(egui::Modifiers::COMMAND, egui::Key::D) {
            if spot == Spot::ByDesktop {
                self.close_by_desktop();
            } else {
                self.open_by_desktop();
            }
            return None;
        }
        if spot == Spot::ByDesktop {
            let backspace =
                self.query.is_empty() && ctx.input(|input| input.key_pressed(egui::Key::Backspace));
            if backspace || take(egui::Modifiers::NONE, egui::Key::Escape) {
                self.close_by_desktop();
                return None;
            }
        } else if self.expanded() {
            let on_rows = matches!(spot, Spot::Window | Spot::More);
            if take(egui::Modifiers::NONE, egui::Key::Escape)
                || (on_rows && take(egui::Modifiers::NONE, egui::Key::ArrowLeft))
            {
                self.collapse();
                return None;
            }
            for (key, down) in [(egui::Key::PageDown, true), (egui::Key::PageUp, false)] {
                if spot == Spot::Window && take(egui::Modifiers::NONE, key) {
                    self.selected = running::page(&self.results.items, self.selected, down);
                    self.follow_selection = true;
                }
            }
        }
        const DIGITS: [egui::Key; running::ALT_KEYS] = [
            egui::Key::Num1,
            egui::Key::Num2,
            egui::Key::Num3,
            egui::Key::Num4,
            egui::Key::Num5,
            egui::Key::Num6,
            egui::Key::Num7,
            egui::Key::Num8,
            egui::Key::Num9,
        ];
        for (place, key) in DIGITS.into_iter().enumerate() {
            if take(egui::Modifiers::ALT, key) {
                let hwnd = self.spot_app()?.alt_target(place + 1)?;
                return Some(Run::Action(Action::Activate(hwnd)));
            }
        }
        None
    }

    /// ↓ or ↑ in the list, past the line that says no window matches.
    pub(super) fn step_selection(&mut self, down: bool) {
        self.selected = running::step(&self.results.items, self.selected, down);
        self.follow_selection = true;
    }

    /// What the inspector shows about the app row: the open count and the
    /// windows per desktop.
    pub(super) fn running_summary(&self) -> Option<(usize, &DeskSummary)> {
        if self.spot()? != Spot::App {
            return None;
        }
        let app = self.running.app.as_ref()?;
        Some((app.windows.len(), self.running.summary.as_ref()?))
    }

    /// The chip over the search box in Ctrl+D's view: "Notepad · by desktop".
    pub(super) fn by_desktop_chip(&self) -> Option<String> {
        self.running
            .by_desktop
            .as_ref()
            .map(|view| format!("{} \u{b7} by desktop", view.app.name))
    }
}
