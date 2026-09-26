//! How often each palette row was picked, so rows picked often rank higher
//! next time. One number per row, decayed with a 14-day half-life: every
//! pick adds 1.0, and the score halves for every 14 days it is not picked.
//! Kept in memory on the UI thread and saved to `palette_usage.json`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::core::config;
use crate::search::ResultItem;

const VERSION: u32 = 1;
pub const HALF_LIFE_SECONDS: f64 = 14.0 * 24.0 * 3600.0;
/// Most points usage can add to a row's match score. A letter that starts a
/// word or follows the one before is worth 5 to 12 points in `fuzzy::score`,
/// so usage can reorder rows that match about as well, but a match better by
/// more than one well-placed letter stays ahead, however often the weaker
/// row was picked.
pub const MAX_BOOST: i32 = 10;
/// Scales the logarithm: one pick is worth 3 points, about 10 picks reach 10.
const BOOST_SCALE: f64 = 4.0;
/// Below this a row has not been picked for about two months (from one
/// pick: 1.0 halved four times is 0.06).
const FORGET_BELOW: f64 = 0.05;
pub const MOST_ENTRIES: usize = 500;
const SAVE_GAP: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct Entry {
    score: f64,
    /// Unix seconds when `score` was last brought up to date.
    updated: i64,
}

#[derive(Serialize, Deserialize)]
struct UsageFile {
    version: u32,
    entries: HashMap<String, Entry>,
}

/// A score decayed from `from` to `to`, in unix seconds. A clock that went
/// backwards leaves the score as it was rather than growing it.
pub fn decayed(score: f64, from: i64, to: i64) -> f64 {
    let elapsed = (to - from).max(0) as f64;
    score * 0.5f64.powf(elapsed / HALF_LIFE_SECONDS)
}

/// The points a decayed usage score adds to a match score.
pub fn boost(score: f64) -> i32 {
    if score <= 0.0 {
        return 0;
    }
    ((BOOST_SCALE * score.ln_1p()).round() as i32).clamp(0, MAX_BOOST)
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// The key for a path row: the same file or folder whatever its case, and
/// "C:\Users\" the same as "C:\Users".
pub fn path_key(path: &str) -> String {
    let lower = path.to_lowercase();
    let trimmed = lower.trim_end_matches('\\');
    if trimmed.ends_with(':') {
        format!("{trimmed}\\")
    } else {
        trimmed.to_string()
    }
}

#[derive(Default)]
pub struct Usage {
    entries: HashMap<String, Entry>,
    /// None for an in-memory store that is never saved, as in tests.
    path: Option<PathBuf>,
    dirty: bool,
    last_save: Option<Instant>,
}

impl Usage {
    /// Reads the file, or starts empty. A file that cannot be read is kept
    /// as `palette_usage.json.bad` for a look later, and a new one starts.
    pub fn load(path: PathBuf) -> Self {
        let entries = match std::fs::read_to_string(&path) {
            Err(_) => HashMap::new(),
            Ok(text) => match parse(&text) {
                Ok(entries) => entries,
                Err(err) => {
                    let bad = path.with_extension("json.bad");
                    log::warn!(
                        "{} cannot be read ({err}); kept as {} and starting fresh",
                        path.display(),
                        bad.display()
                    );
                    if let Err(err) = std::fs::rename(&path, &bad) {
                        log::warn!("could not rename it: {err}");
                    }
                    HashMap::new()
                }
            },
        };
        Self {
            entries,
            path: Some(path),
            dirty: false,
            last_save: None,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The decayed score of a key, 0 for one never picked.
    pub fn score(&self, key: &str, now: i64) -> f64 {
        self.entries
            .get(key)
            .map(|entry| decayed(entry.score, entry.updated, now))
            .unwrap_or(0.0)
    }

    pub fn record(&mut self, key: &str, now: i64) {
        let score = self.score(key, now) + 1.0;
        self.entries.insert(
            key.to_string(),
            Entry {
                score,
                updated: now,
            },
        );
        self.dirty = true;
    }

    /// Adds each row's usage boost to its match score.
    pub fn boost_rows(&self, items: &mut [ResultItem], now: i64) {
        if self.entries.is_empty() {
            return;
        }
        for item in items {
            if let Some(key) = &item.usage_key {
                item.score = item.score.saturating_add(boost(self.score(key, now)));
            }
        }
    }

    /// With nothing typed: within each run of rows of one group, the rows
    /// picked before come first, most used first; the rest keep their order.
    pub fn order_rows(&self, items: &mut [ResultItem], now: i64) {
        if self.entries.is_empty() {
            return;
        }
        let scores: Vec<f64> = items
            .iter()
            .map(|item| {
                item.usage_key
                    .as_deref()
                    .map(|key| self.score(key, now))
                    .unwrap_or(0.0)
            })
            .collect();
        let mut start = 0;
        while start < items.len() {
            let mut end = start + 1;
            while end < items.len() && items[end].group == items[start].group {
                end += 1;
            }
            let mut order: Vec<usize> = (start..end).collect();
            // A stable sort keeps equal scores, and every unpicked row, in
            // the order the provider gave.
            order.sort_by(|a, b| scores[*b].total_cmp(&scores[*a]));
            let run: Vec<ResultItem> = order.iter().map(|index| items[*index].clone()).collect();
            for (offset, item) in run.into_iter().enumerate() {
                items[start + offset] = item;
            }
            start = end;
        }
    }

    /// Drops rows not picked for a long time, then the least used beyond
    /// `MOST_ENTRIES`.
    fn prune(&mut self, now: i64) {
        self.entries
            .retain(|_, entry| decayed(entry.score, entry.updated, now) >= FORGET_BELOW);
        if self.entries.len() <= MOST_ENTRIES {
            return;
        }
        let mut ranked: Vec<(String, f64)> = self
            .entries
            .iter()
            .map(|(key, entry)| (key.clone(), decayed(entry.score, entry.updated, now)))
            .collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        for (key, _) in ranked.into_iter().skip(MOST_ENTRIES) {
            self.entries.remove(&key);
        }
    }

    /// Saves if something changed and the last save was at least 2 s ago.
    pub fn save_soon(&mut self) {
        let due = self.last_save.is_none_or(|last| last.elapsed() >= SAVE_GAP);
        if self.dirty && due {
            self.save_now();
        }
    }

    /// Saves if something changed, at once: the palette was hidden or
    /// WinCraft is closing.
    pub fn flush(&mut self) {
        if self.dirty {
            self.save_now();
        }
    }

    fn save_now(&mut self) {
        self.dirty = false;
        self.last_save = Some(Instant::now());
        let Some(path) = self.path.clone() else {
            return;
        };
        self.prune(now());
        if let Err(err) = self.write(&path) {
            log::warn!("could not save palette usage: {err}");
        }
    }

    fn write(&self, path: &Path) -> Result<(), String> {
        let file = UsageFile {
            version: VERSION,
            entries: self.entries.clone(),
        };
        let text = serde_json::to_string(&file).map_err(|err| err.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
        }
        config::write_atomic(path, &text)
    }
}

/// An empty file reads as no usage yet; anything else must be version 1.
fn parse(text: &str) -> Result<HashMap<String, Entry>, String> {
    let text = config::strip_bom(text).trim();
    if text.is_empty() {
        return Ok(HashMap::new());
    }
    let file: UsageFile = serde_json::from_str(text).map_err(|err| err.to_string())?;
    if file.version != VERSION {
        return Err(format!("version {} is not {VERSION}", file.version));
    }
    Ok(file
        .entries
        .into_iter()
        .filter(|(_, entry)| entry.score.is_finite() && entry.score > 0.0)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::fuzzy;

    const DAY: i64 = 24 * 3600;
    const T0: i64 = 1_790_000_000;

    fn row(group: &str, title: &str, score: i32, key: Option<&str>) -> ResultItem {
        ResultItem {
            group: group.to_string(),
            title: title.to_string(),
            score,
            usage_key: key.map(str::to_string),
            ..Default::default()
        }
    }

    fn titles(items: &[ResultItem]) -> Vec<&str> {
        items.iter().map(|item| item.title.as_str()).collect()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wincraft-usage-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("palette_usage.json")
    }

    #[test]
    fn a_score_halves_every_fourteen_days() {
        assert_eq!(decayed(1.0, T0, T0), 1.0);
        assert!((decayed(1.0, T0, T0 + 14 * DAY) - 0.5).abs() < 1e-12);
        assert!((decayed(8.0, T0, T0 + 28 * DAY) - 2.0).abs() < 1e-12);
        assert!((decayed(1.0, T0, T0 + 7 * DAY) - 0.5f64.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn a_clock_going_backwards_does_not_grow_a_score() {
        assert_eq!(decayed(3.0, T0, T0 - 30 * DAY), 3.0);
        let mut usage = Usage::default();
        usage.record("apps:x", T0);
        assert_eq!(usage.score("apps:x", T0 - DAY), 1.0);
        usage.record("apps:x", T0 - DAY);
        assert_eq!(usage.score("apps:x", T0 - DAY), 2.0);
    }

    #[test]
    fn each_pick_adds_one_to_the_decayed_score() {
        let mut usage = Usage::default();
        usage.record("apps:x", T0);
        usage.record("apps:x", T0 + 14 * DAY);
        assert!((usage.score("apps:x", T0 + 14 * DAY) - 1.5).abs() < 1e-12);
        assert_eq!(usage.score("apps:never", T0), 0.0);
    }

    #[test]
    fn the_boost_grows_slowly_and_never_passes_its_cap() {
        assert_eq!(boost(0.0), 0);
        assert_eq!(boost(-1.0), 0);
        assert_eq!(boost(1.0), 3);
        assert!(boost(2.0) > boost(1.0));
        assert_eq!(boost(10.0), MAX_BOOST);
        for score in [0.5, 1.0, 5.0, 50.0, 1e6, f64::MAX] {
            assert!(boost(score) <= MAX_BOOST, "{score}");
        }
    }

    #[test]
    fn usage_reorders_rows_that_match_about_as_well() {
        // "no": Notion is one letter shorter, so it wins on the match alone.
        let notion = fuzzy::score("no", "Notion").unwrap();
        let notepad = fuzzy::score("no", "Notepad").unwrap();
        assert!(notion > notepad);
        let mut usage = Usage::default();
        usage.record("apps:notepad", T0);
        let mut rows = vec![
            row("Apps", "Notion", notion, Some("apps:notion")),
            row("Apps", "Notepad", notepad, Some("apps:notepad")),
        ];
        usage.boost_rows(&mut rows, T0);
        let ranked = crate::search::rank(rows, false);
        assert_eq!(titles(&ranked), ["Notepad", "Notion"]);
    }

    #[test]
    fn heavy_use_never_lifts_a_weak_match_over_a_clearly_better_one() {
        let strong = fuzzy::score("mon", "monitor").unwrap();
        let weak = fuzzy::score("mon", "my own network").unwrap();
        assert!(strong - weak > MAX_BOOST, "{strong} vs {weak}");
        let mut usage = Usage::default();
        for day in 0..500 {
            usage.record("apps:weak", T0 + day * 60);
        }
        let mut rows = vec![
            row("Apps", "my own network", weak, Some("apps:weak")),
            row("Apps", "monitor", strong, Some("apps:strong")),
        ];
        usage.boost_rows(&mut rows, T0 + 500 * 60);
        assert_eq!(rows[0].score, weak + MAX_BOOST);
        let ranked = crate::search::rank(rows, false);
        assert_eq!(titles(&ranked), ["monitor", "my own network"]);
    }

    #[test]
    fn an_exact_match_stays_first() {
        let mut usage = Usage::default();
        for _ in 0..100 {
            usage.record("apps:plus", T0);
        }
        let mut rows = vec![
            row(
                "Apps",
                "Notepad++",
                fuzzy::score("notepad", "Notepad++").unwrap(),
                Some("apps:plus"),
            ),
            row(
                "Apps",
                "Notepad",
                fuzzy::score("notepad", "Notepad").unwrap(),
                Some("apps:plain"),
            ),
        ];
        usage.boost_rows(&mut rows, T0);
        let ranked = crate::search::rank(rows, false);
        assert_eq!(titles(&ranked), ["Notepad", "Notepad++"]);
    }

    #[test]
    fn rows_without_a_key_are_never_boosted() {
        let mut usage = Usage::default();
        usage.record("calc:8", T0);
        let mut rows = vec![row("Calculator", "8", 5, None)];
        usage.boost_rows(&mut rows, T0);
        assert_eq!(rows[0].score, 5);
    }

    #[test]
    fn with_nothing_typed_picked_rows_lead_their_group() {
        let mut usage = Usage::default();
        usage.record("c:b", T0);
        usage.record("c:d", T0);
        usage.record("c:d", T0);
        usage.record("w:y", T0);
        let mut rows = vec![
            row("Commands", "a", 0, Some("c:a")),
            row("Commands", "b", 0, Some("c:b")),
            row("Commands", "c", 0, None),
            row("Commands", "d", 0, Some("c:d")),
            row("Windows", "x", 0, Some("w:x")),
            row("Windows", "y", 0, Some("w:y")),
        ];
        usage.order_rows(&mut rows, T0);
        assert_eq!(titles(&rows), ["d", "b", "a", "c", "y", "x"]);
    }

    #[test]
    fn equal_usage_and_no_usage_keep_the_providers_order() {
        let mut usage = Usage::default();
        usage.record("k:b", T0);
        usage.record("k:c", T0);
        let mut rows = vec![
            row("G", "a", 0, Some("k:a")),
            row("G", "b", 0, Some("k:b")),
            row("G", "c", 0, Some("k:c")),
            row("G", "d", 0, Some("k:d")),
        ];
        usage.order_rows(&mut rows, T0);
        assert_eq!(titles(&rows), ["b", "c", "a", "d"]);
    }

    #[test]
    fn rows_never_move_across_groups_or_split_a_run() {
        let mut usage = Usage::default();
        usage.record("k:late", T0);
        let mut rows = vec![
            row("A", "first", 0, Some("k:first")),
            row("B", "middle", 0, None),
            row("A", "late", 0, Some("k:late")),
        ];
        usage.order_rows(&mut rows, T0);
        assert_eq!(titles(&rows), ["first", "middle", "late"]);
    }

    #[test]
    fn path_keys_ignore_case_and_a_trailing_backslash() {
        assert_eq!(path_key(r"C:\Users\Admin"), r"c:\users\admin");
        assert_eq!(path_key(r"c:\USERS\admin\"), r"c:\users\admin");
        assert_eq!(path_key(r"C:\"), r"c:\");
        assert_eq!(path_key("D:"), r"d:\");
    }

    #[test]
    fn old_rows_are_forgotten_and_the_store_is_capped() {
        let mut usage = Usage::default();
        // One pick 70 days ago has decayed to 0.03.
        usage.record("old", T0 - 70 * DAY);
        for index in 0..600 {
            let key = format!("k{index:03}");
            for _ in 0..(1 + index % 7) {
                usage.record(&key, T0);
            }
        }
        usage.prune(T0);
        assert!(usage.score("old", T0) == 0.0);
        assert_eq!(usage.len(), MOST_ENTRIES);
        // The rows kept are the most used: every row picked 7 times stays.
        for index in (0..600).filter(|index| index % 7 == 6) {
            assert!(usage.score(&format!("k{index:03}"), T0) > 0.0, "{index}");
        }
    }

    #[test]
    fn usage_survives_a_save_and_a_load() {
        let path = scratch("round-trip");
        let mut usage = Usage::load(path.clone());
        assert_eq!(usage.len(), 0);
        usage.record("apps:c:\\notepad.lnk", now());
        usage.record("apps:c:\\notepad.lnk", now());
        usage.record("windows:telegram.telegramdesktop", now());
        usage.flush();
        let loaded = Usage::load(path.clone());
        assert_eq!(loaded.len(), 2);
        assert!((loaded.score("apps:c:\\notepad.lnk", now()) - 2.0).abs() < 0.01);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"version\":1"), "{text}");
    }

    #[test]
    fn a_missing_empty_or_bom_file_starts_or_reads_cleanly() {
        let path = scratch("missing");
        assert_eq!(Usage::load(path.clone()).len(), 0);
        std::fs::write(&path, "").unwrap();
        assert_eq!(Usage::load(path.clone()).len(), 0);
        assert!(path.exists(), "an empty file is not treated as corrupt");
        std::fs::write(
            &path,
            "\u{feff}{\"version\":1,\"entries\":{\"apps:x\":{\"score\":2.0,\"updated\":0}}}",
        )
        .unwrap();
        assert_eq!(Usage::load(path.clone()).len(), 1);
    }

    #[test]
    fn a_corrupt_file_is_kept_aside_and_a_fresh_one_starts() {
        let path = scratch("corrupt");
        std::fs::write(&path, "{ this is not json").unwrap();
        let mut usage = Usage::load(path.clone());
        assert_eq!(usage.len(), 0);
        let bad = path.with_extension("json.bad");
        assert_eq!(std::fs::read_to_string(&bad).unwrap(), "{ this is not json");
        assert!(!path.exists());
        usage.record("apps:x", now());
        usage.flush();
        assert_eq!(Usage::load(path.clone()).len(), 1);

        std::fs::write(&path, "{\"version\":9,\"entries\":{}}").unwrap();
        assert_eq!(Usage::load(path.clone()).len(), 0);
        assert!(
            !path.exists(),
            "a newer version is kept aside, not overwritten"
        );
    }

    #[test]
    fn saves_wait_two_seconds_after_the_last_one() {
        let path = scratch("debounce");
        let mut usage = Usage::load(path.clone());
        usage.record("a", now());
        usage.save_soon();
        assert_eq!(Usage::load(path.clone()).len(), 1);
        usage.record("b", now());
        usage.save_soon();
        assert_eq!(
            Usage::load(path.clone()).len(),
            1,
            "too soon after the first save"
        );
        usage.flush();
        assert_eq!(Usage::load(path).len(), 2);
    }

    #[test]
    fn ranking_two_thousand_rows_with_full_usage_is_fast() {
        let mut usage = Usage::default();
        for index in 0..MOST_ENTRIES {
            usage.record(&format!("apps:app{index}"), T0);
        }
        let rows: Vec<ResultItem> = (0..2000)
            .map(|index| {
                let key = format!("apps:app{index}");
                row("Apps", &key, (index % 97) as i32, Some(&key))
            })
            .collect();
        let started = Instant::now();
        let mut boosted = rows.clone();
        usage.boost_rows(&mut boosted, T0);
        let ranked = crate::search::rank(boosted, false);
        let with_usage = started.elapsed();
        let started = Instant::now();
        let plain = crate::search::rank(rows, false);
        let without = started.elapsed();
        assert_eq!(ranked.len(), plain.len());
        let added = with_usage.saturating_sub(without);
        // Timings mean nothing in an unoptimised build.
        if !cfg!(debug_assertions) {
            assert!(added < Duration::from_millis(1), "usage added {added:?}");
        }
    }
}
