//! What one monitor is doing, without any Win32: the plugin feeds it
//! hotkeys and worker reports, sends the job it returns to that monitor's
//! worker, and draws the overlay and the cursor fence from `look`.

use super::ddc::{Detected, Job, Outcome, Woke};
use super::saved::{Saved, Standby};

/// The mode chosen in settings for one monitor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// Off when standby works, else dim when brightness works, else cover.
    #[default]
    Auto,
    Off,
    Dim,
    Cover,
}

impl Mode {
    /// What the settings page shows and writes.
    pub const CHOICES: &'static [&'static str] = &["Auto", "Off", "Dim", "Cover"];

    /// Reads the settings page's words and the file's lower-case ones;
    /// "overlay" is another word for Cover.
    pub fn parse(text: &str) -> Mode {
        match text.trim().to_ascii_lowercase().as_str() {
            "off" => Mode::Off,
            "dim" => Mode::Dim,
            "cover" | "overlay" => Mode::Cover,
            _ => Mode::Auto,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Auto => Mode::CHOICES[0],
            Mode::Off => Mode::CHOICES[1],
            Mode::Dim => Mode::CHOICES[2],
            Mode::Cover => Mode::CHOICES[3],
        }
    }
}

/// What the monitor looks like to the user, which decides the overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    On,
    /// The overlay only, see-through under the pointer.
    Covered,
    /// The overlay, see-through under the pointer, and brightness 0.
    Dimmed,
    /// Standby with a solid overlay underneath and the cursor kept out.
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Detect,
    Standby,
    ReadBrightness,
    Darken,
    Wake,
    Restore,
}

impl Step {
    fn is_going_on(self) -> bool {
        matches!(self, Step::Wake | Step::Restore)
    }
}

pub struct Machine {
    pub saved: Saved,
    look: Look,
    step: Option<Step>,
    /// The user asked for the monitor back while it was still going off.
    wake_wanted: bool,
}

impl Machine {
    pub fn new(saved: Saved) -> Self {
        Self {
            saved,
            look: Look::On,
            step: None,
            wake_wanted: false,
        }
    }

    pub fn look(&self) -> Look {
        self.look
    }

    pub fn busy(&self) -> bool {
        self.step.is_some()
    }

    /// Whether a crash or a lost wake left this monitor in standby or
    /// dimmed while it now counts as on.
    pub fn needs_recovery(&self) -> bool {
        self.look == Look::On
            && self.step.is_none()
            && (self.saved.left_in_standby || self.saved.dimmed_from.is_some())
    }

    /// Puts back what a crash left behind: wakes the monitor, or gives it
    /// its brightness back. The overlay stays until it has.
    pub fn recover(&mut self) -> Option<Job> {
        if !self.needs_recovery() {
            return None;
        }
        if self.saved.left_in_standby {
            self.look = Look::Off;
            self.start(Step::Wake, Job::Wake)
        } else {
            self.look = Look::Dimmed;
            let from = self.saved.dimmed_from.unwrap_or_default();
            self.start(Step::Restore, Job::SetBrightness(from))
        }
    }

    /// The monitor's hotkey: off in `mode` when it is on, on otherwise.
    pub fn toggle(&mut self, mode: Mode) -> Option<Job> {
        if self.look == Look::On && self.step.is_none() {
            self.turn_off(mode)
        } else {
            self.wake()
        }
    }

    /// Back on, from whatever it is now. Nothing while it is already
    /// coming back.
    pub fn wake(&mut self) -> Option<Job> {
        match self.step {
            Some(step) if step.is_going_on() => return None,
            Some(_) => {
                self.wake_wanted = true;
                return None;
            }
            None => {}
        }
        match self.look {
            Look::On => None,
            Look::Covered => {
                self.look = Look::On;
                None
            }
            Look::Dimmed => self.restore(),
            Look::Off => self.start(Step::Wake, Job::Wake),
        }
    }

    fn turn_off(&mut self, mode: Mode) -> Option<Job> {
        let mode = match mode {
            Mode::Auto => match (self.saved.standby, self.saved.brightness) {
                (Some(Standby::Works), _) => Mode::Off,
                (Some(Standby::Ignored), Some(false)) | (Some(Standby::NoDdc), _) => Mode::Cover,
                (Some(Standby::Ignored), _) => Mode::Dim,
                (None, _) => {
                    self.saved.left_in_standby = true;
                    self.look = Look::Off;
                    return self.start(Step::Detect, Job::Detect);
                }
            },
            chosen => chosen,
        };
        match mode {
            Mode::Off => {
                self.saved.left_in_standby = true;
                self.look = Look::Off;
                self.start(Step::Standby, Job::Standby)
            }
            Mode::Dim => {
                self.look = Look::Dimmed;
                self.start(Step::ReadBrightness, Job::ReadBrightness)
            }
            _ => {
                self.look = Look::Covered;
                None
            }
        }
    }

    fn start(&mut self, step: Step, job: Job) -> Option<Job> {
        self.step = Some(step);
        Some(job)
    }

    fn restore(&mut self) -> Option<Job> {
        match self.saved.dimmed_from {
            Some(from) => self.start(Step::Restore, Job::SetBrightness(from)),
            None => {
                self.look = Look::On;
                None
            }
        }
    }

    /// Back on without anything to send, as when going off failed.
    fn give_up(&mut self, look: Look) -> Option<Job> {
        self.look = if std::mem::take(&mut self.wake_wanted) {
            Look::On
        } else {
            look
        };
        None
    }

    /// What the worker found; returns the next job, if any.
    pub fn report(&mut self, outcome: Outcome) -> Option<Job> {
        let step = self.step.take()?;
        if outcome == Outcome::NoMonitor {
            // Windows gave no handle, maybe only for now: nothing is learnt
            // about the monitor, and a record of standby is kept in case.
            if !step.is_going_on() {
                self.saved.left_in_standby = false;
                if step == Step::Darken {
                    self.saved.dimmed_from = None;
                }
            }
            return self.give_up(if step.is_going_on() {
                Look::On
            } else {
                Look::Covered
            });
        }
        match (step, outcome) {
            (Step::Detect, Outcome::Detected(found)) => match found {
                Detected::Works => {
                    self.saved.standby = Some(Standby::Works);
                    self.after_off()
                }
                Detected::Ignored => {
                    self.saved.standby = Some(Standby::Ignored);
                    self.saved.left_in_standby = false;
                    if std::mem::take(&mut self.wake_wanted) {
                        self.look = Look::On;
                        return None;
                    }
                    self.look = Look::Dimmed;
                    self.start(Step::ReadBrightness, Job::ReadBrightness)
                }
                Detected::NoDdc => {
                    self.saved.standby = Some(Standby::NoDdc);
                    self.saved.left_in_standby = false;
                    self.give_up(Look::Covered)
                }
                Detected::Stopped => None,
            },
            (Step::Standby, Outcome::StandbySent(true)) => self.after_off(),
            (Step::Standby, Outcome::StandbySent(false)) => {
                self.saved.left_in_standby = false;
                self.give_up(Look::Covered)
            }
            (Step::ReadBrightness, Outcome::Brightness(Some(reading))) => {
                self.saved.brightness = Some(true);
                if std::mem::take(&mut self.wake_wanted) {
                    self.look = Look::On;
                    return None;
                }
                self.saved.dimmed_from = Some(reading.current);
                self.start(Step::Darken, Job::SetBrightness(0))
            }
            (Step::ReadBrightness, Outcome::Brightness(None)) => {
                self.saved.brightness = Some(false);
                self.give_up(Look::Covered)
            }
            (Step::Darken, Outcome::BrightnessSet(true)) => {
                self.look = Look::Dimmed;
                if std::mem::take(&mut self.wake_wanted) {
                    return self.restore();
                }
                None
            }
            (Step::Darken, Outcome::BrightnessSet(false)) => {
                self.saved.dimmed_from = None;
                self.give_up(Look::Covered)
            }
            (Step::Wake, Outcome::Woke(woke)) => {
                if matches!(woke, Woke::On(_)) {
                    self.saved.left_in_standby = false;
                }
                if woke == Woke::Stopped {
                    return None;
                }
                // A timed-out wake keeps its record, so the next start asks
                // again; asking a monitor that is on changes nothing.
                self.look = Look::On;
                if self.saved.dimmed_from.is_some() {
                    self.look = Look::Dimmed;
                    return self.restore();
                }
                None
            }
            (Step::Restore, Outcome::BrightnessSet(ok)) => {
                if ok {
                    self.saved.dimmed_from = None;
                }
                self.look = Look::On;
                None
            }
            (step, outcome) => {
                log::warn!("screen_dimmer: {outcome:?} does not answer {step:?}; ignored");
                self.step = Some(step);
                None
            }
        }
    }

    fn after_off(&mut self) -> Option<Job> {
        self.look = Look::Off;
        if std::mem::take(&mut self.wake_wanted) {
            return self.start(Step::Wake, Job::Wake);
        }
        None
    }

    /// At exit, or when the plugin is switched off: what puts the monitor
    /// back at once, without waiting for it to answer. The monitor counts
    /// as on from here; `released` says whether the request went out.
    pub fn release(&mut self) -> Option<Job> {
        self.step = None;
        self.wake_wanted = false;
        self.look = Look::On;
        let standby = self.saved.left_in_standby;
        let brightness = self.saved.dimmed_from;
        (standby || brightness.is_some()).then_some(Job::Release {
            standby,
            brightness,
        })
    }

    /// A release that failed keeps the record, so the next start tries
    /// again.
    pub fn released(&mut self, ok: bool) {
        if ok {
            self.saved.left_in_standby = false;
            self.saved.dimmed_from = None;
        }
    }

    /// Forgets what testing found, so Auto tests the monitor again the next
    /// time it goes off.
    pub fn forget_tests(&mut self) {
        self.saved.standby = None;
        self.saved.brightness = None;
    }

    /// For the palette and the plugin's page.
    pub fn describe(&self) -> &'static str {
        match self.step {
            Some(Step::Detect | Step::Standby) => "turning off",
            Some(Step::ReadBrightness | Step::Darken) => "dimming",
            Some(Step::Wake | Step::Restore) => "waking",
            None => match self.look {
                Look::On => "on",
                Look::Covered => "covered",
                Look::Dimmed => "dimmed (brightness 0)",
                Look::Off => "off (standby)",
            },
        }
    }

    /// What testing found, for the settings page.
    pub fn capability(&self) -> &'static str {
        match self.saved.standby {
            Some(Standby::Works) => "standby works",
            Some(Standby::Ignored) => match self.saved.brightness {
                Some(false) => "standby ignored, no brightness control",
                _ => "standby ignored",
            },
            Some(Standby::NoDdc) => "no DDC/CI",
            None => "not tested yet",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::plugins::screen_dimmer::ddc::fake::{Fake, Style};
    use crate::plugins::screen_dimmer::ddc::{self, Reading};

    /// Runs the machine's jobs against a fake monitor until it has none.
    fn settle(machine: &mut Machine, mut job: Option<Job>, monitor: &mut Fake) -> Vec<Job> {
        let mut jobs = Vec::new();
        while let Some(next) = job {
            jobs.push(next);
            job = machine.report(ddc::run(monitor, next));
        }
        jobs
    }

    #[test]
    fn the_philips_goes_off_in_auto_and_is_remembered() {
        let mut monitor = Fake::new(Style::Philips);
        let mut machine = Machine::new(Saved::default());
        let job = machine.toggle(Mode::Auto);
        assert!(machine.saved.left_in_standby, "recorded before it is sent");
        assert_eq!(machine.look(), Look::Off);
        assert_eq!(settle(&mut machine, job, &mut monitor), [Job::Detect]);
        assert_eq!(machine.saved.standby, Some(Standby::Works));
        assert_eq!(machine.describe(), "off (standby)");

        let job = machine.toggle(Mode::Auto);
        assert_eq!(
            machine.look(),
            Look::Off,
            "the overlay stays while it wakes"
        );
        assert_eq!(settle(&mut machine, job, &mut monitor), [Job::Wake]);
        assert_eq!(machine.look(), Look::On);
        assert!(!machine.saved.left_in_standby);
        assert!(!monitor.asleep());

        // Known now: the next time it is not tested again.
        let job = machine.toggle(Mode::Auto);
        assert_eq!(job, Some(Job::Standby));
    }

    #[test]
    fn the_aorus_goes_off_in_auto_too() {
        let mut monitor = Fake::new(Style::Aorus);
        let mut machine = Machine::new(Saved::default());
        let job = machine.toggle(Mode::Auto);
        settle(&mut machine, job, &mut monitor);
        assert_eq!(machine.look(), Look::Off);
        assert_eq!(machine.saved.standby, Some(Standby::Works));
    }

    #[test]
    fn the_mq780_ignores_standby_and_is_dimmed_instead() {
        let mut monitor = Fake::new(Style::Ignores);
        let mut machine = Machine::new(Saved::default());
        let job = machine.toggle(Mode::Auto);
        let jobs = settle(&mut machine, job, &mut monitor);
        assert_eq!(
            jobs,
            [Job::Detect, Job::ReadBrightness, Job::SetBrightness(0)]
        );
        assert_eq!(machine.look(), Look::Dimmed);
        assert_eq!(machine.saved.standby, Some(Standby::Ignored));
        assert_eq!(machine.saved.brightness, Some(true));
        assert_eq!(machine.saved.dimmed_from, Some(70));
        assert!(!machine.saved.left_in_standby);
        assert_eq!(monitor.brightness, Some(0));

        let job = machine.toggle(Mode::Auto);
        assert_eq!(
            settle(&mut machine, job, &mut monitor),
            [Job::SetBrightness(70)]
        );
        assert_eq!(machine.look(), Look::On);
        assert_eq!(machine.saved.dimmed_from, None);
        assert_eq!(monitor.brightness, Some(70));

        // Known now: straight to dimming, no standby request.
        assert_eq!(machine.toggle(Mode::Auto), Some(Job::ReadBrightness));
    }

    #[test]
    fn a_monitor_without_ddc_is_only_covered() {
        let mut monitor = Fake::new(Style::Silent);
        let mut machine = Machine::new(Saved::default());
        let job = machine.toggle(Mode::Auto);
        settle(&mut machine, job, &mut monitor);
        assert_eq!(machine.look(), Look::Covered);
        assert_eq!(machine.saved.standby, Some(Standby::NoDdc));
        assert!(!machine.saved.left_in_standby);
        assert_eq!(machine.capability(), "no DDC/CI");
        assert_eq!(machine.toggle(Mode::Auto), None);
        assert_eq!(machine.look(), Look::On);
        assert_eq!(machine.toggle(Mode::Auto), None);
        assert_eq!(machine.look(), Look::Covered);
    }

    #[test]
    fn standby_ignored_without_brightness_falls_back_to_cover() {
        let mut machine = Machine::new(Saved::default());
        machine.toggle(Mode::Auto);
        let job = machine.report(Outcome::Detected(Detected::Ignored));
        assert_eq!(job, Some(Job::ReadBrightness));
        assert_eq!(machine.report(Outcome::Brightness(None)), None);
        assert_eq!(machine.look(), Look::Covered);
        assert_eq!(machine.saved.brightness, Some(false));
        machine.toggle(Mode::Auto);
        assert_eq!(machine.toggle(Mode::Auto), None, "remembered: cover only");
        assert_eq!(machine.look(), Look::Covered);
    }

    #[test]
    fn a_chosen_mode_wins_over_what_testing_found() {
        let works = Saved {
            standby: Some(Standby::Works),
            ..Saved::default()
        };
        let mut machine = Machine::new(works.clone());
        assert_eq!(machine.toggle(Mode::Cover), None);
        assert_eq!(machine.look(), Look::Covered);
        let mut machine = Machine::new(works.clone());
        assert_eq!(machine.toggle(Mode::Dim), Some(Job::ReadBrightness));
        let mut machine = Machine::new(Saved::default());
        assert_eq!(machine.toggle(Mode::Off), Some(Job::Standby));
        assert!(machine.saved.left_in_standby);
    }

    #[test]
    fn a_failed_standby_request_leaves_the_monitor_covered() {
        let mut machine = Machine::new(Saved::default());
        machine.toggle(Mode::Off);
        assert_eq!(machine.report(Outcome::StandbySent(false)), None);
        assert_eq!(machine.look(), Look::Covered);
        assert!(!machine.saved.left_in_standby);
    }

    #[test]
    fn the_hotkey_while_it_is_still_going_off_brings_it_back_after() {
        let mut machine = Machine::new(Saved::default());
        machine.toggle(Mode::Auto);
        assert_eq!(machine.toggle(Mode::Auto), None);
        assert_eq!(machine.describe(), "turning off");
        let job = machine.report(Outcome::Detected(Detected::Works));
        assert_eq!(job, Some(Job::Wake));
        assert_eq!(
            machine.report(Outcome::Woke(Woke::On(Duration::from_secs(2)))),
            None
        );
        assert_eq!(machine.look(), Look::On);
        // And while it is dimming.
        let mut machine = Machine::new(Saved::default());
        machine.toggle(Mode::Dim);
        machine.toggle(Mode::Dim);
        let reading = Reading {
            current: 40,
            max: 100,
        };
        assert_eq!(machine.report(Outcome::Brightness(Some(reading))), None);
        assert_eq!(machine.look(), Look::On);
        assert_eq!(machine.saved.dimmed_from, None, "it was never darkened");
    }

    #[test]
    fn pressing_again_while_it_wakes_changes_nothing() {
        let mut machine = Machine::new(Saved {
            standby: Some(Standby::Works),
            ..Saved::default()
        });
        machine.toggle(Mode::Auto);
        machine.report(Outcome::StandbySent(true));
        assert_eq!(machine.toggle(Mode::Auto), Some(Job::Wake));
        assert_eq!(machine.toggle(Mode::Auto), None);
        assert_eq!(machine.wake(), None);
        assert_eq!(machine.describe(), "waking");
    }

    #[test]
    fn a_wake_that_times_out_removes_the_overlay_and_keeps_the_record() {
        let mut machine = Machine::new(Saved {
            standby: Some(Standby::Works),
            ..Saved::default()
        });
        machine.toggle(Mode::Auto);
        machine.report(Outcome::StandbySent(true));
        machine.toggle(Mode::Auto);
        assert_eq!(machine.report(Outcome::Woke(Woke::TimedOut)), None);
        assert_eq!(machine.look(), Look::On);
        assert!(machine.saved.left_in_standby);
        assert!(machine.needs_recovery());
    }

    #[test]
    fn a_start_after_a_crash_wakes_the_monitor_or_restores_its_brightness() {
        let mut monitor = Fake::new(Style::Aorus);
        ddc::run(&mut monitor, Job::Standby);
        let mut machine = Machine::new(Saved {
            standby: Some(Standby::Works),
            left_in_standby: true,
            ..Saved::default()
        });
        let job = machine.recover();
        assert_eq!(machine.look(), Look::Off);
        assert_eq!(settle(&mut machine, job, &mut monitor), [Job::Wake]);
        assert_eq!(machine.look(), Look::On);
        assert!(!machine.saved.left_in_standby);
        assert!(!monitor.asleep());

        let mut monitor = Fake::new(Style::Ignores);
        ddc::run(&mut monitor, Job::SetBrightness(0));
        let mut machine = Machine::new(Saved {
            dimmed_from: Some(65),
            ..Saved::default()
        });
        let job = machine.recover();
        assert_eq!(
            settle(&mut machine, job, &mut monitor),
            [Job::SetBrightness(65)]
        );
        assert_eq!(monitor.brightness, Some(65));
        assert!(machine.saved.is_empty());
        assert_eq!(machine.recover(), None);
    }

    #[test]
    fn exit_puts_back_whatever_is_off_or_dimmed_even_mid_job() {
        let mut monitor = Fake::new(Style::Aorus);
        let mut machine = Machine::new(Saved::default());
        let job = machine.toggle(Mode::Auto);
        // Exit arrives while the standby test is still running.
        ddc::run(&mut monitor, job.unwrap());
        let release = machine.release();
        assert_eq!(
            release,
            Some(Job::Release {
                standby: true,
                brightness: None
            })
        );
        assert_eq!(
            ddc::run(&mut monitor, release.unwrap()),
            Outcome::Released(true)
        );
        machine.released(true);
        assert!(!machine.saved.left_in_standby);
        assert_eq!(machine.look(), Look::On);

        let mut machine = Machine::new(Saved {
            dimmed_from: Some(80),
            ..Saved::default()
        });
        assert_eq!(
            machine.release(),
            Some(Job::Release {
                standby: false,
                brightness: Some(80)
            })
        );
        machine.released(false);
        assert_eq!(
            machine.saved.dimmed_from,
            Some(80),
            "kept for the next start"
        );
        assert_eq!(Machine::new(Saved::default()).release(), None);
    }

    #[test]
    fn no_handle_from_windows_learns_nothing() {
        let mut machine = Machine::new(Saved::default());
        machine.toggle(Mode::Auto);
        assert_eq!(machine.report(Outcome::NoMonitor), None);
        assert_eq!(machine.look(), Look::Covered);
        assert_eq!(machine.saved, Saved::default());
    }

    #[test]
    fn testing_again_forgets_only_what_was_found() {
        let mut machine = Machine::new(Saved {
            standby: Some(Standby::Ignored),
            brightness: Some(true),
            dimmed_from: Some(30),
            left_in_standby: false,
        });
        machine.forget_tests();
        assert_eq!(machine.saved.standby, None);
        assert_eq!(machine.saved.brightness, None);
        assert_eq!(machine.saved.dimmed_from, Some(30));
        assert_eq!(machine.capability(), "not tested yet");
    }

    #[test]
    fn modes_read_the_page_and_the_file_words() {
        for (text, mode) in [
            ("Auto", Mode::Auto),
            ("off", Mode::Off),
            ("Dim", Mode::Dim),
            ("Cover", Mode::Cover),
            ("overlay", Mode::Cover),
            ("", Mode::Auto),
            ("nonsense", Mode::Auto),
        ] {
            assert_eq!(Mode::parse(text), mode, "{text}");
        }
        for mode in [Mode::Auto, Mode::Off, Mode::Dim, Mode::Cover] {
            assert_eq!(Mode::parse(mode.label()), mode);
        }
    }
}
