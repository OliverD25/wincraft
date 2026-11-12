//! What surrounds the drawing: the plugin's calls, the window's own
//! helpers, the kept place, the chat list's first read and the modes.

use std::time::Duration;

use super::deck::{sample, Deck};
use super::screen::Screen;
use super::*;
use crate::core::config::ThemeChoice;
use crate::core::traits::{HostContext, PluginWindow, WinCraftPlugin};
use crate::core::ui_bridge::{self, ArrangeAction, HostRequest};
use crate::plugins::claude_deck::window::{self, DeckUi, DeckWindow};
use crate::plugins::claude_deck::{data, modes, place, status, ClaudeDeck, Settings};

fn scratch(name: &str) -> std::path::PathBuf {
    let folder = std::env::temp_dir().join(format!(
        "wincraft-claude-deck-around-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("a scratch folder");
    folder
}

#[test]
fn the_plugin_answers_the_host_with_its_hotkeys_tray_entry_and_window() {
    let mut deck = ClaudeDeck::new();
    let meta = deck.metadata();
    assert_eq!(meta.name, "ClaudeDeck");
    assert!(!meta.readme.is_empty());
    let hotkeys = deck.hotkey_actions();
    let names: Vec<&str> = hotkeys.iter().map(|h| h.name).collect();
    assert_eq!(names, ["open", "mini"]);
    assert_eq!(hotkeys[0].default.vk, u32::from(b'C'));
    assert_eq!(hotkeys[1].default, crate::core::traits::Hotkey::NONE);
    let tray = deck.tray_actions();
    assert_eq!(tray.len(), 1);
    assert_eq!(tray[0].label, "ClaudeDeck");
    assert_eq!(deck.windows().len(), 1);
    assert_eq!(deck.windows()[0].id(), window::WINDOW_ID);

    let settings = serde_json::json!({ "mini_mode": true, "only_open_windows": true });
    assert!(deck.on_settings_changed(&settings));
    let context = HostContext {
        hwnd: std::ptr::null_mut(),
        settings: &settings,
    };
    deck.init(&context).expect("init");
    deck.on_arrange_action(&ArrangeAction::Activate(0x7fff_fff0));
    deck.on_arrange_action(&ArrangeAction::Reorder {
        group: "g".into(),
        order: Vec::new(),
    });
    deck.on_hotkey(2);
    deck.on_hotkey(1);
    deck.on_hotkey(99);
    deck.on_tray_action(1);
    deck.on_tray_action(7);
    deck.on_windows_message(0, 0, 0);
    deck.on_windows_message(crate::core::host::PLUGIN_POST, 0xC1A0_DEC0, 0);
    deck.teardown();
}

#[test]
fn the_window_object_opens_and_closes_without_a_native_window() {
    let shared = std::sync::Arc::new(std::sync::Mutex::new(crate::plugins::claude_deck::Shared {
        open: true,
        by_hotkey: Some(5),
        keep_open: false,
        ..Default::default()
    }));
    let mut window = DeckWindow::new(std::sync::Arc::clone(&shared));
    assert!(window.is_open());
    window.open();
    {
        let held = shared.lock().expect("shared");
        assert!(held.by_hotkey.is_none(), "the hotkey's window is taken");
        assert!(!held.keep_open);
    }
    window.close();
    assert!(!window.is_open());
}

#[test]
fn building_the_lines_is_cached_until_the_chats_or_the_view_change() {
    let mut state = DeckUi::default();
    state.chats = sample();
    state.saved = arrange(&state.saved, &state.chats, NOW);
    let settings = Settings::default();
    state.built = Some(std::time::Instant::now());
    window::build(&mut state, &settings, NOW);
    let first = std::sync::Arc::clone(&state.cache.lines);
    assert!(!first.is_empty());
    assert_eq!(
        state
            .cache
            .counts
            .iter()
            .find(|(m, _)| *m == Mark::AsksYou)
            .map(|c| c.1),
        Some(1)
    );
    assert_eq!(state.cache.waiting, 1);
    assert_eq!(state.cache.waiting_ids, ["a2"]);
    window::build(&mut state, &settings, NOW);
    assert!(
        std::sync::Arc::ptr_eq(&first, &state.cache.lines),
        "nothing changed"
    );
    state.built = Some(std::time::Instant::now() + Duration::from_secs(1));
    window::build(&mut state, &settings, NOW);
    let second = std::sync::Arc::clone(&state.cache.lines);
    assert!(
        !std::sync::Arc::ptr_eq(&first, &second),
        "the read time changed"
    );
    state.saved.hidden.insert("h".into());
    window::build(&mut state, &settings, NOW);
    assert_eq!(window::listed(&state).len(), state.chats.len() - 1);
    let only_open = Settings {
        only_open: true,
        ..Settings::default()
    };
    window::build(&mut state, &only_open, NOW);
    assert!(
        state.cache.lines.len() < second.len(),
        "only the chats above an open window stay"
    );
}

#[test]
fn the_next_paint_is_soon_while_a_ring_turns_and_each_second_for_a_young_time() {
    let mut state = DeckUi::default();
    let none: Vec<Line> = Vec::new();
    assert_eq!(
        window::next_frame(&state, &none, NOW),
        Duration::from_secs(1),
        "before the first read"
    );
    state.chats = sample();
    let slow = window::next_frame(&state, &none, NOW);
    assert!(slow >= Duration::from_secs(1));
    state.saved = arrange(&state.saved, &state.chats, NOW);
    let settings = Settings::default();
    window::build(&mut state, &settings, NOW);
    let lines = std::sync::Arc::clone(&state.cache.lines);
    let quiet = window::next_frame(&state, &lines, NOW);
    assert!(quiet >= Duration::from_secs(1));
    state.animate = true;
    assert_eq!(
        window::next_frame(&state, &lines, NOW),
        Duration::from_millis(100),
        "a Working chat"
    );
    state.animate = false;
    let mut fresh = state.chats.clone();
    fresh[1].since = Some(NOW - 10_000);
    state.chats = fresh;
    state.saved = arrange(&state.saved, &state.chats, NOW);
    state.built = Some(std::time::Instant::now());
    window::build(&mut state, &settings, NOW);
    let lines = std::sync::Arc::clone(&state.cache.lines);
    assert_eq!(
        window::next_frame(&state, &lines, NOW),
        Duration::from_secs(1)
    );
    assert_eq!(
        window::ring_turn(&state),
        0.0,
        "no turning while animation is off"
    );
    state.animate = true;
    state.started = Some(std::time::Instant::now());
    let turn = window::ring_turn(&state);
    assert!((0.0..1.0).contains(&turn));
}

#[test]
fn nothing_is_written_while_the_arrangement_is_not_writable() {
    let mut state = DeckUi::default();
    state.saved.cold_open = true;
    window::commit(&mut state);
    assert!(state.saved.cold_open, "the list in memory is kept as it is");
}

#[test]
fn the_empty_text_is_centred_in_the_list_area() {
    let mut screen = Screen::new(ThemeChoice::Light);
    let tokens = screen.tokens();
    let none = screen.frame(Vec::new(), |ui| window::empty(ui, &tokens, false));
    let some = screen.frame(Vec::new(), |ui| window::empty(ui, &tokens, true));
    assert!(none.has("Claude Code runs"));
    assert!(some.has("No chat has a window open."));
}

#[test]
fn mini_mode_has_a_smaller_least_size_and_switching_without_a_window_does_nothing() {
    assert_eq!(modes::min_size(false), window::MIN_SIZE);
    let mini = modes::min_size(true);
    assert!(mini[0] < window::MIN_SIZE[0]);
    let screen = Screen::new(ThemeChoice::Dark);
    let mut state = DeckUi::default();
    modes::switch_mode(&screen.ctx, &mut state, true, &[]);
    assert!(state.mini_on.is_none(), "no window yet");
}

#[test]
fn switching_modes_with_a_window_remembers_the_mode_and_its_size() {
    let screen = Screen::new(ThemeChoice::Dark);
    let mut state = DeckUi::default();
    state.window = 0x7fff_fff0;
    let deck = Deck::new(sample(), Settings::default());
    let lines = deck.lines();
    modes::switch_mode(&screen.ctx, &mut state, false, &lines);
    assert_eq!(state.mini_on, Some(false));
    assert!(state.mini_size.is_none());
    modes::switch_mode(&screen.ctx, &mut state, true, &lines);
    assert_eq!(state.mini_on, Some(true));
    assert!(state
        .mini_size
        .is_some_and(|size| size.x == crate::plugins::claude_deck::mini::WIDTH));
    modes::switch_mode(&screen.ctx, &mut state, true, &lines);
    state.kept = None;
    modes::switch_mode(&screen.ctx, &mut state, false, &lines);
    assert_eq!(state.mini_on, Some(false));
    assert!(state.mini_size.is_none());
}

#[test]
fn giving_the_keyboard_back_forgets_a_window_that_is_gone() {
    let bridge = ui_bridge::create();
    let mut state = DeckUi::default();
    state.keyboard = true;
    modes::give_back(&mut state, &bridge.to_host);
    assert!(state.keyboard, "nothing to give it back to");
    state.before = Some(0x7fff_fff0);
    modes::give_back(&mut state, &bridge.to_host);
    assert!(!state.keyboard);
    assert!(state.before.is_none(), "that window is gone");
    assert!(bridge.host_rx.try_recv().is_err());
}

#[test]
fn the_tray_tip_is_handed_over_once() {
    let mut shared = crate::plugins::claude_deck::Shared::default();
    let mut state = DeckUi::default();
    modes::tell_tray(&mut shared, &mut state, Some("2 chats wait".into()));
    assert_eq!(shared.tray_tip.as_deref(), Some("2 chats wait"));
    shared.tray_tip = None;
    modes::tell_tray(&mut shared, &mut state, Some("2 chats wait".into()));
    assert!(shared.tray_tip.is_none(), "the same tip is not sent again");
    modes::tell_tray(&mut shared, &mut state, None);
    assert!(shared.tray_tip.is_none());
    assert_eq!(state.tip_sent, Some(None));
}

#[test]
fn the_fade_steps_with_the_mouse_and_the_focus() {
    let mut screen = Screen::new(ThemeChoice::Dark);
    let mut state = DeckUi::default();
    let mut next = None;
    screen.frame(Vec::new(), |ui| {
        next = modes::fade_step(ui.ctx(), &mut state, true);
    });
    assert!(next.is_none() || next.is_some_and(|d| d <= Duration::from_secs(5)));
    state.cache.waiting_ids = vec!["a".into()];
    screen.frame(Vec::new(), |ui| {
        modes::fade_step(ui.ctx(), &mut state, false);
    });
    assert_eq!(state.waiting_before, ["a"]);
}

#[test]
fn the_kept_place_saves_loads_and_ignores_what_is_too_small_or_broken() {
    let folder = scratch("place");
    let path = folder.join("claude_deck.place.json");
    assert_eq!(place::load(&path), None, "no file yet");
    let wanted = place::Place {
        x: -300,
        y: 40,
        width: 360,
        height: 480,
    };
    place::save(&path, wanted);
    assert_eq!(place::load(&path), Some(wanted));
    let tiny = place::Place {
        width: 20,
        ..wanted
    };
    place::save(&path, tiny);
    assert_eq!(place::load(&path), None, "too small to keep");
    std::fs::write(&path, "{ not json").expect("write");
    assert_eq!(place::load(&path), None, "a broken file");
    place::save(&folder, wanted);
    assert!(place::path().ends_with("claude_deck.place.json"));
    assert!(place::read(0).is_none(), "no window, no place");
    assert!(!place::worth_keeping(true, true, wanted));
    assert!(!place::worth_keeping(false, false, wanted));
    let minimised = place::Place {
        x: -32000,
        ..wanted
    };
    assert!(!place::worth_keeping(false, true, minimised));
    let _ = std::fs::remove_dir_all(&folder);
}

#[test]
fn the_chat_list_of_an_empty_read_is_empty_and_needs_no_archive() {
    let seen = status::Seen::default();
    let none = data::chats(&[], &[], &seen, NOW);
    assert!(none.is_empty());
    assert!(data::archived(&none).is_empty());
    let whole = vec![chat("a", None, 1), fork("f", "a", 2)];
    assert!(data::archived(&whole).is_empty(), "the parent is listed");
}

#[test]
fn a_chat_request_from_the_window_reaches_the_host_channel() {
    let bridge = ui_bridge::create();
    window::bring_forward(Some(77), &bridge.to_host);
    window::bring_forward(None, &bridge.to_host);
    let sent: Vec<HostRequest> = bridge.host_rx.try_iter().collect();
    assert_eq!(sent.len(), 1);
    assert!(matches!(
        &sent[0],
        HostRequest::Arrange {
            action: ArrangeAction::Activate(77),
            ..
        }
    ));
}
