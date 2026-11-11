use super::*;

fn screen(connected: bool) -> ArrangeMonitor {
    ArrangeMonitor {
        device: String::new(),
        label: String::new(),
        name: String::new(),
        rect: [0, 0, 100, 100],
        work: [0, 0, 100, 90],
        primary: false,
        connected,
    }
}

/// One row of three monitors: left (0), centre (1), right (2).
fn rows() -> Vec<Vec<usize>> {
    vec![vec![0, 1, 2]]
}

fn band(cells: Vec<Vec<usize>>) -> Band {
    Band {
        desktop: String::new(),
        title: String::new(),
        number: None,
        current: true,
        cells: vec![cells],
        ghost_cell: None,
        ghosts: Vec::new(),
        lead: 0.0,
    }
}

#[test]
fn members_on_other_monitors_become_ghosts_where_most_of_the_group_is() {
    let monitors = vec![screen(true); 3];
    // The group is 1, 2, 4 and 5: two in the centre, one left, one right.
    let mut bands = vec![band(vec![vec![0, 1], vec![2, 3, 4], vec![5]])];
    let ghosted = add_group_ghosts(&mut bands, &rows(), &monitors, &[1, 2, 4, 5], None);
    assert_eq!(bands[0].ghost_cell, Some((0, 1)));
    assert_eq!(bands[0].ghosts, [1, 5]);
    assert_eq!(ghosted, [1, 5]);
    // Their own cards stay where they are.
    assert_eq!(bands[0].cells[0][0], [0, 1]);
    assert_eq!(
        bands[0].flow((0, 1)),
        [
            Item::Ghost(1),
            Item::Card(2),
            Item::Card(3),
            Item::Card(4),
            Item::Ghost(5)
        ]
    );
}

#[test]
fn a_tie_goes_to_the_column_of_the_window_used_last() {
    let monitors = vec![screen(true); 3];
    let mut bands = vec![band(vec![vec![0], vec![1], vec![]])];
    add_group_ghosts(&mut bands, &rows(), &monitors, &[0, 1], Some(1));
    assert_eq!(bands[0].ghost_cell, Some((0, 1)));
    assert_eq!(bands[0].ghosts, [0]);
    let mut bands = vec![band(vec![vec![0], vec![1], vec![]])];
    add_group_ghosts(&mut bands, &rows(), &monitors, &[0, 1], None);
    assert_eq!(bands[0].ghost_cell, Some((0, 0)), "else the first");
}

#[test]
fn the_home_column_keeps_its_own_ghosts_and_others_give_way() {
    let monitors = vec![screen(true); 3];
    let mut bands = vec![band(vec![vec![0], vec![1, 2], vec![3]])];
    bands[0].ghost_cell = Some((0, 1));
    bands[0].ghosts = vec![0, 3];
    add_group_ghosts(&mut bands, &rows(), &monitors, &[1, 2, 3], None);
    assert_eq!(bands[0].ghosts, [0, 3], "no copy of 3");

    let mut bands = vec![band(vec![vec![0, 4], vec![1], vec![3]])];
    bands[0].ghost_cell = Some((0, 1));
    bands[0].ghosts = vec![0, 3, 4];
    add_group_ghosts(&mut bands, &rows(), &monitors, &[0, 4, 3], None);
    assert_eq!(bands[0].ghost_cell, Some((0, 0)));
    assert_eq!(bands[0].ghosts, [3]);
}

#[test]
fn nothing_changes_for_a_group_on_one_monitor_or_a_disconnected_one() {
    let mut monitors = vec![screen(true); 3];
    let mut bands = vec![band(vec![vec![0], vec![1, 2], vec![]])];
    assert!(add_group_ghosts(&mut bands, &rows(), &monitors, &[1, 2], None).is_empty());
    assert_eq!(bands[0].ghost_cell, None);
    monitors[0].connected = false;
    let mut bands = vec![band(vec![vec![0], vec![1, 2], vec![]])];
    assert!(add_group_ghosts(&mut bands, &rows(), &monitors, &[0, 1, 2], None).is_empty());
}

#[test]
fn an_open_group_leaves_room_above_each_line_of_cards() {
    assert_eq!(
        led_height(EMPTY, GROUP_LEAD),
        EMPTY,
        "a drop box has no line"
    );
    assert_eq!(
        led_height(CARD_HEIGHT, GROUP_LEAD),
        CARD_HEIGHT + GROUP_LEAD
    );
    let two = 2.0 * CARD_HEIGHT + CARD_GAP;
    assert_eq!(led_height(two, GROUP_LEAD), two + 2.0 * GROUP_LEAD);
    assert_eq!(led_height(two, 0.0), two);

    let top = 100.0;
    let card = |line: f32| {
        Rect::from_min_size(
            pos2(0.0, top + line * (CARD_HEIGHT + CARD_GAP)),
            vec2(150.0, CARD_HEIGHT),
        )
    };
    let placed = vec![
        (Some(Item::Card(0)), card(0.0)),
        (Some(Item::Card(1)), card(1.0)),
    ];
    let led = led_places(placed.clone(), top, GROUP_LEAD);
    assert_eq!(led[0].1.top(), top + GROUP_LEAD);
    assert_eq!(
        led[1].1.top(),
        top + CARD_HEIGHT + CARD_GAP + 2.0 * GROUP_LEAD
    );
    assert_eq!(led_places(placed.clone(), top, 0.0), placed);
}

#[test]
fn the_group_header_line_stays_clear_of_the_monitor_line() {
    use crate::ui::arrange::stacks::{frame_rect, tab_at, TAB_HEIGHT};
    // The monitor line ends LABEL_GAP above the cell's top; the first
    // line of cards starts GROUP_LEAD below it.
    let top = 100.0;
    let label_bottom = top - LABEL_GAP;
    let run = Rect::from_min_size(pos2(10.0, top + GROUP_LEAD), vec2(300.0, CARD_HEIGHT));
    let frame = frame_rect(run, GROUP_LEAD);
    assert!(frame.top() > label_bottom);
    let tab = tab_at(frame, GROUP_LEAD);
    assert!(tab.y >= frame.top());
    assert!(tab.y + TAB_HEIGHT < run.top(), "and clear of the cards");
    // A second line: the frame stays below the line above.
    let above_bottom = top + GROUP_LEAD + CARD_HEIGHT;
    let second = run.translate(vec2(0.0, CARD_HEIGHT + CARD_GAP + GROUP_LEAD));
    assert!(frame_rect(second, GROUP_LEAD).top() > above_bottom);
}

#[test]
fn opening_a_group_gives_every_band_the_room() {
    let monitors = vec![screen(true); 3];
    let group = ArrangeGroup {
        key: "app".into(),
        label: "App".into(),
        windows: Vec::new(),
    };
    let model = Model {
        rows: rows(),
        cols: Vec::new(),
        bands: vec![band(vec![vec![], vec![], vec![]])],
        all: vec![band(vec![vec![], vec![], vec![]])],
        widths: Vec::new(),
    };
    let open = OpenMembers {
        hwnds: vec![1],
        rep: None,
    };
    let (model, _) = model.with_open_group(&group, &monitors, &open);
    assert_eq!(model.bands[0].lead, GROUP_LEAD);
    assert_eq!(model.all[0].lead, GROUP_LEAD);
}
