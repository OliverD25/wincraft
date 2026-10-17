//! DDC/CI, the command channel inside the monitor cable, through dxva2.dll.
//! VCP code 0xD6 is the power mode and 0x10 the brightness. Every call takes
//! 40 to 100 ms and some monitors stop answering for seconds while they
//! wake, so the calls run on one worker thread per monitor and never on the
//! host thread; results come back through a channel.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows_sys::Win32::Devices::Display::{
    DestroyPhysicalMonitors, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, GetVCPFeatureAndVCPFeatureReply, SetVCPFeature,
    MC_VCP_CODE_TYPE, PHYSICAL_MONITOR,
};
use windows_sys::Win32::Graphics::Gdi::HMONITOR;

use crate::core::monitors::from_wide;

pub const POWER: u8 = 0xD6;
pub const BRIGHTNESS: u8 = 0x10;

/// How long `detect_standby` watches the power mode after asking for
/// standby, and how often it looks.
pub const DETECT_FOR: Duration = Duration::from_secs(2);
pub const DETECT_EVERY: Duration = Duration::from_millis(250);
/// The AORUS FV43U does not answer for about 5 s while it wakes.
pub const WAKE_FOR: Duration = Duration::from_secs(8);
pub const WAKE_EVERY: Duration = Duration::from_secs(1);
const PAUSE_SLICE: Duration = Duration::from_millis(50);

/// The power modes WinCraft sends. 5 (off) is left out on purpose: after it
/// the MQ780 stopped answering DDC/CI and only its power button woke it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Power {
    On,
    Standby,
}

impl Power {
    pub fn value(self) -> u32 {
        match self {
            Power::On => 1,
            Power::Standby => 4,
        }
    }
}

/// Everything WinCraft can write. A power write can only carry a `Power`,
/// so no value outside `Power::ALL` can reach code 0xD6.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Write {
    Power(Power),
    Brightness(u32),
}

impl Write {
    pub fn code_and_value(self) -> (u8, u32) {
        match self {
            Write::Power(power) => (POWER, power.value()),
            Write::Brightness(value) => (BRIGHTNESS, value),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    pub current: u32,
    pub max: u32,
}

/// One monitor as the procedures below see it, so tests run them against a
/// fake monitor and a clock that does not really wait.
pub trait Monitor {
    fn read(&mut self, code: u8) -> Option<Reading>;
    fn write(&mut self, write: Write) -> bool;
    /// False when the wait was cut short because WinCraft is stopping.
    fn pause(&mut self, time: Duration) -> bool;
}

/// The physical monitors behind one HMONITOR: one normally, more when
/// Windows duplicates the picture onto several.
pub struct Physical {
    handles: Vec<PHYSICAL_MONITOR>,
    stop: Arc<AtomicBool>,
}

impl Physical {
    pub fn open(monitor: isize, stop: Arc<AtomicBool>) -> Option<Self> {
        let hmonitor = monitor as HMONITOR;
        let mut count = 0u32;
        if unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(hmonitor, &mut count) } == 0
            || count == 0
        {
            return None;
        }
        let mut handles = vec![PHYSICAL_MONITOR::default(); count as usize];
        if unsafe { GetPhysicalMonitorsFromHMONITOR(hmonitor, count, handles.as_mut_ptr()) } == 0 {
            return None;
        }
        Some(Self { handles, stop })
    }

    /// What the driver calls the monitor, often just "Generic PnP Monitor".
    pub fn description(&self) -> String {
        self.handles
            .first()
            .map(|handle| {
                let text = handle.szPhysicalMonitorDescription;
                from_wide(&text)
            })
            .unwrap_or_default()
    }
}

impl Monitor for Physical {
    fn read(&mut self, code: u8) -> Option<Reading> {
        let handle = self.handles.first()?.hPhysicalMonitor;
        let mut kind: MC_VCP_CODE_TYPE = 0;
        let (mut current, mut max) = (0u32, 0u32);
        let ok = unsafe {
            GetVCPFeatureAndVCPFeatureReply(handle, code, &mut kind, &mut current, &mut max)
        } != 0;
        ok.then_some(Reading { current, max })
    }

    fn write(&mut self, write: Write) -> bool {
        let (code, value) = write.code_and_value();
        let mut ok = !self.handles.is_empty();
        for handle in &self.handles {
            if unsafe { SetVCPFeature(handle.hPhysicalMonitor, code, value) } == 0 {
                ok = false;
            }
        }
        ok
    }

    fn pause(&mut self, time: Duration) -> bool {
        let end = Instant::now() + time;
        loop {
            if self.stop.load(Ordering::Relaxed) {
                return false;
            }
            let now = Instant::now();
            if now >= end {
                return true;
            }
            std::thread::sleep(PAUSE_SLICE.min(end - now));
        }
    }
}

impl Drop for Physical {
    fn drop(&mut self) {
        if !self.handles.is_empty() {
            unsafe { DestroyPhysicalMonitors(self.handles.len() as u32, self.handles.as_ptr()) };
        }
    }
}

/// What asking a monitor for standby showed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detected {
    /// It went into standby: it reported another power mode, or it stopped
    /// answering after it had answered before.
    Works,
    /// It stayed on (the MQ780 does this). Nothing more was sent.
    Ignored,
    /// It does not answer DDC/CI, so nothing was sent.
    NoDdc,
    /// WinCraft is stopping; the monitor may be in standby.
    Stopped,
}

/// Asks for standby and watches the power mode for up to `DETECT_FOR`. The
/// first reading other than on settles it, as do reads that all fail: the
/// AORUS FV43U fails its reads for half a second, then reports 2.
pub fn detect_standby(monitor: &mut dyn Monitor) -> Detected {
    if monitor.read(POWER).is_none() || !monitor.write(Write::Power(Power::Standby)) {
        return Detected::NoDdc;
    }
    let mut still_on = false;
    let mut waited = Duration::ZERO;
    while waited < DETECT_FOR {
        if !monitor.pause(DETECT_EVERY) {
            return Detected::Stopped;
        }
        waited += DETECT_EVERY;
        match monitor.read(POWER) {
            Some(reading) if reading.current != Power::On.value() => return Detected::Works,
            Some(_) => still_on = true,
            None => {}
        }
    }
    if still_on {
        Detected::Ignored
    } else {
        Detected::Works
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Woke {
    /// The monitor reported on this long after the first request.
    On(Duration),
    TimedOut,
    Stopped,
}

/// Asks for on, then every `WAKE_EVERY` reads the power mode and asks again
/// until the monitor reports on or `WAKE_FOR` has passed.
pub fn wake(monitor: &mut dyn Monitor) -> Woke {
    monitor.write(Write::Power(Power::On));
    let mut waited = Duration::ZERO;
    while waited < WAKE_FOR {
        if !monitor.pause(WAKE_EVERY) {
            return Woke::Stopped;
        }
        waited += WAKE_EVERY;
        if monitor.read(POWER).map(|reading| reading.current) == Some(Power::On.value()) {
            return Woke::On(waited);
        }
        if waited < WAKE_FOR {
            monitor.write(Write::Power(Power::On));
        }
    }
    Woke::TimedOut
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    Detect,
    Standby,
    Wake,
    ReadBrightness,
    SetBrightness(u32),
    /// At exit: one request each, without waiting for an answer.
    Release {
        standby: bool,
        brightness: Option<u32>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Detected(Detected),
    StandbySent(bool),
    Woke(Woke),
    Brightness(Option<Reading>),
    BrightnessSet(bool),
    Released(bool),
    /// Windows gave no DDC/CI handle for the monitor.
    NoMonitor,
}

pub fn run(monitor: &mut dyn Monitor, job: Job) -> Outcome {
    match job {
        Job::Detect => Outcome::Detected(detect_standby(monitor)),
        Job::Standby => Outcome::StandbySent(monitor.write(Write::Power(Power::Standby))),
        Job::Wake => Outcome::Woke(wake(monitor)),
        Job::ReadBrightness => Outcome::Brightness(monitor.read(BRIGHTNESS)),
        Job::SetBrightness(value) => {
            Outcome::BrightnessSet(monitor.write(Write::Brightness(value)))
        }
        Job::Release {
            standby,
            brightness,
        } => {
            let mut ok = true;
            if standby {
                ok &= monitor.write(Write::Power(Power::On));
            }
            if let Some(value) = brightness {
                ok &= monitor.write(Write::Brightness(value));
            }
            Outcome::Released(ok)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub monitor: String,
    pub job: Job,
    pub outcome: Outcome,
}

/// Opens the monitor on the worker thread, where its handles then live.
/// The flag turns true when the worker is asked to stop.
pub type Opener = Box<dyn FnOnce(Arc<AtomicBool>) -> Option<Box<dyn Monitor>> + Send>;

/// Called on the worker thread after each report, to wake the host.
pub type Nudge = Arc<dyn Fn() + Send + Sync>;

/// One thread for one monitor, running its jobs in order.
pub struct Worker {
    jobs: Option<Sender<Job>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn spawn(id: &str, open: Opener, reports: Sender<Report>, nudge: Nudge) -> Option<Self> {
        let (jobs, queue): (Sender<Job>, Receiver<Job>) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let monitor_id = id.to_string();
        let thread = std::thread::Builder::new()
            .name(format!("ddc {id}"))
            .spawn(move || {
                let mut monitor = open(flag);
                while let Ok(job) = queue.recv() {
                    let outcome = match monitor.as_deref_mut() {
                        Some(monitor) => run(monitor, job),
                        None => Outcome::NoMonitor,
                    };
                    let report = Report {
                        monitor: monitor_id.clone(),
                        job,
                        outcome,
                    };
                    if reports.send(report).is_err() {
                        break;
                    }
                    nudge();
                }
            });
        match thread {
            Ok(thread) => Some(Self {
                jobs: Some(jobs),
                stop,
                thread: Some(thread),
            }),
            Err(err) => {
                log::error!("could not start the DDC/CI thread for {id}: {err}");
                None
            }
        }
    }

    pub fn send(&self, job: Job) {
        if let Some(jobs) = &self.jobs {
            let _ = jobs.send(job);
        }
    }

    /// Cuts a wait in progress short, queues `last` as the final job and
    /// lets the thread end once it has run.
    pub fn stop(&mut self, last: Option<Job>) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(job) = last {
            self.send(job);
        }
        self.jobs = None;
    }

    pub fn is_finished(&self) -> bool {
        self.thread
            .as_ref()
            .is_none_or(|thread| thread.is_finished())
    }

    pub fn join(mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Waits until every worker has ended or `deadline` has come, and says
/// whether they all ended. Workers still running are left to end alone.
pub fn wait_all(workers: Vec<Worker>, deadline: Instant) -> bool {
    while workers.iter().any(|worker| !worker.is_finished()) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let all = workers.iter().all(Worker::is_finished);
    for worker in workers {
        if worker.is_finished() {
            worker.join();
        }
    }
    all
}

#[cfg(test)]
pub mod fake {
    use super::*;

    /// How a fake monitor answers while in standby, after the three real
    /// monitors measured on 2026-10-17.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Style {
        /// Philips 323E7: reports 4 in standby, on again about 2 s after a
        /// request.
        Philips,
        /// AORUS FV43U: no answer for 0.5 s, then 2; no answer for about 5 s
        /// while it wakes.
        Aorus,
        /// MQ780: ignores standby and keeps reporting 1.
        Ignores,
        /// No DDC/CI at all.
        Silent,
    }

    pub struct Fake {
        pub style: Style,
        pub now: Duration,
        pub writes: Vec<(Duration, Write)>,
        pub brightness: Option<u32>,
        /// `pause` reports a stop from this moment on, or once the flag is
        /// set.
        pub stop_at: Option<Duration>,
        pub stop: Option<Arc<AtomicBool>>,
        asleep_since: Option<Duration>,
        waking_since: Option<Duration>,
    }

    impl Fake {
        pub fn new(style: Style) -> Self {
            Self {
                style,
                now: Duration::ZERO,
                writes: Vec::new(),
                brightness: (style != Style::Silent && style != Style::Aorus).then_some(70),
                stop_at: None,
                stop: None,
                asleep_since: None,
                waking_since: None,
            }
        }

        pub fn asleep(&self) -> bool {
            self.asleep_since.is_some() || self.waking_since.is_some()
        }

        fn wake_time(&self) -> Duration {
            match self.style {
                Style::Aorus => Duration::from_millis(5000),
                _ => Duration::from_millis(2000),
            }
        }
    }

    impl Monitor for Fake {
        fn read(&mut self, code: u8) -> Option<Reading> {
            if self.style == Style::Silent {
                return None;
            }
            if code == BRIGHTNESS {
                return self.brightness.map(|current| Reading { current, max: 100 });
            }
            if let Some(since) = self.waking_since {
                if self.now - since >= self.wake_time() {
                    self.waking_since = None;
                } else {
                    return match self.style {
                        Style::Aorus => None,
                        _ => Some(Reading { current: 4, max: 5 }),
                    };
                }
            }
            let current = match self.asleep_since {
                Some(since) => match self.style {
                    Style::Aorus if self.now - since < Duration::from_millis(500) => return None,
                    Style::Aorus => 2,
                    _ => 4,
                },
                None => 1,
            };
            Some(Reading { current, max: 5 })
        }

        fn write(&mut self, write: Write) -> bool {
            let (code, value) = write.code_and_value();
            assert!(
                !(code == POWER && value == 5),
                "power mode 5 must never be sent"
            );
            self.writes.push((self.now, write));
            match write {
                _ if self.style == Style::Silent => false,
                Write::Power(Power::Standby) => {
                    if self.style != Style::Ignores {
                        self.asleep_since = Some(self.now);
                        self.waking_since = None;
                    }
                    true
                }
                Write::Power(Power::On) => {
                    if self.asleep_since.take().is_some() {
                        self.waking_since = Some(self.now);
                    }
                    true
                }
                Write::Brightness(value) => match &mut self.brightness {
                    Some(level) => {
                        *level = value;
                        true
                    }
                    None => false,
                },
            }
        }

        fn pause(&mut self, time: Duration) -> bool {
            self.now += time;
            let flagged = self
                .stop
                .as_ref()
                .is_some_and(|stop| stop.load(Ordering::Relaxed));
            !flagged && !self.stop_at.is_some_and(|at| self.now >= at)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::{Fake, Style};
    use super::*;

    #[test]
    fn power_mode_5_can_never_be_sent() {
        // The match has no catch-all, so a new mode stops this test from
        // compiling until it is added to the list.
        let listed = |power: Power| match power {
            Power::On | Power::Standby => power,
        };
        for power in [Power::On, Power::Standby].map(listed) {
            assert!(matches!(power.value(), 1 | 4), "{power:?}");
        }
        for value in [0, 1, 4, 5, 100] {
            assert_eq!(Write::Brightness(value).code_and_value().0, BRIGHTNESS);
        }
        // Every job against every kind of monitor, with the fake asserting
        // on each write that 5 never reaches code 0xD6.
        let jobs = [
            Job::Detect,
            Job::Standby,
            Job::Wake,
            Job::ReadBrightness,
            Job::SetBrightness(0),
            Job::SetBrightness(5),
            Job::Release {
                standby: true,
                brightness: Some(5),
            },
        ];
        for style in [Style::Philips, Style::Aorus, Style::Ignores, Style::Silent] {
            let mut monitor = Fake::new(style);
            for job in jobs {
                run(&mut monitor, job);
            }
        }
    }

    #[test]
    fn set_vcp_feature_is_called_in_one_place_only() {
        // A source scan, so a later change cannot write to a monitor while
        // going round `Write`, the type that keeps 5 out.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut calls = Vec::new();
        let mut pending = vec![root];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    let needle = ["SetVCP", "Feature("].concat();
                    for _ in text.matches(&needle) {
                        calls.push(path.clone());
                    }
                }
            }
        }
        assert_eq!(calls.len(), 1, "{calls:?}");
        assert!(calls[0].ends_with(std::path::Path::new("screen_dimmer").join("ddc.rs")));
    }

    #[test]
    fn the_philips_reports_standby_at_once() {
        let mut monitor = Fake::new(Style::Philips);
        assert_eq!(detect_standby(&mut monitor), Detected::Works);
        assert_eq!(monitor.now, DETECT_EVERY);
        assert_eq!(
            monitor.writes,
            [(Duration::ZERO, Write::Power(Power::Standby))]
        );
    }

    #[test]
    fn the_aorus_fails_its_reads_first_and_then_reports_2() {
        let mut monitor = Fake::new(Style::Aorus);
        assert_eq!(detect_standby(&mut monitor), Detected::Works);
        assert_eq!(monitor.now, Duration::from_millis(500));
    }

    #[test]
    fn the_mq780_keeps_reporting_on_and_gets_nothing_more() {
        let mut monitor = Fake::new(Style::Ignores);
        assert_eq!(detect_standby(&mut monitor), Detected::Ignored);
        assert_eq!(monitor.now, DETECT_FOR);
        assert_eq!(monitor.writes.len(), 1);
    }

    #[test]
    fn a_monitor_without_ddc_gets_nothing_at_all() {
        let mut monitor = Fake::new(Style::Silent);
        assert_eq!(detect_standby(&mut monitor), Detected::NoDdc);
        assert!(monitor.writes.is_empty());
        assert_eq!(monitor.now, Duration::ZERO);
    }

    struct Scripted {
        replies: Vec<Option<u32>>,
        now: Duration,
    }

    impl Monitor for Scripted {
        fn read(&mut self, _code: u8) -> Option<Reading> {
            if self.now.is_zero() {
                return Some(Reading { current: 1, max: 5 });
            }
            let index = (self.now.as_millis() / DETECT_EVERY.as_millis()) as usize - 1;
            self.replies
                .get(index)
                .copied()
                .flatten()
                .map(|current| Reading { current, max: 5 })
        }
        fn write(&mut self, _write: Write) -> bool {
            true
        }
        fn pause(&mut self, time: Duration) -> bool {
            self.now += time;
            true
        }
    }

    fn verdict(replies: &[Option<u32>]) -> Detected {
        detect_standby(&mut Scripted {
            replies: replies.to_vec(),
            now: Duration::ZERO,
        })
    }

    #[test]
    fn the_standby_verdict_from_read_back_sequences() {
        let none = None;
        // Reads that all fail after an answer before the request: asleep.
        assert_eq!(verdict(&[none; 8]), Detected::Works);
        // On at first, then another mode: asleep, even after failures.
        assert_eq!(verdict(&[Some(1), none, Some(4)]), Detected::Works);
        // On, then failures to the end: it said it was on, so not asleep.
        assert_eq!(
            verdict(&[Some(1), Some(1), none, none, none, none, none, none]),
            Detected::Ignored
        );
        assert_eq!(verdict(&[Some(1); 8]), Detected::Ignored);
        // A late change still counts while the 2 s last.
        let mut late = vec![Some(1); 7];
        late.push(Some(2));
        assert_eq!(verdict(&late), Detected::Works);
    }

    #[test]
    fn waking_the_philips_takes_two_seconds() {
        let mut monitor = Fake::new(Style::Philips);
        detect_standby(&mut monitor);
        let start = monitor.now;
        assert_eq!(wake(&mut monitor), Woke::On(Duration::from_secs(2)));
        assert!(!monitor.asleep());
        let ons: Vec<Duration> = monitor
            .writes
            .iter()
            .filter(|(_, write)| *write == Write::Power(Power::On))
            .map(|(at, _)| *at - start)
            .collect();
        // Asked at once and again after the first second without an answer.
        assert_eq!(ons, [Duration::ZERO, Duration::from_secs(1)]);
    }

    #[test]
    fn waking_the_aorus_waits_through_five_silent_seconds() {
        let mut monitor = Fake::new(Style::Aorus);
        detect_standby(&mut monitor);
        assert_eq!(wake(&mut monitor), Woke::On(Duration::from_secs(5)));
        let ons = monitor
            .writes
            .iter()
            .filter(|(_, write)| *write == Write::Power(Power::On))
            .count();
        assert_eq!(ons, 5);
    }

    #[test]
    fn a_monitor_that_never_answers_times_out_after_eight_seconds() {
        let mut monitor = Fake::new(Style::Silent);
        assert_eq!(wake(&mut monitor), Woke::TimedOut);
        assert_eq!(monitor.now, WAKE_FOR);
        // The first request and one after each of the first seven seconds.
        assert_eq!(monitor.writes.len(), 8);
    }

    #[test]
    fn stopping_cuts_a_wake_short() {
        let mut monitor = Fake::new(Style::Aorus);
        detect_standby(&mut monitor);
        monitor.stop_at = Some(monitor.now + Duration::from_secs(2));
        assert_eq!(wake(&mut monitor), Woke::Stopped);
    }

    #[test]
    fn release_sends_on_and_the_old_brightness_without_waiting() {
        let mut monitor = Fake::new(Style::Philips);
        let outcome = run(
            &mut monitor,
            Job::Release {
                standby: true,
                brightness: Some(55),
            },
        );
        assert_eq!(outcome, Outcome::Released(true));
        assert_eq!(monitor.now, Duration::ZERO);
        assert_eq!(monitor.brightness, Some(55));
        assert_eq!(monitor.writes[0].1, Write::Power(Power::On));
    }

    fn fake_opener(style: Style) -> Opener {
        Box::new(move |stop| {
            let mut monitor = Fake::new(style);
            monitor.stop = Some(stop);
            Some(Box::new(monitor) as Box<dyn Monitor>)
        })
    }

    #[test]
    fn a_worker_runs_jobs_in_order_and_reports_each() {
        let (reports, results) = mpsc::channel();
        let nudges = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = Arc::clone(&nudges);
        let nudge: Nudge = Arc::new(move || {
            counter.fetch_add(1, Ordering::Relaxed);
        });
        let mut worker =
            Worker::spawn("left", fake_opener(Style::Philips), reports, nudge).unwrap();
        worker.send(Job::Detect);
        worker.send(Job::Wake);
        let first: Vec<Report> = (0..2)
            .map(|_| results.recv_timeout(Duration::from_secs(5)).unwrap())
            .collect();
        worker.stop(Some(Job::Release {
            standby: true,
            brightness: None,
        }));
        assert!(wait_all(
            vec![worker],
            Instant::now() + Duration::from_secs(5)
        ));
        let mut got = first;
        got.extend(results.try_iter());
        let outcomes: Vec<Outcome> = got.iter().map(|report| report.outcome).collect();
        assert_eq!(
            outcomes,
            [
                Outcome::Detected(Detected::Works),
                Outcome::Woke(Woke::On(Duration::from_secs(2))),
                Outcome::Released(true),
            ]
        );
        assert!(got.iter().all(|report| report.monitor == "left"));
        assert_eq!(nudges.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn stopping_a_worker_cuts_its_waits_short() {
        let (reports, results) = mpsc::channel();
        let mut worker = Worker::spawn(
            "centre",
            fake_opener(Style::Aorus),
            reports,
            Arc::new(|| {}),
        )
        .unwrap();
        worker.stop(Some(Job::Wake));
        assert!(wait_all(
            vec![worker],
            Instant::now() + Duration::from_secs(5)
        ));
        assert_eq!(
            results.recv().unwrap().outcome,
            Outcome::Woke(Woke::Stopped)
        );
    }

    #[test]
    fn a_worker_without_a_handle_reports_no_monitor() {
        let (reports, results) = mpsc::channel();
        let open: Opener = Box::new(|_stop| None);
        let mut worker = Worker::spawn("right", open, reports, Arc::new(|| {})).unwrap();
        worker.send(Job::Standby);
        worker.stop(None);
        assert!(wait_all(
            vec![worker],
            Instant::now() + Duration::from_secs(5)
        ));
        assert_eq!(results.recv().unwrap().outcome, Outcome::NoMonitor);
    }
}
