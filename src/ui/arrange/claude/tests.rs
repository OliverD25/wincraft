use super::*;

const MIN: u64 = 60_000;
const HOUR: u64 = 60 * MIN;
const DAY: u64 = 24 * HOUR;
const NOW: u64 = 100 * DAY;

fn card(status: Option<Status>, since: Option<u64>, idle_for: Option<u64>) -> Card {
    Card {
        status,
        since,
        last_activity: idle_for.map(|ms| NOW - ms),
        jobs: 0,
    }
}

fn waiting(for_ms: u64) -> Card {
    card(Some(Status::Waiting), Some(NOW - for_ms), Some(for_ms))
}

fn working(for_ms: u64) -> Card {
    card(Some(Status::Working), Some(NOW - for_ms), Some(0))
}

fn idle(for_ms: u64) -> Card {
    card(Some(Status::Idle), None, Some(for_ms))
}

#[test]
fn s_steps_waiting_working_idle_cold_then_none_and_round_again() {
    let mut lit = None;
    let mut seen = Vec::new();
    for _ in 0..6 {
        lit = Filter::next(lit);
        seen.push(lit);
    }
    assert_eq!(
        seen,
        [
            Some(Filter::Waiting),
            Some(Filter::Working),
            Some(Filter::Idle),
            Some(Filter::Cold),
            None,
            Some(Filter::Waiting),
        ]
    );
}

#[test]
fn counters_count_idle_with_cold_inside_it() {
    let cards = [
        waiting(4 * MIN),
        working(2 * MIN),
        idle(8 * MIN),
        idle(DAY),
        idle(5 * DAY),
        Card::default(),
    ];
    assert_eq!(counts(cards.iter(), NOW), [1, 1, 3, 2]);
}

#[test]
fn a_card_without_a_status_matches_no_counter() {
    let none = card(None, None, Some(5 * DAY));
    for filter in Filter::ALL {
        assert!(!filter.matches(&none, NOW));
    }
}

#[test]
fn the_chip_names_the_status_and_how_long() {
    assert_eq!(
        chip_text(&waiting(4 * MIN), NOW).as_deref(),
        Some("Waiting 4 min")
    );
    assert_eq!(
        chip_text(&working(40_000), NOW).as_deref(),
        Some("Working 40 s")
    );
    assert_eq!(chip_text(&idle(HOUR), NOW).as_deref(), Some("Idle"));
    assert_eq!(chip_text(&Card::default(), NOW), None);
}

#[test]
fn arrows_skip_cards_that_are_not_lit() {
    let order = [1, 2, 3, 4, 5];
    let lit = |hwnd: isize| hwnd == 2 || hwnd == 5;
    assert_eq!(step_lit(&order, &lit, Some(2), true), Some(5));
    assert_eq!(step_lit(&order, &lit, Some(5), false), Some(2));
    // From a card that is not lit, the next lit one that way.
    assert_eq!(step_lit(&order, &lit, Some(3), true), Some(5));
    assert_eq!(step_lit(&order, &lit, Some(3), false), Some(2));
    assert_eq!(step_lit(&order, &lit, None, true), Some(2));
    // At the end it stays, and with nothing lit nothing is picked.
    assert_eq!(step_lit(&order, &lit, Some(5), true), None);
    assert_eq!(step_lit(&order, &|_| false, Some(1), true), None);
}

#[test]
fn esc_closes_the_board_then_clears_the_light_then_lets_the_deck_close() {
    let mut deck = Deck {
        highlight: Some(Filter::Waiting),
        board: Some((0, 0)),
        ..Deck::default()
    };
    assert!(deck.takes_escape());
    deck.escape();
    assert_eq!(deck.board, None);
    assert_eq!(deck.highlight, Some(Filter::Waiting));
    assert!(deck.takes_escape());
    deck.escape();
    assert_eq!(deck.highlight, None);
    assert!(!deck.takes_escape());
}

#[test]
fn lighting_a_status_dims_the_others_and_never_moves_a_card() {
    let cards = [waiting(MIN), working(MIN), idle(MIN)];
    let order = [10, 20, 30];
    let marks: Vec<Mark> = cards
        .iter()
        .map(|card| Mark::new(card, NOW, Some(Filter::Waiting), false))
        .collect();
    assert_eq!(marks[0].card_alpha(), 1.0);
    assert_eq!(marks[1].card_alpha(), DIMMED);
    assert_eq!(marks[2].card_alpha(), DIMMED);
    // Lighting only reads the cards; the order they are drawn in is kept.
    let mut deck = Deck {
        order: order.to_vec(),
        ..Deck::default()
    };
    deck.highlight = Filter::next(deck.highlight);
    assert_eq!(deck.order, order);
    let unlit = Mark::new(&cards[1], NOW, None, false);
    assert_eq!(unlit.card_alpha(), 1.0);
}

#[test]
fn pictures_fade_with_age_and_cold_ones_are_tinted() {
    let mark = |idle_for| Mark::new(&idle(idle_for), NOW, None, false);
    assert_eq!(mark(10 * MIN).picture_alpha(), 1.0);
    assert_eq!(mark(2 * HOUR).picture_alpha(), 0.85);
    assert_eq!(mark(DAY).picture_alpha(), 0.6);
    assert!(mark(DAY).tinted());
    assert_eq!(mark(3 * DAY).picture_alpha(), 0.4);
    assert!(!mark(3 * DAY).tinted());
    assert_eq!(mark(5 * DAY).age_text.as_deref(), Some("5 days"));
    assert_eq!(mark(30_000).age_text.as_deref(), Some("now"));
}

fn titled<'a>(hwnd: isize, title: &'a str, card: &'a Card) -> (isize, usize, &'a str, &'a Card) {
    (hwnd, hwnd as usize, title, card)
}

#[test]
fn board_columns_hold_their_cards_in_their_own_order() {
    let cards = [
        idle(8 * MIN),     // 1
        working(2 * MIN),  // 2
        waiting(4 * MIN),  // 3
        idle(3 * HOUR),    // 4
        working(11 * MIN), // 5
        idle(DAY),         // 6
        waiting(38 * MIN), // 7
        working(40_000),   // 8
        idle(5 * DAY),     // 9
        Card::default(),   // 10: no status, on no column
        idle(4 * DAY),     // 11
    ];
    let names: Vec<String> = (1..=cards.len()).map(|n| format!("Chat {n}")).collect();
    let windows: Vec<_> = cards
        .iter()
        .enumerate()
        .map(|(at, card)| titled(at as isize + 1, &names[at], card))
        .collect();
    let columns = columns(&windows, NOW);
    let numbers = |column: &Vec<BoardRow>| column.iter().map(|row| row.number).collect::<Vec<_>>();
    // Waiting for you: longest wait first.
    assert_eq!(numbers(&columns[0]), [7, 3]);
    assert_eq!(columns[0][0].time, "38 min");
    // Working: longest run first.
    assert_eq!(numbers(&columns[1]), [5, 2, 8]);
    assert_eq!(columns[1][2].time, "40 s");
    // Idle under 1 day: newest first.
    assert_eq!(numbers(&columns[2]), [1, 4]);
    assert_eq!(columns[2][1].time, "3 h");
    // Cold, 1 day or more: oldest first.
    assert_eq!(numbers(&columns[3]), [9, 11, 6]);
    assert_eq!(columns[3][0].title, "Chat 9");
}

#[test]
fn board_arrows_stay_in_bounds_and_skip_empty_columns() {
    let lens = [2, 0, 3, 1];
    assert_eq!(board_start(lens), (0, 0));
    assert_eq!(board_start([0, 0, 1, 0]), (2, 0));
    assert_eq!(board_move(lens, (0, 0), egui::Key::ArrowUp), (0, 0));
    assert_eq!(board_move(lens, (0, 0), egui::Key::ArrowDown), (0, 1));
    assert_eq!(board_move(lens, (0, 1), egui::Key::ArrowDown), (0, 1));
    // Right skips the empty Working column and keeps the row.
    assert_eq!(board_move(lens, (0, 1), egui::Key::ArrowRight), (2, 1));
    // The Cold column has one row, so the row comes up to it.
    assert_eq!(board_move(lens, (2, 2), egui::Key::ArrowRight), (3, 0));
    assert_eq!(board_move(lens, (3, 0), egui::Key::ArrowRight), (3, 0));
    assert_eq!(board_move(lens, (2, 0), egui::Key::ArrowLeft), (0, 0));
}

#[test]
fn the_lit_line_says_how_to_go_on() {
    assert_eq!(
        lit_line(Filter::Waiting),
        "Showing: Waiting \u{b7} S next \u{b7} Esc clears"
    );
}

fn background(for_ms: u64, jobs: usize) -> Card {
    Card {
        jobs,
        ..card(Some(Status::Background), Some(NOW - for_ms), Some(for_ms))
    }
}

#[test]
fn the_background_chip_names_the_time_and_the_jobs() {
    assert_eq!(
        chip_text(&background(3 * MIN, 1), NOW).as_deref(),
        Some("Background 3 min \u{b7} 1 job")
    );
    assert_eq!(
        chip_text(&background(3 * MIN, 2), NOW).as_deref(),
        Some("Background 3 min \u{b7} 2 jobs")
    );
}

#[test]
fn a_background_card_counts_as_working_never_idle_or_cold() {
    // Idle for 5 days by its last activity, yet a job runs.
    let card = background(5 * DAY, 1);
    assert!(Filter::Working.matches(&card, NOW));
    assert!(!Filter::Idle.matches(&card, NOW));
    assert!(!Filter::Cold.matches(&card, NOW));
    assert!(!Filter::Waiting.matches(&card, NOW));
    assert_eq!(counts([card].iter(), NOW), [0, 1, 0, 0]);
}

#[test]
fn lighting_working_lights_background_cards_too() {
    let bg = Mark::new(&background(MIN, 1), NOW, Some(Filter::Working), false);
    assert_eq!(bg.card_alpha(), 1.0);
    let idle_mark = Mark::new(&idle(MIN), NOW, Some(Filter::Working), false);
    assert_eq!(idle_mark.card_alpha(), DIMMED);
}

#[test]
fn board_background_rows_follow_the_real_working_rows() {
    let cards = [
        background(30 * MIN, 1), // 1: the longest, but Background
        working(2 * MIN),        // 2
        background(5 * MIN, 2),  // 3
        working(11 * MIN),       // 4
        idle(MIN),               // 5
    ];
    let names: Vec<String> = (1..=cards.len()).map(|n| format!("Chat {n}")).collect();
    let windows: Vec<_> = cards
        .iter()
        .enumerate()
        .map(|(at, card)| titled(at as isize + 1, &names[at], card))
        .collect();
    let columns = columns(&windows, NOW);
    let numbers = |column: &Vec<BoardRow>| column.iter().map(|row| row.number).collect::<Vec<_>>();
    assert_eq!(numbers(&columns[1]), [4, 2, 1, 3]);
    assert_eq!(columns[1][2].time, "30 min \u{b7} 1 job");
    assert_eq!(columns[1][3].time, "5 min \u{b7} 2 jobs");
    assert_eq!(numbers(&columns[2]), [5]);
    assert!(columns[3].is_empty());
}
