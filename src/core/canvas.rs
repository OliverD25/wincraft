//! A picture for a `core::layered` panel that holds more than one line of
//! text: rounded boxes, outlines, dashed outlines, Lucide icons, app icons
//! and text runs in their own colours, drawn one over the other. Sizes are
//! physical pixels; the caller scales by the monitor's DPI.

use egui::Color32;
use windows_sys::Win32::Foundation::SIZE;
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetTextExtentPoint32W, SelectObject, HDC, HFONT,
};

use crate::core::layered::{faces, font, text_mask, utf16, Image, Run};
use crate::search::icons::Pixels;
use crate::ui::widgets::icons::{self, Icon};

/// A box: left, top, width and height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Area {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn inset(self, by: f32) -> Self {
        Self::new(
            self.x + by,
            self.y + by,
            self.w - 2.0 * by,
            self.h - 2.0 * by,
        )
    }

    pub fn right(self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(self) -> f32 {
        self.y + self.h
    }

    /// Signed distance from the point to the edge of this box with rounded
    /// corners: below zero inside.
    fn distance(self, px: f32, py: f32, radius: f32) -> f32 {
        let (half_w, half_h) = (self.w / 2.0, self.h / 2.0);
        let radius = radius.min(half_w).min(half_h).max(0.0);
        let qx = (px - (self.x + half_w)).abs() - (half_w - radius);
        let qy = (py - (self.y + half_h)).abs() - (half_h - radius);
        (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - radius
    }
}

/// Premultiplied BGRA pixels, top row first.
#[derive(Clone)]
pub struct Canvas {
    pub width: i32,
    pub height: i32,
    pixels: Vec<u32>,
}

/// Which face of the UI font a size is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
    Regular,
    Semibold,
}

impl Canvas {
    pub fn new(width: i32, height: i32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            width,
            height,
            pixels: vec![0; (width * height) as usize],
        }
    }

    pub fn into_image(self) -> Image {
        Image {
            width: self.width,
            height: self.height,
            pixels: self.pixels,
        }
    }

    #[cfg(test)]
    pub fn pixel(&self, x: i32, y: i32) -> u32 {
        self.pixels[(y * self.width + x) as usize]
    }

    /// Lays `colour` (its own alpha too) over one pixel, `coverage` of it.
    pub fn blend(&mut self, x: i32, y: i32, colour: Color32, coverage: f32) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        let alpha = coverage.clamp(0.0, 1.0) * f32::from(colour.a()) / 255.0;
        if alpha <= 0.0 {
            return;
        }
        let below = &mut self.pixels[(y * self.width + x) as usize];
        let channel = |shift: u32, source: u8| {
            let under = f32::from((*below >> shift) as u8);
            (f32::from(source) * alpha + under * (1.0 - alpha))
                .round()
                .clamp(0.0, 255.0) as u32
        };
        let out = (255.0 * alpha + f32::from((*below >> 24) as u8) * (1.0 - alpha))
            .round()
            .clamp(0.0, 255.0) as u32;
        *below = (out << 24)
            | (channel(16, colour.r()) << 16)
            | (channel(8, colour.g()) << 8)
            | channel(0, colour.b());
    }

    /// The pixels a shape can touch, clipped to the canvas.
    fn span(&self, area: Area, grow: f32) -> (i32, i32, i32, i32) {
        let left = ((area.x - grow).floor() as i32).max(0);
        let top = ((area.y - grow).floor() as i32).max(0);
        let right = ((area.right() + grow).ceil() as i32).min(self.width);
        let bottom = ((area.bottom() + grow).ceil() as i32).min(self.height);
        (left, top, right, bottom)
    }

    pub fn fill(&mut self, area: Area, radius: f32, colour: Color32) {
        let (left, top, right, bottom) = self.span(area, 1.0);
        for y in top..bottom {
            for x in left..right {
                let distance = area.distance(x as f32 + 0.5, y as f32 + 0.5, radius);
                self.blend(x, y, colour, 0.5 - distance);
            }
        }
    }

    /// A line `width` thick along the inside of the box's edge.
    pub fn outline(&mut self, area: Area, radius: f32, width: f32, colour: Color32) {
        self.dashed(area, radius, width, colour, None);
    }

    /// The outline in dashes of `dash` with gaps as long, or solid with None.
    pub fn dashed(
        &mut self,
        area: Area,
        radius: f32,
        width: f32,
        colour: Color32,
        dash: Option<f32>,
    ) {
        let (left, top, right, bottom) = self.span(area, 1.0);
        let inner = area.inset(width);
        for y in top..bottom {
            for x in left..right {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let outer = (0.5 - area.distance(px, py, radius)).clamp(0.0, 1.0);
                let within =
                    (0.5 - inner.distance(px, py, (radius - width).max(0.0))).clamp(0.0, 1.0);
                let mut coverage = (outer - within).max(0.0);
                if let Some(dash) = dash {
                    // Along the top and bottom the dashes count across, along
                    // the sides down; the corners take the nearer edge's.
                    let near_side = (px - area.x).min(area.right() - px)
                        < (py - area.y).min(area.bottom() - py);
                    let along = if near_side { py - area.y } else { px - area.x };
                    if along.rem_euclid(2.0 * dash) >= dash {
                        coverage = 0.0;
                    }
                }
                self.blend(x, y, colour, coverage);
            }
        }
    }

    /// A soft shadow below a box, fading out over `blur`.
    pub fn shadow(&mut self, area: Area, radius: f32, blur: f32, colour: Color32) {
        let (left, top, right, bottom) = self.span(area, blur);
        for y in top..bottom {
            for x in left..right {
                let distance = area.distance(x as f32 + 0.5, y as f32 + 0.5, radius);
                if distance <= 0.0 {
                    continue;
                }
                let fade = (1.0 - distance / blur.max(1.0)).clamp(0.0, 1.0);
                self.blend(x, y, colour, fade * fade);
            }
        }
    }

    /// Lines through points, `width` thick with round ends.
    pub fn polyline(&mut self, points: &[(f32, f32)], width: f32, colour: Color32) {
        if points.is_empty() {
            return;
        }
        let half = width / 2.0;
        let xs = points.iter().map(|point| point.0);
        let ys = points.iter().map(|point| point.1);
        let (min_x, max_x) = xs.fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
        let (min_y, max_y) = ys.fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
        let bounds = Area::new(min_x, min_y, max_x - min_x, max_y - min_y);
        let (left, top, right, bottom) = self.span(bounds, half + 1.0);
        let segments: Vec<((f32, f32), (f32, f32))> = if points.len() == 1 {
            vec![(points[0], points[0])]
        } else {
            points.windows(2).map(|pair| (pair[0], pair[1])).collect()
        };
        for y in top..bottom {
            for x in left..right {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let distance = segments
                    .iter()
                    .map(|(a, b)| segment_distance((px, py), *a, *b))
                    .fold(f32::MAX, f32::min);
                self.blend(x, y, colour, half + 0.5 - distance);
            }
        }
    }

    /// A Lucide icon in a square of `side` at (x, y), with its usual line.
    pub fn glyph(&mut self, icon: Icon, x: f32, y: f32, side: f32, colour: Color32) {
        for line in icons::outline(icon, side) {
            let moved: Vec<(f32, f32)> = line.iter().map(|(px, py)| (x + px, y + py)).collect();
            self.polyline(&moved, side / 12.0, colour);
        }
    }

    /// An app's icon scaled into a square of `side` at (x, y).
    pub fn icon(&mut self, icon: &Pixels, x: f32, y: f32, side: f32, alpha: f32) {
        let source = icon.side as f32;
        let (left, top, right, bottom) = self.span(Area::new(x, y, side, side), 0.0);
        for py in top..bottom {
            for px in left..right {
                let u = ((px as f32 + 0.5 - x) / side * source - 0.5).clamp(0.0, source - 1.0);
                let v = ((py as f32 + 0.5 - y) / side * source - 0.5).clamp(0.0, source - 1.0);
                let sample = bilinear(icon, u, v);
                self.blend(px, py, sample, alpha);
            }
        }
    }

    /// Text runs drawn with GDI, each in its own colour.
    pub fn text(&mut self, runs: &[Run], fonts: &[HFONT], alpha: f32) {
        let dc = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
        for run in runs {
            let own = Run {
                text: run.text.clone(),
                colour: run.colour,
                x: 0,
                y: 0,
                width: run.width,
                height: run.height,
                font: run.font,
            };
            let Some(mask) = text_mask(dc, run.width.max(1), run.height.max(1), &[own], fonts)
            else {
                continue;
            };
            for y in 0..run.height {
                for x in 0..run.width {
                    let coverage = f32::from(mask[(y * run.width + x) as usize]) / 255.0;
                    self.blend(run.x + x, run.y + y, run.colour, coverage * alpha);
                }
            }
        }
        unsafe { DeleteDC(dc) };
    }
}

fn bilinear(icon: &Pixels, u: f32, v: f32) -> Color32 {
    let side = icon.side;
    let (x0, y0) = (u.floor() as usize, v.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(side - 1), (y0 + 1).min(side - 1));
    let (fx, fy) = (u - x0 as f32, v - y0 as f32);
    let at = |x: usize, y: usize| {
        let i = (y * side + x) * 4;
        let a = f32::from(icon.rgba[i + 3]) / 255.0;
        [
            f32::from(icon.rgba[i]) * a,
            f32::from(icon.rgba[i + 1]) * a,
            f32::from(icon.rgba[i + 2]) * a,
            a,
        ]
    };
    let mix = |a: [f32; 4], b: [f32; 4], t: f32| {
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
            a[3] + (b[3] - a[3]) * t,
        ]
    };
    let top = mix(at(x0, y0), at(x1, y0), fx);
    let bottom = mix(at(x0, y1), at(x1, y1), fx);
    let [r, g, b, a] = mix(top, bottom, fy);
    if a <= 0.0 {
        return Color32::TRANSPARENT;
    }
    let byte = |value: f32| (value / a).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(byte(r), byte(g), byte(b), (a * 255.0).round() as u8)
}

fn segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let t = if length <= f32::EPSILON {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length).clamp(0.0, 1.0)
    };
    let (cx, cy) = (a.0 + t * dx, a.1 + t * dy);
    ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()
}

/// Fonts for one picture: semibold and regular at the sizes asked for,
/// with a device context to measure text in. Dropping it frees them.
pub struct Fonts {
    dc: HDC,
    pub list: Vec<HFONT>,
}

impl Fonts {
    /// `sizes` are (weight, pixels); a run's `font` is its place here.
    pub fn new(sizes: &[(Weight, i32)]) -> Self {
        let [(semi_face, semi_weight), (face, weight)] = *faces();
        let list = sizes
            .iter()
            .map(|(kind, pixels)| match kind {
                Weight::Semibold => font(semi_face, semi_weight, *pixels),
                Weight::Regular => font(face, weight, *pixels),
            })
            .collect();
        Self {
            dc: unsafe { CreateCompatibleDC(std::ptr::null_mut()) },
            list,
        }
    }

    /// Width and height of `text` in font `index`.
    pub fn measure(&self, text: &str, index: usize) -> (i32, i32) {
        let wide = utf16(text);
        let mut size = SIZE { cx: 0, cy: 0 };
        unsafe {
            SelectObject(self.dc, self.list[index]);
            GetTextExtentPoint32W(self.dc, wide.as_ptr(), wide.len() as i32, &mut size);
        }
        (size.cx, size.cy)
    }

    /// `text`, cut with an ellipsis to fit `max` pixels.
    pub fn fit(&self, text: &str, index: usize, max: i32) -> String {
        if self.measure(text, index).0 <= max {
            return text.to_string();
        }
        let chars: Vec<char> = text.chars().collect();
        let (mut low, mut high) = (0, chars.len());
        while low < high {
            let middle = (low + high).div_ceil(2);
            let candidate: String = chars[..middle].iter().collect();
            if self
                .measure(&format!("{}\u{2026}", candidate.trim_end()), index)
                .0
                <= max
            {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        let kept: String = chars[..low].iter().collect();
        format!("{}\u{2026}", kept.trim_end())
    }

    /// A run of `text` with its top left corner at (x, y).
    pub fn run(&self, text: &str, index: usize, colour: Color32, x: i32, y: i32) -> Run {
        let (width, height) = self.measure(text, index);
        Run {
            text: utf16(text),
            colour,
            x,
            y,
            width,
            height,
            font: index,
        }
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        unsafe {
            DeleteDC(self.dc);
            for font in &self.list {
                DeleteObject(*font);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color32 = Color32::from_rgb(255, 0, 0);

    fn alpha(pixel: u32) -> u32 {
        pixel >> 24
    }

    #[test]
    fn a_fill_covers_its_box_and_rounds_its_corners() {
        let mut canvas = Canvas::new(40, 20);
        canvas.fill(Area::new(0.0, 0.0, 40.0, 20.0), 8.0, RED);
        assert_eq!(alpha(canvas.pixel(20, 10)), 255);
        assert_eq!(canvas.pixel(20, 10) & 0x00FF_FFFF, 0x00FF_0000);
        assert_eq!(alpha(canvas.pixel(0, 0)), 0, "the corner stays clear");
    }

    #[test]
    fn an_outline_leaves_the_middle_and_a_dash_leaves_gaps() {
        let mut canvas = Canvas::new(40, 40);
        canvas.outline(Area::new(0.0, 0.0, 40.0, 40.0), 0.0, 2.0, RED);
        assert_eq!(alpha(canvas.pixel(20, 20)), 0);
        assert_eq!(alpha(canvas.pixel(20, 0)), 255);
        let mut dashed = Canvas::new(40, 40);
        dashed.dashed(Area::new(0.0, 0.0, 40.0, 40.0), 0.0, 2.0, RED, Some(4.0));
        let top: Vec<u32> = (0..40).map(|x| alpha(dashed.pixel(x, 0))).collect();
        assert!(top.contains(&255) && top.contains(&0), "{top:?}");
    }

    #[test]
    fn a_line_is_drawn_along_its_points_only() {
        let mut canvas = Canvas::new(20, 20);
        canvas.polyline(&[(2.0, 10.0), (18.0, 10.0)], 2.0, RED);
        assert!(alpha(canvas.pixel(10, 10)) > 200);
        assert_eq!(alpha(canvas.pixel(10, 2)), 0);
        canvas.polyline(&[], 2.0, RED);
        let mut pin = Canvas::new(24, 24);
        pin.glyph(Icon::Pin, 0.0, 0.0, 24.0, RED);
        assert!(alpha(pin.pixel(12, 20)) > 0, "the point of the pin");
    }

    #[test]
    fn an_icon_is_scaled_into_its_square_and_a_shadow_fades() {
        let icon = Pixels {
            side: 2,
            rgba: vec![0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 0],
        };
        let mut canvas = Canvas::new(10, 10);
        canvas.icon(&icon, 0.0, 0.0, 8.0, 1.0);
        assert_eq!(canvas.pixel(1, 1) & 0xFF, 255, "blue at the top left");
        assert_eq!(alpha(canvas.pixel(9, 9)), 0, "outside the square");
        let mut shadowed = Canvas::new(30, 30);
        shadowed.shadow(Area::new(10.0, 10.0, 10.0, 10.0), 2.0, 8.0, RED);
        assert!(alpha(shadowed.pixel(9, 15)) > alpha(shadowed.pixel(4, 15)));
        assert_eq!(alpha(shadowed.pixel(15, 15)), 0, "nothing under the box");
    }

    #[test]
    fn long_text_is_cut_with_an_ellipsis_to_fit() {
        let fonts = Fonts::new(&[(Weight::Regular, 12), (Weight::Semibold, 14)]);
        let long = "A rather long window title that cannot fit";
        let fitted = fonts.fit(long, 0, 60);
        assert!(fitted.ends_with('\u{2026}'), "{fitted}");
        assert!(fonts.measure(&fitted, 0).0 <= 60);
        assert_eq!(fonts.fit("Hi", 1, 60), "Hi");
        let run = fonts.run("Hi", 1, RED, 3, 4);
        let mut canvas = Canvas::new(40, 30);
        canvas.text(&[run], &fonts.list, 1.0);
        let any = (0..40).any(|x| (0..30).any(|y| alpha(canvas.pixel(x, y)) > 0));
        assert!(any, "the text left some pixels");
        assert_eq!(canvas.into_image().width, 40);
    }
}
