use std::collections::{HashMap, HashSet};
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
    GetCursorPos, GetWindowRect, IsWindowVisible, SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE,
};

use crate::core::open_timing::{self, OpenStep};
use crate::core::taskbar_scope::MultiMonitor;
use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{
    self, ArrangeAction, ArrangeDesktop, ArrangeGroup, ArrangeMonitor, ArrangeSnapshot,
    ArrangeView, ArrangeWindow, HostChannel, HostRequest, PeekCard,
};
use crate::ui::dwm_thumbs::{self, picture_source, Placement, Thumbnail};
use crate::ui::peek::{self, Hover, Peek, Step};
use crate::ui::settings;
use crate::ui::widgets::empty_state::empty_state;
use crate::ui::widgets::icons::{self, Icon};
use crate::ui::widgets::{choice, focus, text};

mod by_monitor;
mod cards;
mod compact;
mod flat;
mod hold;
mod input;
mod motion;
mod others;
mod pictures;
mod pointer;
mod rename;
mod reveal;
mod rows;
mod sizing;
mod strip;
mod window;
pub use cards::*;
use flat::*;
pub use input::*;
pub use motion::*;
use pictures::*;
pub use rows::*;
pub use sizing::*;
pub use strip::*;
use window::*;

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
/// Losing the focus does not close the strip while a Windows key is down,
/// nor this long after it was let go: Win and a digit may move the strip,
/// and Win let go alone closes it through the host.
const WIN_GRACE: Duration = Duration::from_millis(300);
/// How long a note such as "Button 4 has no windows" stays.
const NOTE_TIME: Duration = Duration::from_secs(2);
/// How long a card takes to come in or to go.
const CARD_MOTION: Duration = Duration::from_millis(150);
/// A new card starts at this part of its size.
const CARD_GROW_FROM: f32 = 0.9;

pub fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("wincraft-arrange")
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
    /// The By monitor view's ghost of the window was picked up, not its own
    /// card.
    ghost: bool,
}

struct Shared {
    snapshot: Option<ArrangeSnapshot>,
    /// Changes made here that the plugin's data has not caught up with yet.
    holds: Vec<hold::Hold>,
    /// The taskbar group on screen.
    key: Option<String>,
    /// The card the arrow keys act on.
    cursor: Option<isize>,
    /// In the By monitor view the keys are on the ghost of `cursor`, not on
    /// its own card.
    cursor_ghost: bool,
    /// The By monitor view's cards and ghosts drawn last frame, for ↑ and ↓.
    items: Vec<(by_monitor::Item, Rect)>,
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
    /// Every window the strip has shown since it opened.
    known: HashSet<isize>,
    /// When each window that came while the strip was open appeared.
    appeared: HashMap<isize, Instant>,
    leaving: Vec<Leaving>,
    /// The taskbar number of the window to pick once the strip has its
    /// group, for a strip opened by Win+Alt+A and a digit.
    select: Option<usize>,
    /// Such a strip takes the keyboard, although it sits above a button.
    wants_keyboard: bool,
    /// The window to pick once the strip has its group, for the strip the
    /// hotkey pressed twice opened.
    select_window: Option<isize>,
    /// Pick the card after the picked one: Win and the same number again.
    select_next: bool,
    /// A short line on screen, and since when.
    note: Option<(String, Instant)>,
    /// When the host asked for the strip, for the log line that says how
    /// soon it was on screen.
    asked_at: Option<Instant>,
    /// The taskbar button a hover opened the strip above. Such a strip
    /// leaves the keyboard with the window in front.
    anchor: Option<[i32; 4]>,
    /// The compact popup's left edge while it is closed, kept when the other
    /// desktops open so its own cards stay where they are.
    compact_left: Option<i32>,
    /// How far the strip's window is on its way to the screen.
    reveal: reveal::Reveal,
    /// The window `reveal` cloaked.
    reveal_window: isize,
    /// The strip is to take the keyboard once it is on screen.
    focus_wanted: bool,
    /// Whether the mouse has moved since the strip opened.
    pointer: pointer::Pointer,
    /// Ctrl+W closed the last card: the strip goes once its window is gone.
    close_when_empty: bool,
    /// The title being edited after F2.
    rename: Option<rename::Rename>,
    /// When a Windows key was last seen down while the strip was open.
    win_seen: Option<Instant>,
}

impl Shared {
    fn hide_peek(&mut self) {
        self.peek.hide();
        self.hover = Hover::default();
    }

    /// A card's size and opacity now; see `card_motion`.
    fn motion(&self, hwnd: isize) -> (f32, f32) {
        let now = Instant::now();
        card_motion(
            self.appeared
                .get(&hwnd)
                .map(|at| now.saturating_duration_since(*at)),
            self.leaving
                .iter()
                .find(|gone| gone.window.hwnd == hwnd)
                .map(|gone| now.saturating_duration_since(gone.since)),
        )
    }

    /// The window is gone and its card is fading out: it takes no clicks.
    fn is_leaving(&self, hwnd: isize) -> bool {
        self.leaving.iter().any(|gone| gone.window.hwnd == hwnd)
    }

    /// Ends the motions that are over, and the windows that have faded out.
    fn settle_motion(&mut self) -> bool {
        let now = Instant::now();
        let over = |since: Instant| now.saturating_duration_since(since) >= CARD_MOTION;
        let done: Vec<isize> = self
            .leaving
            .iter()
            .filter(|gone| over(gone.since))
            .map(|gone| gone.window.hwnd)
            .collect();
        if !done.is_empty() {
            self.leaving
                .retain(|gone| !done.contains(&gone.window.hwnd));
            if let Some(snapshot) = self.snapshot.as_mut() {
                for group in &mut snapshot.groups {
                    group.windows.retain(|w| !done.contains(&w.hwnd));
                }
            }
        }
        self.appeared.retain(|_, at| !over(*at));
        !self.leaving.is_empty() || !self.appeared.is_empty()
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

    /// Whether the group on screen is shown in the compact popup.
    fn compact(&self) -> bool {
        match (self.group(), &self.snapshot) {
            (Some(group), Some(snapshot)) => {
                compact::applies(group, &snapshot.desktops, &self.visible(group))
            }
            _ => false,
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
            (Some(group), Some(snapshot)) if self.compact() => {
                compact::panel_size(self, group, snapshot).min(limit)
            }
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
                let note = snapshot.keeps_order;
                let mut size = panel_size(&rows, &widths, count > 0, note, min_width, limit);
                size.y = (size.y + SWITCHER).min(limit.y);
                size
            }
            _ => EMPTY_PANEL.min(limit),
        }
    }

    /// Where the strip's window belongs now, in screen pixels.
    fn wanted_rect(&mut self) -> Option<[i32; 4]> {
        let monitor = self.monitor?;
        let panel = self.panel();
        let mut rect = monitor.rect_for(panel, self.anchor);
        if self.compact() {
            if self.others.shown(self.others_count()) {
                if let Some(left) = self.compact_left {
                    rect[0] = compact::keep_left(left, rect[2], monitor.work);
                }
            } else {
                self.compact_left = Some(rect[0]);
            }
        }
        Some(rect)
    }
}

/// The mouse way to see and order a program's windows: live pictures of all
/// of them, one row per desktop, in taskbar order.
pub struct Arrange {
    shared: Arc<Mutex<Shared>>,
    palette_hwnd: isize,
}

#[cfg(test)]
mod tests;
