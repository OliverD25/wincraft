//! The detector's window, drawn with egui on the UI thread: a toolbar, the
//! Check a shortcut box, the list of combinations and a bottom line with the
//! scan's numbers. Keys: Ctrl+F search, ↑↓ select a row, Ctrl+C copy the
//! rows shown, F5 scan again, Enter or Space on the box to check a shortcut,
//! Esc close.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::text::{LayoutJob, TextWrapping};
use egui::{
    pos2, vec2, Align, Align2, Event, Id, Key, Layout, Modifiers, Rect, RichText, Sense,
    StrokeKind, Ui, UiBuilder,
};
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
};

use crate::core::theme::{self, Tokens};
use crate::core::traits::{Hotkey, PluginWindow, WindowFrame};
use crate::core::ui_bridge::HostRequest;
use crate::core::{clipboard, hotkeys};
use crate::ui::settings;
use crate::ui::widgets::button::{self, Kind};
use crate::ui::widgets::icons::Icon;
use crate::ui::widgets::keycap::{self, Caps};
use crate::ui::widgets::search_box::search_box;
use crate::ui::widgets::{choice, focus, hotkey_capture, text, toggle};

use super::probe::{self, Entry, ScanResult};
use super::rows::{self, Filter, Sort, Tone, COLUMNS, MODIFIERS, SIDE};
use super::shared::{Handle, Shared};

pub const WINDOW_ID: &str = "detector";
const TITLE: &str = "WinCraft \u{b7} Shortcut detector";
const SIZE: [f32; 2] = [760.0, 560.0];
const MIN_SIZE: [f32; 2] = [560.0, 400.0];

const ROW: f32 = 38.0;
const HEADER: f32 = 32.0;
const CONTROL: f32 = 32.0;
const SEARCH_HEIGHT: f32 = 36.0;
const SEARCH_MIN: f32 = 200.0;
const SEARCH_MAX: f32 = 280.0;
const GAP: f32 = 12.0;
const CAPTURE_MIN: f32 = 200.0;
const CHECK_PAD: egui::Vec2 = egui::Vec2::new(14.0, 10.0);
const PLACEHOLDER: &str = "Search shortcuts or meaning";
const CHECK_HINT: &str = "Click, then press keys";
const CHANGED: &str = " \u{b7} changed since the last scan, press F5";

const CAPTURE_POLL: Duration = Duration::from_millis(30);
const SCAN_POLL: Duration = Duration::from_millis(100);
const NOTICE_TIME: Duration = Duration::from_secs(3);

/// What the window reads from the plugin each frame.
struct View {
    scan: Option<Arc<ScanResult>>,
    scanning: bool,
    show_free: bool,
}

#[derive(Clone, PartialEq)]
struct Shown {
    scan: usize,
    query: String,
    modifier: usize,
    show_free: bool,
    sort: Sort,
}

#[derive(Default)]
struct WindowUi {
    query: String,
    modifier: usize,
    sort: Sort,
    /// A place in the rows shown, not an index into the scan.
    selected: Option<usize>,
    follow: bool,
    offset: f32,
    capturing: bool,
    /// The modifiers held while the box listens.
    live: u32,
    keys: Option<Hotkey>,
    verdict: Option<(String, Tone)>,
    notice: Option<(String, Instant)>,
    window: isize,
    focused: bool,
    placed: bool,
    closed: bool,
    rows: Option<(Shown, Vec<usize>)>,
}

/// What a frame of the window asks for.
#[derive(Debug, PartialEq)]
enum Outcome {
    Scan,
    Copy(String),
    ShowFree(bool),
    StartCapture,
    StopCapture,
    Close,
}

pub struct DetectorWindow {
    shared: Handle,
    state: Arc<Mutex<WindowUi>>,
}

impl DetectorWindow {
    pub fn new(shared: Handle) -> Self {
        Self {
            shared,
            state: Arc::new(Mutex::new(WindowUi::default())),
        }
    }
}

fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("wincraft-shortcut-detector")
}

impl PluginWindow for DetectorWindow {
    fn id(&self) -> &'static str {
        WINDOW_ID
    }

    /// A second open only brings the window to the front.
    fn open(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.focused = false;
            state.closed = false;
        }
    }

    fn close(&mut self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.open = false;
        }
        if let Ok(mut state) = self.state.lock() {
            if state.capturing {
                hotkey_capture::stop();
            }
            *state = WindowUi::default();
        }
    }

    fn is_open(&self) -> bool {
        self.shared.lock().is_ok_and(|shared| shared.open)
    }

    fn show(&mut self, ctx: &egui::Context, frame: &WindowFrame) {
        let mut builder = egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_inner_size(SIZE)
            .with_min_inner_size(MIN_SIZE);
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
                if !std::mem::replace(&mut state.placed, true) {
                    centre_on_cursor_monitor(state.window as HWND);
                }
                if !std::mem::replace(&mut state.focused, true) {
                    to_host.send(HostRequest::FocusWindow(state.window));
                }
            }
            if state.capturing {
                listen(ui, &shared, &mut state);
            }
            if shared.scanning {
                ui.ctx().request_repaint_after(SCAN_POLL);
            }
            if let Some((_, at)) = &state.notice {
                match NOTICE_TIME.checked_sub(at.elapsed()) {
                    Some(left) => ui.ctx().request_repaint_after(left),
                    None => state.notice = None,
                }
            }

            let view = View {
                scan: shared.scan.clone(),
                scanning: shared.scanning,
                show_free: shared.show_free,
            };
            let mut outcomes = Vec::new();
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(tokens.window_bg))
                .show(ui, |ui| contents(ui, &view, &mut state, &mut outcomes));
            if ui.ctx().input(|i| i.viewport().close_requested()) {
                outcomes.push(Outcome::Close);
            }
            for outcome in outcomes {
                act(outcome, &mut shared, &mut state);
            }
            if state.closed {
                if state.capturing {
                    hotkey_capture::stop();
                }
                shared.open = false;
                *state = WindowUi::default();
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            }
        });
    }
}

fn act(outcome: Outcome, shared: &mut Shared, state: &mut WindowUi) {
    match outcome {
        Outcome::Scan => shared.ask_scan(),
        Outcome::Copy(text) => {
            let rows = text.lines().count().saturating_sub(1);
            let line = if clipboard::put_text(state.window as HWND, &text) {
                format!("Copied {rows} rows.")
            } else {
                "Could not open the clipboard.".to_string()
            };
            state.notice = Some((line, Instant::now()));
        }
        Outcome::ShowFree(on) => shared.show_free = on,
        Outcome::StartCapture => {
            hotkey_capture::start();
            state.capturing = true;
            state.live = 0;
            state.keys = None;
            state.verdict = None;
        }
        Outcome::StopCapture => stop_listening(state),
        Outcome::Close => state.closed = true,
    }
}

/// Takes what the keyboard hook caught. The hook hides keys from every
/// window, so it stops the moment this window is no longer in front.
fn listen(ui: &Ui, shared: &Shared, state: &mut WindowUi) {
    ui.ctx().request_repaint_after(CAPTURE_POLL);
    let in_front = ui.input(|i| i.viewport().focused).unwrap_or(true);
    if !in_front || hotkey_capture::take_cancelled() {
        if in_front {
            state.keys = None;
            state.verdict = None;
        }
        stop_listening(state);
        return;
    }
    state.live = hotkey_capture::live_modifiers();
    if let Some(hotkey) = hotkey_capture::take_result() {
        state.keys = Some(hotkey);
        state.verdict = Some(verdict(hotkey, shared));
    }
}

fn stop_listening(state: &mut WindowUi) {
    hotkey_capture::stop();
    state.capturing = false;
    state.live = 0;
}

/// Today's answer for one combination, and a note when it is not what the
/// last scan said.
fn verdict(hotkey: Hotkey, shared: &Shared) -> (String, Tone) {
    let entry = probe::verdict(std::ptr::null_mut(), hotkey, &shared.mine);
    let before = shared
        .scan
        .as_ref()
        .and_then(|scan| scan.find(hotkey))
        .map(|found| found.status);
    verdict_line(&entry, before)
}

fn verdict_line(entry: &Entry, before: Option<probe::Status>) -> (String, Tone) {
    let mut line = probe::verdict_text(entry);
    if before.is_some_and(|before| before != entry.status) {
        line.push_str(CHANGED);
    }
    (line, rows::look(entry.status).1)
}

fn search_id() -> Id {
    Id::new("wincraft-detector-search")
}

fn capture_id() -> Id {
    Id::new("wincraft-detector-capture")
}

fn contents(ui: &mut Ui, view: &View, state: &mut WindowUi, out: &mut Vec<Outcome>) {
    let full = ui.max_rect();
    let visible = shown_rows(view, state);
    if let Some(selected) = state.selected {
        if selected >= visible.len() {
            state.selected = visible.len().checked_sub(1);
        }
    }
    if !state.capturing {
        keys(ui, view, state, &visible, out);
    }

    let mut y = full.top() + 8.0;
    y = toolbar(ui, view, state, full, y, out) + 16.0;
    y = check_box(ui, state, full, y, out) + 16.0;
    let footer = footer_height(ui, view, state, full.width() - SIDE * 2.0);
    let list = Rect::from_min_max(
        pos2(full.left(), y),
        pos2(full.right(), full.bottom() - footer),
    );
    list_area(ui, view, state, &visible, list);
    footer_line(
        ui,
        view,
        state,
        Rect::from_min_max(pos2(full.left(), list.bottom()), full.max),
    );
}

/// The rows to show, worked out again only when the scan or a filter
/// changed: a full scan has over 1,500 of them.
fn shown_rows(view: &View, state: &mut WindowUi) -> Vec<usize> {
    let Some(scan) = &view.scan else {
        return Vec::new();
    };
    let wanted = Shown {
        scan: Arc::as_ptr(scan) as usize,
        query: state.query.trim().to_lowercase(),
        modifier: state.modifier,
        show_free: view.show_free,
        sort: state.sort,
    };
    if let Some((shown, rows)) = &state.rows {
        if *shown == wanted {
            return rows.clone();
        }
    }
    let filter = Filter {
        query: &wanted.query,
        modifier: rows::modifier_of(wanted.modifier),
        show_free: wanted.show_free,
    };
    let found = rows::visible(&scan.entries, &filter, wanted.sort);
    state.rows = Some((wanted, found.clone()));
    found
}

fn keys(ui: &mut Ui, view: &View, state: &mut WindowUi, visible: &[usize], out: &mut Vec<Outcome>) {
    let typing = ui.memory(|memory| memory.has_focus(search_id()));
    let on_box = ui.memory(|memory| memory.has_focus(capture_id()));
    let (escape, f5, find, down, up, start, copy) = ui.input_mut(|i| {
        let copy = !typing
            && (i.events.iter().any(|event| matches!(event, Event::Copy))
                || i.consume_key(Modifiers::COMMAND, Key::C));
        (
            i.consume_key(Modifiers::NONE, Key::Escape),
            i.consume_key(Modifiers::NONE, Key::F5),
            i.consume_key(Modifiers::COMMAND, Key::F),
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            on_box
                && (i.consume_key(Modifiers::NONE, Key::Enter)
                    || i.consume_key(Modifiers::NONE, Key::Space)),
            copy,
        )
    });
    if escape {
        out.push(Outcome::Close);
    }
    if f5 {
        out.push(Outcome::Scan);
    }
    if find {
        ui.memory_mut(|memory| memory.request_focus(search_id()));
    }
    if start {
        out.push(Outcome::StartCapture);
    }
    if !visible.is_empty() && (down || up) {
        let last = visible.len() - 1;
        state.selected = Some(match (state.selected, down) {
            (None, _) => 0,
            (Some(at), true) => (at + 1).min(last),
            (Some(at), false) => at.saturating_sub(1),
        });
        state.follow = true;
    }
    if copy {
        if let Some(scan) = &view.scan {
            out.push(Outcome::Copy(rows::copy_text(&scan.entries, visible)));
        }
    }
}

/// Which toolbar items go on which line: each in order, a new line when the
/// next no longer fits. The search box counts at its smallest width.
fn flow(widths: &[f32], available: f32) -> Vec<Vec<usize>> {
    let mut lines: Vec<Vec<usize>> = Vec::new();
    let mut used = 0.0;
    for (index, width) in widths.iter().enumerate() {
        match lines.last_mut() {
            Some(line) if used + GAP + width <= available => {
                line.push(index);
                used += GAP + width;
            }
            _ => {
                lines.push(vec![index]);
                used = *width;
            }
        }
    }
    lines
}

fn text_width(ui: &Ui, text: &str, size: f32) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.to_string(),
            theme::regular(size),
            egui::Color32::PLACEHOLDER,
        )
        .size()
        .x
}

fn toolbar(
    ui: &mut Ui,
    view: &View,
    state: &mut WindowUi,
    full: Rect,
    top: f32,
    out: &mut Vec<Outcome>,
) -> f32 {
    let tokens = Tokens::get(ui.ctx());
    let left = full.left() + SIDE;
    let right = full.right() - SIDE;
    let available = right - left;
    // The same sums the shared widgets make: 14 px padding each side of a
    // segment, a 40 px switch, and 12 px padding, a 13 px icon and 6 px gaps
    // in a button.
    let segments: f32 = MODIFIERS
        .iter()
        .map(|part| text_width(ui, part, 13.0) + 28.0)
        .sum();
    let switch = 40.0 + 8.0 + text_width(ui, "Show free", 13.0);
    let buttons = text_width(ui, "Refresh", 14.0)
        + 6.0
        + text_width(ui, "F5", 12.0)
        + text_width(ui, "Copy list", 14.0)
        + 2.0 * (24.0 + 13.0 + 6.0)
        + 8.0;
    let widths = [SEARCH_MIN, segments, switch, buttons];

    let mut y = top;
    for line in flow(&widths, available) {
        let used: f32 =
            line.iter().map(|index| widths[*index]).sum::<f32>() + GAP * (line.len() - 1) as f32;
        let height = if line.contains(&0) {
            SEARCH_HEIGHT
        } else {
            CONTROL
        };
        let mut x = left;
        for index in line {
            let width = if index == 0 {
                (SEARCH_MIN + available - used).min(SEARCH_MAX)
            } else {
                widths[index]
            };
            let (rect, layout) = if index == 3 {
                (
                    Rect::from_min_max(pos2(left, y), pos2(right, y + height)),
                    Layout::right_to_left(Align::Center),
                )
            } else {
                (
                    Rect::from_min_size(pos2(x, y), vec2(width, height)),
                    Layout::left_to_right(Align::Center),
                )
            };
            x += width + GAP;
            ui.scope_builder(
                UiBuilder::new().max_rect(rect).layout(layout),
                |ui| match index {
                    0 => {
                        search_box(ui, search_id(), &mut state.query, PLACEHOLDER, width);
                    }
                    1 => {
                        if let Some(part) = choice::segmented(ui, &MODIFIERS, Some(state.modifier))
                        {
                            state.modifier = part;
                            state.selected = None;
                        }
                    }
                    2 => {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let mut on = view.show_free;
                        let switched = toggle::toggle(ui, &mut on).changed();
                        let label = ui.add(
                            egui::Label::new(
                                RichText::new("Show free")
                                    .font(theme::regular(13.0))
                                    .color(tokens.text_secondary),
                            )
                            .selectable(false)
                            .sense(Sense::click()),
                        );
                        if switched {
                            out.push(Outcome::ShowFree(on));
                        } else if label.clicked() {
                            out.push(Outcome::ShowFree(!view.show_free));
                        }
                    }
                    _ => {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let copy =
                            button::with_icon(ui, "Copy list", Kind::Secondary, Some(Icon::Copy));
                        let refresh =
                            button::with_hint(ui, "Refresh", Kind::Secondary, Icon::Refresh, "F5");
                        if refresh.clicked() {
                            out.push(Outcome::Scan);
                        }
                        if copy.clicked() {
                            if let Some(scan) = &view.scan {
                                let visible = shown_rows(view, state);
                                out.push(Outcome::Copy(rows::copy_text(&scan.entries, &visible)));
                            }
                        }
                    }
                },
            );
        }
        y += height + GAP;
    }
    y - GAP
}

/// The keys the box shows: the modifiers held while it listens, else the
/// last combination pressed.
fn box_keys(state: &WindowUi) -> Vec<String> {
    let text = if state.capturing && state.live != 0 {
        hotkeys::format_modifiers(state.live)
    } else if let Some(keys) = state.keys {
        hotkeys::format(keys)
    } else {
        String::new()
    };
    keycap::split(&text)
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn check_box(
    ui: &mut Ui,
    state: &mut WindowUi,
    full: Rect,
    top: f32,
    out: &mut Vec<Outcome>,
) -> f32 {
    let tokens = Tokens::get(ui.ctx());
    let outer = Rect::from_min_max(
        pos2(full.left() + SIDE, top),
        pos2(full.right() - SIDE, top + CONTROL + CHECK_PAD.y * 2.0),
    );
    let line = theme::line(ui.ctx(), tokens.border);
    ui.painter()
        .rect_stroke(outer, tokens.radius_tile, line, StrokeKind::Inside);
    let inner = outer.shrink2(CHECK_PAD);
    let painter = ui.painter().clone();

    let label = painter.layout_no_wrap(
        "Check a shortcut".to_string(),
        theme::regular(13.0),
        tokens.text_secondary,
    );
    let label_width = label.size().x;
    painter.galley(
        pos2(inner.left(), inner.center().y - label.size().y / 2.0),
        label,
        tokens.text_secondary,
    );

    let steps = vec![box_keys(state)];
    let caps_width = if steps[0].is_empty() {
        0.0
    } else {
        keycap::caps_width(ui, &steps, Caps::palette())
    };
    let box_rect = Rect::from_min_size(
        pos2(inner.left() + label_width + 14.0, inner.top()),
        vec2((caps_width + 20.0).max(CAPTURE_MIN), CONTROL),
    );
    let response = ui
        .interact(box_rect, capture_id(), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.clicked() {
        response.request_focus();
        if !state.capturing {
            out.push(Outcome::StartCapture);
        }
    } else if state.capturing && !response.has_focus() {
        out.push(Outcome::StopCapture);
    }
    let border = if state.capturing {
        tokens.accent
    } else {
        tokens.border
    };
    painter.rect(
        box_rect,
        tokens.radius_row,
        tokens.hover_bg,
        theme::line(ui.ctx(), border),
        StrokeKind::Inside,
    );
    if !state.capturing {
        focus::ring(ui, &response, f32::from(tokens.radius_row));
    }
    let keys_rect = box_rect.shrink2(vec2(10.0, 0.0));
    if steps[0].is_empty() {
        if state.capturing {
            painter.text(
                pos2(keys_rect.left(), keys_rect.center().y),
                Align2::LEFT_CENTER,
                "Press keys\u{2026}",
                theme::regular(13.0),
                tokens.text_disabled,
            );
        }
    } else {
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(keys_rect)
                .layout(Layout::left_to_right(Align::Center)),
            |ui| keycap::caps(ui, &steps, Caps::palette()),
        );
    }

    let hint = painter.layout_no_wrap(
        CHECK_HINT.to_string(),
        theme::regular(12.0),
        tokens.text_disabled,
    );
    let hint_left = inner.right() - hint.size().x;
    let result_left = box_rect.right() + 14.0;
    // The hint gives way first when the window is narrow.
    let end = if hint_left - 14.0 > result_left + 14.0 + 120.0 {
        painter.galley(
            pos2(hint_left, inner.center().y - hint.size().y / 2.0),
            hint,
            tokens.text_disabled,
        );
        hint_left - 14.0
    } else {
        inner.right()
    };
    if let Some((line, tone)) = &state.verdict {
        let colour = tone.colour(&tokens);
        let room = end - result_left - 14.0;
        if room > 0.0 {
            painter.circle_filled(pos2(result_left + 3.0, inner.center().y), 3.0, colour);
            let galley = painter.layout_job(one_line(line, theme::regular(13.0), colour, room));
            painter.galley(
                pos2(result_left + 14.0, inner.center().y - galley.size().y / 2.0),
                galley,
                colour,
            );
        }
    }
    outer.bottom()
}

/// One line cut with "…" when it is wider than `width`.
fn one_line(line: &str, font: egui::FontId, colour: egui::Color32, width: f32) -> LayoutJob {
    let mut job = text::job(line, font, colour, None);
    job.wrap = TextWrapping {
        max_width: width.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    job
}

fn list_area(ui: &mut Ui, view: &View, state: &mut WindowUi, visible: &[usize], area: Rect) {
    let tokens = Tokens::get(ui.ctx());
    let line = theme::line(ui.ctx(), tokens.border);
    let painter = ui.painter().clone();
    painter.hline(area.x_range(), area.top(), line);
    let header = Rect::from_min_size(area.min, vec2(area.width(), HEADER));
    let columns = rows::columns(area.left(), area.right());
    for (at, (column, label)) in COLUMNS.iter().enumerate() {
        let cell = Rect::from_x_y_ranges(columns[at], header.y_range());
        let response = ui
            .interact(
                cell,
                Id::new(("wincraft-detector-header", at)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.clicked() {
            state.sort = state.sort.clicked(*column);
            state.selected = None;
        }
        let sorted = state.sort.column == *column;
        let shown = match (sorted, state.sort.descending) {
            (false, _) => label.to_string(),
            (true, false) => format!("{label} \u{2191}"),
            (true, true) => format!("{label} \u{2193}"),
        };
        let colour = if sorted || response.hovered() {
            tokens.text_secondary
        } else {
            tokens.text_disabled
        };
        painter.text(
            pos2(cell.left(), cell.center().y),
            Align2::LEFT_CENTER,
            shown,
            theme::regular(12.0),
            colour,
        );
        focus::ring(ui, &response, f32::from(tokens.radius_row));
    }
    painter.hline(area.x_range(), header.bottom(), line);

    let body = Rect::from_min_max(pos2(area.left(), header.bottom()), area.max);
    let Some(scan) = &view.scan else {
        empty_line(&painter, body, "Scanning\u{2026}", &tokens);
        return;
    };
    if visible.is_empty() {
        empty_line(&painter, body, "No shortcuts match.", &tokens);
        return;
    }
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt("wincraft-detector-rows")
        .auto_shrink([false, false]);
    if std::mem::take(&mut state.follow) {
        if let Some(selected) = state.selected {
            let top = selected as f32 * ROW;
            let bottom = top + ROW;
            if top < state.offset {
                scroll = scroll.vertical_scroll_offset(top);
            } else if bottom > state.offset + body.height() {
                scroll = scroll.vertical_scroll_offset(bottom - body.height());
            }
        }
    }
    let selected = state.selected;
    let mut clicked = None;
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(body)
            .layout(Layout::top_down(Align::Min)),
    );
    let output = scroll.show_rows(&mut child, ROW, visible.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for place in range {
            let entry = &scan.entries[visible[place]];
            if row(ui, entry, selected == Some(place), &columns) {
                clicked = Some(place);
            }
        }
    });
    state.offset = output.state.offset.y;
    if clicked.is_some() {
        state.selected = clicked;
    }
}

fn empty_line(painter: &egui::Painter, body: Rect, line: &str, tokens: &Tokens) {
    painter.text(
        pos2(body.left() + SIDE, body.top() + 32.0),
        Align2::LEFT_TOP,
        line,
        theme::regular(13.0),
        tokens.text_disabled,
    );
}

/// One row; true when it was clicked.
fn row(ui: &mut Ui, entry: &Entry, selected: bool, columns: &[egui::Rangef; 4]) -> bool {
    let tokens = Tokens::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    if !ui.is_rect_visible(rect) {
        return response.clicked();
    }
    let painter = ui.painter().clone();
    if selected {
        painter.rect_filled(rect, 0, tokens.selection_bg);
    } else if response.hovered() {
        painter.rect_filled(rect, 0, tokens.hover_bg);
    }
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        theme::line(ui.ctx(), tokens.border.gamma_multiply(0.5)),
    );

    let keys = hotkeys::format(entry.hotkey);
    let steps = vec![keycap::split(&keys)
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>()];
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(Rect::from_x_y_ranges(columns[0], rect.y_range()))
            .layout(Layout::left_to_right(Align::Center)),
        |ui| keycap::caps(ui, &steps, Caps::palette()),
    );

    let (word, tone) = rows::look(entry.status);
    let colour = tone.colour(&tokens);
    let galley = painter.layout_no_wrap(word.to_string(), theme::regular(12.0), colour);
    let pill = Rect::from_min_size(
        pos2(columns[1].min, rect.center().y - 10.0),
        vec2(galley.size().x + 16.0, 20.0),
    );
    painter.rect_stroke(pill, 10, theme::line(ui.ctx(), colour), StrokeKind::Inside);
    painter.galley(pill.center() - galley.size() / 2.0, galley, colour);

    let meaning = painter.layout_job(one_line(
        rows::meaning(entry),
        theme::regular(13.0),
        tokens.text_secondary,
        columns[2].span(),
    ));
    painter.galley(
        pos2(columns[2].min, rect.center().y - meaning.size().y / 2.0),
        meaning,
        tokens.text_secondary,
    );
    painter.text(
        pos2(columns[3].min, rect.center().y),
        Align2::LEFT_CENTER,
        entry.source,
        theme::mono(12.0),
        tokens.text_disabled,
    );
    response.clicked()
}

/// The bottom line's parts and the note at its right.
fn footer_parts(view: &View, state: &WindowUi) -> (Vec<String>, String) {
    let parts = match (&view.scan, view.scanning) {
        (Some(scan), false) => scan.summary().to_vec(),
        _ => vec!["Scanning\u{2026}".to_string()],
    };
    let note = state
        .notice
        .as_ref()
        .map_or_else(|| probe::HOOK_NOTE.to_string(), |(line, _)| line.clone());
    (parts, note)
}

fn footer_jobs(ui: &Ui, view: &View, state: &WindowUi, width: f32) -> (LayoutJob, LayoutJob) {
    let tokens = Tokens::get(ui.ctx());
    let (parts, note) = footer_parts(view, state);
    let font = theme::regular(12.0);
    let mut summary = LayoutJob::default();
    for (at, part) in parts.iter().enumerate() {
        if at > 0 {
            summary.append(
                " \u{b7} ",
                0.0,
                text::format(font.clone(), tokens.text_disabled, None),
            );
        }
        summary.append(
            part,
            0.0,
            text::format(font.clone(), tokens.text_secondary, None),
        );
    }
    summary.wrap.max_width = width.max(1.0);
    let mut note = text::job(&note, font, tokens.text_disabled, None);
    note.wrap.max_width = width.max(1.0);
    (summary, note)
}

/// Lays the bottom line out: one line when the numbers and the note fit
/// side by side, else the note under the numbers. Returns the galleys and
/// whether they share a line.
fn footer_galleys(
    ui: &Ui,
    view: &View,
    state: &WindowUi,
    width: f32,
) -> (Arc<egui::Galley>, Arc<egui::Galley>, bool) {
    let (summary, note) = footer_jobs(ui, view, state, width);
    let summary = ui.painter().layout_job(summary);
    let note = ui.painter().layout_job(note);
    let together = summary.rows.len() == 1 && summary.size().x + 16.0 + note.size().x <= width;
    (summary, note, together)
}

fn footer_height(ui: &Ui, view: &View, state: &WindowUi, width: f32) -> f32 {
    let (summary, note, together) = footer_galleys(ui, view, state, width);
    let text = if together {
        summary.size().y.max(note.size().y)
    } else {
        summary.size().y + 4.0 + note.size().y
    };
    text + 20.0
}

fn footer_line(ui: &Ui, view: &View, state: &WindowUi, area: Rect) {
    let tokens = Tokens::get(ui.ctx());
    let painter = ui.painter();
    painter.hline(
        area.x_range(),
        area.top(),
        theme::line(ui.ctx(), tokens.border),
    );
    let inner = area.shrink2(vec2(SIDE, 10.0));
    let (summary, note, together) = footer_galleys(ui, view, state, inner.width());
    let summary_height = summary.size().y;
    painter.galley(inner.min, summary, tokens.text_secondary);
    let at = if together {
        pos2(inner.right() - note.size().x, inner.top())
    } else {
        pos2(inner.left(), inner.top() + summary_height + 4.0)
    };
    painter.galley(at, note, tokens.text_disabled);
}

/// Puts the window in the middle of the monitor under the cursor, as the
/// Win32 window did. winit corrects its size when that monitor has another
/// scale.
fn centre_on_cursor_monitor(hwnd: HWND) {
    let mut point = POINT { x: 0, y: 0 };
    let mut window: RECT = unsafe { std::mem::zeroed() };
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    unsafe {
        if GetCursorPos(&mut point) == 0 || GetWindowRect(hwnd, &mut window) == 0 {
            return;
        }
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTOPRIMARY);
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return;
        }
    }
    let work = info.rcWork;
    let width = window.right - window.left;
    let height = window.bottom - window.top;
    let x = (work.left + (work.right - work.left - width) / 2).max(work.left);
    let y = (work.top + (work.bottom - work.top - height) / 2).max(work.top);
    unsafe {
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
}

#[cfg(test)]
mod tests;
