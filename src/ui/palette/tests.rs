use super::*;

#[test]
fn a_row_with_a_badge_text_carries_the_update_badge() {
    let plain = ResultItem::default();
    assert!(row_badges(&plain, false).is_empty());
    let item = ResultItem {
        badge: Some("0.20.20 available".to_string()),
        ..Default::default()
    };
    assert_eq!(
        row_badges(&item, false),
        [Badge::Update("0.20.20 available".to_string())]
    );
}

fn rename_prompt(renamed: bool) -> Prompt {
    Prompt {
        plugin: "window_namer".to_string(),
        id: 1,
        target: 42,
        chip: "Rename".to_string(),
        placeholder: "Name for \u{201c}Untitled - Notepad\u{201d}".to_string(),
        text: String::new(),
        action: "Rename to".to_string(),
        empty_action: renamed.then(|| "Clear the name".to_string()),
        subtitle: "notepad".to_string(),
        ..Default::default()
    }
}

#[test]
fn the_prompt_row_says_what_enter_will_do() {
    let prompt = rename_prompt(true);
    assert_eq!(
        prompt_row_title(&prompt, "Work notes"),
        "Rename to \u{201c}Work notes\u{201d}"
    );
    assert_eq!(prompt_row_title(&prompt, ""), "Clear the name");
    assert_eq!(
        prompt_row_title(&rename_prompt(false), ""),
        "Type, then press Enter"
    );
}

#[test]
fn a_prompt_fills_the_box_and_opening_the_palette_drops_it() {
    let mut palette = Palette::new(Router::new(Vec::new(), &Default::default()));
    let mut prompt = rename_prompt(true);
    prompt.text = "Mail".to_string();
    palette.opened();
    palette.ask(prompt);
    assert_eq!(palette.query, "Mail");
    assert!(palette.select_all);
    assert!(palette.prompt.is_some());
    palette.opened();
    assert!(palette.prompt.is_none());
    assert!(palette.query.is_empty());
}

fn row_with_keys(group: &str) -> ResultItem {
    let choice = |label: &str| {
        Some(Choice {
            label: label.to_string(),
            action: Action::SetPrefix(String::new()),
        })
    };
    ResultItem {
        group: group.to_string(),
        enter: choice("Open"),
        shift_enter: choice("Show in folder"),
        ctrl_enter: choice("Copy path"),
        bookmark: choice("Bookmark"),
        ..Default::default()
    }
}

fn hint_keys(item: &ResultItem, tab_action: Option<Choice>) -> Vec<String> {
    let situation = Situation {
        tab_action,
        ..Default::default()
    };
    let actions = actions::row_actions(item, &situation);
    actions::fit_hints(actions::footer_hints(&actions, false, "Close"), 690.0)
        .iter()
        .map(|hint| hint.key.to_string())
        .collect()
}

fn browse() -> Option<Choice> {
    Some(Choice {
        label: "Browse".to_string(),
        action: Action::SetPrefix(String::new()),
    })
}

#[test]
fn a_row_that_can_be_bookmarked_advertises_ctrl_b() {
    let keys = hint_keys(&row_with_keys("Files"), None);
    assert!(keys.contains(&"Ctrl+B".to_string()), "{keys:?}");
    assert_eq!(keys.last().map(String::as_str), Some("Esc"));
    // A row without the slot shows no such key.
    let plain = ResultItem {
        enter: Some(Choice {
            label: "Open".to_string(),
            action: Action::SetPrefix(String::new()),
        }),
        ..Default::default()
    };
    assert!(!hint_keys(&plain, None).contains(&"Ctrl+B".to_string()));
}

#[test]
fn the_footer_leaves_out_the_least_needed_keys_instead_of_overflowing() {
    let mut folder = row_with_keys("Folders");
    folder.delete = folder.bookmark.clone().map(|mut choice| {
        choice.label = "Remove".to_string();
        choice
    });
    let situation = Situation {
        tab_action: browse(),
        ..Default::default()
    };
    let actions = actions::row_actions(&folder, &situation);
    let hints = actions::fit_hints(actions::footer_hints(&actions, false, "Close"), 400.0);
    let width: f32 = hints.iter().map(actions::hint_width).sum();
    assert!(width <= 400.0, "{width}");
    assert!(hints.len() <= actions::MOST_HINTS);
    assert_eq!(hints.first().map(|hint| hint.key), Some("\u{21B5}"));
    assert_eq!(hints.last().map(|hint| hint.key), Some("Esc"));
}

#[test]
fn rows_with_their_own_keys_fill_the_footer_with_them() {
    let choice = |label: &str| {
        Some(Choice {
            label: label.to_string(),
            action: Action::SetPrefix(String::new()),
        })
    };
    let item = ResultItem {
        enter: choice("Open"),
        shift_enter: choice("Show in folder"),
        ctrl_enter: choice("Copy path"),
        ..Default::default()
    };
    assert_eq!(
        hint_keys(&item, browse()),
        ["\u{21B5}", "Shift+\u{21B5}", "Ctrl+\u{21B5}", "Tab", "Esc"]
    );
}

fn bookmark_palette(store: &crate::search::bookmarks::Shared) -> Palette {
    use crate::search::providers::bookmarks::Bookmarks;
    let mut palette = Palette::new(Router::new(
        vec![(None, Box::new(Bookmarks::with_store(store.clone())))],
        &Default::default(),
    ));
    palette.bookmarks = Some(store.clone());
    palette
}

fn new_store() -> crate::search::bookmarks::Shared {
    Arc::new(std::sync::Mutex::new(
        crate::search::bookmarks::Store::in_memory(),
    ))
}

fn ctrl_b(target: &str, name: &str) -> Action {
    crate::search::providers::bookmarks::choice(target, name)
        .unwrap()
        .action
}

#[test]
fn ctrl_b_turns_the_palette_into_a_phrase_box_prefilled_with_the_name() {
    let store = new_store();
    let mut palette = bookmark_palette(&store);
    let bridge = crate::core::ui_bridge::create();
    let snapshot = UiSnapshot::default();
    palette.opened();
    let outcome = palette.perform(
        ctrl_b(r"D:\Docs\pswds_OneDrive", "pswds_OneDrive"),
        &snapshot,
        &bridge.to_host,
    );
    assert!(matches!(outcome, Outcome::Stay));
    let prompt = palette.prompt.clone().expect("a question is asked");
    assert_eq!(prompt.plugin, "search:bookmarks");
    assert_eq!(prompt.chip, "Bookmark");
    assert_eq!(prompt.subtitle, r"D:\Docs\pswds_OneDrive");
    assert_eq!(palette.query, "pswds_onedrive");
    assert!(palette.select_all, "typing replaces the suggestion");
    assert_eq!(
        prompt_row_title(&prompt, "pswds"),
        "Save bookmark as \u{201c}pswds\u{201d}"
    );
}

#[test]
fn enter_in_that_box_saves_the_bookmark_here_without_the_host() {
    let store = new_store();
    let mut palette = bookmark_palette(&store);
    let bridge = crate::core::ui_bridge::create();
    let snapshot = UiSnapshot::default();
    palette.opened();
    palette.perform(ctrl_b(r"D:\Docs\a", "a"), &snapshot, &bridge.to_host);
    let prompt = palette.prompt.clone().unwrap();
    let outcome = palette.answer_prompt(&prompt, "pswds".to_string(), &snapshot, &bridge.to_host);
    assert!(
        matches!(outcome, Outcome::Stay),
        "the palette stays for the confirmation"
    );
    assert_eq!(
        store.lock().unwrap().find("pswds").unwrap().target,
        r"D:\Docs\a"
    );
    // The answer itself never goes to the host; only the tips' state does.
    while let Ok(request) = bridge.host_rx.try_recv() {
        assert!(
            matches!(request, HostRequest::SetHostSetting(HostSetting::Tips(_))),
            "only the tips' state is sent"
        );
    }
}

fn saved_phrase_of(palette: &Palette) -> Option<&str> {
    palette.saved.as_ref().map(|saved| saved.phrase.as_str())
}

#[test]
fn saving_shows_the_confirmation_and_counts_as_using_the_ctrl_b_tip() {
    let store = new_store();
    let mut palette = bookmark_palette(&store);
    let bridge = crate::core::ui_bridge::create();
    let snapshot = UiSnapshot::default();
    palette.opened();
    palette.perform(ctrl_b(r"D:\Docs\a", "a"), &snapshot, &bridge.to_host);
    let prompt = palette.prompt.clone().unwrap();
    palette.answer_prompt(&prompt, "  Pswds ".to_string(), &snapshot, &bridge.to_host);
    assert_eq!(saved_phrase_of(&palette), Some("Pswds"));
    assert!(palette.prompt.is_none(), "back in the plain palette");
    assert!(palette.query.is_empty());
    let until = palette.saved.as_ref().and_then(|saved| saved.until);
    assert!(until.is_some_and(|at| at > Instant::now()), "closes later");
    assert!(palette.tips.state[tips::BOOKMARK_CTRL_B.id].done);
    assert_eq!(palette.tips.state[tips::BOOKMARK_SAVED.id].shown, 1);
    let mut sent = None;
    while let Ok(HostRequest::SetHostSetting(HostSetting::Tips(tips))) = bridge.host_rx.try_recv() {
        sent = Some(tips);
    }
    assert_eq!(sent.as_ref(), Some(&palette.tips), "the host keeps it");
}

#[test]
fn with_tips_off_saving_closes_the_palette_as_before() {
    let store = new_store();
    let mut palette = bookmark_palette(&store);
    let bridge = crate::core::ui_bridge::create();
    let mut snapshot = UiSnapshot::default();
    snapshot.tips.show = false;
    palette.opened();
    palette.perform(ctrl_b(r"D:\Docs\a", "a"), &snapshot, &bridge.to_host);
    let prompt = palette.prompt.clone().unwrap();
    let outcome = palette.answer_prompt(&prompt, "pswds".to_string(), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Hide));
    assert!(palette.saved.is_none());
    assert!(store.lock().unwrap().find("pswds").is_some(), "still saved");
}

#[test]
fn the_confirmation_retires_after_three_saves() {
    let store = new_store();
    let mut palette = bookmark_palette(&store);
    let bridge = crate::core::ui_bridge::create();
    let snapshot = UiSnapshot::default();
    for number in 1..=4 {
        palette.opened();
        palette.perform(ctrl_b(r"D:\Docs\a", "a"), &snapshot, &bridge.to_host);
        let prompt = palette.prompt.clone().unwrap();
        let phrase = format!("phrase{number}");
        let outcome = palette.answer_prompt(&prompt, phrase, &snapshot, &bridge.to_host);
        let stays = matches!(outcome, Outcome::Stay);
        assert_eq!(stays, number <= 3, "save {number}");
    }
}

#[test]
fn a_failed_save_is_not_confirmed() {
    let store = new_store();
    let mut palette = bookmark_palette(&store);
    let bridge = crate::core::ui_bridge::create();
    let snapshot = UiSnapshot::default();
    palette.opened();
    let prompt = Prompt {
        plugin: format!("{SEARCH_PROMPT}bookmarks"),
        payload: "target\t".to_string(),
        ..Default::default()
    };
    let outcome = palette.answer_prompt(&prompt, String::new(), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Hide));
    assert!(palette.saved.is_none());
}

/// The width and height of the window the palette is drawn in.
fn window() -> egui::Rect {
    Rect::from_min_size(pos2(0.0, 0.0), vec2(WIDTH, HEIGHT))
}

/// One frame of the palette in a window of its size.
fn frame(
    palette: &mut Palette,
    ctx: &egui::Context,
    snapshot: &UiSnapshot,
    bridge: &crate::core::ui_bridge::Bridge,
    events: Vec<egui::Event>,
) -> Outcome {
    let input = egui::RawInput {
        screen_rect: Some(window()),
        events,
        ..Default::default()
    };
    let mut outcome = Outcome::Stay;
    let mut output = ctx.run_ui(input, |ui| {
        outcome = palette.show(ui, snapshot, &bridge.to_host, 1.0, true);
    });
    output.textures_delta.clear();
    outcome
}

fn drawing_context() -> egui::Context {
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    theme::apply(
        &ctx,
        crate::core::config::ThemeName::Classic,
        crate::core::config::ThemeChoice::Dark,
    );
    ctx
}

#[test]
fn ctrl_k_on_a_command_the_snapshot_does_not_list_opens_the_hotkeys_page() {
    let mut palette = Palette::new(Router::new(Vec::new(), &Default::default()));
    let bridge = crate::core::ui_bridge::create();
    let change = Action::ChangeShortcut {
        id: "x.go".to_string(),
        label: "Go".to_string(),
    };
    let outcome = palette.perform(change, &UiSnapshot::default(), &bridge.to_host);
    assert!(matches!(outcome, Outcome::OpenShortcut { id, .. } if id == "x.go"));
    assert!(palette.hotkey_dialog.is_none());
}

#[test]
fn the_hotkey_dialog_sits_over_the_panel_and_the_palette_stays_open() {
    use crate::ui::widgets::hotkey_dialog::Dialog;
    let mut palette = Palette::new(Router::new(Vec::new(), &Default::default()));
    let bridge = crate::core::ui_bridge::create();
    let snapshot = UiSnapshot::default();
    let ctx = drawing_context();
    palette.opened();
    palette.hotkey_dialog = Some(Editor::preview(Dialog::new(Target {
        id: "x.go".to_string(),
        label: "Go".to_string(),
        chords: true,
        removable: true,
    })));
    for _ in 0..3 {
        let outcome = frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
        assert!(matches!(outcome, Outcome::Stay));
    }
    assert!(palette.hotkey_dialog.is_some());
    assert!(window().contains_rect(palette.panel) && palette.panel.width() > 0.0);
    assert!(bridge.host_rx.try_recv().is_err(), "nothing saved");
    palette.hidden();
    assert!(palette.hotkey_dialog.is_none(), "hiding closes the dialog");
}

#[test]
fn the_confirmation_closes_the_palette_after_four_seconds_and_goes_at_the_first_key() {
    let store = new_store();
    let mut palette = bookmark_palette(&store);
    let bridge = crate::core::ui_bridge::create();
    let snapshot = UiSnapshot::default();
    let ctx = drawing_context();
    palette.opened();
    palette.saved = Some(Saved {
        phrase: "pswds".to_string(),
        until: Some(Instant::now() + SAVED_FOR),
    });
    frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
    assert!(matches!(
        frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new()),
        Outcome::Stay
    ));
    assert!(palette.saved.is_some(), "still showing");
    // Typing clears it and the palette stays open for the typing.
    let outcome = frame(
        &mut palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![egui::Event::Text("d".to_string())],
    );
    assert!(matches!(outcome, Outcome::Stay));
    assert!(palette.saved.is_none());

    palette.saved = Some(Saved {
        phrase: "pswds".to_string(),
        until: Some(Instant::now() - Duration::from_millis(1)),
    });
    assert!(matches!(
        frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new()),
        Outcome::Hide
    ));
}

#[test]
fn a_scene_can_open_on_the_confirmation_and_it_never_times_out() {
    let saved = scene_saved(Some("bookmark-saved")).unwrap();
    assert_eq!(saved.phrase, "pswds");
    assert!(saved.until.is_none());
    assert!(scene_saved(Some("bookmark-ctrl-b")).is_none());
    assert!(scene_saved(None).is_none());
}

/// A provider whose rows each open one place, so Ctrl+B applies.
struct Places(Vec<String>);

impl crate::search::SearchProvider for Places {
    fn id(&self) -> &'static str {
        "places"
    }
    fn name(&self) -> &'static str {
        "Places"
    }
    fn description(&self) -> &'static str {
        "test rows"
    }
    fn blended(&self) -> bool {
        true
    }
    fn query(&mut self, _query: &crate::search::Query, _context: &Context) -> Vec<ResultItem> {
        self.0
            .iter()
            .map(|target| ResultItem {
                group: "Places".to_string(),
                title: target.clone(),
                bookmark: crate::search::providers::bookmarks::choice(target, "name"),
                enter: Some(Choice {
                    label: "Open".to_string(),
                    action: Action::Open(target.into()),
                }),
                ..Default::default()
            })
            .collect()
    }
}

/// A folder of the test's own under the temp folder, so tests running at
/// the same time do not remove each other's.
fn scratch_folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("wincraft-tips-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A palette over one test row per target, typed into so the rows show,
/// with the bookmarks in `store`.
fn places_palette(targets: &[&str], store: &crate::search::bookmarks::Shared) -> Palette {
    let rows = Places(targets.iter().map(|target| target.to_string()).collect());
    let mut palette = Palette::new(Router::new(
        vec![(None, Box::new(rows))],
        &Default::default(),
    ));
    palette.bookmarks = Some(store.clone());
    palette.opened();
    palette.type_in("x");
    palette
}

/// What one more frame of the palette puts over the footer.
fn hint(palette: &mut Palette, bridge: &crate::core::ui_bridge::Bridge) -> Option<Place> {
    let snapshot = UiSnapshot::default();
    frame(palette, &drawing_context(), &snapshot, bridge, Vec::new());
    match palette.current_strip(&bridge.to_host) {
        Some(Strip::Hint(place)) => Some(place),
        _ => None,
    }
}

#[test]
fn a_folder_row_gets_the_tip_and_a_file_row_gets_the_file_wording() {
    let folder = scratch_folder("kinds");
    let file = folder.join("note.txt");
    std::fs::write(&file, "x").unwrap();
    let folder_text = folder.display().to_string();
    let file_text = file.display().to_string();
    let bridge = crate::core::ui_bridge::create();
    let ctx = drawing_context();
    let snapshot = UiSnapshot::default();

    let mut palette = places_palette(&[&folder_text, &file_text], &new_store());
    frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
    assert_eq!(hint(&mut palette, &bridge), Some(Place::Folder));
    palette.selected = 1;
    assert_eq!(hint(&mut palette, &bridge), Some(Place::File));
    std::fs::remove_dir_all(&folder).ok();
}

#[test]
fn rows_that_cannot_be_bookmarked_as_a_place_get_no_tip() {
    let bridge = crate::core::ui_bridge::create();
    let ctx = drawing_context();
    let snapshot = UiSnapshot::default();
    let mut palette = places_palette(&["https://example.com"], &new_store());
    frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
    assert_eq!(hint(&mut palette, &bridge), None, "an address");

    let mut none = Palette::new(Router::new(Vec::new(), &Default::default()));
    none.bookmarks = Some(new_store());
    none.opened();
    frame(&mut none, &ctx, &snapshot, &bridge, Vec::new());
    assert_eq!(hint(&mut none, &bridge), None, "no row at all");
}

#[test]
fn once_a_bookmark_exists_the_tip_does_not_come_back() {
    let folder = scratch_folder("exists").display().to_string();
    let bridge = crate::core::ui_bridge::create();
    let store = new_store();
    let mut palette = places_palette(&[&folder], &store);
    assert_eq!(hint(&mut palette, &bridge), Some(Place::Folder));
    store.lock().unwrap().set("any", r"D:\any", 1);
    palette.opened();
    palette.type_in("x");
    assert_eq!(hint(&mut palette, &bridge), None);
    std::fs::remove_dir_all(&folder).ok();
}

#[test]
fn the_tip_counts_one_show_per_opening_not_per_frame_and_retires_after_three() {
    let folder = scratch_folder("count").display().to_string();
    let bridge = crate::core::ui_bridge::create();
    let ctx = drawing_context();
    let snapshot = UiSnapshot::default();
    let mut palette = places_palette(&[&folder], &new_store());
    let shown = |palette: &Palette| {
        palette
            .tips
            .state
            .get(tips::BOOKMARK_CTRL_B.id)
            .map_or(0, |state| state.shown)
    };
    for opening in 1..=3u32 {
        palette.opened();
        palette.type_in("x");
        for _ in 0..5 {
            frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
            assert_eq!(hint(&mut palette, &bridge), Some(Place::Folder));
        }
        assert_eq!(shown(&palette), opening, "opening {opening}");
    }
    palette.opened();
    palette.type_in("x");
    frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
    assert_eq!(hint(&mut palette, &bridge), None, "the fourth opening");
    assert_eq!(shown(&palette), 3);
    std::fs::remove_dir_all(&folder).ok();
}

#[test]
fn every_counted_show_is_sent_to_the_host_once() {
    let folder = scratch_folder("sent").display().to_string();
    let bridge = crate::core::ui_bridge::create();
    let ctx = drawing_context();
    let snapshot = UiSnapshot::default();
    let mut palette = places_palette(&[&folder], &new_store());
    for _ in 0..4 {
        frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
    }
    let mut sent = 0;
    while let Ok(HostRequest::SetHostSetting(HostSetting::Tips(tips))) = bridge.host_rx.try_recv() {
        assert_eq!(tips.state[tips::BOOKMARK_CTRL_B.id].shown, 1);
        sent += 1;
    }
    assert_eq!(sent, 1);
}

#[test]
fn the_cross_retires_the_tip_for_good_and_the_switch_hides_it() {
    let folder = scratch_folder("cross").display().to_string();
    let bridge = crate::core::ui_bridge::create();
    let ctx = drawing_context();
    let snapshot = UiSnapshot::default();
    let mut palette = places_palette(&[&folder], &new_store());
    frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
    // The strip is the last 32 px of the list area, and the cross sits
    // 8 px in from its right edge.
    let cross = pos2(
        MARGIN + PANEL_WIDTH - inspector::WIDTH - 8.0 - 8.0 - 12.0,
        MARGIN + PANEL_HEIGHT - FOOTER_HEIGHT - 4.0 - STRIP_HEIGHT / 2.0,
    );
    let press = |pressed| egui::Event::PointerButton {
        pos: cross,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(
        &mut palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![egui::Event::PointerMoved(cross)],
    );
    frame(&mut palette, &ctx, &snapshot, &bridge, vec![press(true)]);
    frame(&mut palette, &ctx, &snapshot, &bridge, vec![press(false)]);
    assert!(palette.tips.state[tips::BOOKMARK_CTRL_B.id].done);
    assert_eq!(hint(&mut palette, &bridge), None, "gone in this opening");
    palette.opened();
    palette.type_in("x");
    assert_eq!(hint(&mut palette, &bridge), None, "and in the next");

    let mut off = places_palette(&[&folder], &new_store());
    let mut snapshot = UiSnapshot::default();
    snapshot.tips.show = false;
    frame(&mut off, &ctx, &snapshot, &bridge, Vec::new());
    assert_eq!(hint(&mut off, &bridge), None);
    std::fs::remove_dir_all(&folder).ok();
}

#[test]
fn a_new_snapshot_brings_in_what_the_settings_page_changed() {
    let folder = scratch_folder("snapshot").display().to_string();
    let bridge = crate::core::ui_bridge::create();
    let ctx = drawing_context();
    let mut snapshot = UiSnapshot::default();
    let mut palette = places_palette(&[&folder], &new_store());
    frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
    assert!(palette.tips.show);
    snapshot.tips.show = false;
    frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
    assert!(
        palette.tips.show,
        "the snapshot is read once, until a new one"
    );
    palette.invalidate();
    frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
    assert!(!palette.tips.show);
    std::fs::remove_dir_all(&folder).ok();
}

#[test]
fn a_taken_phrase_asks_again_and_the_second_enter_replaces_it() {
    let store = new_store();
    store.lock().unwrap().set("pswds", r"D:\old", 1);
    let mut palette = bookmark_palette(&store);
    let bridge = crate::core::ui_bridge::create();
    let snapshot = UiSnapshot::default();
    palette.opened();
    palette.perform(ctrl_b(r"D:\new", "new"), &snapshot, &bridge.to_host);
    let first = palette.prompt.clone().unwrap();
    let outcome = palette.answer_prompt(&first, "pswds".to_string(), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Stay), "still in the box");
    let second = palette.prompt.clone().unwrap();
    assert_eq!(second.action, "Replace bookmark");
    assert_eq!(palette.query, "pswds");
    assert_eq!(
        store.lock().unwrap().find("pswds").unwrap().target,
        r"D:\old"
    );
    let outcome = palette.answer_prompt(&second, "pswds".to_string(), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Stay), "the confirmation shows");
    assert_eq!(saved_phrase_of(&palette), Some("pswds"));
    assert_eq!(
        store.lock().unwrap().find("pswds").unwrap().target,
        r"D:\new"
    );
}

fn ctrl_key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        },
    }
}

#[test]
fn ctrl_delete_pressed_twice_removes_the_selected_bookmark() {
    let store = new_store();
    store.lock().unwrap().set("pswds", r"D:\x", 1);
    let mut palette = bookmark_palette(&store);
    let bridge = crate::core::ui_bridge::create();
    let snapshot = UiSnapshot::default();
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    theme::apply(
        &ctx,
        crate::core::config::ThemeName::Classic,
        crate::core::config::ThemeChoice::Dark,
    );
    palette.opened();
    palette.type_in("'");
    let frame = |palette: &mut Palette, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            palette.show(ui, &snapshot, &bridge.to_host, 1.0, true);
        });
        output.textures_delta.clear();
    };
    // The first frames install the fonts and fill the list.
    frame(&mut palette, Vec::new());
    frame(&mut palette, Vec::new());
    let titles = |palette: &Palette| -> Vec<String> {
        palette
            .results
            .items
            .iter()
            .map(|item| item.title.clone())
            .collect()
    };
    assert_eq!(titles(&palette), ["pswds", "Add bookmark\u{2026}"]);
    // The first press only asks for another; the row says so.
    frame(&mut palette, vec![ctrl_key(egui::Key::Delete)]);
    frame(&mut palette, Vec::new());
    assert_eq!(palette.results.items[0].tone, Tone::Warning);
    assert_eq!(store.lock().unwrap().items().len(), 1);
    // The second press removes it. Ctrl+Delete itself must not count as
    // "another key", which would cancel the first press.
    frame(&mut palette, vec![ctrl_key(egui::Key::Delete)]);
    frame(&mut palette, Vec::new());
    assert!(store.lock().unwrap().items().is_empty());
    assert_eq!(
        titles(&palette),
        ["No bookmarks yet", "Add bookmark\u{2026}"]
    );
}

#[test]
fn a_plugins_answer_still_goes_to_the_host() {
    let mut palette = Palette::new(Router::new(Vec::new(), &Default::default()));
    let bridge = crate::core::ui_bridge::create();
    let outcome = palette.answer_prompt(
        &rename_prompt(true),
        "Work".to_string(),
        &UiSnapshot::default(),
        &bridge.to_host,
    );
    assert!(matches!(outcome, Outcome::Hide));
    assert!(matches!(
        bridge.host_rx.try_recv(),
        Ok(HostRequest::PromptAnswer { id: 1, .. })
    ));
}

/// Window rows that never touch a real window: the handles are made up
/// and nothing in these tests reaches the host's window actions.
struct Wins;

impl crate::search::SearchProvider for Wins {
    fn id(&self) -> &'static str {
        "windows"
    }
    fn name(&self) -> &'static str {
        "Windows"
    }
    fn description(&self) -> &'static str {
        "test windows"
    }
    fn scope(&self) -> Option<&'static str> {
        Some("w")
    }
    fn blended(&self) -> bool {
        true
    }
    fn query(&mut self, _query: &crate::search::Query, _context: &Context) -> Vec<ResultItem> {
        [(-7001, "Notes"), (-7002, "Inbox")]
            .into_iter()
            .map(|(hwnd, title)| ResultItem {
                group: "Windows".to_string(),
                title: title.to_string(),
                icon: IconRef::Window {
                    hwnd,
                    exe: std::path::PathBuf::new(),
                },
                enter: Some(Choice {
                    label: "Switch to".to_string(),
                    action: Action::Activate(hwnd),
                }),
                ..Default::default()
            })
            .collect()
    }
}

fn windows_palette() -> Palette {
    let mut palette = Palette::new(Router::new(
        vec![(None, Box::new(Wins))],
        &Default::default(),
    ));
    palette.opened();
    palette
}

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }
}

#[test]
fn a_scope_word_becomes_the_chip_and_backspace_on_an_empty_box_removes_it() {
    let mut palette = windows_palette();
    let bridge = crate::core::ui_bridge::create();
    let snapshot = UiSnapshot::default();
    let ctx = drawing_context();
    palette.type_in("w no");
    assert_eq!(palette.prefix.as_deref(), Some("w"));
    assert_eq!(palette.query, "no");
    palette.query.clear();
    frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
    frame(
        &mut palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![key(egui::Key::Backspace)],
    );
    assert_eq!(palette.prefix, None, "the chip is gone");
}

#[test]
fn esc_clears_the_marks_before_it_closes_the_palette() {
    let mut palette = windows_palette();
    let bridge = crate::core::ui_bridge::create();
    let snapshot = UiSnapshot::default();
    let ctx = drawing_context();
    palette.type_in("w ");
    frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
    frame(
        &mut palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![ctrl_key(egui::Key::Space)],
    );
    assert_eq!(palette.marks.windows(), [-7001]);
    frame(
        &mut palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![ctrl_key(egui::Key::A)],
    );
    assert_eq!(
        palette.marks.windows(),
        [-7001, -7002],
        "Ctrl+A marks every window shown"
    );
    let outcome = frame(
        &mut palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![key(egui::Key::Escape)],
    );
    assert!(matches!(outcome, Outcome::Stay));
    assert!(palette.marks.is_empty());
    let outcome = frame(
        &mut palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![key(egui::Key::Escape)],
    );
    assert!(matches!(outcome, Outcome::Hide));
    assert!(bridge.host_rx.try_recv().is_err(), "no window was acted on");
}

#[test]
fn nothing_matching_shows_the_try_instead_rows() {
    let mut palette = Palette::new(Router::new(Vec::new(), &Default::default()));
    let bridge = crate::core::ui_bridge::create();
    let snapshot = UiSnapshot::default();
    palette.opened();
    palette.type_in("zqx");
    frame(
        &mut palette,
        &drawing_context(),
        &snapshot,
        &bridge,
        Vec::new(),
    );
    assert!(palette.trying);
    let titles: Vec<&str> = palette
        .results
        .items
        .iter()
        .map(|item| item.title.as_str())
        .collect();
    assert_eq!(
        titles,
        ["Ask AI about \u{201c}zqx\u{201d}", "Search the web"]
    );
}

#[test]
fn ctrl_comma_and_ctrl_p_open_settings() {
    let mut palette = windows_palette();
    let bridge = crate::core::ui_bridge::create();
    let snapshot = UiSnapshot::default();
    let ctx = drawing_context();
    frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
    let outcome = frame(
        &mut palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![ctrl_key(egui::Key::Comma)],
    );
    assert!(matches!(outcome, Outcome::OpenSettings(Page::General)));
    let outcome = frame(
        &mut palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![ctrl_key(egui::Key::P)],
    );
    assert!(matches!(outcome, Outcome::OpenSettings(Page::Plugins)));
}

#[test]
fn the_right_arrow_walks_into_the_actions_and_enter_runs_the_one_there() {
    let folder = scratch_folder("actions").display().to_string();
    let bridge = crate::core::ui_bridge::create();
    let snapshot = UiSnapshot::default();
    let ctx = drawing_context();
    let mut palette = places_palette(&[&folder], &new_store());
    palette.query.clear();
    palette.searched = None;
    palette.type_in("x");
    frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
    frame(
        &mut palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![key(egui::Key::ArrowRight)],
    );
    assert_eq!(palette.action_cursor, Some(0));
    frame(
        &mut palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![key(egui::Key::ArrowDown)],
    );
    assert_eq!(palette.action_cursor, Some(1), "on Ctrl+B's bookmark");
    frame(
        &mut palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![key(egui::Key::ArrowUp)],
    );
    assert_eq!(palette.action_cursor, Some(0));
    let outcome = frame(
        &mut palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![key(egui::Key::Enter)],
    );
    assert!(matches!(outcome, Outcome::Hide));
    // The folder tip's state may go to the host first.
    assert!(std::iter::from_fn(|| bridge.host_rx.try_recv().ok())
        .any(|request| matches!(request, HostRequest::RunAction(Action::Open(_)))));
    std::fs::remove_dir_all(&folder).ok();
}

#[test]
fn esc_in_the_actions_goes_back_to_the_list_first() {
    let mut palette = windows_palette();
    let bridge = crate::core::ui_bridge::create();
    let snapshot = UiSnapshot::default();
    let ctx = drawing_context();
    palette.set_narrow(true);
    palette.type_in("w ");
    frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
    frame(
        &mut palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![key(egui::Key::ArrowRight)],
    );
    assert!(
        palette.overlay,
        "on a narrow monitor the inspector opens over the list"
    );
    let outcome = frame(
        &mut palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![key(egui::Key::Escape)],
    );
    assert!(matches!(outcome, Outcome::Stay));
    assert!(!palette.overlay);
    assert_eq!(palette.action_cursor, None);
}

#[test]
fn a_narrow_palette_is_the_list_alone() {
    let mut palette = windows_palette();
    assert_eq!(palette.panel_width(), PANEL_WIDTH);
    palette.set_narrow(true);
    assert_eq!(palette.panel_width(), PANEL_WIDTH - inspector::WIDTH);
    assert_eq!(
        palette.window_width(),
        PANEL_WIDTH - inspector::WIDTH + 2.0 * MARGIN
    );
}

#[test]
fn with_the_glass_the_window_is_the_panel() {
    let mut palette = windows_palette();
    assert_eq!(palette.window_height(), HEIGHT);
    palette.set_glass(true);
    assert!(palette.glass());
    assert_eq!(palette.margin(), 0.0);
    assert_eq!(palette.window_width(), PANEL_WIDTH);
    assert_eq!(palette.window_height(), PANEL_HEIGHT);
    let bridge = crate::core::ui_bridge::create();
    let ctx = drawing_context();
    frame(
        &mut palette,
        &ctx,
        &UiSnapshot::default(),
        &bridge,
        Vec::new(),
    );
    palette.set_glass(false);
    assert_eq!(palette.window_width(), WIDTH);
}

#[test]
fn the_enter_pill_word_comes_from_the_first_action() {
    let mut palette = windows_palette();
    palette.type_in("w ");
    let bridge = crate::core::ui_bridge::create();
    frame(
        &mut palette,
        &drawing_context(),
        &UiSnapshot::default(),
        &bridge,
        Vec::new(),
    );
    let actions = palette.current_actions();
    assert_eq!(actions::enter_verb(&actions[0].label), "Switch");
}

/// A provider that answers every query with the rows it was given.
struct Rows(Vec<ResultItem>);

impl crate::search::SearchProvider for Rows {
    fn id(&self) -> &'static str {
        "rows"
    }
    fn name(&self) -> &'static str {
        "Rows"
    }
    fn description(&self) -> &'static str {
        "test rows"
    }
    fn blended(&self) -> bool {
        true
    }
    fn query(&mut self, _query: &crate::search::Query, _context: &Context) -> Vec<ResultItem> {
        self.0.clone()
    }
}

fn palette_over(rows: Vec<ResultItem>) -> Palette {
    let mut palette = Palette::new(Router::new(
        vec![(None, Box::new(Rows(rows)))],
        &Default::default(),
    ));
    palette.opened();
    palette.type_in("x");
    let bridge = crate::core::ui_bridge::create();
    frame(
        &mut palette,
        &drawing_context(),
        &UiSnapshot::default(),
        &bridge,
        Vec::new(),
    );
    palette
}

fn titled(title: &str) -> ResultItem {
    ResultItem {
        group: "Rows".to_string(),
        title: title.to_string(),
        ..Default::default()
    }
}

#[test]
fn tab_on_a_row_with_a_completion_fills_the_box_and_runs_nothing() {
    let mut row = titled("alpha");
    row.tab = Some(crate::search::Completion {
        label: "Complete".to_string(),
        text: "alpha beta".to_string(),
    });
    let mut palette = palette_over(vec![row]);
    assert_eq!(palette.complete(), None);
    assert_eq!(palette.query, "alpha beta");
}

#[test]
fn tab_on_a_prefix_row_switches_to_that_prefix() {
    let mut row = titled("alpha");
    row.enter = Some(Choice {
        label: "Use".to_string(),
        action: Action::SetPrefix(">".to_string()),
    });
    let mut palette = palette_over(vec![row]);
    assert_eq!(palette.complete(), Some(Action::SetPrefix(">".to_string())));
}

#[test]
fn tab_on_a_row_with_a_tab_action_gives_that_action() {
    let mut row = titled("alpha");
    row.tab_action = Some(Choice {
        label: "Copy".to_string(),
        action: Action::Copy("alpha".to_string()),
    });
    let mut palette = palette_over(vec![row]);
    assert_eq!(
        palette.tab_action().map(|choice| choice.label),
        Some("Copy".to_string())
    );
    assert_eq!(palette.complete(), Some(Action::Copy("alpha".to_string())));
}

#[test]
fn tab_on_a_row_with_nothing_to_complete_does_nothing() {
    let mut palette = palette_over(vec![titled("alpha")]);
    assert_eq!(palette.tab_action(), None);
    assert_eq!(palette.complete(), None);
    assert_eq!(palette.query, "x");
    let mut empty = palette_over(Vec::new());
    assert_eq!(empty.complete(), None);
}

#[test]
fn a_new_command_list_or_cleared_usage_makes_the_next_search_start_again() {
    let mut palette = palette_over(vec![titled("alpha")]);
    palette.searched = Some((None, "x".to_string()));
    palette.tips_synced = true;
    palette.invalidate();
    assert!(palette.searched.is_none());
    assert!(!palette.tips_synced);
    palette.searched = Some((None, "x".to_string()));
    palette.clear_usage();
    assert!(palette.searched.is_none());
    palette.flush_usage();
}

#[test]
fn the_tips_are_taken_from_the_first_snapshot_only() {
    let mut palette = palette_over(Vec::new());
    palette.invalidate();
    let mut first = UiSnapshot::default();
    first.tips.show = false;
    palette.sync_tips(&first);
    assert!(!palette.tips.show);
    let mut second = UiSnapshot::default();
    second.tips.show = true;
    palette.sync_tips(&second);
    assert!(!palette.tips.show, "ours are sent back whole");
    palette.invalidate();
    palette.sync_tips(&second);
    assert!(palette.tips.show);
}

/// A provider whose `act` answers with the reply its command names.
struct Replies;

impl crate::search::SearchProvider for Replies {
    fn id(&self) -> &'static str {
        "replies"
    }
    fn name(&self) -> &'static str {
        "Replies"
    }
    fn description(&self) -> &'static str {
        "test replies"
    }
    fn default_prefix(&self) -> Option<&'static str> {
        Some("r")
    }
    fn query(&mut self, _query: &crate::search::Query, _context: &Context) -> Vec<ResultItem> {
        Vec::new()
    }
    fn act(&mut self, command: &str, _context: &Context) -> crate::search::Reply {
        use crate::search::Reply;
        match command {
            "clear" => Reply::ClearQuery,
            "hide" => Reply::Hide,
            "switch" => Reply::Switch {
                provider: "replies",
                text: "after".to_string(),
            },
            "ask" => Reply::Ask(crate::search::Ask {
                provider: "replies",
                chip: "Name".to_string(),
                placeholder: "Type a name".to_string(),
                text: "old".to_string(),
                action: "Save as".to_string(),
                subtitle: "".to_string(),
                payload: "p".to_string(),
            }),
            _ => Reply::Stay,
        }
    }
}

fn reply_palette() -> Palette {
    let mut palette = Palette::new(Router::new(
        vec![(None, Box::new(Replies))],
        &Default::default(),
    ));
    palette.opened();
    palette
}

fn provider_action(command: &str) -> Action {
    Action::Provider {
        provider: "replies".to_string(),
        command: command.to_string(),
    }
}

#[test]
fn a_provider_reply_decides_what_the_palette_does_next() {
    let bridge = crate::core::ui_bridge::create();
    let snapshot = UiSnapshot::default();
    let mut palette = reply_palette();
    palette.set_query("typed".to_string());
    let outcome = palette.perform(provider_action("stay"), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Stay));
    assert!(palette.searched.is_none());
    let outcome = palette.perform(provider_action("clear"), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Stay));
    assert_eq!(palette.query, "");
    let outcome = palette.perform(provider_action("hide"), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Hide));
    let outcome = palette.perform(provider_action("switch"), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Stay));
    assert_eq!(palette.prefix.as_deref(), Some("r"));
    assert_eq!(palette.query, "after");
    let outcome = palette.perform(provider_action("ask"), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Stay));
    assert!(palette.prompt.is_some());
    assert!(
        bridge.host_rx.try_recv().is_err(),
        "nothing went to the host"
    );
}

#[test]
fn choosing_a_command_or_another_action_goes_to_the_host_and_hides_the_palette() {
    let bridge = crate::core::ui_bridge::create();
    let snapshot = UiSnapshot::default();
    let mut palette = reply_palette();
    let id =
        crate::core::ui_bridge::CommandId::Host(crate::core::ui_bridge::HostCommand::OpenSettings);
    let outcome = palette.perform(Action::Command(id), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Hide));
    assert!(matches!(
        bridge.host_rx.try_recv().ok(),
        Some(HostRequest::RunCommand(_))
    ));
    let outcome = palette.perform(Action::Copy("text".into()), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Hide));
    assert!(matches!(
        bridge.host_rx.try_recv().ok(),
        Some(HostRequest::RunAction(Action::Copy(_)))
    ));
}

#[test]
fn opening_a_plugin_a_prefix_or_an_unknown_shortcut_stays_in_the_palette_logic() {
    let bridge = crate::core::ui_bridge::create();
    let snapshot = UiSnapshot::default();
    let mut palette = reply_palette();
    let outcome = palette.perform(
        Action::OpenPlugin("quick_jump".to_string()),
        &snapshot,
        &bridge.to_host,
    );
    assert!(matches!(outcome, Outcome::OpenPlugin(id) if id == "quick_jump"));
    let outcome = palette.perform(
        Action::SetPrefix("r".to_string()),
        &snapshot,
        &bridge.to_host,
    );
    assert!(matches!(outcome, Outcome::Stay));
    assert_eq!(palette.prefix.as_deref(), Some("r"));
    palette.perform(Action::SetPrefix(String::new()), &snapshot, &bridge.to_host);
    assert_eq!(palette.prefix, None);
    let outcome = palette.perform(
        Action::ChangeShortcut {
            id: "nobody.home".to_string(),
            label: "Nobody".to_string(),
        },
        &snapshot,
        &bridge.to_host,
    );
    assert!(
        matches!(outcome, Outcome::OpenShortcut { id, label } if id == "nobody.home" && label == "Nobody")
    );
    assert!(palette.hotkey_dialog.is_none());
}

#[test]
fn running_a_completion_fills_the_box_and_forgetting_with_no_row_does_nothing() {
    let bridge = crate::core::ui_bridge::create();
    let snapshot = UiSnapshot::default();
    let mut palette = reply_palette();
    let outcome = palette.run(
        Run::Complete("hello".to_string()),
        &snapshot,
        &bridge.to_host,
    );
    assert!(matches!(outcome, Outcome::Stay));
    assert_eq!(palette.query, "hello");
    let outcome = palette.run(Run::Forget, &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Stay));
    let outcome = palette.run(Run::Mark, &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Stay));
    let outcome = palette.run(
        Run::Action(Action::SetPrefix("r".to_string())),
        &snapshot,
        &bridge.to_host,
    );
    assert!(matches!(outcome, Outcome::Stay));
}

#[test]
fn the_glass_panel_takes_windows_own_corner_radius_so_no_glass_shows_at_the_corners() {
    let fluent = Tokens::of(crate::core::config::ThemeName::Fluent, true);
    assert_eq!(fluent.radius_window, 12);
    assert_eq!(
        with_window_corners(fluent, true).radius_window,
        GLASS_CORNER_RADIUS
    );
    assert_eq!(with_window_corners(fluent, false).radius_window, 12);
}
