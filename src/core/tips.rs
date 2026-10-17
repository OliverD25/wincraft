//! Small in-app tips that teach a feature where it is used.
//!
//! A tip has an id and a number of shows. It is retired when it was shown that
//! many times, when the person dismisses it with the cross, or when they use
//! the feature it teaches. What each tip has done is kept in `config.json`
//! under `tips`, next to the switch that turns every tip off.
//!
//! This module holds the rules and the wording. The caller checks a tip's
//! trigger (which row is selected, which page is open) and draws it.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::core::instance;

/// How many times a tip is shown before it retires by itself.
pub const DEFAULT_MAX_SHOWS: u32 = 3;

pub struct Tip {
    pub id: &'static str,
    pub max_shows: u32,
}

/// Over a folder or file row of the palette: Ctrl+B saves it as a bookmark.
pub const BOOKMARK_CTRL_B: Tip = Tip {
    id: "bookmark-ctrl-b",
    max_shows: DEFAULT_MAX_SHOWS,
};

/// After a bookmark was saved: how to use it and where to edit it.
pub const BOOKMARK_SAVED: Tip = Tip {
    id: "bookmark-saved",
    max_shows: DEFAULT_MAX_SHOWS,
};

/// What one tip has done so far.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TipState {
    /// Palette openings (or saves) it was shown in.
    #[serde(default)]
    pub shown: u32,
    /// Dismissed, or the feature it teaches was used.
    #[serde(default)]
    pub done: bool,
}

/// The `tips` section of config.json. Ids this version does not know are
/// kept, so a newer version's tips are not forgotten by an older one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TipsConfig {
    /// The "Show tips" switch.
    #[serde(default = "on_by_default")]
    pub show: bool,
    #[serde(default)]
    pub state: BTreeMap<String, TipState>,
}

fn on_by_default() -> bool {
    true
}

impl Default for TipsConfig {
    fn default() -> Self {
        Self {
            show: true,
            state: BTreeMap::new(),
        }
    }
}

impl TipsConfig {
    fn state_of(&self, tip: &Tip) -> TipState {
        self.state.get(tip.id).cloned().unwrap_or_default()
    }

    /// Whether the tip may be on screen now. `counted_now` says that this
    /// opening already counted as one of its shows, so the show that retires
    /// the tip still lasts until the palette closes.
    pub fn wanted(&self, tip: &Tip, counted_now: bool) -> bool {
        let state = self.state_of(tip);
        allowed(self.show, tip.id) && !state.done && (counted_now || state.shown < tip.max_shows)
    }

    pub fn record_show(&mut self, tip: &Tip) {
        let state = self.state.entry(tip.id.to_string()).or_default();
        state.shown = state.shown.saturating_add(1);
    }

    /// Retires the tip. Returns false when it was retired already, so the
    /// caller saves only a real change.
    pub fn finish(&mut self, tip: &Tip) -> bool {
        let state = self.state.entry(tip.id.to_string()).or_default();
        !std::mem::replace(&mut state.done, true)
    }

    /// "Show all tips again": every tip starts from its first show.
    pub fn reset(&mut self) {
        self.state.clear();
    }
}

/// The tip a test scene asked for with `--show-tip=<id>`.
static SCENE_TIP: Mutex<Option<String>> = Mutex::new(None);

pub fn set_scene_tip(id: Option<String>) {
    *SCENE_TIP
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = id;
}

pub fn scene_tip() -> Option<String> {
    SCENE_TIP
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Whether a tip may appear at all. A test instance shows none unless a scene
/// asks for that one, so screenshots of other scenes never carry a tip.
fn allowed(show: bool, id: &str) -> bool {
    scene_allows(show, instance::is_test(), scene_tip().as_deref(), id)
}

fn scene_allows(show: bool, test_instance: bool, scene: Option<&str>, id: &str) -> bool {
    show && (!test_instance || scene == Some(id))
}

/// A piece of a tip's line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Text(String),
    /// A key or key combination, drawn as a chip.
    Key(String),
    /// Something the person typed or chose, in the heavier weight.
    Strong(String),
    /// A literal, such as a prefix character, in monospace.
    Code(String),
}

impl Part {
    pub fn text(text: &str) -> Self {
        Self::Text(text.to_string())
    }

    pub fn key(text: &str) -> Self {
        Self::Key(text.to_string())
    }

    pub fn code(text: &str) -> Self {
        Self::Code(text.to_string())
    }
}

/// What a bookmark can point at, for the wording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Folder,
    File,
}

impl Place {
    fn noun(self) -> &'static str {
        match self {
            Place::Folder => "folder",
            Place::File => "file",
        }
    }
}

/// The line over the palette's footer on a folder or file row.
pub fn bookmark_hint(place: Place) -> Vec<Part> {
    vec![
        Part::key("Ctrl + B"),
        Part::Text(format!(
            "saves this {} as a bookmark. Later, type its short name to jump here.",
            place.noun()
        )),
    ]
}

/// The longest phrase the one-line confirmation spells out.
const SHOWN_PHRASE: usize = 24;

/// The line after a bookmark was saved.
pub fn bookmark_saved(phrase: &str) -> Vec<Part> {
    let shown = if phrase.chars().count() > SHOWN_PHRASE {
        let head: String = phrase.chars().take(SHOWN_PHRASE - 1).collect();
        format!("{head}\u{2026}")
    } else {
        phrase.to_string()
    };
    vec![
        Part::text("Saved. Type"),
        Part::Strong(shown),
        Part::text("to open it. Edit bookmarks in Settings \u{2192} Bookmarks."),
    ]
}

/// The Bookmarks page when it has no bookmarks: three numbered steps. The
/// palette hotkey is the person's own.
pub fn bookmark_steps(palette_hotkey: &str) -> Vec<Vec<Part>> {
    vec![
        vec![
            Part::text("1. Press"),
            Part::Key(palette_hotkey.to_string()),
            Part::text("and find a folder (type part of its name)."),
        ],
        vec![
            Part::text("2. Press"),
            Part::key("Ctrl + B"),
            Part::text("and give it a short name, like"),
            Part::code("pswds"),
            Part::text("."),
        ],
        vec![
            Part::text("3. Type that name in the palette to jump there."),
            Part::code("'"),
            Part::text("lists all bookmarks."),
        ],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIP: Tip = Tip {
        id: "demo",
        max_shows: 3,
    };

    #[test]
    fn a_new_tip_is_wanted_and_retires_after_its_third_show() {
        let mut tips = TipsConfig::default();
        for opening in 1..=3 {
            assert!(tips.wanted(&TIP, false), "opening {opening} shows it");
            tips.record_show(&TIP);
            // The opening that counted keeps it up until the palette closes.
            assert!(tips.wanted(&TIP, true));
        }
        assert!(!tips.wanted(&TIP, false), "the fourth opening does not");
        assert_eq!(tips.state["demo"].shown, 3);
    }

    #[test]
    fn dismissing_or_using_the_feature_retires_a_tip_at_once() {
        let mut tips = TipsConfig::default();
        tips.record_show(&TIP);
        assert!(tips.finish(&TIP));
        assert!(!tips.wanted(&TIP, true), "gone even in the same opening");
        assert!(!tips.finish(&TIP), "a second finish changes nothing");
        assert_eq!(tips.state["demo"].shown, 1, "the count stays");
    }

    #[test]
    fn show_all_tips_again_starts_every_tip_over_and_keeps_the_switch() {
        let mut tips = TipsConfig::default();
        tips.record_show(&TIP);
        tips.finish(&TIP);
        tips.state.insert("other".into(), TipState::default());
        tips.show = false;
        tips.reset();
        assert!(tips.state.is_empty());
        assert!(!tips.show, "the switch is not a tip's state");
        tips.show = true;
        assert!(tips.wanted(&TIP, false));
    }

    #[test]
    fn the_switch_turns_every_tip_off() {
        let tips = TipsConfig {
            show: false,
            ..TipsConfig::default()
        };
        assert!(!tips.wanted(&TIP, false));
        assert!(!tips.wanted(&TIP, true));
    }

    #[test]
    fn a_test_instance_shows_only_the_tip_a_scene_asks_for() {
        assert!(scene_allows(true, false, None, "demo"));
        assert!(!scene_allows(false, false, None, "demo"));
        assert!(!scene_allows(true, true, None, "demo"));
        assert!(!scene_allows(true, true, Some("other"), "demo"));
        assert!(scene_allows(true, true, Some("demo"), "demo"));
        // The switch still wins in a scene.
        assert!(!scene_allows(false, true, Some("demo"), "demo"));
    }

    #[test]
    fn the_config_section_round_trips_and_keeps_tips_it_does_not_know() {
        let text = r#"{ "show": false, "state": {
            "demo": { "shown": 2, "done": true },
            "from-the-future": { "shown": 1 }
        } }"#;
        let tips: TipsConfig = serde_json::from_str(text).unwrap();
        assert!(!tips.show);
        assert_eq!(tips.state["from-the-future"].shown, 1);
        assert!(
            !tips.state["from-the-future"].done,
            "a missing field is off"
        );
        let again: TipsConfig =
            serde_json::from_str(&serde_json::to_string(&tips).unwrap()).unwrap();
        assert_eq!(again, tips);
    }

    #[test]
    fn an_empty_section_means_tips_on_and_nothing_seen() {
        let tips: TipsConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(tips, TipsConfig::default());
        assert!(tips.show);
    }

    #[test]
    fn the_hint_names_the_place_and_the_confirmation_names_the_phrase() {
        let words = |parts: &[Part]| -> String {
            parts
                .iter()
                .map(|part| match part {
                    Part::Text(text) | Part::Key(text) | Part::Strong(text) | Part::Code(text) => {
                        text.as_str()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert!(words(&bookmark_hint(Place::Folder)).contains("saves this folder as"));
        assert!(words(&bookmark_hint(Place::File)).contains("saves this file as"));
        assert_eq!(bookmark_hint(Place::Folder)[0], Part::key("Ctrl + B"));
        let saved = bookmark_saved("pswds");
        assert_eq!(saved[1], Part::Strong("pswds".to_string()));
        assert!(words(&saved).contains("Settings \u{2192} Bookmarks"));
    }

    #[test]
    fn a_long_phrase_is_cut_so_the_confirmation_stays_on_one_line() {
        let long = "a".repeat(60);
        let Part::Strong(shown) = &bookmark_saved(&long)[1] else {
            panic!("the phrase is the second part");
        };
        assert_eq!(shown.chars().count(), SHOWN_PHRASE);
        assert!(shown.ends_with('\u{2026}'));
    }

    #[test]
    fn the_empty_page_steps_use_the_persons_own_palette_hotkey() {
        let steps = bookmark_steps("Win + Ctrl + Space");
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0][1], Part::key("Win + Ctrl + Space"));
        assert_eq!(steps[1][1], Part::key("Ctrl + B"));
        assert_eq!(steps[2][1], Part::code("'"));
    }

    #[test]
    fn the_scene_tip_is_remembered_and_cleared() {
        set_scene_tip(Some("demo".to_string()));
        assert_eq!(scene_tip().as_deref(), Some("demo"));
        set_scene_tip(None);
        assert_eq!(scene_tip(), None);
    }
}
