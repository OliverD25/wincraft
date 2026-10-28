//! The update panels at the bottom right of the screen, in the same look as
//! LayoutKeeper's restore notice. They take a click, which opens About, and
//! never the keyboard.
//!
//! - While an update runs, one stage at a time with a progress bar:
//!   "Checking for the new version…", "Downloading WinCraft 0.20.69 · 42 %",
//!   "Checking the download…", "Restarting WinCraft…". A failure says why
//!   and that the old version stays, then fades.
//! - When WinCraft starts after an update: "WinCraft · Updated to 0.16.2"
//!   and where to read what changed, held for a few seconds and then faded.

use std::sync::atomic::{AtomicIsize, Ordering};

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyWindow, GetCursorPos};

use crate::plugins::layout_keeper::notice::{Bar, BarKind, Content, Notice};

pub const CLASS_NAME: &str = "WinCraftUpdateNotice";
/// How long the "Updated to" notice stays at full strength, and how long it
/// then takes to fade out.
pub const HOLD_MS: u64 = 6000;
pub const FADE_MS: u64 = 600;
/// A failed update's panel stays this long before it fades.
pub const FAILED_HOLD_MS: u64 = 8000;
/// How often the host's timer calls `tick` while a panel is up.
pub const TICK_MS: u32 = 50;
/// One sweep of a moving bar.
pub const SWEEP_MS: u64 = 1400;

/// Where an update is, as the panel says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stage {
    Checking,
    /// Percent, when the server said how much there is.
    Downloading {
        version: String,
        percent: Option<u8>,
    },
    /// The release was downloaded ahead; its checksum is checked again.
    UsingStaged {
        version: String,
    },
    /// The SHA-256 of the download is compared with the release's.
    Verifying {
        version: String,
    },
    Restarting {
        version: String,
    },
    /// The reason in plain words, and the version that stays.
    Failed {
        reason: String,
        kept: String,
    },
}

impl Stage {
    /// Still under way: not failed.
    pub fn running(&self) -> bool {
        !matches!(self, Stage::Failed { .. })
    }

    /// The same step, whatever its percent: a new step is logged once.
    pub fn same_step(&self, other: &Stage) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

/// "0.20.69" from "v0.20.69".
pub fn plain_version(tag: &str) -> String {
    tag.trim_start_matches(['v', 'V']).to_string()
}

/// What the panel says at `stage`; `phase` moves a bar whose end is not
/// known, in thousandths of one sweep.
pub fn stage_content(stage: &Stage, phase: u16) -> Content {
    let moving = Some(Bar {
        done: 0,
        total: 1,
        finished: false,
        least: 0,
        kind: BarKind::Moving(phase),
    });
    let share = |percent: u8| {
        Some(Bar {
            done: usize::from(percent.min(100)),
            total: 100,
            finished: false,
            least: 0,
            kind: BarKind::Plain,
        })
    };
    let updating = |version: &str| format!("WinCraft \u{b7} Updating to {version}");
    let (title, line, bar) = match stage {
        Stage::Checking => (
            "WinCraft \u{b7} Updating".to_string(),
            "Checking for the new version\u{2026}".to_string(),
            moving,
        ),
        Stage::Downloading {
            version,
            percent: Some(percent),
        } => (
            updating(version),
            format!("Downloading WinCraft {version} \u{b7} {percent} %"),
            share(*percent),
        ),
        Stage::Downloading {
            version,
            percent: None,
        } => (
            updating(version),
            format!("Downloading WinCraft {version}\u{2026}"),
            moving,
        ),
        Stage::UsingStaged { version } => (
            updating(version),
            "Already downloaded, checking it\u{2026}".to_string(),
            moving,
        ),
        Stage::Verifying { version } => (
            updating(version),
            "Checking the download\u{2026}".to_string(),
            moving,
        ),
        Stage::Restarting { version } => (
            updating(version),
            "Restarting WinCraft\u{2026}".to_string(),
            share(100),
        ),
        Stage::Failed { reason, kept } => {
            return Content {
                title: "WinCraft \u{b7} Update failed".to_string(),
                lines: vec![reason.clone(), format!("WinCraft {kept} stays")],
                footer: Some("Click for details in Settings \u{2192} About".to_string()),
                bar: None,
            }
        }
    };
    Content {
        title,
        lines: vec![line],
        footer: None,
        bar,
    }
}

/// A download or install error, said in plain words for the panel. About
/// still shows the full text.
pub fn plain_reason(err: &str) -> String {
    let lower = err.to_ascii_lowercase();
    if lower.contains("checksum") {
        "The download did not match its checksum, so it was deleted.".to_string()
    } else if lower.contains("put the new version in place") || lower.contains("cannot write") {
        "The new version could not be saved next to the old one.".to_string()
    } else if lower.contains("has no ") || lower.contains("refused to download") {
        "This release has no WinCraft download.".to_string()
    } else if lower.contains("http status") {
        "GitHub did not give the download. Try again later.".to_string()
    } else {
        "The download stopped. Is the PC online?".to_string()
    }
}

/// The strength of the panel `elapsed_ms` after it appeared, held for
/// `hold_ms`; None once it has faded out and should go.
pub fn alpha_after(elapsed_ms: u64, hold_ms: u64) -> Option<u8> {
    if elapsed_ms < hold_ms {
        return Some(255);
    }
    let faded = elapsed_ms - hold_ms;
    if faded >= FADE_MS {
        return None;
    }
    Some((255 - faded * 255 / FADE_MS) as u8)
}

/// The "Updated to" panel's strength.
#[cfg(test)]
pub fn alpha_at(elapsed_ms: u64) -> Option<u8> {
    alpha_after(elapsed_ms, HOLD_MS)
}

/// How far a moving bar has gone `elapsed_ms` after the panel appeared.
pub fn sweep_phase(elapsed_ms: u64) -> u16 {
    ((elapsed_ms % SWEEP_MS) * 1000 / SWEEP_MS) as u16
}

pub fn content(version: &str) -> Content {
    Content {
        title: format!("WinCraft \u{b7} Updated to {version}"),
        lines: vec!["See what's new in Settings \u{2192} About".to_string()],
        footer: None,
        bar: None,
    }
}

enum Kind {
    Updated,
    Update(Stage),
}

pub struct UpdateNotice {
    notice: Notice,
    since: u64,
    kind: Kind,
    /// The monitor the panel was opened on, by a point in it: the one the
    /// palette was on, under the mouse.
    near: POINT,
}

impl UpdateNotice {
    /// Shows the notice for `version`; a click on it posts `message` to
    /// `target`.
    pub fn show(version: &str, target: HWND, message: u32) -> Result<Self, String> {
        let mut notice = Notice::create_clickable(CLASS_NAME, target, message)?;
        notice.show(&content(version));
        Ok(Self {
            notice,
            since: unsafe { GetTickCount64() },
            kind: Kind::Updated,
            near: POINT { x: 0, y: 0 },
        })
    }

    /// The update panel at `stage`, on the monitor under the mouse.
    pub fn show_stage(stage: Stage, target: HWND, message: u32) -> Result<Self, String> {
        let mut near = POINT { x: 0, y: 0 };
        unsafe { GetCursorPos(&mut near) };
        let mut panel = Self {
            notice: Notice::create_clickable(CLASS_NAME, target, message)?,
            since: unsafe { GetTickCount64() },
            kind: Kind::Update(stage),
            near,
        };
        panel.draw();
        Ok(panel)
    }

    /// The update under way: not the "Updated to" notice, nor a failure.
    pub fn running(&self) -> bool {
        matches!(&self.kind, Kind::Update(stage) if stage.running())
    }

    /// Whether the panel shows the same step as `stage`, whatever its
    /// percent.
    pub fn at(&self, stage: &Stage) -> bool {
        matches!(&self.kind, Kind::Update(shown) if shown.same_step(stage))
    }

    /// Moves the panel to `stage`. A failure starts its own time on screen.
    pub fn set_stage(&mut self, stage: Stage) {
        if !stage.running() {
            self.since = unsafe { GetTickCount64() };
        }
        self.kind = Kind::Update(stage);
        self.notice.set_alpha(255);
        self.draw();
    }

    fn draw(&mut self) {
        let elapsed = unsafe { GetTickCount64() }.saturating_sub(self.since);
        let content = match &self.kind {
            Kind::Updated => content(env!("CARGO_PKG_VERSION")),
            Kind::Update(stage) => stage_content(stage, sweep_phase(elapsed)),
        };
        self.notice.show_near(&content, self.near);
    }

    /// Fades the panel as time passes, or moves its bar. False once it is
    /// gone.
    pub fn tick(&mut self) -> bool {
        let elapsed = unsafe { GetTickCount64() }.saturating_sub(self.since);
        let alpha = match &self.kind {
            Kind::Updated => alpha_after(elapsed, HOLD_MS),
            Kind::Update(stage) if !stage.running() => alpha_after(elapsed, FAILED_HOLD_MS),
            Kind::Update(stage) => {
                if matches!(stage_content(stage, 0).bar, Some(bar) if matches!(bar.kind, BarKind::Moving(_)))
                {
                    self.draw();
                }
                Some(255)
            }
        };
        match alpha {
            Some(alpha) => {
                if alpha < 255 {
                    self.notice.set_alpha(alpha);
                }
                true
            }
            None => {
                self.close();
                false
            }
        }
    }

    /// Leaves the panel on screen after the host has stopped, so the user
    /// sees "Restarting WinCraft…" until the new version starts;
    /// `close_kept` then takes it away.
    pub fn keep_after_exit(self) {
        KEPT.store(self.notice.hwnd() as isize, Ordering::Relaxed);
    }

    pub fn close(&mut self) {
        self.notice.hide();
        self.notice.destroy();
    }
}

static KEPT: AtomicIsize = AtomicIsize::new(0);

/// Takes away the panel `keep_after_exit` left, from the thread that made
/// it. Nothing happens when there is none.
pub fn close_kept() {
    let hwnd = KEPT.swap(0, Ordering::Relaxed);
    if hwnd != 0 {
        unsafe { DestroyWindow(hwnd as HWND) };
    }
}

#[cfg(test)]
mod tests;
