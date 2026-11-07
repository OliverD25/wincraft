use super::*;

fn screen(rect: [i32; 4], primary: bool) -> ArrangeMonitor {
    ArrangeMonitor {
        device: String::new(),
        label: String::new(),
        name: String::new(),
        rect,
        work: rect,
        primary,
        connected: true,
    }
}

/// The main 4K monitor and a portrait one to its right.
fn screens() -> Vec<ArrangeMonitor> {
    vec![
        screen([0, 0, 3840, 2160], true),
        screen([3840, 0, 6400, 2880], false),
    ]
}

/// `count` windows on this desktop, all on monitor `on`.
fn group_on(count: usize, on: usize) -> ArrangeGroup {
    ArrangeGroup {
        key: "claude.exe".to_string(),
        label: "Claude".to_string(),
        windows: (0..count)
            .map(|i| ArrangeWindow {
                hwnd: i as isize + 1,
                label: format!("w{i}"),
                desktop: String::new(),
                monitor: None,
                on_monitor: Some(on),
                saved_monitor: None,
                renamed: false,
                same_title: false,
                attention: false,
            })
            .collect(),
    }
}

fn snapshot(group: ArrangeGroup, setting: bool) -> ArrangeSnapshot {
    ArrangeSnapshot {
        plugin: "window_deck".to_string(),
        groups: vec![group],
        desktops: Vec::new(),
        focus: 0,
        watched: Vec::new(),
        preview: false,
        can_rename: false,
        monitors: screens(),
        view: ArrangeView::ByMonitor,
        look_plugin: "window_deck".to_string(),
        hints: true,
        toolbar_on_other_monitors: setting,
        keeps_order: false,
        anchor: None,
    }
}

/// The strip's state for `count` windows on the portrait monitor, limited
/// to monitor `only`, on a monitor `work` wide at 175 %.
fn strip(count: usize, only: Option<usize>, setting: bool, width: i32) -> Arrange {
    let bridge = crate::core::ui_bridge::create();
    let arrange = Arrange::new(bridge.to_host.clone(), 0);
    {
        let mut shared = arrange.shared.lock().expect("the strip's state");
        let group = group_on(count, only.unwrap_or(1));
        shared.key = Some(group.key.clone());
        shared.snapshot = Some(snapshot(group, setting));
        shared.only_monitor = only;
        shared.monitor = Some(Monitor {
            work: [3840, 0, 3840 + width, 2832],
            scale: 1.75,
        });
    }
    arrange
}

#[test]
fn cards_that_fit_on_one_line_at_compact_size_share_the_room_up_to_280() {
    // 3 at 180 with two gaps of 8 need 556.
    assert_eq!(tile_width(3, 556.0), Some(180.0));
    assert_eq!(tile_width(3, 700.0), Some(228.0));
    assert_eq!(tile_width(3, 1400.0), Some(TILE_MAX));
    assert_eq!(tile_width(1, 5000.0), Some(TILE_MAX));
    assert_eq!(tile_width(3, 555.0), None, "one point short");
    assert_eq!(tile_width(8, 1400.0), None);
    assert_eq!(tile_width(0, 1400.0), None);
}

#[test]
fn every_card_gets_the_same_width_and_a_picture_in_its_monitors_shape() {
    let monitors = screens();
    let portrait = group_on(4, 1);
    let tiles = tiles_for(&portrait, &monitors, &[0, 1, 2, 3], 1400.0).expect("they fit");
    assert_eq!(tiles.width, TILE_MAX);
    // 280 wide in a 2560 x 2880 shape is 315 tall: held at the most.
    assert_eq!(tiles.thumb, tile::THUMB_MAX);
    let wide = group_on(3, 0);
    let tiles = tiles_for(&wide, &monitors, &[0, 1, 2], 600.0).expect("they fit");
    assert_eq!(tiles.width, 194.0);
    assert_eq!(tiles.thumb, 109.0, "194 wide at 16:9");
    // A very wide monitor's pictures are never flatter than the least.
    assert_eq!(
        tile::thumb_height(200.0, Some(vec2(5120.0, 1440.0))),
        tile::THUMB_MIN
    );
    assert_eq!(tile::thumb_height(180.0, None), 101.0);
}

#[test]
fn the_compact_cards_come_only_when_they_fit_on_a_monitor_that_is_not_the_main_one() {
    // 2560 px at 175 %: 90 % of it is 1316 points, room for 6 cards of 180.
    let three = strip(3, Some(1), false, 2560);
    let tiles = three.shared.lock().unwrap().tiles().expect("3 fit");
    assert_eq!(tiles.width, TILE_MAX);
    assert!(strip(6, Some(1), false, 2560)
        .shared
        .lock()
        .unwrap()
        .tiles()
        .is_some());
    assert!(
        strip(7, Some(1), false, 2560)
            .shared
            .lock()
            .unwrap()
            .tiles()
            .is_none(),
        "7 do not fit: today's cards"
    );
    // One or two windows open the compact popup instead.
    assert!(strip(2, Some(1), false, 2560)
        .shared
        .lock()
        .unwrap()
        .tiles()
        .is_none());
    // The main monitor and Win+Alt+A keep today's cards.
    assert!(strip(3, Some(0), false, 2560)
        .shared
        .lock()
        .unwrap()
        .tiles()
        .is_none());
    assert!(strip(3, None, false, 2560)
        .shared
        .lock()
        .unwrap()
        .tiles()
        .is_none());
}

#[test]
fn the_toolbar_shows_on_the_main_monitor_and_elsewhere_only_with_the_setting() {
    assert!(shows_toolbar(false, false));
    assert!(shows_toolbar(false, true));
    assert!(shows_toolbar(true, true));
    assert!(!shows_toolbar(true, false));
    let monitors = screens();
    assert!(on_other_monitor(Some(1), &monitors));
    assert!(
        !on_other_monitor(Some(0), &monitors),
        "mode 2 on the main one"
    );
    assert!(!on_other_monitor(None, &monitors), "Win+Alt+A");
    let short = |only, setting| {
        strip(3, only, setting, 2560)
            .shared
            .lock()
            .unwrap()
            .short_header()
    };
    assert!(short(Some(1), false));
    assert!(!short(Some(1), true));
    assert!(!short(Some(0), false));
    assert!(!short(None, false));
}

#[test]
fn the_panel_is_as_wide_as_the_line_of_cards_and_as_tall_as_one_card() {
    let tiles = Tiles {
        width: 280.0,
        thumb: 240.0,
    };
    let row = |members: Vec<usize>| Row {
        title: "This desktop".to_string(),
        desktop: String::new(),
        members,
    };
    let limit = vec2(1316.0, 1000.0);
    let size = panel_size(&[row(vec![0, 1, 2])], tiles, false, false, limit);
    assert_eq!(size.x, 2.0 * PADDING + 3.0 * 280.0 + 2.0 * CARD_GAP + SLACK);
    let card = tile::card_height(240.0);
    assert_eq!(size.y, CARDS_TOP + RING_ROOM + card + PADDING + SLACK);
    // Another desktop with more cards than fit wraps them under its name.
    let others = row((3..10).collect());
    let open = panel_size(&[row(vec![0, 1, 2]), others], tiles, true, false, limit);
    assert!(open.x <= limit.x);
    assert!(open.y > size.y + 2.0 * card);
}
