//! Settings → Hotkeys: every command the palette lists, grouped by plugin,
//! with its keys and whether they work. A click, Enter or F2 records new keys; the
//! checks in `core::shortcuts` run as soon as keys are pressed, and nothing
//! is saved until Enter, Save, Replace or Save anyway.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{
    pos2, vec2, Align, Align2, Color32, CursorIcon, EventFilter, Id, Key, Layout, Modifiers, Rect,
    Response, Sense, StrokeKind, Ui,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::MOD_NOREPEAT;

use crate::core::hotkeys::{self, Shortcut};
use crate::core::shortcuts::{self, Check, Filter, CHORD_WAIT, HOST_GROUP};
use crate::core::theme::{self, Tokens};
use crate::core::traits::Hotkey;
use crate::core::ui_bridge::{HostChannel, HostRequest, ShortcutChange, ShortcutInfo, UiSnapshot};
use crate::plugins::shortcut_detector::probe::{self, Status};
use crate::ui::settings::catalog;
use crate::ui::settings::hotkey_status::{self, HotkeyState};
use crate::ui::widgets::button::{self, Kind};
use crate::ui::widgets::icons::{self, Icon};
use crate::ui::widgets::keycap::{self, Caps};
use crate::ui::widgets::{chips, focus, hotkey_capture, search_box, text};

const GROUP_HEIGHT: f32 = 44.0;
const ROW_HEIGHT: f32 = 56.0;
/// Where a name starts: after the chevron and the 24 px icon tile of a group.
const INDENT: f32 = 60.0;
const ACTIONS: f32 = 88.0;
const GAP: f32 = 16.0;
const FIELD_HEIGHT: f32 = 40.0;
const MESSAGE_WIDTH: f32 = 464.0;

/// One place in the list the keyboard can be on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Spot {
    Group(String),
    Row(String),
}

/// Keys being recorded for one command.
#[derive(Clone, Debug)]
struct Recording {
    id: String,
    first: Option<Hotkey>,
    second: Option<Hotkey>,
    first_at: Option<Instant>,
    check: Check,
}

impl Recording {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            first: None,
            second: None,
            first_at: None,
            check: Check::Fine,
        }
    }

    fn keys(&self) -> Option<Shortcut> {
        self.first.map(|first| Shortcut {
            first,
            second: self.second,
        })
    }

    /// Whether a second press could still make a chord.
    fn chord_open(&self, now: Instant) -> bool {
        self.second.is_none()
            && self
                .first_at
                .is_some_and(|at| now.duration_since(at) < CHORD_WAIT)
    }

    /// A key that is not a control. A press after the chord window starts
    /// over; one inside it is the chord's second press.
    fn press(&mut self, hotkey: Hotkey, now: Instant) {
        if self.chord_open(now) {
            self.second = Some(hotkey);
        } else {
            self.first = Some(hotkey);
            self.second = None;
            self.first_at = Some(now);
        }
    }

    fn clear(&mut self) {
        self.first = None;
        self.second = None;
        self.first_at = None;
        self.check = Check::Fine;
    }
}

/// What a plain key does while recording: Enter, Esc, Backspace and Tab
/// are controls unless a modifier is held with them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Save,
    Clear,
    Cancel,
    Key,
}

pub fn control_of(hotkey: Hotkey) -> Control {
    if hotkey.modifiers & !MOD_NOREPEAT != 0 {
        return Control::Key;
    }
    match hotkey.vk {
        0x0D => Control::Save,
        0x08 => Control::Clear,
        0x09 | 0x1B => Control::Cancel,
        _ => Control::Key,
    }
}

/// What the page remembers between frames.
#[derive(Default)]
pub struct View {
    pub query: String,
    pub filter: Filter,
    listening: bool,
    pressed: Vec<Hotkey>,
    pressed_at: Option<Instant>,
    /// Groups the user opened or closed by hand.
    open: BTreeMap<String, bool>,
    selected: Option<Spot>,
    record: Option<Recording>,
    confirm_reset: bool,
    /// Give the list the keyboard on the next frame, as when the palette
    /// opened the page on a command.
    focus_list: bool,
}

impl View {
    /// The palette's "Change hotkey…": the search set to the command's
    /// name and its row picked, ready for Enter.
    pub fn open_at(&mut self, id: &str, label: &str) {
        self.leave();
        self.query = label.to_string();
        self.filter = Filter::All;
        self.pressed.clear();
        self.selected = Some(Spot::Row(id.to_string()));
        self.focus_list = true;
    }

    /// The page is no longer shown: the keyboard hook goes.
    pub fn leave(&mut self) {
        if self.record.take().is_some() || self.listening {
            hotkey_capture::stop();
        }
        self.listening = false;
        self.confirm_reset = false;
    }

    fn start_recording(&mut self, id: &str) {
        hotkey_capture::stop();
        self.listening = false;
        self.record = Some(Recording::new(id));
        self.selected = Some(Spot::Row(id.to_string()));
        hotkey_capture::start();
    }

    fn stop_recording(&mut self) {
        if self.record.take().is_some() {
            hotkey_capture::stop();
        }
    }

    fn toggle_listening(&mut self) {
        self.stop_recording();
        if self.listening {
            self.listening = false;
            hotkey_capture::stop();
        } else {
            self.listening = true;
            self.pressed.clear();
            self.pressed_at = None;
            hotkey_capture::start();
        }
    }

    fn searching(&self) -> bool {
        !self.query.trim().is_empty() || !self.pressed.is_empty() || self.filter != Filter::All
    }
}

/// Whether another app holds `hotkey`, by trying to register it.
fn held_by_app(hotkey: Hotkey) -> bool {
    probe::verdict(std::ptr::null_mut(), hotkey, &[]).status == Status::TakenByApp
}

fn text_of(keys: &Shortcut) -> String {
    hotkeys::format_shortcut(keys)
}

pub fn show(ui: &mut Ui, snapshot: &UiSnapshot, view: &mut View, to_host: &Arc<HostChannel>) {
    let tokens = Tokens::get(ui.ctx());
    let all = &snapshot.shortcuts;
    let states = hotkey_status::states(snapshot);
    poll_keys(ui, view, all, to_host);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let changed: Vec<&ShortcutInfo> =
                all.iter().filter(|info| shortcuts::is_changed(info)).collect();
            catalog::page_title(
                ui,
                "Hotkeys",
                "Give any command a hotkey. Click a row, then press the keys.",
                |ui| {
                    ui.add_enabled_ui(!changed.is_empty(), |ui| {
                        if button::with_icon(ui, "Reset all\u{2026}", Kind::Secondary, Some(Icon::RotateCcw))
                            .clicked()
                        {
                            view.confirm_reset = true;
                        }
                    });
                },
            );
            ui.add_space(20.0);
            search_row(ui, view);
            ui.add_space(8.0);
            text::single(
                ui,
                text::job(
                    &hotkey_status::summary(&states),
                    theme::regular(13.0),
                    tokens.text_secondary,
                    None,
                ),
            );
            ui.add_space(4.0);

            let conflicts = shortcuts::conflicts(all);
            let found: Vec<bool> = all
                .iter()
                .map(|info| {
                    if view.pressed.is_empty() {
                        shortcuts::matches(info, &view.query)
                    } else {
                        shortcuts::starts_with(info, &view.pressed)
                    }
                })
                .collect();
            result_line(ui, view, all, &found);
            ui.add_space(16.0);

            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                for filter in Filter::ALL {
                    let count = all
                        .iter()
                        .zip(&found)
                        .filter(|(info, hit)| **hit && filter.keeps(info, &conflicts))
                        .count();
                    let tint = (filter == Filter::Conflicts && count > 0).then_some(tokens.warning);
                    if chips::chip_tinted(ui, filter.label(), count, view.filter == filter, tint)
                        .clicked()
                    {
                        view.filter = filter;
                    }
                }
            });
            ui.add_space(16.0);

            let keep = |info: &ShortcutInfo| {
                let at = all.iter().position(|other| other.id == info.id).unwrap_or(0);
                found[at] && view.filter.keeps(info, &conflicts)
            };
            let groups = shortcuts::groups(all, keep);
            if groups.is_empty() {
                text::secondary(ui, "No command matches. Try other words, or choose All.", tokens.text_secondary);
            } else {
                list(ui, view, all, &groups, &conflicts, &states, to_host);
            }
            ui.add_space(16.0);
            text::single(
                ui,
                text::job(
                    "A change is saved when you press Enter or Save. Reset brings back the default at any time.",
                    theme::regular(12.0),
                    tokens.text_disabled,
                    None,
                ),
            );
        });

    if view.confirm_reset {
        confirm_reset(ui, view, all, to_host);
    }
}

/// Keys from the hook: a recording's presses, or Search by keys.
fn poll_keys(ui: &Ui, view: &mut View, all: &[ShortcutInfo], to_host: &Arc<HostChannel>) {
    let now = Instant::now();
    let focused = ui.ctx().input(|input| input.viewport().focused) != Some(false);
    if view.listening {
        if hotkey_capture::take_cancelled() || !focused {
            view.listening = false;
            hotkey_capture::stop();
        }
        if let Some(hotkey) = hotkey_capture::take_result() {
            let joins = view.pressed.len() == 1
                && view
                    .pressed_at
                    .is_some_and(|at| now.duration_since(at) < CHORD_WAIT);
            if !joins {
                view.pressed.clear();
            }
            view.pressed.push(hotkey);
            view.pressed_at = Some(now);
        }
        ui.ctx().request_repaint_after(Duration::from_millis(30));
        return;
    }
    let Some(record) = view.record.as_mut() else {
        return;
    };
    if hotkey_capture::take_cancelled() || !focused {
        view.stop_recording();
        return;
    }
    if let Some(hotkey) = hotkey_capture::take_result() {
        match control_of(hotkey) {
            Control::Save => {
                if let Some(changes) = save_changes(record) {
                    to_host.send(HostRequest::SetShortcuts(changes));
                    view.stop_recording();
                }
                return;
            }
            Control::Clear => record.clear(),
            Control::Cancel => {
                view.stop_recording();
                return;
            }
            Control::Key => {
                record.press(hotkey, now);
                if let Some(keys) = record.keys() {
                    record.check = shortcuts::check(&keys, &record.id, all, held_by_app);
                }
            }
        }
    }
    // The hook runs between frames, and the chord bar empties over 3 s.
    ui.ctx().request_repaint_after(Duration::from_millis(30));
}

/// What saving the recorded keys sends, or None while they cannot be saved.
fn save_changes(record: &Recording) -> Option<Vec<ShortcutChange>> {
    let keys = record.keys()?;
    if record.check.blocks() {
        return None;
    }
    if let Check::UsedBy { id, .. } = &record.check {
        return Some(shortcuts::replace(&record.id, &keys, id));
    }
    Some(vec![ShortcutChange {
        id: record.id.clone(),
        binding: Some(text_of(&keys)),
    }])
}

fn search_row(ui: &mut Ui, view: &mut View) {
    let tokens = Tokens::get(ui.ctx());
    let search_id = ui.id().with("shortcut-search");
    if ui.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::F)) {
        ui.memory_mut(|memory| memory.request_focus(search_id));
    }
    let layout = search_layout(ui.available_width(), toggle_width(ui), help_width(ui));
    ui.horizontal(|ui| {
        ui.set_height(36.0);
        ui.spacing_mut().item_spacing.x = SEARCH_GAP;
        if view.listening || !view.pressed.is_empty() {
            keys_box(ui, view, layout.search);
        } else {
            let response = search_box::search_box(
                ui,
                search_id,
                &mut view.query,
                "Search by name, plugin or keys",
                layout.search,
            );
            if response.has_focus()
                && !view.query.is_empty()
                && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
            {
                view.query.clear();
            }
        }
        let on = view.listening;
        if toggle_button(ui, on)
            .on_hover_text("Press keys to find what uses them")
            .clicked()
        {
            view.toggle_listening();
        }
        if layout.help_inline {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                chord_help(ui, &tokens);
            });
        }
    });
    if !layout.help_inline {
        ui.add_space(8.0);
        chord_help(ui, &tokens);
    }
}

const SEARCH_GAP: f32 = 12.0;
const SEARCH_MIN: f32 = 240.0;
const SEARCH_MAX: f32 = 560.0;

/// How wide the search field is and where "What is a chord?" goes.
#[derive(Debug, PartialEq)]
struct SearchLayout {
    search: f32,
    /// False when the row is too narrow: the link sits on its own line
    /// below the row, so it is never cut off.
    help_inline: bool,
}

/// Fits the field, the Search by keys button and the link into `available`
/// with a 12 px gap between them. The field never goes below 240 px while
/// the link is in the row.
fn search_layout(available: f32, button: f32, help: f32) -> SearchLayout {
    let room = available - button - help - 2.0 * SEARCH_GAP;
    if room >= SEARCH_MIN {
        return SearchLayout {
            search: room.min(SEARCH_MAX),
            help_inline: true,
        };
    }
    SearchLayout {
        search: (available - button - SEARCH_GAP).clamp(160.0, SEARCH_MAX),
        help_inline: false,
    }
}

const TOGGLE_LABEL: &str = "Search by keys";
const HELP_LABEL: &str = "What is a chord?";

fn text_width(ui: &Ui, label: &str, size: f32, colour: Color32) -> f32 {
    ui.painter()
        .layout_no_wrap(label.to_string(), theme::regular(size), colour)
        .size()
        .x
}

fn toggle_width(ui: &Ui) -> f32 {
    12.0 + 16.0 + 8.0 + text_width(ui, TOGGLE_LABEL, 14.0, Color32::WHITE) + 12.0
}

fn help_width(ui: &Ui) -> f32 {
    16.0 + 6.0 + text_width(ui, HELP_LABEL, 13.0, Color32::WHITE)
}

/// The search field while Search by keys is on: the presses as keycaps
/// and "● Listening".
fn keys_box(ui: &mut Ui, view: &mut View, width: f32) {
    let tokens = Tokens::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(width, 36.0), Sense::click());
    let border = if view.listening {
        tokens.accent
    } else {
        tokens.border
    };
    ui.painter().rect_stroke(
        rect,
        tokens.radius_row,
        theme::line(ui.ctx(), border),
        StrokeKind::Inside,
    );
    let inner = rect.shrink2(vec2(10.0, 0.0));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            let steps: Vec<Vec<String>> = view
                .pressed
                .iter()
                .map(|hotkey| Shortcut::single(*hotkey).steps().concat())
                .collect();
            if !steps.is_empty() {
                keycap::caps(ui, &steps, Caps::page());
                ui.add_space(10.0);
            }
            if view.listening {
                text::single(
                    ui,
                    text::job(
                        "\u{25CF} Listening",
                        theme::regular(13.0),
                        tokens.accent,
                        None,
                    ),
                );
            } else {
                text::single(
                    ui,
                    text::job(
                        "Click the button to search by name again",
                        theme::regular(12.0),
                        tokens.text_disabled,
                        None,
                    ),
                );
            }
        },
    );
    if response.clicked() && !view.listening {
        view.pressed.clear();
    }
}

/// "Search by keys": 36 high, the keyboard icon, outlined; accent when on.
fn toggle_button(ui: &mut Ui, on: bool) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let label = TOGGLE_LABEL;
    let colour = if on {
        tokens.accent
    } else {
        tokens.text_primary
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), theme::regular(14.0), colour);
    let size = vec2(toggle_width(ui), 36.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.08);
    let (fill, border) = if on {
        (tokens.selection_bg, tokens.accent)
    } else {
        (tokens.hover_bg.gamma_multiply(hover), tokens.border)
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        tokens.radius_row,
        fill,
        theme::line(ui.ctx(), border),
        StrokeKind::Inside,
    );
    let icon = Rect::from_center_size(
        pos2(rect.left() + 12.0 + 8.0, rect.center().y),
        vec2(16.0, 16.0),
    );
    icons::paint(painter, icon, Icon::Keyboard, colour);
    painter.galley(
        pos2(icon.right() + 8.0, rect.center().y - galley.size().y / 2.0),
        galley,
        colour,
    );
    focus::ring(ui, &response, f32::from(tokens.radius_row));
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// "What is a chord?" and its popover.
fn chord_help(ui: &mut Ui, tokens: &Tokens) {
    let label = HELP_LABEL;
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_string(), theme::regular(13.0), tokens.accent);
    let size = vec2(help_width(ui), 36.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let icon = Rect::from_center_size(pos2(rect.left() + 8.0, rect.center().y), vec2(16.0, 16.0));
    icons::paint(ui.painter(), icon, Icon::CircleHelp, tokens.accent);
    ui.painter().galley(
        pos2(icon.right() + 6.0, rect.center().y - galley.size().y / 2.0),
        galley,
        tokens.accent,
    );
    focus::ring(ui, &response, 2.0);
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    egui::Popup::menu(&response)
        .id(ui.id().with("chord-help"))
        .width(320.0)
        .show(|ui| {
            ui.set_width(320.0);
            text::wrapped(
                ui,
                text::job(
                    "A chord is two key presses in a row, like Win+Alt+K, then S.",
                    theme::regular(14.0),
                    tokens.text_primary,
                    Some(20.0),
                ),
            );
        });
}

fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

fn result_line(ui: &mut Ui, view: &View, all: &[ShortcutInfo], found: &[bool]) {
    let tokens = Tokens::get(ui.ctx());
    let count = found.iter().filter(|hit| **hit).count();
    let line = if !view.pressed.is_empty() {
        let keys = view
            .pressed
            .iter()
            .map(|hotkey| hotkeys::format(*hotkey))
            .collect::<Vec<_>>()
            .join(", ");
        if count == 0 {
            format!("No command uses {keys}.")
        } else {
            let chord_start = view.pressed.len() == 1
                && all.iter().zip(found).any(|(info, hit)| {
                    *hit && shortcuts::parse(&info.binding).is_some_and(|keys| keys.is_chord())
                });
            let mut line = format!(
                "{} start with {keys}.",
                plural(count, "command", "commands")
            );
            if count == 1 {
                line = format!("1 command starts with {keys}.");
            }
            if chord_start {
                line.push_str(" It is the first step of a chord.");
            }
            line
        }
    } else if !view.query.trim().is_empty() {
        let verb = if count == 1 { "matches" } else { "match" };
        format!(
            "{} {verb} \"{}\".",
            plural(count, "command", "commands"),
            view.query.trim()
        )
    } else {
        String::new()
    };
    if !line.is_empty() {
        text::single(
            ui,
            text::job(&line, theme::regular(13.0), tokens.text_secondary, None),
        );
    }
}

/// The places the keyboard walks through, in order.
fn spots(
    groups: &[(String, Vec<usize>)],
    all: &[ShortcutInfo],
    open: impl Fn(&str) -> bool,
) -> Vec<Spot> {
    let mut spots = Vec::new();
    for (name, rows) in groups {
        spots.push(Spot::Group(name.clone()));
        if open(name) {
            spots.extend(rows.iter().map(|&at| Spot::Row(all[at].id.clone())));
        }
    }
    spots
}

/// Whether a group shows its rows: matches of a search or filter always
/// do; otherwise WinCraft and the first plugin, unless the user chose.
fn is_open(view: &View, groups: &[(String, Vec<usize>)], name: &str) -> bool {
    if view.searching() {
        return true;
    }
    if let Some(open) = view.open.get(name) {
        return *open;
    }
    let first_plugin = groups
        .iter()
        .find(|(group, _)| group != HOST_GROUP)
        .map(|(group, _)| group.as_str());
    name == HOST_GROUP || Some(name) == first_plugin
}

fn list(
    ui: &mut Ui,
    view: &mut View,
    all: &[ShortcutInfo],
    groups: &[(String, Vec<usize>)],
    conflicts: &BTreeSet<String>,
    states: &BTreeMap<String, HotkeyState>,
    to_host: &Arc<HostChannel>,
) {
    let tokens = Tokens::get(ui.ctx());
    let width = ui.available_width();
    let shortcut_width = ((width - INDENT - ACTIONS - 2.0 * GAP) * 0.5).clamp(160.0, 360.0);
    let shortcut_left = width - ACTIONS - GAP - shortcut_width;
    // The column header doubles as the list's place in the Tab order.
    let list_id = ui.id().with("shortcut-list");
    let (header, _) = ui.allocate_exact_size(vec2(width, 28.0), Sense::hover());
    let list_focus = ui.interact(header, list_id, Sense::focusable_noninteractive());
    if std::mem::take(&mut view.focus_list) {
        ui.memory_mut(|memory| memory.request_focus(list_id));
    }
    let painter = ui.painter();
    painter.text(
        pos2(header.left() + INDENT, header.center().y),
        Align2::LEFT_CENTER,
        "Command",
        theme::regular(12.0),
        tokens.text_secondary,
    );
    painter.text(
        pos2(header.left() + shortcut_left, header.center().y),
        Align2::LEFT_CENTER,
        "Hotkey",
        theme::regular(12.0),
        tokens.text_secondary,
    );
    painter.hline(
        header.x_range(),
        header.bottom(),
        theme::line(ui.ctx(), tokens.border),
    );

    let open_now = |name: &str| is_open(view, groups, name);
    let order = spots(groups, all, open_now);
    let has_focus = list_focus.has_focus();
    if has_focus && view.record.is_none() {
        keyboard(ui, view, all, groups, &order, list_id, to_host);
    }
    let keyboard_on = has_focus && focus::keyboard_active(ui.ctx());
    let columns = Columns {
        width,
        shortcut_left,
        shortcut_width,
        keyboard_on,
        list_id,
        states,
    };

    for (name, rows) in groups {
        let open = is_open(view, groups, name);
        let spot = Spot::Group(name.clone());
        let in_group: Vec<&ShortcutInfo> = rows.iter().map(|&at| &all[at]).collect();
        let clashes = in_group
            .iter()
            .filter(|info| conflicts.contains(&info.id))
            .count();
        let response = group_header(
            ui,
            name,
            &in_group,
            open,
            clashes,
            view.selected.as_ref() == Some(&spot) && keyboard_on,
        );
        if response.clicked() {
            view.open.insert(name.clone(), !open);
            view.selected = Some(spot);
            ui.memory_mut(|memory| memory.request_focus(list_id));
        }
        if open {
            for info in in_group {
                row(ui, view, info, &columns, to_host);
            }
        }
    }
}

/// The list's widths, and where the keyboard is.
struct Columns<'a> {
    states: &'a BTreeMap<String, HotkeyState>,
    width: f32,
    shortcut_left: f32,
    shortcut_width: f32,
    keyboard_on: bool,
    list_id: Id,
}

/// ↑↓ move, ← closes and → opens a group, Home and End, Enter or F2
/// records, Delete removes and Ctrl+Backspace resets the row.
fn keyboard(
    ui: &mut Ui,
    view: &mut View,
    all: &[ShortcutInfo],
    groups: &[(String, Vec<usize>)],
    order: &[Spot],
    list_id: Id,
    to_host: &Arc<HostChannel>,
) {
    if order.is_empty() {
        return;
    }
    ui.memory_mut(|memory| {
        memory.set_focus_lock_filter(
            list_id,
            EventFilter {
                horizontal_arrows: true,
                vertical_arrows: true,
                ..EventFilter::default()
            },
        )
    });
    let at = view
        .selected
        .as_ref()
        .and_then(|spot| order.iter().position(|other| other == spot));
    let take = |ui: &mut Ui, modifiers: Modifiers, key: Key| {
        ui.input_mut(|input| input.consume_key(modifiers, key))
    };
    let last = order.len() - 1;
    let mut to = None;
    if take(ui, Modifiers::NONE, Key::ArrowDown) {
        to = Some(at.map_or(0, |at| (at + 1).min(last)));
    } else if take(ui, Modifiers::NONE, Key::ArrowUp) {
        to = Some(at.map_or(0, |at| at.saturating_sub(1)));
    } else if take(ui, Modifiers::NONE, Key::Home) {
        to = Some(0);
    } else if take(ui, Modifiers::NONE, Key::End) {
        to = Some(last);
    }
    if let Some(to) = to {
        view.selected = Some(order[to].clone());
        return;
    }
    let Some(spot) = at.map(|at| order[at].clone()) else {
        return;
    };
    let left = take(ui, Modifiers::NONE, Key::ArrowLeft);
    let right = take(ui, Modifiers::NONE, Key::ArrowRight);
    match spot {
        Spot::Group(name) => {
            let open = is_open(view, groups, &name);
            if (left && open) || (right && !open) {
                view.open.insert(name, right);
            }
        }
        Spot::Row(id) => {
            if left {
                if let Some(info) = all.iter().find(|info| info.id == id) {
                    view.selected = Some(Spot::Group(info.group.clone()));
                }
                return;
            }
            let Some(info) = all.iter().find(|info| info.id == id) else {
                return;
            };
            if take(ui, Modifiers::NONE, Key::Enter) || take(ui, Modifiers::NONE, Key::F2) {
                view.start_recording(&id);
            } else if take(ui, Modifiers::NONE, Key::Delete) && !info.one_press {
                send(to_host, &id, Some(String::new()));
            } else if take(ui, Modifiers::COMMAND, Key::Backspace) && shortcuts::is_changed(info) {
                send(to_host, &id, None);
            }
        }
    }
}

fn send(to_host: &Arc<HostChannel>, id: &str, binding: Option<String>) {
    to_host.send(HostRequest::SetShortcuts(vec![ShortcutChange {
        id: id.to_string(),
        binding,
    }]));
}

/// Where a group header puts its chevron and icon tile. The name starts at
/// `INDENT`, 10 px after the tile.
struct HeaderRects {
    chevron: Rect,
    tile: Rect,
}

fn header_rects(row: Rect) -> HeaderRects {
    let chevron = Rect::from_center_size(
        pos2(row.left() + 4.0 + 8.0, row.center().y),
        vec2(16.0, 16.0),
    );
    let tile = Rect::from_center_size(
        pos2(chevron.right() + 6.0 + 12.0, row.center().y),
        vec2(24.0, 24.0),
    );
    HeaderRects { chevron, tile }
}

/// 44 high: chevron, the plugin's icon on a small tile, the name, the
/// count and a warning badge for conflicts.
fn group_header(
    ui: &mut Ui,
    name: &str,
    rows: &[&ShortcutInfo],
    open: bool,
    clashes: usize,
    ring: bool,
) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), GROUP_HEIGHT), Sense::click());
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let painter = ui.painter();
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.08);
    painter.rect_filled(
        rect,
        tokens.radius_row,
        tokens.hover_bg.gamma_multiply(hover),
    );
    let HeaderRects { chevron, tile } = header_rects(rect);
    let shape = if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    icons::paint(painter, chevron, shape, tokens.text_secondary);
    let icon = if name == HOST_GROUP {
        Icon::Keyboard
    } else {
        let first = rows.first();
        catalog::icon_of(
            first.and_then(|info| info.icon.as_deref()),
            first.map_or("", |info| info.category.as_str()),
        )
    };
    if tokens.icon_tiles {
        icons::paint_on_tile(
            painter,
            tile,
            tile.shrink(5.0),
            icon,
            f32::from(tokens.radius_tile),
        );
    } else {
        painter.rect_stroke(
            tile,
            tokens.radius_tile,
            theme::line(ui.ctx(), tokens.border),
            StrokeKind::Inside,
        );
        icons::paint(painter, tile.shrink(5.0), icon, tokens.accent);
    }
    let label =
        painter.layout_no_wrap(name.to_string(), theme::semibold(15.0), tokens.text_primary);
    let label_at = pos2(rect.left() + INDENT, rect.center().y - label.size().y / 2.0);
    let label_right = label_at.x + label.size().x;
    painter.galley(label_at, label, tokens.text_primary);
    let count = painter.layout_no_wrap(
        plural(rows.len(), "command", "commands"),
        theme::regular(12.0),
        tokens.text_disabled,
    );
    let count_at = pos2(label_right + 10.0, rect.center().y - count.size().y / 2.0);
    let count_right = count_at.x + count.size().x;
    painter.galley(count_at, count, tokens.text_disabled);
    if clashes > 0 {
        let badge = painter.layout_no_wrap(
            plural(clashes, "conflict", "conflicts"),
            theme::regular(12.0),
            tokens.warning,
        );
        let chip = Rect::from_min_size(
            pos2(count_right + 10.0, rect.center().y - 10.0),
            vec2(badge.size().x + 16.0, 20.0),
        );
        painter.rect_stroke(
            chip,
            tokens.radius_row,
            theme::line(ui.ctx(), tokens.warning),
            StrokeKind::Inside,
        );
        painter.galley(chip.center() - badge.size() / 2.0, badge, tokens.warning);
    }
    if ring {
        focus::paint(ui, rect, f32::from(tokens.radius_row));
    }
    response
}

/// Extra height of a row while it records: the field, the chord bar and
/// hint, the message and the buttons.
fn recording_height(record: &Recording, now: Instant) -> f32 {
    let hint = if record.chord_open(now) && record.first.is_some() {
        26.0
    } else {
        0.0
    };
    let message = if record.check.message("").is_some() {
        64.0
    } else {
        0.0
    };
    12.0 + FIELD_HEIGHT + 3.0 + hint + message + 10.0 + 32.0 + 12.0
}

fn row(
    ui: &mut Ui,
    view: &mut View,
    info: &ShortcutInfo,
    columns: &Columns<'_>,
    to_host: &Arc<HostChannel>,
) {
    let (keyboard_on, list_id) = (columns.keyboard_on, columns.list_id);
    let tokens = Tokens::get(ui.ctx());
    let now = Instant::now();
    let recording = view
        .record
        .as_ref()
        .filter(|record| record.id == info.id)
        .cloned();
    let height = recording.as_ref().map_or(ROW_HEIGHT, |record| {
        recording_height(record, now).max(ROW_HEIGHT)
    });
    let (rect, response) = ui.allocate_exact_size(vec2(columns.width, height), Sense::click());
    let selected = view.selected.as_ref() == Some(&Spot::Row(info.id.clone()));
    if selected && std::mem::take(&mut view.focus_list) {
        response.scroll_to_me(Some(Align::Center));
    }
    if !ui.is_rect_visible(rect) && recording.is_none() {
        return;
    }
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.08);
    if selected {
        crate::ui::widgets::row::paint_selection(ui, rect);
    } else if hover > 0.0 {
        ui.painter().rect_filled(
            rect,
            tokens.radius_row,
            tokens.hover_bg.gamma_multiply(hover),
        );
    }
    let top_line = Rect::from_min_size(rect.min, vec2(rect.width(), ROW_HEIGHT));
    let name_width = (columns.shortcut_left - INDENT - GAP).max(40.0);
    let one_line = |text: &str, size: f32, colour: Color32| {
        let mut job = text::job(text, theme::regular(size), colour, None);
        job.wrap = egui::text::TextWrapping {
            max_width: name_width,
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('\u{2026}'),
        };
        job
    };
    let painter = ui.painter().clone();
    let name = painter.layout_job(one_line(&info.label, 14.0, tokens.text_primary));
    let has_description = !info.description.is_empty();
    let name_top = if has_description {
        top_line.center().y - 19.0
    } else {
        top_line.center().y - name.size().y / 2.0
    };
    painter.galley(
        pos2(rect.left() + INDENT, name_top),
        name,
        tokens.text_primary,
    );
    if has_description {
        let description =
            painter.layout_job(one_line(&info.description, 12.0, tokens.text_secondary));
        painter.galley(
            pos2(rect.left() + INDENT, name_top + 21.0),
            description,
            tokens.text_secondary,
        );
    }

    let cell = Rect::from_min_size(
        pos2(rect.left() + columns.shortcut_left, top_line.top()),
        vec2(columns.shortcut_width, ROW_HEIGHT),
    );
    match &recording {
        Some(record) => recording_area(ui, view, record, info, cell, to_host),
        None => shortcut_cell(
            ui,
            view,
            info,
            columns.states.get(&info.id),
            cell,
            response.hovered(),
        ),
    }
    let actions = Rect::from_min_size(
        pos2(rect.right() - ACTIONS, top_line.top()),
        vec2(ACTIONS, ROW_HEIGHT),
    );
    row_actions(ui, view, info, actions, to_host);

    if response.clicked() && recording.is_none() {
        view.selected = Some(Spot::Row(info.id.clone()));
        ui.memory_mut(|memory| memory.request_focus(list_id));
        view.start_recording(&info.id);
    }
    if selected && keyboard_on {
        focus::paint(ui, rect, f32::from(tokens.radius_row));
    }
}

/// The keys as caps, "Changed" when they are not the default, or
/// "Add hotkey".
fn shortcut_cell(
    ui: &mut Ui,
    view: &View,
    info: &ShortcutInfo,
    state: Option<&HotkeyState>,
    cell: Rect,
    hovered: bool,
) {
    let tokens = Tokens::get(ui.ctx());
    let matched = if shortcuts::starts_with(info, &view.pressed) {
        view.pressed.len()
    } else {
        0
    };
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(cell)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            match shortcuts::parse(&info.binding) {
                Some(keys) => {
                    if let Some(state) = state {
                        status_mark(ui, state);
                    }
                    keycap::caps(
                        ui,
                        &keys.steps(),
                        Caps {
                            matched,
                            ..Caps::page()
                        },
                    );
                }
                None => {
                    let (label, colour) = if hovered {
                        ("+ Add hotkey", tokens.accent)
                    } else {
                        ("Add hotkey", tokens.text_disabled)
                    };
                    text::single(ui, text::job(label, theme::regular(13.0), colour, None));
                }
            }
            if shortcuts::is_changed(info) {
                changed_tag(ui);
            }
        },
    );
}

/// 16 px cell before the keycaps: a 6 px dot, or a small triangle when the
/// keys are taken. The tooltip says why.
fn status_mark(ui: &mut Ui, state: &HotkeyState) {
    let tokens = Tokens::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
    let painter = ui.painter();
    match state {
        HotkeyState::Taken(_) => icons::paint(painter, rect, Icon::TriangleAlert, tokens.warning),
        HotkeyState::Registered => {
            painter.circle_filled(rect.center(), 3.0, tokens.success);
        }
        HotkeyState::PluginOff => {
            painter.circle_filled(rect.center(), 3.0, tokens.text_disabled);
        }
    }
    response.on_hover_text(state.tooltip());
}

fn changed_tag(ui: &mut Ui) {
    let tokens = Tokens::get(ui.ctx());
    let galley =
        ui.painter()
            .layout_no_wrap("Changed".to_string(), theme::regular(12.0), tokens.accent);
    let (rect, _) = ui.allocate_exact_size(vec2(galley.size().x + 16.0, 20.0), Sense::hover());
    ui.painter().rect_stroke(
        rect,
        10,
        theme::line(ui.ctx(), tokens.accent),
        StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, tokens.accent);
}

/// The recording field, the chord bar, the check's message and the buttons.
fn recording_area(
    ui: &mut Ui,
    view: &mut View,
    record: &Recording,
    info: &ShortcutInfo,
    cell: Rect,
    to_host: &Arc<HostChannel>,
) {
    let tokens = Tokens::get(ui.ctx());
    let now = Instant::now();
    let state = match &record.check {
        check if check.blocks() => tokens.danger,
        Check::UsedBy { .. } | Check::HeldByApp { .. } => tokens.warning,
        _ => tokens.accent,
    };
    let field = Rect::from_min_size(
        pos2(cell.left(), cell.top() + 8.0),
        vec2(cell.width(), FIELD_HEIGHT),
    );
    ui.painter().rect(
        field,
        tokens.radius_row,
        tokens.window_bg,
        theme::stroke(ui.ctx(), 2.0, state),
        StrokeKind::Inside,
    );
    let chord_open = record.first.is_some() && record.chord_open(now);
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(field.shrink2(vec2(10.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
        |ui| match record.keys() {
            Some(keys) => {
                let steps = keys.steps();
                keycap::caps(
                    ui,
                    &steps,
                    Caps {
                        waiting: chord_open,
                        ..Caps::page()
                    },
                );
            }
            None => {
                let live = hotkey_capture::live_modifiers();
                if live == 0 {
                    text::single(
                        ui,
                        text::job(
                            "Press the keys\u{2026}",
                            theme::regular(14.0),
                            tokens.text_disabled,
                            None,
                        ),
                    );
                } else {
                    let held: Vec<String> = hotkeys::format_modifiers(live)
                        .split('+')
                        .map(str::to_string)
                        .collect();
                    keycap::caps(ui, &[held], Caps::page());
                }
            }
        },
    );
    let mut y = field.bottom() + 3.0;
    if chord_open {
        let left = record
            .first_at
            .map_or(0.0, |at| {
                1.0 - now.duration_since(at).as_secs_f32() / CHORD_WAIT.as_secs_f32()
            })
            .clamp(0.0, 1.0);
        let bar = Rect::from_min_size(
            pos2(field.left(), field.bottom() + 2.0),
            vec2(field.width() * left, 3.0),
        );
        ui.painter().rect_filled(bar, 1, tokens.accent);
        let keys = record.first.map(hotkeys::format).unwrap_or_default();
        ui.painter().text(
            pos2(field.left(), y + 8.0),
            Align2::LEFT_TOP,
            format!("Press one more key to make a chord. Enter saves {keys} now."),
            theme::regular(12.0),
            tokens.text_secondary,
        );
        y += 26.0;
    }
    if let Some((first, second)) = record.check.message(&info.label) {
        let icon = if record.check.blocks() {
            Icon::Ban
        } else {
            Icon::TriangleAlert
        };
        let width = field.width().min(MESSAGE_WIDTH);
        let boxed = Rect::from_min_size(pos2(field.left(), y + 8.0), vec2(width, 56.0));
        ui.painter().rect_stroke(
            boxed,
            tokens.radius_row,
            theme::line(ui.ctx(), state),
            StrokeKind::Inside,
        );
        let glyph = Rect::from_min_size(boxed.min + vec2(10.0, 10.0), vec2(16.0, 16.0));
        icons::paint(ui.painter(), glyph, icon, state);
        let text_width = (width - 46.0).max(40.0);
        let clamp = |text: &str, size: f32, colour: Color32| {
            let mut job = text::job(text, theme::regular(size), colour, None);
            job.wrap = egui::text::TextWrapping {
                max_width: text_width,
                max_rows: 1,
                break_anywhere: true,
                overflow_character: Some('\u{2026}'),
            };
            ui.painter().layout_job(job)
        };
        let line_one = clamp(&first, 13.0, tokens.text_primary);
        let line_two = clamp(&second, 12.0, tokens.text_secondary);
        ui.painter().galley(
            pos2(glyph.right() + 10.0, boxed.top() + 9.0),
            line_one,
            tokens.text_primary,
        );
        ui.painter().galley(
            pos2(glyph.right() + 10.0, boxed.top() + 30.0),
            line_two,
            tokens.text_secondary,
        );
        y += 64.0;
    }
    let buttons = Rect::from_min_size(pos2(field.left(), y + 10.0), vec2(cell.width(), 32.0));
    let mut save = None;
    let mut cancel = false;
    let mut clear = false;
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(buttons)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let primary = match &record.check {
                check if check.blocks() => None,
                Check::UsedBy { .. } => Some("Replace"),
                Check::HeldByApp { .. } => Some("Save anyway"),
                _ => Some("Save"),
            };
            if let Some(label) = primary.filter(|_| record.first.is_some()) {
                if keyed_button(ui, label, "\u{21B5}", Kind::Primary).clicked() {
                    save = save_changes(record);
                }
            }
            if keyed_button(ui, "Cancel", "Esc", Kind::Secondary).clicked() {
                cancel = true;
            }
            if !matches!(record.check, Check::UsedBy { .. } | Check::HeldByApp { .. })
                && keyed_button(ui, "Clear", "\u{232B}", Kind::Secondary).clicked()
            {
                clear = true;
            }
        },
    );
    if let Some(changes) = save {
        to_host.send(HostRequest::SetShortcuts(changes));
        view.stop_recording();
    } else if cancel {
        view.stop_recording();
    } else if clear {
        if let Some(record) = view.record.as_mut() {
            record.clear();
        }
    }
}

/// A button with its key as a small cap after the label.
fn keyed_button(ui: &mut Ui, label: &str, key: &str, kind: Kind) -> Response {
    let response = button::button(ui, label, kind);
    keycap::hint(ui, key, theme::mono(12.0));
    ui.add_space(8.0);
    response
}

/// The reset button on changed rows and the "…" menu.
fn row_actions(
    ui: &mut Ui,
    view: &mut View,
    info: &ShortcutInfo,
    area: Rect,
    to_host: &Arc<HostChannel>,
) {
    let changed = shortcuts::is_changed(info);
    let more = Rect::from_center_size(pos2(area.right() - 14.0, area.center().y), vec2(28.0, 28.0));
    let reset = Rect::from_center_size(
        pos2(more.left() - 8.0 - 14.0, area.center().y),
        vec2(28.0, 28.0),
    );
    if changed {
        let default = if info.default_binding.is_empty() {
            "no hotkey".to_string()
        } else {
            info.default_binding.clone()
        };
        let response = icon_button(
            ui,
            ui.id().with(("reset", &info.id)),
            reset,
            Icon::RotateCcw,
        )
        .on_hover_text(format!("Reset to default ({default})"));
        if response.clicked() {
            send(to_host, &info.id, None);
        }
    }
    let response = icon_button(ui, ui.id().with(("more", &info.id)), more, Icon::Ellipsis);
    egui::Popup::menu(&response)
        .id(ui.id().with(("menu", &info.id)))
        .width(240.0)
        .show(|ui| {
            ui.set_width(240.0);
            if menu_item(ui, Icon::Keyboard, "Change hotkey", "Enter", true).clicked() {
                view.start_recording(&info.id);
            }
            if menu_item(
                ui,
                Icon::RotateCcw,
                "Reset to default",
                "Ctrl+\u{232B}",
                changed,
            )
            .clicked()
            {
                send(to_host, &info.id, None);
            }
            let removable = !info.one_press && !info.binding.is_empty();
            if menu_item(ui, Icon::Close, "Remove hotkey", "Delete", removable).clicked() {
                send(to_host, &info.id, Some(String::new()));
            }
        });
}

fn icon_button(ui: &mut Ui, id: Id, rect: Rect, icon: Icon) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    let hover = ui
        .ctx()
        .animate_bool_with_time(id.with("hover"), response.hovered(), 0.08);
    ui.painter().rect_filled(
        rect,
        tokens.radius_row,
        tokens.hover_bg.gamma_multiply(hover),
    );
    icons::paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), vec2(16.0, 16.0)),
        icon,
        tokens.text_secondary,
    );
    focus::ring(ui, &response, f32::from(tokens.radius_row));
    response
}

/// 36 high: icon, label, and the key at the right.
fn menu_item(ui: &mut Ui, icon: Icon, label: &str, key: &str, enabled: bool) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), sense);
    let hover = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        enabled && response.hovered(),
        0.08,
    );
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        tokens.radius_row,
        tokens.hover_bg.gamma_multiply(hover),
    );
    let colour = if enabled {
        tokens.text_primary
    } else {
        tokens.text_disabled
    };
    let glyph = Rect::from_center_size(
        pos2(rect.left() + 10.0 + 8.0, rect.center().y),
        vec2(16.0, 16.0),
    );
    icons::paint(painter, glyph, icon, colour);
    painter.text(
        pos2(glyph.right() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::regular(14.0),
        colour,
    );
    painter.text(
        pos2(rect.right() - 10.0, rect.center().y),
        Align2::RIGHT_CENTER,
        key,
        theme::regular(12.0),
        tokens.text_disabled,
    );
    response
}

/// "Reset all hotkeys?" with the names of what goes back.
fn confirm_reset(ui: &mut Ui, view: &mut View, all: &[ShortcutInfo], to_host: &Arc<HostChannel>) {
    let tokens = Tokens::get(ui.ctx());
    let changed: Vec<&ShortcutInfo> = all
        .iter()
        .filter(|info| shortcuts::is_changed(info))
        .collect();
    let modal = egui::Modal::new(Id::new("wincraft-reset-shortcuts")).show(ui.ctx(), |ui| {
        ui.set_width(420.0);
        text::single(
            ui,
            text::job(
                "Reset all hotkeys?",
                theme::semibold(20.0),
                tokens.text_primary,
                Some(26.0),
            ),
        );
        ui.add_space(10.0);
        text::wrapped(
            ui,
            text::job(
                &reset_text(&changed),
                theme::regular(14.0),
                tokens.text_secondary,
                Some(20.0),
            ),
        );
        ui.add_space(16.0);
        let mut done = false;
        ui.horizontal(|ui| {
            if button::button(ui, "Reset all", Kind::Primary).clicked() {
                let changes = changed
                    .iter()
                    .map(|info| ShortcutChange {
                        id: info.id.clone(),
                        binding: None,
                    })
                    .collect();
                to_host.send(HostRequest::SetShortcuts(changes));
                done = true;
            }
            if button::button(ui, "Cancel", Kind::Secondary).clicked() {
                done = true;
            }
        });
        done
    });
    if modal.inner || modal.should_close() || changed.is_empty() {
        view.confirm_reset = false;
    }
}

/// "2 hotkeys go back to their defaults: Arrange strip and Save current
/// layout. You can change them again at any time."
pub fn reset_text(changed: &[&ShortcutInfo]) -> String {
    let names: Vec<&str> = changed.iter().map(|info| info.label.as_str()).collect();
    let listed = match names.as_slice() {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    let (count, verb, pronoun) = if names.len() == 1 {
        ("1 hotkey".to_string(), "goes", "its default")
    } else {
        (format!("{} hotkeys", names.len()), "go", "their defaults")
    };
    let them = if names.len() == 1 { "it" } else { "them" };
    format!("{count} {verb} back to {pronoun}: {listed}. You can change {them} again at any time.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_CONTROL, MOD_WIN};

    fn key(modifiers: u32, vk: u32) -> Hotkey {
        Hotkey {
            modifiers: modifiers | MOD_NOREPEAT,
            vk,
        }
    }

    #[test]
    fn plain_enter_esc_backspace_and_tab_are_controls() {
        assert_eq!(control_of(key(0, 0x0D)), Control::Save);
        assert_eq!(control_of(key(0, 0x08)), Control::Clear);
        assert_eq!(control_of(key(0, 0x1B)), Control::Cancel);
        assert_eq!(control_of(key(0, 0x09)), Control::Cancel);
        assert_eq!(control_of(key(MOD_CONTROL, 0x0D)), Control::Key);
        assert_eq!(control_of(key(MOD_ALT, 0x08)), Control::Key);
        assert_eq!(control_of(key(0, 0x53)), Control::Key);
    }

    #[test]
    fn a_second_key_within_three_seconds_makes_a_chord() {
        let start = Instant::now();
        let mut record = Recording::new("x");
        record.press(key(MOD_WIN | MOD_ALT, 0x4B), start);
        assert!(record.chord_open(start + Duration::from_secs(1)));
        record.press(key(0, 0x53), start + Duration::from_secs(1));
        assert_eq!(text_of(&record.keys().unwrap()), "Win+Alt+K, S");
        assert!(!record.chord_open(start + Duration::from_secs(1)));
        // A key after a full chord starts over.
        record.press(key(MOD_WIN | MOD_ALT, 0x46), start + Duration::from_secs(2));
        assert_eq!(text_of(&record.keys().unwrap()), "Win+Alt+F");
        // A key after the window starts over too.
        record.press(key(MOD_WIN | MOD_ALT, 0x47), start + Duration::from_secs(6));
        assert_eq!(text_of(&record.keys().unwrap()), "Win+Alt+G");
        record.clear();
        assert!(record.keys().is_none());
    }

    fn info(id: &str, label: &str, binding: &str, default: &str) -> ShortcutInfo {
        ShortcutInfo {
            id: id.to_string(),
            command: None,
            group: "LayoutKeeper".to_string(),
            plugin: None,
            category: String::new(),
            icon: None,
            label: label.to_string(),
            description: String::new(),
            binding: binding.to_string(),
            default_binding: default.to_string(),
            registered: true,
            one_press: false,
        }
    }

    #[test]
    fn saving_sends_the_keys_or_a_replace_and_nothing_when_blocked() {
        let mut record = Recording::new("b");
        assert!(save_changes(&record).is_none(), "no keys yet");
        record.press(key(MOD_WIN | MOD_ALT, 0x46), Instant::now());
        assert_eq!(
            save_changes(&record),
            Some(vec![ShortcutChange {
                id: "b".to_string(),
                binding: Some("Win+Alt+F".to_string()),
            }])
        );
        record.check = Check::UsedBy {
            keys: "Win+Alt+A".to_string(),
            id: "a".to_string(),
            label: "Arrange strip".to_string(),
            group: "LayoutKeeper".to_string(),
        };
        assert_eq!(save_changes(&record).map(|changes| changes.len()), Some(2));
        record.check = Check::Reserved {
            keys: "Win+L".to_string(),
        };
        assert!(save_changes(&record).is_none());
    }

    #[test]
    fn the_reset_dialog_names_what_goes_back() {
        let a = info("a", "Arrange strip", "Win+Alt+B", "Win+Alt+A");
        let b = info("b", "Save current layout", "Win+Alt+K, S", "");
        assert_eq!(
            reset_text(&[&a, &b]),
            "2 hotkeys go back to their defaults: Arrange strip and Save current layout. You can change them again at any time."
        );
        assert_eq!(
            reset_text(&[&a]),
            "1 hotkey goes back to its default: Arrange strip. You can change it again at any time."
        );
    }

    #[test]
    fn the_keyboard_walks_open_groups_only() {
        let all = vec![info("a", "A", "", ""), info("b", "B", "", "")];
        let groups = vec![
            ("LayoutKeeper".to_string(), vec![0, 1]),
            ("Other".to_string(), vec![]),
        ];
        let open = spots(&groups, &all, |_| true);
        assert_eq!(open.len(), 4);
        let closed = spots(&groups, &all, |_| false);
        assert_eq!(
            closed,
            vec![
                Spot::Group("LayoutKeeper".into()),
                Spot::Group("Other".into())
            ]
        );
    }

    /// Every theme for the first view only: a theme change rebuilds the
    /// font atlas, which is slow in a test.
    fn draw(view: &mut View, shortcuts: Vec<ShortcutInfo>) {
        draw_in(view, shortcuts, false);
    }

    fn draw_in(view: &mut View, shortcuts: Vec<ShortcutInfo>, every_theme: bool) {
        use crate::core::config::{ThemeChoice, ThemeName};
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let bridge = crate::core::ui_bridge::create();
        let snapshot = UiSnapshot {
            shortcuts,
            ..UiSnapshot::default()
        };
        let themes: &[ThemeName] = if every_theme {
            &ThemeName::ALL
        } else {
            &[ThemeName::Classic]
        };
        for name in themes.iter().copied() {
            for mode in [ThemeChoice::Dark, ThemeChoice::Light] {
                theme::apply(&ctx, name, mode);
                for _ in 0..2 {
                    let input = egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 900.0))),
                        ..Default::default()
                    };
                    let mut output = ctx.run_ui(input, |ui| {
                        show(ui, &snapshot, view, &bridge.to_host);
                    });
                    output.textures_delta.clear();
                }
            }
        }
        assert!(bridge.host_rx.try_recv().is_err(), "drawing sends nothing");
    }

    fn rows() -> Vec<ShortcutInfo> {
        let mut palette = info(
            "wincraft.palette",
            "Open the palette",
            "Win+Alt+P",
            "Win+Alt+P",
        );
        palette.group = HOST_GROUP.to_string();
        palette.one_press = true;
        let mut chord = info("b", "Save current layout", "Win+Alt+K, S", "");
        chord.description = "Keeps where every window is".to_string();
        let mut held = info("c", "Arrange strip", "Win+Alt+A", "Win+Alt+A");
        held.registered = false;
        let mut other = info("d", "Dim all monitors", "", "Win+Alt+D");
        other.group = "ScreenDimmer".to_string();
        vec![palette, chord, held, other]
    }

    #[test]
    fn the_page_draws_in_every_theme_while_idle_searching_and_recording() {
        draw_in(&mut View::default(), rows(), true);
        draw(&mut View::default(), Vec::new());

        let mut searching = View {
            query: "layout".to_string(),
            filter: Filter::Changed,
            ..View::default()
        };
        draw(&mut searching, rows());

        let mut by_keys = View {
            pressed: vec![key(MOD_WIN | MOD_ALT, 0x4B)],
            ..View::default()
        };
        draw(&mut by_keys, rows());

        let mut recording = View {
            selected: Some(Spot::Row("b".to_string())),
            ..View::default()
        };
        let mut record = Recording::new("b");
        record.press(key(MOD_WIN | MOD_ALT, 0x41), Instant::now());
        record.check = Check::UsedBy {
            keys: "Win+Alt+A".to_string(),
            id: "c".to_string(),
            label: "Arrange strip".to_string(),
            group: "LayoutKeeper".to_string(),
        };
        recording.record = Some(record.clone());
        draw(&mut recording, rows());
        record.check = Check::Reserved {
            keys: "Win+L".to_string(),
        };
        recording.record = Some(record);
        draw(&mut recording, rows());

        let mut confirm = View {
            confirm_reset: true,
            ..View::default()
        };
        draw(&mut confirm, rows());
    }

    #[test]
    fn the_group_header_items_do_not_overlap() {
        let row = Rect::from_min_size(pos2(0.0, 0.0), vec2(500.0, GROUP_HEIGHT));
        let HeaderRects { chevron, tile } = header_rects(row);
        assert_eq!(tile.size(), vec2(24.0, 24.0));
        assert!(chevron.right() <= tile.left());
        assert!(
            tile.right() + 10.0 <= row.left() + INDENT,
            "the name clears the tile"
        );
        assert!(row.contains_rect(tile) && row.contains_rect(chevron));
    }

    #[test]
    fn the_search_row_fits_at_the_smallest_and_the_default_window() {
        // Content width: window minus the nav and both margins (720: 168 and
        // 24 each side; 1024: 240 and 44 each side), less room for the scrollbar.
        for (window, nav, side) in [(720.0, 168.0, 24.0), (1024.0, 240.0, 44.0)] {
            let available = window - nav - 2.0 * side - 12.0;
            for (button, help) in [(150.0, 130.0), (170.0, 150.0)] {
                let layout = search_layout(available, button, help);
                let mut used = layout.search + SEARCH_GAP + button;
                if layout.help_inline {
                    assert!(layout.search >= SEARCH_MIN, "{layout:?}");
                    used += SEARCH_GAP + help;
                } else {
                    assert!(help <= available, "the link has a line of its own");
                }
                assert!(
                    used <= available,
                    "{window}: {used} > {available} {layout:?}"
                );
            }
        }
        // Wide: everything on one row and the field capped.
        let wide = search_layout(2000.0, 150.0, 130.0);
        assert_eq!(
            wide,
            SearchLayout {
                search: SEARCH_MAX,
                help_inline: true
            }
        );
    }

    #[test]
    fn the_palette_opens_the_page_on_its_command() {
        let mut view = View::default();
        view.open_at("b", "Save current layout");
        assert_eq!(view.query, "Save current layout");
        assert_eq!(view.selected, Some(Spot::Row("b".to_string())));
        assert!(view.focus_list);
    }

    #[test]
    fn wincraft_and_the_first_plugin_start_open() {
        let view = View::default();
        let groups = vec![
            (HOST_GROUP.to_string(), vec![]),
            ("LayoutKeeper".to_string(), vec![]),
            ("ScreenDimmer".to_string(), vec![]),
        ];
        assert!(is_open(&view, &groups, HOST_GROUP));
        assert!(is_open(&view, &groups, "LayoutKeeper"));
        assert!(!is_open(&view, &groups, "ScreenDimmer"));
        let searching = View {
            query: "dim".to_string(),
            ..View::default()
        };
        assert!(is_open(&searching, &groups, "ScreenDimmer"));
    }
}
