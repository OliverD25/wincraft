use super::*;

fn labels(items: &[MenuItem]) -> Vec<(String, bool)> {
    items
        .iter()
        .map(|item| match item {
            MenuItem::Separator => ("-".to_string(), false),
            MenuItem::Entry { label, checked, .. } => (label.clone(), *checked),
        })
        .collect()
}

#[test]
fn the_menu_offers_hide_while_shown_and_show_while_hidden() {
    let shown = labels(&menu(true, false));
    assert_eq!(
        shown,
        vec![
            ("Hide ClaudeDeck".to_string(), false),
            ("Mini mode".to_string(), false),
            ("-".to_string(), false),
            ("Quick settings\u{2026}".to_string(), false),
            ("All settings\u{2026}".to_string(), false),
        ]
    );
    let hidden = labels(&menu(false, true));
    assert_eq!(hidden[0], ("Show ClaudeDeck".to_string(), false));
    assert_eq!(
        hidden[1],
        ("Mini mode".to_string(), true),
        "ticked in mini mode"
    );
    assert!(
        !hidden.iter().any(|(label, _)| label.contains("Quit")),
        "Quit stays on WinCraft's icon"
    );
}

#[test]
fn the_menu_ids_never_clash_with_the_hotkey_actions() {
    for item in menu(true, true).iter().chain(&menu(false, false)) {
        if let MenuItem::Entry { id, .. } = item {
            assert!(*id > super::super::ACTION_MINI, "{id}");
        }
    }
}

#[test]
fn the_tooltip_names_claudedeck_and_the_badge_counts_what_waits() {
    assert_eq!(
        tip(true, Some((2, 4))),
        "ClaudeDeck \u{2014} 2 chats wait for you, 4 new replies"
    );
    assert_eq!(tip(true, Some((0, 1))), "ClaudeDeck \u{2014} 1 new reply");
    assert_eq!(tip(true, Some((0, 0))), "ClaudeDeck");
    assert_eq!(tip(true, None), "ClaudeDeck", "nothing read yet");
    assert_eq!(tip(false, Some((2, 4))), "ClaudeDeck", "the count is off");
    assert_eq!(badge(true, Some((3, 9))), 3, "only the chats that wait");
    assert_eq!(badge(false, Some((3, 9))), 0);
    assert_eq!(badge(true, None), 0);
}

#[test]
fn a_shown_window_in_front_hides_and_anything_else_comes_forward() {
    assert!(hides(true, true, false));
    assert!(!hides(true, false, false), "behind other windows: forward");
    assert!(!hides(false, true, false), "hidden: shown again");
    assert!(!hides(true, true, true), "Show ClaudeDeck never hides");
}

#[test]
fn a_tray_click_counts_the_window_seen_in_front_before_the_taskbar() {
    let deck = 10;
    assert!(in_front(deck, deck, false, None, false));
    assert!(!in_front(deck, 20, false, Some(deck), false));
    assert!(
        in_front(deck, 30, true, Some(deck), true),
        "the click went to the taskbar"
    );
    assert!(
        !in_front(deck, 30, true, Some(20), true),
        "another app was in front"
    );
    assert!(
        !in_front(deck, 30, true, Some(deck), false),
        "the hotkey: the taskbar is just in front"
    );
    assert!(!in_front(0, 0, true, Some(0), true), "no window yet");
}

#[test]
fn living_in_the_tray_makes_a_tool_window_without_appwindow() {
    let normal = WS_EX_APPWINDOW_TEST | 0x100;
    let live = ex_style(normal, true);
    assert_eq!(live & TOOL, TOOL);
    assert_eq!(live & WS_EX_APPWINDOW_TEST, 0);
    assert_eq!(live & 0x100, 0x100, "other bits stay");
    let back = ex_style(live, false);
    assert_eq!(back & TOOL, 0);
    assert_eq!(back & WS_EX_APPWINDOW_TEST, WS_EX_APPWINDOW_TEST);
}

const TOOL: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WS_EX_TOOLWINDOW;
const WS_EX_APPWINDOW_TEST: u32 = windows_sys::Win32::UI::WindowsAndMessaging::WS_EX_APPWINDOW;

#[test]
fn the_plugin_asks_for_a_hide_or_a_close_through_open() {
    let mut shared = Shared {
        open: true,
        ..Default::default()
    };
    let mut state = DeckUi::default();
    assert!(!open_asked(&mut shared, &mut state), "a plain open");
    shared.home.hide_wanted = true;
    assert!(open_asked(&mut shared, &mut state));
    assert!(!shared.home.hide_wanted, "taken once");
    assert!(!shared.home.hidden, "no window to hide yet");
    shared.home.hidden = true;
    shared.home.close_wanted = true;
    assert!(open_asked(&mut shared, &mut state));
    assert!(!shared.open, "closed for good");
    assert!(!shared.home.hidden);
}

#[test]
fn hiding_needs_the_setting_and_a_window() {
    let mut shared = Shared::default();
    let mut state = DeckUi::default();
    shared.settings.tray_home = false;
    state.window = 5;
    assert!(
        !hides_instead(&mut shared, &mut state),
        "setting off: close"
    );
    shared.settings.tray_home = true;
    state.window = 0;
    assert!(!hides_instead(&mut shared, &mut state), "no window: close");
}

#[test]
fn the_setting_is_on_by_default() {
    assert!(super::super::Settings::default().tray_home);
    assert!(super::super::Settings::read(&serde_json::json!({})).tray_home);
    assert!(!super::super::Settings::read(&serde_json::json!({ "live_in_tray": false })).tray_home);
}

#[test]
fn the_timer_reads_the_chat_windows_with_no_claudedeck_lock_held() {
    // Listing windows waits for the UI thread, which may wait for these
    // locks: holding one here hung WinCraft in 0.20.131.
    let mut hidden = Shared::default();
    hidden.settings.tray_home = true;
    let shared = Mutex::new(hidden);
    let state = Mutex::new(DeckUi::default());
    let mut read = false;
    read_while_hidden_with(
        &shared,
        &state,
        || {
            assert!(shared.try_lock().is_ok(), "Shared is locked");
            assert!(state.try_lock().is_ok(), "DeckUi is locked");
            read = true;
        },
        |_, _| (2, 1),
    );
    assert!(read);
    assert_eq!(shared.lock().unwrap().home.counts, Some((2, 1)));
}

#[test]
fn the_timer_reads_nothing_while_the_window_shows() {
    let mut showing = Shared {
        open: true,
        ..Default::default()
    };
    showing.settings.tray_home = true;
    let shared = Mutex::new(showing);
    let state = Mutex::new(DeckUi::default());
    read_while_hidden_with(&shared, &state, || panic!("read"), |_, _| panic!("counted"));
    assert_eq!(shared.lock().unwrap().home.counts, None);
}

/// The extended style winit writes for ClaudeDeck: read from the user's
/// window on 2026-11-16, with the tray setting on.
const WINIT_STYLE: u32 = 0x0004_0118;

fn plan(set: bool, taskbar: bool, lost: bool) -> StylePlan {
    StylePlan { set, taskbar, lost }
}

#[test]
fn a_lost_tool_flag_is_put_back_although_the_window_was_styled_before() {
    assert_eq!(WINIT_STYLE & TOOL, 0);
    assert_eq!(
        style_plan(Some((5, true)), 5, true, WINIT_STYLE),
        plan(true, false, true)
    );
}

#[test]
fn an_added_app_flag_is_taken_away_again() {
    let both = ex_style(WINIT_STYLE, true) | WS_EX_APPWINDOW_TEST;
    assert_eq!(
        style_plan(Some((5, true)), 5, true, both),
        plan(true, false, true)
    );
}

#[test]
fn a_right_style_is_left_alone() {
    let right = ex_style(WINIT_STYLE, true);
    assert_eq!(
        style_plan(Some((5, true)), 5, true, right),
        plan(false, false, false)
    );
}

#[test]
fn with_the_setting_off_the_style_winit_writes_is_already_right() {
    assert_eq!(
        style_plan(Some((5, false)), 5, false, WINIT_STYLE),
        plan(false, false, false)
    );
    // A new window with the setting off only gets its own button.
    assert_eq!(
        style_plan(None, 5, false, WINIT_STYLE),
        plan(false, true, false)
    );
}

#[test]
fn a_new_window_or_a_changed_setting_is_styled_and_not_counted_as_lost() {
    assert_eq!(
        style_plan(None, 5, true, WINIT_STYLE),
        plan(true, true, false)
    );
    assert_eq!(
        style_plan(Some((4, true)), 5, true, WINIT_STYLE),
        plan(true, true, false)
    );
    let tool = ex_style(WINIT_STYLE, true);
    assert_eq!(
        style_plan(Some((5, true)), 5, false, tool),
        plan(true, true, false)
    );
}

#[test]
fn the_level_is_set_once_per_window_and_again_only_when_the_pin_flips() {
    assert!(level_changed(None, 5, true), "a new window gets its level");
    assert!(
        !level_changed(Some((5, true)), 5, true),
        "same pin, nothing to do"
    );
    assert!(level_changed(Some((5, true)), 5, false), "unpinned");
    assert!(
        level_changed(Some((5, false)), 7, false),
        "a new window after a close"
    );
}
