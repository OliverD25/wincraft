use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::text::TextWrapping;
use egui::{pos2, vec2, Color32, Pos2, Rect, Response, Sense, StrokeKind, Ui, UiBuilder, Vec2};
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowRect, SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE,
};

use crate::core::taskbar_scope::MultiMonitor;
use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{
    self, ArrangeAction, ArrangeDesktop, ArrangeGroup, ArrangeMonitor, ArrangeSnapshot,
    ArrangeView, ArrangeWindow, HostChannel, HostRequest, PeekCard,
};
use crate::ui::dwm_thumbs::{self, Placement, Thumbnail};
use crate::ui::peek::{self, Hover, Peek, Step};
use crate::ui::settings;
use crate::ui::widgets::empty_state::empty_state;
use crate::ui::widgets::{choice, focus, text};

mod by_monitor;
mod hold;
mod others;

/// Unique, so the strip's window can be found among the UI thread's windows.
const TITLE: &str = "WinCraft Arrange";

const PADDING: f32 = 12.0;
/// Small enough that a group of 15+ windows fits on one screen.
const THUMB_HEIGHT: f32 = 81.0;
const CARD_INSET: f32 = 4.0;
/// The line above the picture that holds the monitor chip. Every card has
/// it, so cards on the primary monitor line up with the others.
const LABEL_LINE: f32 = 18.0;
const NAME_HEIGHT: f32 = 16.0;
const CARD_HEIGHT: f32 =
    CARD_INSET + LABEL_LINE + CARD_INSET + THUMB_HEIGHT + CARD_INSET + NAME_HEIGHT + CARD_INSET;
const CARD_GAP: f32 = 8.0;
/// A picture is as wide as the window's shape makes it, but never narrower
/// or wider than these parts of its height, like the taskbar's own.
const NARROWEST: f32 = 0.6;
const WIDEST: f32 = 2.4;
/// The shape of a window whose size is not known.
const DEFAULT_SHAPE: Vec2 = Vec2::new(16.0, 9.0);
const HEADER: f32 = 15.0;
const HEADER_GAP: f32 = 6.0;
const ROW_GAP: f32 = 12.0;
const EMPTY_PANEL: Vec2 = Vec2::new(520.0, 150.0);
/// The note under the rows when more than one desktop is shown.
const FOOTER: f32 = 28.0;
/// The toolbar above the rows: the group list and the view switch.
const SWITCHER: f32 = 44.0;
/// The group list, the gap after it and the view switch.
const TOOLBAR_CONTROLS: f32 = 372.0;
const VIEWS: [&str; 2] = ["By monitor", "Flat"];
/// Rows that fit exactly can round to a pixel too tall and grow a scroll bar.
const SLACK: f32 = 4.0;
/// Space left between the strip and the taskbar.
const TASKBAR_GAP: f32 = 12.0;
/// How long the strip stays open without the focus after it moved a window
/// to another monitor, while it takes the focus back: a window that
/// maximizes itself there can come to the front for a moment.
const MOVE_GRACE: Duration = Duration::from_millis(1500);

pub fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("wincraft-arrange")
}

/// One desktop's windows, in thumbnail order.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub title: String,
    pub desktop: String,
    /// Indexes into the group's windows.
    pub members: Vec<usize>,
}

/// The windows of one program, one row per desktop: the current desktop
/// first, titled "This desktop" and shown even when empty, then the others
/// in Task View order. A window with no known desktop is on this one.
pub fn rows(group: &ArrangeGroup, desktops: &[ArrangeDesktop]) -> Vec<Row> {
    let current = desktops
        .iter()
        .find(|desktop| desktop.current)
        .map(|desktop| desktop.id.clone())
        .unwrap_or_default();
    let known = |id: &str| desktops.iter().any(|desktop| desktop.id == id);
    let mut rows = vec![Row {
        title: "This desktop".to_string(),
        desktop: current.clone(),
        members: group
            .windows
            .iter()
            .enumerate()
            .filter(|(_, window)| window.desktop == current || !known(&window.desktop))
            .map(|(index, _)| index)
            .collect(),
    }];
    for desktop in desktops.iter().filter(|desktop| !desktop.current) {
        let members: Vec<usize> = group
            .windows
            .iter()
            .enumerate()
            .filter(|(_, window)| window.desktop == desktop.id)
            .map(|(index, _)| index)
            .collect();
        if !members.is_empty() {
            rows.push(Row {
                title: desktop.name.clone(),
                desktop: desktop.id.clone(),
                members,
            });
        }
    }
    rows
}

/// Each window's place in Windows' taskbar row for its app on its own
/// desktop, counted from 1 and indexed like `group.windows`. The group keeps
/// WinCraft's order, which is the order it puts on the taskbar, and Win+number
/// goes through the same row. A desktop counts on its own, whether it is on
/// screen or not.
pub fn taskbar_numbers(group: &ArrangeGroup, desktops: &[ArrangeDesktop]) -> Vec<usize> {
    let mut numbers = vec![0; group.windows.len()];
    for row in rows(group, desktops) {
        for (place, member) in row.members.iter().enumerate() {
            numbers[*member] = place + 1;
        }
    }
    numbers
}

/// A window's place in its desktop's taskbar row (from 1) and how many
/// windows the row has.
pub fn place_in_row(
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    member: usize,
) -> Option<(usize, usize)> {
    rows(group, desktops).iter().find_map(|row| {
        row.members
            .iter()
            .position(|m| *m == member)
            .map(|at| (at + 1, row.members.len()))
    })
}

/// The whole group's order once `member` takes `place` (from 1, kept within
/// the row) in its desktop's taskbar row. The row's windows keep their
/// slots in the group's order, so every other desktop and every window's
/// monitor are untouched. None when nothing would change.
pub fn order_with_place(
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    member: usize,
    place: usize,
) -> Option<Vec<isize>> {
    let rows = rows(group, desktops);
    let row = rows.iter().find(|row| row.members.contains(&member))?;
    let mut sequence = row.members.clone();
    sequence.retain(|m| *m != member);
    sequence.insert((place.max(1) - 1).min(sequence.len()), member);
    let now: Vec<isize> = group.windows.iter().map(|w| w.hwnd).collect();
    let mut order = now.clone();
    for (slot, window) in row.members.iter().zip(sequence) {
        order[*slot] = group.windows[window].hwnd;
    }
    (order != now).then_some(order)
}

/// Gives `member` another place in its taskbar row and nothing else: the
/// same Reorder a drag sends, never a move to a monitor or desktop.
pub fn take_place(
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    member: usize,
    place: usize,
) -> Option<ArrangeAction> {
    order_with_place(group, desktops, member, place).map(|order| ArrangeAction::Reorder {
        group: group.key.clone(),
        order,
    })
}

/// Alt+Left (`step` -1) and Alt+Right (+1): one number lower or higher. None
/// at either end of the row.
pub fn nudge(
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    member: usize,
    step: isize,
) -> Option<ArrangeAction> {
    let (place, count) = place_in_row(group, desktops, member)?;
    let wanted = place.checked_add_signed(step)?;
    if (1..=count).contains(&wanted) {
        take_place(group, desktops, member, wanted)
    } else {
        None
    }
}

/// The entries of the "Taskbar place" menu: one per place in a row of
/// `count` windows, the window's own place (`current`) marked.
pub fn place_labels(count: usize, current: usize) -> Vec<String> {
    (1..=count)
        .map(|place| {
            if place == current {
                format!("{place} (now)")
            } else {
                place.to_string()
            }
        })
        .collect()
}

/// The card a Ctrl drop lands on: the one under the pointer, a little
/// beyond its edge so the gap between cards belongs to a card too. The
/// dragged window's own card never counts.
pub fn ctrl_target(pointer: Pos2, dragged: isize, cards: &[(isize, Rect)]) -> Option<isize> {
    cards
        .iter()
        .filter(|(hwnd, _)| *hwnd != dragged)
        .find(|(_, rect)| rect.expand(CARD_GAP / 2.0).contains(pointer))
        .map(|(hwnd, _)| *hwnd)
}

/// The place a Ctrl drop gives `member`: the target card's number. None when
/// the target is on another desktop, whose numbers are not this row's.
pub fn ctrl_place(
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    member: usize,
    target: isize,
) -> Option<usize> {
    let target = group.windows.iter().position(|w| w.hwnd == target)?;
    let (place, _) = place_in_row(group, desktops, target)?;
    let same_row = rows(group, desktops)
        .iter()
        .any(|row| row.members.contains(&member) && row.members.contains(&target));
    (same_row && target != member).then_some(place)
}

/// What a Ctrl drop onto `target` does: `member` takes the target's number.
pub fn ctrl_drop(
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    member: usize,
    target: isize,
) -> Option<ArrangeAction> {
    let place = ctrl_place(group, desktops, member, target)?;
    take_place(group, desktops, member, place)
}

/// The hint shown beside a card dragged with Ctrl held.
pub fn ctrl_hint(place: Option<usize>) -> String {
    match place {
        Some(place) => format!("Change number only: takes number {place}"),
        None => "Change number only: drop on a card of this desktop".to_string(),
    }
}

/// The monitor a hover strip is limited to: the one whose taskbar it sits
/// on, when Windows lists only that monitor's windows on that taskbar. None
/// for a strip opened by the hotkey (no `anchor`) and for a taskbar that
/// lists every window.
pub fn only_monitor(
    anchor: Option<[i32; 4]>,
    monitors: &[ArrangeMonitor],
    setting: MultiMonitor,
) -> Option<usize> {
    let button = anchor?;
    let (x, y) = ((button[0] + button[2]) / 2, (button[1] + button[3]) / 2);
    let at = monitors.iter().position(|m| {
        m.connected && x >= m.rect[0] && x < m.rect[2] && y >= m.rect[1] && y < m.rect[3]
    })?;
    setting.shows_only_own(monitors[at].primary).then_some(at)
}

/// Which of the group's windows the strip shows: all of them, or only those
/// on the `only` monitor (a window of unknown place counts as on the main
/// one).
pub fn visible_flags(
    group: &ArrangeGroup,
    monitors: &[ArrangeMonitor],
    only: Option<usize>,
) -> Vec<bool> {
    let Some(only) = only else {
        return vec![true; group.windows.len()];
    };
    let main = monitors.iter().position(|m| m.primary && m.connected);
    group
        .windows
        .iter()
        .map(|window| by_monitor::column_of(window, monitors).or(main) == Some(only))
        .collect()
}

/// `rows` with only the windows that are shown. This desktop's row stays
/// even when empty; another desktop's row goes when nothing in it is shown.
fn only_visible(rows: Vec<Row>, visible: &[bool]) -> Vec<Row> {
    rows.into_iter()
        .enumerate()
        .filter_map(|(at, mut row)| {
            row.members.retain(|member| visible[*member]);
            (at == 0 || !row.members.is_empty()).then_some(row)
        })
        .collect()
}

/// The group's order once the shown windows are in `shown`'s order: they
/// keep the places the shown windows had, so a window that is not shown stays
/// where it was.
pub fn merge_order(full: &[isize], shown: &[isize]) -> Vec<isize> {
    let wanted: Vec<isize> = shown
        .iter()
        .copied()
        .filter(|hwnd| full.contains(hwnd))
        .collect();
    let mut merged = full.to_vec();
    let slots: Vec<usize> = full
        .iter()
        .enumerate()
        .filter(|(_, hwnd)| wanted.contains(hwnd))
        .map(|(at, _)| at)
        .collect();
    for (slot, hwnd) in slots.into_iter().zip(wanted) {
        merged[slot] = hwnd;
    }
    merged
}

/// The rows on screen: only this desktop's until the other desktops are
/// shown, and only the windows that are shown.
fn shown_rows(
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    shown: bool,
    visible: &[bool],
) -> Vec<Row> {
    let mut rows = only_visible(rows(group, desktops), visible);
    if !shown {
        rows.truncate(1);
    }
    rows
}

/// How wide a picture `height` tall is for a window whose picture shows
/// `source` (its size in pixels): the window's own shape, kept between
/// NARROWEST and WIDEST of the height, 16:9 when the size is not known.
/// Whole points, so a card does not change width while nothing changed.
pub fn picture_width(source: Option<Vec2>, height: f32) -> f32 {
    let shape = source
        .filter(|size| size.x > 0.0 && size.y > 0.0)
        .unwrap_or(DEFAULT_SHAPE);
    (height * (shape.x / shape.y).clamp(NARROWEST, WIDEST)).round()
}

/// The part of a window its picture shows, in the window's own pixels: its
/// visible frame, or the whole window when that frame does not fit it, as
/// for a minimized window, whose frame is the small one it has while
/// minimized.
pub fn picture_source(frame: Option<Rect>, size: Vec2) -> Rect {
    let whole = Rect::from_min_size(pos2(0.0, 0.0), size);
    match frame {
        Some(frame)
            if whole.contains_rect(frame)
                && frame.width() >= size.x / 2.0
                && frame.height() >= size.y / 2.0 =>
        {
            Rect::from_min_max(frame.min.round(), frame.max.round())
        }
        _ => whole,
    }
}

/// Cards of these widths in lines no wider than `available`, in order:
/// each line holds as many as fit, and at least one.
pub fn wrap(widths: &[f32], available: f32) -> Vec<std::ops::Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut used = 0.0;
    for (index, width) in widths.iter().enumerate() {
        let wanted = if index == start {
            *width
        } else {
            used + CARD_GAP + width
        };
        // Half a point of room: a panel sized to fit exactly can round down.
        if index > start && wanted > available + 0.5 {
            lines.push(start..index);
            start = index;
            used = *width;
        } else {
            used = wanted;
        }
    }
    if start < widths.len() {
        lines.push(start..widths.len());
    }
    lines
}

/// How wide cards of these widths are on one line.
pub fn line_width(widths: &[f32]) -> f32 {
    widths.iter().sum::<f32>() + CARD_GAP * widths.len().saturating_sub(1) as f32
}

/// A Flat card's width for a window whose picture shows `source`.
fn card_width(source: Option<Vec2>) -> f32 {
    picture_width(source, THUMB_HEIGHT) + 2.0 * CARD_INSET
}

/// The cards' widths in a row, or the empty row's placeholder.
fn row_widths(row: &Row, widths: &[f32]) -> Vec<f32> {
    if row.members.is_empty() {
        vec![card_width(None)]
    } else {
        row.members.iter().map(|member| widths[*member]).collect()
    }
}

/// The panel size for the rows, whose cards are `widths` wide (one per
/// window of the group): as wide as the widest row needs, at least
/// `min_width` and at most `limit.x`, with longer rows wrapping onto more
/// lines; as tall as that makes it, at most `limit.y`, beyond which the
/// rows scroll. `link` makes room for the line that opens the other
/// desktops.
pub fn panel_size(rows: &[Row], widths: &[f32], link: bool, min_width: f32, limit: Vec2) -> Vec2 {
    let widest = rows
        .iter()
        .map(|row| line_width(&row_widths(row, widths)))
        .fold(card_width(None), f32::max);
    let width = (2.0 * PADDING + widest + SLACK).min(limit.x);
    let wrapped: Vec<(Vec<f32>, Vec<std::ops::Range<usize>>)> = rows
        .iter()
        .map(|row| {
            let cards = row_widths(row, widths);
            let lines = wrap(&cards, width - 2.0 * PADDING);
            (cards, lines)
        })
        .collect();
    // Rows that wrap leave the panel only as wide as their longest line; at
    // that width they wrap the same way.
    let longest = wrapped
        .iter()
        .flat_map(|(cards, lines)| lines.iter().map(|line| line_width(&cards[line.clone()])))
        .fold(0.0, f32::max);
    let width = (2.0 * PADDING + longest + SLACK)
        .min(width)
        .max(min_width)
        .min(limit.x);
    let lines: f32 = wrapped
        .iter()
        .map(|(_, lines)| {
            let lines = lines.len() as f32;
            HEADER + HEADER_GAP + lines * CARD_HEIGHT + (lines - 1.0) * CARD_GAP
        })
        .sum();
    let count = rows.len().max(1) as f32;
    let footer = if rows.len() > 1 { FOOTER } else { 0.0 };
    let link = if link { others::LINK_HEIGHT } else { 0.0 };
    let height = 2.0 * PADDING + lines + (count - 1.0) * ROW_GAP + link + footer + SLACK;
    vec2(width, height.min(limit.y))
}

/// The pointer's monitor: its work area (the screen minus the taskbar) in
/// physical pixels and its scale.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Monitor {
    work: [i32; 4],
    scale: f32,
}

impl Monitor {
    fn under_pointer() -> Option<Self> {
        let mut point = POINT { x: 0, y: 0 };
        unsafe { GetCursorPos(&mut point) };
        Self::at(point)
    }

    fn at(point: POINT) -> Option<Self> {
        let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            return None;
        }
        let (mut dpi_x, mut dpi_y) = (96, 96);
        unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
        let work = info.rcWork;
        Some(Self {
            work: [work.left, work.top, work.right, work.bottom],
            scale: dpi_x as f32 / 96.0,
        })
    }

    /// What the strip may use, in logical points: 90 % of the work area's
    /// width and 60 % of its height; taller than that, it scrolls.
    fn limit(&self) -> Vec2 {
        let width = (self.work[2] - self.work[0]) as f32 / self.scale;
        let height = (self.work[3] - self.work[1]) as f32 / self.scale;
        vec2(width * 0.9, height * 0.6)
    }

    /// The window rectangle for a panel: centred, its bottom edge just above
    /// the taskbar.
    fn window_rect(&self, panel: Vec2) -> [i32; 4] {
        let width = (panel.x * self.scale).round() as i32;
        let height = (panel.y * self.scale).round() as i32;
        let left = self.work[0] + (self.work[2] - self.work[0] - width) / 2;
        let bottom = self.work[3] - (TASKBAR_GAP * self.scale).round() as i32;
        let top = (bottom - height).max(self.work[1]);
        [left, top, width, height]
    }

    /// The window rectangle for a panel opened by a hover: centred on the
    /// taskbar button, kept inside the screen's sides, and just above the
    /// taskbar (or below it, for a taskbar at the top).
    fn above_button(&self, panel: Vec2, button: [i32; 4]) -> [i32; 4] {
        let width = (panel.x * self.scale).round() as i32;
        let height = (panel.y * self.scale).round() as i32;
        let gap = (TASKBAR_GAP * self.scale).round() as i32;
        let centre = (button[0] + button[2]) / 2;
        let right_most = (self.work[2] - width).max(self.work[0]);
        let left = (centre - width / 2).clamp(self.work[0], right_most);
        let top = if (button[1] + button[3]) / 2 < self.work[1] {
            (self.work[1] + gap).min((self.work[3] - height).max(self.work[1]))
        } else {
            (self.work[3] - gap - height).max(self.work[1])
        };
        [left, top, width, height]
    }

    fn rect_for(&self, panel: Vec2, anchor: Option<[i32; 4]>) -> [i32; 4] {
        match anchor {
            Some(button) => self.above_button(panel, button),
            None => self.window_rect(panel),
        }
    }
}

struct Drag {
    hwnd: isize,
    /// Where the card was grabbed, from its top-left corner.
    grab: Vec2,
}

struct Shared {
    snapshot: Option<ArrangeSnapshot>,
    /// Changes made here that the plugin's data has not caught up with yet.
    holds: Vec<hold::Hold>,
    /// The taskbar group on screen.
    key: Option<String>,
    /// The card the arrow keys act on.
    cursor: Option<isize>,
    drag: Option<Drag>,
    /// Last frame's layout, for finding where a dragged card would land.
    row_rects: Vec<Rect>,
    card_rects: Vec<Vec<Rect>>,
    /// The same for the By monitor view.
    zones: Vec<by_monitor::Zone>,
    /// Every card drawn last frame, for finding the card a Ctrl drop lands
    /// on. By window handle, so it survives the group changing.
    card_hits: Vec<(isize, Rect)>,
    /// The only monitor whose windows the strip shows, for a hover strip on
    /// a taskbar that lists just those.
    only_monitor: Option<usize>,
    /// Which windows each taskbar lists, read when the strip opens.
    taskbars: MultiMonitor,
    /// The monitor whose empty rail the keyboard has just moved a window to;
    /// the rail stays unfolded until the next key.
    rail_focus: Option<usize>,
    to_host: Arc<HostChannel>,
    closed: bool,
    window: isize,
    monitor: Option<Monitor>,
    focused_once: bool,
    thumbs: HashMap<isize, Option<Thumbnail>>,
    /// The pictures of the By monitor view's ghosts.
    ghost_thumbs: HashMap<isize, Option<Thumbnail>>,
    /// The part of each window its picture shows; its shape is the card's.
    /// Missing for a window Windows gives no picture of.
    sources: HashMap<isize, Rect>,
    peek: Peek,
    hover: Hover,
    /// A test scene's card to peek at, without a mouse.
    forced_peek: Option<PeekCard>,
    /// Losing the focus does not close the strip before this.
    keep_open_until: Option<Instant>,
    /// Whether the other desktops are shown; closed whenever the strip opens.
    others: others::Others,
    /// The taskbar button a hover opened the strip above. Such a strip
    /// leaves the keyboard with the window in front.
    anchor: Option<[i32; 4]>,
}

impl Shared {
    fn hide_peek(&mut self) {
        self.peek.hide();
        self.hover = Hover::default();
    }

    fn group(&self) -> Option<&ArrangeGroup> {
        let snapshot = self.snapshot.as_ref()?;
        let key = self.key.as_ref()?;
        snapshot.groups.iter().find(|group| &group.key == key)
    }

    /// The view on screen: the one picked last, or for Auto the one that
    /// fits the number of monitors.
    fn view(&self) -> ArrangeView {
        self.snapshot
            .as_ref()
            .map_or(ArrangeView::Flat, |snapshot| {
                let connected = snapshot.monitors.iter().filter(|m| m.connected).count();
                snapshot.view.resolve(connected)
            })
    }

    /// Each of the group's windows' card width, from the shape of its
    /// picture.
    fn widths(&self, group: &ArrangeGroup, width: fn(Option<Vec2>) -> f32) -> Vec<f32> {
        group
            .windows
            .iter()
            .map(|window| width(self.sources.get(&window.hwnd).map(|source| source.size())))
            .collect()
    }

    /// How many other desktops have windows of the group on screen.
    fn others_count(&self) -> usize {
        match (self.group(), &self.snapshot) {
            (Some(group), Some(snapshot)) => {
                others::count(group, &snapshot.desktops, &self.visible(group))
            }
            _ => 0,
        }
    }

    /// Which of the group's windows are on screen.
    fn visible(&self, group: &ArrangeGroup) -> Vec<bool> {
        let monitors = self.snapshot.as_ref().map_or(&[][..], |s| &s.monitors);
        visible_flags(group, monitors, self.only_monitor)
    }

    fn panel(&self) -> Vec2 {
        let limit = self
            .monitor
            .map(|monitor| monitor.limit())
            .unwrap_or(vec2(1600.0, 900.0));
        match (self.group(), &self.snapshot) {
            (Some(group), Some(snapshot)) if self.view() == ArrangeView::ByMonitor => {
                let count = self.others_count();
                let widths = self.widths(group, by_monitor::card_width);
                by_monitor::panel_for(
                    group,
                    snapshot,
                    &widths,
                    self.others.shown(count),
                    count > 0,
                    limit,
                    by_monitor::Scope {
                        only: self.only_monitor,
                        taskbars: self.taskbars,
                    },
                )
            }
            (Some(group), Some(snapshot)) => {
                let count = self.others_count();
                let rows = shown_rows(
                    group,
                    &snapshot.desktops,
                    self.others.shown(count),
                    &self.visible(group),
                );
                let widths = self.widths(group, card_width);
                let min_width = 2.0 * PADDING + TOOLBAR_CONTROLS + SLACK;
                let mut size = panel_size(&rows, &widths, count > 0, min_width, limit);
                size.y = (size.y + SWITCHER).min(limit.y);
                size
            }
            _ => EMPTY_PANEL.min(limit),
        }
    }
}

/// The mouse way to see and order a program's windows: live pictures of all
/// of them, one row per desktop, in taskbar order.
pub struct Arrange {
    shared: Arc<Mutex<Shared>>,
    palette_hwnd: isize,
}

impl Arrange {
    pub fn new(to_host: Arc<HostChannel>, palette_hwnd: isize) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                snapshot: None,
                holds: Vec::new(),
                key: None,
                cursor: None,
                drag: None,
                row_rects: Vec::new(),
                card_rects: Vec::new(),
                zones: Vec::new(),
                card_hits: Vec::new(),
                only_monitor: None,
                taskbars: MultiMonitor::default(),
                rail_focus: None,
                to_host,
                closed: false,
                window: 0,
                monitor: None,
                focused_once: false,
                thumbs: HashMap::new(),
                ghost_thumbs: HashMap::new(),
                sources: HashMap::new(),
                peek: Peek::default(),
                hover: Hover::default(),
                forced_peek: None,
                keep_open_until: None,
                others: others::Others::default(),
                anchor: None,
            })),
            palette_hwnd,
        }
    }

    pub fn open(&self, snapshot: ArrangeSnapshot) {
        let Ok(mut shared) = self.shared.lock() else {
            return;
        };
        shared.key = snapshot
            .groups
            .get(snapshot.focus)
            .map(|group| group.key.clone());
        shared.snapshot = Some(snapshot);
        shared.holds.clear();
        shared.closed = false;
        shared.window = 0;
        shared.focused_once = false;
        shared.cursor = None;
        shared.drag = None;
        shared.others.reset();
        shared.row_rects.clear();
        shared.card_rects.clear();
        shared.zones.clear();
        shared.card_hits.clear();
        shared.anchor = shared.snapshot.as_ref().and_then(|s| s.anchor);
        // Read again at every hover: the user may have changed the setting.
        shared.taskbars = MultiMonitor::read();
        let taskbars = shared.taskbars;
        shared.only_monitor = shared
            .snapshot
            .as_ref()
            .and_then(|s| only_monitor(s.anchor, &s.monitors, taskbars));
        shared.monitor = match shared.anchor {
            Some(button) => Monitor::at(POINT {
                x: (button[0] + button[2]) / 2,
                y: (button[1] + button[3]) / 2,
            }),
            None => Monitor::under_pointer(),
        };
    }

    /// Takes fresh data and keeps the same taskbar group on screen.
    pub fn update(&self, mut snapshot: ArrangeSnapshot) {
        let Ok(mut shared) = self.shared.lock() else {
            return;
        };
        let still_there = shared
            .key
            .as_ref()
            .is_some_and(|key| snapshot.groups.iter().any(|group| &group.key == key));
        if !still_there {
            shared.key = snapshot
                .groups
                .get(snapshot.focus)
                .map(|group| group.key.clone());
        }
        hold::filter(&mut shared.holds, &mut snapshot.groups, Instant::now());
        shared.snapshot = Some(snapshot);
    }

    /// Test scenes only: peek at one card as if the mouse rested on it.
    pub fn peek_card(&self, card: PeekCard) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.forced_peek = Some(card);
        }
    }

    pub fn was_closed(&self) -> bool {
        self.shared
            .lock()
            .map(|shared| shared.closed)
            .unwrap_or(false)
    }

    /// Unregisters every picture and hides the peek; runs on the UI thread,
    /// which owns them.
    pub fn hidden(&self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.closed = false;
            shared.window = 0;
            ui_bridge::set_arrange_window(0);
            shared.anchor = None;
            shared.only_monitor = None;
            shared.drag = None;
            shared.others.reset();
            shared.thumbs.clear();
            shared.ghost_thumbs.clear();
            shared.sources.clear();
            shared.hide_peek();
            shared.forced_peek = None;
        }
    }

    pub fn show(&self, ctx: &egui::Context, icon: Option<Arc<egui::IconData>>) {
        let shared = Arc::clone(&self.shared);
        let palette_hwnd = self.palette_hwnd;
        let (panel, hovered) = self
            .shared
            .lock()
            .map(|shared| (shared.panel(), shared.anchor.is_some()))
            .unwrap_or((EMPTY_PANEL, false));
        let mut builder = egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_decorations(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_taskbar(false)
            .with_resizable(false)
            .with_active(!hovered)
            .with_inner_size(panel);
        if let Some(icon) = icon {
            builder = builder.with_icon(icon);
        }

        ctx.show_viewport_deferred(viewport_id(), builder, move |ui, _class| {
            focus::track(ui.ctx());
            let Ok(mut shared) = shared.lock() else {
                return;
            };
            if shared.window == 0 {
                shared.window = settings::find_window(TITLE, palette_hwnd);
                if shared.window != 0 {
                    let border = theme::colorref(Tokens::get(ui.ctx()).border);
                    dwm_thumbs::round_corners(shared.window as HWND, border);
                    ui_bridge::set_arrange_window(shared.window);
                    if shared.anchor.is_none() {
                        shared.to_host.send(HostRequest::FocusWindow(shared.window));
                    }
                }
            }
            let panel = shared.panel();
            if let (Some(monitor), true) = (shared.monitor, shared.window != 0) {
                keep_placed(
                    shared.window as HWND,
                    monitor.rect_for(panel, shared.anchor),
                );
            }

            let (escape, focused) = ui
                .ctx()
                .input(|i| (i.key_pressed(egui::Key::Escape), i.viewport().focused));
            if focused == Some(true) {
                shared.focused_once = true;
            }
            let grace = shared
                .keep_open_until
                .and_then(|until| until.checked_duration_since(Instant::now()));
            if let Some(left) = grace {
                ui.ctx().request_repaint_after(left);
            }
            if escape || (shared.focused_once && focused == Some(false) && grace.is_none()) {
                shared.closed = true;
            }
            if ui.ctx().input(|i| i.viewport().close_requested()) {
                shared.closed = true;
            }

            strip(ui, &mut shared);

            // The root viewport is the hidden palette and only repaints when
            // asked; it is what stops showing this viewport once closed.
            if shared.closed {
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            }
        });
    }
}

/// Moves the window where it belongs whenever it is not there: winit resizes
/// it itself when it lands on a monitor with other scaling.
fn keep_placed(hwnd: HWND, wanted: [i32; 4]) {
    let mut now = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe { GetWindowRect(hwnd, &mut now) };
    let current = [
        now.left,
        now.top,
        now.right - now.left,
        now.bottom - now.top,
    ];
    if current != wanted {
        unsafe {
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                wanted[0],
                wanted[1],
                wanted[2],
                wanted[3],
                SWP_NOACTIVATE,
            )
        };
    }
}

/// A place in a row while laying out: a window, or the gap where the window
/// being dragged would land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Window(usize),
    Gap,
}

/// The rows as they look during a drag: the dragged window taken out, and a
/// gap at `target`, given as (row, index among the remaining windows).
pub fn preview(
    rows: &[Row],
    dragged: Option<usize>,
    target: Option<(usize, usize)>,
) -> Vec<Vec<Slot>> {
    rows.iter()
        .enumerate()
        .map(|(row_index, row)| {
            let mut slots: Vec<Slot> = row
                .members
                .iter()
                .copied()
                .filter(|member| Some(*member) != dragged)
                .map(Slot::Window)
                .collect();
            if let Some((target_row, index)) = target {
                if target_row == row_index {
                    slots.insert(index.min(slots.len()), Slot::Gap);
                }
            }
            slots
        })
        .collect()
}

/// Every window, row by row, after `dragged` is dropped at `target`.
pub fn dropped_order(rows: &[Row], dragged: usize, target: (usize, usize)) -> Vec<usize> {
    preview(rows, Some(dragged), Some(target))
        .into_iter()
        .flatten()
        .map(|slot| match slot {
            Slot::Window(member) => member,
            Slot::Gap => dragged,
        })
        .collect()
}

/// Where the pointer would drop a window, from last frame's layout: the row
/// under it (or the nearest) and how many of that row's cards come before
/// it, on the lines above or to its left on its own line.
fn drop_target(
    pointer: egui::Pos2,
    row_rects: &[Rect],
    card_rects: &[Vec<Rect>],
) -> Option<(usize, usize)> {
    let row = row_rects
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let distance = |r: &Rect| {
                if r.y_range().contains(pointer.y) {
                    0.0
                } else {
                    (r.center().y - pointer.y).abs()
                }
            };
            distance(a).total_cmp(&distance(b))
        })
        .map(|(index, _)| index)?;
    let index = card_rects
        .get(row)
        .map(|cards| {
            cards
                .iter()
                .filter(|card| {
                    card.max.y <= pointer.y
                        || (card.y_range().contains(pointer.y) && card.center().x < pointer.x)
                })
                .count()
        })
        .unwrap_or(0);
    Some((row, index))
}

/// A picture to put on screen once the whole frame is laid out, so that one
/// under the dragged card or under a menu can be hidden instead.
struct Pending {
    hwnd: isize,
    dest: Rect,
    clip: Rect,
    opacity: u8,
    /// For a ghost in the By monitor view, whose window also has its own
    /// card: a picture has one place, so a ghost needs a picture of its own.
    ghost: bool,
}

/// What a view's body did this frame, for the parts both views share.
struct Frame {
    pending: Vec<Pending>,
    /// Every card in its order on screen, with whether it has a monitor chip.
    cards: Vec<(isize, bool)>,
    hovered: Option<isize>,
    actions: Vec<ArrangeAction>,
    activate: Option<isize>,
    /// Where the cards are; the peek ends when the pointer leaves it.
    area: Rect,
    /// The dragged card when it follows the pointer above the others.
    floating: Option<Rect>,
    dragged: Option<isize>,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            cards: Vec::new(),
            hovered: None,
            actions: Vec::new(),
            activate: None,
            area: Rect::NOTHING,
            floating: None,
            dragged: None,
        }
    }
}

fn strip(ui: &mut Ui, shared: &mut Shared) {
    let tokens = Tokens::get(ui.ctx());
    let panel = ui.max_rect();
    let view = shared.view();
    let background = match view {
        ArrangeView::ByMonitor => tokens.window_bg,
        _ => tokens.elevated_bg,
    };
    ui.painter().rect_filled(panel, 0, background);

    let content = panel.shrink(PADDING);
    let (Some(group), Some(snapshot)) = (shared.group().cloned(), shared.snapshot.clone()) else {
        let watched = shared
            .snapshot
            .as_ref()
            .map(|s| s.watched.join(", "))
            .unwrap_or_default();
        ui.scope_builder(UiBuilder::new().max_rect(content), |ui| {
            empty_state(
                ui,
                "No windows to arrange",
                &format!("No windows of {watched} are open."),
            );
        });
        shared.thumbs.clear();
        shared.ghost_thumbs.clear();
        shared.hide_peek();
        return;
    };

    let bar = match view {
        ArrangeView::ByMonitor => by_monitor::toolbar_rect(panel),
        _ => Rect::from_min_size(content.min, vec2(content.width(), SWITCHER)),
    };
    let visible = shared.visible(&group);
    let (chosen_group, chosen_view) = toolbar(ui, bar, &snapshot, &group, view, &visible);
    if let Some(index) = chosen_group {
        shared.key = Some(snapshot.groups[index].key.clone());
        shared.cursor = None;
        shared.drag = None;
        shared.hide_peek();
        ui.ctx().request_repaint();
    }
    if let Some(chosen) = chosen_view {
        if let Some(snapshot) = shared.snapshot.as_mut() {
            snapshot.view = chosen;
        }
        shared.to_host.send(HostRequest::SetSetting {
            plugin: snapshot.plugin.clone(),
            key: "arrange_view".to_string(),
            value: serde_json::Value::String(chosen.setting().to_string()),
        });
        shared.drag = None;
        shared.hide_peek();
        ui.ctx().request_repaint();
    }

    shared
        .thumbs
        .retain(|hwnd, _| group.windows.iter().any(|window| window.hwnd == *hwnd));
    shared
        .ghost_thumbs
        .retain(|hwnd, _| group.windows.iter().any(|window| window.hwnd == *hwnd));
    let ctx = ui.ctx().clone();
    if refresh_sources(shared, &group) {
        // The cards change shape, and the window with them.
        ctx.request_repaint();
    }
    let ppp = ctx.pixels_per_point();
    let menu_open = egui::Popup::is_any_open(&ctx);
    let others_count = others::count(&group, &snapshot.desktops, &visible);
    if others_count > 0
        && !menu_open
        && shared.drag.is_none()
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::D))
    {
        shared.others.toggle();
        ctx.request_repaint();
    }
    let mut frame = match view {
        ArrangeView::ByMonitor => by_monitor::body(ui, shared, &group, &snapshot, panel, menu_open),
        _ => {
            let mut rows_area = content;
            rows_area.min.y += SWITCHER;
            flat(ui, shared, &group, &snapshot.desktops, rows_area, menu_open)
        }
    };

    let in_rows = ctx
        .input(|i| i.pointer.hover_pos())
        .is_some_and(|at| frame.area.contains(at));
    // Asked again rather than reusing the flags from the top of the frame,
    // so a menu or drag that started on this frame hides the peek at once.
    let blocked = !snapshot.preview
        || egui::Popup::is_any_open(&ctx)
        || shared.drag.is_some()
        || shared.window == 0;
    let forced = shared
        .forced_peek
        .and_then(|card| forced_card(&frame.cards, card));
    match forced {
        Some(hwnd) if !blocked && frame.hovered.is_none() => {
            shared.peek.show(hwnd, shared.window as HWND)
        }
        _ => match shared
            .hover
            .step(Instant::now(), frame.hovered, in_rows, blocked)
        {
            Step::Show(hwnd) => shared.peek.show(hwnd, shared.window as HWND),
            Step::Hide => shared.peek.hide(),
            Step::Wait(after) => ctx.request_repaint_after(after),
        },
    }
    if shared.peek.is_shown() {
        // Looks again now and then, so a window that closes takes its
        // peek with it even when nothing else repaints the strip.
        ctx.request_repaint_after(peek::REST);
    }

    place_pictures(shared, &frame.pending, frame.floating, frame.dragged, ppp);

    let actions = std::mem::take(&mut frame.actions);
    for action in &actions {
        apply_locally(shared, action);
    }
    hold_changes(shared, &actions);
    let moves_monitor = actions
        .iter()
        .any(|action| matches!(action, ArrangeAction::MoveToMonitor { .. }));
    for action in actions {
        shared.to_host.send(HostRequest::Arrange {
            plugin: snapshot.plugin.clone(),
            action,
        });
    }
    if moves_monitor && shared.window != 0 {
        shared.keep_open_until = Some(Instant::now() + MOVE_GRACE);
        shared.to_host.send(HostRequest::FocusWindow(shared.window));
    }
    if let Some(hwnd) = frame.activate {
        shared.hide_peek();
        shared.to_host.send(HostRequest::Arrange {
            plugin: snapshot.plugin.clone(),
            action: ArrangeAction::Activate(hwnd),
        });
        shared.closed = true;
    }
}

/// The group list, the view switch and, where there is room, how many
/// monitors, desktops and windows there are. Returns the group and the view
/// picked on this frame.
fn toolbar(
    ui: &mut Ui,
    area: Rect,
    snapshot: &ArrangeSnapshot,
    group: &ArrangeGroup,
    view: ArrangeView,
    visible: &[bool],
) -> (Option<usize>, Option<ArrangeView>) {
    let tokens = Tokens::get(ui.ctx());
    let labels: Vec<&str> = snapshot.groups.iter().map(|g| g.label.as_str()).collect();
    let current = snapshot.groups.iter().position(|g| g.key == group.key);
    let shown = match view {
        ArrangeView::ByMonitor => 0,
        _ => 1,
    };
    let (picked, switched, right) = ui
        .scope_builder(UiBuilder::new().max_rect(area), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(12.0, 0.0);
                let picked = choice::dropdown(ui, "arrange-group", &labels, current)
                    .filter(|index| Some(*index) != current);
                let switched = choice::segmented(ui, &VIEWS, Some(shown)).map(|index| {
                    if index == 0 {
                        ArrangeView::ByMonitor
                    } else {
                        ArrangeView::Flat
                    }
                });
                (picked, switched, ui.min_rect().right())
            })
            .inner
        })
        .inner;
    let connected = snapshot.monitors.iter().filter(|m| m.connected).count();
    let shown = visible.iter().filter(|v| **v).count();
    // A strip limited to one monitor counts that monitor only.
    let monitors = if shown < group.windows.len() {
        1
    } else {
        connected
    };
    let line = summary(monitors, snapshot.desktops.len(), shown);
    let galley = ui
        .painter()
        .layout_no_wrap(line, theme::regular(13.0), tokens.text_secondary);
    if right + 24.0 + galley.size().x <= area.right() {
        ui.painter().galley(
            pos2(
                area.right() - galley.size().x,
                area.top() + (32.0 - galley.size().y) / 2.0,
            ),
            galley,
            tokens.text_secondary,
        );
    }
    (picked, switched)
}

/// "3 monitors · 6 desktops · 27 windows". Without the desktop list there
/// is still the one desktop.
fn summary(monitors: usize, desktops: usize, windows: usize) -> String {
    let count = |n: usize, one: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {one}s")
        }
    };
    [
        count(monitors.max(1), "monitor"),
        count(desktops.max(1), "desktop"),
        count(windows, "window"),
    ]
    .join(" \u{b7} ")
}

/// The view as before 0.13: one wrapping row of cards per desktop.
fn flat(
    ui: &mut Ui,
    shared: &mut Shared,
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    content: Rect,
    menu_open: bool,
) -> Frame {
    let tokens = Tokens::get(ui.ctx());
    let visible = shared.visible(group);
    let others_count = others::count(group, desktops, &visible);
    let rows = shown_rows(group, desktops, shared.others.shown(others_count), &visible);
    let ctx = ui.ctx().clone();
    let can_rename = shared.snapshot.as_ref().is_some_and(|s| s.can_rename);
    let pointer = ctx.pointer_latest_pos();
    let mut frame = Frame::default();

    let numbers = taskbar_numbers(group, desktops);
    let flat: Vec<usize> = rows.iter().flat_map(|row| row.members.clone()).collect();
    let widths = shared.widths(group, card_width);
    if shared
        .cursor
        .is_none_or(|hwnd| !flat.iter().any(|m| group.windows[*m].hwnd == hwnd))
    {
        shared.cursor = flat.first().map(|m| group.windows[*m].hwnd);
    }

    // The drag: follow the pointer until the button comes up, then drop.
    let dragged = shared.drag.as_ref().and_then(|drag| {
        group
            .windows
            .iter()
            .position(|window| window.hwnd == drag.hwnd)
    });
    if dragged.is_none() {
        shared.drag = None;
    }
    let ctrl = ctx.input(|i| i.modifiers.ctrl);
    let target = match (dragged, pointer) {
        (Some(_), Some(pointer)) => drop_target(pointer, &shared.row_rects, &shared.card_rects),
        _ => None,
    };
    // With Ctrl the drop only changes the number: the card under the
    // pointer says which.
    let ctrl_card = match (dragged, pointer, ctrl) {
        (Some(member), Some(pointer), true) => {
            ctrl_target(pointer, group.windows[member].hwnd, &shared.card_hits)
        }
        _ => None,
    };
    if let Some(member) = dragged {
        if !ctx.input(|i| i.pointer.primary_down()) {
            if ctrl {
                frame
                    .actions
                    .extend(ctrl_card.and_then(|card| ctrl_drop(group, desktops, member, card)));
            } else if let Some(target) = target {
                frame
                    .actions
                    .extend(drop_actions(group, &rows, member, target));
            }
            shared.drag = None;
        }
    }
    let dragging = shared.drag.is_some();
    let gap_width = dragged.map_or(card_width(None), |member| widths[member]);
    // The gap stays where the card was while Ctrl is held: nothing moves.
    let home = dragged.and_then(|member| {
        rows.iter().enumerate().find_map(|(r, row)| {
            row.members
                .iter()
                .position(|m| *m == member)
                .map(|at| (r, at))
        })
    });
    let landing = if ctrl { home } else { target };
    let layout = preview(
        &rows,
        dragged.filter(|_| dragging),
        landing.filter(|_| dragging),
    );

    let mut row_rects: Vec<Rect> = Vec::new();
    let mut card_rects: Vec<Vec<Rect>> = Vec::new();
    let mut hits: Vec<(isize, Rect)> = Vec::new();
    let footer = if rows.len() > 1 { FOOTER } else { 0.0 };
    let rows_area = Rect::from_min_max(content.min, pos2(content.max.x, content.max.y - footer));
    frame.area = rows_area;

    ui.scope_builder(UiBuilder::new().max_rect(rows_area), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("arrange-rows")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(CARD_GAP, 0.0);
                for (row_index, row) in rows.iter().enumerate() {
                    if row_index > 0 {
                        ui.add_space(ROW_GAP);
                    }
                    let row_top = ui.cursor().min.y;
                    text::single(ui, text::section_job(&row.title, tokens.text_secondary));
                    ui.add_space(HEADER_GAP);
                    let mut cards_here: Vec<Rect> = Vec::new();
                    // Rows wrap rather than scroll sideways, so the one scroll
                    // area left is the vertical one and takes the wheel.
                    ui.scope(|ui| {
                        if layout[row_index].is_empty() {
                            placeholder(ui, "No windows on this desktop");
                            return;
                        }
                        let slot_widths: Vec<f32> = layout[row_index]
                            .iter()
                            .map(|slot| match slot {
                                Slot::Window(member) => widths[*member],
                                Slot::Gap => gap_width,
                            })
                            .collect();
                        let lines = wrap(&slot_widths, ui.available_width());
                        for (line_index, line) in lines.into_iter().enumerate() {
                            if line_index > 0 {
                                ui.add_space(CARD_GAP);
                            }
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = vec2(CARD_GAP, 0.0);
                                for slot in &layout[row_index][line] {
                                    let Slot::Window(member) = *slot else {
                                        gap(ui, gap_width);
                                        continue;
                                    };
                                    let window = &group.windows[member];
                                    let desktop_name = desktop_name(desktops, &window.desktop);
                                    let has_picture = !menu_open
                                        && thumbnail(shared, window.hwnd)
                                            .is_some_and(|thumb| thumb.source_size().is_some());
                                    let is_cursor = shared.cursor == Some(window.hwnd);
                                    let (response, picture, rename) = card(
                                        ui,
                                        widths[member],
                                        window,
                                        desktop_name,
                                        CardInfo {
                                            number: numbers[member],
                                            can_rename,
                                            has_picture,
                                            is_cursor,
                                        },
                                    );
                                    if rename {
                                        frame.actions.push(ArrangeAction::AskName(window.hwnd));
                                    }
                                    cards_here.push(response.rect);
                                    hits.push((window.hwnd, response.rect));
                                    frame.cards.push((window.hwnd, window.monitor.is_some()));
                                    if response.hovered() {
                                        frame.hovered = Some(window.hwnd);
                                    }
                                    if has_picture {
                                        frame.pending.push(Pending {
                                            hwnd: window.hwnd,
                                            dest: picture,
                                            clip: ui.clip_rect(),
                                            opacity: 255,
                                            ghost: false,
                                        });
                                    }
                                    if response.clicked() {
                                        frame.activate = Some(window.hwnd);
                                    }
                                    if response.drag_started() {
                                        if let Some(at) = response.interact_pointer_pos() {
                                            shared.drag = Some(Drag {
                                                hwnd: window.hwnd,
                                                grab: at - response.rect.min,
                                            });
                                            shared.cursor = Some(window.hwnd);
                                        }
                                    }
                                    response.context_menu(|ui| {
                                        let names = can_rename.then_some(window.renamed);
                                        if let Some(action) =
                                            card_menu(ui, group, member, desktops, names)
                                        {
                                            frame.actions.push(action);
                                        }
                                    });
                                }
                            });
                        }
                    });
                    card_rects.push(cards_here);
                    row_rects.push(Rect::from_min_max(
                        pos2(rows_area.min.x, row_top),
                        pos2(rows_area.max.x, ui.cursor().min.y),
                    ));
                    if row_index == 0 && others_count > 0 {
                        let (line, _) = ui.allocate_exact_size(
                            vec2(rows_area.width(), others::LINK_HEIGHT),
                            Sense::hover(),
                        );
                        others_link(ui, shared, line, others_count);
                    }
                }
            });
    });
    shared.row_rects = row_rects;
    shared.card_rects = card_rects;
    shared.card_hits = hits;

    // The dragged card rides above everything, following the pointer.
    if let (Some(drag), Some(member), Some(pointer)) = (shared.drag.as_ref(), dragged, pointer) {
        let rect = Rect::from_min_size(pointer - drag.grab, vec2(widths[member], CARD_HEIGHT));
        let window = &group.windows[member];
        let layer = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("arrange-drag"),
        ));
        layer.rect_filled(rect, 6, tokens.hover_bg);
        if let Some(monitor) = &window.monitor {
            monitor_chip(&layer, rect, monitor);
        }
        float_picture(
            &layer,
            &mut frame,
            window.hwnd,
            picture_area(rect),
            menu_open,
        );
        frame.floating = Some(rect);
        if ctrl {
            let place = ctrl_card.and_then(|card| ctrl_place(group, desktops, member, card));
            let outlined = ctrl_card
                .filter(|_| place.is_some())
                .and_then(|card| hit_rect(&shared.card_hits, card));
            let hint = ctrl_mark(&ctx, rect, outlined, &ctrl_hint(place));
            frame.floating = Some(rect.union(hint));
        }
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    frame.dragged = dragged.map(|m| group.windows[m].hwnd);

    if footer > 0.0 {
        let job = text::job(
            "Order on other desktops applies at the next restore.",
            theme::regular(12.0),
            tokens.text_secondary,
            None,
        );
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(
            pos2(content.min.x, content.max.y - galley.size().y),
            galley,
            tokens.text_secondary,
        );
    }

    if !menu_open && !dragging {
        let Frame {
            actions, activate, ..
        } = &mut frame;
        keyboard(&ctx, shared, group, desktops, &rows, actions, activate);
    }
    frame
}

/// The line under this desktop's windows that opens and closes the other
/// desktops. A card dragged onto it opens them after a short rest, so it
/// can be dropped there. `line` has `others::LINK_HEIGHT`, the gap on top.
fn others_link(ui: &mut Ui, shared: &mut Shared, line: Rect, count: usize) {
    let shown = shared.others.shown(count);
    let Some(text) = others::label(count, shown) else {
        return;
    };
    let ctx = ui.ctx().clone();
    let tokens = Tokens::get(&ctx);
    let galley = ui
        .painter()
        .layout_no_wrap(text, theme::regular(12.0), tokens.text_secondary);
    let arrow = 10.0;
    let hit = Rect::from_min_size(line.min, vec2(galley.size().x + 6.0 + arrow, line.height()));
    let response = ui
        .interact(hit, egui::Id::new("arrange-others"), Sense::click())
        .on_hover_text("Press D to show or hide them.");
    let dragging = shared.drag.is_some();
    let over = ctx.pointer_latest_pos().is_some_and(|at| hit.contains(at));
    if response.clicked() {
        shared.others.toggle();
        ctx.request_repaint();
    }
    if let Some(wait) = shared.others.drag_over(Instant::now(), dragging && over) {
        ctx.request_repaint_after(wait);
    }
    if !dragging && response.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let colour = if over {
        tokens.accent
    } else {
        tokens.text_secondary
    };
    let centre = line.bottom() - (line.height() - 6.0) / 2.0;
    let galley_pos = pos2(line.left(), centre - galley.size().y / 2.0);
    ui.painter().galley(galley_pos, galley.clone(), colour);
    // A triangle drawn here, as the fonts may not have one.
    let tip = pos2(line.left() + galley.size().x + 6.0 + arrow / 2.0, centre);
    let (half, rise) = (3.5, 2.0);
    let points = if shown {
        vec![
            pos2(tip.x - half, tip.y + rise),
            pos2(tip.x + half, tip.y + rise),
            pos2(tip.x, tip.y - rise),
        ]
    } else {
        vec![
            pos2(tip.x - half, tip.y - rise),
            pos2(tip.x + half, tip.y - rise),
            pos2(tip.x, tip.y + rise),
        ]
    };
    ui.painter().add(egui::Shape::convex_polygon(
        points,
        colour,
        egui::Stroke::NONE,
    ));
}

fn hit_rect(cards: &[(isize, Rect)], hwnd: isize) -> Option<Rect> {
    cards
        .iter()
        .find(|(card, _)| *card == hwnd)
        .map(|(_, rect)| *rect)
}

/// What a drag with Ctrl held shows: an accent outline round the card that
/// gives its number (`outlined`) and a small hint above the dragged card, or
/// below it when there is no room. Returns the hint's rectangle, so the
/// pictures under it can be hidden: DWM draws them over anything.
fn ctrl_mark(ctx: &egui::Context, ghost: Rect, outlined: Option<Rect>, hint: &str) -> Rect {
    let tokens = Tokens::get(ctx);
    let layer = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("arrange-drag-hint"),
    ));
    if let Some(card) = outlined {
        layer.rect_stroke(
            card,
            6,
            theme::stroke(ctx, 2.0, tokens.accent),
            StrokeKind::Inside,
        );
    }
    let galley = layer.layout_no_wrap(hint.to_string(), theme::regular(12.0), tokens.text_primary);
    let size = galley.size() + vec2(16.0, 8.0);
    let room = ctx.content_rect();
    let above = ghost.top() - size.y - 4.0;
    let top = if above >= room.top() {
        above
    } else {
        ghost.bottom() + 4.0
    };
    let left = ghost.left().min(room.right() - size.x).max(room.left());
    let chip = Rect::from_min_size(pos2(left, top), size);
    layer.rect(
        chip,
        4,
        tokens.panel_bg,
        theme::stroke(ctx, 1.0, tokens.accent),
        StrokeKind::Inside,
    );
    layer.galley(
        chip.center() - galley.size() / 2.0,
        galley,
        tokens.text_primary,
    );
    chip
}

/// The live picture of the dragged window, slightly see-through inside an
/// accent outline, drawn on the layer above everything. Both views use it
/// for the card that follows the pointer.
fn float_picture(
    layer: &egui::Painter,
    frame: &mut Frame,
    hwnd: isize,
    area: Rect,
    menu_open: bool,
) {
    let tokens = Tokens::get(layer.ctx());
    layer.rect(
        area,
        4,
        tokens.panel_bg,
        theme::stroke(layer.ctx(), 1.0, tokens.accent),
        StrokeKind::Inside,
    );
    if !menu_open {
        frame.pending.push(Pending {
            hwnd,
            dest: area.shrink(1.0),
            clip: Rect::EVERYTHING,
            opacity: 204,
            ghost: false,
        });
    }
}

/// The card a test scene asked to peek at, from the cards in their order on
/// screen, each with whether it has a monitor chip.
fn forced_card(cards: &[(isize, bool)], card: PeekCard) -> Option<isize> {
    match card {
        PeekCard::Index(index) => cards.get(index).map(|(hwnd, _)| *hwnd),
        PeekCard::Chip => cards
            .iter()
            .find(|(_, chip)| *chip)
            .or(cards.first())
            .map(|(hwnd, _)| *hwnd),
    }
}

fn desktop_name<'a>(desktops: &'a [ArrangeDesktop], id: &str) -> &'a str {
    desktops
        .iter()
        .find(|desktop| desktop.id == id)
        .map(|desktop| desktop.name.as_str())
        .unwrap_or("")
}

/// The window's picture, registered on first use. Only possible once the
/// strip's own window exists, since it is the destination.
fn thumbnail(shared: &mut Shared, hwnd: isize) -> Option<&mut Thumbnail> {
    registered(&mut shared.thumbs, shared.window, hwnd)
}

/// The second picture of a window that a ghost shows.
fn ghost_thumbnail(shared: &mut Shared, hwnd: isize) -> Option<&mut Thumbnail> {
    registered(&mut shared.ghost_thumbs, shared.window, hwnd)
}

fn registered(
    thumbs: &mut HashMap<isize, Option<Thumbnail>>,
    window: isize,
    hwnd: isize,
) -> Option<&mut Thumbnail> {
    let destination = window as HWND;
    thumbs
        .entry(hwnd)
        .or_insert_with(|| {
            (!destination.is_null())
                .then(|| Thumbnail::register(destination, hwnd as HWND))
                .flatten()
        })
        .as_mut()
}

/// Finds the part of each of the group's windows its picture shows, which
/// gives its card its shape; returns whether any changed. Only possible once
/// the strip's own window exists.
fn refresh_sources(shared: &mut Shared, group: &ArrangeGroup) -> bool {
    if shared.window == 0 {
        return false;
    }
    shared
        .sources
        .retain(|hwnd, _| group.windows.iter().any(|window| window.hwnd == *hwnd));
    let mut changed = false;
    for window in &group.windows {
        let source = thumbnail(shared, window.hwnd)
            .and_then(|thumb| thumb.source_size())
            .map(|size| picture_source(dwm_thumbs::visible_frame(window.hwnd as HWND), size));
        if shared.sources.get(&window.hwnd).copied() != source {
            changed = true;
            match source {
                Some(source) => shared.sources.insert(window.hwnd, source),
                None => shared.sources.remove(&window.hwnd),
            };
        }
    }
    changed
}

/// Puts every picture where its card is. DWM draws pictures on top of
/// everything in the window, so one under the dragged card is hidden, and
/// pictures of windows without a card this frame are hidden too.
fn place_pictures(
    shared: &mut Shared,
    pending: &[Pending],
    floating: Option<Rect>,
    dragged: Option<isize>,
    pixels_per_point: f32,
) {
    for (thumbs, ghost) in [
        (&mut shared.thumbs, false),
        (&mut shared.ghost_thumbs, true),
    ] {
        for (hwnd, thumb) in thumbs.iter_mut() {
            let placed = pending.iter().any(|p| p.hwnd == *hwnd && p.ghost == ghost);
            if let Some(thumb) = thumb.as_mut().filter(|_| !placed) {
                thumb.place(None);
            }
        }
    }
    for item in pending {
        // The dragged window's own picture rides with the pointer; its
        // ghost stays behind like any other.
        let covered = floating.is_some_and(|rect| {
            (item.ghost || Some(item.hwnd) != dragged) && rect.intersects(item.dest)
        });
        let known = shared.sources.get(&item.hwnd).copied();
        let thumb = if item.ghost {
            ghost_thumbnail(shared, item.hwnd)
        } else {
            thumbnail(shared, item.hwnd)
        };
        let Some(thumb) = thumb else {
            continue;
        };
        let Some(size) = thumb.source_size() else {
            thumb.place(None);
            continue;
        };
        if covered {
            thumb.place(None);
            continue;
        }
        let source = known
            .unwrap_or_else(|| picture_source(dwm_thumbs::visible_frame(item.hwnd as HWND), size));
        // The card has the window's shape, so this only leaves a margin for
        // a window narrower or wider than a card can be.
        let fitted = dwm_thumbs::fit(source.size(), item.dest.size());
        let dest = Rect::from_center_size(item.dest.center(), fitted);
        let placement =
            dwm_thumbs::crop(dest, item.clip, source).map(|(shown, cropped)| Placement {
                dest: dwm_thumbs::physical(shown, pixels_per_point),
                source: dwm_thumbs::physical(cropped, 1.0),
                opacity: item.opacity,
            });
        thumb.place(placement);
    }
}

/// What a drop means: a move to the target row's desktop when that differs,
/// then the whole group's new order.
fn drop_actions(
    group: &ArrangeGroup,
    rows: &[Row],
    dragged: usize,
    target: (usize, usize),
) -> Vec<ArrangeAction> {
    let mut actions = Vec::new();
    let from = rows.iter().position(|row| row.members.contains(&dragged));
    let to = &rows[target.0];
    if from != Some(target.0) && !to.desktop.is_empty() {
        actions.push(ArrangeAction::MoveToDesktop {
            hwnd: group.windows[dragged].hwnd,
            desktop: to.desktop.clone(),
        });
    }
    let shown: Vec<isize> = dropped_order(rows, dragged, target)
        .into_iter()
        .map(|member| group.windows[member].hwnd)
        .collect();
    let now: Vec<isize> = group.windows.iter().map(|w| w.hwnd).collect();
    // Windows that are not in the rows (another desktop's, or another
    // monitor's in a limited strip) keep their places.
    let order = merge_order(&now, &shown);
    if order != now || !actions.is_empty() {
        actions.push(ArrangeAction::Reorder {
            group: group.key.clone(),
            order,
        });
    }
    actions
}

/// ←/→ move the cursor, Ctrl+←/→ move its window within its row, Alt+←/→
/// give it the next lower or higher number, Enter switches to it. Esc is
/// handled with the other ways to close.
fn keyboard(
    ctx: &egui::Context,
    shared: &mut Shared,
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    rows: &[Row],
    actions: &mut Vec<ArrangeAction>,
    activate: &mut Option<isize>,
) {
    let flat: Vec<usize> = rows.iter().flat_map(|row| row.members.clone()).collect();
    let Some(position) = shared
        .cursor
        .and_then(|hwnd| flat.iter().position(|m| group.windows[*m].hwnd == hwnd))
    else {
        return;
    };
    let member = flat[position];
    // Alt first: a key pattern without modifiers also matches with Alt down.
    let (alt_left, alt_right, ctrl_left, ctrl_right, left, right, enter) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowLeft),
            i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowRight),
            i.consume_key(egui::Modifiers::CTRL, egui::Key::ArrowLeft),
            i.consume_key(egui::Modifiers::CTRL, egui::Key::ArrowRight),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
        )
    });
    if enter {
        *activate = Some(group.windows[member].hwnd);
        return;
    }
    if alt_left || alt_right {
        let step = if alt_left { -1 } else { 1 };
        actions.extend(nudge(group, desktops, member, step));
        return;
    }
    if left && position > 0 {
        shared.cursor = Some(group.windows[flat[position - 1]].hwnd);
    }
    if right && position + 1 < flat.len() {
        shared.cursor = Some(group.windows[flat[position + 1]].hwnd);
    }
    let Some(row) = rows.iter().position(|row| row.members.contains(&member)) else {
        return;
    };
    let at = rows[row]
        .members
        .iter()
        .position(|m| *m == member)
        .unwrap_or(0);
    if ctrl_left && at > 0 {
        actions.extend(drop_actions(group, rows, member, (row, at - 1)));
    }
    if ctrl_right && at + 1 < rows[row].members.len() {
        actions.extend(drop_actions(group, rows, member, (row, at + 1)));
    }
}

/// Keeps the moves and reorders in `actions` on screen until the plugin's
/// data shows them too, so the steps the host is still doing do not flash by.
fn hold_changes(shared: &mut Shared, actions: &[ArrangeAction]) {
    let Some(snapshot) = shared.snapshot.as_ref() else {
        return;
    };
    let Some(group) = shared.group() else {
        return;
    };
    let place = |device: &str| snapshot.monitors.iter().position(|m| m.device == device);
    let Some(new) = hold::Hold::new(group, actions, &place, Instant::now()) else {
        return;
    };
    shared.holds.retain(|old| old.group() != new.group());
    shared.holds.push(new);
}

/// Shows a change at once; the plugin's own data follows and replaces it
/// (see `hold`).
fn apply_locally(shared: &mut Shared, action: &ArrangeAction) {
    let Some(key) = shared.key.clone() else {
        return;
    };
    let Some(snapshot) = shared.snapshot.as_mut() else {
        return;
    };
    let monitor_place = |device: &str| snapshot.monitors.iter().position(|m| m.device == device);
    let moved_to = match action {
        ArrangeAction::MoveToMonitor { monitor, .. } => monitor_place(monitor),
        _ => None,
    };
    let Some(group) = snapshot.groups.iter_mut().find(|group| group.key == key) else {
        return;
    };
    match action {
        ArrangeAction::MoveToDesktop { hwnd, desktop } => {
            if let Some(window) = group.windows.iter_mut().find(|w| w.hwnd == *hwnd) {
                window.desktop = desktop.clone();
            }
        }
        ArrangeAction::Reorder { order, .. } => {
            group.windows.sort_by_key(|window| {
                order
                    .iter()
                    .position(|h| *h == window.hwnd)
                    .unwrap_or(usize::MAX)
            });
        }
        ArrangeAction::MoveToMonitor { hwnd, .. } => {
            // The plugin saves right after the move, so the saved monitor
            // follows too and no Moved badge flashes up.
            if let Some(window) = group.windows.iter_mut().find(|w| w.hwnd == *hwnd) {
                window.on_monitor = moved_to;
                window.saved_monitor = moved_to;
            }
        }
        ArrangeAction::Close(hwnd) => group.windows.retain(|window| window.hwnd != *hwnd),
        ArrangeAction::Rename {
            hwnd,
            name: Some(name),
        } => {
            if let Some(window) = group.windows.iter_mut().find(|w| w.hwnd == *hwnd) {
                window.label = name.clone();
            }
        }
        ArrangeAction::Rename { name: None, .. }
        | ArrangeAction::AskName(_)
        | ArrangeAction::Activate(_) => {}
    }
}

/// The right-click menu. Closing a window is only offered here, never on a
/// key, because it cannot be undone. `renamed` is None when WindowNamer is
/// off, else whether the window has a name. "Taskbar place" lists the places
/// of the window's row and changes nothing but its number.
fn card_menu(
    ui: &mut Ui,
    group: &ArrangeGroup,
    member: usize,
    desktops: &[ArrangeDesktop],
    renamed: Option<bool>,
) -> Option<ArrangeAction> {
    let hwnd = group.windows[member].hwnd;
    let current = group.windows[member].desktop.as_str();
    let mut chosen = None;
    if let Some(renamed) = renamed {
        if ui.button("Rename\u{2026}").clicked() {
            chosen = Some(ArrangeAction::AskName(hwnd));
            ui.close();
        }
        if renamed && ui.button("Clear name").clicked() {
            chosen = Some(ArrangeAction::Rename { hwnd, name: None });
            ui.close();
        }
        ui.separator();
    }
    if ui.button("Close window").clicked() {
        chosen = Some(ArrangeAction::Close(hwnd));
        ui.close();
    }
    if let Some((place, count)) = place_in_row(group, desktops, member).filter(|(_, n)| *n > 1) {
        ui.menu_button("Taskbar place", |ui| {
            for (index, label) in place_labels(count, place).iter().enumerate() {
                if ui.selectable_label(index + 1 == place, label).clicked() {
                    chosen = take_place(group, desktops, member, index + 1);
                    ui.close();
                }
            }
        });
    }
    ui.menu_button("Move to desktop", |ui| {
        for desktop in desktops.iter().filter(|desktop| desktop.id != current) {
            if ui.button(&desktop.name).clicked() {
                chosen = Some(ArrangeAction::MoveToDesktop {
                    hwnd,
                    desktop: desktop.id.clone(),
                });
                ui.close();
            }
        }
    });
    chosen
}

/// The text of the "Same title" chip's tooltip.
const SAME_TITLE_HINT: &str = "Restores cannot tell these windows apart. Give one a name.";

/// One window, `width` wide: the monitor chip when it is not on the primary
/// monitor, an amber "Same title" chip at the right of that line when
/// another window of the app has the same title (`can_rename` says whether
/// WindowNamer is on, so the chip can offer to rename), its picture area
/// (the live picture is put on top of it later) and its name underneath,
/// after its taskbar number. Without a picture the area shows the name and
/// desktop instead. Returns the card's response, the picture area, and
/// whether the chip was clicked to rename.
fn card(
    ui: &mut Ui,
    width: f32,
    window: &ArrangeWindow,
    desktop: &str,
    info: CardInfo,
) -> (Response, Rect, bool) {
    let CardInfo {
        number,
        can_rename,
        has_picture,
        is_cursor,
    } = info;
    let label = window.label.as_str();
    let monitor = window.monitor.as_deref();
    let same_title = window.same_title.then_some(can_rename);
    let tokens = Tokens::get(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, CARD_HEIGHT), Sense::click_and_drag());
    let lit = response.hovered() || is_cursor;
    let painter = ui.painter();
    let look = card_look(&tokens, lit.then_some(tokens.hover_bg), window.attention);
    paint_look(painter, rect, 6, look);
    if let Some(monitor) = monitor {
        monitor_chip(painter, rect, monitor);
    }
    let area = picture_area(rect);
    let border = if lit { tokens.accent } else { tokens.border };
    painter.rect(
        area,
        4,
        tokens.panel_bg,
        theme::stroke(ui.ctx(), 1.0, border),
        StrokeKind::Inside,
    );
    let picture = area.shrink(1.0);
    if !has_picture {
        name_card(ui.painter(), picture, label, desktop);
    }
    let mut rename = false;
    if let Some(can_rename) = same_title {
        let chip = same_title_chip(ui.painter(), rect);
        let mut chip_response = ui
            .interact(chip, response.id.with("same-title"), Sense::click())
            .on_hover_text(if can_rename {
                format!("{SAME_TITLE_HINT} Click to rename.")
            } else {
                SAME_TITLE_HINT.to_string()
            });
        if can_rename {
            chip_response = chip_response.on_hover_cursor(egui::CursorIcon::PointingHand);
        }
        rename = can_rename && chip_response.clicked();
    }

    let name_top = area.bottom() + CARD_INSET;
    let chip = number_chip(
        ui.painter(),
        pos2(area.left(), name_top),
        NAME_HEIGHT,
        number,
    );
    let name_left = chip.right() + NUMBER_GAP;
    let mut job = text::job(label, theme::regular(12.0), tokens.text_primary, None);
    job.wrap = TextWrapping {
        max_width: (area.right() - name_left).max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    let galley = ui.painter().layout_job(job);
    ui.painter()
        .galley(pos2(name_left, name_top), galley, tokens.text_primary);
    (
        response.on_hover_cursor(egui::CursorIcon::Grab),
        picture,
        rename,
    )
}

/// What a card shows besides its window.
struct CardInfo {
    /// The window's place in Windows' taskbar row, from 1.
    number: usize,
    can_rename: bool,
    has_picture: bool,
    is_cursor: bool,
}

/// The gap between the taskbar number and the name after it.
const NUMBER_GAP: f32 = 4.0;

/// The window's place in Windows' taskbar row, as a small keycap at the
/// start of its name line. It sits below the picture, because DWM draws the
/// picture over anything painted on it, and the name starts after it, so it
/// stays readable on the narrowest card. Returns its rectangle.
fn number_chip(painter: &egui::Painter, at: Pos2, height: f32, number: usize) -> Rect {
    let tokens = Tokens::get(painter.ctx());
    let galley = painter.layout_no_wrap(
        number.to_string(),
        theme::regular(12.0),
        tokens.text_secondary,
    );
    let chip = Rect::from_min_size(at, vec2((galley.size().x + 10.0).max(height), height));
    painter.rect(
        chip,
        4,
        theme::with_alpha(tokens.panel_bg, 0.75),
        theme::stroke(painter.ctx(), 1.0, tokens.border),
        StrokeKind::Inside,
    );
    painter.galley(
        chip.center() - galley.size() / 2.0,
        galley,
        tokens.text_secondary,
    );
    chip
}

/// What is drawn behind a card's picture: a fill and an outline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub fill: Option<Color32>,
    pub outline: Option<Color32>,
}

/// How a card is marked. `lit` is the view's own fill for the card under
/// the mouse or the keys. A window that wants attention gets an orange tint
/// and outline instead, like its orange taskbar button, a little stronger
/// while lit; a view draws its keyboard ring apart.
pub fn card_look(tokens: &Tokens, lit: Option<Color32>, attention: bool) -> Look {
    if attention {
        let tint = if lit.is_some() { 0.28 } else { 0.18 };
        Look {
            fill: Some(theme::with_alpha(tokens.warning, tint)),
            outline: Some(tokens.warning),
        }
    } else {
        Look {
            fill: lit,
            outline: None,
        }
    }
}

/// Paints a card's look in `rect`. The outline stays on the card's edge,
/// clear of the picture, which DWM draws over anything painted under it.
fn paint_look(painter: &egui::Painter, rect: Rect, radius: u8, look: Look) {
    if let Some(fill) = look.fill {
        painter.rect_filled(rect, radius, fill);
    }
    if let Some(outline) = look.outline {
        painter.rect_stroke(
            rect,
            radius,
            theme::stroke(painter.ctx(), 1.5, outline),
            StrokeKind::Inside,
        );
    }
}

/// The amber "Same title" chip, right-aligned in the card's label line;
/// returns its rectangle.
fn same_title_chip(painter: &egui::Painter, card: Rect) -> Rect {
    let tokens = Tokens::get(painter.ctx());
    let galley = painter.layout_no_wrap(
        "Same title".to_string(),
        theme::regular(12.0),
        tokens.warning,
    );
    let chip = Rect::from_min_size(
        pos2(
            card.right() - CARD_INSET - galley.size().x - 12.0,
            card.top() + CARD_INSET,
        ),
        vec2(galley.size().x + 12.0, LABEL_LINE),
    );
    painter.rect(
        chip,
        4,
        tokens.panel_bg,
        theme::stroke(painter.ctx(), 1.0, tokens.warning),
        StrokeKind::Inside,
    );
    painter.galley(chip.center() - galley.size() / 2.0, galley, tokens.warning);
    chip
}

/// Where a card's picture goes, below its label line.
fn picture_area(card: Rect) -> Rect {
    Rect::from_min_size(
        card.min + vec2(CARD_INSET, CARD_INSET + LABEL_LINE + CARD_INSET),
        vec2(card.width() - 2.0 * CARD_INSET, THUMB_HEIGHT),
    )
}

/// The monitor a window is on, left-aligned in the card's label line: the
/// hotkey chip's look at 12 px, with an opaque fill.
fn monitor_chip(painter: &egui::Painter, card: Rect, text: &str) {
    let tokens = Tokens::get(painter.ctx());
    let galley = painter.layout_no_wrap(
        text.to_string(),
        theme::regular(12.0),
        tokens.text_secondary,
    );
    let chip = Rect::from_min_size(
        card.min + Vec2::splat(CARD_INSET),
        vec2(galley.size().x + 12.0, LABEL_LINE),
    );
    painter.rect(
        chip,
        4,
        tokens.panel_bg,
        theme::stroke(painter.ctx(), 1.0, tokens.border),
        StrokeKind::Inside,
    );
    painter.galley(
        chip.center() - galley.size() / 2.0,
        galley,
        tokens.text_secondary,
    );
}

/// The gap a dragged card would land in, as wide as that card.
fn gap(ui: &mut Ui, width: f32) {
    let tokens = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(width, CARD_HEIGHT), Sense::hover());
    let area = picture_area(rect);
    ui.painter().rect(
        area,
        4,
        theme::with_alpha(tokens.accent, 0.12),
        theme::stroke(ui.ctx(), 1.0, tokens.accent),
        StrokeKind::Inside,
    );
}

/// An empty row, still a place to drop a window on.
fn placeholder(ui: &mut Ui, text_line: &str) {
    let tokens = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(card_width(None), CARD_HEIGHT), Sense::hover());
    let area = picture_area(rect);
    ui.painter().rect(
        area,
        4,
        egui::Color32::TRANSPARENT,
        theme::stroke(ui.ctx(), 1.0, tokens.border),
        StrokeKind::Inside,
    );
    let galley = ui.painter().layout_job(text::job(
        text_line,
        theme::regular(12.0),
        tokens.text_secondary,
        None,
    ));
    ui.painter().galley(
        area.center() - galley.size() / 2.0,
        galley,
        tokens.text_secondary,
    );
}

/// Stands in for a picture DWM cannot give, and for all pictures while a menu
/// is open, since DWM would draw them over it.
fn name_card(painter: &egui::Painter, area: Rect, label: &str, desktop: &str) {
    let tokens = Tokens::get(painter.ctx());
    let mut job = text::job(label, theme::semibold(15.0), tokens.text_primary, None);
    job.wrap = TextWrapping {
        max_width: area.width() - 16.0,
        max_rows: 2,
        break_anywhere: false,
        overflow_character: Some('\u{2026}'),
    };
    job.halign = egui::Align::Center;
    let title = painter.layout_job(job);
    let detail = painter.layout_job(text::job(
        desktop,
        theme::regular(12.0),
        tokens.text_secondary,
        None,
    ));
    let height = title.size().y
        + if desktop.is_empty() {
            0.0
        } else {
            4.0 + detail.size().y
        };
    let top = area.center().y - height / 2.0;
    painter.galley(
        pos2(area.center().x, top),
        title.clone(),
        tokens.text_primary,
    );
    if !desktop.is_empty() {
        painter.galley(
            pos2(
                area.center().x - detail.size().x / 2.0,
                top + title.size().y + 4.0,
            ),
            detail,
            tokens.text_secondary,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A card of a 16:9 window.
    const CARD: Vec2 = Vec2::new(152.0, CARD_HEIGHT);
    const SAME: [f32; 20] = [CARD.x; 20];

    fn desktop(id: &str, name: &str, current: bool) -> ArrangeDesktop {
        ArrangeDesktop {
            id: id.to_string(),
            name: name.to_string(),
            current,
        }
    }

    fn group(desktops: &[&str]) -> ArrangeGroup {
        ArrangeGroup {
            key: "chrome.exe".to_string(),
            label: "Chrome".to_string(),
            windows: desktops
                .iter()
                .enumerate()
                .map(|(i, d)| ArrangeWindow {
                    hwnd: i as isize,
                    label: format!("w{i}"),
                    desktop: d.to_string(),
                    monitor: None,
                    on_monitor: None,
                    saved_monitor: None,
                    renamed: false,
                    same_title: false,
                    attention: false,
                })
                .collect(),
        }
    }

    #[test]
    fn a_hover_strip_is_centred_above_its_button() {
        // A 4K monitor at 150 %, taskbar 72 px tall at the bottom.
        let monitor = Monitor {
            work: [0, 0, 3840, 2088],
            scale: 1.5,
        };
        let panel = vec2(400.0, 200.0);
        let [left, top, width, height] = monitor.above_button(panel, [1900, 2088, 1972, 2160]);
        assert_eq!((width, height), (600, 300));
        assert_eq!(left + width / 2, 1936, "centred on the button");
        assert_eq!(top + height, 2088 - 18, "just above the taskbar");
    }

    #[test]
    fn a_hover_strip_stays_inside_the_screen_edges() {
        let monitor = Monitor {
            work: [-1920, 0, 0, 1032],
            scale: 1.0,
        };
        let panel = vec2(800.0, 200.0);
        let near_left = monitor.above_button(panel, [-1900, 1032, -1860, 1080]);
        assert_eq!(near_left[0], -1920);
        let near_right = monitor.above_button(panel, [-60, 1032, -20, 1080]);
        assert_eq!(near_right[0] + near_right[2], 0);
        let wider_than_screen = monitor.above_button(vec2(2400.0, 200.0), [-960, 1032, -920, 1080]);
        assert_eq!(wider_than_screen[0], -1920);
        let too_tall = monitor.above_button(vec2(400.0, 1400.0), [-960, 1032, -920, 1080]);
        assert_eq!(too_tall[1], 0, "never above the top of the screen");
    }

    #[test]
    fn a_taskbar_at_the_top_gets_the_strip_below_it() {
        let monitor = Monitor {
            work: [0, 48, 1920, 1080],
            scale: 1.0,
        };
        let rect = monitor.above_button(vec2(400.0, 200.0), [900, 0, 948, 48]);
        assert_eq!(rect[1], 48 + 12);
        assert_eq!(
            monitor.rect_for(vec2(400.0, 200.0), None),
            monitor.window_rect(vec2(400.0, 200.0)),
            "Win+Alt+A keeps its place"
        );
    }

    #[test]
    fn the_toolbar_counts_monitors_desktops_and_windows() {
        assert_eq!(
            summary(3, 6, 27),
            "3 monitors \u{b7} 6 desktops \u{b7} 27 windows"
        );
        // Without the desktop list there is still the one desktop.
        assert_eq!(
            summary(1, 0, 1),
            "1 monitor \u{b7} 1 desktop \u{b7} 1 window"
        );
    }

    #[test]
    fn a_scene_peeks_at_the_card_it_names() {
        let cards = [(10, false), (11, true), (12, true)];
        assert_eq!(forced_card(&cards, PeekCard::Index(0)), Some(10));
        assert_eq!(forced_card(&cards, PeekCard::Index(2)), Some(12));
        assert_eq!(forced_card(&cards, PeekCard::Index(3)), None);
        assert_eq!(forced_card(&cards, PeekCard::Chip), Some(11));
        // Without a chip card, the first card stands in.
        assert_eq!(
            forced_card(&[(10, false), (11, false)], PeekCard::Chip),
            Some(10)
        );
        assert_eq!(forced_card(&[], PeekCard::Chip), None);
        assert_eq!(forced_card(&[], PeekCard::Index(0)), None);
    }

    #[test]
    fn the_current_desktop_comes_first_then_task_view_order() {
        let desktops = [
            desktop("home", "Home", false),
            desktop("work", "Work", true),
            desktop("read", "Reading", false),
        ];
        let rows = rows(&group(&["read", "work", "home", "work", ""]), &desktops);
        let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["This desktop", "Home", "Reading"]);
        assert_eq!(rows[0].members, [1, 3, 4]);
        assert_eq!(rows[1].members, [2]);
        assert_eq!(rows[2].members, [0]);
    }

    #[test]
    fn taskbar_numbers_count_from_one_on_each_desktop() {
        let desktops = [
            desktop("home", "Home", false),
            desktop("work", "Work", true),
        ];
        let group = group(&["home", "work", "home", "work", "", "home"]);
        // The unknown desktop counts as this one, after the windows before it.
        assert_eq!(taskbar_numbers(&group, &desktops), [1, 1, 2, 2, 3, 3]);
    }

    #[test]
    fn taskbar_numbers_follow_the_order_after_a_drag() {
        let desktops = [desktop("work", "Work", true)];
        let mut group = self::group(&["work", "work", "work"]);
        group.windows.swap(0, 2);
        let numbers = taskbar_numbers(&group, &desktops);
        let place = |hwnd: isize| {
            let member = group.windows.iter().position(|w| w.hwnd == hwnd);
            numbers[member.unwrap()]
        };
        assert_eq!((place(2), place(1), place(0)), (1, 2, 3));
        assert!(taskbar_numbers(&self::group(&[]), &desktops).is_empty());
    }

    #[test]
    fn this_desktop_is_listed_even_when_it_has_no_windows() {
        let desktops = [
            desktop("home", "Home", false),
            desktop("work", "Work", true),
        ];
        let rows = rows(&group(&["home"]), &desktops);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].members.is_empty());
        assert_eq!(rows[1].members, [0]);
    }

    #[test]
    fn the_panel_grows_with_the_widest_row_up_to_the_limit() {
        let row = |n: usize| Row {
            title: String::new(),
            desktop: String::new(),
            members: (0..n).collect(),
        };
        let size = panel_size(&[row(3), row(1)], &SAME, false, 0.0, vec2(5000.0, 5000.0));
        assert_eq!(
            size.x,
            2.0 * PADDING + 3.0 * CARD.x + 2.0 * CARD_GAP + SLACK
        );
        assert_eq!(
            size.y,
            2.0 * PADDING + 2.0 * (HEADER + HEADER_GAP + CARD.y) + ROW_GAP + FOOTER + SLACK
        );
    }

    #[test]
    fn a_long_row_wraps_onto_more_lines_instead_of_scrolling() {
        let row = |n: usize| Row {
            title: String::new(),
            desktop: String::new(),
            members: (0..n).collect(),
        };
        let limit = vec2(2.0 * PADDING + 5.0 * CARD.x + 4.0 * CARD_GAP + 10.0, 5000.0);
        let size = panel_size(&[row(12)], &SAME, false, 0.0, limit);
        assert_eq!(
            size.x,
            2.0 * PADDING + 5.0 * CARD.x + 4.0 * CARD_GAP + SLACK
        );
        // 12 cards at 5 a line make 3 lines.
        assert_eq!(
            size.y,
            2.0 * PADDING + HEADER + HEADER_GAP + 3.0 * CARD.y + 2.0 * CARD_GAP + SLACK
        );
        let short = panel_size(&[row(12)], &SAME, false, 0.0, vec2(limit.x, 200.0));
        assert_eq!(short.y, 200.0);
    }

    #[test]
    fn a_picture_keeps_the_windows_shape_within_limits() {
        let close = |a: f32, b: f32| (a - b).abs() < 0.01;
        // A 16:9 window, a portrait one and an unknown size, 81 tall.
        assert!(close(
            picture_width(Some(vec2(1920.0, 1080.0)), 81.0),
            144.0
        ));
        assert!(close(picture_width(Some(vec2(1000.0, 1250.0)), 81.0), 65.0));
        assert!(close(picture_width(None, 81.0), 144.0));
        assert!(close(picture_width(Some(vec2(0.0, 900.0)), 81.0), 144.0));
        // Very narrow and very wide windows stop at 0.6 and 2.4 heights.
        assert!(close(picture_width(Some(vec2(200.0, 2000.0)), 100.0), 60.0));
        assert!(close(
            picture_width(Some(vec2(7680.0, 1080.0)), 100.0),
            240.0
        ));
        // Whole points, so a window a pixel wider gives the same card.
        assert_eq!(
            picture_width(Some(vec2(1921.0, 1080.0)), 81.0),
            picture_width(Some(vec2(1920.0, 1080.0)), 81.0)
        );
        assert_eq!(card_width(None), CARD.x);
    }

    #[test]
    fn a_picture_shows_the_visible_frame_or_else_the_whole_window() {
        let size = vec2(1936.0, 1048.0);
        let frame = Rect::from_min_max(pos2(8.0, 0.0), pos2(1928.0, 1040.0));
        assert_eq!(picture_source(Some(frame), size), frame);
        let whole = Rect::from_min_size(pos2(0.0, 0.0), size);
        assert_eq!(picture_source(None, size), whole);
        // A minimized window's frame is its small one while minimized.
        let iconic = Rect::from_min_max(pos2(0.0, 0.0), pos2(160.0, 28.0));
        assert_eq!(picture_source(Some(iconic), size), whole);
        let outside = Rect::from_min_max(pos2(-8.0, 0.0), pos2(1928.0, 1040.0));
        assert_eq!(picture_source(Some(outside), size), whole);
    }

    #[test]
    fn cards_of_mixed_widths_wrap_where_the_next_one_does_not_fit() {
        // 100 + 8 + 200 = 308 fits in 310; the 60 after it does not.
        let widths = [100.0, 200.0, 60.0, 60.0, 300.0];
        assert_eq!(wrap(&widths, 310.0), [0..2, 2..4, 4..5]);
        // A card wider than the line still gets a line of its own.
        assert_eq!(wrap(&[400.0, 50.0], 310.0), [0..1, 1..2]);
        assert_eq!(wrap(&widths, 5000.0).first(), Some(&(0..5)));
        assert_eq!(wrap(&widths, 5000.0).len(), 1);
        assert!(wrap(&[], 310.0).is_empty());
        assert_eq!(line_width(&[100.0, 200.0]), 308.0);
        assert_eq!(line_width(&[]), 0.0);
    }

    #[test]
    fn the_panel_follows_the_widths_of_the_cards() {
        let rows = [
            Row {
                title: String::new(),
                desktop: String::new(),
                members: vec![0, 1, 2],
            },
            Row {
                title: String::new(),
                desktop: String::new(),
                members: vec![3],
            },
        ];
        // A wide, a narrow and a 16:9 window, then a very wide one alone.
        let widths = [200.0, 60.0, CARD.x, 300.0];
        let size = panel_size(&rows, &widths, false, 0.0, vec2(5000.0, 5000.0));
        assert_eq!(size.x, 2.0 * PADDING + 412.0 + 2.0 * CARD_GAP + SLACK);
        // Squeezed to 300 points of cards, the first row needs two lines.
        let limit = vec2(2.0 * PADDING + 300.0, 5000.0);
        let size = panel_size(&rows, &widths, false, 0.0, limit);
        assert_eq!(size.x, limit.x);
        assert_eq!(
            size.y,
            2.0 * PADDING
                + 2.0 * (HEADER + HEADER_GAP)
                + 3.0 * CARD_HEIGHT
                + CARD_GAP
                + ROW_GAP
                + FOOTER
                + SLACK
        );
        // Never narrower than the toolbar needs.
        let size = panel_size(&rows[1..], &widths, false, 700.0, vec2(5000.0, 5000.0));
        assert_eq!(size.x, 700.0);
    }

    #[test]
    fn a_card_that_wants_attention_is_tinted_and_outlined_in_orange() {
        for tokens in [Tokens::dark(), Tokens::light()] {
            let plain = card_look(&tokens, None, false);
            assert_eq!(
                plain,
                Look {
                    fill: None,
                    outline: None
                }
            );
            let lit = card_look(&tokens, Some(tokens.hover_bg), false);
            assert_eq!(lit.fill, Some(tokens.hover_bg));
            assert_eq!(lit.outline, None);

            let marked = card_look(&tokens, None, true);
            assert_eq!(marked.outline, Some(tokens.warning));
            let fill = marked.fill.expect("a tint");
            // The theme's orange, see-through.
            assert!(fill.a() > 0 && fill.a() < 255);
            let [r, g, b, _] = fill.to_srgba_unmultiplied();
            let near = |a: u8, b: u8| a.abs_diff(b) <= 4;
            assert!(near(r, tokens.warning.r()));
            assert!(near(g, tokens.warning.g()));
            assert!(near(b, tokens.warning.b()));
            // Stronger under the mouse or the keys.
            let marked_lit = card_look(&tokens, Some(tokens.hover_bg), true);
            assert!(marked_lit.fill.expect("a tint").a() > fill.a());
            assert_eq!(marked_lit.outline, Some(tokens.warning));
        }
    }

    #[test]
    fn the_flat_view_shows_only_this_desktop_until_the_others_are_shown() {
        let desktops = [desktop("a", "Home", true), desktop("b", "Work", false)];
        let group = group(&["a", "b", "b"]);
        assert_eq!(shown_rows(&group, &desktops, false, &[true; 3]).len(), 1);
        assert_eq!(shown_rows(&group, &desktops, true, &[true; 3]).len(), 2);
        let limit = vec2(5000.0, 5000.0);
        let shut = panel_size(
            &shown_rows(&group, &desktops, false, &[true; 3]),
            &SAME,
            true,
            0.0,
            limit,
        );
        let open = panel_size(
            &shown_rows(&group, &desktops, true, &[true; 3]),
            &SAME,
            true,
            0.0,
            limit,
        );
        assert!(shut.y < open.y);
        // The footnote about other desktops needs a second row.
        let alone = panel_size(
            &shown_rows(&group, &desktops, false, &[true; 3]),
            &SAME,
            false,
            0.0,
            limit,
        );
        assert!((shut.y - alone.y - others::LINK_HEIGHT).abs() < 0.01);
    }

    fn two_rows() -> Vec<Row> {
        vec![
            Row {
                title: "This desktop".to_string(),
                desktop: "work".to_string(),
                members: vec![0, 1, 2],
            },
            Row {
                title: "Reading".to_string(),
                desktop: "read".to_string(),
                members: vec![3],
            },
        ]
    }

    #[test]
    fn a_drag_shows_a_gap_where_the_window_will_land() {
        let rows = two_rows();
        let layout = preview(&rows, Some(0), Some((0, 2)));
        assert_eq!(layout[0], [Slot::Window(1), Slot::Window(2), Slot::Gap]);
        assert_eq!(layout[1], [Slot::Window(3)]);
        let untouched = preview(&rows, None, None);
        assert_eq!(
            untouched[0],
            [Slot::Window(0), Slot::Window(1), Slot::Window(2)]
        );
    }

    #[test]
    fn a_drop_gives_the_whole_new_order_row_by_row() {
        let rows = two_rows();
        assert_eq!(dropped_order(&rows, 0, (0, 2)), [1, 2, 0, 3]);
        assert_eq!(dropped_order(&rows, 2, (0, 0)), [2, 0, 1, 3]);
        assert_eq!(dropped_order(&rows, 1, (1, 0)), [0, 2, 1, 3]);
        assert_eq!(dropped_order(&rows, 3, (0, 1)), [0, 3, 1, 2]);
    }

    #[test]
    fn a_drop_on_another_row_moves_the_window_to_that_desktop_first() {
        let rows = two_rows();
        let group = group(&["work", "work", "work", "read"]);
        let actions = drop_actions(&group, &rows, 1, (1, 1));
        assert_eq!(
            actions[0],
            ArrangeAction::MoveToDesktop {
                hwnd: 1,
                desktop: "read".to_string()
            }
        );
        assert_eq!(
            actions[1],
            ArrangeAction::Reorder {
                group: "chrome.exe".to_string(),
                order: vec![0, 2, 3, 1]
            }
        );
        assert!(drop_actions(&group, &rows, 0, (0, 0)).is_empty());
    }

    #[test]
    fn the_drop_target_comes_from_last_frames_layout() {
        let row_rects = [
            Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 200.0)),
            Rect::from_min_max(pos2(0.0, 216.0), pos2(800.0, 416.0)),
        ];
        let card = |x: f32, y: f32| Rect::from_min_size(pos2(x, y), CARD);
        let card_rects = vec![
            vec![card(0.0, 20.0), card(228.0, 20.0)],
            vec![card(0.0, 236.0)],
        ];
        assert_eq!(
            drop_target(pos2(300.0, 100.0), &row_rects, &card_rects),
            Some((0, 1))
        );
        assert_eq!(
            drop_target(pos2(500.0, 100.0), &row_rects, &card_rects),
            Some((0, 2))
        );
        assert_eq!(
            drop_target(pos2(10.0, 300.0), &row_rects, &card_rects),
            Some((1, 0))
        );
        assert_eq!(
            drop_target(pos2(10.0, 900.0), &row_rects, &card_rects),
            Some((1, 1))
        );

        // Cards of other widths: the middle of each real card counts.
        let mixed = vec![vec![
            Rect::from_min_size(pos2(0.0, 20.0), vec2(60.0, CARD_HEIGHT)),
            Rect::from_min_size(pos2(68.0, 20.0), vec2(300.0, CARD_HEIGHT)),
        ]];
        let one = [Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 200.0))];
        assert_eq!(drop_target(pos2(40.0, 60.0), &one, &mixed), Some((0, 1)));
        assert_eq!(drop_target(pos2(200.0, 60.0), &one, &mixed), Some((0, 1)));
        assert_eq!(drop_target(pos2(230.0, 60.0), &one, &mixed), Some((0, 2)));

        // A wrapped row: two cards on the first line, one on the second.
        let wrapped = vec![vec![card(0.0, 20.0), card(228.0, 20.0), card(0.0, 200.0)]];
        let tall = [Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 400.0))];
        assert_eq!(
            drop_target(pos2(300.0, 250.0), &tall, &wrapped),
            Some((0, 3))
        );
        assert_eq!(
            drop_target(pos2(10.0, 250.0), &tall, &wrapped),
            Some((0, 2))
        );
        assert_eq!(drop_target(pos2(10.0, 60.0), &tall, &wrapped), Some((0, 0)));
    }

    fn mixed_desktops() -> (Vec<ArrangeDesktop>, ArrangeGroup) {
        (
            vec![
                desktop("home", "Home", false),
                desktop("work", "Work", true),
            ],
            group(&["work", "home", "work", "work", "home", "work"]),
        )
    }

    fn reorder(action: Option<ArrangeAction>) -> Option<Vec<isize>> {
        match action {
            Some(ArrangeAction::Reorder { order, .. }) => Some(order),
            Some(other) => panic!("only a Reorder changes a number, got {other:?}"),
            None => None,
        }
    }

    #[test]
    fn a_window_takes_a_place_in_its_own_desktops_row_only() {
        let (desktops, group) = mixed_desktops();
        // This desktop is windows 0, 2, 3 and 5; window 5 becomes number 2.
        assert_eq!(
            order_with_place(&group, &desktops, 5, 2),
            Some(vec![0, 1, 5, 2, 4, 3])
        );
        // The other desktop's windows (1 and 4) keep their slots and order.
        assert_eq!(
            order_with_place(&group, &desktops, 4, 1),
            Some(vec![0, 4, 2, 3, 1, 5])
        );
        // Number 1 and the last number, and a place past the end.
        assert_eq!(
            order_with_place(&group, &desktops, 3, 1),
            Some(vec![3, 1, 0, 2, 4, 5])
        );
        assert_eq!(
            order_with_place(&group, &desktops, 0, 4),
            Some(vec![2, 1, 3, 5, 4, 0])
        );
        assert_eq!(
            order_with_place(&group, &desktops, 0, 99),
            order_with_place(&group, &desktops, 0, 4)
        );
        // Its own number changes nothing.
        assert_eq!(order_with_place(&group, &desktops, 3, 3), None);
        assert_eq!(order_with_place(&group, &desktops, 9, 1), None);
    }

    #[test]
    fn a_new_number_is_one_reorder_and_never_a_move() {
        let (desktops, group) = mixed_desktops();
        assert_eq!(
            take_place(&group, &desktops, 5, 2),
            Some(ArrangeAction::Reorder {
                group: "chrome.exe".to_string(),
                order: vec![0, 1, 5, 2, 4, 3]
            })
        );
        assert_eq!(take_place(&group, &desktops, 5, 4), None);
    }

    #[test]
    fn a_window_knows_its_number_and_how_many_share_the_row() {
        let (desktops, group) = mixed_desktops();
        assert_eq!(place_in_row(&group, &desktops, 3), Some((3, 4)));
        assert_eq!(place_in_row(&group, &desktops, 4), Some((2, 2)));
        assert_eq!(place_in_row(&group, &desktops, 7), None);
    }

    #[test]
    fn alt_arrows_step_one_number_and_stop_at_the_ends() {
        let (desktops, group) = mixed_desktops();
        // Window 2 is number 2: Alt+Left makes it 1, Alt+Right makes it 3.
        assert_eq!(
            reorder(nudge(&group, &desktops, 2, -1)),
            Some(vec![2, 1, 0, 3, 4, 5])
        );
        assert_eq!(
            reorder(nudge(&group, &desktops, 2, 1)),
            Some(vec![0, 1, 3, 2, 4, 5])
        );
        assert_eq!(nudge(&group, &desktops, 0, -1), None, "already number 1");
        assert_eq!(nudge(&group, &desktops, 5, 1), None, "already the last");
        // On another desktop, in its own row.
        assert_eq!(
            reorder(nudge(&group, &desktops, 4, -1)),
            Some(vec![0, 4, 2, 3, 1, 5])
        );
        assert_eq!(nudge(&group, &desktops, 7, 1), None);
    }

    #[test]
    fn the_menu_lists_every_place_and_marks_the_current_one() {
        assert_eq!(place_labels(4, 2), ["1", "2 (now)", "3", "4"]);
        assert_eq!(place_labels(2, 2), ["1", "2 (now)"]);
        assert!(place_labels(0, 1).is_empty());
    }

    #[test]
    fn a_ctrl_drop_lands_on_the_card_under_the_pointer() {
        let card = |x: f32| Rect::from_min_size(pos2(x, 20.0), CARD);
        let cards = [(7, card(0.0)), (8, card(160.0)), (9, card(320.0))];
        assert_eq!(ctrl_target(pos2(200.0, 60.0), 7, &cards), Some(8));
        // The gap between two cards belongs to a card.
        assert_eq!(ctrl_target(pos2(154.0, 60.0), 9, &cards), Some(7));
        // Never the dragged window's own card, nor empty space.
        assert_eq!(ctrl_target(pos2(60.0, 60.0), 7, &cards), None);
        assert_eq!(ctrl_target(pos2(600.0, 60.0), 7, &cards), None);
        assert_eq!(ctrl_target(pos2(60.0, 500.0), 8, &cards), None);
    }

    #[test]
    fn a_ctrl_drop_gives_the_dragged_window_the_targets_number() {
        let (desktops, group) = mixed_desktops();
        // Window 5 (number 4) dropped on window 2 (number 2) becomes number 2.
        assert_eq!(ctrl_place(&group, &desktops, 5, 2), Some(2));
        assert_eq!(
            reorder(ctrl_drop(&group, &desktops, 5, 2)),
            Some(vec![0, 1, 5, 2, 4, 3])
        );
        // Dropped on a card further along, it takes that card's number.
        assert_eq!(ctrl_place(&group, &desktops, 0, 5), Some(4));
        assert_eq!(
            reorder(ctrl_drop(&group, &desktops, 0, 5)),
            Some(vec![2, 1, 3, 5, 4, 0])
        );
    }

    #[test]
    fn a_ctrl_drop_on_nothing_useful_changes_nothing() {
        let (desktops, group) = mixed_desktops();
        // A card of the other desktop has a number of that desktop's row.
        assert_eq!(ctrl_place(&group, &desktops, 5, 1), None);
        assert_eq!(ctrl_drop(&group, &desktops, 5, 1), None);
        // Itself, an unknown window, and the card right before it.
        assert_eq!(ctrl_place(&group, &desktops, 5, 5), None);
        assert_eq!(ctrl_place(&group, &desktops, 5, 42), None);
        assert_eq!(ctrl_drop(&group, &desktops, 3, 3), None);
        assert_eq!(
            reorder(ctrl_drop(&group, &desktops, 3, 2)),
            Some(vec![0, 1, 3, 2, 4, 5])
        );
    }

    #[test]
    fn the_ctrl_hint_names_the_number_or_asks_for_a_card() {
        assert_eq!(ctrl_hint(Some(2)), "Change number only: takes number 2");
        assert!(ctrl_hint(None).starts_with("Change number only"));
    }

    fn screen(rect: [i32; 4], primary: bool) -> ArrangeMonitor {
        ArrangeMonitor {
            device: String::new(),
            label: String::new(),
            name: String::new(),
            rect,
            work: rect,
            primary,
            connected: true,
        }
    }

    /// The primary monitor, one to its left and one to its right.
    fn three_screens() -> Vec<ArrangeMonitor> {
        vec![
            screen([0, 0, 3840, 2160], true),
            screen([-1920, 0, 0, 1080], false),
            screen([3840, 0, 6400, 2880], false),
        ]
    }

    const RIGHT_BUTTON: [i32; 4] = [5000, 2830, 5048, 2880];
    const MAIN_BUTTON: [i32; 4] = [1900, 2100, 1948, 2160];

    #[test]
    fn a_secondary_taskbar_limits_the_strip_to_its_monitor_in_mode_1() {
        let screens = three_screens();
        let mode = |enabled, mode| MultiMonitor::from_values(Some(enabled), Some(mode));
        assert_eq!(
            only_monitor(Some(RIGHT_BUTTON), &screens, mode(1, 1)),
            Some(2)
        );
        assert_eq!(only_monitor(Some(MAIN_BUTTON), &screens, mode(1, 1)), None);
    }

    #[test]
    fn every_taskbar_limits_the_strip_in_mode_2_and_none_in_mode_0() {
        let screens = three_screens();
        let mode = |enabled, mode| MultiMonitor::from_values(Some(enabled), Some(mode));
        assert_eq!(
            only_monitor(Some(MAIN_BUTTON), &screens, mode(1, 2)),
            Some(0)
        );
        assert_eq!(
            only_monitor(Some(RIGHT_BUTTON), &screens, mode(1, 2)),
            Some(2)
        );
        assert_eq!(only_monitor(Some(RIGHT_BUTTON), &screens, mode(1, 0)), None);
        assert_eq!(only_monitor(Some(MAIN_BUTTON), &screens, mode(1, 0)), None);
    }

    #[test]
    fn one_taskbar_a_hotkey_strip_or_an_unknown_place_limit_nothing() {
        let screens = three_screens();
        let mode_2 = MultiMonitor::from_values(Some(1), Some(2));
        let one_taskbar = MultiMonitor::from_values(Some(0), Some(2));
        assert_eq!(
            only_monitor(Some(RIGHT_BUTTON), &screens, one_taskbar),
            None
        );
        assert_eq!(only_monitor(None, &screens, mode_2), None, "Win+Alt+A");
        assert_eq!(
            only_monitor(Some([9000, 0, 9040, 40]), &screens, mode_2),
            None
        );
        let mut gone = three_screens();
        gone[2].connected = false;
        assert_eq!(only_monitor(Some(RIGHT_BUTTON), &gone, mode_2), None);
    }

    fn on_screens(places: &[Option<usize>]) -> ArrangeGroup {
        let mut group = group(&vec!["work"; places.len()]);
        for (window, place) in group.windows.iter_mut().zip(places) {
            window.on_monitor = *place;
            window.saved_monitor = *place;
        }
        group
    }

    #[test]
    fn a_limited_strip_shows_the_windows_of_its_monitor() {
        let screens = three_screens();
        // Windows 1 and 3 are on the right monitor; 4 has no known place and
        // counts as on the main one.
        let group = on_screens(&[Some(0), Some(2), Some(1), Some(2), None]);
        assert_eq!(
            visible_flags(&group, &screens, Some(2)),
            [false, true, false, true, false]
        );
        assert_eq!(
            visible_flags(&group, &screens, Some(0)),
            [true, false, false, false, true]
        );
        assert_eq!(visible_flags(&group, &screens, None), [true; 5]);
    }

    #[test]
    fn the_shown_windows_keep_their_own_numbers() {
        let desktops = [desktop("work", "Work", true)];
        let screens = three_screens();
        let group = on_screens(&[Some(0), Some(2), Some(0), Some(2), Some(0), Some(2)]);
        let visible = visible_flags(&group, &screens, Some(2));
        let rows = shown_rows(&group, &desktops, false, &visible);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].members, [1, 3, 5]);
        // Numbers come from the whole row, so they skip: 2, 4 and 6.
        let numbers = taskbar_numbers(&group, &desktops);
        let shown: Vec<usize> = rows[0].members.iter().map(|m| numbers[*m]).collect();
        assert_eq!(shown, [2, 4, 6]);
    }

    #[test]
    fn another_desktops_row_goes_when_none_of_its_windows_are_shown() {
        let desktops = [
            desktop("home", "Home", false),
            desktop("work", "Work", true),
        ];
        let group = group(&["work", "home", "work", "home"]);
        let visible = [true, false, true, false];
        assert_eq!(shown_rows(&group, &desktops, true, &visible).len(), 1);
        assert_eq!(shown_rows(&group, &desktops, true, &[true; 4]).len(), 2);
    }

    #[test]
    fn reordering_shown_windows_keeps_the_others_where_they_were() {
        assert_eq!(
            merge_order(&[0, 1, 2, 3, 4, 5], &[4, 2]),
            [0, 1, 4, 3, 2, 5]
        );
        assert_eq!(merge_order(&[0, 1, 2, 3], &[0, 1, 2, 3]), [0, 1, 2, 3]);
        // A handle that is not in the group is ignored.
        assert_eq!(merge_order(&[0, 1, 2], &[2, 9, 0]), [2, 1, 0]);
        assert_eq!(merge_order(&[0, 1, 2], &[]), [0, 1, 2]);
    }

    #[test]
    fn a_drop_in_a_limited_strip_reorders_the_whole_group() {
        let desktops = [desktop("work", "Work", true)];
        let screens = three_screens();
        let group = on_screens(&[Some(0), Some(2), Some(0), Some(2), Some(0), Some(2)]);
        let visible = visible_flags(&group, &screens, Some(0));
        let rows = shown_rows(&group, &desktops, false, &visible);
        assert_eq!(rows[0].members, [0, 2, 4]);
        // Window 4 dropped first: the right-monitor windows 1, 3, 5 stay.
        let actions = drop_actions(&group, &rows, 4, (0, 0));
        assert_eq!(
            actions,
            [ArrangeAction::Reorder {
                group: "chrome.exe".to_string(),
                order: vec![4, 1, 0, 3, 2, 5]
            }]
        );
        assert!(drop_actions(&group, &rows, 0, (0, 0)).is_empty());
    }
}
