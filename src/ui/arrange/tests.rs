use super::*;

#[test]
fn ctrl_h_flips_the_hints_now_and_asks_the_look_plugin_to_save_them() {
    let bridge = crate::core::ui_bridge::create();
    let arrange = Arrange::new(bridge.to_host.clone(), 0);
    let mut shared = arrange.shared.lock().expect("the strip's state");
    let shared = &mut *shared;
    toggle_hints(shared);
    assert!(
        bridge.host_rx.try_recv().is_err(),
        "no snapshot, no request"
    );
    shared.snapshot = Some(ArrangeSnapshot {
        plugin: "window_deck".to_string(),
        groups: Vec::new(),
        desktops: Vec::new(),
        focus: 0,
        watched: Vec::new(),
        preview: false,
        can_rename: false,
        monitors: Vec::new(),
        view: ArrangeView::ByMonitor,
        look_plugin: "window_deck".to_string(),
        hints: true,
        keeps_order: false,
        anchor: None,
    });
    toggle_hints(shared);
    assert!(!shared.snapshot.as_ref().is_some_and(|s| s.hints));
    match bridge.host_rx.try_recv().ok() {
        Some(HostRequest::SetSetting { plugin, key, value }) => {
            assert_eq!(plugin, "window_deck");
            assert_eq!(key, "show_hints");
            assert_eq!(value, serde_json::Value::Bool(false));
        }
        _ => panic!("expected the setting to be saved"),
    }
    toggle_hints(shared);
    assert!(shared.snapshot.as_ref().is_some_and(|s| s.hints));
}

/// A card of a 16:9 window.
const CARD: Vec2 = Vec2::new(152.0, CARD_HEIGHT);
const SAME: [f32; 20] = [CARD.x; 20];

fn desktop(id: &str, name: &str, current: bool) -> ArrangeDesktop {
    ArrangeDesktop {
        id: id.to_string(),
        name: name.to_string(),
        current,
    }
}

fn group(desktops: &[&str]) -> ArrangeGroup {
    ArrangeGroup {
        key: "chrome.exe".to_string(),
        label: "Chrome".to_string(),
        windows: desktops
            .iter()
            .enumerate()
            .map(|(i, d)| ArrangeWindow {
                hwnd: i as isize,
                label: format!("w{i}"),
                desktop: d.to_string(),
                monitor: None,
                on_monitor: None,
                saved_monitor: None,
                renamed: false,
                same_title: false,
                attention: false,
            })
            .collect(),
    }
}

#[test]
fn a_hover_strip_is_centred_above_its_button() {
    // A 4K monitor at 150 %, taskbar 72 px tall at the bottom.
    let monitor = Monitor {
        work: [0, 0, 3840, 2088],
        scale: 1.5,
    };
    let panel = vec2(400.0, 200.0);
    let [left, top, width, height] = monitor.above_button(panel, [1900, 2088, 1972, 2160]);
    assert_eq!((width, height), (600, 300));
    assert_eq!(left + width / 2, 1936, "centred on the button");
    assert_eq!(top + height, 2088 - 18, "just above the taskbar");
}

#[test]
fn a_hover_strip_stays_inside_the_screen_edges() {
    let monitor = Monitor {
        work: [-1920, 0, 0, 1032],
        scale: 1.0,
    };
    let panel = vec2(800.0, 200.0);
    let near_left = monitor.above_button(panel, [-1900, 1032, -1860, 1080]);
    assert_eq!(near_left[0], -1920);
    let near_right = monitor.above_button(panel, [-60, 1032, -20, 1080]);
    assert_eq!(near_right[0] + near_right[2], 0);
    let wider_than_screen = monitor.above_button(vec2(2400.0, 200.0), [-960, 1032, -920, 1080]);
    assert_eq!(wider_than_screen[0], -1920);
    let too_tall = monitor.above_button(vec2(400.0, 1400.0), [-960, 1032, -920, 1080]);
    assert_eq!(too_tall[1], 0, "never above the top of the screen");
}

#[test]
fn a_taskbar_at_the_top_gets_the_strip_below_it() {
    let monitor = Monitor {
        work: [0, 48, 1920, 1080],
        scale: 1.0,
    };
    let rect = monitor.above_button(vec2(400.0, 200.0), [900, 0, 948, 48]);
    assert_eq!(rect[1], 48 + 12);
    assert_eq!(
        monitor.rect_for(vec2(400.0, 200.0), None),
        monitor.window_rect(vec2(400.0, 200.0)),
        "Win+Alt+A keeps its place"
    );
}

#[test]
fn the_toolbar_counts_monitors_desktops_and_windows() {
    assert_eq!(
        summary(3, 6, 27),
        "3 monitors \u{b7} 6 desktops \u{b7} 27 windows"
    );
    // Without the desktop list there is still the one desktop.
    assert_eq!(
        summary(1, 0, 1),
        "1 monitor \u{b7} 1 desktop \u{b7} 1 window"
    );
}

#[test]
fn a_scene_peeks_at_the_card_it_names() {
    let cards = [(10, false), (11, true), (12, true)];
    assert_eq!(forced_card(&cards, PeekCard::Index(0)), Some(10));
    assert_eq!(forced_card(&cards, PeekCard::Index(2)), Some(12));
    assert_eq!(forced_card(&cards, PeekCard::Index(3)), None);
    assert_eq!(forced_card(&cards, PeekCard::Chip), Some(11));
    // Without a chip card, the first card stands in.
    assert_eq!(
        forced_card(&[(10, false), (11, false)], PeekCard::Chip),
        Some(10)
    );
    assert_eq!(forced_card(&[], PeekCard::Chip), None);
    assert_eq!(forced_card(&[], PeekCard::Index(0)), None);
}

#[test]
fn the_current_desktop_comes_first_then_task_view_order() {
    let desktops = [
        desktop("home", "Home", false),
        desktop("work", "Work", true),
        desktop("read", "Reading", false),
    ];
    let rows = rows(&group(&["read", "work", "home", "work", ""]), &desktops);
    let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(titles, ["This desktop", "Home", "Reading"]);
    assert_eq!(rows[0].members, [1, 3, 4]);
    assert_eq!(rows[1].members, [2]);
    assert_eq!(rows[2].members, [0]);
}

#[test]
fn taskbar_numbers_count_from_one_on_each_desktop() {
    let desktops = [
        desktop("home", "Home", false),
        desktop("work", "Work", true),
    ];
    let group = group(&["home", "work", "home", "work", "", "home"]);
    // The unknown desktop counts as this one, after the windows before it.
    assert_eq!(taskbar_numbers(&group, &desktops), [1, 1, 2, 2, 3, 3]);
}

#[test]
fn a_digit_opens_the_card_with_that_number_on_this_desktop() {
    let desktops = [
        desktop("home", "Home", false),
        desktop("work", "Work", true),
    ];
    // This desktop's cards are windows 1, 3 and 4, numbered 1, 2, 3.
    let group = group(&["home", "work", "home", "work", ""]);
    let all = [true; 5];
    assert_eq!(card_for_digit(&group, &desktops, &all, 1), Some(1));
    assert_eq!(card_for_digit(&group, &desktops, &all, 3), Some(4));
    assert_eq!(
        card_for_digit(&group, &desktops, &all, 4),
        None,
        "no card 4"
    );
    assert_eq!(
        card_for_digit(&group, &desktops, &all, 0),
        None,
        "no card 10"
    );
    // A card that is not on screen is not opened.
    let hidden = [true, true, true, false, true];
    assert_eq!(card_for_digit(&group, &desktops, &hidden, 2), None);

    let twelve = self::group(&["work"; 12]);
    let shown = [true; 12];
    assert_eq!(
        card_for_digit(&twelve, &desktops, &shown, 0),
        Some(9),
        "0 is 10"
    );
}

#[test]
fn win_and_the_same_number_again_picks_the_next_card_round_to_the_first() {
    let cards = [10, 11, 12];
    assert_eq!(next_card(&cards, Some(10)), Some(11));
    assert_eq!(next_card(&cards, Some(12)), Some(10));
    assert_eq!(next_card(&cards, None), Some(10));
    assert_eq!(
        next_card(&cards, Some(99)),
        Some(10),
        "a card of another desktop"
    );
    assert_eq!(next_card(&[], Some(10)), None);
}

#[test]
fn taskbar_numbers_follow_the_order_after_a_drag() {
    let desktops = [desktop("work", "Work", true)];
    let mut group = self::group(&["work", "work", "work"]);
    group.windows.swap(0, 2);
    let numbers = taskbar_numbers(&group, &desktops);
    let place = |hwnd: isize| {
        let member = group.windows.iter().position(|w| w.hwnd == hwnd);
        numbers[member.unwrap()]
    };
    assert_eq!((place(2), place(1), place(0)), (1, 2, 3));
    assert!(taskbar_numbers(&self::group(&[]), &desktops).is_empty());
}

#[test]
fn this_desktop_is_listed_even_when_it_has_no_windows() {
    let desktops = [
        desktop("home", "Home", false),
        desktop("work", "Work", true),
    ];
    let rows = rows(&group(&["home"]), &desktops);
    assert_eq!(rows.len(), 2);
    assert!(rows[0].members.is_empty());
    assert_eq!(rows[1].members, [0]);
}

#[test]
fn the_panel_grows_with_the_widest_row_up_to_the_limit() {
    let row = |n: usize| Row {
        title: String::new(),
        desktop: String::new(),
        members: (0..n).collect(),
    };
    let size = panel_size(
        &[row(3), row(1)],
        &SAME,
        false,
        true,
        0.0,
        vec2(5000.0, 5000.0),
    );
    assert_eq!(
        size.x,
        2.0 * PADDING + 3.0 * CARD.x + 2.0 * CARD_GAP + SLACK
    );
    assert_eq!(
        size.y,
        2.0 * PADDING + 2.0 * (HEADER + HEADER_GAP + CARD.y) + ROW_GAP + FOOTER + SLACK
    );
}

#[test]
fn a_long_row_wraps_onto_more_lines_instead_of_scrolling() {
    let row = |n: usize| Row {
        title: String::new(),
        desktop: String::new(),
        members: (0..n).collect(),
    };
    let limit = vec2(2.0 * PADDING + 5.0 * CARD.x + 4.0 * CARD_GAP + 10.0, 5000.0);
    let size = panel_size(&[row(12)], &SAME, false, true, 0.0, limit);
    assert_eq!(
        size.x,
        2.0 * PADDING + 5.0 * CARD.x + 4.0 * CARD_GAP + SLACK
    );
    // 12 cards at 5 a line make 3 lines.
    assert_eq!(
        size.y,
        2.0 * PADDING + HEADER + HEADER_GAP + 3.0 * CARD.y + 2.0 * CARD_GAP + SLACK
    );
    let short = panel_size(&[row(12)], &SAME, false, true, 0.0, vec2(limit.x, 200.0));
    assert_eq!(short.y, 200.0);
}

#[test]
fn a_picture_keeps_the_windows_shape_within_limits() {
    let close = |a: f32, b: f32| (a - b).abs() < 0.01;
    // A 16:9 window, a portrait one and an unknown size, 81 tall.
    assert!(close(
        picture_width(Some(vec2(1920.0, 1080.0)), 81.0),
        144.0
    ));
    assert!(close(picture_width(Some(vec2(1000.0, 1250.0)), 81.0), 65.0));
    assert!(close(picture_width(None, 81.0), 144.0));
    assert!(close(picture_width(Some(vec2(0.0, 900.0)), 81.0), 144.0));
    // Very narrow and very wide windows stop at 0.6 and 2.4 heights.
    assert!(close(picture_width(Some(vec2(200.0, 2000.0)), 100.0), 60.0));
    assert!(close(
        picture_width(Some(vec2(7680.0, 1080.0)), 100.0),
        240.0
    ));
    // Whole points, so a window a pixel wider gives the same card.
    assert_eq!(
        picture_width(Some(vec2(1921.0, 1080.0)), 81.0),
        picture_width(Some(vec2(1920.0, 1080.0)), 81.0)
    );
    assert_eq!(card_width(None), CARD.x);
}

#[test]
fn a_picture_shows_the_visible_frame_or_else_the_whole_window() {
    let size = vec2(1936.0, 1048.0);
    let frame = Rect::from_min_max(pos2(8.0, 0.0), pos2(1928.0, 1040.0));
    assert_eq!(picture_source(Some(frame), size), frame);
    let whole = Rect::from_min_size(pos2(0.0, 0.0), size);
    assert_eq!(picture_source(None, size), whole);
    // A minimized window's frame is its small one while minimized.
    let iconic = Rect::from_min_max(pos2(0.0, 0.0), pos2(160.0, 28.0));
    assert_eq!(picture_source(Some(iconic), size), whole);
    let outside = Rect::from_min_max(pos2(-8.0, 0.0), pos2(1928.0, 1040.0));
    assert_eq!(picture_source(Some(outside), size), whole);
}

#[test]
fn cards_of_mixed_widths_wrap_where_the_next_one_does_not_fit() {
    // 100 + 8 + 200 = 308 fits in 310; the 60 after it does not.
    let widths = [100.0, 200.0, 60.0, 60.0, 300.0];
    assert_eq!(wrap(&widths, 310.0), [0..2, 2..4, 4..5]);
    // A card wider than the line still gets a line of its own.
    assert_eq!(wrap(&[400.0, 50.0], 310.0), [0..1, 1..2]);
    assert_eq!(wrap(&widths, 5000.0).first(), Some(&(0..5)));
    assert_eq!(wrap(&widths, 5000.0).len(), 1);
    assert!(wrap(&[], 310.0).is_empty());
    assert_eq!(line_width(&[100.0, 200.0]), 308.0);
    assert_eq!(line_width(&[]), 0.0);
}

#[test]
fn the_panel_follows_the_widths_of_the_cards() {
    let rows = [
        Row {
            title: String::new(),
            desktop: String::new(),
            members: vec![0, 1, 2],
        },
        Row {
            title: String::new(),
            desktop: String::new(),
            members: vec![3],
        },
    ];
    // A wide, a narrow and a 16:9 window, then a very wide one alone.
    let widths = [200.0, 60.0, CARD.x, 300.0];
    let size = panel_size(&rows, &widths, false, true, 0.0, vec2(5000.0, 5000.0));
    assert_eq!(size.x, 2.0 * PADDING + 412.0 + 2.0 * CARD_GAP + SLACK);
    // Squeezed to 300 points of cards, the first row needs two lines.
    let limit = vec2(2.0 * PADDING + 300.0, 5000.0);
    let size = panel_size(&rows, &widths, false, true, 0.0, limit);
    assert_eq!(size.x, limit.x);
    assert_eq!(
        size.y,
        2.0 * PADDING
            + 2.0 * (HEADER + HEADER_GAP)
            + 3.0 * CARD_HEIGHT
            + CARD_GAP
            + ROW_GAP
            + FOOTER
            + SLACK
    );
    // Never narrower than the toolbar needs.
    let size = panel_size(
        &rows[1..],
        &widths,
        false,
        true,
        700.0,
        vec2(5000.0, 5000.0),
    );
    assert_eq!(size.x, 700.0);
}

#[test]
fn a_card_that_wants_attention_is_tinted_and_outlined_in_orange() {
    for tokens in [Tokens::dark(), Tokens::light()] {
        let plain = card_look(&tokens, None, false);
        assert_eq!(
            plain,
            Look {
                fill: None,
                outline: None
            }
        );
        let lit = card_look(&tokens, Some(tokens.hover_bg), false);
        assert_eq!(lit.fill, Some(tokens.hover_bg));
        assert_eq!(lit.outline, None);

        let marked = card_look(&tokens, None, true);
        assert_eq!(marked.outline, Some(tokens.warning));
        let fill = marked.fill.expect("a tint");
        // The theme's orange, see-through.
        assert!(fill.a() > 0 && fill.a() < 255);
        let [r, g, b, _] = fill.to_srgba_unmultiplied();
        let near = |a: u8, b: u8| a.abs_diff(b) <= 4;
        assert!(near(r, tokens.warning.r()));
        assert!(near(g, tokens.warning.g()));
        assert!(near(b, tokens.warning.b()));
        // Stronger under the mouse or the keys.
        let marked_lit = card_look(&tokens, Some(tokens.hover_bg), true);
        assert!(marked_lit.fill.expect("a tint").a() > fill.a());
        assert_eq!(marked_lit.outline, Some(tokens.warning));
    }
}

#[test]
fn the_flat_view_shows_only_this_desktop_until_the_others_are_shown() {
    let desktops = [desktop("a", "Home", true), desktop("b", "Work", false)];
    let group = group(&["a", "b", "b"]);
    assert_eq!(shown_rows(&group, &desktops, false, &[true; 3]).len(), 1);
    assert_eq!(shown_rows(&group, &desktops, true, &[true; 3]).len(), 2);
    let limit = vec2(5000.0, 5000.0);
    let shut = panel_size(
        &shown_rows(&group, &desktops, false, &[true; 3]),
        &SAME,
        true,
        true,
        0.0,
        limit,
    );
    let open = panel_size(
        &shown_rows(&group, &desktops, true, &[true; 3]),
        &SAME,
        true,
        true,
        0.0,
        limit,
    );
    assert!(shut.y < open.y);
    // The footnote about other desktops needs a second row.
    let alone = panel_size(
        &shown_rows(&group, &desktops, false, &[true; 3]),
        &SAME,
        false,
        true,
        0.0,
        limit,
    );
    assert!((shut.y - alone.y - others::LINK_HEIGHT).abs() < 0.01);
    // Without a plugin that saves the order there is no note to make
    // room for, and the rows alone set the height.
    let quiet = panel_size(
        &shown_rows(&group, &desktops, true, &[true; 3]),
        &SAME,
        true,
        false,
        0.0,
        limit,
    );
    assert!((open.y - quiet.y - FOOTER).abs() < 0.01);
    assert!(shut.y < quiet.y);
}

fn two_rows() -> Vec<Row> {
    vec![
        Row {
            title: "This desktop".to_string(),
            desktop: "work".to_string(),
            members: vec![0, 1, 2],
        },
        Row {
            title: "Reading".to_string(),
            desktop: "read".to_string(),
            members: vec![3],
        },
    ]
}

#[test]
fn a_drag_shows_a_gap_where_the_window_will_land() {
    let rows = two_rows();
    let layout = preview(&rows, Some(0), Some((0, 2)));
    assert_eq!(layout[0], [Slot::Window(1), Slot::Window(2), Slot::Gap]);
    assert_eq!(layout[1], [Slot::Window(3)]);
    let untouched = preview(&rows, None, None);
    assert_eq!(
        untouched[0],
        [Slot::Window(0), Slot::Window(1), Slot::Window(2)]
    );
}

#[test]
fn a_drop_gives_the_whole_new_order_row_by_row() {
    let rows = two_rows();
    assert_eq!(dropped_order(&rows, 0, (0, 2)), [1, 2, 0, 3]);
    assert_eq!(dropped_order(&rows, 2, (0, 0)), [2, 0, 1, 3]);
    assert_eq!(dropped_order(&rows, 1, (1, 0)), [0, 2, 1, 3]);
    assert_eq!(dropped_order(&rows, 3, (0, 1)), [0, 3, 1, 2]);
}

#[test]
fn a_drop_on_another_row_moves_the_window_to_that_desktop_first() {
    let rows = two_rows();
    let group = group(&["work", "work", "work", "read"]);
    let actions = drop_actions(&group, &rows, 1, (1, 1));
    assert_eq!(
        actions[0],
        ArrangeAction::MoveToDesktop {
            hwnd: 1,
            desktop: "read".to_string()
        }
    );
    assert_eq!(
        actions[1],
        ArrangeAction::Reorder {
            group: "chrome.exe".to_string(),
            order: vec![0, 2, 3, 1]
        }
    );
    assert!(drop_actions(&group, &rows, 0, (0, 0)).is_empty());
}

#[test]
fn the_drop_target_comes_from_last_frames_layout() {
    let row_rects = [
        Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 200.0)),
        Rect::from_min_max(pos2(0.0, 216.0), pos2(800.0, 416.0)),
    ];
    let card = |x: f32, y: f32| Rect::from_min_size(pos2(x, y), CARD);
    let card_rects = vec![
        vec![card(0.0, 20.0), card(228.0, 20.0)],
        vec![card(0.0, 236.0)],
    ];
    assert_eq!(
        drop_target(pos2(300.0, 100.0), &row_rects, &card_rects),
        Some((0, 1))
    );
    assert_eq!(
        drop_target(pos2(500.0, 100.0), &row_rects, &card_rects),
        Some((0, 2))
    );
    assert_eq!(
        drop_target(pos2(10.0, 300.0), &row_rects, &card_rects),
        Some((1, 0))
    );
    assert_eq!(
        drop_target(pos2(10.0, 900.0), &row_rects, &card_rects),
        Some((1, 1))
    );

    // Cards of other widths: the middle of each real card counts.
    let mixed = vec![vec![
        Rect::from_min_size(pos2(0.0, 20.0), vec2(60.0, CARD_HEIGHT)),
        Rect::from_min_size(pos2(68.0, 20.0), vec2(300.0, CARD_HEIGHT)),
    ]];
    let one = [Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 200.0))];
    assert_eq!(drop_target(pos2(40.0, 60.0), &one, &mixed), Some((0, 1)));
    assert_eq!(drop_target(pos2(200.0, 60.0), &one, &mixed), Some((0, 1)));
    assert_eq!(drop_target(pos2(230.0, 60.0), &one, &mixed), Some((0, 2)));

    // A wrapped row: two cards on the first line, one on the second.
    let wrapped = vec![vec![card(0.0, 20.0), card(228.0, 20.0), card(0.0, 200.0)]];
    let tall = [Rect::from_min_max(pos2(0.0, 0.0), pos2(800.0, 400.0))];
    assert_eq!(
        drop_target(pos2(300.0, 250.0), &tall, &wrapped),
        Some((0, 3))
    );
    assert_eq!(
        drop_target(pos2(10.0, 250.0), &tall, &wrapped),
        Some((0, 2))
    );
    assert_eq!(drop_target(pos2(10.0, 60.0), &tall, &wrapped), Some((0, 0)));
}

fn mixed_desktops() -> (Vec<ArrangeDesktop>, ArrangeGroup) {
    (
        vec![
            desktop("home", "Home", false),
            desktop("work", "Work", true),
        ],
        group(&["work", "home", "work", "work", "home", "work"]),
    )
}

fn reorder(action: Option<ArrangeAction>) -> Option<Vec<isize>> {
    match action {
        Some(ArrangeAction::Reorder { order, .. }) => Some(order),
        Some(other) => panic!("only a Reorder changes a number, got {other:?}"),
        None => None,
    }
}

#[test]
fn a_window_takes_a_place_in_its_own_desktops_row_only() {
    let (desktops, group) = mixed_desktops();
    // This desktop is windows 0, 2, 3 and 5; window 5 becomes number 2.
    assert_eq!(
        order_with_place(&group, &desktops, 5, 2),
        Some(vec![0, 1, 5, 2, 4, 3])
    );
    // The other desktop's windows (1 and 4) keep their slots and order.
    assert_eq!(
        order_with_place(&group, &desktops, 4, 1),
        Some(vec![0, 4, 2, 3, 1, 5])
    );
    // Number 1 and the last number, and a place past the end.
    assert_eq!(
        order_with_place(&group, &desktops, 3, 1),
        Some(vec![3, 1, 0, 2, 4, 5])
    );
    assert_eq!(
        order_with_place(&group, &desktops, 0, 4),
        Some(vec![2, 1, 3, 5, 4, 0])
    );
    assert_eq!(
        order_with_place(&group, &desktops, 0, 99),
        order_with_place(&group, &desktops, 0, 4)
    );
    // Its own number changes nothing.
    assert_eq!(order_with_place(&group, &desktops, 3, 3), None);
    assert_eq!(order_with_place(&group, &desktops, 9, 1), None);
}

#[test]
fn a_new_number_is_one_reorder_and_never_a_move() {
    let (desktops, group) = mixed_desktops();
    assert_eq!(
        take_place(&group, &desktops, 5, 2),
        Some(ArrangeAction::Reorder {
            group: "chrome.exe".to_string(),
            order: vec![0, 1, 5, 2, 4, 3]
        })
    );
    assert_eq!(take_place(&group, &desktops, 5, 4), None);
}

#[test]
fn a_window_knows_its_number_and_how_many_share_the_row() {
    let (desktops, group) = mixed_desktops();
    assert_eq!(place_in_row(&group, &desktops, 3), Some((3, 4)));
    assert_eq!(place_in_row(&group, &desktops, 4), Some((2, 2)));
    assert_eq!(place_in_row(&group, &desktops, 7), None);
}

#[test]
fn alt_arrows_step_one_number_and_stop_at_the_ends() {
    let (desktops, group) = mixed_desktops();
    // Window 2 is number 2: Alt+Left makes it 1, Alt+Right makes it 3.
    assert_eq!(
        reorder(nudge(&group, &desktops, 2, -1)),
        Some(vec![2, 1, 0, 3, 4, 5])
    );
    assert_eq!(
        reorder(nudge(&group, &desktops, 2, 1)),
        Some(vec![0, 1, 3, 2, 4, 5])
    );
    assert_eq!(nudge(&group, &desktops, 0, -1), None, "already number 1");
    assert_eq!(nudge(&group, &desktops, 5, 1), None, "already the last");
    // On another desktop, in its own row.
    assert_eq!(
        reorder(nudge(&group, &desktops, 4, -1)),
        Some(vec![0, 4, 2, 3, 1, 5])
    );
    assert_eq!(nudge(&group, &desktops, 7, 1), None);
}

#[test]
fn the_menu_lists_every_place_and_marks_the_current_one() {
    assert_eq!(place_labels(4, 2), ["1", "2 (now)", "3", "4"]);
    assert_eq!(place_labels(2, 2), ["1", "2 (now)"]);
    assert!(place_labels(0, 1).is_empty());
}

#[test]
fn a_ctrl_drop_lands_on_the_card_under_the_pointer() {
    let card = |x: f32| Rect::from_min_size(pos2(x, 20.0), CARD);
    let cards = [(7, card(0.0)), (8, card(160.0)), (9, card(320.0))];
    assert_eq!(ctrl_target(pos2(200.0, 60.0), 7, &cards), Some(8));
    // The gap between two cards belongs to a card.
    assert_eq!(ctrl_target(pos2(154.0, 60.0), 9, &cards), Some(7));
    // Never the dragged window's own card, nor empty space.
    assert_eq!(ctrl_target(pos2(60.0, 60.0), 7, &cards), None);
    assert_eq!(ctrl_target(pos2(600.0, 60.0), 7, &cards), None);
    assert_eq!(ctrl_target(pos2(60.0, 500.0), 8, &cards), None);
}

#[test]
fn a_ctrl_drop_gives_the_dragged_window_the_targets_number() {
    let (desktops, group) = mixed_desktops();
    // Window 5 (number 4) dropped on window 2 (number 2) becomes number 2.
    assert_eq!(ctrl_place(&group, &desktops, 5, 2), Some(2));
    assert_eq!(
        reorder(ctrl_drop(&group, &desktops, 5, 2)),
        Some(vec![0, 1, 5, 2, 4, 3])
    );
    // Dropped on a card further along, it takes that card's number.
    assert_eq!(ctrl_place(&group, &desktops, 0, 5), Some(4));
    assert_eq!(
        reorder(ctrl_drop(&group, &desktops, 0, 5)),
        Some(vec![2, 1, 3, 5, 4, 0])
    );
}

#[test]
fn a_ctrl_drop_on_nothing_useful_changes_nothing() {
    let (desktops, group) = mixed_desktops();
    // A card of the other desktop has a number of that desktop's row.
    assert_eq!(ctrl_place(&group, &desktops, 5, 1), None);
    assert_eq!(ctrl_drop(&group, &desktops, 5, 1), None);
    // Itself, an unknown window, and the card right before it.
    assert_eq!(ctrl_place(&group, &desktops, 5, 5), None);
    assert_eq!(ctrl_place(&group, &desktops, 5, 42), None);
    assert_eq!(ctrl_drop(&group, &desktops, 3, 3), None);
    assert_eq!(
        reorder(ctrl_drop(&group, &desktops, 3, 2)),
        Some(vec![0, 1, 3, 2, 4, 5])
    );
}

#[test]
fn ctrl_w_picks_the_next_card_else_the_one_before() {
    assert_eq!(after_close(&[1, 2, 3], 2), Some(3), "a middle card");
    assert_eq!(after_close(&[1, 2, 3], 3), Some(2), "the last card");
    assert_eq!(after_close(&[1, 2, 3], 1), Some(2), "the first card");
    assert_eq!(after_close(&[7], 7), None, "the only card");
    assert_eq!(after_close(&[1, 2], 9), None, "not on screen");
}

#[test]
fn ctrl_w_closes_the_picked_window_and_picks_its_neighbour() {
    let none = |_: isize| false;
    assert_eq!(ctrl_w_plan(&[1, 2, 3], Some(2), &none), Some((2, Some(3))));
    assert_eq!(ctrl_w_plan(&[1, 2, 3], Some(3), &none), Some((3, Some(2))));
    assert_eq!(ctrl_w_plan(&[4], Some(4), &none), Some((4, None)));
    assert_eq!(ctrl_w_plan(&[1, 2], None, &none), None, "nothing picked");
    // A ghost's window has a card too, so it appears twice.
    assert_eq!(ctrl_w_plan(&[1, 2, 1], Some(1), &none), Some((1, Some(2))));
    // Cards whose windows are already going are skipped.
    let going = |hwnd: isize| hwnd == 3;
    assert_eq!(ctrl_w_plan(&[1, 2, 3], Some(2), &going), Some((2, Some(1))));
    assert_eq!(ctrl_w_plan(&[1, 3], Some(3), &going), None);
}

#[test]
fn the_ctrl_hint_names_the_number_or_asks_for_a_card() {
    assert_eq!(ctrl_hint(Some(2)), "Change number only: takes number 2");
    assert!(ctrl_hint(None).starts_with("Change number only"));
}

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

/// The primary monitor, one to its left and one to its right.
fn three_screens() -> Vec<ArrangeMonitor> {
    vec![
        screen([0, 0, 3840, 2160], true),
        screen([-1920, 0, 0, 1080], false),
        screen([3840, 0, 6400, 2880], false),
    ]
}

const RIGHT_BUTTON: [i32; 4] = [5000, 2830, 5048, 2880];
const MAIN_BUTTON: [i32; 4] = [1900, 2100, 1948, 2160];

#[test]
fn a_secondary_taskbar_limits_the_strip_to_its_monitor_in_mode_1() {
    let screens = three_screens();
    let mode = |enabled, mode| MultiMonitor::from_values(Some(enabled), Some(mode));
    assert_eq!(
        only_monitor(Some(RIGHT_BUTTON), &screens, mode(1, 1)),
        Some(2)
    );
    assert_eq!(only_monitor(Some(MAIN_BUTTON), &screens, mode(1, 1)), None);
}

#[test]
fn every_taskbar_limits_the_strip_in_mode_2_and_none_in_mode_0() {
    let screens = three_screens();
    let mode = |enabled, mode| MultiMonitor::from_values(Some(enabled), Some(mode));
    assert_eq!(
        only_monitor(Some(MAIN_BUTTON), &screens, mode(1, 2)),
        Some(0)
    );
    assert_eq!(
        only_monitor(Some(RIGHT_BUTTON), &screens, mode(1, 2)),
        Some(2)
    );
    assert_eq!(only_monitor(Some(RIGHT_BUTTON), &screens, mode(1, 0)), None);
    assert_eq!(only_monitor(Some(MAIN_BUTTON), &screens, mode(1, 0)), None);
}

#[test]
fn one_taskbar_a_hotkey_strip_or_an_unknown_place_limit_nothing() {
    let screens = three_screens();
    let mode_2 = MultiMonitor::from_values(Some(1), Some(2));
    let one_taskbar = MultiMonitor::from_values(Some(0), Some(2));
    assert_eq!(
        only_monitor(Some(RIGHT_BUTTON), &screens, one_taskbar),
        None
    );
    assert_eq!(only_monitor(None, &screens, mode_2), None, "Win+Alt+A");
    assert_eq!(
        only_monitor(Some([9000, 0, 9040, 40]), &screens, mode_2),
        None
    );
    let mut gone = three_screens();
    gone[2].connected = false;
    assert_eq!(only_monitor(Some(RIGHT_BUTTON), &gone, mode_2), None);
}

fn on_screens(places: &[Option<usize>]) -> ArrangeGroup {
    let mut group = group(&vec!["work"; places.len()]);
    for (window, place) in group.windows.iter_mut().zip(places) {
        window.on_monitor = *place;
        window.saved_monitor = *place;
    }
    group
}

#[test]
fn a_limited_strip_shows_the_windows_of_its_monitor() {
    let screens = three_screens();
    // Windows 1 and 3 are on the right monitor; 4 has no known place and
    // counts as on the main one.
    let group = on_screens(&[Some(0), Some(2), Some(1), Some(2), None]);
    assert_eq!(
        visible_flags(&group, &screens, Some(2)),
        [false, true, false, true, false]
    );
    assert_eq!(
        visible_flags(&group, &screens, Some(0)),
        [true, false, false, false, true]
    );
    assert_eq!(visible_flags(&group, &screens, None), [true; 5]);
}

#[test]
fn the_shown_windows_keep_their_own_numbers() {
    let desktops = [desktop("work", "Work", true)];
    let screens = three_screens();
    let group = on_screens(&[Some(0), Some(2), Some(0), Some(2), Some(0), Some(2)]);
    let visible = visible_flags(&group, &screens, Some(2));
    let rows = shown_rows(&group, &desktops, false, &visible);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].members, [1, 3, 5]);
    // Numbers come from the whole row, so they skip: 2, 4 and 6.
    let numbers = taskbar_numbers(&group, &desktops);
    let shown: Vec<usize> = rows[0].members.iter().map(|m| numbers[*m]).collect();
    assert_eq!(shown, [2, 4, 6]);
}

#[test]
fn another_desktops_row_goes_when_none_of_its_windows_are_shown() {
    let desktops = [
        desktop("home", "Home", false),
        desktop("work", "Work", true),
    ];
    let group = group(&["work", "home", "work", "home"]);
    let visible = [true, false, true, false];
    assert_eq!(shown_rows(&group, &desktops, true, &visible).len(), 1);
    assert_eq!(shown_rows(&group, &desktops, true, &[true; 4]).len(), 2);
}

#[test]
fn reordering_shown_windows_keeps_the_others_where_they_were() {
    assert_eq!(
        merge_order(&[0, 1, 2, 3, 4, 5], &[4, 2]),
        [0, 1, 4, 3, 2, 5]
    );
    assert_eq!(merge_order(&[0, 1, 2, 3], &[0, 1, 2, 3]), [0, 1, 2, 3]);
    // A handle that is not in the group is ignored.
    assert_eq!(merge_order(&[0, 1, 2], &[2, 9, 0]), [2, 1, 0]);
    assert_eq!(merge_order(&[0, 1, 2], &[]), [0, 1, 2]);
}

#[test]
fn a_drop_in_a_limited_strip_reorders_the_whole_group() {
    let desktops = [desktop("work", "Work", true)];
    let screens = three_screens();
    let group = on_screens(&[Some(0), Some(2), Some(0), Some(2), Some(0), Some(2)]);
    let visible = visible_flags(&group, &screens, Some(0));
    let rows = shown_rows(&group, &desktops, false, &visible);
    assert_eq!(rows[0].members, [0, 2, 4]);
    // Window 4 dropped first: the right-monitor windows 1, 3, 5 stay.
    let actions = drop_actions(&group, &rows, 4, (0, 0));
    assert_eq!(
        actions,
        [ArrangeAction::Reorder {
            group: "chrome.exe".to_string(),
            order: vec![4, 1, 0, 3, 2, 5]
        }]
    );
    assert!(drop_actions(&group, &rows, 0, (0, 0)).is_empty());
}

#[test]
fn a_card_motion_eases_out_over_150_ms() {
    let ms = Duration::from_millis;
    assert_eq!(motion_progress(ms(0)), 0.0);
    assert_eq!(motion_progress(ms(150)), 1.0);
    assert_eq!(motion_progress(ms(900)), 1.0);
    // Most of the way after half the time: it slows down at the end.
    assert!(motion_progress(ms(75)) > 0.8);
}

#[test]
fn a_new_card_grows_and_fades_in_and_a_gone_one_fades_out() {
    let ms = Duration::from_millis;
    assert_eq!(card_motion(None, None), (1.0, 1.0));
    assert_eq!(card_motion(Some(ms(0)), None), (0.9, 0.0));
    assert_eq!(card_motion(Some(ms(150)), None), (1.0, 1.0));
    let (scale, fade) = card_motion(Some(ms(75)), None);
    assert!(scale > 0.9 && scale < 1.0 && fade > 0.0 && fade < 1.0);
    assert_eq!(card_motion(None, Some(ms(0))), (1.0, 1.0));
    assert_eq!(card_motion(None, Some(ms(150))), (1.0, 0.0));
    let grown_rect = grown(Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 50.0)), 0.9);
    assert_eq!(grown_rect.size(), vec2(90.0, 45.0));
    assert_eq!(grown_rect.center(), pos2(50.0, 25.0));
}

#[test]
fn up_and_down_pick_the_nearest_card_on_the_line_above_or_below() {
    let card = |x: f32, y: f32| Rect::from_min_size(pos2(x, y), vec2(150.0, 100.0));
    // Two lines in one column, and a card further down in the next.
    let cards = [
        (1, card(0.0, 0.0)),
        (2, card(160.0, 0.0)),
        (3, card(0.0, 110.0)),
        (4, card(400.0, 0.0)),
        (5, card(400.0, 300.0)),
    ];
    assert_eq!(card_above_or_below(&cards, 2, true), Some(3));
    assert_eq!(card_above_or_below(&cards, 3, false), Some(1));
    // From 3 down: the next line below, in the other column.
    assert_eq!(card_above_or_below(&cards, 3, true), Some(5));
    assert_eq!(card_above_or_below(&cards, 5, false), Some(3));
    assert_eq!(card_above_or_below(&cards, 1, false), None);
    assert_eq!(card_above_or_below(&cards, 5, true), None);
    assert_eq!(card_above_or_below(&cards, 9, true), None);
}

#[test]
fn a_middle_click_closes_a_cards_window_but_not_a_ghosts() {
    assert_eq!(middle_close(7, false), Some(7));
    assert_eq!(middle_close(7, true), None);
}

#[test]
fn new_windows_grow_in_and_gone_ones_stay_until_they_fade() {
    let now = Instant::now();
    let before = group(&["d1", "d1", "d1"]);
    let mut known: HashSet<isize> = before.windows.iter().map(|w| w.hwnd).collect();
    let mut appeared = HashMap::new();
    let mut leaving = Vec::new();
    // Window 1 closed, window 9 is new.
    let mut after = before.clone();
    after.windows.remove(1);
    let mut new = after.windows[0].clone();
    new.hwnd = 9;
    after.windows.push(new);
    let mut groups = vec![after];
    note_changes(
        &mut known,
        &mut appeared,
        &mut leaving,
        Some(&before),
        &groups,
        now,
    );
    assert_eq!(appeared.keys().copied().collect::<Vec<_>>(), [9]);
    assert_eq!(leaving.len(), 1);
    assert_eq!(leaving[0].window.hwnd, 1);
    keep_leaving(&leaving, &mut groups);
    let order: Vec<isize> = groups[0].windows.iter().map(|w| w.hwnd).collect();
    assert_eq!(order, [0, 1, 2, 9], "back in its old place while it fades");
    // Seen again, it is not new and not counted as gone a second time.
    let shown = groups[0].clone();
    note_changes(
        &mut known,
        &mut appeared,
        &mut leaving,
        Some(&shown),
        &groups,
        now,
    );
    assert_eq!(appeared.len(), 1);
    assert_eq!(leaving.len(), 1);
    // A window seen before that comes back does not grow in again.
    let mut back = groups[0].clone();
    back.windows.retain(|w| w.hwnd != 9);
    let mut none = HashMap::new();
    note_changes(&mut known, &mut none, &mut Vec::new(), None, &[back], now);
    assert!(none.is_empty());
}

fn thumb(width: f32) -> Rect {
    Rect::from_min_size(pos2(100.0, 50.0), vec2(width, 81.0))
}

#[test]
fn badges_are_text_when_they_all_fit_in_the_picture_less_twelve() {
    // 40 + 12 and 60 + 12 and a gap of 4 is 128.
    let (icons, rects) = badge_rects(thumb(140.0), &[40.0, 60.0]);
    assert!(!icons);
    assert_eq!(rects[0].width(), 52.0);
    assert_eq!(rects[1].width(), 72.0);
    assert_eq!(rects[1].left() - rects[0].right(), BADGE_GAP);
    assert_eq!(rects[1].right(), thumb(140.0).right() - BADGE_INSET);
    assert_eq!(rects[0].top(), thumb(140.0).top() + BADGE_INSET);
    assert_eq!(rects[0].height(), BADGE_HEIGHT);
}

#[test]
fn every_badge_becomes_an_icon_when_one_more_pixel_would_not_fit() {
    assert!(!badge_rects(thumb(140.0), &[40.0, 60.0]).0);
    let (icons, rects) = badge_rects(thumb(139.0), &[40.0, 60.0]);
    assert!(icons);
    assert!(rects.iter().all(|rect| rect.size() == Vec2::splat(18.0)));
    let (icons, _) = badge_rects(thumb(139.0), &[40.0]);
    assert!(!icons);
}

#[test]
fn badges_stay_inside_the_card_whatever_its_width() {
    for width in [60.0, 90.0, 140.0, 228.0, 400.0] {
        let card = Rect::from_min_size(pos2(96.0, 46.0), vec2(width + 8.0, 114.0));
        let area = Rect::from_min_size(card.min + Vec2::splat(4.0), vec2(width, 81.0));
        for widths in [vec![], vec![50.0], vec![50.0, 70.0]] {
            let (_, rects) = badge_rects(area, &widths);
            for rect in rects {
                assert!(card.contains_rect(rect), "{rect:?} in {card:?}");
                assert!(area.contains_rect(rect));
            }
        }
    }
}

#[test]
fn the_title_row_keeps_no_room_for_badges() {
    // The row's right edge is the picture's: a badge is on the picture.
    let (_, rects) = badge_rects(thumb(200.0), &[50.0, 70.0]);
    let picture = thumb(200.0);
    assert!(rects.iter().all(|rect| rect.bottom() <= picture.bottom()));
    assert!(rects.iter().all(|rect| rect.top() >= picture.top()));
}
