//! The detailed log: a switch that records everything WinCraft does, in a
//! file of its own (`%LOCALAPPDATA%\WinCraft\detailed\`), so a bug can be
//! found from the record after it happened. Off by default.
//!
//! Off, it costs one atomic load per place that could write a line: the
//! `detailed!` macro and `line` check `on()` before they format anything,
//! and the watchers (`watch`) do not exist. On, a line is formatted where it
//! happens and handed to the writer thread through a channel, so a keyboard
//! hook's callback never waits for a file.
//!
//! Plugins reach it through `host::detailed`, never through this module.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

use crate::core::{config, instance};

pub mod header;
pub mod watch;
pub mod writer;

#[cfg(test)]
mod tests;

/// Writes one line to the detailed log, formatted only when it is on.
/// The same arguments as `format!`.
#[macro_export]
macro_rules! detailed {
    ($($arg:tt)*) => {
        if $crate::core::detailed::on() {
            $crate::core::detailed::write(::std::format!($($arg)*));
        }
    };
}

static ON: AtomicBool = AtomicBool::new(false);
static WRITER: OnceLock<Sender<writer::Msg>> = OnceLock::new();

/// Whether the detailed log is on. One atomic load.
#[inline]
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// The folder of the detailed log. A test instance gets a folder of its
/// own (`detailed.<suffix>`), so it never writes into the user's.
pub fn folder() -> PathBuf {
    config::data_dir().join(instance::name("detailed"))
}

/// Writes `text` as one line with the time and the thread's name in front.
/// Nothing happens when the log is off. Never blocks: the line goes to the
/// writer thread through a channel.
pub fn write(text: String) {
    if !on() {
        return;
    }
    send_line(stamp(&text));
}

/// `write` for `format_args!`, which formats only when the log is on.
pub fn write_args(args: std::fmt::Arguments) {
    if on() {
        send_line(stamp(&args.to_string()));
    }
}

fn send_line(line: String) {
    if let Some(sender) = WRITER.get() {
        let _ = sender.send(writer::Msg::Line(line));
    }
}

/// `2026-10-28 14:03:05.123 [thread] text`.
fn stamp(text: &str) -> String {
    let mut st: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut st) };
    let thread = std::thread::current();
    stamp_with(&st, thread.name().unwrap_or("thread"), text)
}

pub(crate) fn stamp_with(st: &SYSTEMTIME, thread: &str, text: &str) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03} [{thread}] {text}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond, st.wMilliseconds
    )
}

/// Starts the writer thread the first time it is needed. It then sleeps on
/// its channel for the rest of the run, which costs nothing.
fn writer() -> Option<&'static Sender<writer::Msg>> {
    if WRITER.get().is_none() {
        let (sender, inbox) = mpsc::channel();
        let dir = folder();
        let started = std::thread::Builder::new()
            .name("detailed log".to_string())
            .spawn(move || writer::run(writer::Writer::new(dir, writer::MAX_BYTES), inbox));
        match started {
            Ok(_) => {
                let _ = WRITER.set(sender);
            }
            Err(err) => {
                log::warn!("detailed log: no writer thread ({err})");
                return None;
            }
        }
    }
    WRITER.get()
}

/// Turns the detailed log on or off. `why` goes into the first or last
/// line. Turning it on starts the writer and the watchers, and writes the
/// session header with `plugins`, the plugins on, when they are known yet;
/// off stops the watchers and closes the file.
pub fn set(wanted: bool, why: &str, plugins: Option<Vec<String>>) {
    if wanted == on() {
        return;
    }
    if wanted {
        if writer().is_none() {
            return;
        }
        ON.store(true, Ordering::Relaxed);
        crate::core::logging::follow_detailed();
        write(format!("=== ON {why}"));
        if let Some(plugins) = plugins {
            header::write_later(plugins);
        }
        watch::start();
    } else {
        write(format!("=== OFF {why}"));
        watch::stop();
        ON.store(false, Ordering::Relaxed);
        crate::core::logging::follow_detailed();
        if let Some(sender) = WRITER.get() {
            let _ = sender.send(writer::Msg::Close);
        }
    }
}

/// Writes `=== MARK <time> <words>`, so a reader knows where to look.
pub fn mark(words: &str) {
    let words = words.trim();
    let words = if words.is_empty() {
        "(no words)"
    } else {
        words
    };
    let mut st: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut st) };
    write(format!(
        "=== MARK {:02}:{:02}:{:02} {words}",
        st.wHour, st.wMinute, st.wSecond
    ));
}

/// Waits until every line sent so far is in the file: before WinCraft
/// exits, and before a report copies the file.
pub fn flush() {
    if let Some(sender) = WRITER.get() {
        let (done, wait) = mpsc::channel();
        if sender.send(writer::Msg::Flush(done)).is_ok() {
            let _ = wait.recv_timeout(std::time::Duration::from_secs(5));
        }
    }
}

/// Deletes every detailed log file. When the log is on, the file in use is
/// closed first and a new one starts with the next line. Returns how many
/// files went.
pub fn delete_files() -> usize {
    match WRITER.get() {
        Some(sender) => {
            let (done, wait) = mpsc::channel();
            if sender.send(writer::Msg::DeleteAll(done)).is_err() {
                return 0;
            }
            wait.recv_timeout(std::time::Duration::from_secs(5))
                .unwrap_or(0)
        }
        None => writer::delete_all(&folder()),
    }
}

/// One key event a keyboard hook saw: the fields of its
/// `KBDLLHOOKSTRUCT`, how long the key waited before the hook was called
/// (`lag_ms`) and how long the callback took (`took_ms`).
#[derive(Clone, Copy, Debug)]
pub struct KeyEvent<'a> {
    pub hook: &'a str,
    pub vk: u32,
    pub message: u32,
    pub flags: u32,
    pub extra: usize,
    pub lag_ms: u32,
    pub took_ms: f64,
}

impl KeyEvent<'_> {
    /// Writes the event and the hook's `decision` as one line. Call it after
    /// the decision, and build the event only when `on()` is true.
    pub fn write(&self, decision: &dyn std::fmt::Debug) {
        if on() {
            write(self.line(decision));
        }
    }

    pub(crate) fn line(&self, decision: &dyn std::fmt::Debug) -> String {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            LLKHF_INJECTED, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
        };
        let what = match self.message {
            WM_KEYDOWN => "down",
            WM_SYSKEYDOWN => "sysdown",
            WM_KEYUP => "up",
            WM_SYSKEYUP => "sysup",
            _ => "other",
        };
        let injected = u8::from(self.flags & LLKHF_INJECTED != 0);
        format!(
            "key {}: {} {what} (vk {:#04x}, flags {:#x}, injected {injected}, extra {:#x}, waited {} ms, took {:.2} ms) -> {decision:?}",
            self.hook,
            key_name(self.vk),
            self.vk,
            self.flags,
            self.extra,
            self.lag_ms,
            self.took_ms,
        )
    }
}

/// A Windows call that can fail, made on window `hwnd` (0 for none), and
/// what it returned. With `failed`, the thread's last error is added, so
/// call it right after the call. Formats only while the log is on.
pub fn call(what: &str, hwnd: isize, result: i64, failed: bool) {
    if !on() {
        return;
    }
    let error = failed.then(|| unsafe { windows_sys::Win32::Foundation::GetLastError() });
    write(call_line(what, hwnd, result, error));
}

pub(crate) fn call_line(what: &str, hwnd: isize, result: i64, error: Option<u32>) -> String {
    let mut line = format!("call: {what}");
    if hwnd != 0 {
        line.push_str(&format!(" on {hwnd:#x}"));
    }
    line.push_str(&format!(" -> {result}"));
    if let Some(error) = error {
        line.push_str(&format!(" FAILED (error {error})"));
    }
    line
}

/// "<what> slow N ms" when `ms` reached `limit_ms`.
pub fn slow_line(what: &str, ms: f64, limit_ms: f64) -> Option<String> {
    (ms >= limit_ms).then(|| format!("{what} slow {ms:.0} ms"))
}

/// How long ago, in ms, the event with the time stamp `time` (from
/// `KBDLLHOOKSTRUCT::time`) happened.
pub fn lag_ms(time: u32) -> u32 {
    let now = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() } as u32;
    now.wrapping_sub(time)
}

/// A short name for the keys the Win+digit hunt cares about.
pub(crate) fn key_name(vk: u32) -> String {
    match vk {
        0x5B => "LWin".to_string(),
        0x5C => "RWin".to_string(),
        0x10 | 0xA0 | 0xA1 => "Shift".to_string(),
        0x11 | 0xA2 | 0xA3 => "Ctrl".to_string(),
        0x12 | 0xA4 | 0xA5 => "Alt".to_string(),
        0x1B => "Esc".to_string(),
        0x30..=0x39 | 0x41..=0x5A => char::from(vk as u8).to_string(),
        other => format!("{other:#04x}"),
    }
}
