use super::*;

#[test]
fn arrows_move_in_rows_of_four() {
    // Six cards: 0 1 2 3 / 4 5.
    assert_eq!(step(ListKey::Right, 0, 6, false), Step::Select(1));
    assert_eq!(step(ListKey::Right, 5, 6, false), Step::Select(5));
    assert_eq!(step(ListKey::Left, 0, 6, false), Step::Select(0));
    assert_eq!(step(ListKey::Down, 1, 6, false), Step::Select(5));
    assert_eq!(
        step(ListKey::Down, 3, 6, false),
        Step::Select(5),
        "shorter row"
    );
    assert_eq!(step(ListKey::Down, 5, 6, false), Step::Select(5));
    assert_eq!(step(ListKey::Up, 5, 6, false), Step::Select(1));
    assert_eq!(step(ListKey::Up, 2, 6, false), Step::Select(2));
    assert_eq!(
        step(ListKey::Down, 9, 6, false),
        Step::Select(5),
        "past the end"
    );
}

#[test]
fn ctrl_arrows_reorder_and_the_other_keys_jump_unpin_or_close() {
    assert_eq!(
        step(ListKey::CtrlRight, 0, 6, false),
        Step::Move { from: 0, to: 1 }
    );
    assert_eq!(
        step(ListKey::CtrlDown, 2, 6, false),
        Step::Move { from: 2, to: 5 }
    );
    assert_eq!(step(ListKey::CtrlLeft, 0, 6, false), Step::Nothing);
    assert_eq!(step(ListKey::Enter, 2, 6, false), Step::Jump(2));
    assert_eq!(step(ListKey::Digit(3), 0, 6, false), Step::Jump(2));
    assert_eq!(step(ListKey::Digit(9), 0, 6, false), Step::Nothing);
    assert_eq!(step(ListKey::Digit(0), 0, 10, false), Step::Jump(9));
    assert_eq!(step(ListKey::Delete, 4, 6, false), Step::Remove(4));
    assert_eq!(step(ListKey::F2, 1, 6, false), Step::Details(1));
    assert_eq!(step(ListKey::Escape, 0, 6, true), Step::CloseDetails);
    assert_eq!(step(ListKey::Escape, 0, 6, false), Step::Close);
    assert_eq!(step(ListKey::Enter, 0, 0, false), Step::Nothing);
    assert_eq!(step(ListKey::Escape, 0, 0, false), Step::Close);
}

#[test]
fn the_header_counts_of_ten_and_lists_the_keys_that_work() {
    assert_eq!(count_line(0), "0 of 10");
    assert_eq!(count_line(7), "7 of 10");
    let keys: Vec<&str> = hints(3).iter().map(|(key, _)| *key).collect();
    assert_eq!(keys, ["Esc", "Del", "F2", "1\u{2013}0"], "right to left");
    assert_eq!(hints(3)[3].1, "Switch");
    assert_eq!(hints(0), [("Esc", "Close")], "an empty list has only Esc");
}

#[test]
fn a_card_that_is_not_open_has_the_quiet_title() {
    let tokens = Tokens::dark();
    assert_eq!(title_colour(State::NotOpen, &tokens), tokens.text_secondary);
    assert_eq!(title_colour(State::Open, &tokens), tokens.text_primary);
}

#[test]
fn the_window_fits_the_rows_of_cards() {
    assert_eq!(window_size(0, false).x, 660.0, "empty: the two-card width");
    assert_eq!(window_size(0, false).x, window_size(2, false).x);
    assert_eq!(window_size(0, false).y, 538.0, "the design's empty window");
    assert_eq!(window_size(1, false).x, 660.0, "two columns at least");
    assert_eq!(window_size(4, false).x, 1300.0);
    assert_eq!(window_size(10, false).x, 1300.0);
    assert!(window_size(5, false).y > window_size(4, false).y);
    assert_eq!(window_size(3, true).x, 980.0 + DETAILS);
    let second = card_rect(pos2(0.0, 0.0), 4);
    assert_eq!(second.left(), 0.0);
    assert!(second.top() > CARD_H + CAP);
}

#[test]
fn a_card_icon_is_faded_only_when_the_window_is_not_open() {
    assert_eq!(icon_alpha(State::Open), 1.0);
    assert_eq!(icon_alpha(State::InFront), 1.0);
    assert_eq!(icon_alpha(State::NotOpen), NOT_OPEN);
}

#[test]
fn the_fallback_icon_shows_the_apps_first_letter() {
    assert_eq!(first_letter("notepad"), "N");
    assert_eq!(first_letter("  .Ärger"), "Ä");
    assert_eq!(first_letter(""), "?");
}

#[test]
fn the_title_starts_right_of_the_icon_with_a_10_px_gap() {
    assert_eq!(TEXT_SHIFT, 42.0);
}
