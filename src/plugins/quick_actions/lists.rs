//! The actions that answer with rows to pick from: the programs on a
//! port, the sound output devices, window sizes, USB drives and keep-awake
//! times. `rows` builds the sub-list the palette shows; `pick` does what
//! the chosen row says. Both go through `System`.

use super::actions::{self, failed, on_off, Awake, Command, Outcome, Undo};
use super::drives::EjectError;
use super::geometry::Goal;
use super::ports;
use super::system::System;
use super::tools::size_text;
use crate::core::notice::Glyph;
use crate::core::ui_bridge::{ListRow, SubList};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    Ports,
    Output,
    Sizes,
    Drives,
    AwakeFor,
}

/// The sizes Window: Resize… always offers, before the user's own.
const BUILT_IN: [(i32, i32); 2] = [(1280, 720), (1920, 1080)];

/// Keep Awake for…: minutes, 0 for "until you turn it off".
const AWAKE_FOR: [u64; 4] = [30, 60, 120, 0];

impl ListKind {
    /// The inspector's line before Enter.
    pub fn hint(self) -> &'static str {
        match self {
            ListKind::Ports => "Enter lists the programs that listen on a port",
            ListKind::Output => "Enter lists the sound output devices",
            ListKind::Sizes => "Enter lists the sizes. Add your own on the plugin's page",
            ListKind::Drives => "Enter lists the USB drives and memory cards",
            ListKind::AwakeFor => "Enter lists how long to keep the PC awake",
        }
    }
}

/// "1600x900, 1366x768" into sizes; a part that is not a size is left out.
pub fn parse_sizes(text: &str) -> Vec<(i32, i32)> {
    text.split([',', ';'])
        .filter_map(|part| {
            let part = part.trim().to_lowercase().replace(['\u{d7}', '*'], "x");
            let (w, h) = part.split_once('x')?;
            let (w, h) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
            ((100..=10_000).contains(&w) && (100..=10_000).contains(&h)).then_some((w, h))
        })
        .collect()
}

/// The built-in sizes and then the user's, each once.
pub fn sizes(user: &[(i32, i32)]) -> Vec<(i32, i32)> {
    let mut all: Vec<(i32, i32)> = BUILT_IN.to_vec();
    for size in user {
        if !all.contains(size) {
            all.push(*size);
        }
    }
    all
}

/// The local time `minutes` after `now`, and whether it is tomorrow.
pub fn end_time(now: (u16, u16), minutes: u64) -> ((u16, u16), bool) {
    let start = u64::from(now.0) * 60 + u64::from(now.1);
    let end = start + minutes;
    let day = end % (24 * 60);
    (((day / 60) as u16, (day % 60) as u16), end >= 24 * 60)
}

fn hm((hour, minute): (u16, u16)) -> String {
    format!("{hour:02}:{minute:02}")
}

fn duration_title(minutes: u64) -> String {
    match minutes {
        0 => "Until you turn it off".to_string(),
        60 => "1 hour".to_string(),
        m if m % 60 == 0 => format!("{} hours", m / 60),
        m => format!("{m} minutes"),
    }
}

fn row(id: String, title: String, detail: String) -> ListRow {
    ListRow {
        id,
        title,
        detail,
        ..ListRow::default()
    }
}

pub fn rows(kind: ListKind, system: &dyn System, user_sizes: &[(i32, i32)]) -> SubList {
    match kind {
        ListKind::Ports => {
            let listeners = system.listeners();
            let found = ports::rows(&listeners, system.own_pid(), &|pid| {
                system.process_name(pid)
            });
            SubList {
                title: "Kill process on port".to_string(),
                rows: found
                    .iter()
                    .map(|port| {
                        let name = shown_name(&port.name);
                        ListRow {
                            confirm: format!("Press Enter again to end {name} (PID {})", port.pid),
                            ..row(
                                port.id(),
                                format!("Port {}", port.port),
                                format!(
                                    "{name} \u{b7} PID {} \u{b7} {}",
                                    port.pid,
                                    port.protocols()
                                ),
                            )
                        }
                    })
                    .collect(),
                empty: "No program listens on a TCP port".to_string(),
            }
        }
        ListKind::Output => {
            let (rows, empty) = match system.output_devices() {
                Ok(devices) => (
                    devices
                        .into_iter()
                        .map(|device| ListRow {
                            current: device.default,
                            ..row(device.id, device.name, String::new())
                        })
                        .collect(),
                    "No sound output device is connected".to_string(),
                ),
                Err(reason) => (Vec::new(), reason),
            };
            SubList {
                title: "Sound output".to_string(),
                rows,
                empty,
            }
        }
        ListKind::Sizes => {
            let mut rows: Vec<ListRow> = sizes(user_sizes)
                .into_iter()
                .map(|(w, h)| {
                    row(
                        format!("{w}x{h}"),
                        format!("{w} \u{d7} {h}"),
                        "Screen pixels, in the middle of the screen".to_string(),
                    )
                })
                .collect();
            rows.push(row(
                "80%".to_string(),
                "80 % of the monitor".to_string(),
                "In the middle of the screen".to_string(),
            ));
            SubList {
                title: "Resize the window".to_string(),
                rows,
                empty: String::new(),
            }
        }
        ListKind::Drives => SubList {
            title: "Eject".to_string(),
            rows: system
                .removable_drives()
                .into_iter()
                .map(|drive| {
                    row(
                        drive.letter.to_string(),
                        drive.title(),
                        size_text(drive.bytes),
                    )
                })
                .collect(),
            empty: "No USB drive or memory card is connected".to_string(),
        },
        ListKind::AwakeFor => {
            let now = system.local_time();
            SubList {
                title: "Keep awake".to_string(),
                rows: AWAKE_FOR
                    .iter()
                    .map(|minutes| {
                        let detail = if *minutes == 0 {
                            "Run Keep Awake again to stop".to_string()
                        } else {
                            let (end, tomorrow) = end_time(now, *minutes);
                            let day = if tomorrow { " tomorrow" } else { "" };
                            format!("Until {}{day}", hm(end))
                        };
                        row(minutes.to_string(), duration_title(*minutes), detail)
                    })
                    .collect(),
                empty: String::new(),
            }
        }
    }
}

fn shown_name(name: &str) -> String {
    if name.is_empty() {
        "a program".to_string()
    } else {
        name.to_string()
    }
}

pub fn pick(
    kind: ListKind,
    id: &str,
    system: &mut dyn System,
    awake: &mut Awake,
    now: u64,
) -> Option<Outcome> {
    Some(match kind {
        ListKind::Ports => {
            let (pid, port) = ports::parse_id(id)?;
            // Read again: the program may have stopped, and its PID been
            // given to another, since the list was shown.
            let listeners = system.listeners();
            let still = ports::rows(&listeners, system.own_pid(), &|pid| {
                system.process_name(pid)
            })
            .into_iter()
            .find(|row| row.pid == pid && row.port == port);
            let Some(found) = still else {
                return Some(Outcome::new(
                    Glyph::Info,
                    format!("Nothing listens on port {port} now"),
                    Some("The program has already stopped.".to_string()),
                ));
            };
            let name = shown_name(&found.name);
            match system.end_process(pid) {
                Ok(()) => {
                    log::info!("QuickActions: ended {name} (PID {pid}) on port {port}");
                    Outcome::new(
                        Glyph::Check,
                        format!("Ended {name}"),
                        Some(format!("It listened on port {port} (PID {pid})")),
                    )
                }
                Err(reason) => {
                    log::warn!("QuickActions: could not end {name} (PID {pid}): {reason}");
                    Outcome::new(
                        Glyph::Warning,
                        format!("Could not end {name}"),
                        Some(reason),
                    )
                }
            }
        }
        ListKind::Output => {
            let devices = match system.output_devices() {
                Ok(devices) => devices,
                Err(reason) => return Some(failed("The sound device", reason)),
            };
            let chosen = devices.iter().find(|device| device.id == id)?;
            let before = devices.iter().find(|device| device.default);
            if chosen.default {
                return Some(Outcome::new(
                    Glyph::Info,
                    format!("Sound already plays on {}", chosen.name),
                    None,
                ));
            }
            match system.set_output_device(id) {
                Ok(()) => {
                    log::info!("QuickActions: sound output switched to {}", chosen.name);
                    let outcome = Outcome::new(
                        Glyph::Check,
                        format!("Sound plays on {}", chosen.name),
                        before.map(|device| format!("It played on {} before", device.name)),
                    );
                    match before {
                        Some(device) => outcome.undo(Undo::Output(device.id.clone())),
                        None => outcome,
                    }
                }
                Err(reason) => {
                    log::warn!("QuickActions: switching the sound output failed: {reason}");
                    Outcome::new(
                        Glyph::Warning,
                        "The sound device did not change".to_string(),
                        Some(format!("{reason}. Change it in Sound settings.")),
                    )
                }
            }
        }
        ListKind::Sizes => {
            let goal = if id == "80%" {
                Goal::Share(80)
            } else {
                let (w, h) = parse_sizes(id).first().copied()?;
                Goal::Size(w, h)
            };
            return actions::run(Command::Window(goal), system, awake, &mut None, now);
        }
        ListKind::Drives => {
            let letter = id.chars().next()?;
            let name = system
                .removable_drives()
                .into_iter()
                .find(|drive| drive.letter == letter)
                .map(|drive| drive.title())
                .unwrap_or_else(|| format!("{letter}:"));
            match system.eject(letter) {
                Ok(()) => {
                    log::info!("QuickActions: ejected {name}");
                    Outcome::new(
                        Glyph::Check,
                        format!("{name} can be unplugged"),
                        Some("WinCraft ejected it safely.".to_string()),
                    )
                }
                Err(EjectError::InUse) => {
                    log::info!("QuickActions: {name} is in use, not ejected");
                    Outcome::new(
                        Glyph::Warning,
                        format!("{name} is in use"),
                        Some(
                            "Close the files and windows that use it, then try again.".to_string(),
                        ),
                    )
                }
                Err(EjectError::Other(reason)) => {
                    log::warn!("QuickActions: ejecting {name} failed: {reason}");
                    Outcome::new(
                        Glyph::Warning,
                        format!("{name} was not ejected"),
                        Some(reason),
                    )
                }
            }
        }
        ListKind::AwakeFor => {
            let minutes: u64 = id.parse().ok()?;
            let was = awake.is_on();
            if let Err(reason) = system.keep_awake(true) {
                return Some(failed("Keep awake", reason));
            }
            awake.since = Some(awake.since.unwrap_or(now));
            if minutes == 0 {
                awake.until = None;
                awake.ends_at = None;
            } else {
                awake.until = Some(now + minutes * 60_000);
                awake.ends_at = Some(end_time(system.local_time(), minutes).0);
            }
            log::info!(
                "QuickActions: keep awake {} for {}",
                on_off(true),
                duration_title(minutes)
            );
            let (text, detail) = match awake.ends_at {
                Some(end) => (
                    format!("Keep awake on until {}", hm(end)),
                    "Then the PC and the screen can sleep again.".to_string(),
                ),
                None => (
                    "Keep awake on".to_string(),
                    "The PC and the screen stay awake until you run it again.".to_string(),
                ),
            };
            Outcome::new(Glyph::Check, text, Some(detail)).undo(Undo::Awake(was))
        }
    })
}

/// Keep awake ran out: what the notice says.
pub fn awake_ended() -> Outcome {
    Outcome::new(
        Glyph::Info,
        "Keep awake ended".to_string(),
        Some("The time you set is up. The PC and the screen can sleep again.".to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::super::audio::Device;
    use super::super::drives::Drive;
    use super::super::ports::Listener;
    use super::super::system::tests::Fake;
    use super::*;

    fn device(id: &str, name: &str, default: bool) -> Device {
        Device {
            id: id.to_string(),
            name: name.to_string(),
            default,
        }
    }

    #[test]
    fn sizes_read_from_the_setting_and_join_the_built_in_ones() {
        assert_eq!(
            parse_sizes("1600x900, 1366 X 768; 800\u{d7}600, 50x50, junk, 1280x720"),
            [(1600, 900), (1366, 768), (800, 600), (1280, 720)]
        );
        assert!(parse_sizes("").is_empty());
        assert_eq!(
            sizes(&[(1600, 900), (1280, 720)]),
            [(1280, 720), (1920, 1080), (1600, 900)]
        );
    }

    #[test]
    fn the_end_time_wraps_past_midnight() {
        assert_eq!(end_time((14, 50), 30), ((15, 20), false));
        assert_eq!(end_time((23, 40), 60), ((0, 40), true));
        assert_eq!(duration_title(30), "30 minutes");
        assert_eq!(duration_title(60), "1 hour");
        assert_eq!(duration_title(120), "2 hours");
        assert_eq!(duration_title(0), "Until you turn it off");
        for kind in [
            ListKind::Ports,
            ListKind::Output,
            ListKind::Sizes,
            ListKind::Drives,
            ListKind::AwakeFor,
        ] {
            assert!(!kind.hint().is_empty());
        }
    }

    #[test]
    fn the_port_list_asks_twice_and_never_offers_wincraft() {
        let mut fake = Fake {
            listeners: vec![
                Listener {
                    port: 3000,
                    pid: 50,
                    v6: false,
                },
                Listener {
                    port: 8090,
                    pid: 99,
                    v6: false,
                },
            ],
            ..Fake::default()
        };
        let list = rows(ListKind::Ports, &fake, &[]);
        assert_eq!(list.rows.len(), 1, "WinCraft (PID 99) is left out");
        assert_eq!(list.rows[0].title, "Port 3000");
        assert_eq!(list.rows[0].detail, "app50.exe \u{b7} PID 50 \u{b7} IPv4");
        assert_eq!(
            list.rows[0].confirm,
            "Press Enter again to end app50.exe (PID 50)"
        );
        let mut awake = Awake::default();
        assert!(pick(ListKind::Ports, "99:8090", &mut fake, &mut awake, 0)
            .is_some_and(|o| o.text == "Nothing listens on port 8090 now"));
        let done = pick(ListKind::Ports, "50:3000", &mut fake, &mut awake, 0).expect("done");
        assert_eq!(done.text, "Ended app50.exe");
        assert_eq!(fake.calls(), ["end 50"]);
        let again = pick(ListKind::Ports, "50:3000", &mut fake, &mut awake, 0).expect("done");
        assert_eq!(again.text, "Nothing listens on port 3000 now");
        assert!(pick(ListKind::Ports, "junk", &mut fake, &mut awake, 0).is_none());
        assert_eq!(
            rows(ListKind::Ports, &fake, &[]).empty,
            "No program listens on a TCP port"
        );
    }

    #[test]
    fn a_refused_end_says_why_plainly() {
        let mut fake = Fake {
            listeners: vec![Listener {
                port: 5432,
                pid: 70,
                v6: true,
            }],
            deny_end: true,
            ..Fake::default()
        };
        let done = pick(
            ListKind::Ports,
            "70:5432",
            &mut fake,
            &mut Awake::default(),
            0,
        )
        .expect("done");
        assert_eq!(done.text, "Could not end app70.exe");
        assert!(done.detail.expect("a reason").contains("administrator"));
    }

    #[test]
    fn the_output_list_marks_the_device_in_use_and_undo_goes_back() {
        let mut fake = Fake {
            outputs: vec![device("a", "Speakers", true), device("b", "Headset", false)],
            ..Fake::default()
        };
        let list = rows(ListKind::Output, &fake, &[]);
        assert!(list.rows[0].current && !list.rows[1].current);
        let mut awake = Awake::default();
        let same = pick(ListKind::Output, "a", &mut fake, &mut awake, 0).expect("done");
        assert_eq!(same.text, "Sound already plays on Speakers");
        let done = pick(ListKind::Output, "b", &mut fake, &mut awake, 0).expect("done");
        assert_eq!(done.text, "Sound plays on Headset");
        assert_eq!(done.undo, Some(Undo::Output("a".to_string())));
        let back = actions::undo(
            done.undo.expect("undo"),
            &done.text,
            &mut fake,
            &mut awake,
            0,
        );
        assert_eq!(back.text, "Undone");
        assert!(fake.outputs[0].default);
        assert!(pick(ListKind::Output, "zz", &mut fake, &mut awake, 0).is_none());

        fake.refuse_output = true;
        let failed = pick(ListKind::Output, "b", &mut fake, &mut awake, 0).expect("done");
        assert_eq!(failed.text, "The sound device did not change");
        assert!(failed
            .detail
            .expect("a reason")
            .ends_with("Change it in Sound settings."));
        assert_eq!(rows(ListKind::Output, &Fake::default(), &[]).rows.len(), 0);
    }

    #[test]
    fn a_size_row_resizes_the_window_in_front() {
        let mut fake = Fake::default().with_window(7);
        let list = rows(ListKind::Sizes, &fake, &[(1600, 900)]);
        let ids: Vec<&str> = list.rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["1280x720", "1920x1080", "1600x900", "80%"]);
        let mut awake = Awake::default();
        let done = pick(ListKind::Sizes, "1600x900", &mut fake, &mut awake, 0).expect("done");
        assert_eq!(done.text, "Window resized to 1600 \u{d7} 900");
        let done = pick(ListKind::Sizes, "80%", &mut fake, &mut awake, 0).expect("done");
        assert_eq!(done.text, "Window resized to 1536 \u{d7} 825");
        assert!(pick(ListKind::Sizes, "big", &mut fake, &mut awake, 0).is_none());
    }

    #[test]
    fn a_drive_ejects_or_says_it_is_in_use() {
        let usb = Drive {
            letter: 'E',
            label: "SANDISK".to_string(),
            bytes: 32 * 1024 * 1024 * 1024,
        };
        let mut fake = Fake {
            drives: vec![usb.clone()],
            busy_drive: true,
            ..Fake::default()
        };
        let list = rows(ListKind::Drives, &fake, &[]);
        assert_eq!(list.rows[0].title, "E: SANDISK");
        assert_eq!(list.rows[0].detail, "32.0 GB");
        let mut awake = Awake::default();
        let busy = pick(ListKind::Drives, "E", &mut fake, &mut awake, 0).expect("done");
        assert_eq!(busy.text, "E: SANDISK is in use");
        fake.busy_drive = false;
        let done = pick(ListKind::Drives, "E", &mut fake, &mut awake, 0).expect("done");
        assert_eq!(done.text, "E: SANDISK can be unplugged");
        assert_eq!(fake.calls(), ["eject E"]);
        assert_eq!(
            rows(ListKind::Drives, &fake, &[]).empty,
            "No USB drive or memory card is connected"
        );
    }

    #[test]
    fn keep_awake_for_a_time_ends_at_that_time() {
        let mut fake = Fake::default();
        let list = rows(ListKind::AwakeFor, &fake, &[]);
        let titles: Vec<&str> = list.rows.iter().map(|row| row.title.as_str()).collect();
        assert_eq!(
            titles,
            ["30 minutes", "1 hour", "2 hours", "Until you turn it off"]
        );
        assert_eq!(list.rows[0].detail, "Until 15:20");
        let mut awake = Awake::default();
        let done = pick(ListKind::AwakeFor, "30", &mut fake, &mut awake, 1000).expect("done");
        assert_eq!(done.text, "Keep awake on until 15:20");
        assert!(fake.awake);
        assert_eq!(awake.until, Some(1000 + 30 * 60_000));
        assert_eq!(awake.ends_at, Some((15, 20)));
        assert_eq!(
            awake.line(1000 + 2 * 60_000).as_deref(),
            Some("On until 15:20, 28 min left")
        );
        let done = pick(ListKind::AwakeFor, "0", &mut fake, &mut awake, 5000).expect("done");
        assert_eq!(done.text, "Keep awake on");
        assert_eq!(awake.until, None);
        assert_eq!(awake.since, Some(1000), "it keeps its start");
        assert_eq!(awake_ended().text, "Keep awake ended");
    }
}
