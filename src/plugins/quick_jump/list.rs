//! The pin list: all ten slots with a live picture of each pinned window,
//! and a details panel that says how QuickJump finds the window. Keys:
//! ↑↓ select, Enter jumps, Ctrl+↑↓ or a drag moves a pin, a digit moves it
//! to that slot, Del removes, F2 (or a click on the selected pin) opens the
//! details, Esc closes.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{
    pos2, vec2, Align2, Color32, Key, Painter, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder,
};

use crate::core::theme::{self, Tokens};
use crate::core::traits::{PluginWindow, WindowFrame};
use crate::core::ui_bridge::HostRequest;
use crate::search::icons::IconCache;
use crate::search::IconRef;
use crate::ui::settings;
use crate::ui::widgets::button::{self, Kind};
use crate::ui::widgets::icons::{self, Icon};
use crate::ui::widgets::{choice, focus, input, toggle};

use super::matching::{self, candidates, check_line, CheckTone, SlotView, State};
use super::pictures::Pictures;
use super::pins::{digit_of_slot, slot_of_digit, Pin, SLOTS};
use super::shared::{Handle, Request, View};

pub const WINDOW_ID: &str = "list";
const TITLE: &str = "QuickJump \u{b7} Pins";

const ROW: f32 = 52.0;
const HEADER: f32 = 28.0;
const FOOTER: f32 = 40.0;
const DETAILS: f32 = 360.0;
const SIDE: f32 = 24.0;
const THUMB: egui::Vec2 = egui::Vec2::new(64.0, 36.0);
const PLACE_COLUMN: f32 = 170.0;
const STATE_COLUMN: f32 = 92.0;
const NOT_OPEN: f32 = 0.45;
const REFRESH: Duration = Duration::from_secs(1);

/// The window's size: narrow with no pins yet, wide with the details.
pub fn window_size(pinned: usize, details: bool) -> egui::Vec2 {
    match (pinned, details) {
        (0, false) => vec2(560.0, 340.0),
        (_, true) => vec2(1180.0, 720.0),
        (_, false) => vec2(820.0, 720.0),
    }
}

/// A key the list acts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKey {
    Up,
    Down,
    CtrlUp,
    CtrlDown,
    Enter,
    Digit(u8),
    Delete,
    F2,
    Escape,
}

/// What a key does to the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Select(usize),
    Move { from: usize, to: usize },
    Jump(usize),
    Picker(usize),
    Remove(usize),
    Details(usize),
    CloseDetails,
    Close,
    Nothing,
}

pub fn step(key: ListKey, selected: usize, pinned: &[bool], details: bool) -> Step {
    let here = pinned.get(selected).copied().unwrap_or(false);
    let last = SLOTS - 1;
    match key {
        ListKey::Up => Step::Select(selected.saturating_sub(1)),
        ListKey::Down => Step::Select((selected + 1).min(last)),
        ListKey::CtrlUp if here && selected > 0 => Step::Move {
            from: selected,
            to: selected - 1,
        },
        ListKey::CtrlDown if here && selected < last => Step::Move {
            from: selected,
            to: selected + 1,
        },
        ListKey::Digit(digit) if here => match slot_of_digit(digit) {
            Some(to) if to != selected => Step::Move { from: selected, to },
            _ => Step::Nothing,
        },
        ListKey::Enter if here => Step::Jump(selected),
        ListKey::Enter => Step::Picker(selected),
        ListKey::Delete if here => Step::Remove(selected),
        ListKey::F2 if here => Step::Details(selected),
        ListKey::Escape if details => Step::CloseDetails,
        ListKey::Escape => Step::Close,
        _ => Step::Nothing,
    }
}

/// The pin being edited in the details panel; it changes only on Done.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Draft {
    slot: usize,
    pin: Pin,
}

#[derive(Default)]
struct ListUi {
    selected: usize,
    details: Option<Draft>,
    window: isize,
    focused: bool,
    pictures: Pictures,
    icons: Option<IconCache>,
    drag: Option<usize>,
    refreshed: Option<Instant>,
    closed: bool,
    size: Option<egui::Vec2>,
}

/// What a frame of the list asks for.
#[derive(Debug, PartialEq)]
enum Outcome {
    Send(Request),
    Picker(usize),
    Close,
}

pub struct PinList {
    shared: Handle,
    state: Arc<Mutex<ListUi>>,
}

impl PinList {
    pub fn new(shared: Handle) -> Self {
        Self {
            shared,
            state: Arc::new(Mutex::new(ListUi::default())),
        }
    }
}

fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("wincraft-quick-jump-list")
}

impl PluginWindow for PinList {
    fn id(&self) -> &'static str {
        WINDOW_ID
    }

    fn open(&mut self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.list_open = true;
        }
        if let Ok(mut state) = self.state.lock() {
            state.closed = false;
            state.focused = false;
            state.window = 0;
        }
    }

    fn close(&mut self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.list_open = false;
        }
        if let Ok(mut state) = self.state.lock() {
            state.pictures.clear();
            state.details = None;
            state.window = 0;
        }
    }

    fn is_open(&self) -> bool {
        self.shared.lock().is_ok_and(|shared| shared.list_open)
    }

    fn show(&mut self, ctx: &egui::Context, frame: &WindowFrame) {
        let size = match (self.shared.lock(), self.state.lock()) {
            (Ok(shared), Ok(state)) => window_size(
                shared.view.slots.iter().filter(|v| v.is_pinned()).count(),
                state.details.is_some(),
            ),
            _ => window_size(1, false),
        };
        let mut builder = egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size(size)
            .with_resizable(false)
            .with_maximize_button(false);
        if let Some(icon) = &frame.icon {
            builder = builder.with_icon(Arc::clone(icon));
        }
        let shared = Arc::clone(&self.shared);
        let state = Arc::clone(&self.state);
        let palette = frame.palette_hwnd;
        let to_host = Arc::clone(&frame.to_host);
        ctx.show_viewport_deferred(viewport_id(), builder, move |ui, _class| {
            focus::track(ui.ctx());
            let (Ok(mut shared), Ok(mut state)) = (shared.lock(), state.lock()) else {
                return;
            };
            let tokens = Tokens::get(ui.ctx());
            if state.window == 0 {
                state.window = settings::find_window(TITLE, palette);
            }
            if state.window != 0 {
                settings::sync_title_bar(state.window, tokens.dark);
                if !std::mem::replace(&mut state.focused, true) {
                    to_host.send(HostRequest::FocusWindow(state.window));
                }
            }
            if let Some(slot) = shared.select.take() {
                state.selected = slot;
            }
            if state.icons.is_none() {
                state.icons = Some(IconCache::new(ui.ctx()));
            }
            if state.size != Some(size) {
                state.size = Some(size);
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            }
            if state.refreshed.is_none_or(|at| at.elapsed() >= REFRESH) {
                state.refreshed = Some(Instant::now());
                shared.send(Request::Refresh);
            }
            ui.ctx().request_repaint_after(REFRESH);

            let window = state.window;
            state.pictures.begin(window);
            let view = shared.view.clone();
            let mut outcomes = Vec::new();
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(tokens.window_bg))
                .show(ui, |ui| contents(ui, &view, &mut state, &mut outcomes));
            state.pictures.end();

            if ui.ctx().input(|i| i.viewport().close_requested()) {
                outcomes.push(Outcome::Close);
            }
            for outcome in outcomes {
                match outcome {
                    Outcome::Send(request) => shared.send(request),
                    Outcome::Picker(slot) => {
                        shared.picker = Some(slot);
                        // The picker is drawn by the root's pass, which
                        // sleeps until asked.
                        ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                    }
                    Outcome::Close => state.closed = true,
                }
            }
            if state.closed {
                shared.list_open = false;
                state.closed = false;
                state.pictures.clear();
                state.details = None;
                state.window = 0;
                state.size = None;
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            }
        });
    }
}

fn contents(ui: &mut Ui, view: &View, state: &mut ListUi, out: &mut Vec<Outcome>) {
    let tokens = Tokens::get(ui.ctx());
    let pinned: Vec<bool> = view.slots.iter().map(SlotView::is_pinned).collect();
    let typing = ui.ctx().memory(|memory| memory.focused().is_some());
    let full = ui.max_rect();
    if !pinned.iter().any(|on| *on) && state.details.is_none() {
        first_use(ui, view, full, out);
        if ui.ctx().input(|i| i.key_pressed(Key::Escape)) {
            out.push(Outcome::Close);
        }
        return;
    }
    let details = state.details.is_some();
    let list_area = if details {
        Rect::from_min_max(full.min, pos2(full.right() - DETAILS, full.bottom()))
    } else {
        full
    };

    // Keys, unless a field of the details panel has them.
    for key in pressed_keys(ui, typing, details) {
        match step(key, state.selected, &pinned, details) {
            Step::Select(slot) => state.selected = slot,
            Step::Move { from, to } => {
                out.push(Outcome::Send(Request::Move { from, to }));
                state.selected = to;
                state.details = None;
            }
            Step::Jump(slot) => {
                out.push(Outcome::Send(Request::Jump(slot)));
                out.push(Outcome::Close);
            }
            Step::Picker(slot) => out.push(Outcome::Picker(slot)),
            Step::Remove(slot) => {
                out.push(Outcome::Send(Request::Remove(slot)));
                state.details = None;
            }
            Step::Details(slot) => state.details = draft(view, slot),
            Step::CloseDetails => state.details = None,
            Step::Close => out.push(Outcome::Close),
            Step::Nothing => {}
        }
    }

    let painter = ui.painter().clone();
    let mut y = list_area.top() + 20.0;
    let left = list_area.left() + SIDE;
    let right = list_area.right() - SIDE;
    painter.text(
        pos2(left, y),
        Align2::LEFT_TOP,
        "Pins",
        theme::semibold(20.0),
        tokens.text_primary,
    );
    y += 30.0;
    let count = pinned.iter().filter(|on| **on).count();
    painter.text(
        pos2(left, y),
        Align2::LEFT_TOP,
        format!(
            "{} \u{b7} same list on every desktop",
            matching::pinned_line(count)
        ),
        theme::regular(12.0),
        tokens.text_secondary,
    );
    y += 28.0;

    let columns = Columns::new(left, right);
    let header = Rect::from_min_max(pos2(left, y), pos2(right, y + HEADER));
    for (x, label) in [
        (columns.keycap, "Slot"),
        (columns.window, "Window"),
        (columns.place, "Desktop and monitor"),
        (columns.state, "State"),
    ] {
        painter.text(
            pos2(x, header.center().y),
            Align2::LEFT_CENTER,
            label,
            theme::regular(12.0),
            tokens.text_secondary,
        );
    }
    painter.hline(
        left..=right,
        header.bottom(),
        theme::line(ui.ctx(), tokens.border),
    );
    y = header.bottom();

    let pointer = ui.ctx().pointer_interact_pos();
    let mut row_rects = Vec::new();
    for view_slot in &view.slots {
        let rect = Rect::from_min_size(pos2(left, y), vec2(right - left, ROW));
        row_rects.push(rect);
        let response = ui.interact(
            rect,
            ui.id().with(("quick-jump-row", view_slot.slot)),
            Sense::click_and_drag(),
        );
        if response.clicked() {
            // A click on the pin that is selected already opens its
            // details, so they can be reached without F2 too.
            let again = state.selected == view_slot.slot;
            state.selected = view_slot.slot;
            let other = state
                .details
                .as_ref()
                .is_some_and(|d| d.slot != view_slot.slot);
            if other || again {
                state.details = draft(view, view_slot.slot);
            }
        }
        if response.double_clicked() {
            if view_slot.is_pinned() {
                out.push(Outcome::Send(Request::Jump(view_slot.slot)));
                out.push(Outcome::Close);
            } else {
                out.push(Outcome::Picker(view_slot.slot));
            }
        }
        if response.drag_started() && view_slot.is_pinned() {
            state.drag = Some(view_slot.slot);
        }
        let selected = state.selected == view_slot.slot;
        let hovered = response.hovered();
        draw_row(
            ui,
            &painter,
            RowLook {
                rect,
                view: view_slot,
                selected,
                hovered,
                columns: &columns,
            },
            state,
            out,
        );
        y += ROW;
    }
    if let Some(from) = state.drag {
        if let Some(at) = pointer {
            let target = row_rects.iter().position(|rect| rect.contains(at));
            if let Some(rect) = target.map(|index| row_rects[index]) {
                painter.hline(rect.x_range(), rect.top(), Stroke::new(2.0, tokens.accent));
            }
            if ui.ctx().input(|i| i.pointer.any_released()) {
                if let Some(to) = target.filter(|to| *to != from) {
                    out.push(Outcome::Send(Request::Move { from, to }));
                    state.selected = to;
                }
                state.drag = None;
            }
        }
        if !ui.ctx().input(|i| i.pointer.any_down()) {
            state.drag = None;
        }
    }

    footer(ui, &painter, list_area, &tokens);

    if details {
        let area = Rect::from_min_max(pos2(full.right() - DETAILS, full.top()), full.max);
        details_panel(ui, view, area, state, out);
    }
}

fn pressed_keys(ui: &Ui, typing: bool, details: bool) -> Vec<ListKey> {
    ui.ctx().input(|i| {
        let mut keys = Vec::new();
        if i.key_pressed(Key::Escape) {
            keys.push(ListKey::Escape);
        }
        if typing {
            return keys;
        }
        let ctrl = i.modifiers.ctrl;
        let pairs = [
            (
                Key::ArrowUp,
                if ctrl { ListKey::CtrlUp } else { ListKey::Up },
            ),
            (
                Key::ArrowDown,
                if ctrl {
                    ListKey::CtrlDown
                } else {
                    ListKey::Down
                },
            ),
            (Key::Delete, ListKey::Delete),
            (Key::F2, ListKey::F2),
        ];
        for (key, list_key) in pairs {
            if i.key_pressed(key) {
                keys.push(list_key);
            }
        }
        if i.key_pressed(Key::Enter) && !details {
            keys.push(ListKey::Enter);
        }
        let digits = [
            Key::Num0,
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ];
        for (digit, key) in digits.into_iter().enumerate() {
            if i.key_pressed(key) && !ctrl {
                keys.push(ListKey::Digit(digit as u8));
            }
        }
        keys
    })
}

fn draft(view: &View, slot: usize) -> Option<Draft> {
    let pin = view.pins.get(slot).cloned().flatten()?;
    Some(Draft { slot, pin })
}

/// Where each column starts.
struct Columns {
    grip: f32,
    keycap: f32,
    thumb: f32,
    window: f32,
    place: f32,
    state: f32,
}

impl Columns {
    fn new(left: f32, right: f32) -> Self {
        let gap = 12.0;
        let state = right - STATE_COLUMN;
        let place = state - gap - PLACE_COLUMN;
        let grip = left;
        let keycap = grip + 16.0 + gap;
        let thumb = keycap + 28.0 + gap;
        let window = thumb + THUMB.x + gap;
        Self {
            grip,
            keycap,
            thumb,
            window,
            place,
            state,
        }
    }
}

struct RowLook<'a> {
    rect: Rect,
    view: &'a SlotView,
    selected: bool,
    hovered: bool,
    columns: &'a Columns,
}

fn draw_row(
    ui: &mut Ui,
    painter: &Painter,
    look: RowLook,
    state: &mut ListUi,
    out: &mut Vec<Outcome>,
) {
    let tokens = Tokens::get(ui.ctx());
    let RowLook {
        rect,
        view,
        selected,
        hovered,
        columns,
    } = look;
    if selected {
        painter.rect_filled(rect, tokens.radius_row, tokens.selection_bg);
        painter.rect_filled(
            Rect::from_min_size(rect.min, vec2(2.0, rect.height())),
            0,
            tokens.accent,
        );
    } else if hovered {
        painter.rect_filled(rect, tokens.radius_row, tokens.hover_bg);
    }
    let middle = rect.center().y;
    if (selected || hovered) && view.is_pinned() {
        icons::paint(
            painter,
            Rect::from_center_size(pos2(columns.grip + 8.0, middle), vec2(16.0, 16.0)),
            Icon::Grip,
            tokens.text_disabled,
        );
    }
    paint_keycap(
        painter,
        Rect::from_center_size(pos2(columns.keycap + 12.0, middle), vec2(24.0, 24.0)),
        &view.digit.to_string(),
        &tokens,
    );
    let thumb = Rect::from_min_size(pos2(columns.thumb, middle - THUMB.y / 2.0), THUMB);
    let pixels_per_point = ui.ctx().pixels_per_point();
    match (view.state, view.hwnd) {
        (State::Empty, _) => {
            dashed(painter, thumb, tokens.text_disabled);
            let link = Rect::from_min_size(pos2(columns.window, middle - 12.0), vec2(160.0, 24.0));
            let response = ui.interact(
                link,
                ui.id().with(("quick-jump-pin-link", view.slot)),
                Sense::click(),
            );
            painter.text(
                pos2(columns.window, middle),
                Align2::LEFT_CENTER,
                "+ Pin a window\u{2026}",
                theme::regular(14.0),
                if response.hovered() {
                    tokens.accent_hover
                } else {
                    tokens.accent
                },
            );
            if response.clicked() {
                out.push(Outcome::Picker(view.slot));
            }
            return;
        }
        (State::Open | State::InFront, Some(hwnd)) => {
            painter.rect_filled(thumb, 4, tokens.panel_bg);
            if !state.pictures.show(hwnd, thumb, pixels_per_point) {
                name_card(painter, thumb, &view.name, &tokens, 1.0);
            }
            // DWM draws over the window's own pixels, so the badge sits on
            // the picture's corner rather than inside it.
            app_icon(
                ui,
                painter,
                state,
                view,
                Rect::from_min_size(thumb.max - vec2(12.0, 12.0), vec2(18.0, 18.0)),
                1.0,
            );
        }
        _ => {
            painter.rect_filled(thumb, 4, theme::with_alpha(tokens.panel_bg, NOT_OPEN));
            app_icon(
                ui,
                painter,
                state,
                view,
                Rect::from_center_size(thumb.center(), vec2(24.0, 24.0)),
                NOT_OPEN,
            );
        }
    }
    let dim = if view.state == State::NotOpen {
        NOT_OPEN
    } else {
        1.0
    };
    let text_width = columns.place - 12.0 - columns.window;
    let title = clip_text(painter, &view.name, theme::regular(14.0), text_width);
    painter.text(
        pos2(columns.window, middle - 2.0),
        Align2::LEFT_BOTTOM,
        title,
        theme::regular(14.0),
        theme::with_alpha(tokens.text_primary, dim),
    );
    let second = if view.title.is_empty() || view.title == view.name {
        view.app_name.clone()
    } else {
        format!("{} \u{b7} {}", view.app_name, view.title)
    };
    let second = clip_text(painter, &second, theme::regular(12.0), text_width);
    painter.text(
        pos2(columns.window, middle + 2.0),
        Align2::LEFT_TOP,
        second,
        theme::regular(12.0),
        theme::with_alpha(tokens.text_secondary, dim),
    );
    painter.text(
        pos2(columns.place, middle),
        Align2::LEFT_CENTER,
        clip_text(painter, &view.place, theme::regular(12.0), PLACE_COLUMN),
        theme::regular(12.0),
        tokens.text_secondary,
    );
    let (dot, word) = match view.state {
        State::InFront => (tokens.success, "in front"),
        State::Open => (tokens.success, "open"),
        _ => (tokens.text_disabled, "not open"),
    };
    painter.circle_filled(pos2(columns.state + 3.0, middle), 3.0, dot);
    painter.text(
        pos2(columns.state + 12.0, middle),
        Align2::LEFT_CENTER,
        word,
        theme::regular(12.0),
        tokens.text_secondary,
    );
}

fn app_icon(
    ui: &Ui,
    painter: &Painter,
    state: &mut ListUi,
    view: &SlotView,
    rect: Rect,
    alpha: f32,
) {
    let reference = match view.hwnd {
        Some(hwnd) => IconRef::Window {
            hwnd,
            exe: PathBuf::from(&view.exe),
        },
        None => IconRef::Path(PathBuf::from(&view.exe)),
    };
    let texture = state
        .icons
        .as_mut()
        .and_then(|cache| cache.get(ui.ctx(), &reference));
    if let Some(texture) = texture {
        painter.image(
            texture.id(),
            rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE.gamma_multiply(alpha),
        );
    }
}

fn name_card(painter: &Painter, area: Rect, name: &str, tokens: &Tokens, alpha: f32) {
    let text = clip_text(painter, name, theme::regular(12.0), area.width() - 8.0);
    painter.text(
        area.center(),
        Align2::CENTER_CENTER,
        text,
        theme::regular(12.0),
        theme::with_alpha(tokens.text_secondary, alpha),
    );
}

fn paint_keycap(painter: &Painter, rect: Rect, key: &str, tokens: &Tokens) {
    painter.rect(
        rect,
        4,
        tokens.panel_bg,
        Stroke::new(1.0, tokens.border),
        StrokeKind::Inside,
    );
    painter.hline(
        rect.x_range().shrink(2.0),
        rect.bottom() - 1.0,
        Stroke::new(2.0, tokens.border),
    );
    painter.text(
        rect.center() - vec2(0.0, 1.0),
        Align2::CENTER_CENTER,
        key,
        theme::semibold(12.0),
        tokens.text_secondary,
    );
}

fn dashed(painter: &Painter, rect: Rect, colour: Color32) {
    let stroke = Stroke::new(1.0, colour);
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    for pair in corners.windows(2) {
        painter.extend(egui::Shape::dashed_line(&pair[..2], stroke, 4.0, 3.0));
    }
}

/// The text cut with an ellipsis to fit `width`.
pub fn clip_text(painter: &Painter, text: &str, font: egui::FontId, width: f32) -> String {
    let fits = |candidate: &str| {
        painter
            .layout_no_wrap(candidate.to_string(), font.clone(), Color32::WHITE)
            .size()
            .x
            <= width
    };
    if fits(text) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut keep = chars.len();
    while keep > 0 {
        keep -= 1;
        let candidate = format!(
            "{}\u{2026}",
            chars[..keep].iter().collect::<String>().trim_end()
        );
        if fits(&candidate) {
            return candidate;
        }
    }
    "\u{2026}".to_string()
}

fn footer(ui: &Ui, painter: &Painter, area: Rect, tokens: &Tokens) {
    let rect = Rect::from_min_max(pos2(area.left(), area.bottom() - FOOTER), area.max);
    painter.rect_filled(rect, 0, tokens.elevated_bg);
    painter.hline(
        rect.x_range(),
        rect.top(),
        theme::line(ui.ctx(), tokens.border),
    );
    painter.text(
        pos2(rect.left() + SIDE, rect.center().y),
        Align2::LEFT_CENTER,
        "\u{21b5} Jump \u{b7} Ctrl+\u{2191}\u{2193} Reorder \u{b7} 1\u{2013}0 Move to slot \u{b7} Del Remove \u{b7} F2 Details \u{b7} Esc Close",
        theme::regular(12.0),
        tokens.text_secondary,
    );
}

fn first_use(ui: &mut Ui, view: &View, area: Rect, out: &mut Vec<Outcome>) {
    let tokens = Tokens::get(ui.ctx());
    let inner = area.shrink2(vec2(32.0, 28.0));
    ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.label(
            egui::RichText::new("No pins yet")
                .font(theme::semibold(20.0))
                .color(tokens.text_primary),
        );
        ui.label(
            egui::RichText::new("Pin up to 10 windows and reach each one with a number.")
                .font(theme::regular(14.0))
                .color(tokens.text_secondary),
        );
        ui.add_space(8.0);
        for (keys, line) in [
            (&view.pin_keys, "Pin the window in front to a number."),
            (&view.jump_keys, "Jump back to it from anywhere."),
        ] {
            if keys.is_empty() {
                continue;
            }
            ui.horizontal(|ui| {
                let steps = vec![
                    crate::ui::widgets::keycap::split(keys)
                        .into_iter()
                        .map(str::to_string)
                        .collect::<Vec<_>>(),
                    vec!["1\u{2013}0".to_string()],
                ];
                let mut look = crate::ui::widgets::keycap::Caps::page();
                look.matched = 0;
                crate::ui::widgets::keycap::caps(ui, &steps, look);
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(line)
                        .font(theme::regular(14.0))
                        .color(tokens.text_primary),
                );
            });
        }
        ui.add_space(12.0);
        if button::with_icon(ui, "Pin a window\u{2026}", Kind::Primary, None).clicked() {
            out.push(Outcome::Picker(0));
        }
    });
}

fn details_panel(ui: &mut Ui, view: &View, area: Rect, state: &mut ListUi, out: &mut Vec<Outcome>) {
    let tokens = Tokens::get(ui.ctx());
    ui.painter().rect_filled(area, 0, tokens.panel_bg);
    ui.painter().vline(
        area.left(),
        area.y_range(),
        theme::line(ui.ctx(), tokens.border),
    );
    let Some(draft) = state.details.as_mut() else {
        return;
    };
    let slot = draft.slot;
    let view_slot = view.slots.get(slot).cloned().unwrap_or_default();
    let mut done = false;
    let mut remove = false;
    let inner = area.shrink(20.0);
    ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
            paint_keycap(
                ui.painter(),
                rect,
                &digit_of_slot(slot).to_string(),
                &tokens,
            );
            ui.label(
                egui::RichText::new(format!("Slot {} details", digit_of_slot(slot)))
                    .font(theme::semibold(18.0))
                    .color(tokens.text_primary),
            );
        });
        let place = [view_slot.app_name.as_str(), view_slot.place.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" \u{b7} ");
        caption(ui, &place, tokens.text_secondary);
        ui.add_space(12.0);

        field_label(ui, "Label (optional)", &tokens);
        let mut label = draft.pin.label.clone().unwrap_or_default();
        if input::text(
            ui,
            "quick-jump-label",
            &mut label,
            input::Face::Text,
            inner.width(),
        )
        .changed()
        {
            draft.pin.label = (!label.trim().is_empty()).then_some(label);
        }
        caption(
            ui,
            "Shown in the strip and the list instead of the app name.",
            tokens.text_secondary,
        );
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new("How QuickJump finds this window")
                .font(theme::semibold(13.0))
                .color(tokens.text_primary),
        );
        field_label(ui, "App", &tokens);
        let options: Vec<String> = view
            .apps
            .iter()
            .map(|app| {
                format!(
                    "{} \u{b7} {}",
                    app.name,
                    crate::core::window_identity::exe_key(&app.exe)
                )
            })
            .collect();
        let labels: Vec<&str> = options.iter().map(String::as_str).collect();
        let current = view.apps.iter().position(|app| {
            app.exe.eq_ignore_ascii_case(&draft.pin.exe) && app.app_id == draft.pin.app_id
        });
        if let Some(chosen) = choice::dropdown_sized(
            ui,
            "quick-jump-app",
            &labels,
            current,
            "",
            vec2(inner.width(), 32.0),
        ) {
            if let Some(app) = view.apps.get(chosen) {
                draft.pin.exe = app.exe.clone();
                draft.pin.app_id = app.app_id.clone();
                draft.pin.app_name = app.name.clone();
            }
        }
        field_label(ui, "Title contains", &tokens);
        input::text(
            ui,
            "quick-jump-title",
            &mut draft.pin.title,
            input::Face::Text,
            inner.width(),
        );
        let matches = candidates(&draft.pin, &view.windows);
        let (line, tone) = check_line(matches.len());
        let colour = match tone {
            CheckTone::Success => tokens.success,
            CheckTone::Warning => tokens.warning,
            CheckTone::Quiet => tokens.text_disabled,
        };
        caption(ui, &line, colour);
        if let Some(first) = matches.first() {
            let place = first.place();
            let line = if place.is_empty() {
                first.shown.clone()
            } else {
                format!("{} \u{b7} {place}", first.shown)
            };
            caption(ui, &line, tokens.text_secondary);
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            toggle::toggle(ui, &mut draft.pin.start);
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("If it is not open, start {}", draft.pin.app_name))
                    .font(theme::regular(14.0))
                    .color(tokens.text_primary),
            );
        });
        let buttons = Rect::from_min_max(pos2(inner.left(), inner.bottom() - 32.0), inner.max);
        ui.scope_builder(UiBuilder::new().max_rect(buttons), |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if button::button(ui, "Done", Kind::Primary).clicked() {
                    done = true;
                }
                ui.add_space(8.0);
                if button::button(ui, "Remove pin", Kind::Danger).clicked() {
                    remove = true;
                }
            });
        });
    });
    let enter = ui.ctx().input(|i| i.key_pressed(Key::Enter));
    if done || enter {
        out.push(Outcome::Send(Request::Update {
            slot,
            pin: draft.pin.clone(),
        }));
        state.details = None;
    } else if remove {
        out.push(Outcome::Send(Request::Remove(slot)));
        state.details = None;
    }
}

fn field_label(ui: &mut Ui, text: &str, tokens: &Tokens) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(text)
            .font(theme::regular(12.0))
            .color(tokens.text_secondary),
    );
}

fn caption(ui: &mut Ui, text: &str, colour: Color32) {
    ui.label(
        egui::RichText::new(text)
            .font(theme::regular(12.0))
            .color(colour),
    );
}

/// The window the pin list draws on, for tests that check nothing is
/// drawn on a real one.
#[cfg(test)]
fn window_of(state: &ListUi) -> windows_sys::Win32::Foundation::HWND {
    state.window as windows_sys::Win32::Foundation::HWND
}

#[cfg(test)]
mod tests {
    use super::super::matching::slot_views;
    use super::super::matching::tests::window;
    use super::super::pins::tests::pin;
    use super::super::pins::Slots;
    use super::*;
    use crate::core::config::ThemeName;

    fn pinned_at(slots: &[usize]) -> Vec<bool> {
        (0..SLOTS).map(|slot| slots.contains(&slot)).collect()
    }

    #[test]
    fn the_list_keys_select_move_swap_jump_and_remove() {
        let pinned = pinned_at(&[0, 2]);
        assert_eq!(step(ListKey::Down, 0, &pinned, false), Step::Select(1));
        assert_eq!(step(ListKey::Up, 0, &pinned, false), Step::Select(0));
        assert_eq!(step(ListKey::Down, 9, &pinned, false), Step::Select(9));
        assert_eq!(
            step(ListKey::CtrlDown, 0, &pinned, false),
            Step::Move { from: 0, to: 1 }
        );
        assert_eq!(step(ListKey::CtrlUp, 0, &pinned, false), Step::Nothing);
        assert_eq!(step(ListKey::CtrlUp, 1, &pinned, false), Step::Nothing);
        assert_eq!(
            step(ListKey::Digit(3), 0, &pinned, false),
            Step::Move { from: 0, to: 2 },
            "onto a full slot it swaps"
        );
        assert_eq!(
            step(ListKey::Digit(0), 2, &pinned, false),
            Step::Move { from: 2, to: 9 }
        );
        assert_eq!(step(ListKey::Digit(1), 0, &pinned, false), Step::Nothing);
        assert_eq!(step(ListKey::Enter, 0, &pinned, false), Step::Jump(0));
        assert_eq!(step(ListKey::Enter, 1, &pinned, false), Step::Picker(1));
        assert_eq!(step(ListKey::Delete, 2, &pinned, false), Step::Remove(2));
        assert_eq!(step(ListKey::Delete, 1, &pinned, false), Step::Nothing);
        assert_eq!(step(ListKey::F2, 0, &pinned, false), Step::Details(0));
        assert_eq!(step(ListKey::Escape, 0, &pinned, true), Step::CloseDetails);
        assert_eq!(step(ListKey::Escape, 0, &pinned, false), Step::Close);
    }

    #[test]
    fn the_window_is_narrow_until_something_is_pinned() {
        assert_eq!(window_size(0, false).x, 560.0);
        assert_eq!(window_size(3, false).x, 820.0);
        assert_eq!(window_size(3, true).x, 1180.0);
    }

    fn view() -> View {
        let mut slots = Slots::default();
        slots.pin(0, pin("Claude", None));
        slots.pin(2, pin("Code", None));
        let live = vec![window(1, "Claude", "Claude")];
        View {
            slots: slot_views(&slots, &live, 1),
            pins: (0..SLOTS).map(|slot| slots.get(slot).cloned()).collect(),
            windows: live,
            apps: vec![super::super::shared::App {
                exe: r"C:\Apps\Claude.exe".to_string(),
                app_id: None,
                name: "Claude".to_string(),
            }],
            jump_keys: "Win+Alt+Q".to_string(),
            pin_keys: "Win+Alt+Shift+Q".to_string(),
        }
    }

    /// Draws one frame per entry of `frames`, each with its events, then
    /// one more in each theme with none.
    fn frames(view: &View, state: &mut ListUi, frames: Vec<Vec<egui::Event>>) -> Vec<Outcome> {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let mut out = Vec::new();
        let themes = ThemeName::ALL.into_iter().map(|name| (name, Vec::new()));
        let steps = frames
            .into_iter()
            .map(|events| (ThemeName::Classic, events))
            .chain(themes);
        for (name, events) in steps {
            theme::apply(&ctx, name, crate::core::config::ThemeChoice::Dark);
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1180.0, 720.0))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| contents(ui, view, state, &mut out));
            output.textures_delta.clear();
        }
        out
    }

    fn draw(view: &View, state: &mut ListUi, events: Vec<egui::Event>) -> Vec<Outcome> {
        frames(view, state, vec![Vec::new(), events])
    }

    fn key(key: Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn the_list_and_its_details_draw_in_every_theme() {
        let view = view();
        let mut state = ListUi::default();
        let first = draw(&view, &mut state, Vec::new());
        assert!(first.is_empty(), "{first:?}");
        state.details = draft(&view, 0);
        assert!(state.details.is_some());
        draw(&view, &mut state, Vec::new());
        assert!(window_of(&state).is_null(), "no real window is touched");
        let out = draw(&view, &mut state, vec![key(Key::Escape)]);
        assert!(out.is_empty());
        assert!(state.details.is_none(), "Esc drops the details first");
    }

    fn click(at: egui::Pos2) -> Vec<Vec<egui::Event>> {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        vec![
            Vec::new(),
            vec![egui::Event::PointerMoved(at)],
            vec![button(true)],
            vec![button(false)],
        ]
    }

    #[test]
    fn a_click_on_the_selected_pin_opens_its_details() {
        let view = view();
        let mut state = ListUi::default();
        // The first row, slot 1, which starts selected.
        frames(&view, &mut state, click(pos2(400.0, 130.0)));
        assert_eq!(state.details.as_ref().map(|d| d.slot), Some(0));
    }

    #[test]
    fn keys_in_the_list_become_requests() {
        let view = view();
        let mut state = ListUi::default();
        let out = draw(&view, &mut state, vec![key(Key::Delete)]);
        assert!(out.contains(&Outcome::Send(Request::Remove(0))));
        let mut state = ListUi::default();
        let out = draw(&view, &mut state, vec![key(Key::Enter)]);
        assert!(out.contains(&Outcome::Send(Request::Jump(0))));
        assert!(out.contains(&Outcome::Close));
    }

    #[test]
    fn with_no_pins_the_first_use_page_shows() {
        let mut empty = view();
        empty.slots = slot_views(&Slots::default(), &[], 0);
        let mut state = ListUi::default();
        let out = draw(&empty, &mut state, vec![key(Key::Escape)]);
        assert!(out.contains(&Outcome::Close));
    }
}
