//! Live pictures of windows in the pin list and the picker, drawn by DWM
//! on top of the egui window, as the Arrange strip's are. They belong to
//! the UI thread, which owns the window they are drawn on.

use std::collections::{HashMap, HashSet};

use egui::Rect;
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
        if self.window == 0 {
            return false;
        }
        let destination = self.window as HWND;
        let thumb = self
            .thumbs
            .entry(hwnd)
            .or_insert_with(|| Thumbnail::register(destination, hwnd as HWND))
            .as_mut();
        let Some(thumb) = thumb else {
            return false;
        };
        let Some(size) = thumb.source_size() else {
            thumb.place(None);
            return false;
        };
        let source = dwm_thumbs::picture_source(dwm_thumbs::visible_frame(hwnd as HWND), size);
        let fitted = dwm_thumbs::fit(source.size(), area.size());
        let dest = Rect::from_center_size(area.center(), fitted);
        thumb.place(Some(Placement {
            dest: dwm_thumbs::physical(dest, pixels_per_point),
            source: dwm_thumbs::physical(source, 1.0),
            opacity: 255,
        }));
        self.used.insert(hwnd);
        true
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_window_to_draw_on_no_picture_shows() {
        let mut pictures = Pictures::default();
        pictures.begin(0);
        assert!(!pictures.show(1, Rect::NOTHING, 1.0));
        pictures.end();
        pictures.clear();
    }
}
