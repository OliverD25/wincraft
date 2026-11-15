use std::sync::Mutex;

use super::*;

fn at(start: Instant, ms: u64) -> Instant {
    start + Duration::from_millis(ms)
}

/// A context that counts the repaints asked of it: a new context has one
/// waiting already, so `has_requested_repaint` cannot tell.
fn counting_ctx() -> (
    egui::Context,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    let ctx = egui::Context::default();
    let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = std::sync::Arc::clone(&asked);
    ctx.set_request_repaint_callback(move |_| {
        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    });
    (ctx, asked)
}

fn asked(count: &std::sync::atomic::AtomicUsize) -> usize {
    count.load(std::sync::atomic::Ordering::SeqCst)
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

#[test]
fn try_both_takes_two_free_locks() {
    let (ctx, repaints) = counting_ctx();
    let (a, b) = (Mutex::new(1), Mutex::new("b"));
    let (a_now, b_now) = try_both(&ctx, &a, &b).expect("both free");
    assert_eq!((*a_now, *b_now), (1, "b"));
    assert_eq!(asked(&repaints), 0);
}

#[test]
fn try_both_skips_at_once_and_asks_again_when_another_thread_holds_one() {
    let (ctx, repaints) = counting_ctx();
    let (a, b) = (Mutex::new(()), Mutex::new(()));
    let (held, wait_held) = std::sync::mpsc::channel();
    let (done, wait_done) = std::sync::mpsc::channel::<()>();
    let held_lock = &b;
    std::thread::scope(|scope| {
        scope.spawn(move || {
            let _guard = held_lock.lock().unwrap();
            held.send(()).unwrap();
            let _ = wait_done.recv_timeout(Duration::from_secs(10));
        });
        wait_held.recv().unwrap();
        let start = Instant::now();
        assert!(try_both(&ctx, &a, &b).is_none());
        assert!(start.elapsed() < Duration::from_secs(1));
        done.send(()).unwrap();
    });
    assert!(asked(&repaints) > 0, "no repaint asked");
    assert!(a.try_lock().is_ok(), "the free lock is given back");
}

#[test]
fn try_both_skips_a_poisoned_lock_without_asking_again() {
    let (ctx, repaints) = counting_ctx();
    let (a, b) = (Mutex::new(()), Mutex::new(()));
    let _ = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let _guard = a.lock().unwrap();
                panic!("poisons the lock");
            })
            .join()
    });
    assert!(try_both(&ctx, &a, &b).is_none());
    assert_eq!(asked(&repaints), 0);
}
