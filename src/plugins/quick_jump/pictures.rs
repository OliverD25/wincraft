//! Live pictures of windows in Quick access and the picker, drawn by DWM
//! on top of the egui window, as the Arrange strip's are. They belong to
//! the UI thread, which owns the window they are drawn on.

use std::collections::{HashMap, HashSet};

use egui::{Rect, Vec2};
use windows_sys::Win32::Foundation::HWND;

use crate::ui::dwm_thumbs::{self, Placement, Thumbnail};

#[derive(Default)]
pub struct Pictures {
    window: isize,
    thumbs: HashMap<isize, Option<Thumbnail>>,
    used: HashSet<isize>,
}

impl Pictures {
    /// A new frame drawn on `window`; a picture not asked for again by
    /// `end` is hidden.
    pub fn begin(&mut self, window: isize) {
        if window != self.window {
            self.thumbs.clear();
            self.window = window;
        }
        self.used.clear();
    }

    /// Shows the window's picture fitted into `area` (logical points), and
    /// says whether DWM has one to show.
    pub fn show(&mut self, hwnd: isize, area: Rect, pixels_per_point: f32) -> bool {
        self.show_scaled(hwnd, area, pixels_per_point, None)
            .is_some()
    }

    /// The size of the window's visible picture in pixels, or None when DWM
    /// has none to show.
    pub fn source_size(&mut self, hwnd: isize) -> Option<Vec2> {
        self.source(hwnd).map(|source| source.size())
    }

    fn source(&mut self, hwnd: isize) -> Option<Rect> {
        if self.window == 0 {
            return None;
        }
        let destination = self.window as HWND;
        let thumb = self
            .thumbs
            .entry(hwnd)
            .or_insert_with(|| Thumbnail::register(destination, hwnd as HWND))
            .as_mut()?;
        let Some(size) = thumb.source_size() else {
            thumb.place(None);
            return None;
        };
        Some(dwm_thumbs::picture_source(
            dwm_thumbs::visible_frame(hwnd as HWND),
            size,
        ))
    }

    /// Shows the window's picture centred in `area`: fitted to it, or at
    /// `scale` points per pixel of the window so pictures of several
    /// windows keep their sizes to each other. Returns where it went.
    pub fn show_scaled(
        &mut self,
        hwnd: isize,
        area: Rect,
        pixels_per_point: f32,
        scale: Option<f32>,
    ) -> Option<Rect> {
        self.show_faded(hwnd, area, pixels_per_point, scale, 1.0)
    }

    /// `show_scaled` with only `alpha` of the picture showing (0 to 1), for
    /// a card that fades out.
    pub fn show_faded(
        &mut self,
        hwnd: isize,
        area: Rect,
        pixels_per_point: f32,
        scale: Option<f32>,
        alpha: f32,
    ) -> Option<Rect> {
        let source = self.source(hwnd)?;
        let size = match scale {
            Some(scale) => source.size() * scale,
            None => dwm_thumbs::fit(source.size(), area.size()),
        };
        let dest = Rect::from_center_size(area.center(), size);
        let thumb = self.thumbs.get_mut(&hwnd)?.as_mut()?;
        thumb.place(Some(Placement {
            dest: dwm_thumbs::physical(dest, pixels_per_point),
            source: dwm_thumbs::physical(source, 1.0),
            opacity: (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
        }));
        self.used.insert(hwnd);
        Some(dest)
    }

    /// Puts the window's picture above the others from the next frame on,
    /// for a card that is dragged over them: DWM draws the newest on top.
    pub fn raise(&mut self, hwnd: isize) {
        self.thumbs.remove(&hwnd);
    }

    pub fn end(&mut self) {
        for (hwnd, thumb) in self.thumbs.iter_mut() {
            if !self.used.contains(hwnd) {
                if let Some(thumb) = thumb {
                    thumb.place(None);
                }
            }
        }
    }

    /// Lets every picture go, as when the window closes.
    pub fn clear(&mut self) {
        self.thumbs.clear();
        self.window = 0;
    }
}

/// One scale for several pictures, in points per window pixel: the largest
/// at which every window still fits in `area`, so a narrow window shows
/// narrower than a wide one, as on the screen. None without any window.
pub fn common_scale(sources: &[Vec2], area: Vec2) -> Option<f32> {
    sources
        .iter()
        .filter(|size| size.x > 0.0 && size.y > 0.0)
        .map(|size| (area.x / size.x).min(area.y / size.y))
        .min_by(f32::total_cmp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    #[test]
    fn a_narrow_window_shows_narrower_than_a_wide_one() {
        let area = vec2(280.0, 175.0);
        let wide = vec2(3840.0, 2100.0);
        let narrow = vec2(1280.0, 2100.0);
        let scale = common_scale(&[wide, narrow], area).unwrap();
        assert!((wide * scale).x <= 280.0 + 0.01 && (wide * scale).y <= 175.0 + 0.01);
        assert!(
            (narrow * scale).x < (wide * scale).x / 2.5,
            "a third as wide"
        );
        assert_eq!(
            (narrow * scale).y,
            (wide * scale).y,
            "same height, as on screen"
        );
    }

    #[test]
    fn every_window_fits_at_the_common_scale() {
        let area = vec2(280.0, 175.0);
        let sizes = [
            vec2(3840.0, 2100.0),
            vec2(1280.0, 2880.0),
            vec2(800.0, 600.0),
        ];
        let scale = common_scale(&sizes, area).unwrap();
        for size in sizes {
            let shown = size * scale;
            assert!(shown.x <= 280.01 && shown.y <= 175.01, "{shown:?}");
        }
        assert_eq!(common_scale(&[], area), None);
        assert_eq!(common_scale(&[vec2(0.0, 10.0)], area), None);
    }

    #[test]
    fn without_a_window_to_draw_on_no_picture_shows() {
        let mut pictures = Pictures::default();
        pictures.begin(0);
        assert!(!pictures.show(1, Rect::NOTHING, 1.0));
        pictures.end();
        pictures.clear();
    }
}
