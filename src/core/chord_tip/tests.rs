use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_WIN};

use super::draw;
use super::*;
use crate::core::config::ThemeName;
use crate::core::theme::Tokens;

fn matched() -> End {
    End::Matched {
        key: "1".to_string(),
        text: "Wincraft".to_string(),
    }
}

#[test]
fn the_tip_waits_150_ms_before_it_shows_and_the_bar_runs_down() {
    let timeline = Timeline::new(3000);
    assert_eq!(timeline.frame(0), Frame::Hidden);
    assert_eq!(timeline.frame(149), Frame::Hidden);
    assert_eq!(timeline.frame(150), Frame::Waiting { left: 0.95 });
    assert_eq!(timeline.frame(1500), Frame::Waiting { left: 0.5 });
    assert_eq!(timeline.frame(3500), Frame::Waiting { left: 0.0 });
    assert_eq!(timeline.frame(5000), Frame::Done, "never stays for good");
}

#[test]
fn a_fast_second_key_never_flashes_the_tip() {
    let mut timeline = Timeline::new(3000);
    timeline.end(80, matched());
    assert_eq!(timeline.frame(100), Frame::Done);
    let mut cancelled = Timeline::new(3000);
    cancelled.end(80, End::Cancelled);
    assert_eq!(cancelled.frame(100), Frame::Done);
    // A key with nothing assigned shows why nothing happened, even fast.
    let mut wrong = Timeline::new(3000);
    wrong.end(
        80,
        End::Unassigned {
            key: "X".to_string(),
        },
    );
    assert_eq!(wrong.frame(100), Frame::Closing { alpha: 1.0 });
}

#[test]
fn each_closing_state_stays_its_time_then_fades_in_150_ms() {
    let cases = [
        (matched(), MATCHED_MS),
        (
            End::Unassigned {
                key: "X".to_string(),
            },
            UNASSIGNED_MS,
        ),
        (
            End::Refused {
                key: "A".to_string(),
                text: "Quick access is full".to_string(),
            },
            UNASSIGNED_MS,
        ),
        (End::Cancelled, CANCELLED_MS),
    ];
    for (end, stays) in cases {
        let mut timeline = Timeline::new(3000);
        timeline.end(500, end.clone());
        assert_eq!(timeline.frame(500), Frame::Closing { alpha: 1.0 });
        assert_eq!(
            timeline.frame(500 + stays - 1),
            Frame::Closing { alpha: 1.0 }
        );
        assert_eq!(
            timeline.frame(500 + stays + 75),
            Frame::Closing { alpha: 0.5 },
            "{end:?}"
        );
        assert_eq!(timeline.frame(500 + stays + FADE_MS), Frame::Done);
        // A later end does not count.
        timeline.end(900, End::Cancelled);
        assert_eq!(timeline.ending(), Some(&end));
    }
    assert_eq!(MATCHED_MS, 600);
    assert_eq!(UNASSIGNED_MS, 1200);
}

#[test]
fn the_closing_states_say_what_happened() {
    assert_eq!(matched().text(), "Wincraft");
    assert_eq!(matched().key(), Some("1"));
    assert_eq!(matched().tone(), Tone::Accent);
    let wrong = End::Unassigned {
        key: "X".to_string(),
    };
    assert_eq!(wrong.text(), "Nothing assigned to X");
    assert_eq!(wrong.tone(), Tone::Danger);
    assert_eq!(End::Cancelled.text(), "Cancelled");
    assert_eq!(End::Cancelled.key(), None);
    assert_eq!(End::Cancelled.tone(), Tone::Quiet);
}

#[test]
fn the_dots_rise_one_after_another() {
    let (low, _) = dot(0, 0);
    assert!((low - 0.25).abs() < 1e-4);
    let (top, lift) = dot(480, 0);
    assert!((top - 1.0).abs() < 1e-4 && (lift - 1.0).abs() < 1e-4);
    assert_eq!(dot(480 + 150, 1), dot(480, 0), "the next dot 150 ms later");
    assert_eq!(dot(1200, 2), dot(0, 2), "one rise every 1.2 s");
}

#[test]
fn a_key_is_named_without_what_is_still_held_from_the_first_press() {
    let stroke = |vk, modifiers| Stroke { vk, modifiers };
    assert_eq!(key_label(stroke(0x58, MOD_WIN), MOD_WIN), "X");
    assert_eq!(key_label(stroke(0x58, MOD_WIN | MOD_ALT), MOD_WIN), "Alt+X");
    assert_eq!(key_label(stroke(0x31, 0), MOD_WIN | MOD_ALT), "1");
}

#[test]
fn the_panel_is_620_wide_with_rows_of_34_in_two_columns() {
    assert_eq!(panel_size(0, 0, true), (620.0, 61.0));
    let (_, four) = panel_size(0, 4, true);
    assert_eq!(
        four,
        58.0 + 1.0 + 10.0 + 34.0 * 2.0 + 14.0 + 3.0,
        "no gap between rows"
    );
    assert_eq!(panel_size(0, 5, true).1, panel_size(0, 6, true).1);
    assert_eq!(
        panel_size(0, 9, false),
        (620.0, 61.0),
        "no list once it ends"
    );
}

#[test]
fn a_pin_grid_and_a_command_grid_are_two_blocks_with_a_hairline_between() {
    let (_, one) = panel_size(0, 4, true);
    let (_, both) = panel_size(6, 4, true);
    assert_eq!(
        both - one,
        34.0 * 3.0 + GROUP_GAP,
        "three more lines and the hairline"
    );
    assert_eq!(
        panel_size(5, 4, true).1,
        both,
        "an odd count still fills its last line"
    );
    assert_eq!(
        panel_size(6, 0, true).1,
        panel_size(0, 6, true).1,
        "no hairline alone"
    );
}

#[test]
fn only_a_content_with_pins_has_a_pin_grid() {
    let rows = vec![Row::new("1", "A"), Row::new("2", "B"), Row::new("`", "C")];
    let plain = Content::of_keys("Win+`", rows.clone());
    assert_eq!(plain.pins, 0);
    // More pins than rows is read as "all rows are pins" and still draws.
    let tokens = Tokens::of(ThemeName::ALL[0], true);
    let all = Content {
        pins: 9,
        ..Content::of_keys("Win+`", rows)
    };
    let picture = draw::render(&all, None, &tokens, 1.0);
    assert_eq!(
        picture.base.height,
        panel_size(3, 0, true).1.round() as i32 + 2 * 16
    );
}

#[test]
fn the_tip_with_pins_is_taller_than_the_same_rows_without() {
    let rows: Vec<Row> = (1..=6).map(|n| Row::new(n.to_string(), "Window")).collect();
    let tokens = Tokens::of(ThemeName::ALL[0], true);
    let flat = draw::render(&Content::of_keys("Win+`", rows.clone()), None, &tokens, 1.0);
    let grouped = draw::render(
        &Content {
            pins: 4,
            ..Content::of_keys("Win+`", rows)
        },
        None,
        &tokens,
        1.0,
    );
    assert_eq!(
        grouped.base.height - flat.base.height,
        GROUP_GAP as i32,
        "the hairline's room"
    );
}

#[test]
fn the_keys_of_a_shortcut_become_keycaps() {
    let content = Content::of_keys("Win+Alt+K", vec![Row::new("S", "Open settings")]);
    assert_eq!(content.keys, ["Win", "Alt", "K"]);
    assert_eq!(content.rows[0].text, "Open settings");
}

#[test]
fn the_tip_draws_in_every_theme_and_the_bar_shrinks() {
    let content = Content::of_keys(
        "Win+`",
        vec![
            Row::new("1", "Wincraft"),
            Row::new(
                "2",
                "A window with a very long title that cannot fit at all",
            ),
            Row::new("`", "Open quick access"),
        ],
    );
    for (name, dark) in ThemeName::ALL
        .into_iter()
        .zip([true, false, true, false, true])
    {
        let tokens = Tokens::of(name, dark);
        let picture = draw::render(&content, None, &tokens, 1.0);
        let full = draw::frame(&picture, 0, 1.0, &tokens);
        let none = draw::frame(&picture, 0, 0.0, &tokens);
        assert_ne!(full.pixels, none.pixels, "{name:?}: the bar shows the time");
        assert_eq!(full.width, picture.base.width);
        for end in [
            matched(),
            End::Unassigned {
                key: "X".to_string(),
            },
            End::Cancelled,
        ] {
            let closing = draw::render(&content, Some(&end), &tokens, 1.5);
            assert!(closing.base.width > 900, "{name:?} {end:?}");
            assert!(closing.base.height < picture.base.height * 2);
        }
    }
}
