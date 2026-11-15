//! Whether Windows can really start the logon task, and repairing it when it
//! cannot.
//!
//! On 2026-11-15 the task existed and named the right exe, but Task
//! Scheduler had never run it, and `schtasks /Run` failed with "Element not
//! found". A new task with the same definition worked. So a task that only
//! looks right is not enough: after every create WinCraft starts the task
//! once, and a first start without the task re-creates it.

use std::sync::atomic::{AtomicBool, Ordering};

use super::{xml_unescape, Installed};
use crate::core::session::StartKind;

/// Set once this process holds the single-instance mutex. Only then may the
/// task be started as a test: the copy it starts finds WinCraft running and
/// quits at once.
static HOLDS_LOCK: AtomicBool = AtomicBool::new(false);

/// Called by main.rs right after it claimed the single-instance mutex.
pub fn note_instance_lock() {
    HOLDS_LOCK.store(true, Ordering::Relaxed);
}

/// The task's program and its arguments, as `schtasks /Query /XML` gives them.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Queried {
    pub(super) command: String,
    pub(super) arguments: String,
}

/// The `<Arguments>` of a task's XML, empty when it has none.
pub(super) fn arguments_in_xml(xml: &str) -> String {
    xml.split_once("<Arguments>")
        .and_then(|(_, rest)| rest.split_once("</Arguments>"))
        .map(|(arguments, _)| xml_unescape(arguments).trim().to_string())
        .unwrap_or_default()
}

/// How the check after a create went.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Tried {
    /// Windows started the task.
    Started,
    /// Not tried: this process does not hold the single-instance mutex, or
    /// it is a test instance, whose task would start the user's WinCraft.
    Skipped(&'static str),
    /// Windows could not start it; the task was deleted and the Run key
    /// written instead. The text is the error.
    FellBack(String),
}

/// Starts a task that was just created, to see that Windows can run it. A
/// task that cannot run is deleted, and the Run key starts WinCraft instead.
pub(super) fn try_task(
    may_run: Result<(), &'static str>,
    run: impl FnOnce() -> Result<(), String>,
    delete: impl FnOnce() -> Result<(), String>,
    write_run: impl FnOnce() -> Result<(), String>,
) -> Result<Tried, String> {
    if let Err(why) = may_run {
        return Ok(Tried::Skipped(why));
    }
    let Err(error) = run() else {
        return Ok(Tried::Started);
    };
    if let Err(delete_error) = delete() {
        log::warn!("cannot delete the logon task that does not run: {delete_error}");
    }
    write_run()?;
    Ok(Tried::FellBack(error))
}

/// Whether this process may start its own task as a test.
pub(super) fn may_run_task(holds_lock: bool, test_instance: bool) -> Result<(), &'static str> {
    if test_instance {
        Err("a test instance's task would start the user's own WinCraft")
    } else if !holds_lock {
        Err("this process does not hold the single-instance lock")
    } else {
        Ok(())
    }
}

pub(super) fn holds_lock() -> bool {
    HOLDS_LOCK.load(Ordering::Relaxed)
}

/// The result of a create that was checked, for the log.
pub(super) fn tried_line(tried: &Tried) -> String {
    match tried {
        Tried::Started => {
            "logon task checked: Windows started it, and that copy quit because WinCraft runs"
                .to_string()
        }
        Tried::Skipped(why) => format!("logon task not test-started: {why}"),
        Tried::FellBack(error) => format!(
            "Windows cannot start the logon task ({error}); deleted it and using the Run key instead"
        ),
    }
}

/// What a checked create left in place.
pub(super) fn installed_after(tried: &Tried) -> Installed {
    match tried {
        Tried::FellBack(_) => Installed::RunKey,
        _ => Installed::Task,
    }
}

/// Whether this start shows that the sign-in start did not happen: start at
/// sign-in is on, and yet the first WinCraft of this session came from
/// somewhere else. An unknown session counts as not first, so a doubt never
/// re-creates the task.
pub(super) fn repair_at_start(wanted: bool, start: StartKind) -> bool {
    wanted && start.first_in_session == Some(true) && !start.from_task && !start.started_again
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn the_arguments_come_out_of_the_xml_unescaped() {
        assert_eq!(
            arguments_in_xml("<Exec><Command>x</Command><Arguments> --from-task </Arguments>"),
            "--from-task"
        );
        assert_eq!(
            arguments_in_xml("<Arguments>a &amp; b</Arguments>"),
            "a & b"
        );
        assert_eq!(arguments_in_xml("<Exec><Command>x</Command></Exec>"), "");
    }

    #[test]
    fn a_task_that_runs_stays() {
        let deleted = Cell::new(false);
        let tried = try_task(
            Ok(()),
            || Ok(()),
            || {
                deleted.set(true);
                Ok(())
            },
            || Err("not called".to_string()),
        );
        assert_eq!(tried, Ok(Tried::Started));
        assert!(!deleted.get());
        assert_eq!(installed_after(&Tried::Started), Installed::Task);
    }

    #[test]
    fn a_task_that_does_not_run_is_replaced_by_the_run_key() {
        let deleted = Cell::new(false);
        let written = Cell::new(false);
        let tried = try_task(
            Ok(()),
            || Err("ERROR: Element not found.".to_string()),
            || {
                deleted.set(true);
                Ok(())
            },
            || {
                written.set(true);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            tried,
            Tried::FellBack("ERROR: Element not found.".to_string())
        );
        assert!(deleted.get() && written.get());
        assert_eq!(installed_after(&tried), Installed::RunKey);
        assert!(tried_line(&tried).contains("Element not found"));
    }

    #[test]
    fn a_run_key_that_cannot_be_written_is_the_error() {
        let tried = try_task(
            Ok(()),
            || Err("no".to_string()),
            || Err("cannot delete".to_string()),
            || Err("registry write failed (error 5)".to_string()),
        );
        assert_eq!(tried, Err("registry write failed (error 5)".to_string()));
    }

    #[test]
    fn the_task_is_started_only_by_the_running_wincraft_and_never_by_a_test_instance() {
        assert_eq!(may_run_task(true, false), Ok(()));
        assert!(may_run_task(false, false).is_err());
        assert!(may_run_task(true, true).is_err());
        let ran = Cell::new(false);
        let tried = try_task(
            may_run_task(false, false),
            || {
                ran.set(true);
                Ok(())
            },
            || Ok(()),
            || Ok(()),
        );
        assert!(matches!(tried, Ok(Tried::Skipped(_))));
        assert!(!ran.get());
    }

    #[test]
    fn only_a_first_start_that_the_task_did_not_make_repairs_it() {
        let first = StartKind {
            first_in_session: Some(true),
            ..StartKind::default()
        };
        assert!(repair_at_start(true, first));
        assert!(!repair_at_start(false, first), "start at sign-in is off");
        assert!(!repair_at_start(
            true,
            StartKind {
                from_task: true,
                ..first
            }
        ));
        assert!(!repair_at_start(
            true,
            StartKind {
                started_again: true,
                ..first
            }
        ));
        assert!(!repair_at_start(
            true,
            StartKind {
                first_in_session: Some(false),
                ..first
            }
        ));
        assert!(!repair_at_start(
            true,
            StartKind {
                first_in_session: None,
                ..first
            }
        ));
    }
}
