//! `--ddc-test=<value>`, for a supervised check on real monitors with the
//! same worker and procedures the plugin uses. Only a test instance takes
//! it, and it prints every step:
//!
//! - `list` reads the power mode and brightness of every monitor and
//!   writes nothing;
//! - `left`, `centre` or `right` turns that monitor off the way the Auto
//!   mode does (standby, or brightness 0 when it ignores standby), keeps
//!   it so for 10 s and brings it back;
//! - `wake-left` and so on only wakes that monitor.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::ddc::{self, Job, Monitor, Opener, Outcome, Physical, Reading, Worker, Write};
use super::machine::{Look, Machine, Mode};
use super::saved::Saved;
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
/// The plugin's own state machine decides every step.
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
    let mut machine = Machine::new(Saved {
        left_in_standby: wake_only,
        ..Saved::default()
    });
    let first = if wake_only {
        out("waking it");
        machine.recover()
    } else {
        out("turning it off the way Auto does");
        machine.toggle(Mode::Auto)
    };
    let mut ok = settle(&worker, &results, printed, &mut machine, first, out);
    if ok && !wake_only {
        match machine.look() {
            Look::Off | Look::Dimmed => {
                out(&format!("keeping it {} for {hold:?}", machine.describe()));
                std::thread::sleep(hold);
                out("bringing it back");
                let job = machine.toggle(Mode::Auto);
                ok = settle(&worker, &results, printed, &mut machine, job, out);
            }
            Look::Covered => {
                out("the plugin would only cover this monitor; nothing was changed");
                machine.toggle(Mode::Auto);
            }
            Look::On => {}
        }
    }
    worker.stop(None);
    ddc::wait_all(vec![worker], Instant::now() + Duration::from_secs(2));
    for line in printed.try_iter() {
        out(&line);
    }
    let back = machine.look() == Look::On
        && !machine.saved.left_in_standby
        && machine.saved.dimmed_from.is_none();
    out(&format!(
        "done: {} ({}); {}",
        machine.describe(),
        machine.capability(),
        if back {
            "back as it was"
        } else {
            "NOT back as it was"
        }
    ));
    i32::from(!(ok && back))
}

/// Runs the machine's jobs one by one until it asks for no more. False when
/// a job got no report in time.
fn settle(
    worker: &Worker,
    results: &Receiver<ddc::Report>,
    printed: &Receiver<String>,
    machine: &mut Machine,
    mut job: Option<Job>,
    out: &mut dyn FnMut(&str),
) -> bool {
    while let Some(next) = job {
        out(&format!("job {next:?}"));
        worker.send(next);
        let Some(outcome) = wait(results, printed, out) else {
            out("  -> no report in time");
            return false;
        };
        job = machine.report(outcome);
        out(&format!("  -> {outcome:?}; now {}", machine.describe()));
    }
    true
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
        assert!(output
            .iter()
            .any(|line| line.starts_with("keeping it off (standby)")));
        assert!(output.last().unwrap().ends_with("back as it was"));
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
        assert!(output.iter().any(|line| line.contains("only cover")));
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
