use super::*;

fn cards(lefts: &[f32], width: f32) -> Vec<Rect> {
    lefts
        .iter()
        .map(|left| Rect::from_min_size(pos2(*left, 0.0), Vec2::new(width, 50.0)))
        .collect()
}

#[test]
fn drop_slot_counts_the_cards_left_of_the_pointer() {
    let row = cards(&[0.0, 110.0, 220.0], 100.0);
    assert_eq!(drop_slot(&row, -5.0), 0);
    assert_eq!(drop_slot(&row, 60.0), 1);
    assert_eq!(drop_slot(&row, 200.0), 2);
    assert_eq!(drop_slot(&row, 400.0), 3);
}

#[test]
fn moved_lands_the_window_in_the_gap() {
    assert_eq!(moved(3, 0, 3), Some(vec![1, 2, 0]));
    assert_eq!(moved(3, 2, 0), Some(vec![2, 0, 1]));
    assert_eq!(moved(4, 1, 3), Some(vec![0, 2, 1, 3]));
}

#[test]
fn moved_is_none_when_the_window_stays() {
    assert_eq!(moved(3, 1, 1), None);
    assert_eq!(moved(3, 1, 2), None);
    assert_eq!(moved(3, 5, 0), None);
    assert_eq!(moved(3, 0, 9), None);
}

#[test]
fn nudge_moves_one_place_and_stops_at_the_ends() {
    assert_eq!(nudge_slot(3, 0, true), None);
    assert_eq!(nudge_slot(3, 2, false), None);
    assert_eq!(
        moved(3, 1, nudge_slot(3, 1, true).unwrap()),
        Some(vec![1, 0, 2])
    );
    assert_eq!(
        moved(3, 1, nudge_slot(3, 1, false).unwrap()),
        Some(vec![0, 2, 1])
    );
}

#[test]
fn row_order_keeps_other_windows_in_place() {
    // Group windows 2, 4, 5 among the app's windows 1 to 6.
    let row = [1, 2, 3, 4, 5, 6];
    assert_eq!(row_order(&row, &[5, 2, 4]), vec![1, 5, 3, 2, 4, 6]);
    assert_eq!(row_order(&row, &[2, 4, 5]), row.to_vec());
}

#[test]
fn gap_x_sits_between_cards_or_on_a_fan_edge() {
    let row = cards(&[0.0, 110.0], 100.0);
    assert_eq!(gap_x(&row, 0, 10.0), -5.0);
    assert_eq!(gap_x(&row, 1, 10.0), 105.0);
    assert_eq!(gap_x(&row, 2, 10.0), 215.0);
    let fan = cards(&[0.0, 40.0], 100.0);
    assert_eq!(gap_x(&fan, 1, 10.0), 40.0);
}
