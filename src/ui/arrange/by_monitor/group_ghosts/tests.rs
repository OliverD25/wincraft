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
