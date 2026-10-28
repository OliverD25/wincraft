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
    /// The fill never drops below this, in thousandths: the total grows when
    /// another app opens its first window, and the bar must not run back.
    /// The words always say the true numbers.
    pub least: u16,
    pub kind: BarKind,
}

/// What a bar shows besides its fill.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BarKind {
    /// "7 of 10 windows" at the right end of the bar's row.
    #[default]
    Windows,
    /// No words: the line under the title says the share.
    Plain,
    /// The end is not known: a short piece slides along the track. The
    /// number is how far one sweep has gone, in thousandths.
    Moving(u16),
}

/// The longest the panel stays up after the last window was placed. Every
/// line has its own cap too; this one makes sure nothing at all, not even a
/// group stuck in some state, keeps the panel on screen.
pub const PANEL_MAX_SECONDS: u64 = 60;

/// Whether the panel may show activity at `tick`, given when a window was
/// last placed (None: nothing placed yet in this run) and whether it has
/// already shown and gone in this run. It shows at most once per restore:
/// windows the user opens later are placed without it.
pub fn panel_open(tick: u64, last_placed: Option<u64>, closed: bool) -> bool {
    !closed && last_placed.is_none_or(|at| tick.saturating_sub(at) <= PANEL_MAX_SECONDS)
}

/// `bar` with its fill held at the highest share shown so far in this run,
/// which `floor` keeps.
pub fn steady(bar: Option<Bar>, floor: &mut u16) -> Option<Bar> {
    bar.map(|mut bar| {
        let share = (bar.done.min(bar.total) * 1000 / bar.total.max(1)) as u16;
        *floor = (*floor).max(share);
        bar.least = *floor;
        bar
    })
}

impl Bar {
    fn label(self) -> Option<String> {
        (self.kind == BarKind::Windows).then(|| format!("{} of {}", self.done, windows(self.total)))
    }
}

/// Windows put back and windows expected, summed over every app that has
/// opened a window and is not skipped. Apps that have opened nothing do not
/// count, so the bar can reach its end while they stay in the tray. An app
/// that is restored counts in full, so the first number only grows as the
/// run goes on; an app still opening counts the windows it has, never more
/// than the saved layout holds. None when no window is expected.
pub fn overall_progress(progress: &[Progress]) -> Option<(usize, usize)> {
    let (mut done, mut total) = (0, 0);
    for entry in progress {
        if matches!(entry.phase, Phase::Skipped | Phase::NotOpened) {
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
        least: 0,
        kind: BarKind::Windows,
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
        least: 1000,
        kind: BarKind::Windows,
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
        .filter(|entry| matches!(entry.phase, Phase::NotOpened | Phase::Quiet))
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
        self.show_near(content, POINT { x: 0, y: 0 });
    }

    /// The panel at the bottom right of the monitor that holds `point`,
    /// above its taskbar.
    pub fn show_near(&mut self, content: &Content, point: POINT) {
        let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTOPRIMARY) };
        let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            return;
        }
        let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
        unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
        let scale = dpi_x.max(96) as f32 / 96.0;
        let tokens = theme::current_tokens();
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

    pub fn hwnd(&self) -> windows_sys::Win32::Foundation::HWND {
        self.window.hwnd()
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
    let filled = fill_span(bar, length, thick);
    for y in top..top + thick {
        for x in left..left + length {
            let at = &mut pixels[(y * width + x) as usize];
            blend_over(at, track, capsule_coverage(x, y, rect));
            if let Some((from, long)) = filled {
                blend_over(
                    at,
                    fill,
                    capsule_coverage(x, y, (left + from, top, long, thick)),
                );
            }
        }
    }
}

/// Where the fill sits in a track `length` long: its start from the
/// track's left end and its length, None for no fill. A fill that has
/// started is at least as long as the bar is thick, so its ends stay round.
/// A moving bar's piece is a third of the track; it slides in from the left
/// and out at the right, cut where it leaves the track.
fn fill_span(bar: Bar, length: i32, thick: i32) -> Option<(i32, i32)> {
    if let BarKind::Moving(phase) = bar.kind {
        let piece = (length / 3).max(thick);
        let start = -piece + (length + piece) * i32::from(phase.min(1000)) / 1000;
        let (from, to) = (start.max(0), (start + piece).min(length));
        return (to - from > 0).then_some((from, (to - from).max(thick.min(length))));
    }
    let share = (bar.done.min(bar.total) as f32 / bar.total.max(1) as f32)
        .max(f32::from(bar.least.min(1000)) / 1000.0);
    match (share * length as f32).round() as i32 {
        0 => None,
        part => Some((0, part.max(thick).min(length))),
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
    let bar_label = content.bar.and_then(Bar::label).map(|label| utf16(&label));
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
    let bar_width = match (content.bar, bar_label_size) {
        (Some(_), Some(size)) => px(BAR_MIN_WIDTH) + px(BAR_GAP) + size.cx,
        (Some(_), None) => px(BAR_MIN_WIDTH),
        (None, _) => 0,
    };
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
        if index == 0 && content.bar.is_some() && bar_label.is_none() {
            // A bar without words takes the whole row.
            y += px(SECTION_GAP);
            let (row, thick) = (px(SMALL_TEXT), px(BAR_HEIGHT));
            bar_rect = Some((
                px(PAD_X),
                y + (row - thick) / 2,
                width - 2 * px(PAD_X),
                thick,
            ));
            y += row;
        }
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
mod tests;
