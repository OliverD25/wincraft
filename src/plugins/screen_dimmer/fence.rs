//! Keeps the cursor off monitors that are off, where the user cannot see
//! it. The plugin's 100 ms timer moves a cursor that got onto one back to
//! the nearest point of a monitor that is on, and while the monitors that
//! are on fit in a rectangle that touches no monitor that is off, the
//! cursor is also confined to it so it cannot get there at all.

use windows_sys::Win32::Foundation::{POINT, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    ClipCursor, GetClipCursor, GetCursorPos, GetSystemMetrics, SetCursorPos, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

/// Left, top, right, bottom; right and bottom are outside, as in RECT.
pub type Rect = [i32; 4];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Zones {
    pub allowed: Vec<Rect>,
    pub blocked: Vec<Rect>,
}

pub struct Area {
    pub rect: Rect,
    pub off: bool,
    pub primary: bool,
}

/// The cursor may be on every monitor that is not off. When all of them are
/// off it may still be on the primary one, so it is never trapped.
pub fn zones(areas: &[Area]) -> Zones {
    let all_off = areas.iter().all(|area| area.off);
    let mut zones = Zones::default();
    for area in areas {
        if area.off && !(all_off && area.primary) {
            zones.blocked.push(area.rect);
        } else {
            zones.allowed.push(area.rect);
        }
    }
    zones
}

fn contains(rect: &Rect, (x, y): (i32, i32)) -> bool {
    x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
}

/// Where a cursor at `point` goes when it is on a monitor that is off: the
/// nearest point of a monitor that is on. None when it may stay.
pub fn push_out(point: (i32, i32), zones: &Zones) -> Option<(i32, i32)> {
    if !zones.blocked.iter().any(|rect| contains(rect, point)) {
        return None;
    }
    zones
        .allowed
        .iter()
        .filter(|rect| rect[2] > rect[0] && rect[3] > rect[1])
        .map(|rect| {
            let x = point.0.clamp(rect[0], rect[2] - 1);
            let y = point.1.clamp(rect[1], rect[3] - 1);
            let distance = i64::from(x - point.0).pow(2) + i64::from(y - point.1).pow(2);
            (distance, (x, y))
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, nearest)| nearest)
}

/// The box around the monitors that are on, for ClipCursor, when it
/// touches no monitor that is off. With the monitor in the middle off, the
/// box around the other two would cover it, so there is none.
pub fn clip(zones: &Zones) -> Option<Rect> {
    if zones.blocked.is_empty() {
        return None;
    }
    let first = zones.allowed.first()?;
    let bounds = zones.allowed.iter().fold(*first, |acc, rect| {
        [
            acc[0].min(rect[0]),
            acc[1].min(rect[1]),
            acc[2].max(rect[2]),
            acc[3].max(rect[3]),
        ]
    });
    let overlaps = |rect: &Rect| {
        rect[0] < bounds[2] && bounds[0] < rect[2] && rect[1] < bounds[3] && bounds[1] < rect[3]
    };
    (!zones.blocked.iter().any(overlaps)).then_some(bounds)
}

/// The live fence. It confines the cursor only while nobody else does:
/// a game that confines it to its own window keeps its rectangle.
#[derive(Default)]
pub struct Fence {
    clipped: Option<Rect>,
}

impl Fence {
    pub fn apply(&mut self, zones: &Zones) {
        if zones.blocked.is_empty() {
            self.release();
            return;
        }
        match clip(zones) {
            Some(wanted) => {
                let current = current_clip();
                if current == Some(wanted) {
                    self.clipped = Some(wanted);
                } else if current == Some(virtual_screen()) || current == self.clipped {
                    let rect = to_rect(wanted);
                    if unsafe { ClipCursor(&rect) } != 0 {
                        self.clipped = Some(wanted);
                    }
                } else {
                    self.clipped = None;
                }
            }
            None => self.release(),
        }
        let mut point = POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut point) } == 0 {
            return;
        }
        if let Some((x, y)) = push_out((point.x, point.y), zones) {
            unsafe { SetCursorPos(x, y) };
        }
    }

    /// Lets the cursor go, unless someone else has confined it since.
    pub fn release(&mut self) {
        if let Some(ours) = self.clipped.take() {
            if current_clip() == Some(ours) {
                unsafe { ClipCursor(std::ptr::null()) };
            }
        }
    }
}

fn to_rect(rect: Rect) -> RECT {
    RECT {
        left: rect[0],
        top: rect[1],
        right: rect[2],
        bottom: rect[3],
    }
}

fn current_clip() -> Option<Rect> {
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    (unsafe { GetClipCursor(&mut rect) } != 0).then_some([
        rect.left,
        rect.top,
        rect.right,
        rect.bottom,
    ])
}

/// What GetClipCursor returns while nothing confines the cursor.
fn virtual_screen() -> Rect {
    let (left, top) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
        )
    };
    let (width, height) = unsafe {
        (
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    [left, top, left + width, top + height]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The user's monitors: left 1920 x 1080, primary 3840 x 2160, right
    /// 2560 x 2880, all with their top edge at 0.
    const LEFT: Rect = [-1920, 0, 0, 1080];
    const PRIMARY: Rect = [0, 0, 3840, 2160];
    const RIGHT: Rect = [3840, 0, 6400, 2880];

    fn layout(off: [bool; 3]) -> Zones {
        zones(&[
            Area {
                rect: PRIMARY,
                off: off[1],
                primary: true,
            },
            Area {
                rect: LEFT,
                off: off[0],
                primary: false,
            },
            Area {
                rect: RIGHT,
                off: off[2],
                primary: false,
            },
        ])
    }

    #[test]
    fn nothing_off_means_no_fence() {
        let zones = layout([false, false, false]);
        assert!(zones.blocked.is_empty());
        assert_eq!(push_out((-500, 500), &zones), None);
        assert_eq!(clip(&zones), None);
    }

    #[test]
    fn the_left_monitor_off_sends_the_cursor_to_the_primary_edge() {
        let zones = layout([true, false, false]);
        assert_eq!(push_out((-500, 500), &zones), Some((0, 500)));
        assert_eq!(push_out((-1, 0), &zones), Some((0, 0)));
        assert_eq!(push_out((100, 100), &zones), None);
        // Primary and right fit in a box that does not touch the left one.
        assert_eq!(clip(&zones), Some([0, 0, 6400, 2880]));
    }

    #[test]
    fn the_right_monitor_off_sends_the_cursor_to_the_primary_edge() {
        let zones = layout([false, false, true]);
        assert_eq!(push_out((4000, 100), &zones), Some((3839, 100)));
        // Below the primary's bottom edge: the nearest point is its corner.
        assert_eq!(push_out((4000, 2500), &zones), Some((3839, 2159)));
        assert_eq!(clip(&zones), Some([-1920, 0, 3840, 2160]));
    }

    #[test]
    fn the_primary_off_sends_the_cursor_to_the_nearer_side() {
        let zones = layout([false, true, false]);
        assert_eq!(push_out((100, 500), &zones), Some((-1, 500)));
        assert_eq!(push_out((3800, 500), &zones), Some((3840, 500)));
        // Deep below the left monitor's bottom, the right one is nearer.
        assert_eq!(push_out((1000, 2000), &zones), Some((-1, 1079)));
        assert_eq!(push_out((3000, 2000), &zones), Some((3840, 2000)));
        // A box around left and right would cover the primary.
        assert_eq!(clip(&zones), None);
    }

    #[test]
    fn with_every_monitor_off_the_cursor_keeps_the_primary() {
        let zones = layout([true, true, true]);
        assert_eq!(zones.allowed, [PRIMARY]);
        assert_eq!(push_out((1000, 1000), &zones), None);
        assert_eq!(push_out((-100, 100), &zones), Some((0, 100)));
        assert_eq!(push_out((5000, 2500), &zones), Some((3839, 2159)));
        assert_eq!(clip(&zones), Some(PRIMARY));
    }

    #[test]
    fn two_off_leave_the_one_that_is_on() {
        let zones = layout([true, false, true]);
        assert_eq!(clip(&zones), Some(PRIMARY));
        assert_eq!(push_out((6000, 2800), &zones), Some((3839, 2159)));
    }
}
