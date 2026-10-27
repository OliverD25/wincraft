use super::*;

fn item(group: &str, title: &str, score: i32) -> ResultItem {
    ResultItem {
        group: group.to_string(),
        title: title.to_string(),
        score,
        ..Default::default()
    }
}

fn titles(items: &[ResultItem]) -> Vec<&str> {
    items.iter().map(|item| item.title.as_str()).collect()
}

fn keyed(title: &str, score: i32) -> ResultItem {
    ResultItem {
        usage_key: Some(format!("apps:{title}")),
        ..item("Apps", title, score)
    }
}

fn settings(rank_by_use: bool) -> SearchConfig {
    SearchConfig {
        rank_by_use,
        ..SearchConfig::default()
    }
}

#[test]
fn a_pick_is_recorded_and_lifts_its_row_while_ranking_by_use_is_on() {
    let mut router = Router::new(Vec::new(), &BTreeMap::new());
    router.record_pick("apps:Notepad", None, "no", &settings(true));
    let ranked = router.rank_by_use(
        vec![keyed("Notion", 37), keyed("Notepad", 36)],
        false,
        &settings(true),
    );
    assert_eq!(titles(&ranked), ["Notepad", "Notion"]);
    assert_eq!(ranked[0].score, 36 + usage::boost(1.0));
}

#[test]
fn with_ranking_by_use_off_nothing_is_recorded_or_boosted() {
    let mut router = Router::new(Vec::new(), &BTreeMap::new());
    router.record_pick("apps:Notepad", None, "no", &settings(false));
    assert_eq!(router.usage.len(), 0);
    router.record_pick("apps:Notepad", None, "no", &settings(true));
    let ranked = router.rank_by_use(
        vec![keyed("Notion", 37), keyed("Notepad", 36)],
        false,
        &settings(false),
    );
    assert_eq!(titles(&ranked), ["Notion", "Notepad"]);
    assert_eq!(ranked[1].score, 36);
    let empty_query = router.rank_by_use(
        vec![keyed("Notion", 0), keyed("Notepad", 0)],
        true,
        &settings(false),
    );
    assert_eq!(titles(&empty_query), ["Notion", "Notepad"]);
}

#[test]
fn clearing_forgets_every_pick() {
    let mut router = Router::new(Vec::new(), &BTreeMap::new());
    router.record_pick("apps:Notepad", None, "no", &settings(true));
    router.clear_usage();
    assert_eq!(router.usage.len(), 0);
    let ranked = router.rank_by_use(
        vec![keyed("Notion", 37), keyed("Notepad", 36)],
        false,
        &settings(true),
    );
    assert_eq!(titles(&ranked), ["Notion", "Notepad"]);
}

#[test]
fn with_nothing_typed_the_picked_row_leads_its_group() {
    let mut router = Router::new(Vec::new(), &BTreeMap::new());
    router.record_pick("apps:Notepad", None, "no", &settings(true));
    let ranked = router.rank_by_use(
        vec![keyed("Notion", 0), keyed("Notepad", 0)],
        true,
        &settings(true),
    );
    assert_eq!(titles(&ranked), ["Notepad", "Notion"]);
}

struct Keyed;

impl SearchProvider for Keyed {
    fn id(&self) -> &'static str {
        "keyed"
    }
    fn name(&self) -> &'static str {
        "Keyed"
    }
    fn description(&self) -> &'static str {
        "Rows with usage keys"
    }
    fn blended(&self) -> bool {
        true
    }
    fn query(&mut self, _query: &Query, _context: &Context) -> Vec<ResultItem> {
        vec![
            ResultItem {
                usage_key: Some("one".to_string()),
                ..item("Keyed", "one", 5)
            },
            item("Keyed", "two", 5),
        ]
    }
}

/// A blended provider that answers every query with the same rows:
/// (group, title, score, usage key).
struct Rows {
    id: &'static str,
    rows: Vec<(&'static str, &'static str, i32, Option<&'static str>)>,
}

impl SearchProvider for Rows {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.id
    }
    fn description(&self) -> &'static str {
        "fixed rows"
    }
    fn default_prefix(&self) -> Option<&'static str> {
        (self.id == "commands").then_some("!")
    }
    fn blended(&self) -> bool {
        true
    }
    fn query(&mut self, _query: &Query, _context: &Context) -> Vec<ResultItem> {
        self.rows
            .iter()
            .map(|(group, title, score, key)| ResultItem {
                usage_key: key.map(str::to_string),
                ..item(group, title, *score)
            })
            .collect()
    }
}

fn palette_router() -> Router {
    let commands = Rows {
        id: "commands",
        rows: vec![
            ("WinCraft", "Update WinCraft", 20, Some("update")),
            ("WinCraft", "Open settings", 5, Some("settings")),
        ],
    };
    let apps = Rows {
        id: "apps",
        rows: vec![
            ("Apps", "update_server", 40, Some("server")),
            ("Apps", "Check For Updates", 30, Some("java")),
        ],
    };
    Router::new(
        vec![(None, Box::new(apps)), (None, Box::new(commands))],
        &BTreeMap::new(),
    )
}

fn found(router: &mut Router, prefix: Option<&str>, text: &str) -> Vec<String> {
    let results = router.search(prefix, text, &context());
    results.items.into_iter().map(|item| item.title).collect()
}

#[test]
fn a_word_start_match_lifts_the_wincraft_group_above_apps() {
    let mut router = palette_router();
    assert_eq!(
        found(&mut router, None, "upd"),
        [
            "Update WinCraft",
            "Open settings",
            "update_server",
            "Check For Updates"
        ]
    );
    assert_eq!(found(&mut router, None, "wincr")[0], "Update WinCraft");
}

#[test]
fn without_a_word_start_match_the_groups_keep_their_order() {
    let mut router = palette_router();
    // "ate" is inside the word "Update", not at its start.
    assert_eq!(found(&mut router, None, "ate")[0], "update_server");
    // Nothing typed: the providers' own order.
    assert_eq!(found(&mut router, None, "")[0], "update_server");
}

#[test]
fn a_prefix_leaves_the_group_order_alone() {
    let mut router = palette_router();
    let rows = found(&mut router, Some("!"), "upd");
    assert_eq!(rows, ["Update WinCraft", "Open settings"]);
}

#[test]
fn a_text_starts_a_word_only_at_the_front_of_one() {
    assert!(starts_a_word("Update WinCraft", "UPD"));
    assert!(starts_a_word("Update WinCraft", "update w"));
    assert!(starts_a_word("Update WinCraft", "wincraft"));
    assert!(!starts_a_word("Update WinCraft", "ate"));
    assert!(!starts_a_word("Update", "update wincraft"));
    assert!(!starts_a_word("Update", " "));
}

#[test]
fn a_pick_is_remembered_for_what_was_typed_and_its_starts() {
    let mut router = palette_router();
    router.record_pick("apps:java", None, "  UpD ", &settings(true));
    assert_eq!(router.usage.typed_len(), 1);
    assert_eq!(found(&mut router, None, "upd")[0], "Check For Updates");
    assert_eq!(found(&mut router, None, "up")[0], "Check For Updates");
    assert_eq!(found(&mut router, None, "u")[0], "Check For Updates");
    // Longer than what was remembered, or something else: no help.
    assert_eq!(found(&mut router, None, "updat")[0], "Update WinCraft");
}

#[test]
fn a_remembered_row_brings_its_group_and_keeps_its_group_header_whole() {
    let mut router = palette_router();
    router.record_pick("apps:java", None, "upd", &settings(true));
    assert_eq!(
        found(&mut router, None, "upd"),
        [
            "Check For Updates",
            "update_server",
            "Update WinCraft",
            "Open settings"
        ]
    );
}

#[test]
fn a_remembered_row_that_is_not_in_the_results_changes_nothing() {
    let mut router = palette_router();
    router.record_pick("apps:gone", None, "upd", &settings(true));
    assert_eq!(found(&mut router, None, "upd")[0], "Update WinCraft");
}

#[test]
fn a_prefix_is_part_of_what_is_remembered() {
    let mut router = palette_router();
    router.record_pick("commands:settings", Some("!"), "se", &settings(true));
    assert_eq!(found(&mut router, Some("!"), "s")[0], "Open settings");
    assert_eq!(found(&mut router, Some("!"), "se")[0], "Open settings");
    // The same letters without the prefix are another text.
    assert!(router.usage.remembered("se", usage::now()).is_empty());
    // A prefix with nothing after it is never remembered or used.
    router.record_pick("commands:settings", Some("!"), " ", &settings(true));
    assert_eq!(router.usage.typed_len(), 1);
}

#[test]
fn with_ranking_by_use_off_nothing_is_typed_remembered_or_used() {
    let mut router = palette_router();
    router.record_pick("apps:java", None, "upd", &settings(false));
    assert_eq!(router.usage.typed_len(), 0);
    router.record_pick("apps:java", None, "upd", &settings(true));
    let off = SearchConfig {
        rank_by_use: false,
        ..SearchConfig::default()
    };
    let context = Context {
        search: Box::leak(Box::new(off)),
        ..test_context()
    };
    let results = router.search(None, "upd", &context);
    assert_eq!(results.items[0].title, "Update WinCraft");
}

#[test]
fn clearing_forgets_what_was_typed_too() {
    let mut router = palette_router();
    router.record_pick("apps:java", None, "upd", &settings(true));
    router.clear_usage();
    assert_eq!(router.usage.typed_len(), 0);
    assert_eq!(found(&mut router, None, "upd")[0], "Update WinCraft");
}

#[test]
fn the_router_puts_the_providers_id_in_front_of_each_key() {
    let mut router = Router::new(vec![(None, Box::new(Keyed))], &BTreeMap::new());
    let results = router.search(None, "o", &test_context());
    let keys: Vec<Option<&str>> = results
        .items
        .iter()
        .map(|item| item.usage_key.as_deref())
        .collect();
    assert_eq!(keys, [Some("keyed:one"), None]);
}

/// Answers every query with one row naming itself and the text it got.
struct Echo {
    id: &'static str,
    prefix: Option<&'static str>,
    blended: bool,
}

impl SearchProvider for Echo {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.id
    }
    fn description(&self) -> &'static str {
        "echo"
    }
    fn default_prefix(&self) -> Option<&'static str> {
        self.prefix
    }
    fn blended(&self) -> bool {
        self.blended
    }
    fn query(&mut self, query: &Query, _context: &Context) -> Vec<ResultItem> {
        vec![item(self.id, &format!("{}:{}", self.id, query.text), 1)]
    }
}

fn echo(id: &'static str, prefix: Option<&'static str>, blended: bool) -> Box<dyn SearchProvider> {
    Box::new(Echo {
        id,
        prefix,
        blended,
    })
}

fn router(prefixes: &[(&str, &str)]) -> Router {
    let prefixes = prefixes
        .iter()
        .map(|(id, prefix)| (id.to_string(), prefix.to_string()))
        .collect();
    Router::new(
        vec![
            (None, echo("commands", None, true)),
            (None, echo("windows", Some("<"), true)),
            (None, echo("paths", Some("/"), false)),
            (None, echo("web", Some("?"), false)),
            (
                Some("bookmarks".to_string()),
                echo("marks", Some("*"), false),
            ),
        ],
        &prefixes,
    )
}

fn context() -> Context<'static> {
    test_context()
}

#[test]
fn a_prefix_is_split_off_the_front() {
    let router = router(&[]);
    assert_eq!(router.split("<chrome"), Some(("<".to_string(), "chrome")));
    assert_eq!(router.split("/"), Some(("/".to_string(), "")));
    assert_eq!(router.split("chrome"), None);
    assert_eq!(router.split(" <chrome"), None);
}

#[test]
fn the_longest_prefix_wins() {
    let router = router(&[("paths", "//")]);
    assert_eq!(router.split("//x"), Some(("//".to_string(), "x")));
}

#[test]
fn config_can_move_or_switch_off_a_prefix() {
    let router = router(&[("windows", "w:"), ("paths", "")]);
    assert_eq!(router.split("w:x"), Some(("w:".to_string(), "x")));
    assert_eq!(router.split("<x"), None);
    assert_eq!(router.split("/x"), None);
}

#[test]
fn a_taken_prefix_is_not_given_twice() {
    let router = router(&[("paths", "<")]);
    let mut router = router;
    let found = router.search(Some("<"), "x", &context());
    assert_eq!(titles(&found.items), ["windows:x"]);
}

#[test]
fn no_prefix_asks_only_the_blended_providers() {
    let mut router = router(&[]);
    let found = router.search(None, "x", &context());
    let mut got = titles(&found.items);
    got.sort();
    assert_eq!(got, ["commands:x", "windows:x"]);
}

#[test]
fn a_prefix_asks_only_its_provider() {
    let mut router = router(&[]);
    let found = router.search(Some("/"), "c:\\", &context());
    assert_eq!(titles(&found.items), ["paths:c:\\"]);
}

#[test]
fn the_question_mark_alone_lists_the_prefixes_and_with_text_searches_the_web() {
    let mut router = router(&[]);
    let help = router.search(Some("?"), "", &context());
    assert_eq!(
        titles(&help.items),
        ["Commands, apps and windows", "windows", "paths", "web"]
    );
    assert_eq!(help.items[1].hint, "<");
    assert_eq!(
        help.items[1].enter.as_ref().map(|choice| &choice.action),
        Some(&Action::SetPrefix("<".to_string()))
    );
    let web = router.search(Some("?"), "rust egui", &context());
    assert_eq!(titles(&web.items), ["web:rust egui"]);
}

#[test]
fn the_question_mark_still_lists_prefixes_when_nothing_owns_it() {
    let mut router = router(&[("web", "")]);
    assert_eq!(router.split("?pa"), Some(("?".to_string(), "pa")));
    let found = router.search(Some("?"), "pa", &context());
    assert_eq!(titles(&found.items).first(), Some(&"paths"));
}

#[test]
fn a_plugin_provider_answers_only_while_its_plugin_is_on() {
    let mut router = router(&[]);
    let found = router.search(Some("*"), "x", &context());
    assert!(found.items.is_empty());
}

#[test]
fn nothing_typed_keeps_the_providers_order() {
    let items = vec![item("B", "b1", 0), item("A", "a1", 0), item("B", "b2", 0)];
    assert_eq!(titles(&rank(items, true)), ["b1", "a1", "b2"]);
}

#[test]
fn groups_follow_their_best_match_and_rows_their_own_score() {
    let items = vec![
        item("Apps", "low app", 5),
        item("WinCraft", "good command", 20),
        item("Apps", "best app", 30),
        item("WinCraft", "weak command", 1),
    ];
    assert_eq!(
        titles(&rank(items, false)),
        ["best app", "low app", "good command", "weak command"]
    );
}

#[test]
fn an_exact_title_goes_first_whatever_its_group() {
    let items = vec![
        item(
            "Apps",
            "Notepad++",
            fuzzy::score("notepad", "Notepad++").unwrap(),
        ),
        item(
            "Windows",
            "Notepad",
            fuzzy::score("notepad", "Notepad").unwrap(),
        ),
    ];
    assert_eq!(titles(&rank(items, false)), ["Notepad", "Notepad++"]);
}

#[test]
fn equal_scores_keep_the_order_the_provider_gave() {
    let items = vec![item("Files", "z", 3), item("Files", "a", 3)];
    assert_eq!(titles(&rank(items, false)), ["z", "a"]);
}

#[test]
fn a_result_survives_a_trip_through_json() {
    let shortcut = PathBuf::from(r"C:\Start Menu\Notepad.lnk");
    let mut original = item("Apps", "Notepad", 12);
    original.icon = IconRef::Path(shortcut.clone());
    original.enter = Some(Choice {
        label: "Open".to_string(),
        action: Action::Open(shortcut.clone()),
    });
    original.shift_enter = Some(Choice {
        label: "Show in folder".to_string(),
        action: Action::Reveal(shortcut),
    });
    let text = serde_json::to_string(&original).unwrap();
    let back: ResultItem = serde_json::from_str(&text).unwrap();
    assert_eq!(back, original);
}

/// A provider with a scope word and fixed suggestions.
struct Scoped {
    id: &'static str,
    prefix: Option<&'static str>,
    scope: Option<&'static str>,
    suggested: Vec<(&'static str, bool)>,
}

impl SearchProvider for Scoped {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.id
    }
    fn description(&self) -> &'static str {
        "scoped"
    }
    fn default_prefix(&self) -> Option<&'static str> {
        self.prefix
    }
    fn scope(&self) -> Option<&'static str> {
        self.scope
    }
    fn blended(&self) -> bool {
        true
    }
    fn suggestions(&mut self, _context: &Context) -> Vec<ResultItem> {
        self.suggested
            .iter()
            .map(|(title, open)| ResultItem {
                usage_key: Some(title.to_string()),
                open: *open,
                ..item(self.id, title, 0)
            })
            .collect()
    }
    fn query(&mut self, query: &Query, _context: &Context) -> Vec<ResultItem> {
        if query.text.is_empty() && query.prefix.is_none() {
            return Vec::new();
        }
        vec![item(self.id, &format!("{}:{}", self.id, query.text), 1)]
    }
}

fn scoped_router() -> Router {
    Router::new(
        vec![
            (
                None,
                Box::new(Scoped {
                    id: "apps",
                    prefix: None,
                    scope: Some("a"),
                    suggested: vec![("Notepad", false), ("Paint", false)],
                }) as Box<dyn SearchProvider>,
            ),
            (
                None,
                Box::new(Scoped {
                    id: "windows",
                    prefix: Some("<"),
                    scope: Some("w"),
                    suggested: vec![("Inbox", true), ("Docs", true), ("Notes", true)],
                }),
            ),
        ],
        &BTreeMap::new(),
    )
}

#[test]
fn a_scope_word_needs_its_space_and_the_space_is_taken_off() {
    let router = scoped_router();
    assert_eq!(router.split("w chrome"), Some(("w".to_string(), "chrome")));
    assert_eq!(router.split("w "), Some(("w".to_string(), "")));
    assert_eq!(router.split("a note"), Some(("a".to_string(), "note")));
    assert_eq!(router.split("word"), None);
    assert_eq!(router.split("w"), None);
    assert_eq!(router.split("<x"), Some(("<".to_string(), "x")));
    assert_eq!(router.chip("w", "x").0, "windows");
}

#[test]
fn a_scope_asks_only_its_provider() {
    let mut router = scoped_router();
    let found = router.search(Some("w"), "x", &context());
    assert_eq!(titles(&found.items), ["windows:x"]);
    let all = router.search(Some("a"), "", &context());
    assert_eq!(titles(&all.items), ["apps:"]);
}

#[test]
fn a_scope_already_taken_is_not_given_twice() {
    let router = Router::new(
        vec![
            (None, echo("w", Some("w"), false)),
            (
                None,
                Box::new(Scoped {
                    id: "windows",
                    prefix: None,
                    scope: Some("w"),
                    suggested: Vec::new(),
                }),
            ),
        ],
        &BTreeMap::new(),
    );
    assert_eq!(router.split("w x"), Some(("w".to_string(), " x")));
}

#[test]
fn the_empty_box_suggests_open_windows_and_picked_apps() {
    let mut router = scoped_router();
    let found = router.search(None, "", &context());
    assert_eq!(titles(&found.items), ["Inbox", "Docs", "Notes"]);
    assert!(found.items.iter().all(|item| item.group == SUGGESTED_GROUP));
    // A picked app comes first; an app never picked is not suggested.
    router.record_pick("apps:Paint", None, "pa", &settings(true));
    let found = router.search(None, "", &context());
    assert_eq!(titles(&found.items), ["Paint", "Inbox", "Docs", "Notes"]);
    assert_eq!(found.items[0].provider, "apps");
    // Typing replaces the suggestions with matches.
    let typed = router.search(None, "x", &context());
    assert!(typed.items.iter().all(|item| item.group != SUGGESTED_GROUP));
}

#[test]
fn the_empty_box_suggests_at_most_five_rows_the_most_picked_first() {
    let windows = ["One", "Two", "Three", "Four", "Five", "Six", "Seven"];
    let mut router = Router::new(
        vec![
            (
                None,
                Box::new(Scoped {
                    id: "apps",
                    prefix: None,
                    scope: Some("a"),
                    suggested: vec![("Paint", false), ("Notepad", false)],
                }) as Box<dyn SearchProvider>,
            ),
            (
                None,
                Box::new(Scoped {
                    id: "windows",
                    prefix: Some("<"),
                    scope: Some("w"),
                    suggested: windows.iter().map(|title| (*title, true)).collect(),
                }),
            ),
        ],
        &BTreeMap::new(),
    );
    let found = router.search(None, "", &context());
    assert_eq!(
        titles(&found.items),
        ["One", "Two", "Three", "Four", "Five"]
    );
    // Picked apps push the open windows down; the list stays at five.
    router.record_pick("apps:Paint", None, "pa", &settings(true));
    router.record_pick("apps:Notepad", None, "no", &settings(true));
    let found = router.search(None, "", &context());
    assert_eq!(found.items.len(), SUGGESTED);
    assert_eq!(&titles(&found.items)[2..], ["One", "Two", "Three"]);
    assert!(found.items[..2].iter().all(|item| item.provider == "apps"));
}

#[test]
fn an_answer_leads_the_list() {
    let calc = Rows {
        id: "calc",
        rows: vec![("Calculator", "4", 0, None)],
    };
    let apps = Rows {
        id: "apps",
        rows: vec![("Apps", "2+2 app", 90, Some("x"))],
    };
    struct Answer(Rows);
    impl SearchProvider for Answer {
        fn id(&self) -> &'static str {
            self.0.id
        }
        fn name(&self) -> &'static str {
            self.0.id
        }
        fn description(&self) -> &'static str {
            "answer"
        }
        fn blended(&self) -> bool {
            true
        }
        fn query(&mut self, query: &Query, context: &Context) -> Vec<ResultItem> {
            let mut rows = self.0.query(query, context);
            for row in &mut rows {
                row.answer = true;
            }
            rows
        }
    }
    let mut router = Router::new(
        vec![(None, Box::new(apps)), (None, Box::new(Answer(calc)))],
        &BTreeMap::new(),
    );
    let found = router.search(None, "2+2", &context());
    assert_eq!(titles(&found.items), ["4", "2+2 app"]);
    assert_eq!(found.items[0].provider, "calc");
}

#[test]
fn the_remembered_row_is_the_best_match_until_it_is_forgotten() {
    let mut router = palette_router();
    assert!(!router.search(None, "upd", &context()).best_match);
    router.record_pick("apps:java", None, "upd", &settings(true));
    router.record_pick("apps:java", None, "upd", &settings(true));
    let found = router.search(None, "upd", &context());
    assert!(found.best_match);
    assert_eq!(found.items[0].title, "Check For Updates");
    let info = router.pick_info("apps:java", None, "upd").unwrap();
    assert_eq!(info.this_month, 2);
    assert!(info.last > 0);
    router.forget_pick("apps:java", None, "up");
    let found = router.search(None, "upd", &context());
    assert!(!found.best_match);
    assert_eq!(found.items[0].title, "Update WinCraft");
    assert!(router.pick_info("apps:java", None, "upd").is_none());
}

#[test]
fn with_learning_off_there_is_no_best_match() {
    let mut router = palette_router();
    router.record_pick("apps:java", None, "upd", &settings(true));
    let off = SearchConfig {
        rank_by_use: false,
        ..SearchConfig::default()
    };
    let context = Context {
        search: Box::leak(Box::new(off)),
        ..test_context()
    };
    assert!(!router.search(None, "upd", &context).best_match);
}

#[test]
fn a_window_row_knows_its_window() {
    let window = ResultItem {
        icon: IconRef::Window {
            hwnd: 7,
            exe: PathBuf::new(),
        },
        enter: Some(Choice {
            label: "Switch to".to_string(),
            action: Action::Activate(7),
        }),
        ..Default::default()
    };
    assert_eq!(window.window(), Some(7));
    assert_eq!(item("Apps", "x", 0).window(), None);
}

#[test]
fn picks_are_saved_to_the_usage_file_and_come_back_in_a_new_session() {
    let dir = std::env::temp_dir().join(format!("wincraft-usage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("usage.json");
    let mut router = Router::new(Vec::new(), &BTreeMap::new());
    router.load_usage(path.clone());
    assert_eq!(router.usage.len(), 0);
    router.record_pick("apps:Notepad", None, "note", &settings(true));
    router.record_pick("apps:Notepad", None, "note", &settings(true));
    router.save_usage_soon();
    router.flush_usage();
    assert!(path.exists(), "the picks were written");

    let mut later = Router::new(Vec::new(), &BTreeMap::new());
    later.load_usage(path.clone());
    assert_eq!(later.usage.len(), 1, "one row was picked");
    assert!(later.usage.typed_len() > 0, "what was typed is remembered");
    later.clear_usage();
    assert_eq!(later.usage.len(), 0);
    let mut again = Router::new(Vec::new(), &BTreeMap::new());
    again.load_usage(path);
    assert_eq!(again.usage.len(), 0, "clearing reached the file");
    let _ = std::fs::remove_dir_all(&dir);
}
