//! The user's own short phrases for folders, files and web addresses.
//!
//! A phrase typed in the palette finds its bookmark first, whatever else
//! matches. Ctrl+B on a folder, file or address asks for a phrase and saves
//! it; Ctrl+Delete removes one after a second press.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::search::bookmarks::{self, Bookmark, Kind, Shared, Store};
use crate::search::{
    Action, Ask, Choice, Completion, Context, Glyph, IconRef, Query, Reply, ResultItem,
    SearchProvider, Tone,
};
use crate::ui::fuzzy;

const PROVIDER: &str = "bookmarks";
const GROUP: &str = "Bookmarks";
/// How long a "press again to remove" stays armed.
const CONFIRM_FOR: Duration = Duration::from_secs(5);

/// The verbs of the commands this provider puts on its rows. Their parts are
/// joined with a tab, which a path or an address never holds.
const ADD: &str = "add";
const NEW: &str = "new";
const REMOVE: &str = "remove";
const BROWSE: &str = "browse";
/// The stages of a question, kept in the answer's payload.
const TARGET_STAGE: &str = "target";
const PHRASE_STAGE: &str = "phrase";
const REPLACE_STAGE: &str = "replace";

/// The Ctrl+B action for a row that opens `target`; `name` is what the
/// suggested phrase is made from.
pub fn choice(target: &str, name: &str) -> Option<Choice> {
    Some(Choice {
        label: "Bookmark".to_string(),
        action: Action::Provider {
            provider: PROVIDER.to_string(),
            command: format!("{NEW}\t{target}\t{name}"),
        },
    })
}

/// The question for the phrase of a place, for rows of other providers that
/// cannot ask it themselves.
pub fn ask_for(target: &str) -> Ask {
    let shared = bookmarks::shared();
    let store = shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    phrase_ask(&store, target, &name_of(target))
}

/// "pswds_OneDrive" for `D:\...\pswds_OneDrive`, the host of an address.
fn name_of(target: &str) -> String {
    if bookmarks::kind(target) == Kind::Url {
        return target.to_string();
    }
    let trimmed = target.trim_end_matches('\\');
    trimmed.rsplit('\\').next().unwrap_or(trimmed).to_string()
}

fn phrase_ask(store: &Store, target: &str, name: &str) -> Ask {
    let target = bookmarks::clean_target(target);
    let existing = store.phrase_for(&target);
    Ask {
        provider: PROVIDER,
        chip: "Bookmark".to_string(),
        placeholder: "Short phrase, like docs".to_string(),
        text: existing
            .map(str::to_string)
            .unwrap_or_else(|| bookmarks::suggest_phrase(name)),
        action: "Save bookmark as".to_string(),
        subtitle: match existing {
            Some(phrase) => {
                format!("{target} \u{b7} already bookmarked as \u{201c}{phrase}\u{201d}")
            }
            None => target.clone(),
        },
        payload: format!("{PHRASE_STAGE}\t{target}"),
    }
}

/// How well a phrase answers what was typed. The phrase itself comes before
/// everything else in the palette, one that starts with it comes before any
/// ordinary match, and the rest are matched like any other row.
fn phrase_score(text: &str, phrase: &str) -> Option<i32> {
    let typed = text.trim().to_lowercase();
    let phrase = phrase.to_lowercase();
    if typed == phrase {
        return Some(fuzzy::EXACT + 3);
    }
    if phrase.starts_with(&typed) {
        return Some(150 - phrase.chars().count().min(40) as i32);
    }
    fuzzy::score(&typed, &phrase)
}

pub struct Bookmarks {
    store: Shared,
    /// A bookmark waiting for its second Ctrl+Delete, by lowercase phrase.
    armed: Option<(String, Instant)>,
    /// The palette should ask again: a removal was cancelled or timed out.
    dirty: bool,
}

impl Bookmarks {
    pub fn new() -> Self {
        Self::with_store(bookmarks::shared())
    }

    pub fn with_store(store: Shared) -> Self {
        Self {
            store,
            armed: None,
            dirty: false,
        }
    }

    fn store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn armed_for(&self, phrase: &str) -> bool {
        self.armed.as_ref().is_some_and(|(armed, at)| {
            *armed == phrase.to_lowercase() && at.elapsed() < CONFIRM_FOR
        })
    }

    fn row(&self, bookmark: &Bookmark, score: i32, blended: bool) -> ResultItem {
        let target = &bookmark.target;
        let url = bookmarks::kind(target) == Kind::Url;
        let armed = self.armed_for(&bookmark.phrase);
        let open = Choice {
            label: "Open".to_string(),
            action: if url {
                Action::OpenUrl(target.clone())
            } else {
                Action::Open(PathBuf::from(target))
            },
        };
        ResultItem {
            group: GROUP.to_string(),
            title: bookmark.phrase.clone(),
            subtitle: if armed {
                "Press Ctrl+Delete again to remove this bookmark".to_string()
            } else {
                target.clone()
            },
            icon: if url {
                IconRef::Glyph(Glyph::Globe)
            } else {
                IconRef::Path(PathBuf::from(target))
            },
            hint: if blended {
                "Bookmark".to_string()
            } else {
                String::new()
            },
            score,
            tone: if armed { Tone::Warning } else { Tone::Normal },
            usage_key: Some(bookmark.phrase.to_lowercase()),
            enter: Some(open),
            shift_enter: (!url).then(|| Choice {
                label: "Show in folder".to_string(),
                action: Action::Reveal(PathBuf::from(target)),
            }),
            ctrl_enter: Some(Choice {
                label: "Copy".to_string(),
                action: Action::Copy(target.clone()),
            }),
            tab_action: (!url).then(|| Choice {
                label: "Browse".to_string(),
                action: Action::Provider {
                    provider: PROVIDER.to_string(),
                    command: format!("{BROWSE}\t{target}"),
                },
            }),
            tab: Some(Completion::quiet(&bookmark.phrase)),
            delete: Some(Choice {
                label: "Remove".to_string(),
                action: Action::Provider {
                    provider: PROVIDER.to_string(),
                    command: format!("{REMOVE}\t{}", bookmark.phrase),
                },
            }),
            ..Default::default()
        }
    }

    fn add_row(score: i32) -> ResultItem {
        ResultItem {
            group: GROUP.to_string(),
            title: "Add bookmark\u{2026}".to_string(),
            subtitle: "Save a short phrase for a folder, file or web address".to_string(),
            icon: IconRef::Glyph(Glyph::Command),
            score,
            enter: Some(Choice {
                label: "Add".to_string(),
                action: Action::Provider {
                    provider: PROVIDER.to_string(),
                    command: ADD.to_string(),
                },
            }),
            ..Default::default()
        }
    }

    /// The first question of "Add bookmark…": where it should go. The folder
    /// of the Explorer window that was in front is offered.
    fn target_ask(folder: Option<String>) -> Ask {
        Ask {
            provider: PROVIDER,
            chip: "Bookmark".to_string(),
            placeholder: "Paste a folder, file or web address".to_string(),
            text: folder.unwrap_or_default(),
            action: "Bookmark".to_string(),
            subtitle: "Step 1 of 2: where the bookmark goes".to_string(),
            payload: TARGET_STAGE.to_string(),
        }
    }

    fn after_target(&self, text: &str) -> Option<Ask> {
        let target = bookmarks::clean_target(text);
        if target.is_empty() {
            return None;
        }
        let mut ask = phrase_ask(&self.store(), &target, &name_of(&target));
        if bookmarks::kind(&target) == Kind::Path && !Path::new(&target).exists() {
            ask.subtitle = format!("{target} \u{b7} not found, it is saved anyway");
        }
        Some(ask)
    }

    /// Saves the phrase, unless it already opens something else and the
    /// person has not yet said to replace it.
    fn save(&self, target: &str, text: &str, confirmed: bool) -> Option<Ask> {
        let phrase = bookmarks::clean_phrase(text);
        if phrase.is_empty() {
            return None;
        }
        let mut store = self.store();
        if let Some(existing) = store.find(&phrase) {
            if !bookmarks::same_target(&existing.target, target) && !confirmed {
                return Some(Ask {
                    provider: PROVIDER,
                    chip: "Bookmark".to_string(),
                    placeholder: "Short phrase, like docs".to_string(),
                    text: phrase.clone(),
                    action: "Replace bookmark".to_string(),
                    subtitle: format!(
                        "Press Enter again to replace \u{201c}{}\u{201d}, now {}",
                        existing.phrase, existing.target
                    ),
                    payload: format!("{REPLACE_STAGE}\t{target}\t{phrase}"),
                });
            }
        }
        store.set(&phrase, target, bookmarks::now());
        None
    }

    fn remove(&mut self, phrase: &str) {
        if self.armed_for(phrase) {
            self.armed = None;
            self.store().remove(phrase);
        } else {
            self.armed = Some((phrase.to_lowercase(), Instant::now()));
        }
    }
}

impl Default for Bookmarks {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchProvider for Bookmarks {
    fn id(&self) -> &'static str {
        PROVIDER
    }

    fn name(&self) -> &'static str {
        "Bookmarks"
    }

    fn description(&self) -> &'static str {
        "Your own short phrases for folders, files and web addresses; Ctrl+B adds one"
    }

    fn default_prefix(&self) -> Option<&'static str> {
        Some("'")
    }

    fn blended(&self) -> bool {
        true
    }

    fn placeholder(&self) -> &'static str {
        "Type a bookmark phrase\u{2026}"
    }

    fn waiting(&self) -> bool {
        self.armed.is_some()
    }

    fn has_news(&mut self) -> bool {
        if self
            .armed
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= CONFIRM_FOR)
        {
            self.armed = None;
            self.dirty = true;
        }
        std::mem::take(&mut self.dirty)
    }

    fn interrupted(&mut self) {
        if self.armed.take().is_some() {
            self.dirty = true;
        }
    }

    fn available(&mut self, command: &str) -> bool {
        match command.split_once('\t') {
            Some((BROWSE, folder)) => Path::new(folder).is_dir(),
            _ => true,
        }
    }

    fn query(&mut self, query: &Query, _context: &Context) -> Vec<ResultItem> {
        let text = query.text.trim();
        let prefixed = query.prefix.is_some();
        let sorted = self.store().sorted();
        let mut rows = Vec::new();
        if prefixed {
            for bookmark in &sorted {
                let score = if text.is_empty() {
                    Some(0)
                } else {
                    phrase_score(text, &bookmark.phrase)
                };
                if let Some(score) = score {
                    rows.push(self.row(bookmark, score, false));
                }
            }
            if sorted.is_empty() && text.is_empty() {
                rows.push(ResultItem {
                    group: GROUP.to_string(),
                    title: "No bookmarks yet".to_string(),
                    subtitle: "Press Ctrl+B on a folder or file in the list, or add one below"
                        .to_string(),
                    icon: IconRef::Glyph(Glyph::Search),
                    ..Default::default()
                });
            }
            rows.push(Self::add_row(0));
            return rows;
        }
        if text.is_empty() {
            return rows;
        }
        // One letter would match most phrases; it only finds phrases that start with it.
        let one_letter = text.chars().count() == 1;
        for bookmark in &sorted {
            if one_letter
                && !bookmark
                    .phrase
                    .to_lowercase()
                    .starts_with(&text.to_lowercase())
            {
                continue;
            }
            if let Some(score) = phrase_score(text, &bookmark.phrase) {
                rows.push(self.row(bookmark, score, true));
            }
        }
        rows.truncate(query.limit);
        if text.chars().count() >= 3 {
            if let Some(score) = fuzzy::score(text, "Add bookmark") {
                rows.push(Self::add_row(score));
            }
        }
        rows
    }

    fn act(&mut self, command: &str, _context: &Context) -> Reply {
        let (verb, rest) = command.split_once('\t').unwrap_or((command, ""));
        match verb {
            ADD => Reply::Ask(Self::target_ask(super::windows::active_explorer_folder())),
            NEW => {
                let (target, name) = rest.split_once('\t').unwrap_or((rest, ""));
                if target.trim().is_empty() {
                    return Reply::Stay;
                }
                let name = if name.is_empty() {
                    name_of(target)
                } else {
                    name.to_string()
                };
                Reply::Ask(phrase_ask(&self.store(), target, &name))
            }
            REMOVE => {
                self.remove(rest);
                Reply::Stay
            }
            BROWSE if !rest.is_empty() => Reply::Switch {
                provider: "paths",
                text: if rest.ends_with('\\') {
                    rest.to_string()
                } else {
                    format!("{rest}\\")
                },
            },
            _ => Reply::Stay,
        }
    }

    fn answered(&mut self, payload: &str, text: &str, _context: &Context) -> Option<Ask> {
        let (stage, rest) = payload.split_once('\t').unwrap_or((payload, ""));
        match stage {
            TARGET_STAGE => self.after_target(text),
            PHRASE_STAGE => self.save(rest, text, false),
            REPLACE_STAGE => {
                let (target, phrase) = rest.split_once('\t').unwrap_or((rest, ""));
                // A second Enter on the same phrase confirms; another phrase
                // is a fresh answer.
                let confirmed = bookmarks::clean_phrase(text).to_lowercase()
                    == bookmarks::clean_phrase(phrase).to_lowercase();
                self.save(target, text, confirmed)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::{rank, test_context};
    use std::sync::{Arc, Mutex};

    fn provider() -> Bookmarks {
        Bookmarks::with_store(Arc::new(Mutex::new(Store::in_memory())))
    }

    fn with(entries: &[(&str, &str)]) -> Bookmarks {
        let provider = provider();
        for (phrase, target) in entries {
            provider.store().set(phrase, target, 1);
        }
        provider
    }

    fn blended(text: &str) -> Query {
        Query {
            text: text.to_string(),
            prefix: None,
            limit: 8,
        }
    }

    fn prefixed(text: &str) -> Query {
        Query {
            text: text.to_string(),
            prefix: Some("'".to_string()),
            limit: 500,
        }
    }

    /// What the palette shows: the provider's rows, ranked.
    fn found(provider: &mut Bookmarks, query: Query) -> Vec<ResultItem> {
        let keep_order = query.text.trim().is_empty();
        rank(provider.query(&query, &test_context()), keep_order)
    }

    fn titles(rows: &[ResultItem]) -> Vec<&str> {
        rows.iter().map(|row| row.title.as_str()).collect()
    }

    const PSWDS: &str = r"D:\Windows_OneDrive\OneDrive\pswds_OneDrive";

    #[test]
    fn the_exact_phrase_scores_above_an_exact_app_name() {
        let mut provider = with(&[
            ("pswds", PSWDS),
            ("pswds archive", r"D:\old"),
            ("spwds", r"D:\x"),
        ]);
        let rows = found(&mut provider, blended("pswds"));
        assert_eq!(rows[0].title, "pswds");
        assert!(rows[0].score > fuzzy::EXACT, "above an exact app match");
        assert_eq!(rows[1].title, "pswds archive");
        assert!(rows[1].score > 100 && rows[1].score < fuzzy::EXACT);
        // "spwds" is not a subsequence match for "pswds".
        assert_eq!(titles(&rows), ["pswds", "pswds archive"]);
    }

    #[test]
    fn a_phrase_that_starts_with_the_text_beats_a_scattered_match() {
        let mut provider = with(&[("docs", r"C:\docs"), ("downloads and docs", r"C:\dl")]);
        let rows = found(&mut provider, blended("doc"));
        assert_eq!(rows[0].title, "docs");
        assert!(rows[0].score > rows[1].score);
        // A shorter phrase is nearer to what was typed.
        let mut provider = with(&[("projects", r"C:\p"), ("proj", r"C:\q")]);
        let rows = found(&mut provider, blended("pro"));
        assert_eq!(rows[0].title, "proj");
    }

    #[test]
    fn matching_ignores_case_and_a_letter_alone_only_finds_starts() {
        let mut provider = with(&[("Work", r"C:\w"), ("network", r"C:\n")]);
        let rows = found(&mut provider, blended("WORK"));
        assert_eq!(rows[0].title, "Work");
        let rows = found(&mut provider, blended("w"));
        assert_eq!(
            titles(&rows),
            ["Work"],
            "network has a w but does not start with one"
        );
    }

    #[test]
    fn nothing_typed_lists_nothing_in_the_blended_list() {
        let mut provider = with(&[("a", r"C:\a")]);
        assert!(found(&mut provider, blended("")).is_empty());
    }

    #[test]
    fn a_folder_bookmark_opens_shows_copies_browses_and_removes() {
        let mut provider = with(&[("pswds", PSWDS)]);
        let row = found(&mut provider, blended("pswds")).remove(0);
        assert_eq!(row.group, "Bookmarks");
        assert_eq!(row.subtitle, PSWDS);
        assert_eq!(row.hint, "Bookmark");
        assert_eq!(row.icon, IconRef::Path(PathBuf::from(PSWDS)));
        assert_eq!(row.usage_key.as_deref(), Some("pswds"));
        assert_eq!(
            row.enter.unwrap().action,
            Action::Open(PathBuf::from(PSWDS))
        );
        assert_eq!(
            row.shift_enter.unwrap().action,
            Action::Reveal(PathBuf::from(PSWDS))
        );
        assert_eq!(
            row.ctrl_enter.unwrap().action,
            Action::Copy(PSWDS.to_string())
        );
        let browse = row.tab_action.unwrap();
        let Action::Provider { command, .. } = browse.action else {
            panic!()
        };
        assert_eq!(
            provider.act(&command, &test_context()),
            Reply::Switch {
                provider: "paths",
                text: format!("{PSWDS}\\")
            }
        );
        assert!(row.delete.is_some());
    }

    #[test]
    fn an_address_bookmark_opens_in_the_browser_and_has_no_folder_actions() {
        let mut provider = with(&[("docs", "https://example.com/docs")]);
        let row = found(&mut provider, blended("docs")).remove(0);
        assert_eq!(
            row.enter.unwrap().action,
            Action::OpenUrl("https://example.com/docs".to_string())
        );
        assert_eq!(row.icon, IconRef::Glyph(Glyph::Globe));
        assert!(row.shift_enter.is_none() && row.tab_action.is_none());
    }

    #[test]
    fn the_bookmark_prefix_lists_all_of_them_and_the_add_row_last() {
        let mut provider = with(&[("zeta", r"C:\z"), ("alpha", r"C:\a")]);
        let rows = found(&mut provider, prefixed(""));
        assert_eq!(titles(&rows), ["alpha", "zeta", "Add bookmark\u{2026}"]);
        assert!(rows.iter().all(|row| row.hint.is_empty()));
        let rows = found(&mut provider, prefixed("alp"));
        assert_eq!(titles(&rows), ["alpha", "Add bookmark\u{2026}"]);
        let rows = found(&mut provider, prefixed("nothing"));
        assert_eq!(titles(&rows), ["Add bookmark\u{2026}"]);
    }

    #[test]
    fn with_no_bookmarks_the_list_says_how_to_make_one() {
        let mut provider = provider();
        let rows = found(&mut provider, prefixed(""));
        assert_eq!(titles(&rows), ["No bookmarks yet", "Add bookmark\u{2026}"]);
        assert!(rows[0].enter.is_none());
    }

    #[test]
    fn add_bookmark_can_be_found_by_typing_its_words() {
        let mut provider = provider();
        let rows = found(&mut provider, blended("add book"));
        assert_eq!(titles(&rows), ["Add bookmark\u{2026}"]);
        assert!(found(&mut provider, blended("ad")).is_empty());
    }

    #[test]
    fn the_new_bookmark_question_suggests_the_name_in_lower_case() {
        let mut provider = provider();
        let Some(Choice {
            action: Action::Provider { command, .. },
            ..
        }) = choice(PSWDS, "pswds_OneDrive")
        else {
            panic!()
        };
        let Reply::Ask(ask) = provider.act(&command, &test_context()) else {
            panic!("asks")
        };
        assert_eq!(ask.provider, "bookmarks");
        assert_eq!(ask.text, "pswds_onedrive");
        assert_eq!(ask.subtitle, PSWDS);
        assert_eq!(ask.action, "Save bookmark as");
        assert_eq!(ask.payload, format!("phrase\t{PSWDS}"));
    }

    #[test]
    fn answering_saves_the_bookmark_and_a_second_phrase_for_one_place_is_allowed() {
        let mut provider = provider();
        let ask = phrase_ask(&provider.store(), PSWDS, "pswds_OneDrive");
        assert_eq!(
            provider.answered(&ask.payload, "  pswds ", &test_context()),
            None
        );
        assert_eq!(provider.store().find("pswds").unwrap().target, PSWDS);
        // The place now offers its phrase.
        let again = phrase_ask(&provider.store(), PSWDS, "pswds_OneDrive");
        assert_eq!(again.text, "pswds");
        assert!(again
            .subtitle
            .contains("already bookmarked as \u{201c}pswds\u{201d}"));
        assert_eq!(
            provider.answered(&again.payload, "pw", &test_context()),
            None
        );
        assert_eq!(provider.store().items().len(), 2);
    }

    #[test]
    fn a_phrase_that_points_elsewhere_needs_a_second_enter_to_be_replaced() {
        let mut provider = with(&[("pswds", r"D:\old")]);
        let ask = phrase_ask(&provider.store(), PSWDS, "x");
        let confirm = provider
            .answered(&ask.payload, "PSWDS", &test_context())
            .expect("asks to replace");
        assert_eq!(confirm.action, "Replace bookmark");
        assert_eq!(confirm.text, "PSWDS");
        assert!(confirm.subtitle.starts_with("Press Enter again to replace"));
        assert!(confirm.subtitle.ends_with(r"D:\old"));
        assert_eq!(
            provider.store().find("pswds").unwrap().target,
            r"D:\old",
            "not yet"
        );
        // Enter again with the same phrase replaces it.
        assert_eq!(
            provider.answered(&confirm.payload, "pswds", &test_context()),
            None
        );
        assert_eq!(provider.store().find("pswds").unwrap().target, PSWDS);
        assert_eq!(provider.store().items().len(), 1);
    }

    #[test]
    fn changing_the_phrase_at_the_confirmation_is_a_new_answer() {
        let mut provider = with(&[("pswds", r"D:\old"), ("other", r"D:\o")]);
        let ask = phrase_ask(&provider.store(), PSWDS, "x");
        let confirm = provider
            .answered(&ask.payload, "pswds", &test_context())
            .unwrap();
        // A new free phrase is saved at once, with nothing replaced.
        assert_eq!(
            provider.answered(&confirm.payload, "fresh", &test_context()),
            None
        );
        assert_eq!(provider.store().find("fresh").unwrap().target, PSWDS);
        assert_eq!(provider.store().find("pswds").unwrap().target, r"D:\old");
        // Another taken phrase asks again.
        let again = provider.answered(&confirm.payload, "other", &test_context());
        assert!(again.is_some_and(|ask| ask.payload.starts_with("replace\t")));
    }

    #[test]
    fn add_bookmark_asks_for_the_place_then_for_the_phrase() {
        let mut provider = provider();
        let target = Bookmarks::target_ask(Some(r"D:\Docs".to_string()));
        assert_eq!(target.text, r"D:\Docs");
        assert_eq!(target.payload, "target");
        let phrase = provider
            .answered(&target.payload, r#" "D:/Docs/Work/" "#, &test_context())
            .expect("asks for the phrase");
        assert_eq!(phrase.payload, "phrase\tD:\\Docs\\Work");
        assert_eq!(phrase.text, "work");
        assert!(
            phrase.subtitle.ends_with("not found, it is saved anyway"),
            "{}",
            phrase.subtitle
        );
        assert_eq!(
            provider.answered(&phrase.payload, "work", &test_context()),
            None
        );
        assert_eq!(
            provider.store().find("work").unwrap().target,
            r"D:\Docs\Work"
        );
        // An address needs no folder to exist.
        let phrase = provider
            .answered("target", "https://www.example.com/a", &test_context())
            .unwrap();
        assert_eq!(phrase.text, "example.com");
        assert_eq!(phrase.subtitle, "https://www.example.com/a");
        assert!(Bookmarks::target_ask(None).text.is_empty());
    }

    #[test]
    fn removing_needs_a_second_press_and_any_other_key_cancels_it() {
        let mut provider = with(&[("pswds", PSWDS)]);
        let context = test_context();
        let remove = format!("remove\t{}", "pswds");
        assert_eq!(provider.act(&remove, &context), Reply::Stay);
        let row = found(&mut provider, blended("pswds")).remove(0);
        assert_eq!(row.tone, Tone::Warning);
        assert!(row.subtitle.contains("Ctrl+Delete again"));
        assert!(provider.waiting());
        assert_eq!(provider.store().items().len(), 1);
        // A key in between cancels, and the next press only arms again.
        provider.interrupted();
        assert!(provider.has_news());
        let row = found(&mut provider, blended("pswds")).remove(0);
        assert_eq!(row.tone, Tone::Normal);
        provider.act(&remove, &context);
        provider.act(&remove, &context);
        assert!(provider.store().items().is_empty());
        assert!(!provider.waiting() || provider.armed.is_none());
    }

    #[test]
    fn an_armed_removal_runs_out() {
        let mut provider = with(&[("a", r"C:\a")]);
        provider.act("remove\ta", &test_context());
        provider.armed = Some(("a".to_string(), Instant::now() - CONFIRM_FOR));
        assert!(provider.has_news());
        assert!(provider.armed.is_none());
        provider.act("remove\ta", &test_context());
        assert_eq!(provider.store().items().len(), 1, "it only armed again");
    }

    #[test]
    fn tab_browses_only_a_folder_that_is_there() {
        let mut provider = provider();
        let dir = std::env::temp_dir();
        assert!(provider.available(&format!("browse\t{}", dir.display())));
        assert!(!provider.available("browse\tZ:\\no\\such\\folder"));
        assert!(provider.available("add"));
    }

    #[test]
    fn the_prefix_is_the_apostrophe() {
        assert_eq!(provider().default_prefix(), Some("'"));
    }
}
