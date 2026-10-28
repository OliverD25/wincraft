//! "Open WindowDeck with Win+1…9": a low-level keyboard hook, put in only
//! while the switch is on (Principle 5), that keeps Win and a top-row digit
//! from Windows and asks the host for WindowDeck on that taskbar button.
//!
//! Windows registers Win+number for itself, so no hotkey can have it; a
//! hook sees the key first. Only Win with a digit and no other modifier is
//! kept: Win+Shift, Win+Ctrl and Win+Alt with a number go on to Windows, and
//! so does every other key. A kept digit's release is kept too, so Windows
//! never sees half a key, and the mask key follows it, so letting go of Win
//! does not open the Start menu. A button whose app has no window is handed
//! back to Windows by the host (`core::mask_key::replay_win_digit`), with a
//! tag this hook lets through.
//!
//! The hook runs on a thread of its own and only decides and posts; the
//! work happens on the host thread when the host's message comes round.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex, PoisonError};
use std::time::Instant;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, VK_CONTROL, VK_LWIN, VK_MENU,
    VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN,
    WM_SYSKEYUP,
};

use crate::core::ll_hook::HookThread;
use crate::core::{chord, host, mask_key, open_timing, taskbar_number, ui_bridge};

/// What the hook does with one key event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Windows and the app in front get it as usual.
    Pass,
    /// Kept from Windows: the release of a kept digit, or the key repeat of
    /// one still held.
    Hide,
    /// Kept, with the mask key after it, and WindowDeck asked for button
    /// `digit`. `again` is the same digit once more while Win stayed down.
    Take { digit: u8, again: bool },
    /// A Windows key went down (not its key repeat): Windows gets it, and
    /// the host gathers WindowDeck's windows for a digit that may follow.
    WinDown,
    /// A Windows key let go: Windows gets it as usual (so a Win pressed
    /// alone still opens Start), and the host hears of it. `alone` is true
    /// when no other key went down while Win was held.
    WinUp { alone: bool },
}

/// What the hook remembers between events.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rule {
    /// Digits whose press was kept and whose release is still to come.
    held: Vec<u32>,
    /// The digit taken last while Win has been down.
    last: Option<u8>,
    /// A Windows key is down.
    win: bool,
    /// No other key went down since the Windows key did.
    alone: bool,
    /// A digit was taken since the Windows key went down.
    took: bool,
}

impl Rule {
    /// One key event: `vk` going `down` or up, with `modifiers` held
    /// (`MOD_WIN`, `MOD_ALT`, `MOD_CONTROL`, `MOD_SHIFT`). `ours` is a key
    /// WinCraft sent itself.
    pub fn step(&mut self, vk: u32, down: bool, modifiers: u32, ours: bool) -> Step {
        if ours || vk == mask_key::VK {
            return Step::Pass;
        }
        if vk == u32::from(VK_LWIN) || vk == u32::from(VK_RWIN) {
            if down {
                // A held key repeats its down; only the first one counts.
                // The key state still shows Win down for a repeat, and up
                // for a new press, so a press after a release this hook
                // never saw (another program's hook kept it) starts afresh.
                if self.win && modifiers & MOD_WIN != 0 {
                    return Step::Pass;
                }
                self.win = true;
                self.alone = true;
                self.took = false;
                self.last = None;
                self.held.clear();
                return Step::WinDown;
            }
            self.last = None;
            let alone = self.win && self.alone;
            self.win = false;
            self.alone = false;
            return Step::WinUp { alone };
        }
        if down {
            self.alone = false;
        }
        if chord::is_modifier(vk) {
            return Step::Pass;
        }
        if !down {
            return match self.held.iter().position(|held| *held == vk) {
                Some(at) => {
                    self.held.swap_remove(at);
                    Step::Hide
                }
                None => Step::Pass,
            };
        }
        if self.held.contains(&vk) {
            return Step::Hide;
        }
        let Some(digit) = taskbar_number::digit_of(vk) else {
            return Step::Pass;
        };
        if modifiers & (MOD_WIN | MOD_ALT | MOD_CONTROL | MOD_SHIFT) != MOD_WIN {
            return Step::Pass;
        }
        self.held.push(vk);
        let again = self.last == Some(digit);
        self.last = Some(digit);
        self.took = true;
        Step::Take { digit, again }
    }

    /// Whether a digit was taken since the Windows key last went down.
    pub fn took(&self) -> bool {
        self.took
    }
}

/// `Rule::took` of the hook's rule, for the hover strip on the host thread.
static TOOK: AtomicBool = AtomicBool::new(false);

/// Whether Win and a digit opened or moved WindowDeck during the Windows
/// key's last press (the one held now, or the last one let go).
pub fn took_digit_this_press() -> bool {
    TOOK.load(Ordering::Relaxed)
}

/// The hook's thread, while the hook is in.
static WORKER: Mutex<Option<HookThread>> = Mutex::new(None);

/// Work the hook hands off, so its callback only decides and returns.
/// Windows waits for the callback and skips the hook when it is late, and
/// a skipped Win+digit goes to Windows, which switches to the app instead.
enum Job {
    /// The mask key, after a taken digit.
    Mask,
    Log(String),
    #[cfg(test)]
    Ping(mpsc::Sender<()>),
}

static JOBS: Mutex<Option<mpsc::Sender<Job>>> = Mutex::new(None);
static SIDE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

fn lock<T>(cell: &'static Mutex<T>) -> std::sync::MutexGuard<'static, T> {
    cell.lock().unwrap_or_else(PoisonError::into_inner)
}

fn start_side_work() {
    let (jobs, inbox) = mpsc::channel::<Job>();
    let thread = std::thread::Builder::new()
        .name("win-number side work".to_string())
        .spawn(move || {
            for job in inbox {
                match job {
                    Job::Mask => mask_key::send("WindowDeck"),
                    Job::Log(line) => log::info!("{line}"),
                    #[cfg(test)]
                    Job::Ping(back) => {
                        let _ = back.send(());
                    }
                }
            }
        });
    match thread {
        Ok(thread) => {
            *lock(&JOBS) = Some(jobs);
            *lock(&SIDE) = Some(thread);
        }
        Err(err) => log::warn!("win-number hook: no side thread ({err}); it works inline"),
    }
}

/// Ends the side thread once its queue is empty.
fn stop_side_work() {
    lock(&JOBS).take();
    if let Some(thread) = lock(&SIDE).take() {
        let _ = thread.join();
    }
}

/// Hands `job` to the side thread; false when there is none.
fn hand_off(job: Job) -> bool {
    match lock(&JOBS).as_ref() {
        Some(jobs) => jobs.send(job).is_ok(),
        None => false,
    }
}

thread_local! {
    /// Only the hook's own thread uses it.
    static RULE: RefCell<Rule> = RefCell::new(Rule::default());
}

fn worker() -> std::sync::MutexGuard<'static, Option<HookThread>> {
    WORKER.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Puts the hook in, on a thread of its own (see `core::ll_hook`): on the
/// busy host thread a repeated Win+number could reach Windows. False when
/// Windows refused it; Win+number then stays with Windows.
pub fn start() -> bool {
    let mut worker = worker();
    if worker.is_none() {
        *worker = HookThread::start("win-number hook", vec![(WH_KEYBOARD_LL, Some(hook_proc))]);
        if worker.is_some() {
            start_side_work();
        }
    }
    worker.is_some()
}

/// Takes the hook out, if it is in, and ends its thread and the side
/// thread.
pub fn stop() {
    let thread = worker().take();
    if let Some(thread) = thread {
        thread.stop();
        stop_side_work();
    }
}

pub fn is_running() -> bool {
    worker().is_some()
}

fn down(vk: u16) -> bool {
    (unsafe { GetAsyncKeyState(i32::from(vk)) } as u16 & 0x8000) != 0
}

fn modifiers_now() -> u32 {
    let mut modifiers = 0;
    if down(VK_LWIN) || down(VK_RWIN) {
        modifiers |= MOD_WIN;
    }
    if down(VK_MENU) {
        modifiers |= MOD_ALT;
    }
    if down(VK_CONTROL) {
        modifiers |= MOD_CONTROL;
    }
    if down(VK_SHIFT) {
        modifiers |= MOD_SHIFT;
    }
    modifiers
}

/// While Win+digit after another Win+digit is looked into: the Windows
/// keys, and digits while a Windows key is down.
fn worth_logging(vk: u32, modifiers: u32, win: bool) -> bool {
    let win_key = vk == u32::from(VK_LWIN) || vk == u32::from(VK_RWIN);
    let digit = taskbar_number::digit_of(vk).is_some();
    win_key || vk == mask_key::VK || (digit && (win || modifiers & MOD_WIN != 0))
}

/// A callback this slow, or a key that waited this long, is worth a line
/// whatever the key: Windows skips a hook that answers late.
const SLOW_MS: f64 = 20.0;

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        // Never seen so far; a line here would show a call this hook was
        // told not to act on.
        let line = format!("win-number hook: called with code {code}, message {wparam:#x}");
        if !hand_off(Job::Log(line.clone())) {
            log::info!("{line}");
        }
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }
    let began = Instant::now();
    let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
    // How long the key waited before Windows called this hook: a long wait
    // means the hook was late before, and Windows may have skipped it.
    let lag = (unsafe { GetTickCount64() } as u32).wrapping_sub(info.time);
    let event = wparam as u32;
    let is_down = event == WM_KEYDOWN || event == WM_SYSKEYDOWN;
    let is_up = event == WM_KEYUP || event == WM_SYSKEYUP;
    let (step, line) = if is_down || is_up {
        let ours = mask_key::is_ours(info.dwExtraInfo);
        RULE.with(|cell| match cell.try_borrow_mut() {
            Ok(mut rule) => {
                let modifiers = modifiers_now();
                let step = rule.step(info.vkCode, is_down, modifiers, ours);
                TOOK.store(rule.took(), Ordering::Relaxed);
                let line = worth_logging(info.vkCode, modifiers, rule.win).then(|| {
                    format!(
                        "win-number hook: key {:#04x} {} (modifiers {modifiers:#x}, ours {ours}, flags {:#x}, extra {:#x}, waited {lag} ms): {step:?}",
                        info.vkCode,
                        if is_down { "down" } else { "up" },
                        info.flags,
                        info.dwExtraInfo,
                    )
                });
                (step, line)
            }
            Err(_) => (Step::Pass, None),
        })
    } else {
        // Only the four key messages are documented; anything else is news.
        let line = format!(
            "win-number hook: unexpected message {event:#x} for key {:#04x}",
            info.vkCode
        );
        (Step::Pass, Some(line))
    };
    let result = match step {
        Step::Pass => unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) },
        Step::Hide => 1,
        Step::Take { digit, again } => {
            open_timing::key_seen();
            if !hand_off(Job::Mask) {
                // After the borrow: the mask comes back through this hook.
                mask_key::send("WindowDeck");
            }
            host::post_taskbar_button(digit, again);
            1
        }
        Step::WinDown => {
            ui_bridge::note_win_key(true, false);
            host::post_win_down();
            unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
        }
        Step::WinUp { alone } => {
            ui_bridge::note_win_key(false, took_digit_this_press());
            host::post_win_released(alone);
            unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
        }
    };
    let took = began.elapsed().as_secs_f64() * 1000.0;
    let line = line.or_else(|| {
        (took >= SLOW_MS || f64::from(lag) >= SLOW_MS).then(|| {
            format!(
                "win-number hook: slow for key {:#04x} {} (waited {lag} ms)",
                info.vkCode,
                if is_down { "down" } else { "up" }
            )
        })
    });
    if let Some(mut line) = line {
        line.push_str(&format!(", took {took:.2} ms"));
        if !hand_off(Job::Log(line.clone())) {
            log::info!("{line}");
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: u32 = 0x31;
    const THREE: u32 = 0x33;
    const WIN: u32 = 0x5B;

    #[test]
    fn win_and_a_digit_alone_is_kept_with_its_release() {
        let mut rule = Rule::default();
        assert_eq!(rule.step(WIN, true, MOD_WIN, false), Step::WinDown);
        assert_eq!(
            rule.step(THREE, true, MOD_WIN, false),
            Step::Take {
                digit: 3,
                again: false
            }
        );
        assert_eq!(
            rule.step(THREE, true, MOD_WIN, false),
            Step::Hide,
            "key repeat"
        );
        assert_eq!(
            rule.step(THREE, false, MOD_WIN, false),
            Step::Hide,
            "its release"
        );
        assert_eq!(
            rule.step(WIN, false, 0, false),
            Step::WinUp { alone: false }
        );
        rule.step(WIN, true, MOD_WIN, false);
        let zero = rule.step(0x30, true, MOD_WIN, false);
        assert_eq!(
            zero,
            Step::Take {
                digit: 0,
                again: false
            },
            "0 is button 10"
        );
    }

    #[test]
    fn the_side_thread_runs_the_handed_off_work_and_ends_on_stop() {
        start_side_work();
        let (back, heard) = mpsc::channel();
        assert!(hand_off(Job::Ping(back)));
        assert!(heard
            .recv_timeout(std::time::Duration::from_secs(5))
            .is_ok());
        stop_side_work();
        let (back, _heard) = mpsc::channel();
        assert!(!hand_off(Job::Ping(back)), "nothing to hand off to");
    }

    #[test]
    fn a_win_press_after_a_release_the_hook_never_saw_starts_afresh() {
        // Reported 2026-10-28: another program's hook kept the Win release,
        // so the next Win+1 was taken for a key repeat of a held digit and
        // hidden, and WindowDeck did not open.
        let mut rule = Rule::default();
        assert_eq!(rule.step(WIN, true, 0, false), Step::WinDown);
        assert_eq!(
            rule.step(ONE, true, MOD_WIN, false),
            Step::Take {
                digit: 1,
                again: false
            }
        );
        // No release of 1 and no release of Win: the next press comes with
        // the key state showing Win up.
        assert_eq!(rule.step(WIN, true, 0, false), Step::WinDown);
        assert_eq!(
            rule.step(ONE, true, MOD_WIN, false),
            Step::Take {
                digit: 1,
                again: false
            },
            "not a repeat, not again"
        );
        // A real key repeat of Win, with the key state showing it down.
        assert_eq!(rule.step(WIN, true, MOD_WIN, false), Step::Pass);
    }

    #[test]
    fn the_same_digit_while_win_stays_down_is_again() {
        let mut rule = Rule::default();
        let press = |rule: &mut Rule, vk| {
            let step = rule.step(vk, true, MOD_WIN, false);
            rule.step(vk, false, MOD_WIN, false);
            step
        };
        assert_eq!(
            press(&mut rule, ONE),
            Step::Take {
                digit: 1,
                again: false
            }
        );
        assert_eq!(
            press(&mut rule, ONE),
            Step::Take {
                digit: 1,
                again: true
            }
        );
        assert_eq!(
            press(&mut rule, THREE),
            Step::Take {
                digit: 3,
                again: false
            }
        );
        rule.step(WIN, false, 0, false);
        assert_eq!(
            press(&mut rule, THREE),
            Step::Take {
                digit: 3,
                again: false
            },
            "Win was let go in between"
        );
    }

    #[test]
    fn win_with_shift_ctrl_or_alt_and_a_number_stays_with_windows() {
        let mut rule = Rule::default();
        for modifiers in [
            MOD_WIN | MOD_SHIFT,
            MOD_WIN | MOD_CONTROL,
            MOD_WIN | MOD_ALT,
            MOD_ALT,
            0,
        ] {
            assert_eq!(
                rule.step(ONE, true, modifiers, false),
                Step::Pass,
                "{modifiers:#x}"
            );
            assert_eq!(rule.step(ONE, false, modifiers, false), Step::Pass);
        }
        // Other keys with Win, the keypad and the modifiers themselves pass.
        assert_eq!(rule.step(0x45, true, MOD_WIN, false), Step::Pass, "Win+E");
        assert_eq!(
            rule.step(0x61, true, MOD_WIN, false),
            Step::Pass,
            "keypad 1"
        );
        assert_eq!(rule.step(0xA0, true, MOD_WIN, false), Step::Pass, "Shift");
    }

    #[test]
    fn keys_wincraft_sent_itself_pass() {
        let mut rule = Rule::default();
        // The replay of Win+number for a pinned app with no windows, and
        // the mask key.
        assert_eq!(rule.step(ONE, true, MOD_WIN, true), Step::Pass);
        assert_eq!(rule.step(ONE, false, MOD_WIN, true), Step::Pass);
        assert_eq!(rule.step(mask_key::VK, true, MOD_WIN, false), Step::Pass);
        assert_eq!(rule.step(mask_key::VK, false, MOD_WIN, false), Step::Pass);
    }

    #[test]
    fn win_let_go_alone_is_told_apart_from_win_with_a_key() {
        let mut rule = Rule::default();
        // Win alone, with its key repeat: Start is Windows' to open.
        assert_eq!(rule.step(WIN, true, MOD_WIN, false), Step::WinDown);
        assert_eq!(rule.step(WIN, true, MOD_WIN, false), Step::Pass, "repeat");
        assert!(!rule.took());
        assert_eq!(rule.step(WIN, false, 0, false), Step::WinUp { alone: true });
        // Win and a digit.
        rule.step(WIN, true, MOD_WIN, false);
        rule.step(THREE, true, MOD_WIN, false);
        rule.step(THREE, false, MOD_WIN, false);
        assert!(rule.took());
        assert_eq!(
            rule.step(WIN, false, 0, false),
            Step::WinUp { alone: false }
        );
        assert!(rule.took(), "still known after Win was let go");
        // Win and a key that is not a digit, or a modifier.
        for key in [0x45, 0xA0] {
            rule.step(WIN, true, MOD_WIN, false);
            assert!(!rule.took(), "a new press starts afresh");
            rule.step(key, true, MOD_WIN, false);
            rule.step(key, false, MOD_WIN, false);
            assert_eq!(
                rule.step(WIN, false, 0, false),
                Step::WinUp { alone: false },
                "{key:#x}"
            );
        }
        // The mask key and keys WinCraft sent itself do not count.
        rule.step(WIN, true, MOD_WIN, false);
        rule.step(mask_key::VK, true, MOD_WIN, false);
        rule.step(mask_key::VK, false, MOD_WIN, false);
        rule.step(ONE, true, MOD_WIN, true);
        assert_eq!(rule.step(WIN, false, 0, false), Step::WinUp { alone: true });
        // A Win up with no Win down seen (the hook went in while it was
        // held) is not "alone".
        assert_eq!(
            rule.step(WIN, false, 0, false),
            Step::WinUp { alone: false }
        );
    }

    #[test]
    fn with_the_switch_off_there_is_no_hook() {
        stop();
        assert!(!is_running());
    }
}
