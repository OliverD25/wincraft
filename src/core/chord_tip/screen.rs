//! The chord tip on screen: one at a time per thread, made and removed on
//! the thread that waits for the chord (the host's). A thread timer moves
//! the dots and the bar and runs the closing states; it runs only while a
//! tip lives (Principle 5).

use std::cell::RefCell;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONEAREST};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, KillTimer, SetTimer};

use crate::core::layered::Layered;
use crate::core::panel_look as look;
use crate::core::{instance, theme};
use crate::ui::taskbar_badges;

use super::draw::{self, Picture};
use super::{Content, End, Frame, Timeline};

const CLASS_NAME: &str = "WinCraftChordTip";
/// About 30 pictures a second while the dots move.
const TICK_MS: u32 = 33;
/// Between the tip and the top of the taskbar.
const LIFT: f32 = 24.0;

struct Tip {
    owner: isize,
    content: Content,
    timeline: Timeline,
    started: Instant,
    work: [i32; 4],
    scale: f32,
    window: Option<Layered>,
    /// The picture drawn, and whether it is the closing one.
    picture: Option<(Picture, bool, POINT)>,
    timer: usize,
}

thread_local! {
    static TIP: RefCell<Option<Tip>> = const { RefCell::new(None) };
}

/// The monitor with the mouse pointer: its work area and scale.
fn pointer_monitor() -> Option<([i32; 4], f32)> {
    let mut point = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut point) };
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    taskbar_badges::work_area(monitor)
}

/// Starts the tip for `owner`'s chord, which waits `wait` for its next
/// key. It shows after `SHOW_AFTER_MS` unless the chord ends first. A tip
/// of another chord goes.
pub fn show(owner: isize, content: Content, wait: Duration) {
    take_down();
    let Some((work, scale)) = pointer_monitor() else {
        return;
    };
    let timer = unsafe { SetTimer(std::ptr::null_mut(), 0, TICK_MS, Some(tick_proc)) };
    let wait_ms = wait.as_millis().min(u64::MAX as u128) as u64;
    TIP.with(|cell| {
        *cell.borrow_mut() = Some(Tip {
            owner,
            content,
            timeline: Timeline::new(wait_ms),
            started: Instant::now(),
            work,
            scale,
            window: None,
            picture: None,
            timer,
        })
    });
}

/// `owner`'s chord ended this way; the tip says so, then fades. Nothing
/// happens when another chord's tip is up, or none.
pub fn end(owner: isize, end: End) {
    let mine = TIP.with(|cell| {
        let mut slot = cell.borrow_mut();
        match slot.as_mut().filter(|tip| tip.owner == owner) {
            Some(tip) => {
                let at = tip.started.elapsed().as_millis() as u64;
                tip.timeline.end(at, end);
                true
            }
            None => false,
        }
    });
    if mine {
        tick();
    }
}

/// Takes `owner`'s tip away at once, as when a notice takes its place.
pub fn hide(owner: isize) {
    let mine = TIP.with(|cell| cell.borrow().as_ref().is_some_and(|tip| tip.owner == owner));
    if mine {
        take_down();
    }
}

fn take_down() {
    let tip = TIP.with(|cell| cell.borrow_mut().take());
    if let Some(mut tip) = tip {
        unsafe { KillTimer(std::ptr::null_mut(), tip.timer) };
        if let Some(window) = &mut tip.window {
            window.destroy();
        }
    }
}

unsafe extern "system" fn tick_proc(_hwnd: HWND, _msg: u32, _id: usize, _time: u32) {
    tick();
}

fn tick() {
    let done = TIP.with(|cell| match cell.try_borrow_mut() {
        Ok(mut slot) => slot.as_mut().is_some_and(Tip::step),
        Err(_) => false,
    });
    if done {
        take_down();
    }
}

impl Tip {
    /// Draws what is due now; true when the tip is over.
    fn step(&mut self) -> bool {
        let ms = self.started.elapsed().as_millis() as u64;
        match self.timeline.frame(ms) {
            Frame::Hidden => false,
            Frame::Done => true,
            Frame::Waiting { left } => {
                let tokens = theme::current_tokens();
                if self.picture.is_none() {
                    let picture = draw::render(&self.content, None, &tokens, self.scale);
                    let at = self.place(&picture);
                    self.picture = Some((picture, false, at));
                }
                if let Some((picture, _, at)) = &self.picture {
                    let image = draw::frame(picture, ms, left, &tokens);
                    self.present(&image, *at);
                }
                false
            }
            Frame::Closing { alpha } => {
                if !self
                    .picture
                    .as_ref()
                    .is_some_and(|(_, closing, _)| *closing)
                {
                    let tokens = theme::current_tokens();
                    let picture =
                        draw::render(&self.content, self.timeline.ending(), &tokens, self.scale);
                    let at = self.place(&picture);
                    let image = picture.base.clone().into_image();
                    self.present(&image, at);
                    self.picture = Some((picture, true, at));
                }
                if let Some(window) = &self.window {
                    window.set_alpha((alpha.clamp(0.0, 1.0) * 255.0).round() as u8);
                }
                false
            }
        }
    }

    fn place(&self, picture: &Picture) -> POINT {
        let lift = ((LIFT - look::SHADOW) * self.scale).round() as i32;
        taskbar_badges::bottom_centre(self.work, (picture.base.width, picture.base.height), lift)
    }

    fn present(&mut self, image: &crate::core::layered::Image, at: POINT) {
        if self.window.is_none() {
            match Layered::create(&instance::name(CLASS_NAME)) {
                Ok(window) => {
                    if window.present(image, at) {
                        window.show();
                    }
                    self.window = Some(window);
                }
                Err(reason) => log::warn!("the chord tip could not be made: {reason}"),
            }
            return;
        }
        if let Some(window) = &self.window {
            window.present(image, at);
        }
    }
}
