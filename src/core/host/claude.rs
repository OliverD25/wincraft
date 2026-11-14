//! Custom WindowDeck: the host gives every window of the Claude desktop app
//! its Claude card (status and age) before the strip is sent, the same way
//! it fills in `attention`. The session files are read by
//! `core::claude_sessions` on a thread of its own; this only looks at what
//! was read last, so opening WindowDeck never waits for the disk.
//!
//! ClaudeDeck asks for the same matching of windows to chats, and gets it
//! written to `core::claude_windows` for its window on the UI thread.

use super::*;
use crate::core::claude_jobs;
use crate::core::claude_sessions::{self, Popout, Session, ShownWindow};
use crate::core::claude_windows::{self, ChatWindow};
use crate::core::ui_bridge::ArrangeWindow;
use crate::core::{claude_waits, server_jobs, windows_list};
use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;

/// ClaudeDeck reads the background jobs only of chats active this recently,
/// so a long list of old chats never has its transcripts read.
const JOBS_WITHIN_MS: u64 = 24 * 60 * 60 * 1000;

/// A Claude window as the matching sees it. A renamed window is matched by
/// the title the app gave it.
fn shown_window(hwnd: isize) -> ShownWindow {
    let handle = hwnd as HWND;
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe { GetWindowRect(handle, &mut rect) };
    ShownWindow {
        hwnd,
        title: windows_list::window_text(handle),
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
        scale: f64::from(unsafe { GetDpiForWindow(handle) }) / 96.0,
    }
}

/// The session a window shows: by its title, else by its size.
fn session_for<'a>(
    sessions: &'a [Session],
    popouts: &[Popout],
    by_size: &[(isize, String)],
    seen: &ShownWindow,
) -> Option<&'a Session> {
    claude_sessions::find(sessions, popouts, &seen.title).or_else(|| {
        let (_, id) = by_size.iter().find(|(hwnd, _)| *hwnd == seen.hwnd)?;
        sessions.iter().find(|s| &s.id == id)
    })
}

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
    let shown: Vec<ShownWindow> = windows
        .iter()
        .map(|window| shown_window(window.hwnd))
        .collect();
    let by_size = claude_sessions::match_by_size(&shown, &sessions, &popouts);
    let now = claude_sessions::now_ms();
    let mut wanted: Vec<String> = Vec::new();
    ATTENTION.with(|attention| {
        let attention = attention.borrow();
        for (window, seen) in windows.iter_mut().zip(&shown) {
            let session = session_for(&sessions, &popouts, &by_size, seen);
            let waiting = attention
                .waiting_since(window.hwnd)
                .map(claude_sessions::instant_ms);
            let jobs = session.map_or((0, None), |s| {
                if !s.cli_session_id.is_empty() && !wanted.contains(&s.cli_session_id) {
                    wanted.push(s.cli_session_id.clone());
                }
                claude_jobs::running(&s.cli_session_id, now)
            });
            let card = claude_sessions::card(session, waiting, jobs);
            window.claude = Some(server_jobs::with_job(card, session.map(|s| s.id.as_str())));
        }
    });
    claude_jobs::want(wanted);
}

/// ClaudeDeck's window is open: starts a read of the session files (at most
/// every few seconds), names the chats whose jobs it shows, and writes
/// which chats have a window and which of those wait. Only reads windows.
pub(super) fn publish_chat_windows() {
    if !claude_windows::wanted() {
        return;
    }
    claude_sessions::read_soon();
    let (sessions, popouts) = claude_sessions::sessions();
    let shown: Vec<ShownWindow> = windows_list::app_window_records(&|_| true)
        .into_iter()
        .filter(|(hwnd, _)| {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(*hwnd, &mut pid) };
            claude_waits::is_claude_app(&windows_list::exe_path(pid))
        })
        .map(|(hwnd, _)| shown_window(hwnd as isize))
        .collect();
    let by_size = claude_sessions::match_by_size(&shown, &sessions, &popouts);
    let windows: Vec<ChatWindow> = ATTENTION.with(|attention| {
        let attention = attention.borrow();
        shown
            .iter()
            .filter_map(|seen| {
                let session = session_for(&sessions, &popouts, &by_size, seen)?;
                Some(ChatWindow {
                    hwnd: seen.hwnd,
                    session_id: session.id.clone(),
                    waiting_since: attention
                        .waiting_since(seen.hwnd)
                        .map(claude_sessions::instant_ms),
                    approval: attention.waits_for_approval(seen.hwnd),
                })
            })
            .collect()
    });
    let now = claude_sessions::now_ms();
    let wanted = sessions
        .iter()
        .filter(|s| !s.cli_session_id.is_empty())
        .filter(|s| {
            claude_windows::of(&windows, &s.id).is_some()
                || s.last_activity
                    .is_some_and(|at| now.saturating_sub(at) < JOBS_WITHIN_MS)
        })
        .map(|s| s.cli_session_id.clone())
        .collect();
    claude_jobs::want_for_deck(wanted);
    claude_windows::publish(windows);
    // The window may have closed while this ran: its close cleared the
    // lists first, so clear what was just written.
    if !claude_windows::wanted() {
        claude_jobs::want_for_deck(Vec::new());
        claude_windows::publish(Vec::new());
    }
}

#[cfg(test)]
mod tests;
