//! Custom WindowDeck: the host gives every window of the Claude desktop app
//! its Claude card (status and age) before the strip is sent, the same way
//! it fills in `attention`. The session files are read by
//! `core::claude_sessions` on a thread of its own; this only looks at what
//! was read last, so opening WindowDeck never waits for the disk.

use super::*;
use crate::core::claude_jobs;
use crate::core::claude_sessions::{self, ShownWindow};
use crate::core::ui_bridge::ArrangeWindow;
use crate::core::{claude_waits, windows_list};
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;

/// Fills `claude` on each window of a Claude group. Other groups are left
/// alone, and nothing is read when no Claude group is open.
pub(super) fn fill_claude_cards(groups: &mut [ArrangeGroup]) {
    let mut windows: Vec<&mut ArrangeWindow> = groups
        .iter_mut()
        .filter(|group| claude_waits::is_claude_app(&group.key))
        .flat_map(|group| group.windows.iter_mut())
        .collect();
    if windows.is_empty() {
        return;
    }
    claude_sessions::read_soon();
    let (sessions, popouts) = claude_sessions::sessions();
    // A renamed window is matched by the title the app gave it.
    let shown: Vec<ShownWindow> = windows
        .iter()
        .map(|window| {
            let hwnd = window.hwnd as HWND;
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            unsafe { GetWindowRect(hwnd, &mut rect) };
            ShownWindow {
                hwnd: window.hwnd,
                title: windows_list::window_text(hwnd),
                width: rect.right - rect.left,
                height: rect.bottom - rect.top,
                scale: f64::from(unsafe { GetDpiForWindow(hwnd) }) / 96.0,
            }
        })
        .collect();
    let by_size = claude_sessions::match_by_size(&shown, &sessions, &popouts);
    let now = claude_sessions::now_ms();
    let mut wanted: Vec<String> = Vec::new();
    ATTENTION.with(|attention| {
        let attention = attention.borrow();
        for (window, seen) in windows.iter_mut().zip(&shown) {
            let session = claude_sessions::find(&sessions, &popouts, &seen.title).or_else(|| {
                let (_, id) = by_size.iter().find(|(hwnd, _)| *hwnd == window.hwnd)?;
                sessions.iter().find(|s| &s.id == id)
            });
            let waiting = attention
                .waiting_since(window.hwnd)
                .map(claude_sessions::instant_ms);
            let jobs = session.map_or((0, None), |s| {
                if !s.cli_session_id.is_empty() && !wanted.contains(&s.cli_session_id) {
                    wanted.push(s.cli_session_id.clone());
                }
                claude_jobs::running(&s.cli_session_id, now)
            });
            window.claude = Some(claude_sessions::card(session, waiting, jobs));
        }
    });
    claude_jobs::want(wanted);
}
