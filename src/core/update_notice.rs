//! The notice shown once, at the bottom right of the screen, when WinCraft
//! starts after an update: "WinCraft · Updated to 0.16.2" and where to read
//! what changed. It is the same panel as LayoutKeeper's restore notice, held
//! for a few seconds and then faded. Unlike that one it takes a click, which
//! opens About.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::SystemInformation::GetTickCount64;

use crate::plugins::layout_keeper::notice::{Content, Notice};

pub const CLASS_NAME: &str = "WinCraftUpdateNotice";
/// How long the notice stays at full strength, and how long it then takes
/// to fade out.
pub const HOLD_MS: u64 = 6000;
pub const FADE_MS: u64 = 600;
/// How often the host's timer calls `tick` while the notice is up.
pub const TICK_MS: u32 = 50;

pub fn content(version: &str) -> Content {
    Content {
        title: format!("WinCraft \u{b7} Updated to {version}"),
        lines: vec!["See what's new in Settings \u{2192} About".to_string()],
        footer: None,
        bar: None,
    }
}

/// The strength of the panel `elapsed_ms` after it appeared; None once it
/// has faded out and should go.
pub fn alpha_at(elapsed_ms: u64) -> Option<u8> {
    if elapsed_ms < HOLD_MS {
        return Some(255);
    }
    let faded = elapsed_ms - HOLD_MS;
    if faded >= FADE_MS {
        return None;
    }
    Some((255 - faded * 255 / FADE_MS) as u8)
}

pub struct UpdateNotice {
    notice: Notice,
    since: u64,
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
        })
    }

    /// Fades the panel as time passes. False once it is gone.
    pub fn tick(&mut self) -> bool {
        let elapsed = unsafe { GetTickCount64() }.saturating_sub(self.since);
        match alpha_at(elapsed) {
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

    pub fn close(&mut self) {
        self.notice.hide();
        self.notice.destroy();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notice_says_which_version_and_where_to_read_more() {
        let content = content("0.16.2");
        assert_eq!(content.title, "WinCraft \u{b7} Updated to 0.16.2");
        assert_eq!(content.lines, ["See what's new in Settings \u{2192} About"]);
        assert_eq!(content.footer, None);
    }

    #[test]
    fn it_holds_for_six_seconds_then_fades_and_goes() {
        assert_eq!(alpha_at(0), Some(255));
        assert_eq!(alpha_at(5_999), Some(255));
        assert_eq!(alpha_at(HOLD_MS), Some(255));
        let half = alpha_at(HOLD_MS + FADE_MS / 2).unwrap();
        assert!((120..=135).contains(&half), "{half}");
        let late = alpha_at(HOLD_MS + FADE_MS - 1).unwrap();
        assert!(late < 5, "{late}");
        assert_eq!(alpha_at(HOLD_MS + FADE_MS), None);
        assert_eq!(alpha_at(60_000), None);
    }

    #[test]
    fn the_fade_never_gets_stronger() {
        let mut last = 255;
        for elapsed in (HOLD_MS..HOLD_MS + FADE_MS).step_by(10) {
            let alpha = alpha_at(elapsed).unwrap();
            assert!(alpha <= last);
            last = alpha;
        }
    }
}
