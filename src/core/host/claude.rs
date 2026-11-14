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
use crate::core::claude_waits::Ask;
use crate::core::claude_windows::{self, ChatWindow};
use crate::core::ui_bridge::ArrangeWindow;
use crate::core::{claude_waits, server_jobs, windows_list};
use std::time::Instant;
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
                .or_else(|| {
                    let cli = &session?.cli_session_id;
                    attention.session_waiting(cli).map(|(since, _)| since)
                })
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
    let shown = claude_windows_now();
    let by_size = claude_sessions::match_by_size(&shown, &sessions, &popouts);
    let (windows, waits) = ATTENTION.with(|attention| {
        let attention = attention.borrow();
        let windows: Vec<ChatWindow> = shown
            .iter()
            .filter_map(|seen| {
                let session = session_for(&sessions, &popouts, &by_size, seen)?;
                let by_chat = attention.session_waiting(&session.cli_session_id);
                let (waiting_since, approval) = match attention.waiting_since(seen.hwnd) {
                    Some(since) => (Some(since), attention.waits_for_approval(seen.hwnd)),
                    None => (
                        by_chat.map(|(since, _)| since),
                        by_chat.is_some_and(|(_, ask)| ask == Ask::Approval),
                    ),
                };
                Some(ChatWindow {
                    hwnd: seen.hwnd,
                    session_id: session.id.clone(),
                    waiting_since: waiting_since.map(claude_sessions::instant_ms),
                    approval,
                })
            })
            .collect();
        let waits = windowless_waits(&sessions, &windows, |cli| attention.session_waiting(cli));
        (windows, waits)
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
    claude_windows::publish_waits(waits);
    // The window may have closed while this ran: its close cleared the
    // lists first, so clear what was just written.
    if !claude_windows::wanted() {
        claude_jobs::want_for_deck(Vec::new());
        claude_windows::publish(Vec::new());
        claude_windows::publish_waits(Vec::new());
    }
}

/// The visible windows of the Claude app, as the matching sees them.
fn claude_windows_now() -> Vec<ShownWindow> {
    windows_list::app_window_records(&|_| true)
        .into_iter()
        .filter(|(hwnd, _)| {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(*hwnd, &mut pid) };
            claude_waits::is_claude_app(&windows_list::exe_path(pid))
        })
        .map(|(hwnd, _)| shown_window(hwnd as isize))
        .collect()
}

/// The chats that wait by WinCraft's own hook but have no window of their
/// own, for ClaudeDeck's rows. `hwnd` is 0: there is no window to show.
fn windowless_waits(
    sessions: &[Session],
    windows: &[ChatWindow],
    waiting: impl Fn(&str) -> Option<(Instant, Ask)>,
) -> Vec<ChatWindow> {
    sessions
        .iter()
        .filter(|s| !s.cli_session_id.is_empty() && claude_windows::of(windows, &s.id).is_none())
        .filter_map(|s| {
            let (since, ask) = waiting(&s.cli_session_id)?;
            Some(ChatWindow {
                hwnd: 0,
                session_id: s.id.clone(),
                waiting_since: Some(claude_sessions::instant_ms(since)),
                approval: ask == Ask::Approval,
            })
        })
        .collect()
}

/// The windows that show the chat with this Claude Code session id now,
/// matched the way ClaudeDeck matches them, from what was read last.
pub(super) fn windows_of_session(cli_session_id: &str) -> Vec<isize> {
    claude_sessions::read_soon();
    let (sessions, popouts) = claude_sessions::sessions();
    showing(&sessions, &popouts, &claude_windows_now(), cli_session_id)
}

fn showing(
    sessions: &[Session],
    popouts: &[Popout],
    shown: &[ShownWindow],
    cli_session_id: &str,
) -> Vec<isize> {
    let Some(wanted) = sessions
        .iter()
        .find(|s| !cli_session_id.is_empty() && s.cli_session_id == cli_session_id)
    else {
        return Vec::new();
    };
    let by_size = claude_sessions::match_by_size(shown, sessions, popouts);
    shown
        .iter()
        .filter(|seen| {
            session_for(sessions, popouts, &by_size, seen).is_some_and(|s| s.id == wanted.id)
        })
        .map(|seen| seen.hwnd)
        .collect()
}

#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod tests;
