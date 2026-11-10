//! `wincraft.exe --waiting "<title>"` and `--answered "<title>"`: a Claude
//! Code hook tells the running WinCraft that the session with this title
//! waits for the user's answer, or was answered. The running WinCraft marks
//! the Claude windows with that title (see `attention`). The Claude app
//! flashes a window only when the app is not in front, so a question in one
//! Claude window while the user works in another would go unseen.
//!
//! The command finds the running WinCraft's host window and hands it the
//! title in one `WM_COPYDATA`, then exits. It returns fast: the hook waits
//! for it.

use windows_sys::Win32::Foundation::{LPARAM, WPARAM};
use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, SendMessageTimeoutW, SMTO_ABORTIFHUNG, WM_COPYDATA,
};

use crate::core::{host, instance, wide};

/// How long the command waits for the running WinCraft to take the title.
const SEND_TIMEOUT_MS: u32 = 2000;
/// A title longer than this is not a session title; the copy stays small.
const MOST_TITLE_CHARS: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Waiting,
    Answered,
}

impl Kind {
    pub fn flag(self) -> &'static str {
        match self {
            Kind::Waiting => "--waiting",
            Kind::Answered => "--answered",
        }
    }

    /// The `dwData` of the `WM_COPYDATA`. Random-looking, so a stray copy
    /// from another program is not taken for one.
    pub fn code(self) -> usize {
        match self {
            Kind::Waiting => 0x5743_0001,
            Kind::Answered => 0x5743_0002,
        }
    }

    pub fn from_code(code: usize) -> Option<Kind> {
        [Kind::Waiting, Kind::Answered]
            .into_iter()
            .find(|kind| kind.code() == code)
    }
}

/// Reads `--waiting <title>` or `--answered <title>` from the command line.
/// None when neither flag is there; an error line when the title is missing
/// or empty.
pub fn parse_args(args: &[String]) -> Option<Result<(Kind, String), String>> {
    for kind in [Kind::Waiting, Kind::Answered] {
        let Some(at) = args.iter().skip(1).position(|arg| arg == kind.flag()) else {
            continue;
        };
        let title = args.get(at + 2).map(|title| title.trim()).unwrap_or("");
        if title.is_empty() {
            return Some(Err(format!(
                "{} needs the session title, for example: wincraft.exe {} \"Fix the build\"",
                kind.flag(),
                kind.flag()
            )));
        }
        return Some(Ok((kind, title.to_string())));
    }
    None
}

/// The text of the copy: the title in UTF-16, cut to a sane length.
pub fn encode(title: &str) -> Vec<u16> {
    title
        .trim()
        .chars()
        .take(MOST_TITLE_CHARS)
        .collect::<String>()
        .encode_utf16()
        .collect()
}

/// What the host reads from a copy: the kind and the trimmed title. None
/// for a copy that is not ours or has no title.
pub fn decode(code: usize, bytes: &[u8]) -> Option<(Kind, String)> {
    let kind = Kind::from_code(code)?;
    if !bytes.len().is_multiple_of(2) || bytes.len() > MOST_TITLE_CHARS * 4 {
        return None;
    }
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    let title = String::from_utf16_lossy(&units).trim().to_string();
    (!title.is_empty()).then_some((kind, title))
}

/// The Claude desktop app, from its program path or its taskbar group:
/// a Store package folder `windowsapps\claude_…` holding `claude.exe`.
pub fn is_claude_app(path_or_group: &str) -> bool {
    let lower = path_or_group.to_lowercase();
    lower.contains(r"windowsapps\claude_") && lower.ends_with(r"\claude.exe")
}

/// A window named by its handle, `hwnd:<number>` (decimal or `0x` hex),
/// instead of a title. A hook that found the window itself (a popped-out
/// window, a renamed one) names it this way.
pub fn window_handle(name: &str) -> Option<isize> {
    let number = name.trim().strip_prefix("hwnd:")?;
    let parsed = match number.strip_prefix("0x") {
        Some(hex) => isize::from_str_radix(hex, 16).ok(),
        None => number.parse().ok(),
    };
    parsed.filter(|hwnd| *hwnd != 0)
}

/// Whether a window with this program and app title shows the session
/// `wanted`: a Claude window whose title is exactly that, after trimming.
pub fn shows_session(exe_path: &str, window_title: &str, wanted: &str) -> bool {
    let wanted = wanted.trim();
    !wanted.is_empty() && is_claude_app(exe_path) && window_title.trim() == wanted
}

/// What the host answers: how many windows the title matched, plus one, so
/// that 0 still means "not taken".
pub fn reply_for(matched: usize) -> isize {
    matched.min(isize::MAX as usize - 1) as isize + 1
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Taken { kind: Kind, matched: usize },
    NotRunning,
    NoAnswer,
    BadArgs(String),
}

impl Outcome {
    pub fn exit_code(&self) -> i32 {
        match self {
            Outcome::Taken { .. } => 0,
            Outcome::NotRunning | Outcome::NoAnswer => 1,
            Outcome::BadArgs(_) => 2,
        }
    }

    pub fn message(&self, title: &str) -> String {
        let windows = |n: usize| match n {
            1 => "1 Claude window".to_string(),
            n => format!("{n} Claude windows"),
        };
        match self {
            Outcome::Taken {
                kind: Kind::Waiting,
                matched,
            } => format!("WinCraft marked {} titled \"{title}\"", windows(*matched)),
            Outcome::Taken {
                kind: Kind::Answered,
                matched,
            } => format!(
                "WinCraft cleared the mark of {} titled \"{title}\"",
                windows(*matched)
            ),
            Outcome::NotRunning => "WinCraft is not running".to_string(),
            Outcome::NoAnswer => "WinCraft did not answer".to_string(),
            Outcome::BadArgs(line) => line.clone(),
        }
    }
}

/// Hands the title to the running WinCraft.
pub fn run(kind: Kind, title: &str) -> Outcome {
    let class = wide(&instance::name(host::CLASS_NAME));
    let hwnd = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
    if hwnd.is_null() {
        return Outcome::NotRunning;
    }
    let text = encode(title);
    let data = COPYDATASTRUCT {
        dwData: kind.code(),
        cbData: (text.len() * 2) as u32,
        lpData: text.as_ptr() as *mut std::ffi::c_void,
    };
    let mut result = 0usize;
    let sent = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_COPYDATA,
            0 as WPARAM,
            &data as *const COPYDATASTRUCT as LPARAM,
            SMTO_ABORTIFHUNG,
            SEND_TIMEOUT_MS,
            &mut result,
        )
    };
    if sent == 0 || result == 0 {
        return Outcome::NoAnswer;
    }
    Outcome::Taken {
        kind,
        matched: result - 1,
    }
}

#[cfg(test)]
mod tests;
