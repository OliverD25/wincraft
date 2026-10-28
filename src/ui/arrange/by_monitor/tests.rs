use super::*;

/// A card of a 16:9 window.
fn card_size() -> Vec2 {
    vec2(card_width(None), CARD_HEIGHT)
}

const SAME: [f32; 10] = [179.0; 10];

fn monitor(rect: [i32; 4], taskbar: i32, primary: bool) -> ArrangeMonitor {
    ArrangeMonitor {
        device: format!(r"\\.\DISPLAY{}", rect[0]),
        label: String::new(),
        name: String::new(),
        rect,
        work: [rect[0], rect[1], rect[2], rect[3] - taskbar],
        primary,
        connected: true,
    }
}

fn disconnected() -> ArrangeMonitor {
    ArrangeMonitor {
        connected: false,
        ..monitor([0, 0, 0, 0], 0, false)
    }
}

/// The monitors of the PC this was built on, in the order Windows lists
/// them: the primary 3840 x 2160 one (150 %), the 1920 x 1080 one to its
/// left (100 %) and the portrait-like 2560 x 2880 one to its right
/// (175 %).
fn desk() -> Vec<ArrangeMonitor> {
    vec![
        monitor([0, 0, 3840, 2160], 72, true),
        monitor([-1920, 0, 0, 1080], 48, false),
        monitor([3840, 0, 6400, 2880], 84, false),
    ]
}

fn every(_: usize) -> bool {
    true
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn columns_follow_the_monitors_from_left_to_right() {
    assert_eq!(monitor_rows(&desk(), &every), [vec![1, 0, 2]]);
}

#[test]
fn stacked_monitors_get_a_row_each_and_ties_go_by_top() {
    let monitors = vec![
        monitor([0, 0, 1920, 1080], 48, true),
        monitor([1920, 0, 4480, 1440], 48, false),
        monitor([0, -1080, 1920, 0], 0, false),
    ];
    // The one above the primary comes first at the same left edge; the
    // primary under it shares its width, so it starts a second row.
    assert_eq!(monitor_rows(&monitors, &every), [vec![2, 1], vec![0]]);
}

#[test]
fn a_disconnected_monitor_follows_on_the_first_row_when_shown() {
    let mut monitors = desk();
    monitors.push(disconnected());
    assert_eq!(monitor_rows(&monitors, &every), [vec![1, 0, 2, 3]]);
    assert_eq!(monitor_rows(&monitors, &|i| i != 3), [vec![1, 0, 2]]);
}

fn col(need: f32, count: usize) -> Col {
    Col {
        need,
        count,
        primary: false,
        rail: false,
        screen: 1920.0,
    }
}

fn rail_col() -> Col {
    Col {
        rail: true,
        ..col(INSET + card_width(None), 0)
    }
}

#[test]
fn columns_that_all_fit_get_their_content_and_the_last_one_the_spare() {
    // 300 + 700 + 260 fit in 1400: the spare 140 goes to the last column.
    let widths = tracks(&[col(300.0, 1), col(700.0, 5), col(260.0, 2)], &[], 1400.0);
    assert_eq!(widths, [300.0, 700.0, 400.0]);
    assert!(close(widths.iter().sum(), 1400.0));
    // An empty last column does not take it; the last one with windows does.
    let widths = tracks(&[col(300.0, 1), col(200.0, 2), col(200.0, 0)], &[], 900.0);
    assert_eq!(widths, [300.0, 400.0, 200.0]);
    // A column's content may be under MIN_TRACK when everything fits.
    assert_eq!(
        tracks(&[col(150.0, 1), col(300.0, 1)], &[], 450.0),
        [150.0, 300.0]
    );
}

#[test]
fn a_column_with_ghosts_gets_its_whole_line_beside_a_busier_column() {
    // Left empty, centre one card and two ghosts, right two cards: the
    // centre gets exactly its content and the right one the rest.
    let card = card_width(None);
    let centre = INSET + card + 2.0 * (CARD_GAP + GHOST_WIDTH);
    let right = INSET + 2.0 * card + CARD_GAP;
    let cols = [
        Col {
            rail: true,
            ..col(INSET + card, 0)
        },
        col(centre, 1),
        col(right, 2),
    ];
    let available = 1100.0;
    let widths = tracks(&cols, &[], available);
    assert_eq!(widths[0], RAIL);
    assert!(close(widths[1], centre));
    assert!(close(widths[2], available - RAIL - centre));
    assert!(widths[2] >= right);
    assert_eq!(
        cell_lines(&[card, GHOST_WIDTH, GHOST_WIDTH], widths[1]).len(),
        1
    );
}

#[test]
fn when_the_columns_do_not_fit_the_widest_content_takes_the_rest() {
    // Equal monitors share 1000 as 333.3 each. The first column has two
    // cards (fewer windows, wider content) and wins over the second.
    let widths = tracks(&[col(500.0, 2), col(400.0, 3), col(250.0, 1)], &[], 1000.0);
    assert!(close(widths[1], 1000.0 / 3.0));
    assert!(close(widths[2], 250.0));
    assert!(close(widths[0], 1000.0 - widths[1] - widths[2]));
    // Monitors 1920, 3840 and 2560 wide share 1664 as 384, 768 and 512.
    let mut cols = [col(700.0, 1), col(900.0, 2), col(900.0, 3)];
    for (col, screen) in cols.iter_mut().zip([1920.0, 3840.0, 2560.0]) {
        col.screen = screen;
    }
    let widths = tracks(&cols, &[], 1664.0);
    assert!(close(widths[0], 384.0));
    assert!(close(widths[1], 768.0));
    assert!(close(widths[2], 512.0));
    // A share under 200 still leaves the column 200.
    let mut narrow = [col(400.0, 1), col(800.0, 3)];
    narrow[0].screen = 100.0;
    assert_eq!(tracks(&narrow, &[], 1000.0)[0], MIN_TRACK);
    // Too narrow for both: each keeps 200 and the row scrolls.
    let widths = tracks(&[col(220.0, 1), col(300.0, 3)], &[], 380.0);
    assert_eq!(widths, [MIN_TRACK; 2]);
    assert!(widths.iter().sum::<f32>() > 380.0);
    assert!(tracks(&[], &[], 600.0).is_empty());
}

#[test]
fn the_widest_content_wins_not_the_most_windows() {
    // One card and two ghosts (600) beat two cards (450).
    let widths = tracks(&[col(450.0, 2), col(600.0, 1)], &[], 1000.0);
    assert_eq!(widths, [450.0, 550.0]);
}

#[test]
fn a_tie_for_the_widest_content_goes_to_the_primary_monitor() {
    // 3 x 400 do not fit in 1000.
    let mut cols = [col(400.0, 2), col(400.0, 2), col(400.0, 1)];
    // No primary among the tied: the leftmost takes the rest.
    assert_eq!(winner(&cols), Some(0));
    cols[1].primary = true;
    assert_eq!(winner(&cols), Some(1));
    // A primary monitor with narrower content does not win.
    cols[2].need = 450.0;
    assert_eq!(winner(&cols), Some(2));
}

#[test]
fn a_monitor_without_windows_is_a_rail_beside_columns_with_some() {
    let monitors = desk();
    let rows = monitor_rows(&monitors, &every);
    let widths = [179.0; 10];
    let one = |members: &[(&str, Option<usize>, Option<usize>)]| {
        let bands = bands(&group(members), &desktops(), &monitors, &rows);
        columns(&bands, &rows, &monitors, &widths)
    };
    let rails = |cols: &[Vec<Col>]| cols[0].iter().map(|col| col.rail).collect::<Vec<_>>();
    // Windows on the primary monitor only: the left and right ones fold.
    let cols = one(&[("d1", Some(0), None), ("d1", Some(0), None)]);
    assert_eq!(rails(&cols), [true, false, true]);
    assert_eq!(cols[0][1].count, 2);
    assert!(cols[0][1].primary);
    // A window on another desktop keeps its monitor's column.
    let cols = one(&[("d1", Some(0), None), ("d0", Some(1), None)]);
    assert_eq!(rails(&cols), [false, false, true]);
}

#[test]
fn rails_are_40_wide_and_the_busiest_column_gives_way_when_they_open() {
    let cols = [rail_col(), col(500.0, 4), rail_col()];
    assert_eq!(tracks(&cols, &[0.0, 0.0, 0.0], 1000.0), [40.0, 920.0, 40.0]);
    assert_eq!(tracks(&cols, &[], 1000.0), [40.0, 920.0, 40.0]);
    let open = tracks(&cols, &[1.0, 0.0, 1.0], 1000.0);
    assert_eq!(open, [RAIL_OPEN, 600.0, RAIL_OPEN]);
    let half = tracks(&cols, &[0.5, 0.0, 0.0], 1000.0);
    assert_eq!(half, [120.0, 840.0, 40.0]);
    assert_eq!(rail_width(0.0), 40.0);
    assert_eq!(rail_width(1.0), 200.0);
}

#[test]
fn rails_open_while_a_card_is_dragged_and_while_the_keyboard_is_on_them() {
    assert!(!rail_wanted(false, None, 2));
    assert!(rail_wanted(true, None, 2));
    assert!(rail_wanted(true, Some(0), 2));
    // The keyboard unfolds only the rail it is on.
    assert!(rail_wanted(false, Some(2), 2));
    assert!(!rail_wanted(false, Some(0), 2));
}

#[test]
fn a_strip_of_one_column_or_of_empty_columns_has_no_rails() {
    let monitors = desk();
    let widths = [179.0; 10];
    let empty = |row: Vec<usize>| {
        let band = Band {
            desktop: String::new(),
            title: String::new(),
            number: None,
            current: true,
            cells: vec![vec![Vec::new(); row.len()]],
            ghost_cell: None,
            ghosts: Vec::new(),
        };
        columns(&[band], &[row], &monitors, &widths)
    };
    // A hover strip limited to one taskbar: the one column takes it all.
    let one = empty(vec![2]);
    assert!(!one[0][0].rail);
    assert_eq!(row_tracks(&one[0], &[], 1000.0), [1000.0 - 2.0 * SIDE]);
    // Nothing anywhere: ordinary columns with their drop boxes.
    assert!(empty(vec![1, 0, 2])[0].iter().all(|col| !col.rail));
}

#[test]
fn a_disconnected_monitor_is_never_a_rail() {
    let mut monitors = desk();
    monitors.push(disconnected());
    let rows = monitor_rows(&monitors, &every);
    let band = Band {
        desktop: String::new(),
        title: String::new(),
        number: None,
        current: true,
        cells: vec![vec![vec![0], vec![], vec![], vec![]]],
        ghost_cell: None,
        ghosts: Vec::new(),
    };
    let cols = columns(&[band], &rows, &monitors, &[179.0]);
    let rails: Vec<bool> = cols[0].iter().map(|col| col.rail).collect();
    assert_eq!(rails, [false, true, true, false]);
}

#[test]
fn a_rail_row_is_at_least_150_high() {
    let band = Band {
        desktop: String::new(),
        title: String::new(),
        number: None,
        current: true,
        cells: vec![vec![Vec::new(), Vec::new()]],
        ghost_cell: None,
        ghosts: Vec::new(),
    };
    let tracks = [40.0, 300.0];
    // Windows are only on another desktop: this band's row would be
    // 18 + 8 + 112 high.
    const { assert!(LABEL + LABEL_GAP + EMPTY < RAIL_MIN_HEIGHT) };
    assert_eq!(
        row_height(&band, 0, &[rail_col(), col(200.0, 1)], &tracks, &SAME),
        RAIL_MIN_HEIGHT
    );
    assert_eq!(
        row_height(&band, 0, &[col(200.0, 0), col(200.0, 1)], &tracks, &SAME),
        LABEL + LABEL_GAP + EMPTY
    );
}

fn one_busy_monitor() -> Vec<Band> {
    vec![Band {
        desktop: "d1".to_string(),
        title: "This desktop".to_string(),
        number: Some(1),
        current: true,
        cells: vec![vec![vec![], vec![0, 1], vec![]]],
        ghost_cell: None,
        ghosts: Vec::new(),
    }]
}

#[test]
fn the_keys_can_land_on_a_rail_and_it_holds_the_focus() {
    let monitors = desk();
    let rows = monitor_rows(&monitors, &every);
    let bands = one_busy_monitor();
    let cols = columns(&bands, &rows, &monitors, &[179.0; 2]);
    let Some(Step::Move(spot)) = step(&bands, &rows, &monitors, 0, Key::MonitorRight) else {
        panic!("expected a move");
    };
    assert_eq!(spot.cell, (0, 2));
    // The rail on the right belongs to monitor 2.
    assert_eq!(rail_at(&cols, &rows, spot.cell), Some(2));
    // A column with windows is not a rail.
    assert_eq!(rail_at(&cols, &rows, (0, 1)), None);
    assert_eq!(rail_at(&cols, &rows, (3, 0)), None);
}

#[test]
fn a_drop_on_a_rail_moves_the_window_to_its_monitor() {
    let monitors = desk();
    let rows = monitor_rows(&monitors, &every);
    let bands = one_busy_monitor();
    let group = group(&[("d1", Some(0), None), ("d1", Some(0), None)]);
    // The rail of the right monitor, as the zone it leaves behind.
    let zones = vec![Zone {
        band: 0,
        cell: Some((0, 2)),
        rect: Rect::from_min_size(pos2(900.0, 0.0), vec2(200.0, 300.0)),
        open: true,
        cards: Vec::new(),
    }];
    let landing = drop_spot(pos2(1000.0, 150.0), &zones, 1, (0, 1)).expect("a spot");
    assert_eq!(landing, spot(0, (0, 2), 0));
    let actions = drop_actions(&group, &desktops(), &bands, &rows, &monitors, 1, landing);
    assert_eq!(
        actions[0],
        ArrangeAction::MoveToMonitor {
            hwnd: 101,
            monitor: monitors[2].device.clone()
        }
    );
}

fn desktops() -> Vec<ArrangeDesktop> {
    ["Home", "Work", "Reading"]
        .iter()
        .enumerate()
        .map(|(i, name)| ArrangeDesktop {
            id: format!("d{i}"),
            name: name.to_string(),
            current: i == 1,
        })
        .collect()
}

/// Windows as (desktop, monitor now, monitor at the last save).
fn group(windows: &[(&str, Option<usize>, Option<usize>)]) -> ArrangeGroup {
    ArrangeGroup {
        key: "chrome.exe".to_string(),
        label: "Chrome".to_string(),
        windows: windows
            .iter()
            .enumerate()
            .map(|(i, (desktop, on, saved))| ArrangeWindow {
                hwnd: 100 + i as isize,
                label: format!("w{i}"),
                desktop: desktop.to_string(),
                monitor: None,
                on_monitor: *on,
                saved_monitor: *saved,
                renamed: false,
                same_title: false,
                attention: false,
            })
            .collect(),
    }
}

#[test]
fn bands_put_this_desktop_first_then_task_view_order() {
    let monitors = desk();
    let rows = monitor_rows(&monitors, &every);
    let group = group(&[
        ("d2", Some(0), Some(0)),
        ("d1", Some(1), Some(1)),
        ("d1", Some(0), Some(0)),
        ("", None, None),
        ("d1", Some(2), Some(2)),
    ]);
    let bands = bands(&group, &desktops(), &monitors, &rows);
    let titles: Vec<&str> = bands.iter().map(|b| b.title.as_str()).collect();
    assert_eq!(titles, ["This desktop", "Home", "Reading"]);
    assert_eq!(bands[0].number, Some(2));
    // Columns are Left, Centre (primary), Right; the window with no
    // known desktop or monitor is on this desktop's primary monitor.
    assert_eq!(bands[0].cells, [vec![vec![1], vec![2, 3], vec![4]]]);
    assert_eq!(bands[2].cells, [vec![vec![], vec![0], vec![]]]);
    assert_eq!(bands[0].meta(), "Desktop 2 \u{b7} 4 windows");
    assert_eq!(bands[2].meta(), "Desktop 3 \u{b7} 1 window");
}

#[test]
fn a_desktop_without_windows_is_its_header_alone() {
    let monitors = desk();
    let rows = monitor_rows(&monitors, &every);
    let bands = bands(
        &group(&[("d1", Some(0), None)]),
        &desktops(),
        &monitors,
        &rows,
    );
    assert_eq!(bands[1].count(), 0);
    let cols = columns(&bands[..1], &rows, &monitors, &SAME);
    let tracks = vec![row_tracks(&cols[0], &[], 1200.0)];
    assert_eq!(
        band_height(&bands[1], &cols, &tracks, &SAME),
        1.0 + BAND_TOP + BAND_HEADER + BAND_TOP
    );
    // One card in the primary column; the empty monitors are rails as
    // high as the row.
    assert_eq!(
        band_height(&bands[0], &cols, &tracks, &SAME),
        1.0 + BAND_TOP + BAND_HEADER + HEADER_GAP + LABEL + LABEL_GAP + CARD_HEIGHT + BAND_BOTTOM
    );
}

#[test]
fn without_desktops_every_window_is_on_this_desktop() {
    let monitors = desk();
    let rows = monitor_rows(&monitors, &every);
    let bands = bands(&group(&[("d7", Some(1), None)]), &[], &monitors, &rows);
    assert_eq!(bands.len(), 1);
    assert_eq!(bands[0].number, None);
    assert_eq!(bands[0].meta(), "1 window");
    assert_eq!(bands[0].cells, [vec![vec![0], vec![], vec![]]]);
}

#[test]
fn a_window_saved_on_a_disconnected_monitor_stays_in_its_column() {
    let mut monitors = desk();
    monitors.push(disconnected());
    let windows = group(&[
        ("d1", Some(0), Some(3)),
        ("d1", Some(0), Some(1)),
        ("d1", Some(2), None),
    ]);
    assert_eq!(column_of(&windows.windows[0], &monitors), Some(3));
    // Moved by hand since the save: its column is where it is now.
    assert_eq!(column_of(&windows.windows[1], &monitors), Some(0));
    assert_eq!(column_of(&windows.windows[2], &monitors), Some(2));
}

#[test]
fn the_panel_is_as_wide_as_one_line_of_cards_per_column_needs() {
    let monitors = desk();
    let rows = monitor_rows(&monitors, &every);
    let one_each = bands(
        &group(&[
            ("d1", Some(1), None),
            ("d1", Some(0), None),
            ("d1", Some(0), None),
            ("d1", Some(2), None),
        ]),
        &desktops(),
        &monitors,
        &rows,
    );
    let cols = columns(&one_each, &rows, &monitors, &SAME);
    assert_eq!(card_size().x, SAME[0]);
    // Each column needs its cards on one line: one card (192) on the
    // left and right, two cards in the primary one.
    let needed = (INSET + SAME[0]) + (INSET + 2.0 * SAME[0] + CARD_GAP) + (INSET + SAME[0]);
    let width = natural_width(&cols);
    assert!(close(width, needed + 2.0 * COLUMN_GAP + 2.0 * SIDE));
    let size = panel_size(&one_each, &cols, &SAME, false, true, vec2(5000.0, 5000.0));
    assert!(close(size.x, width + SLACK));
    let tracks = row_tracks(&cols[0], &[], size.x);
    assert!(close(tracks[0], INSET + SAME[0]));
    assert!(close(tracks[1], INSET + 2.0 * SAME[0] + CARD_GAP));
    // The spare width (SLACK) goes to the last column.
    assert!(close(tracks[2], INSET + SAME[0] + SLACK));
    // Two cards fit side by side in the middle column, not three.
    assert_eq!(cell_lines(&SAME[..3], tracks[1]), [0..2, 2..3]);

    // Never wider than the limit, nor taller.
    let small = panel_size(&one_each, &cols, &SAME, false, true, vec2(800.0, 300.0));
    assert_eq!(small, vec2(800.0, 300.0));
}

#[test]
fn rails_leave_the_panel_at_its_minimum_width() {
    let monitors = desk();
    let rows = monitor_rows(&monitors, &every);
    let bands = bands(
        &group(&[("d1", Some(0), None)]),
        &desktops(),
        &monitors,
        &rows,
    );
    let cols = columns(&bands, &rows, &monitors, &SAME);
    // Two 40 px rails and one column of one card: well under MIN_PANEL.
    assert!(close(natural_width(&cols), MIN_PANEL));
    let tracks = row_tracks(&cols[0], &[], MIN_PANEL + SLACK);
    assert_eq!(tracks[0], RAIL);
    assert_eq!(tracks[2], RAIL);
    assert!(tracks[1] > MIN_TRACK);
}

#[test]
fn the_panel_has_room_for_the_line_that_open_rails_wrap_onto() {
    // Reported 2026-10-28: nine cards fitted on one line beside two folded
    // rails; dragging opened the rails, the cards wrapped onto a second line
    // and the panel cut it off.
    let monitors = desk();
    let rows = monitor_rows(&monitors, &every);
    let windows: Vec<(&str, Option<usize>, Option<usize>)> =
        (0..9).map(|_| ("d1", Some(0), None)).collect();
    let bands = bands(&group(&windows), &desktops(), &monitors, &rows);
    let cols = columns(&bands, &rows, &monitors, &SAME);
    let limit = vec2(5000.0, 5000.0);
    let size = panel_size(&bands, &cols, &SAME, false, true, limit);
    let folded = row_tracks(&cols[0], &[], size.x);
    let open = row_tracks(&cols[0], &[1.0; 3], size.x);
    let middle = cols[0].iter().position(|col| !col.rail).unwrap();
    assert_eq!(cell_lines(&SAME[..9], folded[middle]).len(), 1);
    assert_eq!(cell_lines(&SAME[..9], open[middle]).len(), 2);
    let tracks = vec![open];
    let needed: f32 = bands
        .iter()
        .map(|band| band_height(band, &cols, &tracks, &SAME))
        .sum();
    assert!(size.y >= TOOLBAR + needed + footer_height(true));
}

#[test]
fn mixed_cards_wrap_by_their_widths_inside_a_column() {
    // A 240 column holds 227 of cards: a 60 and a 140 side by side
    // (208), then a 230 that only fits alone.
    let widths = [60.0, 140.0, 230.0, 60.0];
    assert_eq!(cell_lines(&widths, 240.0), [0..2, 2..3, 3..4]);
    assert_eq!(
        cell_height(&widths, 240.0),
        3.0 * CARD_HEIGHT + 2.0 * CARD_GAP
    );
    assert_eq!(cell_height(&[], 240.0), EMPTY);
    // The widest cell on one line sets the panel's width.
    let band = Band {
        desktop: String::new(),
        title: String::new(),
        number: None,
        current: true,
        cells: vec![vec![vec![0, 1, 2]]],
        ghost_cell: None,
        ghosts: Vec::new(),
    };
    let cols = columns(&[band], &[vec![0]], &desk(), &widths);
    let width = natural_width(&cols);
    assert!(close(width, MIN_PANEL.max(INSET + 446.0 + 2.0 * SIDE)));
}

#[test]
fn the_landing_spot_follows_cards_of_mixed_widths() {
    let at = |x: f32, w: f32, y: f32| Rect::from_min_size(pos2(x, y), vec2(w, CARD_HEIGHT));
    let zones = vec![Zone {
        band: 0,
        cell: Some((0, 0)),
        rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 400.0)),
        open: true,
        // A narrow card, a wide one, and a third card on the next line.
        cards: vec![
            (0, at(13.0, 60.0, 26.0)),
            (1, at(81.0, 230.0, 26.0)),
            (2, at(13.0, 179.0, 26.0 + CARD_HEIGHT + CARD_GAP)),
        ],
    }];
    // Over the wide card's left half: before it.
    assert_eq!(
        drop_spot(pos2(150.0, 60.0), &zones, 9, (0, 0)),
        Some(spot(0, (0, 0), 1))
    );
    // Over its right half: after it.
    assert_eq!(
        drop_spot(pos2(250.0, 60.0), &zones, 9, (0, 0)),
        Some(spot(0, (0, 0), 2))
    );
    // On the second line, past the third card's middle.
    assert_eq!(
        drop_spot(pos2(150.0, 200.0), &zones, 9, (0, 0)),
        Some(spot(0, (0, 0), 3))
    );
}

#[test]
fn hidden_bands_are_not_laid_out_or_found_while_collapsed() {
    let all = three_columns();
    let collapsed = visible(all.clone(), false);
    assert_eq!(collapsed.len(), 1);
    assert!(collapsed[0].current);
    // Arrow keys and drops only know the bands on screen.
    assert_eq!(walk(&collapsed), [0, 1, 2, 3, 4]);
    assert_eq!(locate(&collapsed, 5), None);
    assert_eq!(visible(all, true).len(), 2);
}

#[test]
fn hiding_the_hints_makes_the_panel_exactly_the_footer_shorter() {
    let monitors = vec![monitor([0, 0, 1920, 1080], 48, true); 3];
    let rows = vec![vec![0, 1, 2]];
    let all = three_columns();
    let cols = columns(&all, &rows, &monitors, &SAME);
    let limit = vec2(5000.0, 5000.0);
    let with = panel_size(&all, &cols, &SAME, false, true, limit);
    let without = panel_size(&all, &cols, &SAME, false, false, limit);
    assert_eq!(with.x, without.x);
    assert!(close(with.y - without.y, FOOTER));
    assert_eq!(footer_height(false), 0.0);
}

#[test]
fn the_collapsed_panel_is_shorter_and_the_link_adds_a_line() {
    let monitors = vec![monitor([0, 0, 1920, 1080], 48, true); 3];
    let rows = vec![vec![0, 1, 2]];
    let cols_of = |bands: &[Band]| columns(bands, &rows, &monitors, &SAME);
    let limit = vec2(5000.0, 5000.0);
    let all = three_columns();
    let shown = visible(all.clone(), false);
    let open = panel_size(&all, &cols_of(&all), &SAME, true, true, limit);
    let shut = panel_size(&shown, &cols_of(&shown), &SAME, true, true, limit);
    let bare = panel_size(&shown, &cols_of(&shown), &SAME, false, true, limit);
    assert!(shut.y < open.y);
    assert!(close(shut.y - bare.y, others::LINK_HEIGHT));
}

#[test]
fn shift_down_reaches_a_hidden_desktop_through_all_the_bands() {
    let all = three_columns();
    let monitors = vec![monitor([0, 0, 1920, 1080], 48, true); 3];
    let rows = vec![vec![0, 1, 2]];
    let collapsed = visible(all.clone(), false);
    assert_eq!(
        step(&collapsed, &rows, &monitors, 0, Key::DesktopDown),
        None
    );
    let Some(Step::Move(spot)) = step(&all, &rows, &monitors, 0, Key::DesktopDown) else {
        panic!("expected a move");
    };
    assert!(spot.band >= collapsed.len());
}

fn three_columns() -> Vec<Band> {
    vec![
        Band {
            desktop: "d1".to_string(),
            title: "This desktop".to_string(),
            number: Some(2),
            current: true,
            cells: vec![vec![vec![0, 1], vec![2, 3, 4], vec![]]],
            ghost_cell: None,
            ghosts: Vec::new(),
        },
        Band {
            desktop: "d0".to_string(),
            title: "Home".to_string(),
            number: Some(1),
            current: false,
            cells: vec![vec![vec![], vec![5], vec![]]],
            ghost_cell: None,
            ghosts: Vec::new(),
        },
    ]
}

fn spot(band: usize, cell: (usize, usize), index: usize) -> Spot {
    Spot { band, cell, index }
}

#[test]
fn a_move_inside_a_column_keeps_every_other_window_in_its_order() {
    let bands = three_columns();
    // 3 goes before 2, its neighbour in the centre column.
    assert_eq!(
        order_after(6, &bands, 3, spot(0, (0, 1), 0)),
        [0, 1, 3, 2, 4, 5]
    );
    // 0 goes after 1, the last other window of its column.
    assert_eq!(
        order_after(6, &bands, 0, spot(0, (0, 0), 1)),
        [1, 0, 2, 3, 4, 5]
    );
    // Into an empty column it keeps its place.
    assert_eq!(
        order_after(6, &bands, 2, spot(0, (0, 2), 0)),
        [0, 1, 2, 3, 4, 5]
    );
    // Into the other band, before 5.
    assert_eq!(
        order_after(6, &bands, 1, spot(1, (0, 1), 0)),
        [0, 2, 3, 4, 1, 5]
    );
}

#[test]
fn a_new_number_keeps_the_window_in_its_column() {
    // Six windows on this desktop; 1 and 3 are on the right monitor.
    let monitors = desk();
    let desktops = [ArrangeDesktop {
        id: "d1".to_string(),
        name: "Work".to_string(),
        current: true,
    }];
    let on = |hwnd: isize, monitor: usize| ArrangeWindow {
        hwnd,
        label: format!("w{hwnd}"),
        desktop: "d1".to_string(),
        monitor: None,
        on_monitor: Some(monitor),
        saved_monitor: Some(monitor),
        renamed: false,
        same_title: false,
        attention: false,
    };
    let mut group = ArrangeGroup {
        key: "claude.exe".to_string(),
        label: "Claude".to_string(),
        windows: vec![on(0, 0), on(1, 2), on(2, 0), on(3, 2), on(4, 1), on(5, 0)],
    };
    let rows = monitor_rows(&monitors, &every);
    let right = |group: &ArrangeGroup| -> Vec<isize> {
        let bands = bands(group, &desktops, &monitors, &rows);
        bands[0].cells[0][2]
            .iter()
            .map(|member| group.windows[*member].hwnd)
            .collect()
    };
    let number_of = |group: &ArrangeGroup, hwnd: isize| {
        let numbers = taskbar_numbers(group, &desktops);
        numbers[group.windows.iter().position(|w| w.hwnd == hwnd).unwrap()]
    };
    assert_eq!(right(&group), [1, 3]);
    assert_eq!(number_of(&group, 3), 4);

    // Window 3 becomes number 2, as the menu, Alt+Left or a Ctrl drop do.
    let Some(ArrangeAction::Reorder { order, .. }) =
        crate::ui::arrange::take_place(&group, &desktops, 3, 2)
    else {
        panic!("a Reorder");
    };
    group
        .windows
        .sort_by_key(|window| order.iter().position(|h| *h == window.hwnd));
    assert_eq!(number_of(&group, 3), 2);
    // Still on the right monitor, now above window 1 in its column.
    assert_eq!(right(&group), [3, 1]);
    assert!(group
        .windows
        .iter()
        .all(|w| w.on_monitor == w.saved_monitor));
    assert_eq!(group.windows[1].on_monitor, Some(2));
}

#[test]
fn a_limited_strip_has_one_column_with_only_that_monitors_windows() {
    let monitors = desk();
    let on = |hwnd: isize, monitor: usize| ArrangeWindow {
        hwnd,
        label: format!("w{hwnd}"),
        desktop: "d1".to_string(),
        monitor: None,
        on_monitor: Some(monitor),
        saved_monitor: Some(monitor),
        renamed: false,
        same_title: false,
        attention: false,
    };
    let group = ArrangeGroup {
        key: "claude.exe".to_string(),
        label: "Claude".to_string(),
        windows: vec![on(0, 0), on(1, 2), on(2, 0), on(3, 2), on(4, 1)],
    };
    let snapshot = ArrangeSnapshot {
        plugin: "layout_keeper".to_string(),
        look_plugin: "window_deck".to_string(),
        hints: true,
        keeps_order: true,
        groups: vec![group.clone()],
        desktops: vec![ArrangeDesktop {
            id: "d1".to_string(),
            name: "Work".to_string(),
            current: true,
        }],
        focus: 0,
        watched: Vec::new(),
        preview: true,
        can_rename: false,
        monitors,
        view: crate::core::ui_bridge::ArrangeView::ByMonitor,
        anchor: None,
    };
    let widths = [179.0; 5];
    let all = Model::new(
        &group,
        &snapshot,
        &widths,
        false,
        scope(None, MultiMonitor::default()),
    );
    assert_eq!(all.rows, [vec![1, 0, 2]]);
    let right = Model::new(
        &group,
        &snapshot,
        &widths,
        false,
        scope(Some(2), MultiMonitor::default()),
    );
    assert_eq!(right.rows, [vec![2]]);
    // One column in its row: no rail, and it takes the whole row.
    assert_eq!(right.cols.len(), 1);
    assert_eq!(right.cols[0].len(), 1);
    assert!(!right.cols[0][0].rail);
    assert!(!Model::new(
        &group,
        &snapshot,
        &widths,
        false,
        scope(Some(3), MultiMonitor::default())
    )
    .cols
    .iter()
    .flatten()
    .any(|col| col.rail));
    assert_eq!(right.bands[0].cells, [vec![vec![1, 3]]]);
    assert_eq!(walk(&right.bands), [1, 3]);
    // A drop inside the one column reorders the whole group.
    let spot = Spot {
        band: 0,
        cell: (0, 0),
        index: 0,
    };
    assert_eq!(order_after(5, &right.bands, 3, spot), [0, 3, 1, 2, 4]);
}

#[test]
fn a_drop_in_another_band_moves_the_window_to_that_desktop_first() {
    let bands = three_columns();
    let monitors = desk();
    let rows = monitor_rows(&monitors, &every);
    let group = group(&[
        ("d1", Some(1), None),
        ("d1", Some(1), None),
        ("d1", Some(0), None),
        ("d1", Some(0), None),
        ("d1", Some(0), None),
        ("d0", Some(0), None),
    ]);
    let actions = drop_actions(
        &group,
        &desktops(),
        &bands,
        &rows,
        &monitors,
        1,
        spot(1, (0, 1), 1),
    );
    assert_eq!(
        actions[0],
        ArrangeAction::MoveToDesktop {
            hwnd: 101,
            desktop: "d0".to_string()
        }
    );
    // From the left monitor's column to the primary's: moved there too.
    assert_eq!(
        actions[1],
        ArrangeAction::MoveToMonitor {
            hwnd: 101,
            monitor: monitors[0].device.clone()
        }
    );
    assert_eq!(
        actions[2],
        ArrangeAction::Reorder {
            group: "chrome.exe".to_string(),
            order: vec![100, 102, 103, 104, 105, 101],
        }
    );
    // Dropped where it already is: nothing to do.
    assert!(drop_actions(
        &group,
        &desktops(),
        &bands,
        &rows,
        &monitors,
        3,
        spot(0, (0, 1), 1)
    )
    .is_empty());
}

#[test]
fn a_drop_on_another_column_moves_the_window_to_that_monitor_now() {
    let bands = three_columns();
    let monitors = desk();
    let rows = monitor_rows(&monitors, &every);
    let group = group(&[
        ("d1", Some(1), None),
        ("d1", Some(1), None),
        ("d1", Some(0), None),
        ("d1", Some(0), None),
        ("d1", Some(0), None),
        ("d0", Some(0), None),
    ]);
    // Window 3, on the primary monitor, to the empty right column.
    assert_eq!(
        drop_actions(
            &group,
            &desktops(),
            &bands,
            &rows,
            &monitors,
            3,
            spot(0, (0, 2), 0)
        ),
        [
            ArrangeAction::MoveToMonitor {
                hwnd: 103,
                monitor: monitors[2].device.clone()
            },
            // Its place in the taskbar is the same; the order is sent
            // anyway, as after a desktop move.
            ArrangeAction::Reorder {
                group: "chrome.exe".to_string(),
                order: (100..106).collect(),
            }
        ]
    );
}

#[test]
fn a_window_from_a_disconnected_column_is_kept_where_it_is_dropped() {
    let mut monitors = desk();
    monitors.push(disconnected());
    let rows = monitor_rows(&monitors, &every);
    // Saved on the unplugged monitor, on the primary one now.
    let group = group(&[("d1", Some(0), Some(3))]);
    let bands = bands(&group, &desktops(), &monitors, &rows);
    assert_eq!(bands[0].cells[0][3], [0]);
    // Dropped on the primary column: that is where it is, but the
    // plugin hears of it, so the saved monitor follows.
    assert_eq!(
        drop_actions(
            &group,
            &desktops(),
            &bands,
            &rows,
            &monitors,
            0,
            spot(0, (0, 1), 0)
        ),
        [
            ArrangeAction::MoveToMonitor {
                hwnd: 100,
                monitor: monitors[0].device.clone()
            },
            ArrangeAction::Reorder {
                group: "chrome.exe".to_string(),
                order: vec![100],
            }
        ]
    );
}

#[test]
fn a_window_on_another_monitor_than_saved_is_marked_moved() {
    let mut monitors = desk();
    monitors.push(disconnected());
    let windows = group(&[
        ("d1", Some(0), Some(1)),
        ("d1", Some(0), Some(0)),
        ("d1", Some(0), None),
        ("d1", Some(0), Some(3)),
        ("d1", None, Some(1)),
    ]);
    let marked: Vec<bool> = windows
        .windows
        .iter()
        .map(|w| moved(w, &monitors))
        .collect();
    // Only the first: the second is where it was saved, the third was
    // never saved, the fourth sits in its disconnected column and the
    // fifth is on no known monitor.
    assert_eq!(marked, [true, false, false, false, false]);
}

#[test]
fn the_keys_walk_across_columns_and_move_within_one() {
    let bands = three_columns();
    let monitors = desk();
    let rows = monitor_rows(&monitors, &every);
    let step = |cursor: usize, key: Key| step(&bands, &rows, &monitors, cursor, key);
    assert_eq!(walk(&bands), [0, 1, 2, 3, 4, 5]);
    assert_eq!(step(1, Key::Next), Some(Step::Select(Item::Card(2))));
    assert_eq!(step(4, Key::Next), Some(Step::Select(Item::Card(5))));
    assert_eq!(step(5, Key::Next), None);
    assert_eq!(step(0, Key::Prev), None);
    assert_eq!(step(2, Key::Prev), Some(Step::Select(Item::Card(1))));
    assert_eq!(step(3, Key::MoveLeft), Some(Step::Move(spot(0, (0, 1), 0))));
    assert_eq!(
        step(3, Key::MoveRight),
        Some(Step::Move(spot(0, (0, 1), 2)))
    );
    assert_eq!(step(4, Key::MoveRight), None);
    assert_eq!(step(2, Key::MoveLeft), None);
    assert_eq!(
        order_after(6, &bands, 3, spot(0, (0, 1), 2)),
        [0, 1, 2, 4, 3, 5]
    );
}

#[test]
fn shift_moves_to_the_end_of_the_next_monitor_or_desktop() {
    let bands = three_columns();
    let mut monitors = desk();
    let rows = monitor_rows(&monitors, &every);
    let step = |monitors: &[ArrangeMonitor], cursor: usize, key: Key| {
        step(&bands, &rows, monitors, cursor, key)
    };
    // Window 0 is on the left monitor: right goes to the primary's end.
    assert_eq!(
        step(&monitors, 0, Key::MonitorRight),
        Some(Step::Move(spot(0, (0, 1), 3)))
    );
    assert_eq!(step(&monitors, 0, Key::MonitorLeft), None);
    // From the primary to the empty right column.
    assert_eq!(
        step(&monitors, 3, Key::MonitorRight),
        Some(Step::Move(spot(0, (0, 2), 0)))
    );
    // Down to the next desktop, same monitor; up from the first is none.
    assert_eq!(
        step(&monitors, 3, Key::DesktopDown),
        Some(Step::Move(spot(1, (0, 1), 1)))
    );
    assert_eq!(
        step(&monitors, 0, Key::DesktopDown),
        Some(Step::Move(spot(1, (0, 0), 0)))
    );
    assert_eq!(step(&monitors, 0, Key::DesktopUp), None);
    assert_eq!(
        step(&monitors, 5, Key::DesktopUp),
        Some(Step::Move(spot(0, (0, 1), 3)))
    );
    // A disconnected right monitor is skipped; its windows go down to
    // the primary column of the next desktop.
    monitors[2].connected = false;
    assert_eq!(step(&monitors, 3, Key::MonitorRight), None);
    let mut in_gone = three_columns();
    in_gone[0].cells[0][2].push(6);
    assert_eq!(
        super::step(&in_gone, &rows, &monitors, 6, Key::DesktopDown),
        Some(Step::Move(spot(1, (0, 1), 1)))
    );
    assert_eq!(
        super::step(&in_gone, &rows, &monitors, 6, Key::MonitorLeft),
        Some(Step::Move(spot(0, (0, 1), 3)))
    );
}

#[test]
fn the_drop_spot_comes_from_last_frames_zones() {
    let card = |x: f32, y: f32| Rect::from_min_size(pos2(x, y), card_size());
    let column = |x: f32| Rect::from_min_size(pos2(x, 0.0), vec2(300.0, 400.0));
    let zones = vec![
        Zone {
            band: 0,
            cell: Some((0, 0)),
            rect: column(0.0),
            open: true,
            cards: vec![(0, card(13.0, 26.0)), (1, card(13.0, 162.0))],
        },
        Zone {
            band: 0,
            cell: Some((0, 1)),
            rect: column(316.0),
            open: false,
            cards: vec![(2, card(329.0, 26.0))],
        },
        Zone {
            band: 1,
            cell: None,
            rect: Rect::from_min_size(pos2(0.0, 400.0), vec2(700.0, 43.0)),
            open: true,
            cards: Vec::new(),
        },
    ];
    // Below the first card of the first column.
    assert_eq!(
        drop_spot(pos2(50.0, 170.0), &zones, 2, (0, 0)),
        Some(spot(0, (0, 0), 1))
    );
    // Its own card does not count when the dragged one is in the zone.
    assert_eq!(
        drop_spot(pos2(50.0, 300.0), &zones, 0, (0, 0)),
        Some(spot(0, (0, 0), 1))
    );
    // A disconnected column takes no other window: the nearest open
    // zone does, and its own window can still be put back in it.
    assert_eq!(
        drop_spot(pos2(500.0, 100.0), &zones, 0, (0, 0)),
        Some(spot(0, (0, 0), 0))
    );
    assert_eq!(
        drop_spot(pos2(500.0, 100.0), &zones, 2, (0, 0)),
        Some(spot(0, (0, 1), 0))
    );
    // A bare band header puts the window in the home cell.
    assert_eq!(
        drop_spot(pos2(500.0, 420.0), &zones, 0, (0, 1)),
        Some(spot(1, (0, 1), 0))
    );
}

fn scope(only: Option<usize>, taskbars: MultiMonitor) -> Scope {
    Scope { only, taskbars }
}

#[test]
fn the_footer_lists_select_and_move_first_and_drops_from_the_end() {
    let keys = |others| -> Vec<&str> {
        footer_hints(others)
            .into_iter()
            .map(|(_, label)| label)
            .collect()
    };
    assert_eq!(
        keys(true),
        [
            "Select",
            "Open",
            "Move",
            "Next monitor",
            "Next desktop",
            "Change number",
            "Open by number",
            "Taskbar button",
            "Close",
            "New window",
            "Rename",
            "Other desktops",
            "Hide hints"
        ]
    );
    assert!(!keys(false).contains(&"Other desktops"));
    assert_eq!(footer_hints(false)[6].0, "1\u{2013}9");
    assert_eq!(footer_hints(false)[7].0, "Ctrl + 1\u{2013}9");
    assert_eq!(footer_hints(false)[8].0, "Ctrl + W");
    assert_eq!(footer_hints(false)[9].0, "Ctrl + N");
    assert_eq!(footer_hints(false)[10], ("F2", "Rename"));
    // Ctrl + H comes last, after the optional D hint too.
    assert_eq!(
        footer_hints(false).last(),
        Some(&("Ctrl + H", "Hide hints"))
    );
    assert_eq!(footer_hints(true).last(), Some(&HIDE_HINT));
    // Too narrow for all: they go from the end, and Select, Open and
    // Move stay even when they do not fit.
    let widths = [80.0, 60.0, 90.0, 140.0, 140.0];
    assert_eq!(hints_that_fit(&widths, 16.0, 1000.0, 3), 5);
    assert_eq!(hints_that_fit(&widths, 16.0, 270.0, 3), 3);
    assert_eq!(hints_that_fit(&widths, 16.0, 450.0, 3), 4);
    assert_eq!(hints_that_fit(&widths, 16.0, 50.0, 3), 3);
    assert_eq!(hints_that_fit(&widths[..2], 16.0, 50.0, 3), 2);
}

#[test]
fn plain_arrows_only_pick_and_ctrl_arrows_move() {
    let bands = three_columns();
    let monitors = desk();
    let rows = monitor_rows(&monitors, &every);
    for (modifiers, _, key) in arrow_keys() {
        let result = step(&bands, &rows, &monitors, 3, key);
        if modifiers == egui::Modifiers::NONE {
            assert!(matches!(result, Some(Step::Select(_))), "{key:?}");
        } else {
            assert!(modifiers.ctrl, "{key:?} moves, so it needs Ctrl");
            // Up from the first desktop has nowhere to go.
            assert!(!matches!(result, Some(Step::Select(_))), "{key:?}");
        }
    }
    // The keys with more modifiers come before those with fewer.
    let counts: Vec<usize> = arrow_keys()
        .iter()
        .map(|(modifiers, _, _)| usize::from(modifiers.ctrl) + usize::from(modifiers.shift))
        .collect();
    assert!(counts.windows(2).all(|pair| pair[0] >= pair[1]));
}

fn taskbars(mode: u32) -> MultiMonitor {
    MultiMonitor::from_values(Some(1), Some(mode))
}

#[test]
fn only_the_primary_column_gets_ghosts_and_only_when_its_taskbar_lists_all() {
    let monitors = desk();
    let rows = monitor_rows(&monitors, &every);
    // The primary monitor is the centre column.
    assert_eq!(
        ghost_cell(&monitors, &rows, taskbars(0), None),
        Some((0, 1))
    );
    assert_eq!(
        ghost_cell(&monitors, &rows, taskbars(1), None),
        Some((0, 1))
    );
    assert_eq!(ghost_cell(&monitors, &rows, taskbars(2), None), None);
    // One taskbar only: it lists every window.
    let one_taskbar = MultiMonitor::from_values(Some(0), Some(2));
    assert_eq!(
        ghost_cell(&monitors, &rows, one_taskbar, None),
        Some((0, 1))
    );
    // A hover strip limited to a secondary taskbar's own monitor.
    assert_eq!(ghost_cell(&monitors, &rows, taskbars(1), Some(2)), None);
    // A single monitor, even with a disconnected one beside it.
    let single = vec![monitor([0, 0, 1920, 1080], 48, true), disconnected()];
    let single_rows = monitor_rows(&single, &every);
    assert_eq!(ghost_cell(&single, &single_rows, taskbars(0), None), None);
}

/// Six windows of one desktop: 1 and 3 on the right monitor, 4 on the
/// left, the others on the primary one.
fn spread() -> (ArrangeGroup, ArrangeSnapshot) {
    let on = |hwnd: isize, monitor: usize| ArrangeWindow {
        hwnd,
        label: format!("w{hwnd}"),
        desktop: "d1".to_string(),
        monitor: None,
        on_monitor: Some(monitor),
        saved_monitor: Some(monitor),
        renamed: false,
        same_title: false,
        attention: false,
    };
    let group = ArrangeGroup {
        key: "claude.exe".to_string(),
        label: "Claude".to_string(),
        windows: vec![on(0, 0), on(1, 2), on(2, 0), on(3, 2), on(4, 1), on(5, 0)],
    };
    let snapshot = ArrangeSnapshot {
        plugin: "layout_keeper".to_string(),
        look_plugin: "window_deck".to_string(),
        hints: true,
        keeps_order: true,
        groups: vec![group.clone()],
        desktops: vec![ArrangeDesktop {
            id: "d1".to_string(),
            name: "Work".to_string(),
            current: true,
        }],
        focus: 0,
        watched: Vec::new(),
        preview: true,
        can_rename: false,
        monitors: desk(),
        view: crate::core::ui_bridge::ArrangeView::ByMonitor,
        anchor: None,
    };
    (group, snapshot)
}

// The user wants ghosts draggable; earlier designs said they were not.
// These tests fail if that ever comes back.

#[test]
fn ghosts_can_be_dragged_with_the_mouse_like_cards() {
    assert_eq!(item_sense(Item::Ghost(1), false), Sense::click_and_drag());
    assert_eq!(item_sense(Item::Card(1), false), Sense::click_and_drag());
    // Only a card whose window is gone takes no drag.
    assert_eq!(item_sense(Item::Card(1), true), Sense::hover());
}

fn reorder(order: Vec<isize>) -> ArrangeAction {
    ArrangeAction::Reorder {
        group: "claude.exe".to_string(),
        order,
    }
}

#[test]
fn ghosts_can_be_dragged_within_the_primary_column_and_their_window_stays() {
    let (group, snapshot) = spread();
    let model = spread_model(1);
    let drop = |moved: Item, index: usize| {
        drop_item_actions(
            &group,
            &snapshot.desktops,
            &model.bands,
            &model.rows,
            &snapshot.monitors,
            moved,
            spot(0, (0, 1), index),
        )
    };
    // Window 1 (on the right monitor) by its ghost, after card 2: it
    // takes number 3, and no move to the primary monitor is asked for.
    assert_eq!(drop(Item::Ghost(1), 2), [reorder(vec![0, 2, 1, 3, 4, 5])]);
    // Its real card dropped at the same place moves it there too.
    assert!(matches!(
        drop(Item::Card(1), 2).first(),
        Some(ArrangeAction::MoveToMonitor { hwnd: 1, .. })
    ));
    // Put back where it was: nothing to do.
    assert!(drop(Item::Ghost(1), 1).is_empty());
}

#[test]
fn ghosts_can_be_dragged_to_another_monitors_column_and_move_the_window() {
    let (group, snapshot) = spread();
    let model = spread_model(1);
    let to_right = spot(0, (0, 2), 0);
    let by_ghost = drop_item_actions(
        &group,
        &snapshot.desktops,
        &model.bands,
        &model.rows,
        &snapshot.monitors,
        Item::Ghost(4),
        to_right,
    );
    assert_eq!(
        by_ghost[0],
        ArrangeAction::MoveToMonitor {
            hwnd: 4,
            monitor: snapshot.monitors[2].device.clone()
        }
    );
    // Exactly what its own card would do.
    assert_eq!(
        by_ghost,
        drop_item_actions(
            &group,
            &snapshot.desktops,
            &model.bands,
            &model.rows,
            &snapshot.monitors,
            Item::Card(4),
            to_right,
        )
    );
}

#[test]
fn ghosts_can_be_moved_with_ctrl_arrows_and_their_window_stays() {
    let (group, snapshot) = spread();
    let model = spread_model(1);
    let actions = |spot: Spot| {
        drop_item_actions(
            &group,
            &snapshot.desktops,
            &model.bands,
            &model.rows,
            &snapshot.monitors,
            Item::Ghost(1),
            spot,
        )
    };
    let right = ghost_step(&model.bands, 1, true).expect("a place to the right");
    assert_eq!(right, spot(0, (0, 1), 2));
    assert_eq!(actions(right), [reorder(vec![0, 2, 1, 3, 4, 5])]);
    let left = ghost_step(&model.bands, 1, false).expect("a place to the left");
    assert_eq!(actions(left), [reorder(vec![1, 0, 2, 3, 4, 5])]);
    // Ghost 4 to the right: after card 5, the last place.
    assert_eq!(ghost_step(&model.bands, 4, true), Some(spot(0, (0, 1), 5)));
    // A window with no ghost has no ghost place to step to.
    assert_eq!(ghost_step(&model.bands, 0, true), None);
}

fn spread_model(mode: u32) -> Model {
    let (group, snapshot) = spread();
    Model::new(
        &group,
        &snapshot,
        &[179.0; 6],
        false,
        scope(None, taskbars(mode)),
    )
}

#[test]
fn ghosts_sit_at_their_windows_taskbar_numbers() {
    let (group, snapshot) = spread();
    let model = spread_model(1);
    let band = &model.bands[0];
    assert_eq!(band.ghost_cell, Some((0, 1)));
    let flow = band.flow((0, 1));
    assert_eq!(
        flow,
        [
            Item::Card(0),
            Item::Ghost(1),
            Item::Card(2),
            Item::Ghost(3),
            Item::Ghost(4),
            Item::Card(5),
        ]
    );
    // Left to right, the column reads the taskbar's numbers in order.
    let numbers = taskbar_numbers(&group, &snapshot.desktops);
    let read: Vec<usize> = flow.iter().map(|item| numbers[item.member()]).collect();
    assert_eq!(read, [1, 2, 3, 4, 5, 6]);
    // The other columns have no ghosts, and ghosts are not counted.
    assert_eq!(band.flow((0, 2)), [Item::Card(1), Item::Card(3)]);
    assert_eq!(model.cols[0][1].count, 3);
    // Three cards and three ghosts on one line.
    let need = INSET + 3.0 * 179.0 + 3.0 * GHOST_WIDTH + 5.0 * CARD_GAP;
    assert!(close(model.cols[0][1].need, need));
    // Every taskbar lists only its own windows: no ghosts.
    assert!(spread_model(2).bands[0].ghosts.is_empty());
}

#[test]
fn a_primary_monitor_without_windows_folds_into_a_rail_without_ghosts() {
    let (mut group, mut snapshot) = spread();
    group.windows.retain(|window| window.on_monitor != Some(0));
    snapshot.groups = vec![group.clone()];
    let model = Model::new(
        &group,
        &snapshot,
        &[179.0; 3],
        false,
        scope(None, taskbars(0)),
    );
    assert!(model.cols[0][1].rail);
    assert_eq!(model.bands[0].ghost_cell, None);
    assert!(model.bands[0].ghosts.is_empty());
}

#[test]
fn a_disconnected_monitors_windows_get_no_ghosts() {
    let mut monitors = desk();
    monitors.push(disconnected());
    let rows = monitor_rows(&monitors, &every);
    let mut bands = bands(
        &group(&[("d1", Some(0), Some(3)), ("d1", Some(2), None)]),
        &desktops(),
        &monitors,
        &rows,
    );
    add_ghosts(&mut bands, &rows, &monitors, Some((0, 1)));
    assert_eq!(bands[0].ghosts, [1]);
}

#[test]
fn the_keys_land_on_ghosts_and_move_among_cards() {
    let (group, snapshot) = spread();
    let model = spread_model(1);
    let monitors = &snapshot.monitors;
    // The left column, the primary one with its ghosts, the right one.
    assert_eq!(walk(&model.bands), [4, 0, 2, 5, 1, 3]);
    assert_eq!(
        walk_items(&model.bands),
        [
            Item::Card(4),
            Item::Card(0),
            Item::Ghost(1),
            Item::Card(2),
            Item::Ghost(3),
            Item::Ghost(4),
            Item::Card(5),
            Item::Card(1),
            Item::Card(3),
        ]
    );
    let step = |cursor: usize, key: Key| step(&model.bands, &model.rows, monitors, cursor, key);
    assert_eq!(step(0, Key::Next), Some(Step::Select(Item::Ghost(1))));
    assert_eq!(step(5, Key::Next), Some(Step::Select(Item::Card(1))));
    assert_eq!(
        next_item(&model.bands, Item::Ghost(4), true),
        Some(Item::Card(5))
    );
    assert_eq!(
        next_item(&model.bands, Item::Ghost(1), false),
        Some(Item::Card(0))
    );
    assert_eq!(next_item(&model.bands, Item::Card(3), true), None);
    // Up and down pick ghosts by place too.
    let at = |x: f32, y: f32| Rect::from_min_size(pos2(x, y), vec2(84.0, CARD_HEIGHT));
    let drawn = [
        (Item::Card(0), at(0.0, 0.0)),
        (Item::Ghost(1), at(0.0, 140.0)),
    ];
    assert_eq!(
        card_above_or_below(&drawn, Item::Card(0), true),
        Some(Item::Ghost(1))
    );
    // Window 5 one card to the left: before window 2, past ghosts 3, 4.
    let Some(Step::Move(left)) = step(5, Key::MoveLeft) else {
        panic!("expected a move");
    };
    assert_eq!(left, spot(0, (0, 1), 2));
    assert_eq!(
        drop_actions(
            &group,
            &snapshot.desktops,
            &model.bands,
            &model.rows,
            monitors,
            5,
            left
        ),
        [ArrangeAction::Reorder {
            group: "claude.exe".to_string(),
            order: vec![0, 1, 5, 2, 3, 4],
        }]
    );
    // Window 4 from the left monitor to the end of the primary column:
    // after its last card.
    assert_eq!(
        step(4, Key::MonitorRight),
        Some(Step::Move(spot(0, (0, 1), 5)))
    );
}

#[test]
fn a_drop_beside_a_ghost_takes_the_place_next_to_its_window() {
    let (group, snapshot) = spread();
    let model = spread_model(1);
    let drop = |moved: usize, index: usize| {
        drop_actions(
            &group,
            &snapshot.desktops,
            &model.bands,
            &model.rows,
            &snapshot.monitors,
            moved,
            spot(0, (0, 1), index),
        )
    };
    let reorder = |order: Vec<isize>| ArrangeAction::Reorder {
        group: "claude.exe".to_string(),
        order,
    };
    // Window 0 after ghost 3: before ghost 4, so number 4.
    assert_eq!(drop(0, 3), [reorder(vec![1, 2, 3, 0, 4, 5])]);
    // Window 2 before ghost 1: number 2.
    assert_eq!(drop(2, 1), [reorder(vec![0, 2, 1, 3, 4, 5])]);
    // Past the last item: the last number.
    assert_eq!(drop(0, 5), [reorder(vec![1, 2, 3, 4, 5, 0])]);
    // Window 1 from the right monitor, before ghost 4: it moves to the
    // primary monitor and takes number 4.
    assert_eq!(
        drop(1, 3),
        [
            ArrangeAction::MoveToMonitor {
                hwnd: 1,
                monitor: snapshot.monitors[0].device.clone()
            },
            reorder(vec![0, 2, 3, 1, 4, 5]),
        ]
    );
    // Back where it is: nothing to do.
    assert!(drop(2, 2).is_empty());
}

#[test]
fn a_drop_beside_a_ghost_keeps_other_desktops_in_their_slots() {
    let (mut group, mut snapshot) = spread();
    group.windows[2].desktop = "d0".to_string();
    snapshot.desktops.push(ArrangeDesktop {
        id: "d0".to_string(),
        name: "Home".to_string(),
        current: false,
    });
    snapshot.groups = vec![group.clone()];
    let model = Model::new(
        &group,
        &snapshot,
        &[179.0; 6],
        false,
        scope(None, taskbars(0)),
    );
    // Window 5 before ghost 1, on this desktop's row 0 1 3 4 5.
    let actions = drop_actions(
        &group,
        &snapshot.desktops,
        &model.bands,
        &model.rows,
        &snapshot.monitors,
        5,
        spot(0, (0, 1), 1),
    );
    assert_eq!(
        actions,
        [ArrangeAction::Reorder {
            group: "claude.exe".to_string(),
            order: vec![0, 5, 2, 1, 3, 4],
        }]
    );
}

#[test]
fn a_ghost_points_toward_its_monitor_and_its_picture_covers_its_box() {
    let monitors = desk();
    assert!(points_left(&monitors[1], &monitors[0]));
    assert!(!points_left(&monitors[2], &monitors[0]));
    assert_eq!(monitor_letter("right"), "R");
    assert_eq!(monitor_letter(""), "");
    // A wide window covers the 62 x 126 box: as tall, centred, cut at
    // the sides.
    let area = Rect::from_min_size(pos2(20.0, 0.0), vec2(62.0, 126.0));
    let wide = cover(vec2(1600.0, 900.0), area);
    assert!(close(wide.height(), 126.0));
    assert!(wide.width() > 62.0);
    assert_eq!(wide.center(), area.center());
    let tall = cover(vec2(100.0, 1000.0), area);
    assert!(close(tall.width(), 62.0));
    assert!(tall.height() > 126.0);
    assert_eq!(cover(Vec2::ZERO, area), area);
}

#[test]
fn a_drag_lands_beside_ghosts_like_beside_cards() {
    let at = |x: f32, w: f32| Rect::from_min_size(pos2(x, 26.0), vec2(w, CARD_HEIGHT));
    let zones = vec![Zone {
        band: 0,
        cell: Some((0, 1)),
        rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 200.0)),
        open: true,
        // A card, a ghost, a card.
        cards: vec![
            (0, at(13.0, 179.0)),
            (1, at(200.0, 84.0)),
            (2, at(292.0, 179.0)),
        ],
    }];
    // Over the ghost's right half: after it, before card 2.
    assert_eq!(
        drop_spot(pos2(270.0, 60.0), &zones, 5, (0, 1)),
        Some(spot(0, (0, 1), 2))
    );
    // Over its left half: before it.
    assert_eq!(
        drop_spot(pos2(210.0, 60.0), &zones, 5, (0, 1)),
        Some(spot(0, (0, 1), 1))
    );
}

#[test]
fn a_ghost_of_a_window_that_wants_attention_is_tinted_with_orange_dashes() {
    let tokens = Tokens::dark();
    let (fill, outline) = ghost_colours(&tokens, true);
    assert_eq!(fill, card_look(&tokens, None, true).fill);
    assert!(fill.is_some());
    assert_eq!(outline, tokens.warning);
    assert_eq!(ghost_colours(&tokens, false), (None, tokens.border));
}
