//! The decisions and sums behind the hide test, as plain data. Nothing here
//! calls Windows, so all of it is tested on made-up measurements.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::super::geometry::Rect;
pub use crate::core::taskbar_popup::keys::{
    decide, Decision, Inputs, Reason, POPUP_CLASS, POPUP_TITLE,
};
pub use crate::core::taskbar_popup::Method;

pub const DEFAULT_SECONDS: u64 = 60;
pub const MIN_SECONDS: u64 = 5;
pub const MAX_SECONDS: u64 = 600;
const FLAG: &str = "--test-hide-popup";
const DRY_RUN: &str = "--dry-run";

/// Where method (c) puts the popup. Windows itself parks minimized windows
/// at -32000, so nothing on a real monitor is ever there.
pub const OFFSCREEN: (i32, i32) = (-32000, -32000);

/// A hide must show within this long to count as working.
pub const EFFECT_MS: u64 = 50;
/// A show this soon after a hide is Explorer showing it again.
pub const RESHOW_MS: u64 = 1000;
/// The longest the last part of the run, which only watches, may be.
pub const WATCH_MS: u64 = 10_000;
pub const CLEAN_MEDIAN_MS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    pub seconds: u64,
    pub dry_run: bool,
}

/// `None` without the flag, else the options or why they cannot be read.
/// `--dry-run` counts only together with `--test-hide-popup`.
pub fn options_from_args(args: &[String]) -> Option<Result<Options, String>> {
    let dry_run = args.iter().skip(1).any(|arg| arg == DRY_RUN);
    args.iter().skip(1).find_map(|arg| {
        if arg == FLAG {
            return Some(Ok(Options {
                seconds: DEFAULT_SECONDS,
                dry_run,
            }));
        }
        let value = arg.strip_prefix(FLAG)?.strip_prefix('=')?;
        Some(
            value
                .trim()
                .parse::<u64>()
                .ok()
                .filter(|seconds| (MIN_SECONDS..=MAX_SECONDS).contains(seconds))
                .map(|seconds| Options { seconds, dry_run })
                .ok_or_else(|| {
                    format!(
                        "expected {FLAG}=<seconds> from {MIN_SECONDS} to {MAX_SECONDS}, got {value}"
                    )
                }),
        )
    })
}

/// How long the last part of the run only watches: 10 s, or half of a
/// shorter run.
pub fn watch_ms(seconds: u64) -> u64 {
    WATCH_MS.min(seconds * 1000 / 2)
}

pub fn is_offscreen(rect: Rect) -> bool {
    rect[0] <= OFFSCREEN.0 + 1000 && rect[1] <= OFFSCREEN.1 + 1000
}

/// Whether a hide shows: the window is no longer visible, cloaked, or
/// parked off-screen.
pub fn took_effect(visible: bool, cloak: u32, rect: Rect) -> bool {
    !visible || cloak != 0 || is_offscreen(rect)
}

/// Tries the methods in order and keeps the first that works for the rest
/// of the run.
#[derive(Debug, Default)]
pub struct Picker {
    winner: Option<Method>,
    next: usize,
    pub tried: Vec<(Method, bool)>,
}

impl Picker {
    /// The method to use now, or `None` when every one has failed.
    pub fn current(&self) -> Option<Method> {
        self.winner.or_else(|| Method::ALL.get(self.next).copied())
    }

    pub fn winner(&self) -> Option<Method> {
        self.winner
    }

    pub fn result(&mut self, method: Method, worked: bool) {
        self.tried.push((method, worked));
        if worked {
            self.winner = Some(method);
        } else if self.winner.is_none() && Method::ALL.get(self.next) == Some(&method) {
            self.next += 1;
        }
    }
}

/// The middle value, the mean of the two middle ones for an even count.
pub fn median(values: &[u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let mid = sorted.len() / 2;
    Some(if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2
    } else {
        sorted[mid]
    })
}

/// One popup a hover opened, and what the test did about it.
#[derive(Debug, Clone, Default)]
pub struct Hover {
    pub app: String,
    pub method: Option<Method>,
    /// From first seen visible to hidden.
    pub visible_ms: Option<u64>,
    pub reshows: u32,
    pub cursor_still_on_button: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Clean,
    Flashes,
    FightsBack,
    NotAllowed,
    NoHover,
    NotSeenAfter,
    DryRun,
}

impl Verdict {
    pub fn text(self) -> &'static str {
        match self {
            Verdict::Clean => {
                "clean: hidden before it could be seen, and Explorer still shows it normally"
            }
            Verdict::Flashes => {
                "flashes: hiding works, but the popup is on screen long enough to see"
            }
            Verdict::FightsBack => {
                "fights back: Explorer shows the popup again after it was hidden"
            }
            Verdict::NotAllowed => "not allowed: no method hid the popup",
            Verdict::NoHover => "no answer: no hover over a taskbar app button was seen",
            Verdict::NotSeenAfter => {
                "no answer yet: hiding works, but the popup was not seen in the last part, \
                 so it is not proven that Explorer still shows it"
            }
            Verdict::DryRun => "dry run: nothing was hidden",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Totals {
    pub dry_run: bool,
    pub hovers: Vec<Hover>,
    pub tried: Vec<(Method, bool)>,
    pub winner: Option<Method>,
    pub after_shows: u32,
    pub win_number_seen: u32,
    pub win_number_left_alone: u32,
    pub left_alone: BTreeMap<Reason, u32>,
    pub first_paths: BTreeMap<String, u32>,
    pub errors: Vec<String>,
    pub win_digit_presses: u32,
}

impl Totals {
    pub fn visible_times(&self) -> Vec<u64> {
        self.hovers
            .iter()
            .filter_map(|hover| hover.visible_ms)
            .collect()
    }

    pub fn reshows(&self) -> u32 {
        self.hovers.iter().map(|hover| hover.reshows).sum()
    }

    pub fn verdict(&self) -> Verdict {
        if self.dry_run {
            return Verdict::DryRun;
        }
        if self.hovers.is_empty() {
            return Verdict::NoHover;
        }
        if self.winner.is_none() {
            return Verdict::NotAllowed;
        }
        if self.reshows() > 0 {
            return Verdict::FightsBack;
        }
        match median(&self.visible_times()) {
            Some(ms) if ms >= CLEAN_MEDIAN_MS => Verdict::Flashes,
            _ if self.after_shows == 0 => Verdict::NotSeenAfter,
            _ => Verdict::Clean,
        }
    }

    pub fn summary(&self) -> String {
        let mut text = String::from("\nSummary\n");
        let times = self.visible_times();
        let _ = writeln!(text, "hovers seen: {}", self.hovers.len());
        let _ = writeln!(
            text,
            "hides done: {}{}",
            times.len(),
            if self.dry_run { " (dry run)" } else { "" }
        );
        let method = match (self.winner, self.dry_run) {
            (_, true) => "none (dry run)".to_string(),
            (Some(method), false) => method.name().to_string(),
            (None, false) => "none".to_string(),
        };
        let _ = writeln!(text, "method that worked: {method}");
        for (method, worked) in &self.tried {
            let _ = writeln!(
                text,
                "  tried {}: {}",
                method.name(),
                if *worked { "worked" } else { "did not work" }
            );
        }
        let ms = |value: Option<u64>| value.map_or("-".to_string(), |ms| format!("{ms} ms"));
        let _ = writeln!(
            text,
            "visible before hidden: median {}, max {}",
            ms(median(&times)),
            ms(times.iter().max().copied())
        );
        let _ = writeln!(text, "re-shows by Explorer within 1 s: {}", self.reshows());
        let still = self
            .hovers
            .iter()
            .filter(|hover| hover.cursor_still_on_button == Some(true))
            .count();
        let _ = writeln!(
            text,
            "cursor still on the button 1 s after the hide: {still} of {}",
            times.len()
        );
        let _ = writeln!(
            text,
            "after test: popup shown normally {} times",
            self.after_shows
        );
        let _ = writeln!(
            text,
            "Win+number: popup left alone {} of {} times ({} Win+digit presses seen)",
            self.win_number_left_alone, self.win_number_seen, self.win_digit_presses
        );
        for (reason, count) in &self.left_alone {
            let _ = writeln!(text, "  {}: {count}", reason.text());
        }
        let paths: Vec<String> = self
            .first_paths
            .iter()
            .map(|(path, count)| format!("{path}={count}"))
            .collect();
        let _ = writeln!(text, "seen first by: {}", none_or(&paths.join(", ")));
        let _ = writeln!(text, "errors: {}", self.errors.len());
        for error in &self.errors {
            let _ = writeln!(text, "  {error}");
        }
        let _ = writeln!(text, "verdict: {}", self.verdict().text());
        text
    }
}

fn none_or(text: &str) -> &str {
    if text.is_empty() {
        "nothing"
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn the_flag_gives_seconds_and_dry_run() {
        assert_eq!(options_from_args(&args(&["wincraft.exe"])), None);
        assert_eq!(
            options_from_args(&args(&["wincraft.exe", "--dry-run"])),
            None
        );
        assert_eq!(
            options_from_args(&args(&["wincraft.exe", "--test-hide-popup"])),
            Some(Ok(Options {
                seconds: 60,
                dry_run: false
            }))
        );
        assert_eq!(
            options_from_args(&args(&["wincraft.exe", "--test-hide-popup=5", "--dry-run"])),
            Some(Ok(Options {
                seconds: 5,
                dry_run: true
            }))
        );
        assert_eq!(
            options_from_args(&args(&[
                "wincraft.exe",
                "--dry-run",
                "--test-hide-popup= 600 "
            ])),
            Some(Ok(Options {
                seconds: 600,
                dry_run: true
            }))
        );
        for bad in ["4", "601", "0", "ten", ""] {
            let result = options_from_args(&args(&[
                "wincraft.exe",
                &format!("--test-hide-popup={bad}"),
            ]));
            assert!(matches!(result, Some(Err(_))), "{bad}");
        }
        assert_eq!(
            options_from_args(&args(&["wincraft.exe", "--test-hide-popup-now"])),
            None
        );
        assert_eq!(options_from_args(&args(&["--test-hide-popup"])), None);
    }

    #[test]
    fn the_watch_part_is_ten_seconds_or_half_a_short_run() {
        assert_eq!(watch_ms(60), 10_000);
        assert_eq!(watch_ms(20), 10_000);
        assert_eq!(watch_ms(5), 2_500);
    }

    #[test]
    fn a_hide_shows_as_hidden_cloaked_or_parked() {
        let screen = [0, 0, 3840, 2088];
        assert!(!took_effect(true, 0, screen));
        assert!(took_effect(false, 0, screen));
        assert!(took_effect(true, 2, screen));
        assert!(took_effect(true, 0, [-32000, -32000, -28160, -29912]));
        assert!(
            !took_effect(true, 0, [-3840, 0, 0, 2088]),
            "a monitor on the left"
        );
    }

    #[test]
    fn the_first_method_that_works_is_kept() {
        let mut picker = Picker::default();
        assert_eq!(picker.current(), Some(Method::ShowWindowAsync));
        picker.result(Method::ShowWindowAsync, false);
        assert_eq!(picker.current(), Some(Method::SetWindowPosHide));
        picker.result(Method::SetWindowPosHide, true);
        assert_eq!(picker.winner(), Some(Method::SetWindowPosHide));
        assert_eq!(picker.current(), Some(Method::SetWindowPosHide));
        picker.result(Method::SetWindowPosHide, false);
        assert_eq!(
            picker.current(),
            Some(Method::SetWindowPosHide),
            "one miss does not drop the winner"
        );
        assert_eq!(picker.tried.len(), 3);
    }

    #[test]
    fn when_every_method_fails_none_is_left() {
        let mut picker = Picker::default();
        for method in Method::ALL {
            assert_eq!(picker.current(), Some(method));
            picker.result(method, false);
        }
        assert_eq!(picker.current(), None);
        assert_eq!(picker.winner(), None);
    }

    #[test]
    fn the_median_takes_the_middle() {
        assert_eq!(median(&[]), None);
        assert_eq!(median(&[7]), Some(7));
        assert_eq!(median(&[30, 10, 20]), Some(20));
        assert_eq!(median(&[10, 40, 20, 30]), Some(25));
    }

    fn hidden_after(ms: u64) -> Hover {
        Hover {
            method: Some(Method::ShowWindowAsync),
            visible_ms: Some(ms),
            ..Hover::default()
        }
    }

    fn totals(hovers: Vec<Hover>, after_shows: u32) -> Totals {
        Totals {
            hovers,
            winner: Some(Method::ShowWindowAsync),
            after_shows,
            ..Totals::default()
        }
    }

    #[test]
    fn the_verdict_follows_the_measurements() {
        assert_eq!(
            totals(vec![hidden_after(8), hidden_after(12), hidden_after(40)], 3).verdict(),
            Verdict::Clean
        );
        assert_eq!(
            totals(vec![hidden_after(45), hidden_after(60)], 3).verdict(),
            Verdict::Flashes
        );
        let mut fought = hidden_after(8);
        fought.reshows = 2;
        assert_eq!(totals(vec![fought], 3).verdict(), Verdict::FightsBack);
        assert_eq!(
            totals(vec![hidden_after(8)], 0).verdict(),
            Verdict::NotSeenAfter
        );
        assert_eq!(totals(Vec::new(), 3).verdict(), Verdict::NoHover);
        let refused = Totals {
            winner: None,
            ..totals(vec![Hover::default()], 3)
        };
        assert_eq!(refused.verdict(), Verdict::NotAllowed);
        let dry = Totals {
            dry_run: true,
            ..totals(vec![hidden_after(8)], 3)
        };
        assert_eq!(dry.verdict(), Verdict::DryRun);
    }

    #[test]
    fn the_summary_names_every_count() {
        let mut fought = hidden_after(20);
        fought.reshows = 1;
        fought.cursor_still_on_button = Some(true);
        let mut all = totals(vec![hidden_after(10), fought], 4);
        all.tried = vec![(Method::ShowWindowAsync, true)];
        all.win_number_seen = 3;
        all.win_number_left_alone = 3;
        all.win_digit_presses = 5;
        all.left_alone.insert(Reason::WinKey, 3);
        all.first_paths.insert("event:show".to_string(), 2);
        all.errors
            .push("ShowWindowAsync: error 5 (access denied)".to_string());
        let text = all.summary();
        for expected in [
            "hovers seen: 2",
            "hides done: 2",
            "method that worked: (a) ShowWindowAsync SW_HIDE",
            "tried (a) ShowWindowAsync SW_HIDE: worked",
            "visible before hidden: median 15 ms, max 20 ms",
            "re-shows by Explorer within 1 s: 1",
            "cursor still on the button 1 s after the hide: 1 of 2",
            "after test: popup shown normally 4 times",
            "Win+number: popup left alone 3 of 3 times (5 Win+digit presses seen)",
            "left alone (Win+number): 3",
            "seen first by: event:show=2",
            "errors: 1",
            "error 5 (access denied)",
            "verdict: fights back",
        ] {
            assert!(text.contains(expected), "{expected}\n{text}");
        }
        let dry = Totals {
            dry_run: true,
            ..Totals::default()
        };
        let text = dry.summary();
        assert!(
            text.contains("method that worked: none (dry run)"),
            "{text}"
        );
        assert!(text.contains("seen first by: nothing"), "{text}");
        assert!(
            text.contains("visible before hidden: median -, max -"),
            "{text}"
        );
    }
}
