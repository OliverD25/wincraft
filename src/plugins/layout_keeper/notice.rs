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
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

use super::restore::{Phase, Progress};
use crate::core::layered::{compose, faces, font, text_mask, utf16, Image, Layered, Run};
use crate::core::theme;

const CLASS_NAME: &str = "WinCraftRestoreNotice";
const TITLE: &str = "Putting your windows back \u{2014} please wait";
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

/// What the panel says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Content {
    pub title: String,
    pub lines: Vec<String>,
    pub footer: Option<String>,
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
            match entry.phase {
                Phase::Moving => format!("{name}: moving {}", windows(entry.count)),
                Phase::LateWindows => {
                    format!("{name}: {} now, waiting for the rest", windows(entry.count))
                }
                _ if entry.expected > entry.count => format!(
                    "{name}: {} of {} opened, waiting for the rest",
                    entry.count, entry.expected
                ),
                _ => format!(
                    "{name}: {} opened, waiting for the rest",
                    windows(entry.count)
                ),
            }
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
    })
}

/// The line shown for a few seconds when nothing is active any more, or None
/// when no app was restored.
pub fn final_line(progress: &[Progress], label: &dyn Fn(&str) -> String) -> Option<String> {
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
        return Some(format!("Layout restored, {} skipped", apps(skipped)));
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
    Some(if done.len() == 1 {
        format!("Layout restored: {biggest}")
    } else {
        format!("Layout restored: {biggest}, {}", apps(done.len()))
    })
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
        format!("WinCraft \u{2014} restoring layout: {}", parts.join("; "))
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

    /// `active` is the panel's content while an app is active, `last` the
    /// line to leave behind when the activity ends.
    pub fn update(&mut self, tick: u64, active: Option<Content>, last: Option<String>) -> Command {
        match (&self.state, active) {
            (State::Active(shown), Some(content)) if *shown == content => Command::Nothing,
            (_, Some(content)) => {
                self.state = State::Active(content.clone());
                Command::Show(content)
            }
            (State::Active(_), None) => match last {
                Some(line) => {
                    self.state = State::Final {
                        until: tick + FINAL_HOLD_SECONDS,
                    };
                    Command::Show(Content {
                        title: line,
                        lines: Vec::new(),
                        footer: None,
                    })
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

    // (text, colour, font) in reading order, with the gap above each.
    let mut rows: Vec<(Vec<u16>, Color32, usize, i32)> = Vec::new();
    rows.push((utf16(&content.title), tokens.text_primary, 0, 0));
    for (index, line) in content.lines.iter().enumerate() {
        let gap = if index == 0 { SECTION_GAP } else { LINE_GAP };
        rows.push((utf16(line), tokens.text_primary, 1, px(gap)));
    }
    if let Some(footer) = &content.footer {
        rows.push((utf16(footer), tokens.text_secondary, 2, px(SECTION_GAP)));
    }
    let sizes: Vec<SIZE> = rows
        .iter()
        .map(|(text, _, font, _)| measure(text, fonts[*font]))
        .collect();
    let text_width = sizes.iter().map(|size| size.cx).max().unwrap_or(0);
    let width = (text_width + 2 * px(PAD_X)).max(px(MIN_WIDTH));
    let mut y = px(PAD_Y);
    let mut runs = Vec::new();
    for ((text, colour, font, gap), size) in rows.into_iter().zip(&sizes) {
        y += gap;
        runs.push(Run {
            text,
            colour,
            x: px(PAD_X),
            y,
            width: size.cx,
            height: size.cy,
            font,
        });
        y += size.cy;
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
    let pixels = compose(
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
            "Putting your windows back \u{2014} please wait"
        );
        assert_eq!(
            content.lines,
            [
                "Claude: moving 4 windows",
                "Chrome: 12 of 22 opened, waiting for the rest",
                "Notes: 1 window opened, waiting for the rest",
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
            final_line(&list, &name).as_deref(),
            Some("Layout restored: 22 Chrome windows, 3 apps")
        );
        assert_eq!(
            final_line(&[progress("Chrome", Phase::Done, 1, 1)], &name).as_deref(),
            Some("Layout restored: 1 Chrome window")
        );
        assert_eq!(
            final_line(&[progress("Chrome", Phase::Done, 22, 22)], &name).as_deref(),
            Some("Layout restored: 22 Chrome windows")
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
            final_line(&list, &name).as_deref(),
            Some("Layout restored, 2 apps skipped")
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
                "WinCraft \u{2014} restoring layout: Chrome 12/22 settling; waiting for 3 apps to open"
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
        let line = "Layout restored: 5 Chrome windows".to_string();
        let shown = ctl.update(5, None, Some(line.clone()));
        assert_eq!(
            shown,
            Command::Show(Content {
                title: line.clone(),
                lines: Vec::new(),
                footer: None
            })
        );
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
        ctl.update(2, None, Some("done".to_string()));
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
}
