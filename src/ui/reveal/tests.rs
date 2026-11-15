use super::*;

fn at(start: Instant, ms: u64) -> Instant {
    start + Duration::from_millis(ms)
}

#[test]
fn a_window_not_found_yet_waits_hidden() {
    let mut reveal = Reveal::Hidden;
    assert_eq!(reveal.on_main_frame(false, Instant::now()), Step::Again);
    assert_eq!(reveal, Reveal::Hidden);
    assert_eq!(reveal.on_window_frame(), Step::Nothing);
    assert_eq!(reveal, Reveal::Hidden);
}

#[test]
fn hidden_then_covered_then_drawn_then_shown() {
    let start = Instant::now();
    let mut reveal = Reveal::Hidden;
    assert_eq!(reveal.on_main_frame(true, start), Step::Show);
    assert_eq!(reveal, Reveal::Covered { since: start });
    assert_eq!(reveal.on_main_frame(true, at(start, 10)), Step::Again);
    assert_eq!(reveal.on_window_frame(), Step::Again);
    assert_eq!(reveal, Reveal::Drawn);
    assert_eq!(reveal.on_main_frame(true, at(start, 20)), Step::Again);
    assert_eq!(reveal.on_window_frame(), Step::Uncover);
    assert_eq!(reveal, Reveal::Shown);
    assert_eq!(reveal.on_main_frame(true, at(start, 30)), Step::Nothing);
    assert_eq!(reveal.on_window_frame(), Step::Nothing);
}

#[test]
fn a_window_that_does_not_draw_is_uncovered_after_the_limit() {
    let start = Instant::now();
    let mut reveal = Reveal::Covered { since: start };
    let just_before = COVERED_LONGEST.as_millis() as u64 - 1;
    assert_eq!(
        reveal.on_main_frame(true, at(start, just_before)),
        Step::Again
    );
    let limit = COVERED_LONGEST.as_millis() as u64;
    assert_eq!(
        reveal.on_main_frame(true, at(start, limit)),
        Step::UncoverLate
    );
    assert_eq!(reveal, Reveal::Shown);
}

#[test]
fn the_limit_is_400_ms() {
    assert_eq!(COVERED_LONGEST, Duration::from_millis(400));
}

#[test]
fn a_drawn_window_is_not_uncovered_late() {
    let start = Instant::now();
    let mut reveal = Reveal::Drawn;
    assert_eq!(reveal.on_main_frame(true, at(start, 5_000)), Step::Again);
    assert_eq!(reveal.on_window_frame(), Step::Uncover);
}

#[test]
fn a_close_resets_to_hidden_and_the_next_open_shows_again() {
    let start = Instant::now();
    let mut reveal = Reveal::Shown;
    assert_eq!(reveal.on_main_frame(true, start), Step::Nothing);
    reveal = Reveal::default();
    assert_eq!(reveal, Reveal::Hidden);
    assert_eq!(reveal.on_main_frame(true, start), Step::Show);
}

#[test]
fn covered_again_waits_for_the_next_frame_and_keeps_an_older_start() {
    let start = Instant::now();
    let mut reveal = Reveal::Shown;
    reveal.covered_again(start);
    assert_eq!(reveal, Reveal::Covered { since: start });
    reveal.covered_again(at(start, 100));
    assert_eq!(reveal, Reveal::Covered { since: start });
    assert_eq!(reveal.on_window_frame(), Step::Again);
    reveal.covered_again(at(start, 200));
    assert_eq!(
        reveal,
        Reveal::Covered {
            since: at(start, 200)
        }
    );
}

#[test]
fn a_gap_in_the_parents_passes_means_a_new_immediate_window() {
    assert!(fresh_window(None, 7));
    assert!(!fresh_window(Some(6), 7));
    assert!(fresh_window(Some(5), 7));
    assert!(fresh_window(Some(7), 7));
    assert!(fresh_window(Some(9), 0));
}
