use super::*;

const LEFT: Rect = [0, 0, 1920, 1080];
const RIGHT: Rect = [1920, 0, 4480, 1440];
const ABOVE: Rect = [0, -1080, 1920, 0];

#[test]
fn the_monitor_looked_at_is_the_front_windows() {
    // Front on monitor 0, target on 1: a hint.
    let looking = looking_at(None, Some(0), Some(1));
    assert_eq!(looking, Some(0));
    assert!(another_monitor(looking, Some(1)));
    // Same monitor: nothing new.
    assert!(!another_monitor(looking, Some(0)));
}

#[test]
fn with_no_window_in_front_the_mouse_monitor_counts() {
    let looking = looking_at(None, None, Some(1));
    assert_eq!(looking, Some(1));
    assert!(another_monitor(looking, Some(0)));
    assert!(!another_monitor(looking, Some(1)));
    assert!(is_desktop("Progman"));
    assert!(is_desktop("WorkerW"));
    assert!(!is_desktop("Notepad"));
}

#[test]
fn quick_access_counts_before_the_window_in_front() {
    // Quick access closed on monitor 1; Windows already put another
    // window in front on monitor 0.
    let looking = looking_at(Some(1), Some(0), Some(0));
    assert_eq!(looking, Some(1));
    assert!(!another_monitor(looking, Some(1)));
    assert!(another_monitor(looking, Some(0)));
}

#[test]
fn nothing_known_means_no_hint() {
    assert!(!another_monitor(None, Some(1)));
    assert!(!another_monitor(Some(0), None));
}

#[test]
fn the_arrow_points_from_centre_to_centre() {
    assert_eq!(direction(LEFT, RIGHT), Dir::Right);
    assert_eq!(direction(RIGHT, LEFT), Dir::Left);
    assert_eq!(direction(LEFT, ABOVE), Dir::Up);
    assert_eq!(direction(ABOVE, LEFT), Dir::Down);
    // A small laptop below and to the right of a big monitor: mostly
    // across, so across.
    let big = [0, 0, 3840, 2160];
    let laptop = [3840, 1400, 5120, 2200];
    assert_eq!(direction(big, laptop), Dir::Right);
    // A small one sitting high on top of a wide one: up.
    let wide = [0, 0, 5120, 1440];
    let small = [1500, -1080, 3420, 0];
    assert_eq!(direction(wide, small), Dir::Up);
    assert_eq!(Dir::Right.arrow(), "\u{2192}");
    assert_eq!(Dir::Left.arrow(), "\u{2190}");
}

#[test]
fn the_pill_sits_at_the_facing_edge_in_line_with_the_window() {
    let work = [0, 0, 1920, 1040];
    let size = (300, 40);
    let window = [2000, 400, 2800, 600];
    assert_eq!(pill_spot(work, Dir::Right, window, size, 16), (1604, 480));
    assert_eq!(pill_spot(work, Dir::Left, window, size, 16), (16, 480));
    let below = [600, 1200, 1000, 1400];
    assert_eq!(pill_spot(work, Dir::Down, below, size, 16), (650, 984));
    assert_eq!(pill_spot(work, Dir::Up, below, size, 16), (650, 16));
}

#[test]
fn the_pill_is_kept_inside_the_work_area() {
    let work = [0, 0, 1920, 1040];
    let size = (300, 40);
    // A window far below the bottom of this monitor's work area.
    let low = [2000, 1300, 2800, 1440];
    assert_eq!(pill_spot(work, Dir::Right, low, size, 16), (1604, 984));
    let high = [2000, -500, 2800, -100];
    assert_eq!(pill_spot(work, Dir::Right, high, size, 16), (1604, 16));
    let far_left = [-3000, 1100, -2000, 1300];
    assert_eq!(pill_spot(work, Dir::Down, far_left, size, 16), (16, 984));
}

#[test]
fn the_glow_goes_round_a_normal_window() {
    let work = [1920, 0, 4480, 1400];
    let frame = [2100, 200, 3100, 900];
    let glow = ring(frame, work, false);
    assert_eq!(
        glow,
        Ring {
            frame,
            inside: false
        }
    );
    assert_eq!(ring_box(glow, 16), [2084, 184, 3116, 916]);
}

#[test]
fn the_glow_runs_inside_the_work_area_for_a_maximised_window() {
    let work = [1920, 0, 4480, 1400];
    let maximised = ring([1920, 0, 4480, 1400], work, true);
    assert_eq!(
        maximised,
        Ring {
            frame: work,
            inside: true
        }
    );
    assert_eq!(ring_box(maximised, 16), work);
    // Bigger than the work area without being maximised: the same.
    let over = ring([1900, -10, 4500, 1440], work, false);
    assert_eq!(over.frame, work);
    assert!(over.inside);
}

#[test]
fn the_fade_rises_holds_and_falls() {
    let hold = GLOW_HOLD_MS;
    assert_eq!(strength(0, hold, true), Some(0.0));
    assert_eq!(strength(FADE_IN_MS / 2, hold, true), Some(0.5));
    assert_eq!(strength(FADE_IN_MS, hold, true), Some(1.0));
    assert_eq!(strength(FADE_IN_MS + hold - 1, hold, true), Some(1.0));
    let falling = strength(FADE_IN_MS + hold + FADE_OUT_MS / 2, hold, true).unwrap();
    assert!((falling - 0.5).abs() < 0.01);
    assert_eq!(strength(FADE_IN_MS + hold + FADE_OUT_MS, hold, true), None);
    // The glow takes about 0.8 s in all, the pill about 1.6 s.
    assert_eq!(FADE_IN_MS + GLOW_HOLD_MS + FADE_OUT_MS, 820);
    assert_eq!(FADE_IN_MS + PILL_HOLD_MS + FADE_OUT_MS, 1620);
}

#[test]
fn without_animations_it_shows_for_the_hold_and_goes() {
    let hold = PILL_HOLD_MS;
    assert_eq!(strength(0, hold, false), Some(1.0));
    assert_eq!(strength(hold - 1, hold, false), Some(1.0));
    assert_eq!(strength(hold, hold, false), None);
}

#[test]
fn long_names_are_cut() {
    assert_eq!(cut("  Telegram ", 40), "Telegram");
    let long = "A very long window title that goes on and on and on";
    let short = cut(long, 20);
    // 19 characters kept, the space before the cut dropped.
    assert_eq!(short, "A very long window\u{2026}");
    assert!(short.ends_with('\u{2026}'));
    assert_eq!(cut("abc", 3), "abc");
}

#[test]
fn the_pill_names_the_window_and_the_monitor() {
    assert_eq!(
        pill_text("Telegram", Some("Right"), Dir::Right),
        "Telegram \u{b7} Right monitor \u{2192}"
    );
    assert_eq!(
        pill_text("Telegram", Some("Left"), Dir::Left),
        "\u{2190} Telegram \u{b7} Left monitor"
    );
    assert_eq!(
        pill_text("Mail", Some("Monitor 4"), Dir::Right),
        "Mail \u{b7} Monitor 4 \u{2192}"
    );
    assert_eq!(pill_words("", Some("Centre")), "Centre monitor");
    assert_eq!(pill_words("Mail", None), "Mail \u{b7} Other monitor");
    let long = "x".repeat(60);
    assert!(pill_words(&long, Some("Right")).starts_with(&cut(&long, NAME_CHARS)));
}

#[test]
fn the_glow_paints_the_edge_and_leaves_the_window_clear() {
    use crate::core::canvas::Area;
    let tokens = crate::core::theme::Tokens::dark();
    let image = draw::glow(
        (100, 100),
        Area::new(20.0, 20.0, 60.0, 60.0),
        false,
        &tokens,
        1.0,
    );
    let alpha = |x: i32, y: i32| image.pixels[(y * image.width + x) as usize] >> 24;
    assert_eq!(alpha(50, 50), 0, "the window itself stays clear");
    assert!(alpha(18, 50) > 200, "the line just outside the frame");
    assert!(
        alpha(10, 50) > 0 && alpha(10, 50) < alpha(18, 50),
        "the soft part"
    );
    assert_eq!(alpha(1, 50), 0, "past the glow");
    // Inside: the line runs along the inner edge of the box.
    let image = draw::glow(
        (100, 100),
        Area::new(0.0, 0.0, 100.0, 100.0),
        true,
        &tokens,
        1.0,
    );
    let alpha = |x: i32, y: i32| image.pixels[(y * image.width + x) as usize] >> 24;
    assert!(alpha(1, 50) > 200);
    assert_eq!(alpha(50, 50), 0);
}

#[test]
fn a_new_hint_replaces_the_old_one() {
    let first = screen::begin(screen::Live::bare());
    assert_eq!(screen::current(), Some(first));
    let second = screen::begin(screen::Live::bare());
    assert_ne!(first, second);
    assert_eq!(screen::current(), Some(second));
    hide();
    assert_eq!(screen::current(), None);
}
