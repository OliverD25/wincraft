//! `--ddc-test=<value>`, for a supervised check on real monitors with the
//! same worker and procedures the plugin uses. Only a test instance takes
//! it, and it prints every step:
//!
//! - `list` reads the power mode and brightness of every monitor and
//!   writes nothing;
//! - `left`, `centre` or `right` runs the automatic choice on that monitor,
//!   keeps it off (or dimmed) for 10 s and brings it back;
//! - `wake-left` and so on only wakes that monitor.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::ddc::{self, Detected, Job, Monitor, Opener, Outcome, Physical, Reading, Worker, Write};
use super::screens::{self, Screen};

const HOLD: Duration = Duration::from_secs(10);
/// Longer than the slowest job, a wake of 8 s.
const JOB_TIMEOUT: Duration = Duration::from_secs(20);

pub fn run(value: &str) -> i32 {
    let screens = screens::list();
    let mut out = |line: &str| {
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), "{line}");
    };
    for screen in &screens {
        out(&describe(screen));
    }
    let value = value.trim().to_ascii_lowercase();
    if value == "list" {
        for screen in &screens {
            read_only(screen, &mut out);
        }
        return 0;
    }
    let (wake_only, place) = match value.strip_prefix("wake-") {
        Some(place) => (true, place.to_string()),
        None => (false, value.clone()),
    };
    let place = if place == "center" {
        "centre".to_string()
    } else {
        place
    };
    let Some(screen) = screens
        .iter()
        .find(|screen| screen.place.as_deref() == Some(place.as_str()))
    else {
        out(&format!(
            "no monitor at \"{place}\"; use list, left, centre, right or wake-<place>"
        ));
        return 1;
    };
    out(&format!("testing {}", screen.id));
    let (lines, printed) = mpsc::channel();
    let handle = screen.handle as isize;
    let start = Instant::now();
    let open: Opener = Box::new(move |stop| {
        let inner = Physical::open(handle, stop)?;
        Some(Box::new(Traced {
            inner,
            start,
            lines,
        }) as Box<dyn Monitor>)
    });
    drive(&screen.id, open, wake_only, HOLD, &printed, &mut out)
}

fn describe(screen: &Screen) -> String {
    let rect = screen.rect;
    format!(
        "monitor {} (EDID name \"{}\") at {} ({}, {}) - ({}, {}){}",
        screen.id,
        screen.name,
        screen.place.as_deref().unwrap_or("the only place"),
        rect.left,
        rect.top,
        rect.right,
        rect.bottom,
        if screen.primary { ", primary" } else { "" }
    )
}

fn read_only(screen: &Screen, out: &mut dyn FnMut(&str)) {
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let Some(mut monitor) = Physical::open(screen.handle as isize, stop) else {
        out(&format!("{}: Windows gave no DDC/CI handle", screen.id));
        return;
    };
    let power = monitor.read(ddc::POWER);
    let brightness = monitor.read(ddc::BRIGHTNESS);
    out(&format!(
        "{} (\"{}\"): power mode {}, brightness {}",
        screen.id,
        monitor.description(),
        reading(power),
        reading(brightness)
    ));
}

fn reading(value: Option<Reading>) -> String {
    match value {
        Some(reading) => format!("{} of {}", reading.current, reading.max),
        None => "no answer".to_string(),
    }
}

/// The test itself, against any monitor, so it can be tried on a fake one.
pub fn drive(
    id: &str,
    open: Opener,
    wake_only: bool,
    hold: Duration,
    printed: &Receiver<String>,
    out: &mut dyn FnMut(&str),
) -> i32 {
    let (reports, results) = mpsc::channel();
    let Some(mut worker) = Worker::spawn(id, open, reports, Arc::new(|| {})) else {
        out("could not start the DDC/CI thread");
        return 1;
    };
    let job = |job: Job, out: &mut dyn FnMut(&str)| -> Option<Outcome> {
        out(&format!("job {job:?}"));
        worker.send(job);
        let outcome = wait(&results, printed, out);
        match outcome {
            Some(outcome) => out(&format!("  -> {outcome:?}")),
            None => out("  -> no report in time"),
        }
        outcome
    };
    let code = if wake_only {
        match job(Job::Wake, out) {
            Some(Outcome::Woke(ddc::Woke::On(_))) => 0,
            _ => 1,
        }
    } else {
        match job(Job::Detect, out) {
            Some(Outcome::Detected(Detected::Works)) => {
                out(&format!("standby works; keeping it off for {hold:?}"));
                std::thread::sleep(hold);
                match job(Job::Wake, out) {
                    Some(Outcome::Woke(ddc::Woke::On(_))) => 0,
                    _ => 1,
                }
            }
            Some(Outcome::Detected(Detected::Ignored)) => {
                out("standby ignored; dimming instead");
                match job(Job::ReadBrightness, out) {
                    Some(Outcome::Brightness(Some(old))) => {
                        job(Job::SetBrightness(0), out);
                        out(&format!("dimmed; keeping it for {hold:?}"));
                        std::thread::sleep(hold);
                        match job(Job::SetBrightness(old.current), out) {
                            Some(Outcome::BrightnessSet(true)) => 0,
                            _ => 1,
                        }
                    }
                    _ => {
                        out("no brightness either; the plugin would only cover it");
                        0
                    }
                }
            }
            _ => {
                out("nothing was changed; the plugin would only cover it");
                0
            }
        }
    };
    worker.stop(None);
    ddc::wait_all(vec![worker], Instant::now() + Duration::from_secs(2));
    for line in printed.try_iter() {
        out(&line);
    }
    code
}

fn wait(
    results: &Receiver<ddc::Report>,
    printed: &Receiver<String>,
    out: &mut dyn FnMut(&str),
) -> Option<Outcome> {
    let end = Instant::now() + JOB_TIMEOUT;
    while Instant::now() < end {
        for line in printed.try_iter() {
            out(&line);
        }
        if let Ok(report) = results.recv_timeout(Duration::from_millis(50)) {
            for line in printed.try_iter() {
                out(&line);
            }
            return Some(report.outcome);
        }
    }
    None
}

/// Prints every read and write with the time since the test started.
pub struct Traced<M: Monitor> {
    pub inner: M,
    pub start: Instant,
    pub lines: Sender<String>,
}

impl<M: Monitor> Monitor for Traced<M> {
    fn read(&mut self, code: u8) -> Option<Reading> {
        let value = self.inner.read(code);
        let _ = self.lines.send(format!(
            "  {:>6.2} s  read  {code:02X} -> {}",
            self.start.elapsed().as_secs_f64(),
            reading(value)
        ));
        value
    }

    fn write(&mut self, write: Write) -> bool {
        let ok = self.inner.write(write);
        let (code, value) = write.code_and_value();
        let _ = self.lines.send(format!(
            "  {:>6.2} s  write {code:02X} = {value} -> {}",
            self.start.elapsed().as_secs_f64(),
            if ok { "ok" } else { "failed" }
        ));
        ok
    }

    fn pause(&mut self, time: Duration) -> bool {
        self.inner.pause(time)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::plugins::screen_dimmer::ddc::fake::{Fake, Style};

    struct Shared(Arc<Mutex<Fake>>);

    impl Monitor for Shared {
        fn read(&mut self, code: u8) -> Option<Reading> {
            self.0.lock().unwrap().read(code)
        }
        fn write(&mut self, write: Write) -> bool {
            self.0.lock().unwrap().write(write)
        }
        fn pause(&mut self, time: Duration) -> bool {
            self.0.lock().unwrap().pause(time)
        }
    }

    fn test(style: Style, wake_only: bool) -> (i32, Vec<String>, Arc<Mutex<Fake>>) {
        let fake = Arc::new(Mutex::new(Fake::new(style)));
        let shared = Arc::clone(&fake);
        let (lines, printed) = mpsc::channel();
        let start = Instant::now();
        let open: Opener = Box::new(move |_stop| {
            Some(Box::new(Traced {
                inner: Shared(shared),
                start,
                lines,
            }) as Box<dyn Monitor>)
        });
        let mut output = Vec::new();
        let code = drive(
            "test",
            open,
            wake_only,
            Duration::ZERO,
            &printed,
            &mut |line: &str| output.push(line.to_string()),
        );
        (code, output, fake)
    }

    #[test]
    fn a_monitor_whose_standby_works_goes_off_and_wakes() {
        let (code, output, fake) = test(Style::Aorus, false);
        assert_eq!(code, 0, "{output:#?}");
        assert!(!fake.lock().unwrap().asleep());
        assert!(output
            .iter()
            .any(|line| line.contains("write D6 = 4 -> ok")));
        assert!(output.iter().any(|line| line.starts_with("standby works")));
    }

    #[test]
    fn a_monitor_that_ignores_standby_is_dimmed_and_gets_its_brightness_back() {
        let (code, output, fake) = test(Style::Ignores, false);
        assert_eq!(code, 0, "{output:#?}");
        let fake = fake.lock().unwrap();
        assert_eq!(fake.brightness, Some(70));
        assert!(fake.writes.contains(&(fake.now, Write::Brightness(0))));
        assert!(output
            .iter()
            .any(|line| line.contains("write 10 = 70 -> ok")));
    }

    #[test]
    fn a_monitor_without_ddc_is_left_alone() {
        let (code, output, fake) = test(Style::Silent, false);
        assert_eq!(code, 0, "{output:#?}");
        assert!(fake.lock().unwrap().writes.is_empty());
    }

    #[test]
    fn wake_only_sends_on() {
        let (code, output, fake) = test(Style::Philips, true);
        assert_eq!(code, 0, "{output:#?}");
        assert_eq!(
            fake.lock().unwrap().writes[0].1,
            Write::Power(ddc::Power::On)
        );
    }
}
