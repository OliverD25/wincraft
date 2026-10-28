use egui::{pos2, Rect, Vec2};
use windows_sys::Win32::Foundation::{HWND, RECT, SIZE};
use windows_sys::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DwmQueryThumbnailSourceSize, DwmRegisterThumbnail,
    DwmSetWindowAttribute, DwmUnregisterThumbnail, DwmUpdateThumbnailProperties,
    DWMWA_BORDER_COLOR, DWMWA_CLOAK, DWMWA_EXTENDED_FRAME_BOUNDS, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND, DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION,
    DWM_TNP_RECTSOURCE, DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;

/// Where and how a thumbnail is drawn, in physical pixels: `dest` in the
/// destination window's client area, `source` inside the source window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub dest: [i32; 4],
    pub source: [i32; 4],
    pub opacity: u8,
}

/// A live DWM thumbnail of another window. DWM composes the picture on top
/// of the destination window by itself, so egui only reserves the space. It
/// must be created, moved and dropped on the thread that owns the
/// destination window.
pub struct Thumbnail {
    id: isize,
    shown: Option<Placement>,
    hidden: bool,
}

impl Thumbnail {
    pub fn register(destination: HWND, source: HWND) -> Option<Self> {
        let mut id = 0;
        let hr = unsafe { DwmRegisterThumbnail(destination, source, &mut id) };
        (hr >= 0 && id != 0).then(|| Self {
            id,
            shown: None,
            hidden: true,
        })
    }

    /// The source window's size, or None when DWM has nothing to show for
    /// it, which is when a card falls back to the window's name.
    pub fn source_size(&self) -> Option<Vec2> {
        let mut size = SIZE { cx: 0, cy: 0 };
        let hr = unsafe { DwmQueryThumbnailSourceSize(self.id, &mut size) };
        (hr >= 0 && size.cx > 0 && size.cy > 0).then(|| Vec2::new(size.cx as f32, size.cy as f32))
    }

    /// Moves the picture, or hides it with None. DWM is only called when
    /// something changed, since a card is laid out again every frame.
    pub fn place(&mut self, placement: Option<Placement>) {
        let changed = match placement {
            Some(placement) => self.hidden || self.shown != Some(placement),
            None => !self.hidden,
        };
        if !changed {
            return;
        }
        let (flags, visible, placement_or_last) = match placement {
            Some(placement) => (
                DWM_TNP_RECTDESTINATION
                    | DWM_TNP_RECTSOURCE
                    | DWM_TNP_OPACITY
                    | DWM_TNP_VISIBLE
                    | DWM_TNP_SOURCECLIENTAREAONLY,
                1,
                placement,
            ),
            None => (
                DWM_TNP_VISIBLE,
                0,
                self.shown.unwrap_or(Placement {
                    dest: [0; 4],
                    source: [0; 4],
                    opacity: 0,
                }),
            ),
        };
        let properties = DWM_THUMBNAIL_PROPERTIES {
            dwFlags: flags,
            rcDestination: rect(placement_or_last.dest),
            rcSource: rect(placement_or_last.source),
            opacity: placement_or_last.opacity,
            fVisible: visible,
            fSourceClientAreaOnly: 0,
        };
        if unsafe { DwmUpdateThumbnailProperties(self.id, &properties) } >= 0 {
            self.hidden = placement.is_none();
            if placement.is_some() {
                self.shown = placement;
            }
        }
    }
}

impl Drop for Thumbnail {
    fn drop(&mut self) {
        unsafe { DwmUnregisterThumbnail(self.id) };
    }
}

/// The part of a window that is drawn, in its own pixels. Windows 10 and 11
/// windows carry invisible resize borders several pixels wide, which a
/// whole-window thumbnail shows as dark bands.
pub fn visible_frame(window: HWND) -> Option<Rect> {
    let mut outer = rect([0; 4]);
    let mut frame = rect([0; 4]);
    if unsafe { GetWindowRect(window, &mut outer) } == 0 {
        return None;
    }
    let hr = unsafe {
        DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            &mut frame as *mut RECT as *mut core::ffi::c_void,
            std::mem::size_of::<RECT>() as u32,
        )
    };
    if hr < 0 {
        return None;
    }
    let inner = Rect::from_min_max(
        pos2(
            (frame.left - outer.left) as f32,
            (frame.top - outer.top) as f32,
        ),
        pos2(
            (frame.right - outer.left) as f32,
            (frame.bottom - outer.top) as f32,
        ),
    );
    inner.is_positive().then_some(inner)
}

/// Rounded corners and a border in the theme's colour. A second eframe
/// window cannot be transparent with this GL setup, so the strip cannot draw
/// its own rounded panel and shadow the way the palette does.
pub fn round_corners(window: HWND, border: u32) {
    let corners = DWMWCP_ROUND;
    let (round, colour) = unsafe {
        (
            DwmSetWindowAttribute(
                window,
                DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                &corners as *const _ as *const core::ffi::c_void,
                std::mem::size_of_val(&corners) as u32,
            ),
            DwmSetWindowAttribute(
                window,
                DWMWA_BORDER_COLOR as u32,
                &border as *const u32 as *const core::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
            ),
        )
    };
    let hr = if round < 0 { round } else { colour };
    crate::core::detailed::call(
        "DwmSetWindowAttribute corners and border",
        window as isize,
        i64::from(hr),
        hr < 0,
    );
}

/// Keeps one of WinCraft's own windows off the screen while it still draws
/// (`on`), or puts it back.
pub fn cloak(window: HWND, on: bool) {
    let value: i32 = on.into();
    let hr = unsafe {
        DwmSetWindowAttribute(
            window,
            DWMWA_CLOAK as u32,
            &value as *const i32 as *const core::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        )
    };
    crate::core::detailed::call(
        if on {
            "DwmSetWindowAttribute DWMWA_CLOAK on"
        } else {
            "DwmSetWindowAttribute DWMWA_CLOAK off"
        },
        window as isize,
        i64::from(hr),
        hr < 0,
    );
}

fn rect(r: [i32; 4]) -> RECT {
    RECT {
        left: r[0],
        top: r[1],
        right: r[2],
        bottom: r[3],
    }
}

/// The part of a window its picture shows, in the window's own pixels: its
/// visible frame, or the whole window when that frame does not fit it, as
/// for a minimized window, whose frame is the small one it has while
/// minimized.
pub fn picture_source(frame: Option<Rect>, size: Vec2) -> Rect {
    let whole = Rect::from_min_size(pos2(0.0, 0.0), size);
    match frame {
        Some(frame)
            if whole.contains_rect(frame)
                && frame.width() >= size.x / 2.0
                && frame.height() >= size.y / 2.0 =>
        {
            Rect::from_min_max(frame.min.round(), frame.max.round())
        }
        _ => whole,
    }
}

/// The largest size with the source's aspect ratio that fits in `area`.
pub fn fit(source: Vec2, area: Vec2) -> Vec2 {
    if source.x <= 0.0 || source.y <= 0.0 {
        return area;
    }
    let scale = (area.x / source.x).min(area.y / source.y);
    source * scale
}

/// Where a picture of `source` goes in `area` so that it keeps off `clear`,
/// the badges painted over it: DWM draws its picture above everything egui
/// paints and cannot clip it round a corner. The picture is fitted and
/// centred in `area` as usual; when that meets `clear`, it is fitted instead
/// into the part of `area` below `clear` or left of it, whichever keeps it
/// larger, so the shrink is as small as it can be.
pub fn fit_clear(area: Rect, source: Vec2, clear: Rect) -> Rect {
    let placed = |part: Rect| Rect::from_center_size(part.center(), fit(source, part.size()));
    let whole = placed(area);
    if !whole.intersects(clear) {
        return whole;
    }
    let below = Rect::from_min_max(pos2(area.min.x, clear.bottom().max(area.min.y)), area.max);
    let beside = Rect::from_min_max(area.min, pos2(clear.left().min(area.max.x), area.max.y));
    [below, beside]
        .into_iter()
        .filter(|part| part.is_positive())
        .map(placed)
        .max_by(|a, b| a.area().total_cmp(&b.area()))
        .unwrap_or(whole)
}
/// A picture of `source` drawn at `dest` but only visible inside `clip`: DWM
/// cannot clip, so the destination shrinks to the visible part and the
/// source is cropped by the same fraction. None when nothing is visible.
pub fn crop(dest: Rect, clip: Rect, source: Rect) -> Option<(Rect, Rect)> {
    let visible = dest.intersect(clip);
    if !visible.is_positive() || !dest.is_positive() {
        return None;
    }
    let map = |p: egui::Pos2| {
        pos2(
            source.min.x + (p.x - dest.min.x) / dest.width() * source.width(),
            source.min.y + (p.y - dest.min.y) / dest.height() * source.height(),
        )
    };
    Some((
        visible,
        Rect::from_min_max(map(visible.min), map(visible.max)),
    ))
}

/// Logical points to physical pixels, the unit DWM works in.
pub fn physical(rect: Rect, pixels_per_point: f32) -> [i32; 4] {
    [
        (rect.min.x * pixels_per_point).round() as i32,
        (rect.min.y * pixels_per_point).round() as i32,
        (rect.max.x * pixels_per_point).round() as i32,
        (rect.max.y * pixels_per_point).round() as i32,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    #[test]
    fn a_source_is_fitted_without_changing_its_shape() {
        let close = |a: Vec2, b: Vec2| (a - b).length() < 0.001;
        assert!(close(
            fit(vec2(1920.0, 1080.0), vec2(200.0, 120.0)),
            vec2(200.0, 112.5)
        ));
        assert!(close(
            fit(vec2(600.0, 900.0), vec2(200.0, 120.0)),
            vec2(80.0, 120.0)
        ));
        assert_eq!(fit(vec2(0.0, 0.0), vec2(200.0, 120.0)), vec2(200.0, 120.0));
    }

    #[test]
    fn a_partly_hidden_picture_is_cropped_not_squeezed() {
        let dest = Rect::from_min_size(pos2(100.0, 0.0), vec2(200.0, 100.0));
        let source = Rect::from_min_size(pos2(8.0, 0.0), vec2(2000.0, 1000.0));
        let clip = Rect::from_min_max(pos2(0.0, 0.0), pos2(200.0, 100.0));
        let (shown, cropped) = crop(dest, clip, source).expect("half is visible");
        assert_eq!(
            shown,
            Rect::from_min_max(pos2(100.0, 0.0), pos2(200.0, 100.0))
        );
        assert_eq!(
            cropped,
            Rect::from_min_max(pos2(8.0, 0.0), pos2(1008.0, 1000.0))
        );

        let whole = crop(dest, Rect::EVERYTHING, source).unwrap();
        assert_eq!(whole.1, source);
        let outside = Rect::from_min_size(pos2(400.0, 0.0), vec2(50.0, 50.0));
        assert_eq!(crop(dest, outside, source), None);
    }

    #[test]
    fn logical_rects_become_physical_pixels() {
        let r = Rect::from_min_max(pos2(10.0, 20.0), pos2(110.0, 80.5));
        assert_eq!(physical(r, 1.5), [15, 30, 165, 121]);
    }

    #[test]
    fn a_picture_keeps_off_the_badges_by_the_smallest_shrink() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 96.0));
        let wide = vec2(200.0, 96.0);
        let badges = Rect::from_min_size(pos2(120.0, 6.0), vec2(74.0, 18.0));
        assert_eq!(fit_clear(area, wide, Rect::NOTHING), area);
        let moved = fit_clear(area, wide, badges);
        assert!(!moved.intersect(badges).is_positive());
        assert!(area.contains_rect(moved));
        assert!(moved.area() > 0.0);
        // Tall and narrow: it already stays left of the badges.
        let far = Rect::from_min_size(pos2(130.0, 6.0), vec2(64.0, 18.0));
        let tall = fit_clear(area, vec2(50.0, 100.0), far);
        assert_eq!(tall, fit_clear(area, vec2(50.0, 100.0), Rect::NOTHING));
        // Badges across the whole width leave only the part below them.
        let across = Rect::from_min_size(pos2(6.0, 6.0), vec2(188.0, 18.0));
        let below = fit_clear(area, wide, across);
        assert!(below.top() >= across.bottom());
    }
}
