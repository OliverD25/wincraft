//! The short notice a plugin shows after a change: which plugin is
//! talking, what was done, and an Undo button when it can be undone. One
//! notice per plugin at a time; a new one takes the old one's place. It
//! sits above the taskbar, takes the mouse but never the keyboard, and its
//! clock stops while the mouse rests on it. QuickJump and QuickActions use
//! it; the plugin keeps the clock and a hidden window that hears the mouse.

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::UI::Controls::WM_MOUSELEAVE;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, GetWindowLongPtrW, PostMessageW, SetWindowLongPtrW, GWLP_USERDATA,
    MA_NOACTIVATE, WM_APP, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
};

use crate::core::canvas::{Area, Canvas, Fonts, Weight};
use crate::core::layered::Layered;
use crate::core::panel_look::{self as look, Cap};
use crate::core::theme::{self, Tokens};
use crate::ui::taskbar_badges;
use crate::ui::widgets::icons::Icon;

/// Posted to the plugin's window: the mouse came onto the notice (1) or
/// left it (0).
pub const WM_APP_NOTICE_HOVER: u32 = WM_APP + 41;
/// Posted to the plugin's window on a click, with the point in the
/// notice's own pixels in `lParam`.
pub const WM_APP_NOTICE_CLICK: u32 = WM_APP + 42;

/// Shown this long, or with Undo this long.
pub const PLAIN_MS: u64 = 4000;
pub const UNDO_MS: u64 = 8000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Pin,
    Info,
    Trash,
    Loader,
    Monitor,
    Settings,
    Check,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// The plugin's name, on the first line, so the user knows who talks.
    pub app: &'static str,
    pub glyph: Glyph,
    pub text: String,
    pub detail: Option<String>,
    pub undo: bool,
    /// The key that undoes it, drawn on the Undo button, when the plugin
    /// listens for one.
    pub undo_key: Option<&'static str>,
}

impl Message {
    pub fn new(app: &'static str, glyph: Glyph, text: String) -> Self {
        Self {
            app,
            glyph,
            text,
            detail: None,
            undo: false,
            undo_key: None,
        }
    }

    pub fn with_undo(mut self) -> Self {
        self.undo = true;
        self
    }

    /// With Undo, and `key` on its button.
    pub fn with_undo_key(mut self, key: &'static str) -> Self {
        self.undo = true;
        self.undo_key = Some(key);
        self
    }

    pub fn detail(mut self, line: String) -> Self {
        self.detail = Some(line);
        self
    }
}

/// When a notice is due to go: it shows `length` milliseconds, not
/// counting the time the mouse rests on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
    started: u64,
    length: u64,
    paused: u64,
    hover_since: Option<u64>,
}

impl Clock {
    pub fn new(now: u64, undo: bool) -> Self {
        Self {
            started: now,
            length: if undo { UNDO_MS } else { PLAIN_MS },
            paused: 0,
            hover_since: None,
        }
    }

    pub fn hover(&mut self, now: u64, on: bool) {
        match (on, self.hover_since) {
            (true, None) => self.hover_since = Some(now),
            (false, Some(since)) => {
                self.paused += now.saturating_sub(since);
                self.hover_since = None;
            }
            _ => {}
        }
    }

    pub fn expired(&self, now: u64) -> bool {
        self.hover_since.is_none() && self.shown(now) >= self.length
    }

    fn shown(&self, now: u64) -> u64 {
        let until = self.hover_since.unwrap_or(now);
        until
            .saturating_sub(self.started)
            .saturating_sub(self.paused)
    }
}

// Sizes in logical pixels, from the design.
const WIDTH: f32 = 500.0;
const MIN_HEIGHT: f32 = 56.0;
const PAD_X: f32 = 14.0;
const PAD_Y: f32 = 10.0;
const ICON_TILE: f32 = 28.0;
const GLYPH: f32 = 16.0;
const GAP: f32 = 12.0;
const BUTTON_H: f32 = 30.0;

const SMALL: usize = 0;
const SMALL_SEMI: usize = 1;
const BODY: usize = 2;

/// The picture, and where its Undo button is in its own pixels.
pub fn render(message: &Message, scale: f32, tokens: &Tokens) -> (Canvas, Option<Area>) {
    let px = |logical: f32| logical * scale;
    let fonts = Fonts::new(&[
        (Weight::Regular, px(12.0).round() as i32),
        (Weight::Semibold, px(12.0).round() as i32),
        (Weight::Regular, px(14.0).round() as i32),
    ]);
    let line = |index: usize| fonts.measure("Ag", index).1 as f32;
    let text_h = line(SMALL_SEMI)
        + line(BODY)
        + message
            .detail
            .as_ref()
            .map_or(0.0, |_| line(SMALL) + px(2.0));
    let inner_h = text_h.max(px(ICON_TILE)).max(px(BUTTON_H));
    let panel_h = (inner_h + 2.0 * px(PAD_Y)).max(px(MIN_HEIGHT));
    let margin = (look::SHADOW * scale).round();
    let mut canvas = Canvas::new(
        (px(WIDTH) + 2.0 * margin).round() as i32,
        (panel_h + 2.0 * margin).round() as i32,
    );
    let panel = look::panel(&mut canvas, tokens, scale);
    let middle = panel.y + panel.h / 2.0;

    let tile = Area::new(
        panel.x + px(PAD_X),
        middle - px(ICON_TILE) / 2.0,
        px(ICON_TILE),
        px(ICON_TILE),
    );
    canvas.outline(tile, px(6.0), look::border(tokens, scale), tokens.border);
    let (icon, colour) = match message.glyph {
        Glyph::Pin => (Icon::Pin, tokens.accent),
        Glyph::Info => (Icon::Info, tokens.text_secondary),
        Glyph::Trash => (Icon::Trash, tokens.text_secondary),
        Glyph::Loader => (Icon::Loader, tokens.accent),
        Glyph::Monitor => (Icon::Monitor, tokens.accent),
        Glyph::Settings => (Icon::Settings, tokens.accent),
        Glyph::Check => (Icon::Check, tokens.accent),
        Glyph::Warning => (Icon::TriangleAlert, tokens.text_secondary),
    };
    canvas.glyph(
        icon,
        tile.x + (tile.w - px(GLYPH)) / 2.0,
        tile.y + (tile.h - px(GLYPH)) / 2.0,
        px(GLYPH),
        colour,
    );

    let mut right = panel.right() - px(PAD_X);
    let mut button = None;
    if message.undo {
        let cap = Cap::new(tokens, SMALL, px(20.0), scale);
        let cap_w = message.undo_key.map_or(0.0, |key| {
            let (cap_w, _) = fonts.measure(key, SMALL);
            (cap_w as f32 + px(12.0)).max(px(20.0)).round() + px(8.0)
        });
        let (label_w, label_h) = fonts.measure("Undo", BODY);
        let tail = if message.undo_key.is_some() {
            px(6.0)
        } else {
            px(12.0)
        };
        let width = px(12.0) + label_w as f32 + cap_w + tail;
        let area = Area::new(
            right - width,
            middle - px(BUTTON_H) / 2.0,
            width,
            px(BUTTON_H),
        );
        canvas.outline(
            area,
            f32::from(tokens.radius_row) * scale,
            look::border(tokens, scale),
            tokens.accent,
        );
        let label = fonts.run(
            "Undo",
            BODY,
            tokens.accent,
            (area.x + px(12.0)).round() as i32,
            (middle - label_h as f32 / 2.0).round() as i32,
        );
        canvas.text(&[label], &fonts.list, 1.0);
        if let Some(key) = message.undo_key {
            look::keycap(
                &mut canvas,
                &fonts,
                key,
                (
                    area.x + px(12.0) + label_w as f32 + px(8.0),
                    middle - px(10.0),
                ),
                &cap,
            );
        }
        right = area.x - px(GAP);
        button = Some(area);
    }

    let left = tile.right() + px(GAP);
    let room = (right - left).max(px(40.0)).round() as i32;
    let mut y = middle - text_h / 2.0;
    let mut runs = vec![fonts.run(
        message.app,
        SMALL_SEMI,
        tokens.text_disabled,
        left as i32,
        y.round() as i32,
    )];
    y += line(SMALL_SEMI);
    let text = fonts.fit(&message.text, BODY, room);
    runs.push(fonts.run(
        &text,
        BODY,
        tokens.text_primary,
        left as i32,
        y.round() as i32,
    ));
    y += line(BODY) + px(2.0);
    if let Some(detail) = &message.detail {
        let detail = fonts.fit(detail, SMALL, room);
        runs.push(fonts.run(
            &detail,
            SMALL,
            tokens.text_secondary,
            left as i32,
            y.round() as i32,
        ));
    }
    canvas.text(&runs, &fonts.list, 1.0);
    (canvas, button)
}

/// The notice on screen; dropping it takes it away.
pub struct Notice {
    window: Layered,
    button: Option<Area>,
}

impl Notice {
    /// Shows `message` above the taskbar of the monitor in front; the
    /// mouse on it is reported to `target`. `class_name` is the plugin's
    /// own, so two plugins' notices never share a window class.
    pub fn show(message: &Message, class_name: &str, target: HWND) -> Option<Self> {
        let (work, scale) = taskbar_badges::work_area(taskbar_badges::front_monitor())?;
        let tokens = theme::current_tokens();
        let (canvas, button) = render(message, scale, &tokens);
        let lift = ((24.0 - look::SHADOW) * scale).round() as i32;
        let at = taskbar_badges::bottom_centre(work, (canvas.width, canvas.height), lift);
        let window = Layered::create_with(class_name, notice_proc)
            .map_err(|reason| log::warn!("the {} notice could not be made: {reason}", message.app))
            .ok()?;
        unsafe { SetWindowLongPtrW(window.hwnd(), GWLP_USERDATA, target as isize) };
        if !window.present(&canvas.into_image(), at) {
            return None;
        }
        window.show();
        Some(Self { window, button })
    }

    /// Whether a click at this point of the notice was on Undo.
    pub fn on_undo(&self, x: i32, y: i32) -> bool {
        self.button.is_some_and(|area| {
            let (x, y) = (x as f32, y as f32);
            x >= area.x && x < area.right() && y >= area.y && y < area.bottom()
        })
    }
}

impl Drop for Notice {
    fn drop(&mut self) {
        self.window.destroy();
    }
}

unsafe extern "system" fn notice_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let target = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as HWND;
    match msg {
        WM_MOUSEACTIVATE => MA_NOACTIVATE as LRESULT,
        WM_MOUSEMOVE => {
            let mut track = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            unsafe {
                TrackMouseEvent(&mut track);
                PostMessageW(target, WM_APP_NOTICE_HOVER, 1, 0);
            }
            0
        }
        WM_MOUSELEAVE => {
            unsafe { PostMessageW(target, WM_APP_NOTICE_HOVER, 0, 0) };
            0
        }
        WM_LBUTTONUP => {
            unsafe { PostMessageW(target, WM_APP_NOTICE_CLICK, 0, lparam) };
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// The point in a click's `lParam`.
pub fn click_point(lparam: LPARAM) -> POINT {
    POINT {
        x: (lparam & 0xFFFF) as i16 as i32,
        y: ((lparam >> 16) & 0xFFFF) as i16 as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::ThemeName;

    #[test]
    fn a_notice_goes_after_4_s_or_8_s_with_undo_and_waits_under_the_mouse() {
        let plain = Clock::new(1000, false);
        assert!(!plain.expired(4999));
        assert!(plain.expired(5000));
        let mut undo = Clock::new(0, true);
        assert!(!undo.expired(7999));
        undo.hover(2000, true);
        assert!(!undo.expired(60_000), "never while the mouse is on it");
        undo.hover(12_000, false);
        assert!(!undo.expired(17_999));
        assert!(undo.expired(18_000));
        undo.hover(18_000, false);
        assert!(undo.expired(18_000));
    }

    #[test]
    fn every_notice_draws_in_every_theme_with_its_button_inside() {
        let glyphs = [
            Glyph::Pin,
            Glyph::Info,
            Glyph::Trash,
            Glyph::Loader,
            Glyph::Monitor,
            Glyph::Settings,
            Glyph::Check,
            Glyph::Warning,
        ];
        for (name, dark) in ThemeName::ALL
            .into_iter()
            .zip([false, true, true, false, true])
        {
            let tokens = Tokens::of(name, dark);
            for glyph in glyphs {
                for message in [
                    Message::new("QuickJump", glyph, "Pinned Claude to 3".to_string())
                        .with_undo_key("Ctrl+Z"),
                    Message::new("QuickActions", glyph, "Dark mode on".to_string()).with_undo(),
                    Message::new("QuickJump", glyph, "Slot 5 is empty".to_string())
                        .detail("Open the pin list to pin a window.".to_string()),
                ] {
                    let (canvas, button) = render(&message, 1.5, &tokens);
                    assert_eq!(message.undo, button.is_some());
                    if let Some(area) = button {
                        assert!(area.right() <= canvas.width as f32);
                    }
                    assert!(canvas.height as f32 >= MIN_HEIGHT * 1.5);
                }
            }
        }
        let point = click_point((40 << 16) | 25);
        assert_eq!((point.x, point.y), (25, 40));
    }

    #[test]
    fn the_button_is_narrower_without_a_key() {
        let tokens = Tokens::of(ThemeName::ALL[0], false);
        let text = "Dark mode on".to_string();
        let with_key =
            Message::new("QuickActions", Glyph::Settings, text.clone()).with_undo_key("Ctrl+Z");
        let plain = Message::new("QuickActions", Glyph::Settings, text).with_undo();
        let (_, keyed) = render(&with_key, 1.0, &tokens);
        let (_, bare) = render(&plain, 1.0, &tokens);
        assert!(bare.expect("a button").w < keyed.expect("a button").w);
    }
}
