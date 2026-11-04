use super::*;

fn stored(jump: &str, pin: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (JUMP_ACTION.to_string(), jump.to_string()),
        (PIN_ACTION.to_string(), pin.to_string()),
    ])
}

#[test]
fn the_old_defaults_move_once_and_chosen_keys_stay() {
    let mut hotkeys = stored("Win+Alt+Q", "Win+Alt+Shift+Q");
    let mut settings = serde_json::json!({});
    assert!(move_old_defaults(&mut hotkeys, &mut settings));
    assert_eq!(hotkeys[JUMP_ACTION], "Win+`");
    assert_eq!(hotkeys[PIN_ACTION], "", "the pin shortcut has no keys now");
    assert_eq!(settings[MOVED_NOTE], true);
    // Picked again later, they are choices and stay.
    let mut again = stored("win+alt+q", "Win+Alt+Shift+Q");
    assert!(!move_old_defaults(&mut again, &mut settings));
    assert_eq!(again[JUMP_ACTION], "win+alt+q");
    assert_eq!(again[PIN_ACTION], "Win+Alt+Shift+Q");
}

#[test]
fn keys_the_user_chose_are_never_moved() {
    let mut custom = stored("Win+Alt+F12", "Win+Alt+P, 1");
    assert!(!move_old_defaults(&mut custom, &mut serde_json::json!({})));
    assert_eq!(custom[JUMP_ACTION], "Win+Alt+F12");
    assert_eq!(custom[PIN_ACTION], "Win+Alt+P, 1");
    let mut none = stored("", "");
    assert!(!move_old_defaults(&mut none, &mut serde_json::json!({})));
    assert_eq!(none[JUMP_ACTION], "");
    // Only the jump key was the old default.
    let mut one = stored("Win+Alt+Q", "Win+Alt+F11");
    assert!(move_old_defaults(&mut one, &mut serde_json::json!({})));
    assert_eq!(one[JUMP_ACTION], "Win+`");
    assert_eq!(one[PIN_ACTION], "Win+Alt+F11");
}

#[test]
fn the_jump_starts_on_win_backtick_and_the_pin_without_keys() {
    let actions = QuickJump::new().hotkey_actions();
    assert_eq!(crate::core::hotkeys::format(actions[0].default), JUMP_KEYS);
    assert!(actions[1].default.is_none());
    assert_eq!(actions[1].name, PIN_ACTION);
}
