//! Which keys follow the mouse into the hover strip, and what the hook does
//! with each key event, as plain data. Nothing here calls Windows.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, VK_DOWN, VK_ESCAPE, VK_F2, VK_LEFT, VK_RETURN,
    VK_RIGHT, VK_UP,
};

use crate::core::ui_bridge::ArrangeKey;

const NONE: u32 = 0;
const CTRL: u32 = MOD_CONTROL;
const SHIFT: u32 = MOD_SHIFT;

/// The strip's key for `vk` with `modifiers` held (`MOD_WIN`, `MOD_ALT`,
/// `MOD_CONTROL`, `MOD_SHIFT`), or None when the key stays with the app in
/// front. Win and Alt never go to the strip: they close it, so Win+number
/// and Alt+Tab work as in Windows.
pub fn deck_key(vk: u32, modifiers: u32) -> Option<ArrangeKey> {
    if modifiers & (MOD_WIN | MOD_ALT) != 0 {
        return None;
    }
    let arrows: &[u32] = &[NONE, CTRL, CTRL | SHIFT];
    let (key, allowed): (egui::Key, &[u32]) = match vk {
        0x30..=0x39 => (digit_key(vk - 0x30), &[NONE, SHIFT, CTRL]),
        _ if vk == u32::from(VK_LEFT) => (egui::Key::ArrowLeft, arrows),
        _ if vk == u32::from(VK_RIGHT) => (egui::Key::ArrowRight, arrows),
        _ if vk == u32::from(VK_UP) => (egui::Key::ArrowUp, arrows),
        _ if vk == u32::from(VK_DOWN) => (egui::Key::ArrowDown, arrows),
        _ if vk == u32::from(VK_RETURN) => (egui::Key::Enter, &[NONE]),
        _ if vk == u32::from(VK_ESCAPE) => (egui::Key::Escape, &[NONE]),
        _ if vk == u32::from(VK_F2) => (egui::Key::F2, &[NONE]),
        0x57 => (egui::Key::W, &[CTRL]),
        0x4E => (egui::Key::N, &[CTRL]),
        0x44 => (egui::Key::D, &[NONE]),
        _ => return None,
    };
    allowed.contains(&modifiers).then(|| ArrangeKey {
        key,
        modifiers: egui_modifiers(modifiers),
        repeat: false,
    })
}

fn digit_key(digit: u32) -> egui::Key {
    use egui::Key;
    [
        Key::Num0,
        Key::Num1,
        Key::Num2,
        Key::Num3,
        Key::Num4,
        Key::Num5,
        Key::Num6,
        Key::Num7,
        Key::Num8,
        Key::Num9,
    ][digit.min(9) as usize]
}

/// The modifiers as egui reports them from a real key on Windows, where
/// Ctrl is also the command key.
fn egui_modifiers(modifiers: u32) -> egui::Modifiers {
    let ctrl = modifiers & CTRL != 0;
    egui::Modifiers {
        alt: false,
        ctrl,
        shift: modifiers & SHIFT != 0,
        mac_cmd: false,
        command: ctrl,
    }
}

/// What the hook does with one key event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The app in front gets it as usual.
    Pass,
    /// Kept from the app and handed to the strip.
    Route(ArrangeKey),
    /// Kept from the app: the release or key repeat of a routed key.
    Hide,
}

/// What the hook remembers between events: the keys it routed whose
/// release is still to come.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Router {
    held: Vec<u32>,
}

impl Router {
    /// One key event: `vk` going `down` or up with `modifiers` held, while
    /// keys go to the strip (`routing`) or not. `was_down` is the key's
    /// state before this event: a down of a key already down is a key
    /// repeat. `ours` is a key WinCraft sent itself.
    pub fn step(
        &mut self,
        vk: u32,
        down: bool,
        modifiers: u32,
        routing: bool,
        was_down: bool,
        ours: bool,
    ) -> Step {
        if ours {
            return Step::Pass;
        }
        let held = self.held.iter().position(|held| *held == vk);
        if !down {
            // A routed key's release is kept even when routing stopped
            // meanwhile, so the app never sees half a key. Any other
            // release goes on.
            return match held {
                Some(at) => {
                    self.held.swap_remove(at);
                    Step::Hide
                }
                None => Step::Pass,
            };
        }
        if held.is_some() {
            return match deck_key(vk, modifiers).filter(|_| routing) {
                Some(key) => Step::Route(ArrangeKey {
                    repeat: true,
                    ..key
                }),
                None => Step::Hide,
            };
        }
        // A key held since before routing started stays with the app.
        if !routing || was_down {
            return Step::Pass;
        }
        match deck_key(vk, modifiers) {
            Some(key) => {
                self.held.push(vk);
                Step::Route(key)
            }
            None => Step::Pass,
        }
    }

    /// How many routed keys are still down.
    pub fn held(&self) -> usize {
        self.held.len()
    }
}

/// How long the hook may stay in after routing stopped, waiting for the
/// release of a routed key, before it goes anyway: a release can be lost
/// to another program's hook.
pub const RELEASE_WAIT_MS: u64 = 1000;

/// Whether the hook stays in: while keys go to the strip, and after that
/// only until every routed key is let go, for at most `RELEASE_WAIT_MS`.
pub fn keep_hook(routing: bool, held: usize, off_for_ms: u64) -> bool {
    routing || (held > 0 && off_for_ms < RELEASE_WAIT_MS)
}
