//! `wincraft --test-hide-popup[=<seconds>] [--dry-run]`: can WinCraft hide
//! the taskbar's hover popup ("Task Switching") from outside Explorer, the
//! moment it opens over an app button? The user starts it by hand. It only
//! hides a popup that a mouse hover opened; one the keyboard opened
//! (Win+number) is always left alone. The last part of the run only watches,
//! to prove Explorer still shows the popup normally afterwards.
//!
//! Only plain hides are used, never a move off-screen: Explorer shows a
//! hidden popup again on the next hover by itself, so a run that is killed
//! halfway cannot leave the taskbar without its popup.

mod logic;

use std::fmt::Write as _;
use std::io::Write as _;
use std::time::{Duration, Instant, SystemTime};

use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_LBUTTON, VK_LWIN, VK_MBUTTON, VK_MENU, VK_RBUTTON, VK_RWIN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetWindowTextW, IsWindowVisible, SetWindowPos, ShowWindowAsync,
    SWP_ASYNCWINDOWPOS, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SW_HIDE,
};

use super::geometry::Rect;
use super::{report, scan};
use crate::core::{clock, config, taskbar, wide};
use logic::{
    decide, took_effect, watch_ms, Decision, Hover, Method, Picker, Reason, Totals, EFFECT_MS,
    POPUP_CLASS, POPUP_TITLE, RESHOW_MS,
};

pub use logic::{options_from_args, Options};

const POLL: Duration = Duration::from_millis(5);
const STILL_ON_BUTTON: Duration = Duration::from_secs(1);

/// One popup window and what the test knows about it.
struct Popup {
    hwnd: HWND,
    shown: bool,
    last_hide: Option<Instant>,
    /// The hover this instance belongs to, while it is open.
    hover: Option<usize>,
}

pub fn run(options: Options) -> i32 {
    let start = Instant::now();
    let started_at = clock::local_iso(SystemTime::now());
    let path = config::data_dir().join(format!(
        "hide-popup-test-{}.txt",
        report::file_stamp(&started_at)
    ));
    let act_ms = options.seconds * 1000 - watch_ms(options.seconds);
    say(&format!(
        "WinCraft hide-popup test{}: {} s. For the first {} s it hides the taskbar's hover popup\n\
         when the mouse opens it over an app button; Win+number is always left alone.\n\
         The last {} s only watch, to prove the popup works normally again.\n\
         Hover several taskbar buttons (slowly and quickly, Chrome too) and press Win+number a few times.\n\
         The report will be written to {}",
        if options.dry_run { " (dry run: nothing is hidden)" } else { "" },
        options.seconds,
        act_ms / 1000,
        watch_ms(options.seconds) / 1000,
        path.display()
    ));

    let mut log = String::new();
    let mut totals = Totals {
        dry_run: options.dry_run,
        ..Totals::default()
    };
    let mut picker = Picker::default();
    let mut popups: Vec<Popup> = Vec::new();
    let mut checks: Vec<(Instant, usize)> = Vec::new();
    let mut win_was_down = false;
    let mut digit_was_down = false;
    let mut last_win_up: Option<Instant> = None;
    let mut last_win_combo: Option<Instant> = None;
    let mut watching_noted = false;
    let mut next_countdown = 5;

    let deadline = start + Duration::from_secs(options.seconds);
    let act_until = start + Duration::from_millis(act_ms);
    while Instant::now() < deadline {
        let now = Instant::now();
        let t_ms = now.duration_since(start).as_millis() as u64;

        let win = key_down(VK_LWIN) || key_down(VK_RWIN);
        let digit = digit_down();
        if win && digit {
            if !digit_was_down {
                totals.win_digit_presses += 1;
                note(&mut log, t_ms, "Win+digit pressed");
            }
            last_win_combo = Some(now);
        }
        if win_was_down && !win {
            last_win_up = Some(now);
        }
        win_was_down = win;
        digit_was_down = digit;

        for hwnd in find_popups() {
            if !popups.iter().any(|popup| popup.hwnd == hwnd) {
                let shown = is_shown(hwnd);
                note(
                    &mut log,
                    t_ms,
                    &format!("popup window {hwnd:?} found, shown={shown}"),
                );
                popups.push(Popup {
                    hwnd,
                    shown,
                    last_hide: None,
                    hover: None,
                });
            }
        }

        if now >= act_until && !watching_noted {
            watching_noted = true;
            note(
                &mut log,
                t_ms,
                "acting ends; from now on the test only watches",
            );
        }

        for popup in &mut popups {
            let shown = is_shown(popup.hwnd);
            if shown && !popup.shown && window_title(popup.hwnd) != POPUP_TITLE {
                let title = window_title(popup.hwnd);
                note(
                    &mut log,
                    t_ms,
                    &format!("another Explorer window of the same class shown: \"{title}\""),
                );
            } else if shown && !popup.shown {
                if now >= act_until {
                    totals.after_shows += 1;
                    note(&mut log, t_ms, "after test: popup shown normally");
                } else {
                    let reshow = popup.last_hide.is_some_and(|at| {
                        now.duration_since(at) <= Duration::from_millis(RESHOW_MS)
                    });
                    if let (true, Some(index)) = (reshow, popup.hover) {
                        totals.hovers[index].reshows += 1;
                        note(&mut log, t_ms, "Explorer showed the popup again");
                        if !options.dry_run {
                            if let Some(method) = picker.winner() {
                                hide(popup.hwnd, method);
                                popup.last_hide = Some(Instant::now());
                            }
                        }
                    } else {
                        let inputs = logic::Inputs {
                            win_down: win,
                            since_win_combo: last_win_combo.map(|at| ms_since(at, now)),
                            since_win_up: last_win_up.map(|at| ms_since(at, now)),
                            alt_down: key_down(VK_MENU),
                            since_key: None,
                            mouse_button_down: key_down(VK_LBUTTON)
                                || key_down(VK_RBUTTON)
                                || key_down(VK_MBUTTON),
                            on_app_button: cursor_on_taskbar(),
                        };
                        match decide(&inputs) {
                            Decision::LeaveAlone(reason) => {
                                *totals.left_alone.entry(reason).or_insert(0) += 1;
                                if reason == Reason::WinKey {
                                    totals.win_number_seen += 1;
                                    totals.win_number_left_alone += 1;
                                }
                                popup.hover = None;
                                note(&mut log, t_ms, reason.text());
                            }
                            Decision::Hide => {
                                let index = totals.hovers.len();
                                let mut hover = Hover::default();
                                if options.dry_run {
                                    note(&mut log, t_ms, "would hide (dry run)");
                                } else {
                                    match hide_with_picker(popup.hwnd, &mut picker, now) {
                                        Some((method, visible_ms)) => {
                                            hover.method = Some(method);
                                            hover.visible_ms = Some(visible_ms);
                                            popup.last_hide = Some(Instant::now());
                                            note(
                                                &mut log,
                                                t_ms,
                                                &format!(
                                                    "hidden with {} after {visible_ms} ms on screen",
                                                    method.name()
                                                ),
                                            );
                                        }
                                        None => {
                                            totals.errors.push(format!(
                                                "[{t_ms} ms] no method hid the popup"
                                            ));
                                            note(&mut log, t_ms, "no method hid the popup");
                                        }
                                    }
                                }
                                hover.app = cursor_app();
                                *totals.first_paths.entry("poll".to_string()).or_insert(0) += 1;
                                totals.hovers.push(hover);
                                popup.hover = Some(index);
                                checks.push((now + STILL_ON_BUTTON, index));
                            }
                        }
                    }
                }
            }
            // Read again: the test may have just hidden it.
            popup.shown = is_shown(popup.hwnd);
        }

        checks.retain(|(due, index)| {
            if now < *due {
                return true;
            }
            totals.hovers[*index].cursor_still_on_button = Some(cursor_on_taskbar());
            false
        });

        let passed = start.elapsed().as_secs();
        if passed >= next_countdown && passed < options.seconds {
            say(&format!("{} s left", options.seconds - passed));
            next_countdown = passed + 5;
        }
        std::thread::sleep(POLL);
    }

    totals.tried = picker.tried.clone();
    totals.winner = picker.winner();
    let mut text = header(&started_at, options);
    text.push_str("\nTimeline\n");
    text.push_str(&log);
    text.push_str(&totals.summary());

    let written = path
        .parent()
        .map(std::fs::create_dir_all)
        .unwrap_or(Ok(()))
        .and_then(|_| std::fs::write(&path, &text));
    say(&text);
    match written {
        Ok(()) => {
            say(&format!("Report written to {}", path.display()));
            0
        }
        Err(err) => {
            say(&format!(
                "The report could not be written to {}: {err}",
                path.display()
            ));
            1
        }
    }
}

/// Tries the methods in the picker's order until one shows within
/// `EFFECT_MS`; the move off-screen is never used (see the module note).
fn hide_with_picker(hwnd: HWND, picker: &mut Picker, first_seen: Instant) -> Option<(Method, u64)> {
    while let Some(method) = picker.current() {
        if method == Method::MoveOffScreen {
            return None;
        }
        hide(hwnd, method);
        let until = Instant::now() + Duration::from_millis(EFFECT_MS);
        let mut worked = false;
        while Instant::now() < until {
            let (visible, cloak, rect) = state(hwnd);
            if took_effect(visible, cloak, rect) {
                worked = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        picker.result(method, worked);
        if worked {
            return Some((method, first_seen.elapsed().as_millis() as u64));
        }
    }
    None
}

fn hide(hwnd: HWND, method: Method) {
    match method {
        Method::ShowWindowAsync => {
            unsafe { ShowWindowAsync(hwnd, SW_HIDE) };
        }
        Method::SetWindowPosHide => {
            unsafe {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_HIDEWINDOW
                        | SWP_NOACTIVATE
                        | SWP_NOMOVE
                        | SWP_NOSIZE
                        | SWP_NOZORDER
                        | SWP_ASYNCWINDOWPOS,
                )
            };
        }
        Method::MoveOffScreen => {}
    }
}

fn find_popups() -> Vec<HWND> {
    // By class only: Explorer gives the window its title only while shown.
    let class = wide(POPUP_CLASS);
    let mut found = Vec::new();
    let mut after: HWND = std::ptr::null_mut();
    loop {
        let hwnd = unsafe {
            FindWindowExW(
                std::ptr::null_mut(),
                after,
                class.as_ptr(),
                std::ptr::null(),
            )
        };
        if hwnd.is_null() || found.len() >= 16 {
            break;
        }
        found.push(hwnd);
        after = hwnd;
    }
    found
}

fn state(hwnd: HWND) -> (bool, u32, Rect) {
    let visible = unsafe { IsWindowVisible(hwnd) } != 0;
    let mut cloak = 0u32;
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED as u32,
            &mut cloak as *mut u32 as *mut core::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        )
    };
    (visible, cloak, scan::window_rect(hwnd))
}

fn is_shown(hwnd: HWND) -> bool {
    let (visible, cloak, _) = state(hwnd);
    visible && cloak == 0
}

fn window_title(hwnd: HWND) -> String {
    let mut buffer = [0u16; 128];
    let len = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

fn key_down(key: u16) -> bool {
    (unsafe { GetAsyncKeyState(key as i32) } as u16) & 0x8000 != 0
}

fn digit_down() -> bool {
    (b'0'..=b'9').any(|key| key_down(u16::from(key)))
}

fn cursor_on_taskbar() -> bool {
    scan::cursor_pos().is_some_and(|(x, y)| taskbar::on_taskbar(POINT { x, y }))
}

fn cursor_app() -> String {
    scan::cursor_pos()
        .and_then(|(x, y)| taskbar::app_at(x, y))
        .unwrap_or_default()
}

fn ms_since(at: Instant, now: Instant) -> u64 {
    now.duration_since(at).as_millis() as u64
}

fn note(log: &mut String, t_ms: u64, text: &str) {
    let _ = writeln!(log, "[{t_ms} ms] {text}");
}

fn header(started_at: &str, options: Options) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "WinCraft hide-popup test, started {started_at}");
    let _ = writeln!(
        text,
        "seconds: {}, dry run: {}, acting part: {} s, watch-only part: {} s",
        options.seconds,
        options.dry_run,
        (options.seconds * 1000 - watch_ms(options.seconds)) / 1000,
        watch_ms(options.seconds) / 1000
    );
    let _ = writeln!(
        text,
        "popup: class {POPUP_CLASS}, title \"{POPUP_TITLE}\"; methods tried in order: \
         ShowWindowAsync SW_HIDE, then SetWindowPos SWP_HIDEWINDOW (never a move off-screen)"
    );
    text
}

/// Nobody may be reading: a closed pipe must not turn into a panic.
fn say(text: &str) {
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{text}");
    let _ = out.flush();
}
