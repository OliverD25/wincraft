//! The strip's peek: while the mouse rests on a card, the monitor that
//! window is on goes dark and the window's live picture is drawn exactly
//! where the window is, like the taskbar's Aero Peek but with documented
//! calls only. The window itself is never raised, restored or moved.
//!
//! Two click-through windows under the strip: a dark one over the monitor,
//! and above it one the size of the window that carries its DWM picture. A
//! layered window's opacity also fades the pictures drawn on it, so the
//! picture cannot sit on the dark window and stay fully opaque.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, GetStockObject, MonitorFromRect, BLACK_BRUSH, HBRUSH, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowPlacement, GetWindowRect, IsIconic,
    IsWindow, RegisterClassW, SetLayeredWindowAttributes, SetWindowPos, ShowWindow, LWA_ALPHA,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WINDOWPLACEMENT, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::core::wide;
use crate::ui::dwm_thumbs::{self, Placement, Thumbnail};

const CLASS_NAME: &str = "WinCraftPeek";
/// How long the mouse rests on a card before its window is shown.
pub const REST: Duration = Duration::from_millis(250);
/// About 55 % black.
const DIM_ALPHA: u8 = 140;

static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Where the picture window goes on screen and which part of the window's
/// picture it shows, both as [left, top, right, bottom] in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub screen: [i32; 4],
    pub source: [i32; 4],
}

/// `window` is the window's rectangle on screen (the restored one for a
/// minimized window), `frame` the part of it that is drawn, without the
/// invisible resize borders, and `source` the size DWM reports for its
/// picture. The picture covers exactly the drawn part.
pub fn layout(window: [i32; 4], frame: Option<[i32; 4]>, source: (i32, i32)) -> Option<Layout> {
    let (width, height) = (window[2] - window[0], window[3] - window[1]);
    if source.0 <= 0 || source.1 <= 0 || width <= 0 || height <= 0 {
        return None;
    }
    let inside = |f: &[i32; 4]| {
        f[0] >= window[0] && f[1] >= window[1] && f[2] <= window[2] && f[3] <= window[3]
    };
    let visible = frame
        .filter(|f| f[2] > f[0] && f[3] > f[1] && inside(f))
        .unwrap_or(window);
    // The picture can be smaller than the window, as for some minimized
    // windows, so the source is scaled rather than offset one to one.
    let scale_x = source.0 as f64 / width as f64;
    let scale_y = source.1 as f64 / height as f64;
    let x = |value: i32| ((value - window[0]) as f64 * scale_x).round() as i32;
    let y = |value: i32| ((value - window[1]) as f64 * scale_y).round() as i32;
    Some(Layout {
        screen: visible,
        source: [x(visible[0]), y(visible[1]), x(visible[2]), y(visible[3])],
    })
}

/// What the strip should do with the peek this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Show(isize),
    Hide,
    /// Leave the peek as it is and look again after this long.
    Wait(Duration),
}

/// Which card the mouse rests on, and since when.
#[derive(Default)]
pub struct Hover {
    card: Option<(isize, Instant)>,
    /// When the mouse went into the space between cards.
    between: Option<Instant>,
}

impl Hover {
    /// `card` is the card under the mouse, `in_rows` whether the mouse is
    /// still among the cards, and `blocked` whether a peek must not show at
    /// all now (a drag, a menu, the setting off).
    pub fn step(
        &mut self,
        now: Instant,
        card: Option<isize>,
        in_rows: bool,
        blocked: bool,
    ) -> Step {
        if blocked {
            *self = Self::default();
            return Step::Hide;
        }
        let Some(hwnd) = card else {
            self.card = None;
            if !in_rows {
                self.between = None;
                return Step::Hide;
            }
            // Crossing the gap to the next card keeps the old peek, so
            // moving along the cards does not flicker.
            let since = *self.between.get_or_insert(now);
            let waited = now.duration_since(since);
            return if waited >= REST {
                Step::Hide
            } else {
                Step::Wait(REST - waited)
            };
        };
        self.between = None;
        let since = match self.card {
            Some((current, since)) if current == hwnd => since,
            _ => {
                self.card = Some((hwnd, now));
                now
            }
        };
        let waited = now.duration_since(since);
        if waited >= REST {
            Step::Show(hwnd)
        } else {
            Step::Wait(REST - waited)
        }
    }
}

/// The two peek windows, made on first use on the UI thread and kept until
/// the strip goes away. Handles are kept as numbers so the strip's shared
/// state stays sendable.
#[derive(Default)]
pub struct Peek {
    dim: isize,
    picture: isize,
    thumb: Option<Thumbnail>,
    target: Option<isize>,
}

impl Peek {
    /// Shows `target`'s window just below `strip`. Hides the peek instead
    /// when the window is gone or DWM has no picture of it.
    pub fn show(&mut self, target: isize, strip: HWND) {
        if unsafe { IsWindow(target as HWND) } == 0 {
            self.hide();
            return;
        }
        if self.target == Some(target) {
            return;
        }
        if !self.ensure_windows() {
            return;
        }
        let window = target as HWND;
        let Some(thumb) = Thumbnail::register(self.picture as HWND, window) else {
            self.hide();
            return;
        };
        let Some(size) = thumb.source_size() else {
            self.hide();
            return;
        };
        let (rect, frame) = placed_rect(window);
        let Some(layout) = layout(rect, frame, (size.x as i32, size.y as i32)) else {
            self.hide();
            return;
        };
        let monitor = monitor_rect(rect);
        let [left, top, right, bottom] = layout.screen;
        let mut thumb = thumb;
        unsafe {
            SetWindowPos(
                self.picture as HWND,
                strip,
                left,
                top,
                right - left,
                bottom - top,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        thumb.place(Some(Placement {
            dest: [0, 0, right - left, bottom - top],
            source: layout.source,
            opacity: 255,
        }));
        unsafe {
            SetWindowPos(
                self.dim as HWND,
                self.picture as HWND,
                monitor[0],
                monitor[1],
                monitor[2] - monitor[0],
                monitor[3] - monitor[1],
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        // Replaced only now, so the old picture stays until the new one is up.
        self.thumb = Some(thumb);
        self.target = Some(target);
    }

    pub fn hide(&mut self) {
        self.thumb = None;
        self.target = None;
        for hwnd in [self.picture, self.dim] {
            if hwnd != 0 {
                unsafe { ShowWindow(hwnd as HWND, SW_HIDE) };
            }
        }
    }

    pub fn is_shown(&self) -> bool {
        self.target.is_some()
    }

    fn ensure_windows(&mut self) -> bool {
        if self.dim == 0 {
            self.dim = create(DIM_ALPHA);
        }
        if self.picture == 0 {
            self.picture = create(255);
        }
        self.dim != 0 && self.picture != 0
    }
}

impl Drop for Peek {
    fn drop(&mut self) {
        self.thumb = None;
        for hwnd in [self.picture, self.dim] {
            if hwnd != 0 {
                unsafe { DestroyWindow(hwnd as HWND) };
            }
        }
    }
}

/// The window's rectangle on screen and its drawn part. A minimized window
/// is placed where it will come back to; its drawn part is not known then.
fn placed_rect(window: HWND) -> ([i32; 4], Option<[i32; 4]>) {
    if unsafe { IsIconic(window) } != 0 {
        let mut place: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
        place.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
        if unsafe { GetWindowPlacement(window, &mut place) } != 0 {
            let r = place.rcNormalPosition;
            return ([r.left, r.top, r.right, r.bottom], None);
        }
    }
    let mut r: RECT = unsafe { std::mem::zeroed() };
    unsafe { GetWindowRect(window, &mut r) };
    let rect = [r.left, r.top, r.right, r.bottom];
    let frame = dwm_thumbs::visible_frame(window).map(|inner| {
        [
            r.left + inner.min.x.round() as i32,
            r.top + inner.min.y.round() as i32,
            r.left + inner.max.x.round() as i32,
            r.top + inner.max.y.round() as i32,
        ]
    });
    (rect, frame)
}

fn monitor_rect(rect: [i32; 4]) -> [i32; 4] {
    let area = RECT {
        left: rect[0],
        top: rect[1],
        right: rect[2],
        bottom: rect[3],
    };
    let monitor = unsafe { MonitorFromRect(&area, MONITOR_DEFAULTTONEAREST) };
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return rect;
    }
    let m = info.rcMonitor;
    [m.left, m.top, m.right, m.bottom]
}

fn create(alpha: u8) -> isize {
    if !ensure_class() {
        return 0;
    }
    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let hwnd = unsafe {
        CreateWindowExW(
            // Layered and Transparent let clicks through; NoActivate and
            // ToolWindow keep the peek out of the focus chain and the taskbar.
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            wide(CLASS_NAME).as_ptr(),
            wide("").as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        log::warn!("could not create a peek window");
        return 0;
    }
    unsafe { SetLayeredWindowAttributes(hwnd, 0, alpha, LWA_ALPHA) };
    hwnd as isize
}

fn ensure_class() -> bool {
    if CLASS_REGISTERED.load(Ordering::Relaxed) {
        return true;
    }
    let name = wide(CLASS_NAME);
    let mut class: WNDCLASSW = unsafe { std::mem::zeroed() };
    class.lpfnWndProc = Some(DefWindowProcW);
    class.hInstance = unsafe { GetModuleHandleW(std::ptr::null()) };
    class.lpszClassName = name.as_ptr();
    class.hbrBackground = unsafe { GetStockObject(BLACK_BRUSH) } as HBRUSH;
    if unsafe { RegisterClassW(&class) } == 0 {
        log::error!("could not register the peek window class");
        return false;
    }
    CLASS_REGISTERED.store(true, Ordering::Relaxed);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    // This PC, in physical pixels: left (-1920,0)-(0,1080) at 100 %, primary
    // (0,0)-(3840,2160) at 150 % with its work area ending at 2088, right
    // (3840,0)-(6400,2880) at 175 %.
    // Windows 11 windows carry 7 px invisible borders left, right and below.

    #[test]
    fn a_window_on_the_left_monitor_keeps_its_negative_position() {
        let window = [-1807, 101, -807, 908];
        let frame = [-1800, 101, -814, 901];
        let got = layout(window, Some(frame), (1000, 807)).unwrap();
        assert_eq!(got.screen, frame);
        assert_eq!(got.source, [7, 0, 993, 800]);
    }

    #[test]
    fn a_maximized_window_on_the_primary_fills_the_monitor() {
        let window = [-11, -11, 3851, 2099];
        let frame = [0, 0, 3840, 2088];
        let got = layout(window, Some(frame), (3862, 2110)).unwrap();
        assert_eq!(got.screen, frame);
        assert_eq!(got.source, [11, 11, 3851, 2099]);
    }

    #[test]
    fn a_window_on_the_right_monitor_without_a_frame_uses_its_whole_rect() {
        let window = [4000, 300, 5200, 1100];
        let got = layout(window, None, (1200, 800)).unwrap();
        assert_eq!(got.screen, window);
        assert_eq!(got.source, [0, 0, 1200, 800]);
    }

    #[test]
    fn a_frame_outside_the_window_is_ignored() {
        let window = [4000, 300, 5200, 1100];
        let got = layout(window, Some([3990, 300, 5200, 1100]), (1200, 800)).unwrap();
        assert_eq!(got.screen, window);
    }

    #[test]
    fn a_smaller_picture_is_scaled_to_the_window() {
        let window = [-1807, 101, -807, 908];
        let frame = [-1800, 101, -814, 901];
        let got = layout(window, Some(frame), (500, 403)).unwrap();
        assert_eq!(got.screen, frame);
        assert_eq!(got.source, [4, 0, 497, 400]);
    }

    #[test]
    fn no_picture_means_no_peek() {
        assert_eq!(layout([0, 0, 100, 100], None, (0, 0)), None);
        assert_eq!(layout([0, 0, 0, 100], None, (100, 100)), None);
    }

    #[test]
    fn the_peek_waits_for_the_mouse_to_rest() {
        let start = Instant::now();
        let mut hover = Hover::default();
        assert_eq!(hover.step(start, Some(1), true, false), Step::Wait(REST));
        let later = start + Duration::from_millis(100);
        assert_eq!(
            hover.step(later, Some(1), true, false),
            Step::Wait(Duration::from_millis(150))
        );
        assert_eq!(
            hover.step(start + REST, Some(1), true, false),
            Step::Show(1)
        );
    }

    #[test]
    fn moving_to_another_card_keeps_the_old_peek_until_the_new_one_rests() {
        let start = Instant::now();
        let mut hover = Hover::default();
        hover.step(start, Some(1), true, false);
        assert_eq!(
            hover.step(start + REST, Some(1), true, false),
            Step::Show(1)
        );
        let gap = start + REST + Duration::from_millis(20);
        assert!(matches!(hover.step(gap, None, true, false), Step::Wait(_)));
        let next = gap + Duration::from_millis(20);
        assert!(matches!(
            hover.step(next, Some(2), true, false),
            Step::Wait(_)
        ));
        assert_eq!(hover.step(next + REST, Some(2), true, false), Step::Show(2));
    }

    #[test]
    fn the_peek_hides_at_once_when_the_mouse_leaves_or_is_blocked() {
        let start = Instant::now();
        let mut hover = Hover::default();
        hover.step(start, Some(1), true, false);
        assert_eq!(hover.step(start + REST, None, false, false), Step::Hide);
        hover.step(start, Some(1), true, false);
        assert_eq!(hover.step(start + REST, Some(1), true, true), Step::Hide);
        // A block starts the rest over.
        assert!(matches!(
            hover.step(start + REST, Some(1), true, false),
            Step::Wait(_)
        ));
    }

    #[test]
    fn resting_between_cards_hides_the_peek_after_a_while() {
        let start = Instant::now();
        let mut hover = Hover::default();
        assert!(matches!(
            hover.step(start, None, true, false),
            Step::Wait(_)
        ));
        assert_eq!(hover.step(start + REST, None, true, false), Step::Hide);
    }
}
