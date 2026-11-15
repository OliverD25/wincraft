use std::sync::mpsc;
use std::time::Duration;

use super::*;

/// Runs `step` while another thread holds `lock`, and gives back what it
/// returned and how long it took.
fn while_held<T: Send, R>(lock: &Mutex<T>, step: impl FnOnce() -> R) -> (R, Duration) {
    let (held, wait_held) = mpsc::channel();
    let (done, wait_done) = mpsc::channel::<()>();
    std::thread::scope(|scope| {
        scope.spawn(move || {
            let _guard = lock.lock().unwrap();
            held.send(()).unwrap();
            // Holds the lock until the step is over, or for a long time.
            let _ = wait_done.recv_timeout(Duration::from_secs(10));
        });
        wait_held.recv().unwrap();
        let start = Instant::now();
        let result = step();
        let took = start.elapsed();
        done.send(()).unwrap();
        (result, took)
    })
}

fn open_shared() -> Mutex<Shared> {
    Mutex::new(Shared {
        open: true,
        ..Default::default()
    })
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
fn the_main_frame_skips_and_asks_again_while_the_host_holds_shared() {
    // 0.20.131 hung here: the host held a lock while it waited for the UI
    // thread, and the UI thread waited for the lock.
    let (ctx, repaints) = counting_ctx();
    let shared = open_shared();
    let state = Mutex::new(DeckUi::default());
    let (look, took) = while_held(&shared, || main_frame(&ctx, &shared, &state, 0));
    assert_eq!(look, None);
    assert!(took < Duration::from_secs(1), "waited {took:?}");
    assert!(asked(&repaints) > 0, "no repaint asked");
}

#[test]
fn the_main_frame_skips_and_asks_again_while_the_host_holds_the_deck() {
    let (ctx, repaints) = counting_ctx();
    let shared = open_shared();
    let state = Mutex::new(DeckUi::default());
    let (look, took) = while_held(&state, || main_frame(&ctx, &shared, &state, 0));
    assert_eq!(look, None);
    assert!(took < Duration::from_secs(1), "waited {took:?}");
    assert!(asked(&repaints) > 0, "no repaint asked");
    // Neither lock is left taken.
    assert!(shared.try_lock().is_ok());
}

#[test]
fn with_both_locks_free_the_main_frame_gives_the_pin_and_the_mini_mode() {
    let (ctx, repaints) = counting_ctx();
    let mut closed = Shared::default();
    closed.settings.stay_on_top = false;
    closed.settings.mini = true;
    let shared = Mutex::new(closed);
    let state = Mutex::new(DeckUi::default());
    assert_eq!(main_frame(&ctx, &shared, &state, 0), Some((false, true)));
    // A closed deck has nothing to show, so nothing is asked.
    assert_eq!(asked(&repaints), 0);
}

#[test]
fn while_hidden_in_the_tray_the_builder_keeps_its_pin() {
    // A new pin would make winit show the hidden window again.
    let (ctx, _) = counting_ctx();
    let mut hidden = Shared {
        open: true,
        ..Default::default()
    };
    hidden.home.hidden = true;
    let shared = Mutex::new(hidden);
    let state = Mutex::new(DeckUi::default());
    assert_eq!(main_frame(&ctx, &shared, &state, 0), None);
    shared.lock().unwrap().home.hidden = false;
    shared.lock().unwrap().settings.stay_on_top = false;
    assert!(main_frame(&ctx, &shared, &state, 0).is_some_and(|(pin, _)| !pin));
}
