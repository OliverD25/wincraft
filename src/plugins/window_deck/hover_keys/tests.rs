//! No test here puts a hook on the real keyboard: the rules are plain data,
//! and `HoverKeys` is only driven as a read-only test instance.

use super::rule::{deck_key, RELEASE_WAIT_MS};
use super::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN, VK_DOWN, VK_ESCAPE, VK_F2, VK_LEFT, VK_RETURN,
    VK_RIGHT, VK_TAB, VK_UP,
};

const CTRL: u32 = MOD_CONTROL;
const SHIFT: u32 = MOD_SHIFT;
const D: u32 = 0x44;
const W: u32 = 0x57;
const N: u32 = 0x4E;
const A: u32 = 0x41;
const TWO: u32 = 0x32;

fn routed(vk: u32, modifiers: u32) -> Option<(egui::Key, bool, bool)> {
    deck_key(vk, modifiers).map(|key| (key.key, key.modifiers.ctrl, key.modifiers.shift))
}

#[test]
fn digits_go_plain_with_shift_and_with_ctrl() {
    for (vk, key) in [
        (0x30, egui::Key::Num0),
        (0x31, egui::Key::Num1),
        (0x35, egui::Key::Num5),
        (0x39, egui::Key::Num9),
    ] {
        assert_eq!(routed(vk, 0), Some((key, false, false)));
        assert_eq!(routed(vk, SHIFT), Some((key, false, true)));
        assert_eq!(routed(vk, CTRL), Some((key, true, false)));
        assert_eq!(routed(vk, CTRL | SHIFT), None);
    }
}

#[test]
fn arrows_go_plain_with_ctrl_and_with_ctrl_shift() {
    for (vk, key) in [
        (VK_LEFT, egui::Key::ArrowLeft),
        (VK_RIGHT, egui::Key::ArrowRight),
        (VK_UP, egui::Key::ArrowUp),
        (VK_DOWN, egui::Key::ArrowDown),
    ] {
        let vk = u32::from(vk);
        assert_eq!(routed(vk, 0), Some((key, false, false)));
        assert_eq!(routed(vk, CTRL), Some((key, true, false)));
        assert_eq!(routed(vk, CTRL | SHIFT), Some((key, true, true)));
        assert_eq!(routed(vk, SHIFT), None, "Shift+arrow selects text");
    }
}

#[test]
fn enter_esc_f2_d_ctrl_w_and_ctrl_n_go_to_the_strip() {
    assert_eq!(
        routed(u32::from(VK_RETURN), 0),
        Some((egui::Key::Enter, false, false))
    );
    assert_eq!(
        routed(u32::from(VK_ESCAPE), 0),
        Some((egui::Key::Escape, false, false))
    );
    assert_eq!(
        routed(u32::from(VK_F2), 0),
        Some((egui::Key::F2, false, false))
    );
    assert_eq!(routed(D, 0), Some((egui::Key::D, false, false)));
    assert_eq!(routed(W, CTRL), Some((egui::Key::W, true, false)));
    assert_eq!(routed(N, CTRL), Some((egui::Key::N, true, false)));
    let ctrl_w = deck_key(W, CTRL).unwrap().modifiers;
    assert!(ctrl_w.command, "as egui reports Ctrl on Windows");
}

#[test]
fn typing_stays_with_the_app() {
    assert_eq!(routed(A, 0), None);
    assert_eq!(routed(A, CTRL), None);
    assert_eq!(routed(W, 0), None, "a plain W is typing");
    assert_eq!(routed(N, 0), None);
    assert_eq!(routed(D, SHIFT), None);
    assert_eq!(routed(D, CTRL), None);
    assert_eq!(routed(u32::from(VK_TAB), 0), None);
    assert_eq!(routed(u32::from(VK_RETURN), SHIFT), None);
    assert_eq!(routed(u32::from(VK_ESCAPE), CTRL), None);
    assert_eq!(routed(u32::from(VK_F2), SHIFT), None);
    assert_eq!(routed(0x10, SHIFT), None, "Shift alone");
    assert_eq!(routed(0x11, CTRL), None, "Ctrl alone");
}

#[test]
fn win_and_alt_with_anything_stay_with_windows() {
    for modifiers in [MOD_WIN, MOD_ALT, MOD_WIN | SHIFT, MOD_ALT | CTRL] {
        for vk in [
            TWO,
            D,
            W,
            u32::from(VK_LEFT),
            u32::from(VK_RETURN),
            u32::from(VK_ESCAPE),
            u32::from(VK_TAB),
            u32::from(VK_F2),
        ] {
            assert_eq!(routed(vk, modifiers), None, "{vk:#x} with {modifiers:#x}");
        }
    }
}

#[test]
fn a_routed_key_is_kept_with_its_repeats_and_release() {
    let mut router = Router::default();
    let Step::Route(key) = router.step(TWO, true, 0, true, false, false) else {
        panic!("2 goes to the strip");
    };
    assert_eq!(key.key, egui::Key::Num2);
    assert!(!key.repeat);
    assert_eq!(router.held(), 1);
    let Step::Route(again) = router.step(TWO, true, 0, true, false, false) else {
        panic!("its key repeat goes too");
    };
    assert!(again.repeat);
    assert_eq!(router.step(TWO, false, 0, true, false, false), Step::Hide);
    assert_eq!(router.held(), 0);
}

#[test]
fn a_routed_key_keeps_its_release_after_routing_stopped() {
    let mut router = Router::default();
    assert!(matches!(
        router.step(D, true, 0, true, false, false),
        Step::Route(_)
    ));
    assert_eq!(
        router.step(D, true, 0, false, false, false),
        Step::Hide,
        "a repeat after routing stopped never reaches the app"
    );
    assert_eq!(router.step(D, false, 0, false, false, false), Step::Hide);
    assert_eq!(router.held(), 0);
    assert_eq!(
        router.step(D, false, 0, false, false, false),
        Step::Pass,
        "only once"
    );
}

#[test]
fn a_key_down_before_routing_started_passes_with_its_release() {
    let mut router = Router::default();
    assert_eq!(router.step(TWO, true, 0, false, false, false), Step::Pass);
    // Routing starts while 2 is held: its key repeat and release go on.
    assert_eq!(router.step(TWO, true, 0, true, true, false), Step::Pass);
    assert_eq!(router.step(TWO, false, 0, true, false, false), Step::Pass);
    assert_eq!(router.held(), 0);
}

#[test]
fn without_routing_or_for_our_own_keys_everything_passes() {
    let mut router = Router::default();
    assert_eq!(router.step(TWO, true, 0, false, false, false), Step::Pass);
    assert_eq!(router.step(TWO, true, 0, true, false, true), Step::Pass);
    assert_eq!(router.step(TWO, false, 0, true, false, true), Step::Pass);
    assert_eq!(router.step(A, true, 0, true, false, false), Step::Pass);
    assert_eq!(router.step(A, false, 0, true, false, false), Step::Pass);
    assert_eq!(router.held(), 0);
}

#[test]
fn the_hook_stays_only_while_routing_or_waiting_for_a_release() {
    assert!(keep_hook(true, 0, 0));
    assert!(keep_hook(true, 2, 5000));
    assert!(!keep_hook(false, 0, 0), "nothing held: it goes at once");
    assert!(keep_hook(false, 1, 0));
    assert!(keep_hook(false, 1, RELEASE_WAIT_MS - 1));
    assert!(
        !keep_hook(false, 1, RELEASE_WAIT_MS),
        "a lost release does not keep it"
    );
}

#[test]
fn a_read_only_test_instance_never_puts_the_hook_in() {
    let mut keys = HoverKeys::default();
    assert!(!keys.may_route);
    keys.update(true, 10);
    assert!(keys.is_routing(), "the decision is still made and logged");
    assert!(!is_running());
    assert!(!ROUTING.load(Ordering::Relaxed), "the real keys are left");
    keys.update(false, 20);
    assert!(!keys.is_routing());
    keys.update(true, 30);
    keys.stop();
    assert!(!keys.is_routing());
    assert!(!is_running());
}
