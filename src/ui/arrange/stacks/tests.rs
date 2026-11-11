use super::fold::{expand_action, expand_order, fold, unfold};
use super::*;

fn window(hwnd: isize, label: &str, monitor: Option<usize>) -> ArrangeWindow {
    ArrangeWindow {
        hwnd,
        label: label.to_string(),
        desktop: String::new(),
        monitor: monitor.map(|m| format!("M{m}")),
        on_monitor: monitor,
        saved_monitor: monitor,
        renamed: false,
        same_title: false,
        attention: false,
    }
}

/// Chrome with Inbox (1), NewsHQ (2), Docs (3), NewsHQ - My Tasks (4),
/// NewsHQ - design (5), in this taskbar order.
fn chrome() -> ArrangeGroup {
    ArrangeGroup {
        key: "chrome".to_string(),
        label: "Chrome".to_string(),
        windows: vec![
            window(1, "Inbox", None),
            window(2, "NewsHQ", None),
            window(3, "Docs", None),
            window(4, "NewsHQ - My Tasks", Some(1)),
            window(5, "NewsHQ - design", Some(1)),
        ],
    }
}

fn news() -> Vec<DeckGroup> {
    vec![DeckGroup {
        app: "chrome".to_string(),
        name: "NewsHQ".to_string(),
        members: vec![
            "NewsHQ".to_string(),
            "NewsHQ - My Tasks".to_string(),
            "NewsHQ - design".to_string(),
        ],
    }]
}

fn hwnds(group: &ArrangeGroup) -> Vec<isize> {
    group.windows.iter().map(|w| w.hwnd).collect()
}

/// Window 4 was used last, then the rest in handle order.
fn recent(hwnd: isize) -> usize {
    if hwnd == 4 {
        0
    } else {
        hwnd as usize + 10
    }
}

#[test]
fn a_closed_group_is_one_card_at_the_place_of_its_first_window() {
    let mut group = chrome();
    group.windows[2 + 2].attention = true;
    let found = fold(&mut group, &news(), &recent, None);
    assert_eq!(hwnds(&group), [1, 4, 3], "the card is the window used last");
    let card = &group.windows[1];
    assert_eq!(card.label, "NewsHQ");
    assert!(
        card.attention,
        "any window wanting attention marks the stack"
    );
    // Two of its three windows are on monitor 1.
    assert_eq!(card.on_monitor, Some(1));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].kind, Kind::Closed);
    assert_eq!(found[0].rep, 4);
    assert_eq!(found[0].hwnds(), [2, 4, 5]);
    // The numbers after the stack move up: Docs is 3, not 5.
    let numbers = taskbar_numbers(&group, &[]);
    assert_eq!(numbers, [1, 2, 3]);
    unfold(&mut group, &found);
    assert_eq!(hwnds(&group), [1, 2, 4, 5, 3]);
}

#[test]
fn an_open_group_lies_together_and_a_lone_window_keeps_its_card() {
    let mut group = chrome();
    let found = fold(&mut group, &news(), &recent, Some("NewsHQ"));
    assert_eq!(hwnds(&group), [1, 2, 4, 5, 3]);
    assert_eq!(found[0].kind, Kind::Open);

    let mut group = chrome();
    group.windows.retain(|w| w.hwnd != 4 && w.hwnd != 5);
    let found = fold(&mut group, &news(), &recent, None);
    assert_eq!(hwnds(&group), [1, 2, 3]);
    assert_eq!(found[0].kind, Kind::Lone);

    let mut group = chrome();
    group.windows.retain(|w| ![2, 4, 5].contains(&w.hwnd));
    assert!(fold(&mut group, &news(), &recent, None).is_empty());
}

#[test]
fn a_tie_of_monitors_goes_to_the_window_used_last() {
    let mut group = chrome();
    group.windows[3].on_monitor = Some(2);
    group.windows[3].monitor = Some("M2".into());
    fold(&mut group, &news(), &recent, None);
    assert_eq!(group.windows[1].on_monitor, Some(2));
    assert_eq!(group.windows[1].monitor.as_deref(), Some("M2"));
}

#[test]
fn another_app_and_a_group_named_like_a_window_are_left_alone() {
    let mut defs = news();
    defs.push(DeckGroup {
        app: "chrome".into(),
        name: "Pair".into(),
        // "NewsHQ" is the first group's card now, not a window.
        members: vec!["NewsHQ".into(), "Docs".into()],
    });
    let mut group = chrome();
    let found = fold(&mut group, &defs, &recent, None);
    assert_eq!(found[1].kind, Kind::Lone);
    let mut other = chrome();
    other.key = "edge".into();
    assert!(fold(&mut other, &defs, &recent, None).is_empty());
}

#[test]
fn moves_and_orders_of_a_stack_take_all_its_windows() {
    let mut group = chrome();
    let found = fold(&mut group, &news(), &recent, None);
    assert_eq!(expand_order(&[3, 4, 1], &found), [3, 2, 4, 5, 1]);
    let moved = expand_action(
        ArrangeAction::MoveToMonitor {
            hwnd: 4,
            monitor: "D2".into(),
        },
        &found,
    );
    assert_eq!(moved.len(), 3);
    let one = expand_action(ArrangeAction::Close(1), &found);
    assert_eq!(one, [ArrangeAction::Close(1)]);
}

fn snapshot() -> ArrangeSnapshot {
    ArrangeSnapshot {
        plugin: "window_deck".into(),
        groups: vec![chrome()],
        desktops: Vec::new(),
        focus: 0,
        watched: Vec::new(),
        preview: false,
        can_rename: false,
        monitors: Vec::new(),
        view: ArrangeView::ByMonitor,
        look_plugin: "window_deck".into(),
        hints: true,
        toolbar_on_other_monitors: false,
        deck_groups: Some(news()),
        keeps_order: false,
        anchor: None,
    }
}

#[test]
fn an_open_group_numbers_its_windows_from_one_and_dims_the_rest() {
    let mut stacks = Stacks::default();
    let mut data = snapshot();
    stacks.arrive(&mut data, true);
    let rep = stacks.found[0].rep;
    assert!(stacks.closed(rep).is_some());
    assert_eq!(stacks.digit(&data.groups[0], 1), None, "no group open");
    assert!(!stacks.wants_escape());

    stacks.open = Some(("chrome".into(), "NewsHQ".into()));
    stacks.refold(&mut data);
    let group = &data.groups[0];
    assert_eq!(hwnds(group), [1, 2, 4, 5, 3]);
    let numbers = stacks.numbers(group, taskbar_numbers(group, &data.desktops));
    assert_eq!(numbers, [1, 1, 2, 3, 5], "Inbox and Docs keep theirs");
    assert_eq!(stacks.digit(group, 2), Some(Some(4)));
    assert_eq!(stacks.digit(group, 4), Some(None));
    assert_eq!(stacks.dim(1), DIMMED);
    assert_eq!(stacks.dim(5), 1.0);
    assert!(stacks.wants_escape());
    assert_eq!(stacks.hints(&data), GroupHints::Open);

    // New data keeps the group open while its windows are there.
    let mut fresh = snapshot();
    stacks.arrive(&mut fresh, false);
    assert!(stacks.open_stack().is_some());
    // A strip that opens again starts closed.
    stacks.arrive(&mut fresh, true);
    assert!(stacks.open_stack().is_none());
}

#[test]
fn with_grouping_off_nothing_folds() {
    let mut stacks = Stacks::default();
    let mut data = snapshot();
    data.deck_groups = None;
    stacks.arrive(&mut data, true);
    assert_eq!(hwnds(&data.groups[0]), [1, 2, 3, 4, 5]);
    assert!(stacks.found.is_empty());
    assert_eq!(stacks.hints(&data), GroupHints::Off);
}

#[test]
fn the_footer_names_the_group_keys() {
    let base = vec![
        ("\u{2190}\u{2192}", "Select"),
        ("Enter", "Open"),
        ("1\u{2013}9", "Open by number"),
        ("Ctrl + W", "Close"),
    ];
    assert_eq!(with_group_hints(base.clone(), GroupHints::Off), base);
    let on = with_group_hints(base.clone(), GroupHints::On);
    assert_eq!(on[2], ("Ctrl + G", "Group or ungroup"));
    assert_eq!(on[3], ("1 \u{2013} 0", "Card or group"));
    let open = with_group_hints(base, GroupHints::Open);
    assert_eq!(open[3], ("Esc", "Close the group"));
    assert!(!open.iter().any(|(_, label)| *label == "Open by number"));
}

#[test]
fn a_stack_names_its_other_windows_until_they_do_not_fit() {
    let mut group = chrome();
    let found = fold(&mut group, &news(), &recent, None);
    let stack = &found[0];
    assert_eq!(names_line(stack, &|_| true), "NewsHQ \u{b7} design");
    assert_eq!(
        names_line(stack, &|text| text.len() < 16),
        "NewsHQ  +1 more"
    );
    assert_eq!(names_line(stack, &|_| false), "+2 more");
}

#[test]
fn an_open_groups_cards_frame_in_runs() {
    let card = |x: f32, y: f32| Rect::from_min_size(pos2(x, y), vec2(100.0, 50.0));
    let runs = paint::runs(&[
        card(108.0, 0.0),
        card(0.0, 0.0),
        card(0.0, 60.0),
        card(400.0, 0.0),
    ]);
    assert_eq!(runs.len(), 3);
    assert_eq!(
        runs[0],
        Rect::from_min_max(pos2(0.0, 0.0), pos2(208.0, 50.0))
    );
}

#[test]
fn a_drop_groups_only_over_the_middle_of_a_card() {
    let cards = [
        (1, Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0))),
        (2, Rect::from_min_size(pos2(108.0, 0.0), vec2(100.0, 100.0))),
    ];
    assert_eq!(middle_hit(pos2(158.0, 50.0), 1, &cards), Some(2));
    assert_eq!(
        middle_hit(pos2(104.0, 50.0), 1, &cards),
        None,
        "the gap moves"
    );
    assert_eq!(
        middle_hit(pos2(50.0, 50.0), 1, &cards),
        None,
        "not on itself"
    );
}

#[test]
fn the_close_question_names_wincraft_and_every_window() {
    let windows = vec![
        (2, "NewsHQ".to_string()),
        (4, "NewsHQ - My Tasks".to_string()),
        (5, "NewsHQ - design".to_string()),
    ];
    let (title, text) = dialogs::close_words("NewsHQ", &windows);
    assert_eq!(title, "Close 3 windows in NewsHQ?");
    assert_eq!(
        text,
        "WinCraft will close NewsHQ, NewsHQ - My Tasks and NewsHQ - design. The group stays, and the windows join it again when they open."
    );
    assert_eq!(dialogs::and_list(&["A".into()]), "A");
}

#[test]
fn the_flat_view_lists_a_stack_as_one_card_and_an_open_group_together() {
    let mut stacks = Stacks::default();
    let mut data = snapshot();
    stacks.arrive(&mut data, true);
    let rep = stacks.found[0].rep;
    let flat = rows(&data.groups[0], &data.desktops);
    assert_eq!(flat.len(), 1);
    assert_eq!(flat[0].members.len(), 3, "Inbox, the stack, Docs");
    // A stack's picture is shorter, so its names fit under its title.
    let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(144.0, 81.0));
    assert_eq!(
        flat_picture(&stacks, rep, area).height(),
        81.0 - paint::LINES
    );
    assert_eq!(flat_picture(&stacks, 1, area), area);

    stacks.open = Some(("chrome".into(), "NewsHQ".into()));
    stacks.refold(&mut data);
    let group = &data.groups[0];
    let flat = rows(group, &data.desktops);
    assert_eq!(flat[0].members, [0, 1, 2, 3, 4]);
    let numbers = stacks.numbers(group, taskbar_numbers(group, &data.desktops));
    assert_eq!(numbers, [1, 1, 2, 3, 5]);
    // The Flat view has no ghosts: every window keeps its own card.
    stacks.set_ghosted(vec![5], 0.0);
    assert_eq!(stacks.dim_card(5), DIMMED);
    stacks.ghosted.clear();
    assert_eq!(stacks.dim_card(5), 1.0);
    assert_eq!(stacks.dim_card(1), DIMMED);
    assert_eq!(
        stacks.open_members(group),
        OpenMembers {
            hwnds: vec![2, 4, 5],
            rep: Some(rep)
        }
    );
}

#[test]
fn a_cover_hides_pictures_also_while_a_card_is_dragged() {
    let picture = |x: f32| Pending {
        hwnd: 7,
        dest: Rect::from_min_size(pos2(x, 0.0), vec2(100.0, 60.0)),
        clip: Rect::EVERYTHING,
        opacity: 255,
        clear: Rect::NOTHING,
        ghost: false,
    };
    let tab = [Rect::from_min_size(pos2(0.0, -8.0), vec2(200.0, 10.0))];
    let dragged_card = Some(Rect::from_min_size(pos2(500.0, 0.0), vec2(100.0, 60.0)));
    let over = Over {
        floating: dragged_card,
        covers: &tab,
    };
    assert!(
        is_covered(&picture(0.0), &over, Some(3)),
        "under the header line"
    );
    assert!(!is_covered(&picture(250.0), &over, Some(3)));
    assert!(
        is_covered(&picture(520.0), &over, Some(3)),
        "under the dragged card"
    );
    assert!(
        !is_covered(&picture(520.0), &over, Some(7)),
        "its own picture rides along"
    );
}
