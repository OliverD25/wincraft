//! `recover`, `release_all` and `wake_all` against fake monitors behind real
//! workers: what is sent to each monitor, what the saved record says
//! afterwards, and that one monitor failing does not stop the others.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows_sys::Win32::Foundation::RECT;

use super::ddc::fake::{Fake, Style};
use super::ddc::{Monitor, Power, Reading, Write};
use super::saved::{self, Saved};
use super::*;

/// A fake monitor the test keeps a handle on after the worker owns it.
#[derive(Clone)]
struct Shared {
    fake: Arc<Mutex<Fake>>,
    /// How long each write really takes, to show the monitors are asked at
    /// the same time.
    write_takes: Duration,
}

impl Monitor for Shared {
    fn read(&mut self, code: u8) -> Option<Reading> {
        self.fake.lock().unwrap().read(code)
    }

    fn write(&mut self, write: Write) -> bool {
        std::thread::sleep(self.write_takes);
        self.fake.lock().unwrap().write(write)
    }

    fn pause(&mut self, time: Duration) -> bool {
        self.fake.lock().unwrap().pause(time)
    }
}

struct Rig {
    dimmer: ScreenDimmer,
    fakes: BTreeMap<String, Arc<Mutex<Fake>>>,
    /// The ids in the order their workers were made.
    opened: Arc<Mutex<Vec<String>>>,
    dir: PathBuf,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Spec {
    id: &'static str,
    saved: Saved,
    style: Style,
    /// The fake starts in standby, or at brightness 0, as the record says.
    /// False leaves the worker with no DDC/CI handle at all.
    present: bool,
}

fn spec(id: &'static str, saved: Saved, style: Style) -> Spec {
    Spec {
        id,
        saved,
        style,
        present: true,
    }
}

fn in_standby() -> Saved {
    Saved {
        left_in_standby: true,
        ..Saved::default()
    }
}

fn dimmed_from(level: u32) -> Saved {
    Saved {
        dimmed_from: Some(level),
        ..Saved::default()
    }
}

fn screen(id: &str, index: i32) -> Screen {
    Screen {
        handle: std::ptr::null_mut(),
        rect: RECT {
            left: index * 100,
            top: 0,
            right: index * 100 + 100,
            bottom: 100,
        },
        primary: index == 0,
        name: id.to_string(),
        place: None,
        id: id.to_string(),
    }
}

fn rig(specs: Vec<Spec>, write_takes: Duration) -> Rig {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wincraft-dimmer-rig-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let mut dimmer = ScreenDimmer {
        state_path: dir.join("screen_dimmer.state.json"),
        reports: Some(mpsc::channel()),
        fence: fence::Fence::inert(),
        ..ScreenDimmer::default()
    };
    let mut fakes = BTreeMap::new();
    let mut shared = BTreeMap::new();
    for (index, spec) in specs.iter().enumerate() {
        dimmer.screens.push(screen(spec.id, index as i32));
        dimmer
            .machines
            .insert(spec.id.to_string(), Machine::new(spec.saved.clone()));
        let mut fake = Fake::new(spec.style);
        if spec.saved.left_in_standby {
            fake.write(Write::Power(Power::Standby));
        }
        if spec.saved.dimmed_from.is_some() && fake.brightness.is_some() {
            fake.write(Write::Brightness(0));
        }
        fake.writes.clear();
        let fake = Arc::new(Mutex::new(fake));
        fakes.insert(spec.id.to_string(), Arc::clone(&fake));
        if spec.present {
            shared.insert(spec.id.to_string(), fake);
        }
    }
    dimmer.written = StateFile {
        monitors: dimmer
            .machines
            .iter()
            .filter(|(_, machine)| !machine.saved.is_empty())
            .map(|(id, machine)| (id.clone(), machine.saved.clone()))
            .collect(),
    };
    let opened = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&opened);
    dimmer.opener = Some(Box::new(move |id, _| {
        log.lock().unwrap().push(id.to_string());
        let found = shared.get(id).cloned();
        Box::new(move |stop: Arc<AtomicBool>| {
            let fake = found?;
            fake.lock().unwrap().stop = Some(stop);
            Some(Box::new(Shared { fake, write_takes }) as Box<dyn Monitor>)
        })
    }));
    Rig {
        dimmer,
        fakes,
        opened,
        dir,
    }
}

impl Rig {
    fn writes(&self, id: &str) -> Vec<Write> {
        let fake = self.fakes[id].lock().unwrap();
        fake.writes.iter().map(|(_, write)| *write).collect()
    }

    fn fake(&self, id: &str) -> std::sync::MutexGuard<'_, Fake> {
        self.fakes[id].lock().unwrap()
    }

    fn record(&self, id: &str) -> Saved {
        self.dimmer.machines[id].saved.clone()
    }

    /// Reads the workers' reports until no monitor is busy.
    fn settle(&mut self) {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            self.dimmer.drain();
            if !self.dimmer.machines.values().any(Machine::busy) {
                return;
            }
            assert!(Instant::now() < end, "the monitors never settled");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn file(&self) -> StateFile {
        saved::load(&self.dimmer.state_path)
    }
}

#[test]
fn start_wakes_a_monitor_left_in_standby_and_restores_a_dimmed_one() {
    let mut rig = rig(
        vec![
            spec("A", in_standby(), Style::Philips),
            spec("B", dimmed_from(70), Style::Philips),
            spec("C", Saved::default(), Style::Philips),
        ],
        Duration::ZERO,
    );
    rig.dimmer.recover();
    assert_eq!(rig.dimmer.machines["A"].look(), Look::Off);
    assert_eq!(rig.dimmer.machines["B"].look(), Look::Dimmed);
    rig.settle();

    assert!(!rig.fake("A").asleep());
    assert!(rig.writes("A").contains(&Write::Power(Power::On)));
    assert_eq!(rig.writes("B"), [Write::Brightness(70)]);
    assert_eq!(rig.fake("B").brightness, Some(70));
    assert!(rig.writes("C").is_empty(), "a monitor with no record");
    assert_eq!(*rig.opened.lock().unwrap(), ["A", "B"]);
    for id in ["A", "B", "C"] {
        assert_eq!(rig.dimmer.machines[id].look(), Look::On, "{id}");
    }
    assert!(rig.record("A").is_empty() && rig.record("B").is_empty());
    assert!(
        rig.file().monitors.is_empty(),
        "the record is cleared on disk"
    );
}

#[test]
fn a_covered_monitor_leaves_no_record_so_start_does_nothing_for_it() {
    let mut rig = rig(
        vec![spec("A", Saved::default(), Style::Philips)],
        Duration::ZERO,
    );
    rig.dimmer.machines.get_mut("A").unwrap().cover_key();
    assert_eq!(rig.dimmer.machines["A"].look(), Look::Covered);
    rig.dimmer.recover();
    assert!(rig.writes("A").is_empty());
    assert!(rig.opened.lock().unwrap().is_empty(), "no worker was made");
}

#[test]
fn a_monitor_that_does_not_wake_in_time_keeps_its_record_for_the_next_start() {
    let mut rig = rig(
        vec![
            spec("A", in_standby(), Style::Silent),
            spec("B", in_standby(), Style::Philips),
        ],
        Duration::ZERO,
    );
    rig.dimmer.recover();
    rig.settle();
    assert!(rig.record("A").left_in_standby);
    assert!(!rig.record("B").left_in_standby);
    assert!(!rig.fake("B").asleep());
    assert!(rig.file().monitors.contains_key("A"));
    assert!(!rig.file().monitors.contains_key("B"));
}

#[test]
fn leaving_wakes_standby_monitors_and_restores_dimmed_ones_but_not_the_rest() {
    let mut rig = rig(
        vec![
            spec("A", in_standby(), Style::Philips),
            spec("B", dimmed_from(70), Style::Philips),
            spec("C", Saved::default(), Style::Philips),
        ],
        Duration::ZERO,
    );
    rig.dimmer.release_all();

    assert_eq!(rig.writes("A"), [Write::Power(Power::On)]);
    assert_eq!(rig.writes("B"), [Write::Brightness(70)]);
    assert_eq!(rig.fake("B").brightness, Some(70));
    assert!(rig.writes("C").is_empty());
    assert!(rig.dimmer.workers.is_empty(), "every worker is stopped");
    assert!(rig.record("A").is_empty() && rig.record("B").is_empty());
    assert!(rig.file().monitors.is_empty());
}

#[test]
fn leaving_asks_every_monitor_at_once_within_the_time_it_waits() {
    let take = Duration::from_millis(500);
    let mut rig = rig(
        vec![
            spec("A", in_standby(), Style::Philips),
            spec("B", dimmed_from(70), Style::Philips),
            spec("C", in_standby(), Style::Philips),
        ],
        take,
    );
    let started = Instant::now();
    rig.dimmer.release_all();
    let took = started.elapsed();
    // One after another these would take 1.5 s, the whole time release_all
    // waits.
    assert!(took < RELEASE_WAIT, "took {took:?}");
    for id in ["A", "B", "C"] {
        assert!(!rig.writes(id).is_empty(), "{id} was asked");
        assert!(rig.record(id).is_empty(), "{id} is recorded as put back");
    }
}

#[test]
fn a_monitor_that_cannot_be_reached_does_not_stop_the_others_leaving() {
    let mut rig = rig(
        vec![
            spec("A", in_standby(), Style::Silent),
            Spec {
                present: false,
                ..spec("B", in_standby(), Style::Philips)
            },
            spec("C", dimmed_from(70), Style::Philips),
            spec("D", in_standby(), Style::Philips),
        ],
        Duration::ZERO,
    );
    rig.dimmer.release_all();

    assert_eq!(rig.fake("C").brightness, Some(70));
    assert_eq!(rig.writes("D"), [Write::Power(Power::On)]);
    assert!(rig.record("C").is_empty() && rig.record("D").is_empty());
    // The two that could not be asked keep the record, so the next start
    // wakes them.
    assert!(rig.record("A").left_in_standby, "its write failed");
    assert!(rig.record("B").left_in_standby, "it has no DDC/CI handle");
    let file = rig.file();
    assert!(file.monitors.contains_key("A") && file.monitors.contains_key("B"));
    assert!(!file.monitors.contains_key("C") && !file.monitors.contains_key("D"));
}

fn all_off(styles: [Style; 3]) -> Vec<Spec> {
    ["L", "M", "R"]
        .into_iter()
        .zip(styles)
        .map(|(id, style)| spec(id, in_standby(), style))
        .collect()
}

fn mark_off(rig: &mut Rig) {
    for id in ["L", "M", "R"] {
        let machine = rig.dimmer.machines.get_mut(id).unwrap();
        machine.off_key(Mode::Off);
        machine.report(Outcome::StandbySent(true));
        assert_eq!(machine.look(), Look::Off);
    }
}

#[test]
fn waking_all_goes_through_the_monitors_in_screen_order() {
    let mut rig = rig(all_off([Style::Philips; 3]), Duration::ZERO);
    mark_off(&mut rig);
    rig.dimmer.wake_all();
    rig.settle();
    assert_eq!(*rig.opened.lock().unwrap(), ["L", "M", "R"]);
    for id in ["L", "M", "R"] {
        assert!(!rig.fake(id).asleep(), "{id}");
        assert_eq!(rig.dimmer.machines[id].look(), Look::On, "{id}");
    }
}

#[test]
fn waking_all_twice_asks_each_monitor_once() {
    let wakes = |twice: bool| {
        let mut rig = rig(all_off([Style::Philips; 3]), Duration::ZERO);
        mark_off(&mut rig);
        rig.dimmer.wake_all();
        if twice {
            rig.dimmer.wake_all();
        }
        rig.settle();
        rig.dimmer.wake_all();
        rig.settle();
        ["L", "M", "R"].map(|id| rig.writes(id))
    };
    let once = wakes(false);
    assert!(once.iter().all(|writes| !writes.is_empty()));
    assert_eq!(wakes(true), once, "a second wake_all sent nothing more");
}

#[test]
fn waking_all_goes_on_when_one_monitor_does_not_answer() {
    let mut rig = rig(
        all_off([Style::Philips, Style::Silent, Style::Aorus]),
        Duration::ZERO,
    );
    mark_off(&mut rig);
    rig.dimmer.wake_all();
    rig.settle();
    assert!(!rig.fake("L").asleep());
    assert!(!rig.fake("R").asleep());
    assert!(
        !rig.writes("M").is_empty(),
        "the silent one was still asked"
    );
    assert!(rig.record("M").left_in_standby, "and keeps its record");
    assert!(!rig.record("L").left_in_standby && !rig.record("R").left_in_standby);
}
