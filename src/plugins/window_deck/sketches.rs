//! The pictures under "Open a group as": what each option looks like when
//! a group opens. Both show the strip's row faintly at the bottom with the
//! group's stack in the middle, and the group's layer over it, as
//! `ui::arrange::stacks::overlay::place` lays it out.

use crate::core::traits::{Sketch, SketchLook, SketchShape};

const fn shape(x: f32, y: f32, w: f32, h: f32, look: SketchLook) -> SketchShape {
    SketchShape {
        rect: [x, y, w, h],
        look,
    }
}

use SketchLook::{Faint, Panel, Selected, Text, Window};

/// The row of the strip; the third card is the stack, with a second card
/// behind it.
macro_rules! strip_row {
    () => {
        [
            shape(7.0, 44.0, 18.0, 12.0, Faint),
            shape(29.0, 44.0, 18.0, 12.0, Faint),
            shape(54.0, 41.0, 18.0, 12.0, Faint),
            shape(51.0, 44.0, 18.0, 12.0, Faint),
            shape(73.0, 44.0, 18.0, 12.0, Faint),
            shape(95.0, 44.0, 18.0, 12.0, Faint),
        ]
    };
}

/// "A row centred on the stack": the windows side by side, the middle of
/// the row over the middle of the stack.
const ROW: &[SketchShape] = &{
    let row = strip_row!();
    [
        row[0],
        row[1],
        row[2],
        row[3],
        row[4],
        row[5],
        shape(6.0, 6.0, 108.0, 42.0, Panel),
        shape(11.0, 10.0, 44.0, 3.0, Text),
        shape(11.0, 17.0, 30.0, 26.0, Window),
        shape(45.0, 17.0, 30.0, 26.0, Window),
        shape(79.0, 17.0, 30.0, 26.0, Window),
    ]
};

/// "A fan": the windows overlap from the stack's left edge; the selected
/// one is whole in front, and the nearer a window is to it, the higher.
const FAN: &[SketchShape] = &{
    let row = strip_row!();
    [
        row[0],
        row[1],
        row[2],
        row[3],
        row[4],
        row[5],
        shape(45.0, 6.0, 73.0, 42.0, Panel),
        shape(49.0, 10.0, 36.0, 3.0, Text),
        shape(82.0, 17.0, 32.0, 26.0, Window),
        shape(49.0, 17.0, 32.0, 26.0, Window),
        shape(71.0, 17.0, 32.0, 26.0, Window),
        shape(60.0, 17.0, 32.0, 26.0, Selected),
    ]
};

/// One per option of "Open a group as", in its order.
pub const GROUP_OPEN: &[Sketch] = &[Sketch { shapes: ROW }, Sketch { shapes: FAN }];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_fits_the_box() {
        for sketch in GROUP_OPEN {
            for shape in sketch.shapes {
                let [x, y, w, h] = shape.rect;
                assert!(
                    x >= 0.0 && y >= 0.0 && x + w <= 120.0 && y + h <= 60.0,
                    "{shape:?}"
                );
            }
        }
    }

    #[test]
    fn the_fan_is_drawn_in_the_strips_order_with_the_selected_window_last() {
        let windows: Vec<f32> = FAN
            .iter()
            .filter(|shape| matches!(shape.look, Window | Selected))
            .map(|shape| shape.rect[0])
            .collect();
        let mut by_place = windows.clone();
        by_place.sort_by(f32::total_cmp);
        // The strip draws a fan back to front: the farther a window is from
        // the selected one, the earlier.
        let front = by_place[1];
        let distance = |x: f32| {
            by_place
                .iter()
                .filter(|other| (**other - front).abs() < (x - front).abs())
                .count()
        };
        let steps: Vec<usize> = windows.iter().map(|x| distance(*x)).collect();
        assert!(steps.windows(2).all(|pair| pair[0] >= pair[1]), "{steps:?}");
        assert_eq!(FAN.last().map(|shape| shape.look), Some(Selected));
    }

    #[test]
    fn the_row_is_centred_on_the_stack() {
        let windows: Vec<[f32; 4]> = ROW
            .iter()
            .filter(|shape| shape.look == Window)
            .map(|shape| shape.rect)
            .collect();
        let left = windows[0][0];
        let right = windows[windows.len() - 1][0] + windows[windows.len() - 1][2];
        let stack = ROW[3].rect;
        assert_eq!((left + right) / 2.0, stack[0] + stack[2] / 2.0);
    }
}
