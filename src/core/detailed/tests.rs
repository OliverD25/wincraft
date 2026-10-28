use std::cell::Cell;
use std::fmt;
use std::fs;
use std::path::PathBuf;

use super::writer::{self, Writer};
use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wincraft-detailed-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

struct Counted<'a>(&'a Cell<u32>);

impl fmt::Display for Counted<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.set(self.0.get() + 1);
        write!(f, "counted")
    }
}

#[test]
fn the_log_is_off_by_default_and_formats_nothing_while_off() {
    // No test turns the global switch on, so this holds for the whole run.
    assert!(!on());
    let calls = Cell::new(0);
    crate::detailed!("value {}", Counted(&calls));
    write_args(format_args!("value {}", Counted(&calls)));
    KeyEvent {
        hook: "test",
        vk: 0x33,
        message: 0x100,
        flags: 0,
        extra: 0,
        lag_ms: 0,
        took_ms: 0.0,
    }
    .write(&Counted(&calls).to_string());
    assert_eq!(calls.get(), 1, "only the explicit to_string above formats");
}

#[test]
fn a_line_starts_with_the_time_to_the_millisecond_and_the_thread() {
    let mut st: SYSTEMTIME = unsafe { std::mem::zeroed() };
    st.wYear = 2026;
    st.wMonth = 10;
    st.wDay = 28;
    st.wHour = 9;
    st.wMinute = 5;
    st.wSecond = 7;
    st.wMilliseconds = 42;
    assert_eq!(
        stamp_with(&st, "win-number hook", "key x"),
        "2026-10-28 09:05:07.042 [win-number hook] key x"
    );
}

#[test]
fn a_key_line_names_the_key_its_flags_and_the_decision() {
    let event = KeyEvent {
        hook: "win-number",
        vk: 0x5B,
        message: 0x100,
        flags: 0x10,
        extra: 0x57,
        lag_ms: 3,
        took_ms: 0.25,
    };
    assert_eq!(
        event.line(&"WinDown"),
        "key win-number: LWin down (vk 0x5b, flags 0x10, injected 1, extra 0x57, waited 3 ms, took 0.25 ms) -> \"WinDown\""
    );
    assert_eq!(key_name(0x33), "3");
    assert_eq!(key_name(0x41), "A");
    assert_eq!(key_name(0x70), "0x70");
}

#[test]
fn file_names_go_both_ways_and_others_are_ignored() {
    assert_eq!(
        writer::file_name("2026-10-28", 1),
        "detailed-2026-10-28.log"
    );
    assert_eq!(
        writer::file_name("2026-10-28", 3),
        "detailed-2026-10-28-3.log"
    );
    assert_eq!(
        writer::parse_name("detailed-2026-10-28.log"),
        Some(("2026-10-28".to_string(), 1))
    );
    assert_eq!(
        writer::parse_name("detailed-2026-10-28-12.log"),
        Some(("2026-10-28".to_string(), 12))
    );
    for other in [
        "wincraft.log",
        "detailed-2026-10-28.txt",
        "detailed-2026-1x-28.log",
        "detailed-2026-10-28-.log",
        "detailed-2026-10-28-a.log",
        "detailed-.log",
    ] {
        assert_eq!(writer::parse_name(other), None, "{other}");
    }
}

#[test]
fn a_full_file_rolls_over_and_at_most_five_are_kept() {
    let dir = scratch("roll");
    let mut log = Writer::new(dir.clone(), 100);
    let line = |n: u32| format!("2026-10-28 10:00:00.000 [t] line {n:03} {}", "x".repeat(40));
    for n in 0..20 {
        log.line(&line(n));
    }
    log.close();
    let files = writer::files_in(&dir);
    let names: Vec<String> = files
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), writer::KEEP, "{names:?}");
    assert_eq!(names.last().unwrap(), "detailed-2026-10-28-10.log");
    for path in &files {
        let size = fs::metadata(path).unwrap().len();
        assert!(size <= 100 + 70, "{path:?} has {size} bytes");
    }
    let last = fs::read_to_string(files.last().unwrap()).unwrap();
    assert!(last.contains("line 019"), "{last}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_new_day_starts_a_new_file_and_a_restart_adds_to_the_last_one() {
    let dir = scratch("day");
    let mut log = Writer::new(dir.clone(), 1_000_000);
    log.line("2026-10-28 23:59:59.999 [t] late");
    log.line("2026-10-29 00:00:00.001 [t] early");
    log.close();
    let mut again = Writer::new(dir.clone(), 1_000_000);
    again.line("2026-10-29 08:00:00.000 [t] after a restart");
    again.close();
    let names: Vec<String> = writer::files_in(&dir)
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        ["detailed-2026-10-28.log", "detailed-2026-10-29.log"]
    );
    let today = fs::read_to_string(dir.join("detailed-2026-10-29.log")).unwrap();
    assert_eq!(
        today,
        "2026-10-29 00:00:00.001 [t] early\n2026-10-29 08:00:00.000 [t] after a restart\n"
    );
    assert_eq!(log.delete_all(), 2);
    assert!(writer::files_in(&dir).is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn deleting_leaves_other_files_alone() {
    let dir = scratch("delete");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("detailed-2026-10-28.log"), "a").unwrap();
    fs::write(dir.join("notes.txt"), "keep").unwrap();
    assert_eq!(writer::delete_all(&dir), 1);
    assert!(dir.join("notes.txt").exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_folder_is_under_the_data_folder() {
    let folder = folder();
    assert!(folder.starts_with(config::data_dir()), "{folder:?}");
    let name = folder.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("detailed"), "{name}");
}

#[test]
fn a_test_instance_gets_a_folder_of_its_own() {
    assert_eq!(instance::with_suffix("detailed", ""), "detailed");
    assert_eq!(instance::with_suffix("detailed", "t1"), "detailed.t1");
}

#[test]
fn the_key_state_watcher_writes_only_changes() {
    use super::watch::{describe, key_change};
    assert_eq!(describe(0), "none");
    let win_three = 1 | (1 << (2 + 3));
    assert_eq!(describe(win_three), "LWin+3");
    assert_eq!(describe(2 | (1 << 2)), "RWin+0");
    assert_eq!(key_change(0, 0), None);
    assert_eq!(key_change(1, 1), None);
    assert_eq!(key_change(0, 1).as_deref(), Some("keystate: LWin"));
    assert_eq!(
        key_change(1, win_three).as_deref(),
        Some("keystate: LWin+3")
    );
    assert_eq!(key_change(win_three, 0).as_deref(), Some("keystate: none"));
}

#[test]
fn the_header_finds_keyboard_tools_scripts_and_powertoys_modules() {
    use super::header::{enabled_modules, keyboard_tools, script_of_title};
    let processes: Vec<String> = [
        "explorer.exe",
        "AutoHotkey64.exe",
        "autohotkey64.exe",
        "PowerToys.KeyboardManagerEngine.exe",
        "chrome.exe",
    ]
    .iter()
    .map(|name| name.to_string())
    .collect();
    assert_eq!(
        keyboard_tools(&processes),
        ["AutoHotkey64.exe", "PowerToys.KeyboardManagerEngine.exe"]
    );
    assert_eq!(
        script_of_title(r"C:\Tools\keys.ahk - AutoHotkey v2.0.18"),
        r"C:\Tools\keys.ahk"
    );
    assert_eq!(script_of_title("plain"), "plain");
    assert_eq!(
        enabled_modules(
            r#"{"enabled": {"FancyZones": true, "Keyboard Manager": true, "Awake": false}}"#
        ),
        "FancyZones, Keyboard Manager"
    );
    assert_eq!(enabled_modules("{}"), "none listed");
    assert_eq!(enabled_modules("not json"), "settings unreadable");
}

#[test]
fn only_a_slow_step_gets_a_slow_line() {
    assert_eq!(slow_line("ui frame", 20.0, 33.0), None);
    assert_eq!(
        slow_line("ui frame", 45.4, 33.0).as_deref(),
        Some("ui frame slow 45 ms")
    );
    assert!(slow_line("ui frame", 33.0, 33.0).is_some());
}

#[test]
fn a_call_line_names_the_call_its_window_and_a_failure() {
    assert_eq!(
        call_line("ShowWindow SW_HIDE", 0x1234, 1, None),
        "call: ShowWindow SW_HIDE on 0x1234 -> 1"
    );
    assert_eq!(
        call_line("SendInput (mask key)", 0, 0, Some(5)),
        "call: SendInput (mask key) -> 0 FAILED (error 5)"
    );
}

#[test]
fn the_summary_script_reads_the_sample_log() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(root.join("tools").join("detailed-log.ps1"))
        .arg(
            root.join("tools")
                .join("samples")
                .join("detailed-sample.log"),
        )
        .output()
        .expect("powershell runs");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    for wanted in [
        "== Session",
        "header: Windows build 26200",
        "=== MARK 14:00:10 Win+2 opened Chrome instead of the strip",
        "Press 1 at 14:00:05.000",
        "Press 2 at 14:00:08.000",
        "!! NO HOOK: Windows saw digit 2 down",
        "1 digit press(es) reached Windows without a WinCraft hook line.",
        "14:00:08.100  host busy 920 ms in WM_APP_BUTTONS",
        "14:00:09.100  plugin window_deck: WM_TIMER 31.00 ms SLOW",
        "not asked by WinCraft",
        "-- gap 190 ms --",
    ] {
        assert!(text.contains(wanted), "{wanted:?} missing from:\n{text}");
    }
    assert_eq!(
        text.matches("!! NO HOOK").count(),
        1,
        "digit 3 was taken by the hook, so only digit 2 is flagged"
    );
}

#[test]
fn the_sample_log_has_the_format_the_writer_writes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let sample = fs::read_to_string(
        root.join("tools")
            .join("samples")
            .join("detailed-sample.log"),
    )
    .unwrap();
    let mut st: SYSTEMTIME = unsafe { std::mem::zeroed() };
    st.wYear = 2026;
    st.wMonth = 10;
    st.wDay = 28;
    st.wHour = 14;
    st.wMinute = 0;
    st.wSecond = 12;
    st.wMilliseconds = 1;
    let written = stamp_with(&st, "main", "=== OFF by the user");
    assert!(sample.lines().any(|line| line == written), "{written}");
}

#[test]
fn a_report_copies_the_newest_log_the_normal_logs_and_the_config() {
    use super::report::{delete_reports, header_of, reports_dir, save, Sources};
    let dir = scratch("report");
    let detailed = dir.join("detailed");
    fs::create_dir_all(&detailed).unwrap();
    fs::write(detailed.join("detailed-2026-10-27.log"), "old\n").unwrap();
    fs::write(
        detailed.join("detailed-2026-10-28.log"),
        "2026-10-28 10:00:00.000 [main] === ON by the user\n\
         2026-10-28 10:00:00.100 [h] header: WinCraft 1\n\
         2026-10-28 10:00:01.000 [main] host: x 1.00 ms\n\
         2026-10-28 11:00:00.000 [main] === ON at start (left on)\n\
         2026-10-28 11:00:00.100 [h] header: WinCraft 2\n",
    )
    .unwrap();
    fs::write(dir.join("wincraft.log"), "now\n").unwrap();
    fs::write(dir.join("config.json"), "{}").unwrap();
    let sources = Sources {
        detailed: detailed.clone(),
        log: dir.join("wincraft.log"),
        config: dir.join("config.json"),
    };
    let folder = save(&sources, "2026-10-28-120000").unwrap();
    assert_eq!(
        folder,
        reports_dir(&detailed).join("report-2026-10-28-120000")
    );
    assert!(folder.join("detailed-2026-10-28.log").exists());
    assert!(
        !folder.join("detailed-2026-10-27.log").exists(),
        "the newest only"
    );
    assert!(folder.join("wincraft.log").exists());
    assert!(!folder.join("wincraft.prev.log").exists(), "there was none");
    assert!(folder.join("config.json").exists());
    let header = fs::read_to_string(folder.join("header.txt")).unwrap();
    assert!(
        header.contains("WinCraft 2") && !header.contains("WinCraft 1"),
        "{header}"
    );
    let listing = fs::read_to_string(folder.join("report.txt")).unwrap();
    assert!(listing.contains("- config.json"), "{listing}");
    assert!(listing.contains("window titles"), "{listing}");
    assert_eq!(
        header_of(["x".to_string()].into_iter()),
        "",
        "no header, nothing"
    );
    // A report is not a detailed log file, so it never counts as one.
    assert_eq!(writer::files_in(&detailed).len(), 2);
    assert_eq!(delete_reports(&detailed), 1);
    assert!(!reports_dir(&detailed).exists());
    let _ = fs::remove_dir_all(&dir);
}
