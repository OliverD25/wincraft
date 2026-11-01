//! The palette itself, driven by keys over fake apps and windows. Nothing
//! here reads or touches a real window.

use super::tests::{app_row, many, notepad, plain};
use super::*;
use crate::core::theme;
use crate::core::ui_bridge::UiSnapshot;
use crate::search::{Context, Router};
use crate::ui::palette::{Outcome, Palette, HEIGHT, WIDTH};

struct AppRows(Vec<ResultItem>);

impl crate::search::SearchProvider for AppRows {
    fn id(&self) -> &'static str {
        "apps"
    }
    fn name(&self) -> &'static str {
        "Apps"
    }
    fn description(&self) -> &'static str {
        "test apps"
    }
    fn blended(&self) -> bool {
        true
    }
    fn query(&mut self, _query: &crate::search::Query, _context: &Context) -> Vec<ResultItem> {
        self.0.clone()
    }
}

fn palette_with(windows: Vec<OpenWindow>) -> Palette {
    let pad = notepad();
    let mut row = app_row(&pad);
    row.provider = String::new();
    row.usage_key = Some(pad.usage_key.clone());
    let mut palette = Palette::new(Router::new(
        vec![(None, Box::new(AppRows(vec![row, plain("Next")])))],
        &Default::default(),
    ));
    palette.opened();
    palette.running.fake = Some((vec![pad], windows));
    palette.type_in("note");
    palette
}

fn press(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn frame(palette: &mut Palette, ctx: &egui::Context, events: Vec<egui::Event>) -> Outcome {
    frame_with(palette, ctx, &crate::core::ui_bridge::create(), events)
}

fn frame_with(
    palette: &mut Palette,
    ctx: &egui::Context,
    bridge: &crate::core::ui_bridge::Bridge,
    events: Vec<egui::Event>,
) -> Outcome {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(WIDTH, HEIGHT),
        )),
        events,
        ..Default::default()
    };
    let mut outcome = Outcome::Stay;
    let mut output = ctx.run_ui(input, |ui| {
        outcome = palette.show(ui, &UiSnapshot::default(), &bridge.to_host, 1.0, true);
    });
    output.textures_delta.clear();
    outcome
}

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    theme::apply(
        &ctx,
        crate::core::config::ThemeName::Classic,
        crate::core::config::ThemeChoice::Dark,
    );
    ctx
}

const NONE: egui::Modifiers = egui::Modifiers::NONE;

#[test]
fn the_palette_nests_the_windows_and_the_arrows_walk_them() {
    let ctx = context();
    let mut palette = palette_with(many(2));
    frame(&mut palette, &ctx, Vec::new());
    assert_eq!(palette.results.items.len(), 4);
    assert_eq!(palette.spot(), Some(Spot::App));
    frame(&mut palette, &ctx, vec![press(egui::Key::ArrowDown, NONE)]);
    assert_eq!(palette.selected, 1);
    assert_eq!(palette.spot(), Some(Spot::Window));
    frame(&mut palette, &ctx, vec![press(egui::Key::ArrowUp, NONE)]);
    assert_eq!(palette.selected, 0);
    // Up and Down never move the text cursor (US-001): the box keeps "note".
    assert_eq!(palette.query, "note");
}

#[test]
fn n_more_expands_and_esc_or_left_collapse_before_esc_closes() {
    let ctx = context();
    let mut palette = palette_with(many(15));
    frame(&mut palette, &ctx, Vec::new());
    assert_eq!(palette.results.items.len(), 1 + 4 + 1);
    palette.selected = 4;
    assert_eq!(palette.spot(), Some(Spot::More));
    frame(&mut palette, &ctx, vec![press(egui::Key::Enter, NONE)]);
    frame(&mut palette, &ctx, Vec::new());
    assert!(palette.expanded());
    assert_eq!(palette.results.items.len(), 1 + 15 + 1);
    assert_eq!(palette.spot(), Some(Spot::Window));
    // Esc folds the list; the palette stays.
    let outcome = frame(&mut palette, &ctx, vec![press(egui::Key::Escape, NONE)]);
    assert!(matches!(outcome, Outcome::Stay));
    frame(&mut palette, &ctx, Vec::new());
    assert!(!palette.expanded());
    assert_eq!(palette.selected, 4);
    // Expand again, then ← folds it too.
    frame(&mut palette, &ctx, vec![press(egui::Key::Enter, NONE)]);
    frame(&mut palette, &ctx, Vec::new());
    palette.selected = 9;
    frame(&mut palette, &ctx, vec![press(egui::Key::ArrowLeft, NONE)]);
    frame(&mut palette, &ctx, Vec::new());
    assert!(!palette.expanded());
    assert_eq!(palette.selected, 4);
    // Now Esc closes.
    let outcome = frame(&mut palette, &ctx, vec![press(egui::Key::Escape, NONE)]);
    assert!(matches!(outcome, Outcome::Hide));
}

#[test]
fn ctrl_d_opens_the_view_by_desktop_and_ctrl_d_or_backspace_return() {
    let ctx = context();
    let mut palette = palette_with(many(3));
    frame(&mut palette, &ctx, Vec::new());
    frame(
        &mut palette,
        &ctx,
        vec![press(egui::Key::D, egui::Modifiers::COMMAND)],
    );
    frame(&mut palette, &ctx, Vec::new());
    assert_eq!(palette.spot(), Some(Spot::ByDesktop));
    assert_eq!(palette.query, "");
    assert_eq!(
        palette.by_desktop_chip().as_deref(),
        Some("Notepad \u{b7} by desktop")
    );
    assert_eq!(palette.results.items.len(), 3);
    assert_eq!(
        palette.results.items[0].group,
        "Desktop 1 \u{b7} this one \u{b7} 3"
    );
    frame(
        &mut palette,
        &ctx,
        vec![press(egui::Key::D, egui::Modifiers::COMMAND)],
    );
    frame(&mut palette, &ctx, Vec::new());
    assert_eq!(palette.query, "note");
    assert_eq!(palette.spot(), Some(Spot::App));

    frame(
        &mut palette,
        &ctx,
        vec![press(egui::Key::D, egui::Modifiers::COMMAND)],
    );
    frame(&mut palette, &ctx, vec![press(egui::Key::Backspace, NONE)]);
    frame(&mut palette, &ctx, Vec::new());
    assert_eq!(palette.query, "note");
    assert_eq!(palette.spot(), Some(Spot::App));
}

#[test]
fn shift_enter_and_alt_digits_pick_the_windows_in_screen_order() {
    let ctx = context();
    let mut palette = palette_with(many(3));
    frame(&mut palette, &ctx, Vec::new());
    let actions = palette.current_actions();
    let shift = actions
        .iter()
        .find(|action| action.key == Key::ShiftEnter)
        .unwrap();
    assert_eq!(shift.run, Run::Action(Action::Activate(100)));
    assert_eq!(shift.short, "Switch");
    assert_eq!(actions[0].label, "Open new window");
    let bridge = crate::core::ui_bridge::create();
    let outcome = frame_with(
        &mut palette,
        &ctx,
        &bridge,
        vec![press(egui::Key::Num2, egui::Modifiers::ALT)],
    );
    assert!(matches!(outcome, Outcome::Hide));
    let sent: Vec<_> = bridge.host_rx.try_iter().collect();
    assert!(sent.iter().any(|request| matches!(
        request,
        crate::core::ui_bridge::HostRequest::RunAction(Action::Activate(101))
    )));
}

#[test]
fn esc_on_a_result_under_the_expanded_list_folds_it_and_keeps_the_cursor() {
    let ctx = context();
    let mut palette = palette_with(many(15));
    frame(&mut palette, &ctx, Vec::new());
    palette.selected = 4;
    frame(&mut palette, &ctx, vec![press(egui::Key::Enter, NONE)]);
    frame(&mut palette, &ctx, Vec::new());
    // App, 15 windows, Next.
    palette.selected = 16;
    assert_eq!(palette.results.items[16].title, "Next");
    let outcome = frame(&mut palette, &ctx, vec![press(egui::Key::Escape, NONE)]);
    assert!(matches!(outcome, Outcome::Stay));
    frame(&mut palette, &ctx, Vec::new());
    assert!(!palette.expanded());
    assert_eq!(palette.results.items[palette.selected].title, "Next");
}
