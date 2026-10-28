//! Where a window goes for Center and the Resize actions. Pure maths on
//! physical pixels, so it is the same on every monitor scale: a window
//! resized to 1280 × 720 is 1280 × 720 pixels on screen, as a screen
//! recording of it would be.
//!
//! A rectangle is `[left, top, right, bottom]`, as in `core::monitors`.
//! Windows 11 draws an invisible resize border around most windows, so the
//! rectangle Windows moves (the outer one) is a few pixels larger than the
//! one the user sees (the frame). The sizes here are for the frame; the
//! border is added back before the window is moved.

use crate::core::monitors::Rect;

/// What the action wants for the visible frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    /// Keep the size, move to the middle of the work area.
    Center,
    /// This many pixels wide and high, in the middle of the work area.
    Size(i32, i32),
    /// This share of the work area's width and height, in percent.
    Share(i32),
}

/// Where the outer rectangle goes, and whether the size had to shrink to
/// fit the work area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    pub outer: Rect,
    /// The visible frame's size after the move.
    pub size: (i32, i32),
    pub shrunk: bool,
}

pub fn width(rect: Rect) -> i32 {
    rect[2] - rect[0]
}

pub fn height(rect: Rect) -> i32 {
    rect[3] - rect[1]
}

/// The invisible border on each side: how far the outer rectangle reaches
/// past the frame. Never negative; an odd DWM answer counts as no border.
fn border(outer: Rect, frame: Rect) -> [i32; 4] {
    [
        (frame[0] - outer[0]).max(0),
        (frame[1] - outer[1]).max(0),
        (outer[2] - frame[2]).max(0),
        (outer[3] - frame[3]).max(0),
    ]
}

/// The new outer rectangle for `goal`. `outer` and `frame` are the
/// window's rectangles now (the frame as DWM reports it, or `outer` again
/// when it does not), and `work` is the work area of its monitor: the
/// screen without the taskbar.
pub fn place(goal: Goal, outer: Rect, frame: Rect, work: Rect) -> Placed {
    let room = (width(work).max(1), height(work).max(1));
    let wanted = match goal {
        Goal::Center => (width(frame), height(frame)),
        Goal::Size(w, h) => (w, h),
        Goal::Share(percent) => {
            let percent = percent.clamp(10, 100);
            (room.0 * percent / 100, room.1 * percent / 100)
        }
    };
    let size = (wanted.0.clamp(1, room.0), wanted.1.clamp(1, room.1));
    // Center keeps a window larger than the screen as it is, at the work
    // area's corner, rather than calling that a shrink.
    let shrunk = goal != Goal::Center && size != wanted;
    let size = if goal == Goal::Center {
        (wanted.0.max(1), wanted.1.max(1))
    } else {
        size
    };
    let left = work[0] + ((room.0 - size.0) / 2).max(0);
    let top = work[1] + ((room.1 - size.1) / 2).max(0);
    let edge = border(outer, frame);
    Placed {
        outer: [
            left - edge[0],
            top - edge[1],
            left + size.0 + edge[2],
            top + size.1 + edge[3],
        ],
        size,
        shrunk,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = [0, 0, 1920, 1032];

    #[test]
    fn a_size_lands_in_the_middle_of_the_work_area() {
        let outer = [100, 100, 900, 700];
        let placed = place(Goal::Size(1280, 720), outer, outer, WORK);
        assert_eq!(placed.outer, [320, 156, 1600, 876]);
        assert_eq!(placed.size, (1280, 720));
        assert!(!placed.shrunk);
    }

    #[test]
    fn the_invisible_border_is_added_back_so_the_frame_has_the_size() {
        // Windows 11 at 100 %: 7 px left, right and bottom, none on top.
        let outer = [93, 100, 907, 707];
        let frame = [100, 100, 900, 700];
        let placed = place(Goal::Size(1280, 720), outer, frame, WORK);
        assert_eq!(placed.outer, [313, 156, 1607, 883]);
        let frame_after = [
            placed.outer[0] + 7,
            placed.outer[1],
            placed.outer[2] - 7,
            placed.outer[3] - 7,
        ];
        assert_eq!(width(frame_after), 1280);
        assert_eq!(height(frame_after), 720);
    }

    #[test]
    fn the_border_is_wider_on_a_scaled_monitor_and_still_comes_off() {
        // 150 %: the border is 11 px; the frame still ends up 1280 x 720.
        let outer = [989, 0, 2011, 811];
        let frame = [1000, 0, 2000, 800];
        let work = [0, 0, 2560, 1380];
        let placed = place(Goal::Size(1280, 720), outer, frame, work);
        assert_eq!(width(placed.outer), 1280 + 22);
        assert_eq!(height(placed.outer), 720 + 11);
        assert_eq!(placed.outer[0] + 11, (2560 - 1280) / 2);
    }

    #[test]
    fn a_size_larger_than_the_work_area_shrinks_to_fit() {
        let outer = [0, 0, 800, 600];
        let placed = place(Goal::Size(1920, 1080), outer, outer, WORK);
        assert_eq!(placed.size, (1920, 1032));
        assert!(placed.shrunk);
        assert_eq!(placed.outer, [0, 0, 1920, 1032]);
    }

    #[test]
    fn the_work_area_of_a_second_monitor_is_used_as_it_is() {
        let work = [1920, -200, 4480, 1240];
        let outer = [2000, 0, 2400, 300];
        let placed = place(Goal::Size(1280, 720), outer, outer, work);
        assert_eq!(
            placed.outer,
            [1920 + 640, -200 + 360, 1920 + 1920, -200 + 1080]
        );
    }

    #[test]
    fn a_share_takes_that_part_of_the_work_area() {
        let outer = [0, 0, 400, 300];
        let placed = place(Goal::Share(80), outer, outer, WORK);
        assert_eq!(placed.size, (1536, 825));
        assert_eq!(placed.outer, [192, 103, 1728, 928]);
        assert!(!placed.shrunk);
        assert_eq!(
            place(Goal::Share(500), outer, outer, WORK).size,
            (1920, 1032)
        );
        assert_eq!(place(Goal::Share(0), outer, outer, WORK).size, (192, 103));
    }

    #[test]
    fn center_keeps_the_size_and_moves_the_frame_to_the_middle() {
        let outer = [3, 10, 1007, 517];
        let frame = [10, 10, 1000, 510];
        let placed = place(Goal::Center, outer, frame, WORK);
        assert_eq!(placed.size, (990, 500));
        assert_eq!(placed.outer, [465 - 7, 266, 1455 + 7, 766 + 7]);
        assert!(!placed.shrunk);
    }

    #[test]
    fn center_leaves_a_window_larger_than_the_screen_at_the_corner() {
        let outer = [-50, -50, 2500, 1500];
        let placed = place(Goal::Center, outer, outer, WORK);
        assert_eq!(placed.outer, [0, 0, 2550, 1550]);
        assert!(!placed.shrunk);
    }

    #[test]
    fn an_odd_frame_counts_as_no_border() {
        let outer = [100, 100, 900, 700];
        let frame = [90, 90, 910, 710];
        let placed = place(Goal::Size(1280, 720), outer, frame, WORK);
        assert_eq!(width(placed.outer), 1280);
        assert_eq!(height(placed.outer), 720);
    }
}
