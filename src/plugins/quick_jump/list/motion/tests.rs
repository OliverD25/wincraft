use super::*;
use egui::{pos2, vec2};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

const KEYS: [isize; 4] = [11, 22, 33, 44];

#[test]
fn the_gap_follows_the_pointer_to_the_nearest_place() {
    let origin = pos2(20.0, 64.0);
    for place in 0..6 {
        let centre = card_rect(origin, place).center() + vec2(30.0, -20.0);
        assert_eq!(gap_at(origin, centre, 6), place);
    }
    // Below the last card the gap stays at the last place.
    let far = card_rect(origin, 5).center() + vec2(0.0, 400.0);
    assert_eq!(gap_at(origin, far, 6), 5);
    assert_eq!(gap_at(origin, pos2(-500.0, -500.0), 6), 0);
}

#[test]
fn while_dragging_the_others_close_up_and_open_the_gap() {
    let mut motion = Motion::default();
    motion.start(0, vec2(10.0, 10.0));
    assert_eq!(motion.order(4), vec![0, 1, 2, 3], "no gap moved yet");
    motion.drag_to(2);
    assert_eq!(motion.order(4), vec![1, 2, 0, 3]);
    motion.drag_to(3);
    assert_eq!(motion.order(4), vec![1, 2, 3, 0]);
}

#[test]
fn a_drop_sends_the_move_and_the_cards_keep_the_new_order() {
    let t0 = Instant::now();
    let mut motion = Motion::default();
    motion.start(1, Vec2::ZERO);
    motion.drag_to(3);
    assert_eq!(
        motion.release(&KEYS, t0),
        Some(Request::Move { from: 1, to: 3 })
    );
    assert!(!motion.dragging());
    assert_eq!(motion.order(4), vec![0, 2, 3, 1], "until the view shows it");
    // The view shows it: the keys changed, and its own order is right.
    motion.begin(&[11, 33, 44, 22], t0 + ms(30));
    assert_eq!(motion.order(4), vec![0, 1, 2, 3]);
}

#[test]
fn a_drop_on_its_own_place_and_esc_send_nothing() {
    let t0 = Instant::now();
    let mut motion = Motion::default();
    motion.start(2, Vec2::ZERO);
    assert_eq!(motion.release(&KEYS, t0), None);
    motion.start(2, Vec2::ZERO);
    motion.drag_to(0);
    motion.cancel();
    assert!(!motion.dragging());
    assert_eq!(motion.release(&KEYS, t0), None);
    assert_eq!(motion.order(4), vec![0, 1, 2, 3], "everything goes back");
}

#[test]
fn a_change_the_view_never_shows_is_given_up() {
    let t0 = Instant::now();
    let mut motion = Motion::default();
    motion.moved(&KEYS, 0, 1, t0);
    motion.begin(&KEYS, t0 + ms(100));
    assert_eq!(motion.order(4), vec![1, 0, 2, 3]);
    motion.begin(&KEYS, t0 + PENDING_FOR);
    assert_eq!(motion.order(4), vec![0, 1, 2, 3]);
}

#[test]
fn an_unpinned_card_fades_out_while_the_rest_close_the_gap() {
    let t0 = Instant::now();
    let mut motion = Motion::default();
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 250.0));
    motion.removed(&KEYS, 1, Some((SlotView::default(), rect)), t0);
    assert_eq!(motion.order(4), vec![0, 2, 3]);
    let (start, shown) = motion.leaving.as_ref().unwrap().look(t0);
    assert_eq!((start, shown), (rect, 1.0));
    let (half, fading) = motion.leaving.as_ref().unwrap().look(t0 + ms(90));
    assert!(half.width() < 300.0 && fading < 1.0);
    assert!(motion.moving());
    motion.begin(&KEYS, t0 + CLOSE_TIME);
    assert!(motion.leaving.is_none());
}

#[test]
fn moves_and_unpins_in_a_row_build_on_each_other() {
    let t0 = Instant::now();
    let mut motion = Motion::default();
    motion.moved(&KEYS, 0, 3, t0);
    motion.removed(&KEYS, 0, None, t0);
    assert_eq!(motion.order(4), vec![2, 3, 0]);
}

#[test]
fn a_cards_key_stays_with_its_pin_when_the_pins_trade_places() {
    use super::super::super::pins::tests::list;
    let pins: Vec<_> = list(&["Claude", "Code", "Claude"])
        .iter()
        .map(|(_, pin)| pin.clone())
        .collect();
    let slots = vec![SlotView::default(); 3];
    let view = View {
        slots: slots.clone(),
        pins: pins.clone(),
        ..View::default()
    };
    let keys = card_keys(&view);
    assert_eq!(keys.len(), 3);
    assert_ne!(keys[0], keys[2], "two pins that are alike get two keys");
    let swapped = View {
        slots,
        pins: vec![pins[1].clone(), pins[0].clone(), pins[2].clone()],
        ..View::default()
    };
    let moved = card_keys(&swapped);
    assert_eq!(moved[0], keys[1]);
    assert_eq!(moved[1], keys[0]);
}
