//! `wincraft --probe-thumbnails[=<seconds>]`: a read-only look at the
//! taskbar's thumbnail popup, to learn whether WinCraft could find each
//! thumbnail's rectangle and window from outside Explorer. That is what a
//! drag of thumbnails inside the popup, done by WinCraft itself, would need.
//!
//! While it runs, the user hovers an app's taskbar button and moves over
//! the thumbnails. Every 100 ms it writes down the popup windows, their UI
//! Automation tree, the hovered group's windows and the cursor; a low-level
//! mouse hook writes down button presses and moves inside the popup. The
//! hook passes every event on unchanged, and nothing is moved, activated or
//! closed. The report goes to `%LOCALAPPDATA%\WinCraft` and to stdout.

mod geometry;
mod report;
mod scan;

use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, FindWindowW, GetMessageW, IsWindowVisible, PeekMessageW, PostThreadMessageW,
    SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE,
    WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP,
};

use crate::core::{clock, com, config, wide};
use geometry::Rect;
use report::{MouseEvent, MouseKind, Recorder, Sample, KNOWN_POPUP_CLASSES};

pub const DEFAULT_SECONDS: u64 = 30;
pub const MAX_SECONDS: u64 = 600;
const TICK: Duration = Duration::from_millis(100);
/// How long the end of the run waits for a tick still asking Explorer.
const DRAIN: Duration = Duration::from_secs(2);
const MAX_EVENTS: usize = 50_000;

static START: OnceLock<Instant> = OnceLock::new();
static EVENTS: Mutex<Vec<MouseEvent>> = Mutex::new(Vec::new());
/// The rectangles of the popups the last tick saw, for the hook to tell a
/// move inside a popup from any other.
static POPUP_RECTS: Mutex<Vec<Rect>> = Mutex::new(Vec::new());

/// Runs the probe for `seconds` and returns the exit code: 0 when the
/// report was written.
pub fn run(seconds: u64) -> i32 {
    let start = *START.get_or_init(Instant::now);
    let started_at = clock::local_iso(SystemTime::now());
    let path = config::data_dir().join(format!(
        "probe-thumbnails-{}.txt",
        report::file_stamp(&started_at)
    ));
    say(&format!(
        "WinCraft thumbnail probe: watching for {seconds} s. It only reads; it moves, closes and clicks nothing.\n\
         Now hover an app's taskbar button until its thumbnails show, move over them,\n\
         and press and release the mouse on a thumbnail.\n\
         The report will be written to {}",
        path.display()
    ));

    let mut recorder = Recorder::default();
    let hook = start_hook();
    recorder.add(
        0,
        Sample::Note(match hook {
            Some(_) => "mouse hook installed (it only watches)".to_string(),
            None => {
                "the mouse hook could not be installed; no mouse events are recorded".to_string()
            }
        }),
    );

    let (sender, receiver) = mpsc::channel::<(u64, Sample)>();
    let stop = Arc::new(AtomicBool::new(false));
    let scan_stop = Arc::clone(&stop);
    std::thread::spawn(move || scan_loop(&sender, &scan_stop, start));

    let deadline = start + Duration::from_secs(seconds);
    let mut next_countdown = 5;
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        match receiver.recv_timeout((deadline - now).min(Duration::from_secs(1))) {
            Ok((t_ms, sample)) => recorder.add(t_ms, sample),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let passed = start.elapsed().as_secs();
        if passed >= next_countdown && passed < seconds {
            say(&format!("{} s left", seconds - passed));
            next_countdown = passed + 5;
        }
    }
    stop.store(true, Ordering::SeqCst);
    // A tick may be waiting on Explorer; what it still sends is kept, but a
    // hung Explorer must not keep the probe from ending.
    let drain_until = Instant::now() + DRAIN;
    while let Ok((t_ms, sample)) =
        receiver.recv_timeout(drain_until.saturating_duration_since(Instant::now()))
    {
        recorder.add(t_ms, sample);
    }
    if let Some(hook) = hook {
        hook.stop();
    }
    let events = EVENTS
        .lock()
        .map(|events| events.clone())
        .unwrap_or_default();

    let text = recorder.finish(&header(&started_at, seconds), &events);
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

/// Nobody may be reading: a closed pipe must not turn into a panic.
fn say(text: &str) {
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{text}");
    let _ = out.flush();
}

fn header(started_at: &str, seconds: u64) -> String {
    let mut text = format!(
        "WinCraft {} thumbnail probe, started {started_at}, {seconds} s, one look every {} ms.\n\
         This probe only read the screen. It moved, closed and activated nothing,\n\
         and its mouse hook passed every event on unchanged.\n\
         Rectangles are left,top,right,bottom in physical screen pixels.\n\
         Slot guess: the picked popup minus {} px padding at 96 DPI (scaled by its\n\
         monitor's DPI), split into equal slots of about {}:1 width to height. Both numbers are guesses.\n\
         DWM: another program cannot list the thumbnails Explorer registered with DWM;\n\
         only window attributes are readable (cloak, and the extended frame bounds shown as dwm-frame).\n",
        env!("CARGO_PKG_VERSION"),
        TICK.as_millis(),
        geometry::PADDING_96,
        geometry::SLOT_ASPECT
    );
    text.push_str("Known popup classes at the start:");
    for class in KNOWN_POPUP_CLASSES {
        let hwnd = unsafe { FindWindowW(wide(class).as_ptr(), std::ptr::null()) };
        let state = if hwnd.is_null() {
            "none"
        } else if unsafe { IsWindowVisible(hwnd) } != 0 {
            "visible"
        } else {
            "hidden"
        };
        text.push_str(&format!(" {class}={state}"));
    }
    text.push('\n');
    text.push_str("Monitors:");
    for (rect, dpi) in monitors() {
        text.push_str(&format!(" {} dpi={dpi};", report::rect_text(rect)));
    }
    text.push('\n');
    text
}

fn monitors() -> Vec<(Rect, u32)> {
    let mut found: Vec<(Rect, u32)> = Vec::new();
    unsafe extern "system" fn collect(
        monitor: HMONITOR,
        _: HDC,
        rect: *mut RECT,
        found: LPARAM,
    ) -> BOOL {
        let found = unsafe { &mut *(found as *mut Vec<(Rect, u32)>) };
        let rect = unsafe { &*rect };
        let (mut x, mut y) = (0, 0);
        unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) };
        found.push(([rect.left, rect.top, rect.right, rect.bottom], x));
        1
    }
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect),
            &mut found as *mut Vec<(Rect, u32)> as LPARAM,
        )
    };
    found
}

/// UI Automation asks Explorer across processes, so the ticks run on a
/// thread of their own with its own COM; the hook thread never waits on
/// them.
fn scan_loop(sender: &mpsc::Sender<(u64, Sample)>, stop: &AtomicBool, start: Instant) {
    let since = || start.elapsed().as_millis() as u64;
    let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
    if !com::ok(hr) {
        let note = format!("COM did not start ({}); nothing was scanned", com::hex(hr));
        let _ = sender.send((since(), Sample::Note(note)));
        return;
    }
    {
        let mut scanner = scan::Scanner::new();
        let _ = sender.send((since(), Sample::Note(scanner.automation_note())));
        'ticks: while !stop.load(Ordering::SeqCst) {
            let began = Instant::now();
            let (samples, rects) = scanner.tick();
            if let Ok(mut shared) = POPUP_RECTS.lock() {
                *shared = rects;
            }
            let t_ms = since();
            for sample in samples {
                if sender.send((t_ms, sample)).is_err() {
                    break 'ticks;
                }
            }
            std::thread::sleep(TICK.saturating_sub(began.elapsed()));
        }
    }
    unsafe { CoUninitialize() };
}

struct Hook {
    thread_id: u32,
    thread: std::thread::JoinHandle<()>,
}

impl Hook {
    fn stop(self) {
        unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
        let _ = self.thread.join();
    }
}

/// A low-level hook is called through the message loop of the thread that
/// set it, so it gets a thread that does nothing else: a slow tick can then
/// never hold up the user's mouse.
fn start_hook() -> Option<Hook> {
    let (sender, receiver) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let mut msg: MSG = unsafe { std::mem::zeroed() };
        // Makes the thread's message queue, so the WM_QUIT posted later
        // has somewhere to go.
        unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE) };
        let hook = unsafe {
            SetWindowsHookExW(
                WH_MOUSE_LL,
                Some(mouse_hook),
                GetModuleHandleW(std::ptr::null()),
                0,
            )
        };
        let _ = sender.send((unsafe { GetCurrentThreadId() }, !hook.is_null()));
        if hook.is_null() {
            return;
        }
        while unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) } > 0 {}
        unsafe { UnhookWindowsHookEx(hook) };
    });
    match receiver.recv() {
        Ok((thread_id, true)) => Some(Hook { thread_id, thread }),
        _ => {
            let _ = thread.join();
            None
        }
    }
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam != 0 {
        let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        record(wparam as u32, info.pt.x, info.pt.y);
    }
    // Always passed on and never answered: the probe only watches.
    unsafe {
        CallNextHookEx(
            std::ptr::null_mut::<core::ffi::c_void>() as _,
            code,
            wparam,
            lparam,
        )
    }
}

fn record(message: u32, x: i32, y: i32) {
    let Some(kind) = mouse_kind(message) else {
        return;
    };
    let rects = POPUP_RECTS
        .lock()
        .map(|rects| rects.clone())
        .unwrap_or_default();
    let event = MouseEvent {
        t_ms: START
            .get()
            .map(|start| start.elapsed().as_millis() as u64)
            .unwrap_or(0),
        kind,
        x,
        y,
        inside: rects.iter().any(|rect| geometry::contains(*rect, x, y)),
    };
    if let Ok(mut events) = EVENTS.lock() {
        keep(&mut events, event);
    }
}

/// Moves count only inside a popup, and the list stops growing at
/// [`MAX_EVENTS`] so a long run cannot fill memory.
fn keep(events: &mut Vec<MouseEvent>, event: MouseEvent) {
    if event.kind == MouseKind::Move && !event.inside {
        return;
    }
    if events.len() < MAX_EVENTS {
        events.push(event);
    }
}

fn mouse_kind(message: u32) -> Option<MouseKind> {
    Some(match message {
        WM_MOUSEMOVE => MouseKind::Move,
        WM_LBUTTONDOWN => MouseKind::LeftDown,
        WM_LBUTTONUP => MouseKind::LeftUp,
        WM_RBUTTONDOWN => MouseKind::RightDown,
        WM_RBUTTONUP => MouseKind::RightUp,
        WM_MBUTTONDOWN => MouseKind::MiddleDown,
        WM_MBUTTONUP => MouseKind::MiddleUp,
        WM_MOUSEWHEEL => MouseKind::Wheel,
        _ => return None,
    })
}

/// `None` without the flag, else the number of seconds or why it cannot
/// be read: `--probe-thumbnails` alone means [`DEFAULT_SECONDS`].
pub fn seconds_from_args(args: &[String]) -> Option<Result<u64, String>> {
    args.iter().skip(1).find_map(|arg| {
        if arg == "--probe-thumbnails" {
            return Some(Ok(DEFAULT_SECONDS));
        }
        let value = arg.strip_prefix("--probe-thumbnails=")?;
        Some(
            value
                .trim()
                .parse::<u64>()
                .ok()
                .filter(|seconds| (1..=MAX_SECONDS).contains(seconds))
                .ok_or_else(|| {
                    format!(
                        "expected --probe-thumbnails=<seconds> from 1 to {MAX_SECONDS}, got {value}"
                    )
                }),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn the_flag_gives_the_seconds_or_the_default() {
        assert_eq!(seconds_from_args(&args(&["wincraft.exe"])), None);
        assert_eq!(
            seconds_from_args(&args(&["wincraft.exe", "--probe-thumbnails"])),
            Some(Ok(30))
        );
        assert_eq!(
            seconds_from_args(&args(&["wincraft.exe", "--probe-thumbnails=45"])),
            Some(Ok(45))
        );
        assert_eq!(
            seconds_from_args(&args(&["wincraft.exe", "--probe-thumbnails= 600 "])),
            Some(Ok(600))
        );
        for bad in ["0", "601", "ten", ""] {
            let result = seconds_from_args(&args(&[
                "wincraft.exe",
                &format!("--probe-thumbnails={bad}"),
            ]));
            assert!(matches!(result, Some(Err(_))), "{bad}");
        }
        assert_eq!(seconds_from_args(&args(&["--probe-thumbnails"])), None);
        assert_eq!(
            seconds_from_args(&args(&["wincraft.exe", "--probe-thumbnails-now"])),
            None
        );
    }

    #[test]
    fn mouse_messages_are_named_and_others_ignored() {
        assert_eq!(mouse_kind(WM_MOUSEMOVE), Some(MouseKind::Move));
        assert_eq!(mouse_kind(WM_LBUTTONDOWN), Some(MouseKind::LeftDown));
        assert_eq!(mouse_kind(WM_LBUTTONUP), Some(MouseKind::LeftUp));
        assert_eq!(mouse_kind(WM_RBUTTONDOWN), Some(MouseKind::RightDown));
        assert_eq!(mouse_kind(WM_RBUTTONUP), Some(MouseKind::RightUp));
        assert_eq!(mouse_kind(WM_MBUTTONDOWN), Some(MouseKind::MiddleDown));
        assert_eq!(mouse_kind(WM_MBUTTONUP), Some(MouseKind::MiddleUp));
        assert_eq!(mouse_kind(WM_MOUSEWHEEL), Some(MouseKind::Wheel));
        assert_eq!(mouse_kind(0x020E), None, "horizontal wheel");
    }

    #[test]
    fn moves_outside_a_popup_are_dropped_and_the_list_has_a_ceiling() {
        let event = |kind, inside| MouseEvent {
            t_ms: 0,
            kind,
            x: 0,
            y: 0,
            inside,
        };
        let mut events = Vec::new();
        keep(&mut events, event(MouseKind::Move, false));
        keep(&mut events, event(MouseKind::Move, true));
        keep(&mut events, event(MouseKind::LeftDown, false));
        assert_eq!(events.len(), 2);
        let mut full = vec![event(MouseKind::LeftUp, true); MAX_EVENTS];
        keep(&mut full, event(MouseKind::LeftDown, true));
        assert_eq!(full.len(), MAX_EVENTS);
    }

    #[test]
    fn the_header_names_the_guesses_and_the_screens() {
        let text = header("2026-10-24T13:05:11+03:00", 30);
        assert!(
            text.contains("started 2026-10-24T13:05:11+03:00, 30 s"),
            "{text}"
        );
        assert!(text.contains("8 px padding"), "{text}");
        assert!(text.contains("TaskListThumbnailWnd="), "{text}");
        assert!(text.contains("Monitors: ["), "{text}");
    }
}
