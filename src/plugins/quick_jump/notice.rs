//! The short notice QuickJump shows after a change or a jump that could not
//! happen: what was done, and an Undo button when it can be undone. One
//! notice at a time; a new one takes the old one's place. It sits where the
//! strip sits, takes the mouse but never the keyboard, and the timer stops
//! while the mouse rests on it.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::UI::Controls::WM_MOUSELEAVE;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, PostMessageW, MA_NOACTIVATE, WM_APP, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE,
};

use crate::core::canvas::{Area, Canvas, Fonts, Weight};
use crate::core::layered::Layered;
use crate::core::theme::{self, Tokens};
use crate::ui::taskbar_badges;
use crate::ui::widgets::icons::Icon;

use super::look::{self, Cap};
use super::pins::{digit_of_slot, Pinned};

/// Posted to the plugin's window: the mouse came onto the notice (1) or
/// left it (0).
pub const WM_APP_NOTICE_HOVER: u32 = WM_APP + 41;
/// Posted to the plugin's window on a click, with the point in the
/// notice's own pixels in `lParam`.
pub const WM_APP_NOTICE_CLICK: u32 = WM_APP + 42;

const CLASS_NAME: &str = "WinCraftQuickJumpNotice";
/// Shown this long, or with Undo this long.
pub const PLAIN_MS: u64 = 4000;
pub const UNDO_MS: u64 = 8000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Pin,
    Info,
    Trash,
    Loader,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub glyph: Glyph,
    pub text: String,
    pub detail: Option<String>,
    pub undo: bool,
}

impl Message {
    fn new(glyph: Glyph, text: String) -> Self {
        Self {
            glyph,
            text,
            detail: None,
            undo: false,
        }
    }

    fn with_undo(mut self) -> Self {
        self.undo = true;
        self
    }

    fn detail(mut self, line: String) -> Self {
        self.detail = Some(line);
        self
    }
}

/// After a pin. A pin of the window into the slot it already has says so
/// without Undo, since nothing changed.
pub fn pinned(name: &str, slot: usize, done: &Pinned) -> Message {
    let digit = digit_of_slot(slot);
    match done {
        Pinned::New => Message::new(Glyph::Pin, format!("Pinned {name} to {digit}")).with_undo(),
        Pinned::Replaced(old) => Message::new(
            Glyph::Pin,
            format!("Pinned {name} to {digit} (was {})", old.name()),
        )
        .with_undo(),
        Pinned::Moved { from } => moved(name, *from, slot),
        Pinned::Same => Message::new(Glyph::Info, format!("{name} is already in {digit}")),
    }
}

pub fn moved(name: &str, from: usize, to: usize) -> Message {
    Message::new(
        Glyph::Pin,
        format!(
            "Moved {name} from {} to {}",
            digit_of_slot(from),
            digit_of_slot(to)
        ),
    )
    .with_undo()
}

pub fn removed(slot: usize) -> Message {
    Message::new(
        Glyph::Trash,
        format!("Removed slot {}", digit_of_slot(slot)),
    )
    .with_undo()
}

pub fn changed(name: &str, slot: usize) -> Message {
    Message::new(
        Glyph::Pin,
        format!("Changed how {} finds {name}", digit_of_slot(slot)),
    )
    .with_undo()
}

/// `pin_keys` is the pin shortcut as the user set it, or empty for none.
pub fn empty(slot: usize, pin_keys: &str) -> Message {
    let digit = digit_of_slot(slot);
    let message = Message::new(Glyph::Info, format!("Slot {digit} is empty"));
    if pin_keys.is_empty() {
        message.detail("Open the pin list to pin a window.".to_string())
    } else {
        message.detail(format!(
            "{pin_keys}, then {digit} pins the window in front."
        ))
    }
}

pub fn starting(name: &str, slot: usize) -> Message {
    Message::new(
        Glyph::Loader,
        format!("Starting {name} for slot {}\u{2026}", digit_of_slot(slot)),
    )
}

pub fn not_open(name: &str, slot: usize) -> Message {
    Message::new(
        Glyph::Info,
        format!("{name} in slot {} is not open", digit_of_slot(slot)),
    )
    .detail("Its pin is set not to start the app.".to_string())
}

pub fn not_started(name: &str) -> Message {
    Message::new(Glyph::Info, format!("{name} did not open a window"))
        .detail("QuickJump stopped waiting for it.".to_string())
}

pub fn nothing_to_pin() -> Message {
    Message::new(Glyph::Info, "There is no window to pin".to_string())
        .detail("Click the window you want, then try again.".to_string())
}

pub fn no_keys() -> Message {
    Message::new(
        Glyph::Info,
        "QuickJump cannot read the next key".to_string(),
    )
    .detail("Windows refused it, so the pin list opens instead.".to_string())
}

pub fn undone(what: &str) -> Message {
    Message::new(Glyph::Info, "Undone".to_string()).detail(what.to_string())
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
        let (cap_w, _) = fonts.measure("Ctrl+Z", SMALL);
        let cap_w = (cap_w as f32 + px(12.0)).max(px(20.0)).round();
        let (label_w, label_h) = fonts.measure("Undo", BODY);
        let width = px(12.0) + label_w as f32 + px(8.0) + cap_w + px(6.0);
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
        look::keycap(
            &mut canvas,
            &fonts,
            "Ctrl+Z",
            (
                area.x + px(12.0) + label_w as f32 + px(8.0),
                middle - px(10.0),
            ),
            &cap,
        );
        right = area.x - px(GAP);
        button = Some(area);
    }

    let left = tile.right() + px(GAP);
    let room = (right - left).max(px(40.0)).round() as i32;
    let mut y = middle - text_h / 2.0;
    let mut runs = vec![fonts.run(
        "QuickJump",
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

/// Where the plugin hears of the mouse on the notice.
static TARGET: AtomicIsize = AtomicIsize::new(0);

/// The notice on screen; dropping it takes it away.
pub struct Notice {
    window: Layered,
    button: Option<Area>,
}

impl Notice {
    /// Shows `message` where the strip shows; the mouse on it is reported
    /// to `target`.
    pub fn show(message: &Message, target: HWND) -> Option<Self> {
        TARGET.store(target as isize, Ordering::Relaxed);
        let (work, scale) = taskbar_badges::work_area(taskbar_badges::front_monitor())?;
        let tokens = theme::current_tokens();
        let (canvas, button) = render(message, scale, &tokens);
        let lift = ((24.0 - look::SHADOW) * scale).round() as i32;
        let at = taskbar_badges::bottom_centre(work, (canvas.width, canvas.height), lift);
        let window = Layered::create_with(CLASS_NAME, notice_proc)
            .map_err(|reason| log::warn!("the QuickJump notice could not be made: {reason}"))
            .ok()?;
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
    let target = TARGET.load(Ordering::Relaxed) as HWND;
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
    use super::super::pins::tests::pin;
    use super::*;
    use crate::core::config::ThemeName;

    #[test]
    fn the_messages_name_the_app_and_the_digits() {
        assert_eq!(pinned("Claude", 2, &Pinned::New).text, "Pinned Claude to 3");
        let was = pinned("Claude", 2, &Pinned::Replaced(pin("Chrome", None)));
        assert_eq!(was.text, "Pinned Claude to 3 (was Chrome)");
        assert!(was.undo);
        let moved = pinned("Claude", 2, &Pinned::Moved { from: 5 });
        assert_eq!(moved.text, "Moved Claude from 6 to 3");
        assert!(moved.undo);
        assert!(!pinned("Claude", 9, &Pinned::Same).undo);
        assert_eq!(
            pinned("Claude", 9, &Pinned::Same).text,
            "Claude is already in 0"
        );
        assert_eq!(removed(3).text, "Removed slot 4");
        let empty = empty(4, "Win+Alt+Shift+Q");
        assert_eq!(empty.text, "Slot 5 is empty");
        assert_eq!(
            empty.detail.as_deref(),
            Some("Win+Alt+Shift+Q, then 5 pins the window in front.")
        );
        assert_eq!(
            starting("Claude", 1).text,
            "Starting Claude for slot 2\u{2026}"
        );
        assert!(super::empty(4, "").detail.is_some());
        for message in [
            not_open("Code", 0),
            not_started("Code"),
            nothing_to_pin(),
            no_keys(),
            undone("Pinned Claude to 3"),
            changed("Code", 0),
        ] {
            assert!(!message.text.is_empty());
        }
    }

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
        for (name, dark) in ThemeName::ALL
            .into_iter()
            .zip([false, true, true, false, true])
        {
            let tokens = Tokens::of(name, dark);
            for message in [
                pinned("Claude", 2, &Pinned::New),
                empty(4, "Win+Alt+Shift+Q"),
            ] {
                let (canvas, button) = render(&message, 1.5, &tokens);
                assert_eq!(message.undo, button.is_some());
                if let Some(area) = button {
                    assert!(area.right() <= canvas.width as f32);
                }
                assert!(canvas.height as f32 >= MIN_HEIGHT * 1.5);
            }
        }
        let point = click_point((40 << 16) | 25);
        assert_eq!((point.x, point.y), (25, 40));
    }
}
