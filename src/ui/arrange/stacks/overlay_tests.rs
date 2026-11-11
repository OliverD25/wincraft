//! The open group's layer on the 11-card row of the design page "Window
//! Deck Group Opens In Place": 162 x 124 cards, the NewsHQ stack (3
//! windows) as card 6 at x = 732, in a window 1654 px wide.

use super::*;

const CARD: Vec2 = vec2(162.0, 124.0);

fn window() -> Rect {
    Rect::from_min_size(pos2(0.0, 0.0), vec2(1654.0, 260.0))
}

fn stack_at(x: f32) -> Rect {
    Rect::from_min_size(pos2(x, 44.0), CARD)
}

fn lefts(placed: &Placed) -> Vec<f32> {
    placed.cards.iter().map(|card| card.left()).collect()
}

#[test]
fn a_row_is_centred_on_the_stack_with_tops_level() {
    let stack = stack_at(732.0);
    let placed = place(stack, 3, CARD, window(), Style::Row);
    // As on the page: window 1 covers card 5, window 3 covers card 7.
    assert_eq!(lefts(&placed), [562.0, 732.0, 902.0]);
    assert!(placed.cards.iter().all(|card| card.top() == stack.top()));
    assert!(placed.cards.iter().all(|card| card.size() == CARD));
    assert_eq!(placed.panel.center().x, stack.center().x);
    assert!(
        placed.panel.top() < stack.top() - 16.0,
        "room for the header"
    );
    assert!(window().contains_rect(placed.panel));
}

#[test]
fn a_row_slides_only_as_far_as_the_window_edge() {
    let first = place(stack_at(0.0), 3, CARD, window(), Style::Row);
    assert_eq!(first.panel.left(), MARGIN);
    assert_eq!(first.cards[0].left(), MARGIN + 10.0);
    let last = place(stack_at(1472.0), 3, CARD, window(), Style::Row);
    assert_eq!(last.panel.right(), window().right() - MARGIN);
    assert_eq!(
        last.cards[2].right(),
        window().right() - MARGIN - 10.0,
        "slid left just enough"
    );
    // A stack in the middle does not move at all.
    let middle = place(stack_at(732.0), 3, CARD, window(), Style::Row);
    assert_eq!(middle.cards[1].left(), 732.0);
}

#[test]
fn a_row_too_wide_for_the_window_uses_narrower_cards() {
    let placed = place(stack_at(732.0), 11, CARD, window(), Style::Row);
    let width = placed.cards[0].width();
    assert!((96.0..CARD.x).contains(&width), "{width}");
    assert_eq!(
        placed.cards[0].height(),
        CARD.y,
        "the text line keeps its size"
    );
    assert!(window().shrink(MARGIN - 0.01).contains_rect(placed.panel));
    // Even more windows: the cards stop at the smallest size.
    let many = place(stack_at(732.0), 30, CARD, window(), Style::Row);
    assert_eq!(many.cards[0].width(), 96.0);
}

#[test]
fn a_fan_starts_on_the_stacks_place_and_slides_left_at_the_edge() {
    let stack = stack_at(732.0);
    let placed = place(stack, 3, CARD, window(), Style::Fan);
    assert_eq!(placed.cards[0], stack, "window 1 is where the stack was");
    assert_eq!(lefts(&placed), [732.0, 796.0, 860.0], "a 64 px step");
    let last = place(stack_at(1472.0), 3, CARD, window(), Style::Fan);
    assert_eq!(last.panel.right(), window().right() - MARGIN);
    assert_eq!(lefts(&last), [1350.0, 1414.0, 1478.0]);
    // Six windows still lie near the stack.
    let six = place(stack, 6, CARD, window(), Style::Fan);
    assert_eq!(six.cards[5].right(), 732.0 + 5.0 * 64.0 + 162.0);
}

#[test]
fn a_fans_front_card_is_drawn_last_and_nearer_cards_higher() {
    let order = draw_order(5, 2);
    assert_eq!(order.last(), Some(&2));
    let at = |card: usize| order.iter().position(|c| *c == card).unwrap();
    assert!(at(1) > at(0) && at(3) > at(4), "nearer ones higher");
    assert_eq!(draw_order(3, 0), [2, 1, 0]);

    let cards = place(stack_at(732.0), 3, CARD, window(), Style::Fan).cards;
    let parts = shown_parts(&cards, 0);
    assert_eq!(parts[0], cards[0], "the front card shows whole");
    assert_eq!(parts[1].left(), cards[0].right());
    assert_eq!(parts[2].left(), cards[1].right());
    let parts = shown_parts(&cards, 2);
    assert_eq!(parts[0].right(), cards[1].left());
    assert_eq!(parts[1].right(), cards[2].left());
    assert!(
        parts
            .windows(2)
            .all(|pair| pair[0].right() <= pair[1].left()),
        "the shown parts never overlap"
    );
}

#[test]
fn left_and_right_move_among_the_windows_without_wrapping() {
    assert_eq!(step(0, 3, true, false), 0);
    assert_eq!(step(0, 3, false, true), 1);
    assert_eq!(step(2, 3, false, true), 2);
    assert_eq!(step(5, 3, false, false), 2, "a window left the group");
}

#[test]
fn a_row_picture_keeps_only_the_part_the_layer_leaves_free() {
    let picture = Rect::from_min_max(pos2(100.0, 50.0), pos2(250.0, 130.0));
    let layer = Rect::from_min_max(pos2(200.0, 10.0), pos2(600.0, 200.0));
    assert_eq!(
        left_over(picture, layer),
        Rect::from_min_max(pos2(100.0, 50.0), pos2(200.0, 130.0)),
        "covered on its right: the left part shows"
    );
    let from_left = Rect::from_min_max(pos2(0.0, 10.0), pos2(150.0, 200.0));
    assert_eq!(left_over(picture, from_left).min.x, 150.0);
    let far = Rect::from_min_max(pos2(700.0, 0.0), pos2(800.0, 100.0));
    assert_eq!(left_over(picture, far), picture, "not touched");
    let corner = Rect::from_min_max(pos2(200.0, 100.0), pos2(600.0, 300.0));
    assert!(
        !left_over(picture, corner).is_positive(),
        "an L shape hides"
    );
    let whole = picture.expand(5.0);
    assert!(!left_over(picture, whole).is_positive(), "covered whole");
}
