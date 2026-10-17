//! The end of `wincraft.log`, for pages that show recent activity. The log
//! is written one whole line at a time, but it can be long and it grows
//! while it is read, so only a window at its end is read, and what could be
//! half a line is dropped.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// The first window read from the end. It doubles until enough lines are
/// found or `MAX_WINDOW` is reached.
const FIRST_WINDOW: u64 = 256 * 1024;
const MAX_WINDOW: u64 = 8 * 1024 * 1024;

/// One log line: "2026-10-17 14:01:43 INFO  layout_keeper: text".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// "10-17 14:01:43", the date without its year.
    pub when: String,
    pub level: String,
    pub text: String,
}

/// The line as an entry when it was logged by `target`.
pub fn parse(line: &str, target: &str) -> Option<Entry> {
    let line = line.trim_end_matches(['\r', '\n']);
    let (date, rest) = line.split_once(' ')?;
    let (time, rest) = rest.split_once(' ')?;
    let rest = rest.trim_start();
    let (level, rest) = rest.split_once(' ')?;
    let text = rest.trim_start().strip_prefix(target)?.strip_prefix(": ")?;
    let looks_like_date = date.len() == 10 && date.as_bytes()[4] == b'-';
    if !looks_like_date || time.len() != 8 {
        return None;
    }
    Some(Entry {
        when: format!("{} {time}", &date[5..]),
        level: level.to_string(),
        text: text.to_string(),
    })
}

/// The last `max` lines `target` logged, newest first. A missing or
/// unreadable file gives none.
pub fn tail(path: &Path, target: &str, max: usize) -> Vec<Entry> {
    tail_with(path, target, max, FIRST_WINDOW)
}

fn tail_with(path: &Path, target: &str, max: usize, first_window: u64) -> Vec<Entry> {
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    let Ok(length) = file.metadata().map(|meta| meta.len()) else {
        return Vec::new();
    };
    let mut window = first_window.max(1);
    loop {
        let start = length.saturating_sub(window);
        let mut bytes = Vec::new();
        if file.seek(SeekFrom::Start(start)).is_err() || file.read_to_end(&mut bytes).is_err() {
            return Vec::new();
        }
        let entries = entries_from(&bytes, start == 0, target, max);
        if entries.len() >= max || start == 0 || window >= MAX_WINDOW {
            return entries;
        }
        window *= 2;
    }
}

/// Entries in `bytes`, newest first. `from_start` says the bytes begin at
/// the start of the file; otherwise the first line may be cut and is
/// skipped. A last line with no line end may be half written and is
/// skipped too. Bytes that are not UTF-8 become replacement characters.
fn entries_from(bytes: &[u8], from_start: bool, target: &str, max: usize) -> Vec<Entry> {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    if !text.ends_with('\n') {
        match text.rfind('\n') {
            Some(at) => text.truncate(at + 1),
            None => return Vec::new(),
        }
    }
    let mut lines = text.split_inclusive('\n');
    let mut skipped_first = from_start;
    let mut found: Vec<Entry> = Vec::new();
    for line in &mut lines {
        if !skipped_first {
            skipped_first = true;
            continue;
        }
        if let Some(entry) = parse(line, target) {
            found.push(entry);
        }
    }
    found.reverse();
    found.truncate(max);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(second: usize, who: &str, text: &str) -> String {
        format!("2026-10-17 14:01:{second:02} INFO  {who}: {text}\n")
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "wincraft-test-logtail-{name}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn a_line_of_the_target_is_split_into_its_parts() {
        let entry = parse(
            "2026-10-17 14:01:43 WARN  layout_keeper: could not move \"x\"\r\n",
            "layout_keeper",
        )
        .unwrap();
        assert_eq!(entry.when, "10-17 14:01:43");
        assert_eq!(entry.level, "WARN");
        assert_eq!(entry.text, "could not move \"x\"");
    }

    #[test]
    fn lines_of_other_targets_and_odd_lines_are_not_entries() {
        let other = "2026-10-17 14:01:43 INFO  host: layout_keeper enabled";
        assert_eq!(parse(other, "layout_keeper"), None);
        assert_eq!(parse("", "layout_keeper"), None);
        assert_eq!(parse("not a log line", "layout_keeper"), None);
        // A target that only starts the same is another target.
        let longer = "2026-10-17 14:01:43 INFO  layout_keeperx: text";
        assert_eq!(parse(longer, "layout_keeper"), None);
    }

    #[test]
    fn the_newest_lines_come_first_and_only_the_target_counts() {
        let mut log = String::new();
        for second in 0..10 {
            log.push_str(&line(second, "layout_keeper", &format!("event {second}")));
            log.push_str(&line(second, "host", "noise"));
        }
        let entries = entries_from(log.as_bytes(), true, "layout_keeper", 3);
        let texts: Vec<&str> = entries.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, ["event 9", "event 8", "event 7"]);
    }

    #[test]
    fn a_half_written_last_line_is_left_out() {
        let mut log = line(1, "layout_keeper", "whole");
        log.push_str("2026-10-17 14:01:02 INFO  layout_keeper: half writt");
        let entries = entries_from(log.as_bytes(), true, "layout_keeper", 10);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "whole");
        assert!(entries_from(b"no line end at all", true, "layout_keeper", 10).is_empty());
    }

    #[test]
    fn a_window_that_starts_mid_file_skips_its_cut_first_line() {
        let log = format!(
            "{}{}",
            line(1, "layout_keeper", "cut line"),
            line(2, "layout_keeper", "whole")
        );
        // Start in the middle of the first line.
        let bytes = &log.as_bytes()[20..];
        let entries = entries_from(bytes, false, "layout_keeper", 10);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "whole");
        // From the start of the file the first line is whole.
        assert_eq!(
            entries_from(log.as_bytes(), true, "layout_keeper", 10).len(),
            2
        );
    }

    #[test]
    fn bytes_that_are_not_utf8_do_not_stop_the_reading() {
        let mut bytes = line(1, "layout_keeper", "before").into_bytes();
        bytes.extend_from_slice(b"2026-10-17 14:01:02 INFO  layout_keeper: caf\xE9 \xFF\n");
        bytes.extend_from_slice(&[0xC3, 0x28, b'\n']);
        let entries = entries_from(&bytes, true, "layout_keeper", 10);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].text.starts_with("caf"));
        assert!(entries[0].text.contains('\u{FFFD}'));
        assert_eq!(entries[1].text, "before");
    }

    #[test]
    fn a_big_file_is_read_from_its_end() {
        let path = scratch("big");
        let mut log = String::new();
        // About 3 MB, with LayoutKeeper's lines rare among the others.
        for n in 0..40_000 {
            log.push_str(&line(
                n % 60,
                "host",
                &format!("filler line number {n} {}", "x".repeat(40)),
            ));
            if n % 400 == 0 {
                log.push_str(&line(n % 60, "layout_keeper", &format!("event {n}")));
            }
        }
        std::fs::write(&path, &log).unwrap();
        // A small first window forces the window to grow until 200 are found.
        let entries = tail_with(&path, "layout_keeper", 50, 4096);
        assert_eq!(entries.len(), 50);
        assert_eq!(entries[0].text, "event 39600");
        assert_eq!(entries[49].text, "event 20000");
        // Asking for more than the file has gives all of them, newest first.
        let all = tail(&path, "layout_keeper", 500);
        assert_eq!(all.len(), 100);
        assert_eq!(all[99].text, "event 0");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_missing_file_has_no_lines() {
        assert!(tail(&scratch("missing"), "layout_keeper", 10).is_empty());
    }
}
