use super::super::matching::slot_views;
use super::super::matching::tests::window;
use super::super::pins::tests::list;
use super::super::pins::Slots;
use super::*;
use crate::core::config::ThemeName;

fn view() -> View {
    let slots = list(&["Claude", "Code"]);
    let live = vec![window(1, "Claude", "Claude")];
    View {
        slots: slot_views(&slots, &live, 1),
        pins: slots.iter().map(|(_, pin)| pin.clone()).collect(),
        windows: live,
        apps: vec![super::super::shared::App {
            exe: r"C:\Apps\Claude.exe".to_string(),
            app_id: None,
            name: "Claude".to_string(),
        }],
        jump_keys: "Win+`".to_string(),
        pin_keys: String::new(),
    }
}

/// Draws one frame per entry of `frames`, each with its events, then
/// one more in each theme with none.
fn frames(view: &View, state: &mut ListUi, frames: Vec<Vec<egui::Event>>) -> Vec<Outcome> {
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    let mut out = Vec::new();
    let themes = ThemeName::ALL.into_iter().map(|name| (name, Vec::new()));
    let steps = frames
        .into_iter()
        .map(|events| (ThemeName::Classic, events))
        .chain(themes);
    let size = window_size(view.slots.len(), true);
    for (name, events) in steps {
        theme::apply(&ctx, name, crate::core::config::ThemeChoice::Dark);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), size)),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| grid::contents(ui, view, state, &mut out));
        output.textures_delta.clear();
    }
    out
}

fn draw(view: &View, state: &mut ListUi, events: Vec<egui::Event>) -> Vec<Outcome> {
    frames(view, state, vec![Vec::new(), events])
}

fn key(key: Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn quick_access_and_its_details_draw_in_every_theme() {
    let view = view();
    let mut state = ListUi::default();
    let first = draw(&view, &mut state, Vec::new());
    assert!(first.is_empty(), "{first:?}");
    state.details = draft(&view, 0);
    assert!(state.details.is_some());
    draw(&view, &mut state, Vec::new());
    assert!(window_of(&state).is_null(), "no real window is touched");
    let out = draw(&view, &mut state, vec![key(Key::Escape)]);
    assert!(out.is_empty());
    assert!(state.details.is_none(), "Esc drops the details first");
}

fn click(at: egui::Pos2) -> Vec<Vec<egui::Event>> {
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    vec![
        Vec::new(),
        vec![egui::Event::PointerMoved(at)],
        vec![button(true)],
        vec![button(false)],
    ]
}

#[test]
fn a_click_on_the_selected_card_opens_its_details() {
    let view = view();
    let mut state = ListUi::default();
    // The first card, which starts selected.
    frames(&view, &mut state, click(pos2(150.0, 150.0)));
    assert_eq!(state.details.as_ref().map(|d| d.slot), Some(0));
}

#[test]
fn keys_in_quick_access_become_requests() {
    let view = view();
    let mut state = ListUi::default();
    let out = draw(&view, &mut state, vec![key(Key::Delete)]);
    assert!(out.contains(&Outcome::Send(Request::Remove(0))));
    let mut state = ListUi::default();
    let out = draw(&view, &mut state, vec![key(Key::Enter)]);
    assert!(out.contains(&Outcome::Send(Request::Jump(0))));
    assert!(out.contains(&Outcome::Close));
    let mut state = ListUi::default();
    let out = draw(&view, &mut state, vec![key(Key::Num2)]);
    assert!(out.contains(&Outcome::Send(Request::Jump(1))));
    let mut state = ListUi::default();
    let out = draw(&view, &mut state, vec![key(Key::ArrowRight)]);
    assert!(out.is_empty());
    assert_eq!(state.selected, 1);
}

#[test]
fn with_no_pins_one_plain_line_shows_and_esc_closes() {
    let mut empty = view();
    empty.slots = slot_views(&Slots::default(), &[], 0);
    empty.pins.clear();
    let mut state = ListUi::default();
    let out = draw(&empty, &mut state, vec![key(Key::Escape)]);
    assert!(out.contains(&Outcome::Close));
}

#[test]
fn the_empty_state_opens_the_picker_with_enter_and_with_its_button() {
    let mut empty = view();
    empty.slots = slot_views(&Slots::default(), &[], 0);
    empty.pins.clear();
    let mut state = ListUi::default();
    let quiet = draw(&empty, &mut state, Vec::new());
    assert!(quiet.is_empty(), "drawing alone asks for nothing");
    let out = draw(&empty, &mut state, vec![key(Key::Enter)]);
    assert!(out.contains(&Outcome::Send(Request::OpenPicker)), "{out:?}");
    // Without a jump shortcut the state still draws, with its button.
    empty.jump_keys.clear();
    let mut state = ListUi::default();
    let out = draw(&empty, &mut state, vec![key(Key::Enter)]);
    assert!(out.contains(&Outcome::Send(Request::OpenPicker)));
    // With pins Enter jumps, it never opens the picker.
    let mut state = ListUi::default();
    let out = draw(&view(), &mut state, vec![key(Key::Enter)]);
    assert!(!out.contains(&Outcome::Send(Request::OpenPicker)));
}

#[test]
fn an_unopened_pin_draws_with_its_big_icon_in_every_theme() {
    let mut view = view();
    view.slots = slot_views(&list(&["Claude", "Code"]), &[], 0);
    let mut state = ListUi::default();
    let out = draw(&view, &mut state, Vec::new());
    assert!(out.is_empty());
    state.details = draft(&view, 1);
    draw(&view, &mut state, Vec::new());
}

/// The window Quick access draws on, for tests that check nothing is
/// drawn on a real one.
fn window_of(state: &ListUi) -> windows_sys::Win32::Foundation::HWND {
    state.window as windows_sys::Win32::Foundation::HWND
}
