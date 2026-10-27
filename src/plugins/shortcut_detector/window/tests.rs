use super::super::probe::{classify, Status};
use super::*;
use crate::core::config::{ThemeChoice, ThemeName};

fn key(text: &str) -> Hotkey {
    hotkeys::parse(text).expect(text)
}

fn scan() -> Arc<ScanResult> {
    Arc::new(ScanResult {
        entries: vec![
            classify(key("Win+E"), false, None),
            classify(key("Ctrl+Alt+F9"), true, None),
            classify(key("Win+Alt+S"), false, Some(("ShortcutDetector", "Open"))),
            classify(key("Ctrl+Shift+F7"), false, None),
        ],
        probed: 1_575,
        elapsed_ms: 57,
    })
}

fn view(scan: Option<Arc<ScanResult>>) -> View {
    View {
        scan,
        scanning: false,
        show_free: false,
    }
}

/// Draws one frame per entry of `frames`, each with its events, then one
/// more in each theme, light and dark, with none.
fn frames(view: &View, state: &mut WindowUi, frames: Vec<Vec<Event>>) -> Vec<Outcome> {
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    let mut out = Vec::new();
    let themes = ThemeName::ALL.into_iter().flat_map(|name| {
        [ThemeChoice::Light, ThemeChoice::Dark].map(|mode| (name, mode, Vec::new()))
    });
    let steps = frames
        .into_iter()
        .map(|events| (ThemeName::Classic, ThemeChoice::Dark, events))
        .chain(themes);
    for (name, mode, events) in steps {
        theme::apply(&ctx, name, mode);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(760.0, 560.0))),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| contents(ui, view, state, &mut out));
        output.textures_delta.clear();
    }
    out
}

fn press(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn the_window_draws_in_every_theme_while_scanning_empty_and_full() {
    let mut state = WindowUi::default();
    let scanning = View {
        scanning: true,
        ..view(None)
    };
    assert!(frames(&scanning, &mut state, Vec::new()).is_empty());
    state.query = "nothing like this".to_string();
    assert!(frames(&view(Some(scan())), &mut state, Vec::new()).is_empty());
    state.query.clear();
    state.verdict = Some((
        "Free \u{b7} no app or Windows shortcut uses it".to_string(),
        Tone::Success,
    ));
    state.keys = Some(key("Ctrl+Shift+K"));
    state.notice = Some(("Copied 3 rows.".to_string(), Instant::now()));
    assert!(frames(&view(Some(scan())), &mut state, Vec::new()).is_empty());
    assert_eq!(state.window, 0, "no real window is touched");
}

#[test]
fn keys_select_copy_scan_and_close() {
    let mut state = WindowUi::default();
    let shown = view(Some(scan()));
    let down = press(Key::ArrowDown, Modifiers::NONE);
    let out = frames(&shown, &mut state, vec![vec![down.clone()], vec![down]]);
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(state.selected, Some(1));

    let out = frames(
        &shown,
        &mut state,
        vec![vec![press(Key::C, Modifiers::COMMAND)]],
    );
    let [Outcome::Copy(text)] = out.as_slice() else {
        panic!("{out:?}");
    };
    assert!(text.starts_with("Shortcut\tStatus\tMeaning\tSource\r\n"));
    assert_eq!(text.lines().count(), 4, "the three taken rows and a header");

    let out = frames(
        &shown,
        &mut state,
        vec![vec![
            press(Key::F5, Modifiers::NONE),
            press(Key::Escape, Modifiers::NONE),
        ]],
    );
    assert_eq!(out, [Outcome::Close, Outcome::Scan]);
}

#[test]
fn enter_on_the_box_starts_listening_and_nothing_else_does() {
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    let mut state = WindowUi::default();
    let shown = view(Some(scan()));
    let mut out = Vec::new();
    let run = |events: Vec<Event>, state: &mut WindowUi, out: &mut Vec<Outcome>| {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(760.0, 560.0))),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| contents(ui, &shown, state, out));
        output.textures_delta.clear();
    };
    run(
        vec![press(Key::Enter, Modifiers::NONE)],
        &mut state,
        &mut out,
    );
    assert!(out.is_empty(), "the box has no focus yet");
    ctx.memory_mut(|memory| memory.request_focus(capture_id()));
    run(Vec::new(), &mut state, &mut out);
    run(
        vec![press(Key::Enter, Modifiers::NONE)],
        &mut state,
        &mut out,
    );
    assert_eq!(out, [Outcome::StartCapture]);
}

#[test]
fn the_toolbar_wraps_on_a_narrow_window() {
    let widths = [200.0, 230.0, 110.0, 200.0];
    assert_eq!(flow(&widths, 1000.0), [vec![0, 1, 2, 3]]);
    assert_eq!(flow(&widths, 720.0), [vec![0, 1, 2], vec![3]]);
    assert_eq!(flow(&widths, 520.0), [vec![0, 1], vec![2, 3]]);
    assert_eq!(flow(&widths, 100.0).len(), 4, "each on its own line");
}

#[test]
fn a_verdict_says_when_the_scan_said_otherwise() {
    let free = classify(key("Ctrl+Shift+F7"), false, None);
    let (line, tone) = verdict_line(&free, Some(Status::Free));
    assert_eq!(line, "Free \u{b7} no app or Windows shortcut uses it");
    assert_eq!(tone, Tone::Success);
    let (line, _) = verdict_line(&free, Some(Status::TakenByApp));
    assert!(line.ends_with(CHANGED), "{line}");
    let (_, tone) = verdict_line(&classify(key("Win+E"), false, None), None);
    assert_eq!(tone, Tone::Info);
}

#[test]
fn the_bottom_line_shows_the_numbers_or_the_scan_and_a_note() {
    let state = WindowUi::default();
    let (parts, note) = footer_parts(&view(Some(scan())), &state);
    assert_eq!(
        parts,
        [
            "1,575 combinations checked in 57 ms",
            "1 taken by apps",
            "1 Windows",
            "1 WinCraft"
        ]
    );
    assert_eq!(note, probe::HOOK_NOTE);
    let scanning = View {
        scanning: true,
        ..view(Some(scan()))
    };
    assert_eq!(footer_parts(&scanning, &state).0, ["Scanning\u{2026}"]);
    let copied = WindowUi {
        notice: Some(("Copied 3 rows.".to_string(), Instant::now())),
        ..WindowUi::default()
    };
    assert_eq!(footer_parts(&view(None), &copied).1, "Copied 3 rows.");
}

#[test]
fn the_box_shows_held_modifiers_then_the_combination() {
    let mut state = WindowUi {
        capturing: true,
        live: windows_sys::Win32::UI::Input::KeyboardAndMouse::MOD_WIN,
        ..WindowUi::default()
    };
    assert_eq!(box_keys(&state), ["Win"]);
    state.live = 0;
    state.keys = Some(key("Win+Alt+K"));
    assert_eq!(box_keys(&state), ["Win", "Alt", "K"]);
    state.keys = None;
    assert!(box_keys(&state).is_empty());
}
