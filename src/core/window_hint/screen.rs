//! The hint on screen: one at a time per thread, on the thread that shows
//! it (the host's). A thread timer runs the fades, and only while a hint
//! lives (Principle 5).

use std::cell::{Cell, RefCell};
use std::time::Instant;

use windows_sys::Win32::Foundation::{HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetCursorPos, IsIconic, IsWindow, IsZoomed, KillTimer, SetTimer,
};

use crate::core::canvas::Area;
use crate::core::layered::{Image, Layered};
use crate::core::monitors::{self, Rect, Screen};
use crate::core::{animations, instance, theme};
use crate::ui::taskbar_badges::scale_at;

use super::draw;
use super::{
    another_monitor, centre, direction, is_desktop, looking_at, pill_parts, pill_spot, pill_text,
    ring, ring_box, strength, Before, Dir, Hint, Ring, EDGE_GAP, GLOW_HOLD_MS, PILL_HOLD_MS,
};

const GLOW_CLASS: &str = "WinCraftWindowHintGlow";
const PILL_CLASS: &str = "WinCraftWindowHintPill";
const TEST_CLASS: &str = "WinCraftWindowHintTest";
/// About 60 pictures a second while it fades.
const TICK_MS: u32 = 16;
/// A window still minimised this long after the jump gets no glow.
const GLOW_WAIT_MS: u64 = 600;
/// The scene's hint stays until the test instance quits.
const DEMO_HOLD_MS: u64 = 10 * 60 * 1000;
/// The scene's test window, in logical pixels.
const DEMO_SIZE: (f32, f32) = (900.0, 520.0);

/// What the glow goes round.
enum Around {
    /// A window: its frame is read once it is restored.
    Window { hwnd: isize, work: Rect },
    /// A box known already (the scene's own test window).
    Fixed(Ring),
}

/// The glow or the pill.
#[derive(Default)]
struct Part {
    window: Option<Layered>,
    /// When it began, in ms after the hint did; None until it shows.
    began: Option<u64>,
    hold: u64,
    over: bool,
}

impl Part {
    fn new(hold: u64) -> Self {
        Self {
            hold,
            ..Self::default()
        }
    }

    fn close(&mut self) {
        if let Some(window) = &mut self.window {
            window.destroy();
        }
        self.window = None;
        self.over = true;
    }

    /// Sets the strength for `ms`; closes the part when its time is over.
    fn fade(&mut self, ms: u64, animate: bool) {
        let Some(began) = self.began else {
            return;
        };
        match strength(ms.saturating_sub(began), self.hold, animate) {
            Some(alpha) => {
                if let Some(window) = &self.window {
                    window.set_alpha((alpha.clamp(0.0, 1.0) * 255.0).round() as u8);
                }
            }
            None => self.close(),
        }
    }
}

pub(super) struct Live {
    id: u64,
    started: Instant,
    animate: bool,
    around: Around,
    glow: Part,
    pill: Part,
    /// The pill's picture and spot until it is first shown.
    pill_picture: Option<(Image, POINT)>,
    /// The scene's test window.
    test_window: Option<Layered>,
    timer: usize,
}

thread_local! {
    static LIVE: RefCell<Option<Live>> = const { RefCell::new(None) };
    static NEXT_ID: Cell<u64> = const { Cell::new(1) };
}

impl Live {
    fn new(around: Around, glow_hold: u64, pill_hold: u64) -> Self {
        let id = NEXT_ID.with(|next| {
            let id = next.get();
            next.set(id + 1);
            id
        });
        Self {
            id,
            started: Instant::now(),
            animate: animations::on(),
            around,
            glow: Part::new(glow_hold),
            pill: Part::new(pill_hold),
            pill_picture: None,
            test_window: None,
            timer: 0,
        }
    }

    /// A hint with nothing to draw, for the tests.
    #[cfg(test)]
    pub(super) fn bare() -> Self {
        Self::new(
            Around::Fixed(Ring {
                frame: [0, 0, 1, 1],
                inside: false,
            }),
            GLOW_HOLD_MS,
            PILL_HOLD_MS,
        )
    }

    /// Draws what is due now; true when the hint is over.
    fn step(&mut self) -> bool {
        let ms = self.started.elapsed().as_millis() as u64;
        if self.glow.window.is_none() && !self.glow.over {
            match self.ring() {
                Some(ring) => {
                    self.glow.window = glow_window(ring);
                    self.glow.began = Some(ms);
                    if self.glow.window.is_none() {
                        self.glow.over = true;
                    }
                }
                None if ms >= GLOW_WAIT_MS => self.glow.over = true,
                None => {}
            }
        }
        if let Some((image, at)) = self.pill_picture.take() {
            self.pill.window = open(PILL_CLASS, &image, at);
            self.pill.began = Some(ms);
            if self.pill.window.is_none() {
                self.pill.over = true;
            }
        }
        self.glow.fade(ms, self.animate);
        self.pill.fade(ms, self.animate);
        self.glow.over && self.pill.over
    }

    /// The glow's box, once the window can be seen.
    fn ring(&self) -> Option<Ring> {
        match self.around {
            Around::Fixed(ring) => Some(ring),
            Around::Window { hwnd, work } => {
                let window = hwnd as HWND;
                if unsafe { IsWindow(window) } == 0 || unsafe { IsIconic(window) } != 0 {
                    return None;
                }
                let frame = frame_bounds(hwnd)?;
                Some(ring(frame, work, unsafe { IsZoomed(window) } != 0))
            }
        }
    }

    fn close(&mut self) {
        unsafe { KillTimer(std::ptr::null_mut(), self.timer) };
        self.glow.close();
        self.pill.close();
        if let Some(window) = &mut self.test_window {
            window.destroy();
        }
        self.test_window = None;
    }
}

/// Points to `hint.window` when it is on another monitor than the one the
/// user looks at. Returns whether it did. A hint still on screen goes
/// either way, since it pointed to another jump.
pub fn show(hint: &Hint) -> bool {
    hide();
    let screens = monitors::screens();
    let target = monitors::screen_of(hint.window as HWND, &screens);
    let closed = match hint.from {
        Before::Area(rect) => monitors::screen_at(rect, &screens),
        Before::Window(_) => None,
    };
    let front = match hint.from {
        Before::Window(hwnd) if hwnd != 0 && !is_desktop(&class_name(hwnd)) => {
            monitors::screen_of(hwnd as HWND, &screens)
        }
        _ => None,
    };
    let looking = looking_at(closed, front, pointer_screen(&screens));
    if !another_monitor(looking, target) {
        return false;
    }
    let (Some(from), Some(to)) = (
        looking.and_then(|index| screens.get(index)),
        target.and_then(|index| screens.get(index)),
    ) else {
        return false;
    };
    let dir = direction(from.rect, to.rect);
    let rects: Vec<monitors::Rect> = screens.iter().map(|screen| screen.rect).collect();
    let names = monitors::place_names(&rects);
    let word = target.and_then(|index| names.get(index).cloned().flatten());
    let words = pill_parts(&hint.name, word.as_deref());
    let visible = unsafe { IsIconic(hint.window as HWND) } == 0;
    let target_rect = frame_bounds(hint.window)
        .filter(|_| visible)
        .unwrap_or(to.rect);
    crate::detailed!(
        "host: window hint {:#x}: {}",
        hint.window,
        pill_text(&hint.name, word.as_deref(), dir)
    );
    let mut live = Live::new(
        Around::Window {
            hwnd: hint.window,
            work: to.work,
        },
        GLOW_HOLD_MS,
        PILL_HOLD_MS,
    );
    live.pill_picture = Some(pill_picture(&words, dir, dir, from.work, target_rect));
    begin(live);
    tick();
    true
}

/// The `--show-window-hint` scene: a test window of WinCraft's own in the
/// middle of the monitor with the mouse, the glow around it and a sample
/// pill at its left edge. No real window is read or touched. It stays until
/// the test instance quits.
pub fn show_demo() {
    hide();
    let screens = monitors::screens();
    let Some(screen) = pointer_screen(&screens)
        .or(Some(0))
        .and_then(|index| screens.get(index))
        .copied()
    else {
        return;
    };
    let s = scale_at(point(centre(screen.work)));
    let size = (
        (DEMO_SIZE.0 * s).round() as i32,
        (DEMO_SIZE.1 * s).round() as i32,
    );
    let (mid_x, mid_y) = centre(screen.work);
    let (left, top) = (mid_x - size.0 / 2, mid_y - size.1 / 2);
    let rect = [left, top, left + size.0, top + size.1];
    let tokens = theme::current_tokens();
    let mut live = Live::new(
        Around::Fixed(Ring {
            frame: rect,
            inside: false,
        }),
        DEMO_HOLD_MS,
        DEMO_HOLD_MS,
    );
    live.test_window = open(
        TEST_CLASS,
        &draw::test_window(size, &tokens, s),
        POINT { x: left, y: top },
    );
    if let Some(window) = &live.test_window {
        window.set_alpha(255);
    }
    let words = pill_parts("Telegram", Some("Right"));
    live.pill_picture = Some(pill_picture(&words, Dir::Right, Dir::Left, rect, rect));
    begin(live);
    tick();
}

/// Takes the hint away at once.
pub fn hide() {
    let live = LIVE.with(|cell| cell.borrow_mut().take());
    if let Some(mut live) = live {
        live.close();
    }
}

/// Puts `live` on screen in place of any other hint; returns its id.
pub(super) fn begin(mut live: Live) -> u64 {
    hide();
    live.timer = unsafe { SetTimer(std::ptr::null_mut(), 0, TICK_MS, Some(tick_proc)) };
    let id = live.id;
    LIVE.with(|cell| *cell.borrow_mut() = Some(live));
    id
}

/// The id of the hint on screen.
#[cfg(test)]
pub(super) fn current() -> Option<u64> {
    LIVE.with(|cell| cell.borrow().as_ref().map(|live| live.id))
}

unsafe extern "system" fn tick_proc(_hwnd: HWND, _msg: u32, _id: usize, _time: u32) {
    tick();
}

fn tick() {
    let done = LIVE.with(|cell| match cell.try_borrow_mut() {
        Ok(mut slot) => slot.as_mut().is_some_and(Live::step),
        Err(_) => false,
    });
    if done {
        hide();
    }
}

/// The pill's picture for the work area `work`, drawn with the arrow of
/// `arrow`, and where it goes: at the edge that faces `edge`, in line with
/// `target`.
fn pill_picture(
    words: &(String, String),
    arrow: Dir,
    edge: Dir,
    work: Rect,
    target: Rect,
) -> (Image, POINT) {
    let s = scale_at(point(centre(work)));
    let (image, panel) = draw::pill(&words.0, &words.1, arrow, &theme::current_tokens(), s);
    let size = (panel.w.round() as i32, panel.h.round() as i32);
    let gap = (EDGE_GAP * s).round() as i32;
    let (x, y) = pill_spot(work, edge, target, size, gap);
    let at = POINT {
        x: x - panel.x.round() as i32,
        y: y - panel.y.round() as i32,
    };
    (image, at)
}

fn glow_window(ring: Ring) -> Option<Layered> {
    let s = scale_at(point(centre(ring.frame)));
    let outer = ring_box(ring, draw::reach(s));
    let size = (outer[2] - outer[0], outer[3] - outer[1]);
    let frame = Area::new(
        (ring.frame[0] - outer[0]) as f32,
        (ring.frame[1] - outer[1]) as f32,
        (ring.frame[2] - ring.frame[0]) as f32,
        (ring.frame[3] - ring.frame[1]) as f32,
    );
    let image = draw::glow(size, frame, ring.inside, &theme::current_tokens(), s);
    open(
        GLOW_CLASS,
        &image,
        POINT {
            x: outer[0],
            y: outer[1],
        },
    )
}

/// A click-through panel showing `image` at `at`, not yet visible to the
/// eye: its strength starts at nothing.
fn open(class: &str, image: &Image, at: POINT) -> Option<Layered> {
    match Layered::create(&instance::name(class)) {
        Ok(mut window) => {
            if !window.present(image, at) {
                window.destroy();
                return None;
            }
            window.set_alpha(0);
            window.show();
            Some(window)
        }
        Err(reason) => {
            log::warn!("the window hint could not be made: {reason}");
            None
        }
    }
}

/// The window's visible frame, without the invisible resize border.
fn frame_bounds(hwnd: isize) -> Option<Rect> {
    let mut frame: RECT = unsafe { std::mem::zeroed() };
    let read = unsafe {
        DwmGetWindowAttribute(
            hwnd as HWND,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            (&mut frame as *mut RECT).cast(),
            std::mem::size_of::<RECT>() as u32,
        )
    } == 0;
    let rect = [frame.left, frame.top, frame.right, frame.bottom];
    (read && rect[2] > rect[0] && rect[3] > rect[1]).then_some(rect)
}

fn class_name(hwnd: isize) -> String {
    let mut buffer = [0u16; 64];
    let length = unsafe { GetClassNameW(hwnd as HWND, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..length.max(0) as usize])
}

fn pointer_screen(screens: &[Screen]) -> Option<usize> {
    let mut at = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut at) } == 0 {
        return None;
    }
    monitors::screen_at([at.x, at.y, at.x + 1, at.y + 1], screens)
}

fn point((x, y): (i32, i32)) -> POINT {
    POINT { x, y }
}
