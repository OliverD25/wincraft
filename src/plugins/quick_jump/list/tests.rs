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

/// Draws one frame per entry of `frames` in `ctx`, in one theme.
fn run(
    ctx: &egui::Context,
    view: &View,
    state: &mut ListUi,
    frames: Vec<Vec<egui::Event>>,
) -> Vec<Outcome> {
    theme::install_fonts(ctx);
    theme::apply(
        ctx,
        ThemeName::Classic,
        crate::core::config::ThemeChoice::Dark,
    );
    let size = window_size(view.slots.len(), true);
    let mut out = Vec::new();
    for events in frames {
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

fn button(at: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: at,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

/// Holds the first card down and carries it over the second.
fn drag_to_second() -> Vec<Vec<egui::Event>> {
    let start = pos2(150.0, 150.0);
    let mut frames = vec![
        Vec::new(),
        vec![egui::Event::PointerMoved(start)],
        vec![button(start, egui::PointerButton::Primary, true)],
    ];
    for x in [170.0, 250.0, 350.0, 450.0, 490.0] {
        frames.push(vec![egui::Event::PointerMoved(pos2(x, 150.0))]);
    }
    frames
}

#[test]
fn a_card_dragged_onto_another_place_sends_the_move() {
    let view = view();
    let mut state = ListUi::default();
    let mut frames = drag_to_second();
    let quiet = run(&egui::Context::default(), &view, &mut state, frames.clone());
    assert!(quiet.is_empty(), "nothing is sent while it is dragged");
    assert_eq!(state.motion.drag.map(|drag| drag.gap), Some(1));

    let mut state = ListUi::default();
    let end = pos2(490.0, 150.0);
    frames.push(vec![button(end, egui::PointerButton::Primary, false)]);
    frames.push(Vec::new());
    let out = run(&egui::Context::default(), &view, &mut state, frames);
    assert_eq!(out, vec![Outcome::Send(Request::Move { from: 0, to: 1 })]);
    assert_eq!(state.selected, 1, "the dropped card stays selected");
    assert!(state.motion.drag.is_none());
}

#[test]
fn esc_during_a_drag_puts_the_card_back_and_sends_nothing() {
    let view = view();
    let mut state = ListUi::default();
    let mut frames = drag_to_second();
    frames.push(vec![key(Key::Escape)]);
    let end = pos2(490.0, 150.0);
    frames.push(vec![button(end, egui::PointerButton::Primary, false)]);
    frames.push(Vec::new());
    let out = run(&egui::Context::default(), &view, &mut state, frames);
    assert!(out.is_empty(), "no move and no close: {out:?}");
    assert!(state.motion.drag.is_none());
}

#[test]
fn a_middle_click_unpins_like_del() {
    let view = view();
    let mut state = ListUi::default();
    let at = pos2(490.0, 150.0);
    let frames = vec![
        Vec::new(),
        vec![egui::Event::PointerMoved(at)],
        vec![button(at, egui::PointerButton::Middle, true)],
        vec![button(at, egui::PointerButton::Middle, false)],
    ];
    let out = run(&egui::Context::default(), &view, &mut state, frames);
    assert_eq!(out, vec![Outcome::Send(Request::Remove(1))]);
    let mut del = ListUi {
        selected: 1,
        ..ListUi::default()
    };
    let by_key = draw(&view, &mut del, vec![key(Key::Delete)]);
    assert_eq!(by_key, out, "the same request, so the same Undo notice");
}

#[test]
fn shift_f10_opens_the_card_menu_and_esc_closes_only_the_menu() {
    let view = view();
    let mut state = ListUi::default();
    let ctx = egui::Context::default();
    let shift_f10 = egui::Event::Key {
        key: Key::F10,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::SHIFT,
    };
    let out = run(&ctx, &view, &mut state, vec![Vec::new(), vec![shift_f10]]);
    assert!(out.is_empty(), "{out:?}");
    assert!(egui::Popup::is_any_open(&ctx), "the menu is open");
    let out = run(
        &ctx,
        &view,
        &mut state,
        vec![vec![key(Key::Escape)], Vec::new()],
    );
    assert!(!out.contains(&Outcome::Close), "Quick access stays open");
    assert!(!egui::Popup::is_any_open(&ctx), "the menu closed");
}
