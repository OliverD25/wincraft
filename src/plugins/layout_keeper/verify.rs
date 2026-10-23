//! The check a few seconds after a restore: is every window on the desktop
//! and the monitor the restore put it on? Re-adding a taskbar button pulls
//! its window onto the current desktop a moment later, not at once, and an
//! app can still move its own windows after the restore, so a move that
//! reported success is not proof. The plugin reads where each window is and
//! hands it here; this module only compares and words the result.

use super::desktops::DesktopId;
use super::order::Handle;

/// Where a restore wanted one window.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub hwnd: Handle,
    pub group: String,
    pub label: String,
    /// None: the desktop is not checked (pinned, on no desktop, or desktop
    /// moves are off).
    pub desktop: Option<DesktopId>,
    /// The monitor's device name, such as "\\.\DISPLAY1"; None: not checked
    /// (the saved monitor is unplugged, or monitors are not restored).
    pub monitor: Option<String>,
}

/// Where a window is now.
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub desktop: Option<DesktopId>,
    pub monitor: Option<String>,
}

/// One window that is not where it was wanted: the place in the target list,
/// and for each of desktop and monitor that is wrong, (wanted, now).
#[derive(Clone, Debug, PartialEq)]
pub struct Wrong {
    pub index: usize,
    pub desktop: Option<(DesktopId, Option<DesktopId>)>,
    pub monitor: Option<(String, Option<String>)>,
}

/// The windows not in place. `seen` follows `targets`; None is a window that
/// is no longer open, which is not wrong, only gone.
pub fn wrong(targets: &[Target], seen: &[Option<Seen>]) -> Vec<Wrong> {
    targets
        .iter()
        .zip(seen)
        .enumerate()
        .filter_map(|(index, (target, seen))| {
            let seen = seen.as_ref()?;
            let desktop = target
                .desktop
                .filter(|wanted| seen.desktop != Some(*wanted))
                .map(|wanted| (wanted, seen.desktop));
            let monitor = target
                .monitor
                .as_ref()
                .filter(|wanted| seen.monitor.as_ref() != Some(*wanted))
                .map(|wanted| (wanted.clone(), seen.monitor.clone()));
            (desktop.is_some() || monitor.is_some()).then_some(Wrong {
                index,
                desktop,
                monitor,
            })
        })
        .collect()
}

/// How one group came out of the check.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tally {
    pub checked: usize,
    pub gone: usize,
    pub wrong: usize,
}

/// The groups of `targets` in their first order, each with its tally.
pub fn tallies(targets: &[Target], seen: &[Option<Seen>], wrong: &[Wrong]) -> Vec<(String, Tally)> {
    let mut all: Vec<(String, Tally)> = Vec::new();
    for (index, target) in targets.iter().enumerate() {
        let at = match all.iter().position(|(group, _)| *group == target.group) {
            Some(at) => at,
            None => {
                all.push((target.group.clone(), Tally::default()));
                all.len() - 1
            }
        };
        let tally = &mut all[at].1;
        tally.checked += 1;
        if seen.get(index).is_none_or(Option::is_none) {
            tally.gone += 1;
        } else if wrong.iter().any(|w| w.index == index) {
            tally.wrong += 1;
        }
    }
    all
}

/// The summary line of one group. `retried` says whether the windows that
/// were out of place have already been moved a second time.
pub fn group_line(label: &str, tally: &Tally, retried: usize) -> String {
    let open = tally.checked - tally.gone;
    let mut line = if tally.wrong == 0 {
        format!("check after the restore: all {open} {label} windows are on their desktops and monitors")
    } else {
        format!(
            "check after the restore: {} of {open} {label} windows are not on their desktops or monitors",
            tally.wrong
        )
    };
    if retried > 0 {
        line.push_str(&format!(" ({retried} were moved a second time)"));
    }
    if tally.gone > 0 {
        line.push_str(&format!("; {} closed since", tally.gone));
    }
    line
}

/// One line for a window still out of place, with what it wants and where
/// it is, by name and id. `desktop_name` names a desktop.
pub fn window_line(
    target: &Target,
    wrong: &Wrong,
    desktop_name: &dyn Fn(DesktopId) -> String,
) -> String {
    let desktop = |id: Option<DesktopId>| match id {
        Some(id) => format!("{} {id}", desktop_name(id)),
        None => "no readable desktop".to_string(),
    };
    let mut parts = Vec::new();
    if let Some((wanted, now)) = &wrong.desktop {
        parts.push(format!(
            "wants desktop {}, is on {}",
            desktop(Some(*wanted)),
            desktop(*now)
        ));
    }
    if let Some((wanted, now)) = &wrong.monitor {
        parts.push(format!(
            "wants monitor {wanted}, is on {}",
            now.as_deref().unwrap_or("no monitor")
        ));
    }
    format!(
        "still out of place: \"{}\" {}",
        target.label,
        parts.join("; ")
    )
}

/// One window of an applied taskbar order: (label, wanted desktop, desktop
/// now).
pub type Placed = (String, Option<DesktopId>, Option<DesktopId>);

/// The order a group's windows were put in, split by desktop: for each
/// desktop, in the order desktops first appear, the windows wanted there and
/// the windows on it now, both in taskbar order. Each entry of `applied` is
/// (label, wanted desktop, desktop now); a window with no wanted desktop
/// counts only where it is now.
pub fn desktop_orders(applied: &[Placed]) -> Vec<(DesktopId, Vec<String>, Vec<String>)> {
    let mut desktops: Vec<DesktopId> = Vec::new();
    for (_, wanted, now) in applied {
        for id in [wanted, now].into_iter().flatten() {
            if !desktops.contains(id) {
                desktops.push(*id);
            }
        }
    }
    desktops
        .into_iter()
        .map(|desktop| {
            let on = |pick: &dyn Fn(&Placed) -> bool| {
                applied
                    .iter()
                    .filter(|entry| pick(entry))
                    .map(|(label, _, _)| label.clone())
                    .collect::<Vec<_>>()
            };
            let wanted = on(&|(_, wanted, _)| *wanted == Some(desktop));
            let now = on(&|(_, _, now)| *now == Some(desktop));
            (desktop, wanted, now)
        })
        .collect()
}

/// The log line for one desktop of `desktop_orders`.
pub fn order_line(label: &str, desktop: &str, wanted: &[String], now: &[String]) -> String {
    if wanted == now {
        format!(
            "taskbar order of {label} on {desktop}, as wanted: {}",
            now.join(" | ")
        )
    } else {
        format!(
            "taskbar order of {label} on {desktop}: wanted {}; now {}",
            list_or_none(wanted),
            list_or_none(now)
        )
    }
}

fn list_or_none(labels: &[String]) -> String {
    if labels.is_empty() {
        "none".to_string()
    } else {
        labels.join(" | ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: DesktopId = DesktopId(1);
    const WORK: DesktopId = DesktopId(2);

    fn target(hwnd: Handle, label: &str, desktop: Option<DesktopId>, monitor: &str) -> Target {
        Target {
            hwnd,
            group: "Chrome".to_string(),
            label: label.to_string(),
            desktop,
            monitor: (!monitor.is_empty()).then(|| monitor.to_string()),
        }
    }

    fn seen(desktop: Option<DesktopId>, monitor: &str) -> Option<Seen> {
        Some(Seen {
            desktop,
            monitor: Some(monitor.to_string()),
        })
    }

    #[test]
    fn only_a_window_on_another_desktop_or_monitor_is_wrong() {
        let targets = [
            target(1, "Home", Some(HOME), "D1"),
            target(2, "Reading", Some(WORK), "D1"),
            target(3, "Mail", Some(WORK), "D2"),
            target(4, "Closed", Some(HOME), "D1"),
            target(5, "Pinned", None, ""),
        ];
        let seen = [
            seen(Some(HOME), "D1"),
            seen(Some(HOME), "D1"),
            seen(None, "D1"),
            None,
            seen(Some(DesktopId::ALL), "D3"),
        ];
        let wrong = wrong(&targets, &seen);
        assert_eq!(
            wrong,
            [
                Wrong {
                    index: 1,
                    desktop: Some((WORK, Some(HOME))),
                    monitor: None,
                },
                Wrong {
                    index: 2,
                    desktop: Some((WORK, None)),
                    monitor: Some(("D2".to_string(), Some("D1".to_string()))),
                },
            ]
        );
        assert_eq!(
            tallies(&targets, &seen, &wrong),
            [(
                "Chrome".to_string(),
                Tally {
                    checked: 5,
                    gone: 1,
                    wrong: 2
                }
            )]
        );
    }

    #[test]
    fn the_lines_say_what_is_wrong_by_name_and_id() {
        let names = |id: DesktopId| if id == HOME { "Home" } else { "Work" }.to_string();
        let all_right = Tally {
            checked: 3,
            gone: 0,
            wrong: 0,
        };
        assert_eq!(
            group_line("Chrome", &all_right, 0),
            "check after the restore: all 3 Chrome windows are on their desktops and monitors"
        );
        let two_wrong = Tally {
            checked: 4,
            gone: 1,
            wrong: 2,
        };
        assert_eq!(
            group_line("Chrome", &two_wrong, 2),
            "check after the restore: 2 of 3 Chrome windows are not on their desktops or monitors (2 were moved a second time); 1 closed since"
        );
        let line = window_line(
            &target(2, "Reading", Some(WORK), "D1"),
            &Wrong {
                index: 0,
                desktop: Some((WORK, Some(HOME))),
                monitor: Some(("D1".to_string(), None)),
            },
            &names,
        );
        assert_eq!(
            line,
            format!(
                "still out of place: \"Reading\" wants desktop Work {WORK}, is on Home {HOME}; wants monitor D1, is on no monitor"
            )
        );
    }

    #[test]
    fn the_order_is_split_by_the_desktop_wanted_and_the_desktop_now() {
        let entry = |label: &str, wanted, now| (label.to_string(), wanted, now);
        let applied = [
            entry("Home", Some(HOME), Some(HOME)),
            entry("Mail", Some(WORK), Some(WORK)),
            entry("Reading", Some(WORK), Some(HOME)),
            entry("New", None, Some(WORK)),
        ];
        let orders = desktop_orders(&applied);
        let strings = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            orders,
            [
                (HOME, strings(&["Home"]), strings(&["Home", "Reading"])),
                (
                    WORK,
                    strings(&["Mail", "Reading"]),
                    strings(&["Mail", "New"])
                ),
            ]
        );
        assert_eq!(
            order_line("Chrome", "Home", &orders[0].1, &orders[0].1),
            "taskbar order of Chrome on Home, as wanted: Home"
        );
        assert_eq!(
            order_line("Chrome", "Work", &[], &orders[1].2),
            "taskbar order of Chrome on Work: wanted none; now Mail | New"
        );
    }
}
