//! The equal-slot guess: where each thumbnail would be if the popup split
//! its inner area into equal slots, row by row. It is only a guess, and the
//! report compares it with what UI Automation says, when it says anything.

/// Left, top, right, bottom in physical screen pixels.
pub type Rect = [i32; 4];

/// Space between the popup's edge and its thumbnails at 96 DPI. A guess.
pub const PADDING_96: i32 = 8;
/// Width over height of one thumbnail slot, its title included. A guess.
pub const SLOT_ASPECT: f64 = 1.4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotGrid {
    pub inner: Rect,
    pub rows: usize,
    pub cols: usize,
    pub count: usize,
    pub padding: i32,
}

/// A length at 96 DPI in pixels at `dpi`; 0 counts as 96.
pub fn scale(value: i32, dpi: u32) -> i32 {
    let dpi = if dpi == 0 { 96 } else { i64::from(dpi) };
    ((i64::from(value) * dpi + 48) / 96) as i32
}

/// The grid of `count` slots in `rect` whose slots come closest to
/// [`SLOT_ASPECT`]; among equally close ones, the one with fewer rows. No
/// grid for no windows or a popup too small to hold the padding.
pub fn slot_grid(rect: Rect, count: usize, dpi: u32) -> Option<SlotGrid> {
    if count == 0 {
        return None;
    }
    let padding = scale(PADDING_96, dpi);
    let inner = [
        rect[0] + padding,
        rect[1] + padding,
        rect[2] - padding,
        rect[3] - padding,
    ];
    let (width, height) = (inner[2] - inner[0], inner[3] - inner[1]);
    if width <= 0 || height <= 0 {
        return None;
    }
    let mut best: Option<(f64, usize, usize)> = None;
    for rows in 1..=count {
        let cols = count.div_ceil(rows);
        if (rows - 1) * cols >= count {
            continue;
        }
        let aspect = (f64::from(width) / cols as f64) / (f64::from(height) / rows as f64);
        let score = (aspect / SLOT_ASPECT).ln().abs();
        if best.is_none_or(|(least, _, _)| score < least - 1e-9) {
            best = Some((score, rows, cols));
        }
    }
    let (_, rows, cols) = best?;
    Some(SlotGrid {
        inner,
        rows,
        cols,
        count,
        padding,
    })
}

impl SlotGrid {
    pub fn slot_rect(&self, index: usize) -> Option<Rect> {
        if index >= self.count {
            return None;
        }
        let (col, row) = ((index % self.cols) as i64, (index / self.cols) as i64);
        let (cols, rows) = (self.cols as i64, self.rows as i64);
        let width = i64::from(self.inner[2] - self.inner[0]);
        let height = i64::from(self.inner[3] - self.inner[1]);
        let x = |c: i64| self.inner[0] + (width * c / cols) as i32;
        let y = |r: i64| self.inner[1] + (height * r / rows) as i32;
        Some([x(col), y(row), x(col + 1), y(row + 1)])
    }

    /// The slot under a point, first slot = 0; none in the padding or in
    /// the empty end of the last row.
    pub fn slot_at(&self, x: i32, y: i32) -> Option<usize> {
        if !contains(self.inner, x, y) {
            return None;
        }
        let width = i64::from(self.inner[2] - self.inner[0]);
        let height = i64::from(self.inner[3] - self.inner[1]);
        let col = (i64::from(x - self.inner[0]) * self.cols as i64 / width) as usize;
        let row = (i64::from(y - self.inner[1]) * self.rows as i64 / height) as usize;
        let index = row * self.cols + col;
        (index < self.count).then_some(index)
    }
}

/// The right and bottom edges are outside, as in Windows.
pub fn contains(rect: Rect, x: i32, y: i32) -> bool {
    rect[0] <= x && x < rect[2] && rect[1] <= y && y < rect[3]
}

pub fn area(rect: Rect) -> i64 {
    i64::from((rect[2] - rect[0]).max(0)) * i64::from((rect[3] - rect[1]).max(0))
}

/// Overlap over union, 0 for rectangles that do not touch, 1 for the same.
pub fn overlap(a: Rect, b: Rect) -> f64 {
    let common = area([
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]);
    let union = area(a) + area(b) - common;
    if union <= 0 {
        0.0
    } else {
        common as f64 / union as f64
    }
}

/// Whether `inner` lies inside `outer`, edges included.
pub fn within(inner: Rect, outer: Rect) -> bool {
    outer[0] <= inner[0] && outer[1] <= inner[1] && inner[2] <= outer[2] && inner[3] <= outer[3]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_scale_with_dpi() {
        assert_eq!(scale(8, 96), 8);
        assert_eq!(scale(8, 144), 12);
        assert_eq!(scale(8, 120), 10);
        assert_eq!(scale(8, 0), 8);
        assert_eq!(scale(150, 168), 263);
    }

    #[test]
    fn a_wide_popup_is_one_row() {
        // Twelve thumbnails of about 200 x 140 side by side.
        let grid = slot_grid([0, 0, 2416, 156], 12, 96).unwrap();
        assert_eq!((grid.rows, grid.cols), (1, 12));
        assert_eq!(grid.inner, [8, 8, 2408, 148]);
        assert_eq!(grid.slot_rect(0), Some([8, 8, 208, 148]));
        assert_eq!(grid.slot_rect(11), Some([2208, 8, 2408, 148]));
        assert_eq!(grid.slot_rect(12), None);
    }

    #[test]
    fn a_square_popup_wraps_into_rows() {
        let grid = slot_grid([100, 100, 916, 676], 12, 96).unwrap();
        assert_eq!((grid.rows, grid.cols), (3, 4));
        assert_eq!(grid.slot_rect(4), Some([108, 294, 308, 481]));
    }

    #[test]
    fn padding_follows_the_monitor_dpi() {
        let grid = slot_grid([0, 0, 624, 228], 2, 144).unwrap();
        assert_eq!(grid.padding, 12);
        assert_eq!(grid.inner, [12, 12, 612, 216]);
        assert_eq!((grid.rows, grid.cols), (1, 2));
    }

    #[test]
    fn a_last_row_is_never_left_empty() {
        let grid = slot_grid([0, 0, 300, 2000], 3, 96).unwrap();
        assert_eq!((grid.rows, grid.cols), (3, 1));
        let grid = slot_grid([0, 0, 300, 2000], 5, 96).unwrap();
        assert!((grid.rows - 1) * grid.cols < 5);
    }

    #[test]
    fn no_grid_without_windows_or_room() {
        assert_eq!(slot_grid([0, 0, 500, 200], 0, 96), None);
        assert_eq!(slot_grid([0, 0, 16, 200], 3, 96), None);
        assert_eq!(slot_grid([0, 0, 500, 10], 3, 96), None);
    }

    #[test]
    fn the_slot_under_a_point_is_found() {
        let grid = slot_grid([0, 0, 2416, 156], 12, 96).unwrap();
        assert_eq!(grid.slot_at(8, 8), Some(0));
        assert_eq!(grid.slot_at(207, 100), Some(0));
        assert_eq!(grid.slot_at(208, 100), Some(1));
        assert_eq!(grid.slot_at(2407, 147), Some(11));
        assert_eq!(grid.slot_at(4, 100), None, "in the padding");
        assert_eq!(grid.slot_at(2408, 100), None);
    }

    #[test]
    fn the_empty_end_of_the_last_row_is_no_slot() {
        let grid = SlotGrid {
            inner: [0, 0, 300, 200],
            rows: 2,
            cols: 3,
            count: 5,
            padding: 0,
        };
        assert_eq!(grid.slot_at(150, 150), Some(4));
        assert_eq!(grid.slot_at(250, 150), None);
    }

    #[test]
    fn negative_monitors_work_like_any_other() {
        let grid = slot_grid([-1928, 900, -1112, 1100], 4, 96).unwrap();
        assert_eq!(grid.slot_at(-1900, 1000), Some(0));
        assert_eq!(grid.slot_at(-1130, 1000), Some(3));
    }

    #[test]
    fn overlap_is_zero_apart_and_one_for_the_same() {
        assert_eq!(overlap([0, 0, 10, 10], [0, 0, 10, 10]), 1.0);
        assert_eq!(overlap([0, 0, 10, 10], [20, 0, 30, 10]), 0.0);
        assert!((overlap([0, 0, 10, 10], [5, 0, 15, 10]) - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(overlap([0, 0, 0, 0], [0, 0, 0, 0]), 0.0);
    }

    #[test]
    fn containment_includes_the_left_edge_only() {
        assert!(contains([0, 0, 10, 10], 0, 0));
        assert!(!contains([0, 0, 10, 10], 10, 5));
        assert!(within([1, 1, 9, 9], [0, 0, 10, 10]));
        assert!(within([0, 0, 10, 10], [0, 0, 10, 10]));
        assert!(!within([0, 0, 11, 10], [0, 0, 10, 10]));
        assert_eq!(area([5, 5, 0, 0]), 0);
    }
}
