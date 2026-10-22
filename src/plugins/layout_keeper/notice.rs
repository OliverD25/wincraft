//! The "please wait" notice: a small click-through panel at the bottom right
//! of the primary monitor that tells the person WinCraft is putting windows
//! back, so they do not start moving them at the same time.
//!
//! The words and the rule for when the panel is on screen are plain data and
//! functions here, so they are tested without a window. The window itself is
//! a `core::layered` popup, like the language switch.

use egui::Color32;
use windows_sys::Win32::Foundation::{POINT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetMonitorInfoW, GetTextExtentPoint32W,
    MonitorFromPoint, SelectObject, HFONT, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, LoadImageW, HICON, IMAGE_ICON, LR_DEFAULTCOLOR,
};

use super::restore::{Phase, Progress};
use crate::core::layered::{compose, faces, font, text_mask, utf16, Image, Layered, Run};
use crate::core::theme;
use crate::search::icons;

const CLASS_NAME: &str = "WinCraftRestoreNotice";
/// Every line the panel shows starts with who is speaking: it is a small
/// window of its own, and the person should know which program it is.
const TITLE: &str = "WinCraft \u{b7} Putting your windows back \u{2014} please wait";
const FINAL_PREFIX: &str = "WinCraft \u{b7} ";
/// A line is cut to this many characters, and the panel is never wider than
/// `MAX_WIDTH` logical pixels whatever the lines say.
const MAX_LINE_CHARS: usize = 60;
const MAX_WIDTH: f32 = 560.0;
const ICON_SIZE: f32 = 18.0;
const ICON_GAP: f32 = 8.0;
/// Apps listed before "+N more".
const MAX_LINES: usize = 5;
/// The final line stays this many seconds before it fades.
pub const FINAL_HOLD_SECONDS: u64 = 4;
/// Windows limits a tray tooltip to 127 characters.
const TIP_CHARS: usize = 127;

/// Sizes in logical pixels, scaled by the monitor's DPI.
const PAD_X: f32 = 18.0;
const PAD_Y: f32 = 14.0;
const SECTION_GAP: f32 = 8.0;
const LINE_GAP: f32 = 4.0;
const RADIUS: f32 = 12.0;
const MARGIN: f32 = 16.0;
const TITLE_TEXT: f32 = 14.0;
const BODY_TEXT: f32 = 13.0;
const SMALL_TEXT: f32 = 12.0;
const MIN_WIDTH: f32 = 300.0;
/// The progress bar is this high, with fully rounded ends. Its words sit at
/// the right end of the same row.
const BAR_HEIGHT: f32 = 4.0;
const BAR_GAP: f32 = 10.0;
const BAR_MIN_WIDTH: f32 = 120.0;

/// What the panel says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Content {
    pub title: String,
    pub lines: Vec<String>,
    pub footer: Option<String>,
    pub bar: Option<Bar>,
}

/// The overall progress bar: windows put back out of windows expected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bar {
    pub done: usize,
    pub total: usize,
    /// The restore is over, so the bar is full and drawn in the success colour.
    pub finished: bool,
}

impl Bar {
    fn label(self) -> String {
        format!("{} of {}", self.done, windows(self.total))
    }
}

/// Windows put back and windows expected, summed over every app that is not
/// skipped. An app that is restored counts in full, so the first number only
/// grows as the run goes on; an app still opening counts the windows it has,
/// never more than the saved layout holds. None when no window is expected.
pub fn overall_progress(progress: &[Progress]) -> Option<(usize, usize)> {
    let (mut done, mut total) = (0, 0);
    for entry in progress {
        if entry.phase == Phase::Skipped {
            continue;
        }
        let all = entry.expected.max(entry.count);
        total += all;
        done += match entry.phase {
            Phase::Moving | Phase::LateWindows | Phase::Done => all,
            _ => entry.count.min(entry.expected),
        };
    }
    (total > 0).then_some((done, total))
}

fn bar_for(progress: &[Progress]) -> Option<Bar> {
    overall_progress(progress).map(|(done, total)| Bar {
        done,
        total,
        finished: false,
    })
}

fn windows(count: usize) -> String {
    if count == 1 {
        "1 window".to_string()
    } else {
        format!("{count} windows")
    }
}

fn apps(count: usize) -> String {
    if count == 1 {
        "1 app".to_string()
    } else {
        format!("{count} apps")
    }
}

/// The panel while at least one app is active: a line for each app that has
/// windows and is not done, and a summary of the finished ones. None when
/// nothing is active, so nothing stays on screen for apps that have not
/// opened a window.
pub fn active_content(progress: &[Progress], label: &dyn Fn(&str) -> String) -> Option<Content> {
    let mut active: Vec<&Progress> = progress
        .iter()
        .filter(|entry| entry.phase.is_active())
        .collect();
    if active.is_empty() {
        return None;
    }
    // What is moving now first, then the apps still taking windows, the
    // biggest first.
    let rank = |phase: Phase| match phase {
        Phase::Moving => 0,
        Phase::LateWindows => 1,
        _ => 2,
    };
    active.sort_by(|a, b| {
        rank(a.phase)
            .cmp(&rank(b.phase))
            .then(b.count.cmp(&a.count))
            .then(label(&a.group).cmp(&label(&b.group)))
    });
    let mut lines: Vec<String> = active
        .iter()
        .take(MAX_LINES)
        .map(|entry| {
            let name = label(&entry.group);
            fit(&line_for(entry, &name), MAX_LINE_CHARS)
        })
        .collect();
    if active.len() > MAX_LINES {
        lines.push(format!("+{} more", active.len() - MAX_LINES));
    }

    let mut done: Vec<&Progress> = progress
        .iter()
        .filter(|entry| entry.phase == Phase::Done)
        .collect();
    done.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then(label(&a.group).cmp(&label(&b.group)))
    });
    let footer = (!done.is_empty()).then(|| {
        let mut names: Vec<String> = done.iter().take(2).map(|e| label(&e.group)).collect();
        if done.len() > 2 {
            names.push(format!("+{}", done.len() - 2));
        }
        format!("Done: {}", names.join(", "))
    });
    Some(Content {
        title: TITLE.to_string(),
        lines,
        footer,
        bar: bar_for(progress),
    })
}

/// What one active app's line says. "Waiting for the rest" only when the
/// saved layout has more windows than are open.
fn line_for(entry: &Progress, name: &str) -> String {
    match entry.phase {
        Phase::Moving => format!("{name}: moving {}", windows(entry.count)),
        Phase::LateWindows => format!("{name}: {} now, waiting for the rest", windows(entry.count)),
        _ if entry.expected == 0 => format!(
            "{name}: {} open, waiting for them to settle",
            windows(entry.count)
        ),
        _ if entry.count < entry.expected => format!(
            "{name}: {} of {} opened, waiting for the rest",
            entry.count, entry.expected
        ),
        _ if entry.count == entry.expected => format!(
            "{name}: {} of {} opened, placing them",
            entry.count, entry.expected
        ),
        _ => format!("{name}: {} open, placing them", windows(entry.count)),
    }
}

/// What the panel shows for a few seconds when nothing is active any more:
/// one line and a full bar. None when no app was restored.
pub fn final_line(progress: &[Progress], label: &dyn Fn(&str) -> String) -> Option<Content> {
    let title = final_title(progress, label)?;
    let bar = overall_progress(progress).map(|(_, total)| Bar {
        done: total,
        total,
        finished: true,
    });
    Some(Content {
        title,
        lines: Vec::new(),
        footer: None,
        bar,
    })
}

fn final_title(progress: &[Progress], label: &dyn Fn(&str) -> String) -> Option<String> {
    let mut done: Vec<&Progress> = progress
        .iter()
        .filter(|entry| entry.phase == Phase::Done)
        .collect();
    if done.is_empty() {
        return None;
    }
    let skipped = progress
        .iter()
        .filter(|entry| entry.phase == Phase::Skipped)
        .count();
    if skipped > 0 {
        return Some(format!(
            "{FINAL_PREFIX}Layout restored, {} skipped",
            apps(skipped)
        ));
    }
    done.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then(label(&a.group).cmp(&label(&b.group)))
    });
    let name = label(&done[0].group);
    let biggest = match done[0].count {
        1 => format!("1 {name} window"),
        count => format!("{count} {name} windows"),
    };
    Some(fit(
        &if done.len() == 1 {
            format!("{FINAL_PREFIX}Layout restored: {biggest}")
        } else {
            format!(
                "{FINAL_PREFIX}Layout restored: {biggest}, {}",
                apps(done.len())
            )
        },
        MAX_LINE_CHARS + FINAL_PREFIX.chars().count(),
    ))
}

/// Cuts `text` to `max` characters, ending in an ellipsis when it was cut.
pub fn fit(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(max.saturating_sub(1)).collect();
    cut.push('\u{2026}');
    cut
}

/// The tray tooltip while a restore runs, or None to show the plain name.
pub fn tooltip(progress: &[Progress], label: &dyn Fn(&str) -> String) -> Option<String> {
    let active: Vec<&Progress> = progress
        .iter()
        .filter(|entry| entry.phase.is_active())
        .collect();
    let waiting = progress
        .iter()
        .filter(|entry| entry.phase == Phase::NotOpened)
        .count();
    let skipped: Vec<String> = progress
        .iter()
        .filter(|entry| entry.phase == Phase::Skipped)
        .map(|entry| label(&entry.group))
        .collect();
    let mut parts: Vec<String> = active
        .iter()
        .take(2)
        .map(|entry| {
            let name = label(&entry.group);
            match entry.phase {
                Phase::Moving => format!("{name} moving"),
                Phase::LateWindows => format!("{name} late windows"),
                _ if entry.expected > 0 => {
                    format!("{name} {}/{} settling", entry.count, entry.expected)
                }
                _ => format!("{name} {} settling", entry.count),
            }
        })
        .collect();
    if active.len() > 2 {
        parts.push(format!("+{} more", active.len() - 2));
    }
    if waiting > 0 {
        parts.push(format!("waiting for {} to open", apps(waiting)));
    }
    let text = if !active.is_empty() {
        let count = overall_progress(progress)
            .map(|(done, total)| format!(" ({done} of {})", windows(total)))
            .unwrap_or_default();
        format!(
            "WinCraft \u{2014} restoring layout{count}: {}",
            parts.join("; ")
        )
    } else if !skipped.is_empty() {
        format!(
            "WinCraft \u{2014} layout restored; not opened: {}",
            skipped.join(", ")
        )
    } else if waiting > 0 {
        format!("WinCraft \u{2014} layout restored; {}", parts.join("; "))
    } else {
        return None;
    };
    Some(fit(&text, TIP_CHARS))
}

/// What the panel should do now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Nothing,
    /// Draw this and show it at full strength.
    Show(Content),
    /// Start the fade-out; the panel hides when it ends.
    Fade,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Hidden,
    Active(Content),
    /// The final line is on screen until this tick.
    Final {
        until: u64,
    },
}

/// Decides, once a second, whether the panel is shown, changed, held on its
/// final line, or faded out.
#[derive(Debug, Default)]
pub struct Controller {
    state: State,
}

impl Controller {
    pub fn is_visible(&self) -> bool {
        self.state != State::Hidden
    }

    /// `active` is the panel's content while an app is active, `last` what
    /// to leave behind when the activity ends.
    pub fn update(&mut self, tick: u64, active: Option<Content>, last: Option<Content>) -> Command {
        match (&self.state, active) {
            (State::Active(shown), Some(content)) if *shown == content => Command::Nothing,
            (_, Some(content)) => {
                self.state = State::Active(content.clone());
                Command::Show(content)
            }
            (State::Active(_), None) => match last {
                Some(content) => {
                    self.state = State::Final {
                        until: tick + FINAL_HOLD_SECONDS,
                    };
                    Command::Show(content)
                }
                None => {
                    self.state = State::Hidden;
                    Command::Fade
                }
            },
            (State::Final { until }, None) if tick >= *until => {
                self.state = State::Hidden;
                Command::Fade
            }
            _ => Command::Nothing,
        }
    }

    /// The panel was hidden by something else (the plugin switched off).
    pub fn reset(&mut self) {
        self.state = State::Hidden;
    }
}

/// The popup window.
pub struct Notice {
    window: Layered,
}

impl Notice {
    pub fn create() -> Result<Self, String> {
        Ok(Self {
            window: Layered::create(CLASS_NAME)?,
        })
    }

    /// A panel in the same look that takes clicks: a click posts `message`
    /// to `target`.
    pub fn create_clickable(
        class_name: &str,
        target: windows_sys::Win32::Foundation::HWND,
        message: u32,
    ) -> Result<Self, String> {
        Ok(Self {
            window: Layered::create_clickable(class_name, target, message)?,
        })
    }

    pub fn show(&mut self, content: &Content) {
        // The primary monitor's work area, so the panel sits above the
        // taskbar wherever it is.
        let monitor = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            return;
        }
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
        let scale = dpi_x.max(96) as f32 / 96.0;
        let tokens = theme::Tokens::for_choice(theme::current());
        let Some(image) = render(content, scale, &tokens) else {
            log::warn!(target: "layout_keeper", "could not draw the restore notice");
            return;
        };
        let margin = (MARGIN * scale).round() as i32;
        let work = info.rcWork;
        let at = POINT {
            x: work.right - image.width - margin,
            y: work.bottom - image.height - margin,
        };
        if !self.window.present(&image, at) {
            log::warn!(target: "layout_keeper", "could not show the restore notice");
            return;
        }
        self.window.show();
    }

    pub fn set_alpha(&mut self, alpha: u8) {
        self.window.set_alpha(alpha);
    }

    pub fn hide(&mut self) {
        self.window.hide();
    }

    pub fn destroy(&mut self) {
        self.window.destroy();
    }
}

/// `text`, shortened with an ellipsis until `measure` says it fits in `max`
/// pixels. Cuts by characters, from the end.
fn ellipsize(text: &str, max: i32, measure: &dyn Fn(&str) -> i32) -> String {
    if measure(text) <= max {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    // The longest start of the text that fits together with the ellipsis.
    let (mut low, mut high) = (0usize, chars.len());
    while low < high {
        let middle = (low + high).div_ceil(2);
        let candidate: String = chars[..middle].iter().collect::<String>() + "\u{2026}";
        if measure(&candidate) <= max {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    let cut: String = chars[..low].iter().collect();
    format!("{}\u{2026}", cut.trim_end())
}

/// The app's icon at `side` pixels, as straight-alpha RGBA.
fn app_icon(side: i32) -> Option<icons::Pixels> {
    // The exe's first icon resource, the one the tray's colour icon is.
    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let resource: u32 = 1;
    let icon = unsafe {
        LoadImageW(
            hinstance,
            resource as usize as *const u16,
            IMAGE_ICON,
            side,
            side,
            LR_DEFAULTCOLOR,
        )
    };
    if icon.is_null() {
        return None;
    }
    let pixels = icons::icon_pixels(icon as HICON);
    unsafe { DestroyIcon(icon as HICON) };
    pixels
}

/// Lays a straight-alpha icon over premultiplied BGRA pixels with its top
/// left corner at (x0, y0); what falls outside the image is left out.
fn blend_icon(pixels: &mut [u32], width: i32, height: i32, x0: i32, y0: i32, icon: &icons::Pixels) {
    let side = icon.side as i32;
    for iy in 0..side {
        for ix in 0..side {
            let (x, y) = (x0 + ix, y0 + iy);
            if x < 0 || y < 0 || x >= width || y >= height {
                continue;
            }
            let at = (iy as usize * icon.side + ix as usize) * 4;
            let alpha = f32::from(icon.rgba[at + 3]) / 255.0;
            if alpha == 0.0 {
                continue;
            }
            let below = &mut pixels[(y * width + x) as usize];
            let (below_a, below_r, below_g, below_b) = (
                f32::from((*below >> 24) as u8),
                f32::from((*below >> 16) as u8),
                f32::from((*below >> 8) as u8),
                f32::from(*below as u8),
            );
            let over = |source: u8, under: f32| {
                (f32::from(source) * alpha + under * (1.0 - alpha))
                    .round()
                    .clamp(0.0, 255.0) as u32
            };
            let out_a = (alpha * 255.0 + below_a * (1.0 - alpha))
                .round()
                .clamp(0.0, 255.0) as u32;
            *below = (out_a << 24)
                | (over(icon.rgba[at], below_r) << 16)
                | (over(icon.rgba[at + 1], below_g) << 8)
                | over(icon.rgba[at + 2], below_b);
        }
    }
}

/// How much of the pixel at (x, y) a capsule covers: a line from the left
/// end to the right end of the rectangle, as thick as it is high, with round
/// ends.
fn capsule_coverage(x: i32, y: i32, (left, top, length, thick): (i32, i32, i32, i32)) -> f32 {
    let radius = thick as f32 / 2.0;
    let centre_y = top as f32 + radius;
    let from = left as f32 + radius;
    let to = ((left + length) as f32 - radius).max(from);
    let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
    let dx = cx - cx.clamp(from, to);
    let distance = (dx * dx + (cy - centre_y) * (cy - centre_y)).sqrt();
    (radius + 0.5 - distance).clamp(0.0, 1.0)
}

/// Lays an opaque colour over a premultiplied pixel with `coverage` (0 to 1).
fn blend_over(below: &mut u32, colour: Color32, coverage: f32) {
    if coverage <= 0.0 {
        return;
    }
    let channel = |shift: u32, source: u8| {
        let under = f32::from((*below >> shift) as u8);
        (f32::from(source) * coverage + under * (1.0 - coverage)).round() as u32
    };
    let alpha = (255.0 * coverage + f32::from((*below >> 24) as u8) * (1.0 - coverage)).round();
    *below = ((alpha as u32) << 24)
        | (channel(16, colour.r()) << 16)
        | (channel(8, colour.g()) << 8)
        | channel(0, colour.b());
}

/// The bar: a track with the fill over its left part. A fill that has started
/// is at least as long as the bar is thick, so its ends stay round.
fn draw_bar(
    pixels: &mut [u32],
    width: i32,
    rect: (i32, i32, i32, i32),
    bar: Bar,
    fill: Color32,
    track: Color32,
) {
    let (left, top, length, thick) = rect;
    let share = bar.done.min(bar.total) as f32 / bar.total.max(1) as f32;
    let filled = match (share * length as f32).round() as i32 {
        0 => 0,
        part => part.max(thick).min(length),
    };
    for y in top..top + thick {
        for x in left..left + length {
            let at = &mut pixels[(y * width + x) as usize];
            blend_over(at, track, capsule_coverage(x, y, rect));
            if filled > 0 {
                blend_over(at, fill, capsule_coverage(x, y, (left, top, filled, thick)));
            }
        }
    }
}

fn render(content: &Content, scale: f32, tokens: &theme::Tokens) -> Option<Image> {
    let px = |logical: f32| (logical * scale).round() as i32;
    let [(semi_face, semi_weight), (face, weight)] = *faces();
    let fonts: [HFONT; 3] = [
        font(semi_face, semi_weight, px(TITLE_TEXT)),
        font(face, weight, px(BODY_TEXT)),
        font(face, weight, px(SMALL_TEXT)),
    ];
    let dc = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
    let measure = |text: &[u16], font: HFONT| -> SIZE {
        let mut size = SIZE { cx: 0, cy: 0 };
        unsafe {
            SelectObject(dc, font);
            GetTextExtentPoint32W(dc, text.as_ptr(), text.len() as i32, &mut size);
        }
        size
    };

    // The title sits beside the app's icon; every row is cut to what fits
    // inside the widest the panel may be.
    let icon = app_icon(px(ICON_SIZE));
    let icon_room = icon.as_ref().map_or(0, |_| px(ICON_SIZE + ICON_GAP));
    let inner = px(MAX_WIDTH) - 2 * px(PAD_X);
    let bar_label = content.bar.map(|bar| utf16(&bar.label()));
    let bar_label_size = bar_label.as_ref().map(|text| measure(text, fonts[2]));

    // (text, colour, font, gap above, left offset) in reading order.
    let mut rows: Vec<(String, Color32, usize, i32, i32)> =
        vec![(content.title.clone(), tokens.text_primary, 0, 0, icon_room)];
    for (index, line) in content.lines.iter().enumerate() {
        let gap = if index == 0 { SECTION_GAP } else { LINE_GAP };
        rows.push((line.clone(), tokens.text_primary, 1, px(gap), 0));
    }
    if let Some(footer) = &content.footer {
        rows.push((footer.clone(), tokens.text_secondary, 2, px(SECTION_GAP), 0));
    }
    let rows: Vec<(Vec<u16>, Color32, usize, i32, i32)> = rows
        .into_iter()
        .map(|(text, colour, font, gap, left)| {
            let fitted = ellipsize(&text, inner - left, &|candidate| {
                measure(&utf16(candidate), fonts[font]).cx
            });
            (utf16(&fitted), colour, font, gap, left)
        })
        .collect();
    let sizes: Vec<SIZE> = rows
        .iter()
        .map(|(text, _, font, _, _)| measure(text, fonts[*font]))
        .collect();
    let text_width = rows
        .iter()
        .zip(&sizes)
        .map(|((_, _, _, _, left), size)| left + size.cx)
        .max()
        .unwrap_or(0);
    let bar_width = bar_label_size.map_or(0, |size| px(BAR_MIN_WIDTH) + px(BAR_GAP) + size.cx);
    let width = (text_width.max(bar_width) + 2 * px(PAD_X)).clamp(px(MIN_WIDTH), px(MAX_WIDTH));
    let mut y = px(PAD_Y);
    let mut runs = Vec::new();
    // Left, top, length and thickness of the bar's track.
    let mut bar_rect = None;
    let mut icon_at = (px(PAD_X), y);
    for (index, ((text, colour, font, gap, left), size)) in rows.into_iter().zip(&sizes).enumerate()
    {
        y += gap;
        // The title row is as tall as the icon beside it.
        let row_height = if index == 0 && icon.is_some() {
            size.cy.max(px(ICON_SIZE))
        } else {
            size.cy
        };
        if index == 0 {
            icon_at = (px(PAD_X), y + (row_height - px(ICON_SIZE)) / 2);
        }
        runs.push(Run {
            text,
            colour,
            x: px(PAD_X) + left,
            y: y + (row_height - size.cy) / 2,
            width: size.cx,
            height: size.cy,
            font,
        });
        y += row_height;
        if let (0, Some(label), Some(size)) = (index, &bar_label, bar_label_size) {
            y += px(SECTION_GAP);
            let label_x = width - px(PAD_X) - size.cx;
            let thick = px(BAR_HEIGHT);
            bar_rect = Some((
                px(PAD_X),
                y + (size.cy - thick) / 2,
                label_x - px(BAR_GAP) - px(PAD_X),
                thick,
            ));
            runs.push(Run {
                text: label.clone(),
                colour: tokens.text_secondary,
                x: label_x,
                y,
                width: size.cx,
                height: size.cy,
                font: 2,
            });
            y += size.cy;
        }
    }
    let height = y + px(PAD_Y);

    let mask = text_mask(dc, width, height, &runs, &fonts);
    unsafe {
        DeleteDC(dc);
        for font in fonts {
            DeleteObject(font);
        }
    }
    let mask = mask?;
    let mut pixels = compose(
        width,
        height,
        RADIUS * scale,
        scale.round().max(1.0),
        tokens.elevated_bg,
        tokens.border,
        |x, y| {
            let coverage = f32::from(mask[(y * width + x) as usize]) / 255.0;
            let colour = runs
                .iter()
                .find(|run| {
                    x >= run.x && x < run.x + run.width && y >= run.y && y < run.y + run.height
                })
                .map(|run| run.colour)
                .unwrap_or(tokens.text_primary);
            (coverage, colour)
        },
    );
    if let (Some(bar), Some(rect)) = (content.bar, bar_rect) {
        let fill = if bar.finished {
            tokens.success
        } else {
            tokens.accent
        };
        draw_bar(&mut pixels, width, rect, bar, fill, tokens.border);
    }
    if let Some(icon) = &icon {
        blend_icon(&mut pixels, width, height, icon_at.0, icon_at.1, icon);
    }
    Some(Image {
        width,
        height,
        pixels,
    })
}

/// Sample progress for `--show-restore-panel`: Chrome is taking windows,
/// Claude is being moved, three apps are done, one has not opened.
pub fn sample() -> Vec<Progress> {
    let entry = |group: &str, phase, count, expected| Progress {
        group: group.to_string(),
        phase,
        count,
        expected,
    };
    vec![
        entry("Chrome", Phase::Settling, 12, 22),
        entry("Claude", Phase::Moving, 4, 4),
        entry("Telegram", Phase::Done, 1, 1),
        entry("Terminal", Phase::Done, 3, 3),
        entry("Notes", Phase::Done, 2, 2),
        entry("Viber", Phase::NotOpened, 0, 1),
    ]
}

/// Labels for the sample: the group is its own label.
pub fn same_label(group: &str) -> String {
    group.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::ThemeChoice;

    fn title_of(content: Option<Content>) -> Option<String> {
        content.map(|content| content.title)
    }

    fn progress(group: &str, phase: Phase, count: usize, expected: usize) -> Progress {
        Progress {
            group: group.to_string(),
            phase,
            count,
            expected,
        }
    }

    fn name(group: &str) -> String {
        group.to_string()
    }

    #[test]
    fn nothing_is_shown_for_apps_that_have_not_opened_a_window() {
        let list = [
            progress("Chrome", Phase::NotOpened, 0, 22),
            progress("Viber", Phase::NotOpened, 0, 1),
            progress("Telegram", Phase::Done, 1, 1),
        ];
        assert_eq!(active_content(&list, &name), None);
        assert_eq!(active_content(&[], &name), None);
    }

    #[test]
    fn each_active_app_gets_a_line_and_the_finished_ones_a_summary() {
        let list = [
            progress("Chrome", Phase::Settling, 12, 22),
            progress("Claude", Phase::Moving, 4, 4),
            progress("Notes", Phase::Settling, 1, 0),
            progress("Telegram", Phase::Done, 1, 1),
            progress("Terminal", Phase::Done, 3, 3),
            progress("Code", Phase::Done, 2, 2),
            progress("Viber", Phase::NotOpened, 0, 1),
        ];
        let content = active_content(&list, &name).unwrap();
        assert_eq!(
            content.title,
            "WinCraft \u{b7} Putting your windows back \u{2014} please wait"
        );
        assert_eq!(
            content.lines,
            [
                "Claude: moving 4 windows",
                "Chrome: 12 of 22 opened, waiting for the rest",
                "Notes: 1 window open, waiting for them to settle",
            ]
        );
        assert_eq!(content.footer.as_deref(), Some("Done: Terminal, Code, +1"));
    }

    #[test]
    fn at_most_five_apps_are_listed_then_a_count() {
        let list: Vec<Progress> = (0..8)
            .map(|n| progress(&format!("App{n}"), Phase::Settling, 8 - n, 0))
            .collect();
        let content = active_content(&list, &name).unwrap();
        assert_eq!(content.lines.len(), 6);
        assert!(content.lines[0].starts_with("App0"));
        assert_eq!(content.lines[5], "+3 more");
        assert_eq!(content.footer, None);
    }

    #[test]
    fn late_windows_are_a_line_too() {
        let list = [progress("Chrome", Phase::LateWindows, 23, 22)];
        let content = active_content(&list, &name).unwrap();
        assert_eq!(
            content.lines,
            ["Chrome: 23 windows now, waiting for the rest"]
        );
    }

    #[test]
    fn the_final_line_names_the_biggest_app_and_counts_the_apps() {
        let list = [
            progress("Chrome", Phase::Done, 22, 22),
            progress("Telegram", Phase::Done, 1, 1),
            progress("Notes", Phase::Done, 1, 1),
        ];
        assert_eq!(
            title_of(final_line(&list, &name)).as_deref(),
            Some("WinCraft \u{b7} Layout restored: 22 Chrome windows, 3 apps")
        );
        assert_eq!(
            title_of(final_line(&[progress("Chrome", Phase::Done, 1, 1)], &name)).as_deref(),
            Some("WinCraft \u{b7} Layout restored: 1 Chrome window")
        );
        assert_eq!(
            title_of(final_line(
                &[progress("Chrome", Phase::Done, 22, 22)],
                &name
            ))
            .as_deref(),
            Some("WinCraft \u{b7} Layout restored: 22 Chrome windows")
        );
        assert_eq!(final_line(&[], &name), None);
        assert_eq!(
            final_line(&[progress("Viber", Phase::Skipped, 0, 1)], &name),
            None
        );
    }

    #[test]
    fn skipped_apps_change_the_final_line_and_go_in_the_tooltip() {
        let list = [
            progress("Chrome", Phase::Done, 22, 22),
            progress("Viber", Phase::Skipped, 0, 1),
            progress("Zoom", Phase::Skipped, 0, 1),
        ];
        assert_eq!(
            title_of(final_line(&list, &name)).as_deref(),
            Some("WinCraft \u{b7} Layout restored, 2 apps skipped")
        );
        assert_eq!(
            tooltip(&list, &name).as_deref(),
            Some("WinCraft \u{2014} layout restored; not opened: Viber, Zoom")
        );
    }

    #[test]
    fn the_tooltip_says_what_is_settling_and_what_is_awaited() {
        let list = [
            progress("Chrome", Phase::Settling, 12, 22),
            progress("Viber", Phase::NotOpened, 0, 1),
            progress("Zoom", Phase::NotOpened, 0, 1),
            progress("Slack", Phase::NotOpened, 0, 1),
        ];
        assert_eq!(
            tooltip(&list, &name).as_deref(),
            Some(
                "WinCraft \u{2014} restoring layout (12 of 25 windows): Chrome 12/22 settling; waiting for 3 apps to open"
            )
        );
        let only_waiting = [
            progress("Telegram", Phase::Done, 1, 1),
            progress("Viber", Phase::NotOpened, 0, 1),
        ];
        assert_eq!(
            tooltip(&only_waiting, &name).as_deref(),
            Some("WinCraft \u{2014} layout restored; waiting for 1 app to open")
        );
        assert_eq!(tooltip(&[progress("A", Phase::Done, 1, 1)], &name), None);
    }

    #[test]
    fn a_tooltip_never_passes_the_windows_limit() {
        let long = "x".repeat(200);
        let list = [
            progress(&long, Phase::Settling, 1, 2),
            progress("B", Phase::Settling, 1, 2),
            progress("C", Phase::Settling, 1, 2),
        ];
        let tip = tooltip(&list, &name).unwrap();
        assert_eq!(tip.chars().count(), 127);
        assert!(tip.ends_with('\u{2026}'));
        assert_eq!(fit("short", 127), "short");
    }

    fn active() -> Content {
        Content {
            title: "t".to_string(),
            lines: vec!["Chrome: moving 4 windows".to_string()],
            footer: None,
            bar: None,
        }
    }

    #[test]
    fn the_panel_shows_while_active_holds_a_last_line_and_then_fades() {
        let mut ctl = Controller::default();
        assert!(!ctl.is_visible());
        assert_eq!(ctl.update(1, None, None), Command::Nothing);

        // Active: shown once, then left alone while the text is the same.
        assert_eq!(ctl.update(2, Some(active()), None), Command::Show(active()));
        assert!(ctl.is_visible());
        assert_eq!(ctl.update(3, Some(active()), None), Command::Nothing);
        let mut changed = active();
        changed.lines[0] = "Chrome: moving 5 windows".to_string();
        assert_eq!(
            ctl.update(4, Some(changed.clone()), None),
            Command::Show(changed)
        );

        // Nothing active any more: the last line stays for four seconds.
        let line = Content {
            title: "Layout restored: 5 Chrome windows".to_string(),
            lines: Vec::new(),
            footer: None,
            bar: None,
        };
        let shown = ctl.update(5, None, Some(line.clone()));
        assert_eq!(shown, Command::Show(line.clone()));
        assert_eq!(ctl.update(6, None, Some(line.clone())), Command::Nothing);
        assert_eq!(ctl.update(8, None, Some(line.clone())), Command::Nothing);
        assert_eq!(ctl.update(9, None, Some(line)), Command::Fade);
        assert!(!ctl.is_visible());
        assert_eq!(ctl.update(10, None, None), Command::Nothing);
    }

    #[test]
    fn without_a_last_line_the_panel_fades_at_once() {
        let mut ctl = Controller::default();
        ctl.update(1, Some(active()), None);
        assert_eq!(ctl.update(2, None, None), Command::Fade);
        assert!(!ctl.is_visible());
    }

    #[test]
    fn new_activity_during_the_last_line_brings_the_panel_back() {
        let mut ctl = Controller::default();
        ctl.update(1, Some(active()), None);
        ctl.update(
            2,
            None,
            Some(Content {
                title: "done".to_string(),
                ..active()
            }),
        );
        assert_eq!(ctl.update(3, Some(active()), None), Command::Show(active()));
        ctl.reset();
        assert!(!ctl.is_visible());
    }

    #[test]
    fn the_sample_has_every_kind_of_line() {
        let content = active_content(&sample(), &same_label).unwrap();
        assert_eq!(content.lines.len(), 2);
        assert!(content.footer.is_some());
        assert!(tooltip(&sample(), &same_label).is_some());
    }

    #[test]
    fn the_panel_is_drawn_wide_enough_for_its_text() {
        let tokens = theme::Tokens::for_choice(ThemeChoice::Dark);
        let content = active_content(&sample(), &same_label).unwrap();
        let image = render(&content, 1.0, &tokens).expect("GDI draws the panel");
        assert!(image.width >= MIN_WIDTH as i32);
        assert!(image.height > 60, "{}", image.height);
        assert_eq!(image.pixels.len(), (image.width * image.height) as usize);
        // The middle of the panel is translucent but not empty.
        let centre = image.pixels[(image.height / 2 * image.width + 2) as usize];
        assert_ne!(centre >> 24, 0);
    }

    #[test]
    fn the_wording_follows_how_many_windows_the_saved_layout_expects() {
        let line = |count, expected| {
            let list = [progress("Chrome", Phase::Settling, count, expected)];
            active_content(&list, &name).unwrap().lines.remove(0)
        };
        assert_eq!(
            line(12, 22),
            "Chrome: 12 of 22 opened, waiting for the rest"
        );
        // All of them are there: nothing is being waited for.
        assert_eq!(line(22, 22), "Chrome: 22 of 22 opened, placing them");
        assert_eq!(line(23, 22), "Chrome: 23 windows open, placing them");
        // The saved layout says nothing about this app.
        assert_eq!(
            line(5, 0),
            "Chrome: 5 windows open, waiting for them to settle"
        );
        assert_eq!(
            line(1, 0),
            "Chrome: 1 window open, waiting for them to settle"
        );
        let moving = [progress("Chrome", Phase::Moving, 22, 22)];
        assert_eq!(
            active_content(&moving, &name).unwrap().lines,
            ["Chrome: moving 22 windows"]
        );
    }

    #[test]
    fn a_long_name_is_cut_with_an_ellipsis() {
        let long = "x".repeat(200);
        let list = [progress(&long, Phase::Settling, 3, 9)];
        let lines = active_content(&list, &name).unwrap().lines;
        assert_eq!(lines[0].chars().count(), MAX_LINE_CHARS);
        assert!(lines[0].ends_with('\u{2026}'));
        let final_line = final_line(&[progress(&long, Phase::Done, 3, 3)], &name)
            .unwrap()
            .title;
        assert!(final_line.starts_with("WinCraft \u{b7} Layout restored: "));
        assert!(final_line.chars().count() <= MAX_LINE_CHARS + FINAL_PREFIX.chars().count());
    }

    #[test]
    fn text_is_cut_to_the_pixels_there_are() {
        // Seven pixels a character.
        let measure = |text: &str| text.chars().count() as i32 * 7;
        assert_eq!(ellipsize("short", 100, &measure), "short");
        let cut = ellipsize("a rather long line of text", 70, &measure);
        assert_eq!(cut, "a rather\u{2026}");
        assert!(measure(&cut) <= 70);
        // Nothing fits but the ellipsis.
        assert_eq!(ellipsize("abc", 3, &measure), "\u{2026}");
        assert_eq!(ellipsize("", 0, &measure), "");
    }

    #[test]
    fn the_panel_never_passes_its_widest() {
        let tokens = theme::Tokens::for_choice(ThemeChoice::Dark);
        let long = "a very long app name ".repeat(20);
        let content = Content {
            title: TITLE.to_string(),
            lines: vec![long.clone()],
            footer: Some(long),
            bar: Some(Bar {
                done: 3,
                total: 10,
                finished: false,
            }),
        };
        for scale in [1.0, 1.5, 2.0] {
            let image = render(&content, scale, &tokens).expect("GDI draws the panel");
            assert!(
                image.width <= (MAX_WIDTH * scale).round() as i32,
                "{scale}: {}",
                image.width
            );
        }
    }

    #[test]
    fn the_icon_is_laid_over_the_panel_with_its_alpha() {
        // A 2x2 panel pixel row, premultiplied: opaque grey.
        let grey = 0xFF80_8080u32;
        let mut pixels = vec![grey; 4];
        let icon = icons::Pixels {
            side: 2,
            rgba: vec![
                255, 0, 0, 255, // opaque red
                0, 255, 0, 0, // clear
                0, 0, 255, 128, // half blue
                0, 0, 0, 0, // clear
            ],
        };
        blend_icon(&mut pixels, 2, 2, 0, 0, &icon);
        assert_eq!(pixels[0], 0xFFFF_0000);
        assert_eq!(pixels[1], grey);
        let half = pixels[2];
        assert_eq!(half >> 24, 0xFF);
        assert!((half & 0xFF) > 0xB0, "blue is mostly there: {half:08x}");
        assert!(
            ((half >> 16) & 0xFF) < 0x50,
            "red is mostly gone: {half:08x}"
        );
        assert_eq!(pixels[3], grey);
        // An icon hanging off the edge only draws what is inside.
        let mut edge = vec![grey; 4];
        blend_icon(&mut edge, 2, 2, 1, 1, &icon);
        assert_eq!(edge[3], 0xFFFF_0000);
        assert_eq!(edge[0], grey);
    }

    #[test]
    fn the_panel_has_the_app_icon_beside_its_title() {
        // The exe in a test run has no icon resource, so the panel is drawn
        // without it, and the same code path is exercised either way.
        let tokens = theme::Tokens::for_choice(ThemeChoice::Light);
        let content = active_content(&sample(), &same_label).unwrap();
        let image = render(&content, 1.0, &tokens).expect("GDI draws the panel");
        assert!(image.width >= MIN_WIDTH as i32);
        assert!(content.title.starts_with("WinCraft"));
    }

    #[test]
    fn the_bar_sums_windows_over_all_apps_and_counts_finished_apps_in_full() {
        let list = [
            progress("Chrome", Phase::Settling, 12, 22),
            progress("Claude", Phase::Moving, 4, 4),
            progress("Telegram", Phase::Done, 1, 1),
            progress("Viber", Phase::NotOpened, 0, 1),
            progress("Zoom", Phase::Skipped, 0, 3),
        ];
        // Skipped windows are not expected any more: 12 + 4 + 1 + 0 of 22 + 4 + 1 + 1.
        assert_eq!(overall_progress(&list), Some((17, 28)));
        let content = active_content(&list, &name).unwrap();
        assert_eq!(
            content.bar,
            Some(Bar {
                done: 17,
                total: 28,
                finished: false
            })
        );
    }

    #[test]
    fn the_bar_clamps_an_app_that_has_more_windows_than_saved() {
        let list = [progress("Chrome", Phase::Settling, 30, 22)];
        assert_eq!(overall_progress(&list), Some((22, 30)));
        // An app with no saved count has nothing put back until it is restored.
        let unknown = [progress("Notes", Phase::Settling, 2, 0)];
        assert_eq!(overall_progress(&unknown), Some((0, 2)));
        let late = [progress("Chrome", Phase::LateWindows, 23, 22)];
        assert_eq!(overall_progress(&late), Some((23, 23)));
    }

    #[test]
    fn there_is_no_bar_when_no_window_is_expected() {
        assert_eq!(overall_progress(&[]), None);
        let list = [
            progress("Viber", Phase::Skipped, 0, 1),
            progress("Zoom", Phase::NotOpened, 0, 0),
        ];
        assert_eq!(overall_progress(&list), None);
    }

    #[test]
    fn the_first_number_only_grows_as_windows_open_and_apps_finish() {
        let steps = [
            [
                progress("A", Phase::NotOpened, 0, 4),
                progress("B", Phase::NotOpened, 0, 2),
            ],
            [
                progress("A", Phase::Settling, 2, 4),
                progress("B", Phase::NotOpened, 0, 2),
            ],
            [
                progress("A", Phase::Moving, 4, 4),
                progress("B", Phase::Settling, 1, 2),
            ],
            [
                progress("A", Phase::Done, 4, 4),
                progress("B", Phase::Done, 2, 2),
            ],
        ];
        let done: Vec<usize> = steps
            .iter()
            .map(|step| overall_progress(step).unwrap().0)
            .collect();
        assert_eq!(done, [0, 2, 5, 6]);
    }

    #[test]
    fn the_final_state_has_a_full_finished_bar_and_the_tooltip_counts_windows() {
        let list = [
            progress("Chrome", Phase::Done, 22, 22),
            progress("Viber", Phase::Skipped, 0, 1),
        ];
        let last = final_line(&list, &name).unwrap();
        assert_eq!(
            last.bar,
            Some(Bar {
                done: 22,
                total: 22,
                finished: true
            })
        );
        assert!(last.lines.is_empty());
        assert_eq!(
            Bar {
                done: 1,
                total: 1,
                finished: false
            }
            .label(),
            "1 of 1 window"
        );
        assert_eq!(
            Bar {
                done: 7,
                total: 10,
                finished: false
            }
            .label(),
            "7 of 10 windows"
        );
    }

    #[test]
    fn the_bar_is_drawn_in_the_track_and_fill_colours() {
        let tokens = theme::Tokens::for_choice(ThemeChoice::Dark);
        let with = |bar| Content {
            bar,
            ..active_content(&sample(), &same_label).unwrap()
        };
        let bar = Bar {
            done: 5,
            total: 10,
            finished: false,
        };
        let plain = render(&with(None), 1.0, &tokens).expect("GDI draws the panel");
        let barred = render(&with(Some(bar)), 1.0, &tokens).expect("GDI draws the panel");
        assert!(barred.height > plain.height);
        assert_eq!(barred.pixels.len(), (barred.width * barred.height) as usize);

        let mut pixels = vec![0u32; 40 * 6];
        draw_bar(
            &mut pixels,
            40,
            (4, 1, 32, 4),
            bar,
            tokens.accent,
            tokens.border,
        );
        let at = |x: i32, y: i32| pixels[(y * 40 + x) as usize];
        let rgb = |colour: Color32| {
            (u32::from(colour.r()) << 16) | (u32::from(colour.g()) << 8) | u32::from(colour.b())
        };
        assert_eq!(at(10, 3) & 0xFF_FFFF, rgb(tokens.accent));
        assert_eq!(at(30, 3) & 0xFF_FFFF, rgb(tokens.border));
        // Outside the bar nothing is drawn, and the ends are rounded.
        assert_eq!(at(10, 0), 0);
        assert_eq!(at(10, 5), 0);
        assert!(at(4, 1) >> 24 < 0xFF);
        // A full bar is filled to the far end.
        let mut full = vec![0u32; 40 * 6];
        let done = Bar {
            done: 10,
            total: 10,
            finished: true,
        };
        draw_bar(
            &mut full,
            40,
            (4, 1, 32, 4),
            done,
            tokens.success,
            tokens.border,
        );
        assert_eq!(full[3 * 40 + 30] & 0xFF_FFFF, rgb(tokens.success));
    }
}
