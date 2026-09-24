use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::text::TextWrapping;
use egui::{pos2, vec2, Rect, Response, Sense, StrokeKind, Ui, UiBuilder, Vec2};
use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowRect, SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE,
};

use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{
    ArrangeAction, ArrangeDesktop, ArrangeGroup, ArrangeSnapshot, HostChannel, HostRequest,
};
use crate::ui::dwm_thumbs::{self, Placement, Thumbnail};
use crate::ui::settings;
use crate::ui::widgets::empty_state::empty_state;
use crate::ui::widgets::{focus, text};

/// Unique, so the strip's window can be found among the UI thread's windows.
const TITLE: &str = "WinCraft Arrange";

const PADDING: f32 = 20.0;
const THUMB: Vec2 = Vec2::new(200.0, 120.0);
const CARD_INSET: f32 = 8.0;
const NAME_HEIGHT: f32 = 20.0;
const CARD: Vec2 = Vec2::new(
    THUMB.x + 2.0 * CARD_INSET,
    CARD_INSET + THUMB.y + CARD_INSET + NAME_HEIGHT + CARD_INSET,
);
const CARD_GAP: f32 = 12.0;
const HEADER: f32 = 15.0;
const HEADER_GAP: f32 = 8.0;
const ROW_GAP: f32 = 16.0;
const EMPTY_PANEL: Vec2 = Vec2::new(520.0, 150.0);
/// Space left between the strip and the taskbar.
const TASKBAR_GAP: f32 = 12.0;

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

/// The panel size that shows every row with all its cards, within `limit`;
/// rows that do not fit scroll.
pub fn panel_size(rows: &[Row], limit: Vec2) -> Vec2 {
    let widest = rows
        .iter()
        .map(|row| row.members.len())
        .max()
        .unwrap_or(0)
        .max(1) as f32;
    let width = 2.0 * PADDING + widest * CARD.x + (widest - 1.0) * CARD_GAP;
    let count = rows.len().max(1) as f32;
    let height = 2.0 * PADDING + count * (HEADER + HEADER_GAP + CARD.y) + (count - 1.0) * ROW_GAP;
    vec2(width.min(limit.x), height.min(limit.y))
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

    /// What the strip may use, in logical points: 90 % of the width, and the
    /// height above the taskbar.
    fn limit(&self) -> Vec2 {
        let width = (self.work[2] - self.work[0]) as f32 / self.scale;
        let height = (self.work[3] - self.work[1]) as f32 / self.scale;
        vec2(width * 0.9, height * 0.85)
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
}

struct Shared {
    snapshot: Option<ArrangeSnapshot>,
    exe: Option<String>,
    to_host: Arc<HostChannel>,
    closed: bool,
    window: isize,
    monitor: Option<Monitor>,
    focused_once: bool,
    thumbs: HashMap<isize, Option<Thumbnail>>,
}

impl Shared {
    fn group(&self) -> Option<&ArrangeGroup> {
        let snapshot = self.snapshot.as_ref()?;
        let exe = self.exe.as_ref()?;
        snapshot.groups.iter().find(|group| &group.exe == exe)
    }

    fn panel(&self) -> Vec2 {
        let limit = self
            .monitor
            .map(|monitor| monitor.limit())
            .unwrap_or(vec2(1600.0, 900.0));
        match (self.group(), &self.snapshot) {
            (Some(group), Some(snapshot)) => panel_size(&rows(group, &snapshot.desktops), limit),
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
                exe: None,
                to_host,
                closed: false,
                window: 0,
                monitor: None,
                focused_once: false,
                thumbs: HashMap::new(),
            })),
            palette_hwnd,
        }
    }

    pub fn open(&self, snapshot: ArrangeSnapshot) {
        let Ok(mut shared) = self.shared.lock() else {
            return;
        };
        shared.exe = snapshot
            .groups
            .get(snapshot.focus)
            .map(|group| group.exe.clone());
        shared.snapshot = Some(snapshot);
        shared.closed = false;
        shared.window = 0;
        shared.focused_once = false;
        shared.monitor = Monitor::under_pointer();
    }

    /// Takes fresh data and keeps the same program on screen.
    pub fn update(&self, snapshot: ArrangeSnapshot) {
        let Ok(mut shared) = self.shared.lock() else {
            return;
        };
        let still_there = shared
            .exe
            .as_ref()
            .is_some_and(|exe| snapshot.groups.iter().any(|group| &group.exe == exe));
        if !still_there {
            shared.exe = snapshot
                .groups
                .get(snapshot.focus)
                .map(|group| group.exe.clone());
        }
        shared.snapshot = Some(snapshot);
    }

    pub fn was_closed(&self) -> bool {
        self.shared
            .lock()
            .map(|shared| shared.closed)
            .unwrap_or(false)
    }

    /// Unregisters every picture; runs on the UI thread, which owns them.
    pub fn hidden(&self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.closed = false;
            shared.window = 0;
            shared.thumbs.clear();
        }
    }

    pub fn show(&self, ctx: &egui::Context, icon: Option<Arc<egui::IconData>>) {
        let shared = Arc::clone(&self.shared);
        let palette_hwnd = self.palette_hwnd;
        let panel = self
            .shared
            .lock()
            .map(|shared| shared.panel())
            .unwrap_or(EMPTY_PANEL);
        let mut builder = egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_decorations(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_taskbar(false)
            .with_resizable(false)
            .with_active(true)
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
                    shared.to_host.send(HostRequest::FocusWindow(shared.window));
                }
            }
            let panel = shared.panel();
            if let (Some(monitor), true) = (shared.monitor, shared.window != 0) {
                keep_placed(shared.window as HWND, monitor.window_rect(panel));
            }

            let (escape, focused) = ui
                .ctx()
                .input(|i| (i.key_pressed(egui::Key::Escape), i.viewport().focused));
            if focused == Some(true) {
                shared.focused_once = true;
            }
            if escape || (shared.focused_once && focused == Some(false)) {
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

fn strip(ui: &mut Ui, shared: &mut Shared) {
    let tokens = Tokens::get(ui.ctx());
    let panel = ui.max_rect();
    ui.painter().rect_filled(panel, 0, tokens.elevated_bg);

    let content = panel.shrink(PADDING);
    let Some((group, desktops)) = shared
        .group()
        .cloned()
        .zip(shared.snapshot.as_ref().map(|s| s.desktops.clone()))
    else {
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
        return;
    };

    let rows = rows(&group, &desktops);
    shared
        .thumbs
        .retain(|hwnd, _| group.windows.iter().any(|window| window.hwnd == *hwnd));
    let ppp = ui.ctx().pixels_per_point();
    let mut activate: Option<isize> = None;

    ui.scope_builder(UiBuilder::new().max_rect(content), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("arrange-rows")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(CARD_GAP, 0.0);
                for (row_index, row) in rows.iter().enumerate() {
                    if row_index > 0 {
                        ui.add_space(ROW_GAP);
                    }
                    text::single(ui, text::section_job(&row.title, tokens.text_disabled));
                    ui.add_space(HEADER_GAP);
                    egui::ScrollArea::horizontal()
                        .id_salt(("arrange-row", &row.desktop))
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if row.members.is_empty() {
                                    ui.allocate_exact_size(CARD, Sense::hover());
                                    return;
                                }
                                for &member in &row.members {
                                    let window = &group.windows[member];
                                    let desktop_name = desktops
                                        .iter()
                                        .find(|d| d.id == window.desktop)
                                        .map(|d| d.name.as_str())
                                        .unwrap_or("");
                                    let destination = shared.window as HWND;
                                    let thumb =
                                        shared.thumbs.entry(window.hwnd).or_insert_with(|| {
                                            (!destination.is_null())
                                                .then(|| {
                                                    Thumbnail::register(
                                                        destination,
                                                        window.hwnd as HWND,
                                                    )
                                                })
                                                .flatten()
                                        });
                                    let response = card(
                                        ui,
                                        window.hwnd as HWND,
                                        &window.label,
                                        desktop_name,
                                        thumb.as_mut(),
                                        ppp,
                                    );
                                    if response.clicked() {
                                        activate = Some(window.hwnd);
                                    }
                                }
                            });
                        });
                }
            });
    });

    if let Some(hwnd) = activate {
        if let Some(snapshot) = &shared.snapshot {
            shared.to_host.send(HostRequest::Arrange {
                plugin: snapshot.plugin.clone(),
                action: ArrangeAction::Activate(hwnd),
            });
        }
        shared.closed = true;
    }
}

/// One window: its live picture in a bordered 200x120 area, letterboxed on
/// the panel colour, and its name underneath. Without a picture the area
/// shows the name and desktop instead.
fn card(
    ui: &mut Ui,
    source_window: HWND,
    label: &str,
    desktop: &str,
    thumb: Option<&mut Thumbnail>,
    pixels_per_point: f32,
) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(CARD, Sense::click());
    let hovered = response.hovered();
    let painter = ui.painter();
    if hovered {
        painter.rect_filled(rect, 6, tokens.hover_bg);
    }
    let area = Rect::from_min_size(rect.min + Vec2::splat(CARD_INSET), THUMB);
    let border = if hovered {
        tokens.accent
    } else {
        tokens.border
    };
    painter.rect(
        area,
        4,
        tokens.panel_bg,
        theme::stroke(ui.ctx(), 1.0, border),
        StrokeKind::Inside,
    );

    let picture = area.shrink(1.0);
    let source = thumb.as_ref().and_then(|thumb| thumb.source_size());
    match (thumb, source) {
        (Some(thumb), Some(size)) => {
            let frame = dwm_thumbs::visible_frame(source_window)
                .unwrap_or(Rect::from_min_size(pos2(0.0, 0.0), size));
            let dest = Rect::from_center_size(
                picture.center(),
                dwm_thumbs::fit(frame.size(), picture.size()),
            );
            let placement =
                dwm_thumbs::crop(dest, ui.clip_rect(), frame).map(|(shown, cropped)| Placement {
                    dest: dwm_thumbs::physical(shown, pixels_per_point),
                    source: dwm_thumbs::physical(cropped, 1.0),
                    opacity: 255,
                });
            thumb.place(placement);
        }
        (thumb, _) => {
            if let Some(thumb) = thumb {
                thumb.place(None);
            }
            name_card(ui, picture, label, desktop);
        }
    }

    let mut job = text::job(label, theme::lora(14.0), tokens.text_primary, None);
    job.wrap = TextWrapping {
        max_width: THUMB.x,
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        pos2(area.left(), area.bottom() + CARD_INSET),
        galley,
        tokens.text_primary,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Stands in for a picture DWM cannot give: the name, and the desktop.
fn name_card(ui: &Ui, area: Rect, label: &str, desktop: &str) {
    let tokens = Tokens::get(ui.ctx());
    let painter = ui.painter();
    let mut job = text::job(label, theme::cormorant(17.0), tokens.text_primary, None);
    job.wrap = TextWrapping {
        max_width: area.width() - 24.0,
        max_rows: 2,
        break_anywhere: false,
        overflow_character: Some('\u{2026}'),
    };
    job.halign = egui::Align::Center;
    let title = painter.layout_job(job);
    let detail = painter.layout_job(text::job(
        desktop,
        theme::lora(12.0),
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
    use crate::core::ui_bridge::ArrangeWindow;

    fn desktop(id: &str, name: &str, current: bool) -> ArrangeDesktop {
        ArrangeDesktop {
            id: id.to_string(),
            name: name.to_string(),
            current,
        }
    }

    fn group(desktops: &[&str]) -> ArrangeGroup {
        ArrangeGroup {
            exe: "chrome.exe".to_string(),
            label: "Chrome".to_string(),
            windows: desktops
                .iter()
                .enumerate()
                .map(|(i, d)| ArrangeWindow {
                    hwnd: i as isize,
                    label: format!("w{i}"),
                    desktop: d.to_string(),
                })
                .collect(),
        }
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
        let size = panel_size(&[row(3), row(1)], vec2(5000.0, 5000.0));
        assert_eq!(size.x, 2.0 * PADDING + 3.0 * CARD.x + 2.0 * CARD_GAP);
        assert_eq!(
            size.y,
            2.0 * PADDING + 2.0 * (HEADER + HEADER_GAP + CARD.y) + ROW_GAP
        );
        let capped = panel_size(&[row(30)], vec2(1000.0, 5000.0));
        assert_eq!(capped.x, 1000.0);
    }
}
