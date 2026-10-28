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

/// A provider that suggests one open row at once and two more when its
/// "index" is ready, like the apps provider.
struct Late {
    ready: Arc<std::sync::atomic::AtomicBool>,
    told: bool,
}

impl crate::search::SearchProvider for Late {
    fn id(&self) -> &'static str {
        "late"
    }
    fn name(&self) -> &'static str {
        "Late"
    }
    fn description(&self) -> &'static str {
        "test rows"
    }
    fn blended(&self) -> bool {
        true
    }
    fn suggestions(&mut self, _context: &Context) -> Vec<ResultItem> {
        let count = if self.ready.load(std::sync::atomic::Ordering::SeqCst) {
            3
        } else {
            1
        };
        (0..count)
            .map(|n| ResultItem {
                title: format!("row {n}"),
                open: true,
                ..Default::default()
            })
            .collect()
    }
    fn index_news(&mut self) -> bool {
        let ready = self.ready.load(std::sync::atomic::Ordering::SeqCst);
        ready && !std::mem::replace(&mut self.told, true)
    }
    fn query(&mut self, _query: &crate::search::Query, _context: &Context) -> Vec<ResultItem> {
        Vec::new()
    }
}

fn late_palette() -> (Palette, Arc<std::sync::atomic::AtomicBool>) {
    let ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let provider = Late {
        ready: Arc::clone(&ready),
        told: false,
    };
    let mut palette = Palette::new(Router::new(
        vec![(None, Box::new(provider))],
        &Default::default(),
    ));
    palette.opened();
    (palette, ready)
}

#[test]
fn an_empty_view_is_built_again_when_the_index_arrives_and_the_row_stays() {
    let (mut palette, ready) = late_palette();
    let (ctx, snapshot) = (egui::Context::default(), UiSnapshot::default());
    palette.refresh(&snapshot, &ctx);
    assert_eq!(palette.results.items.len(), 1);

    ready.store(true, std::sync::atomic::Ordering::SeqCst);
    palette.refresh(&snapshot, &ctx);
    assert_eq!(palette.results.items.len(), 3);
    assert_eq!(palette.selected_item().unwrap().title, "row 0");
}

#[test]
fn a_row_the_user_moved_to_is_followed_when_the_view_is_built_again() {
    let (mut palette, ready) = late_palette();
    let (ctx, snapshot) = (egui::Context::default(), UiSnapshot::default());
    palette.refresh(&snapshot, &ctx);
    palette.results.items.insert(
        0,
        ResultItem {
            title: "new first".to_string(),
            open: true,
            ..Default::default()
        },
    );
    palette.selected = 1;
    let kept = palette.selected_item().unwrap().title.clone();
    ready.store(true, std::sync::atomic::Ordering::SeqCst);
    palette.refresh(&snapshot, &ctx);
    assert_eq!(palette.selected_item().unwrap().title, kept);
}

#[test]
fn a_typed_list_is_left_alone_when_the_index_arrives() {
    let (mut palette, ready) = late_palette();
    let (ctx, snapshot) = (egui::Context::default(), UiSnapshot::default());
    palette.type_in("zzz");
    palette.refresh(&snapshot, &ctx);
    let before = palette.results.items.len();
    ready.store(true, std::sync::atomic::Ordering::SeqCst);
    palette.refresh(&snapshot, &ctx);
    assert_eq!(palette.results.items.len(), before);
    assert_eq!(palette.searched.as_ref().map(|s| s.1.as_str()), Some("zzz"));
}

fn plugin_row(action: u32) -> (crate::core::ui_bridge::CommandId, ResultItem) {
    let id = crate::core::ui_bridge::CommandId::Plugin {
        index: 0,
        kind: crate::core::ui_bridge::ActionKind::Palette,
        action,
    };
    let item = ResultItem {
        title: "System: Empty Recycle Bin".to_string(),
        enter: Some(Choice {
            label: "Run".to_string(),
            action: Action::Command(id),
        }),
        ..Default::default()
    };
    (id, item)
}

fn asking_snapshot(id: crate::core::ui_bridge::CommandId) -> UiSnapshot {
    UiSnapshot {
        commands: vec![crate::core::ui_bridge::PaletteEntry {
            id,
            group: "QuickActions".to_string(),
            label: "System: Empty Recycle Bin".to_string(),
            hint: String::new(),
            words: String::new(),
            confirm: "Press Enter again to empty it".to_string(),
            list: false,
            disabled: false,
            plugin: Some("quick_actions".to_string()),
            subtitle: None,
            shortcut_id: String::new(),
            shortcut: String::new(),
            quiet: false,
            badge: None,
            toggle: None,
        }],
        ..Default::default()
    }
}

#[test]
fn a_selected_plugin_command_asks_the_host_once_and_shows_its_answer() {
    let bridge = crate::core::ui_bridge::create();
    let mut palette = reply_palette();
    let (id, item) = plugin_row(35);
    palette.results.items = vec![item];
    palette.selected = 0;
    palette.refresh_command_details(&bridge.to_host);
    palette.refresh_command_details(&bridge.to_host);
    let asked: Vec<HostRequest> = std::iter::from_fn(|| bridge.host_rx.try_recv().ok()).collect();
    assert_eq!(asked.len(), 1, "once per selection");
    assert!(matches!(asked[0], HostRequest::CommandDetails(asked_id) if asked_id == id));
    assert!(palette.shown_command_details().is_none(), "no answer yet");
    let (other, _) = plugin_row(1);
    palette.set_command_details(other, Some(CommandDetails::default()));
    assert!(
        palette.shown_command_details().is_none(),
        "an old answer is dropped"
    );
    let answer = CommandDetails {
        status: "12 items".to_string(),
        lines: Vec::new(),
    };
    palette.set_command_details(id, Some(answer.clone()));
    assert_eq!(palette.shown_command_details(), Some(&answer));
    palette.opened();
    assert!(palette.command_details.is_none());
}

#[test]
fn a_command_that_asks_first_runs_on_the_second_enter_only() {
    let bridge = crate::core::ui_bridge::create();
    let mut palette = reply_palette();
    let (id, item) = plugin_row(35);
    palette.results.items = vec![item];
    palette.selected = 0;
    let snapshot = asking_snapshot(id);
    let outcome = palette.perform(Action::Command(id), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Stay));
    assert!(bridge.host_rx.try_recv().is_err(), "nothing ran yet");
    assert_eq!(
        palette.armed_text(&snapshot).as_deref(),
        Some("Press Enter again to empty it")
    );
    let outcome = palette.perform(Action::Command(id), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Hide));
    assert!(matches!(
        bridge.host_rx.try_recv().ok(),
        Some(HostRequest::RunCommand(ran)) if ran == id
    ));
    assert!(palette.armed_text(&snapshot).is_none());

    // A first Enter that is too old counts as a new first one.
    palette.armed = Some((id, String::new(), Instant::now() - state::CONFIRM_FOR));
    assert!(palette.armed_text(&snapshot).is_none());
    let outcome = palette.perform(Action::Command(id), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Stay));
    // Any other command runs at once.
    let (plain, _) = plugin_row(1);
    let outcome = palette.perform(Action::Command(plain), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Hide));
}

fn list_snapshot(id: crate::core::ui_bridge::CommandId) -> UiSnapshot {
    let mut snapshot = asking_snapshot(id);
    snapshot.commands[0].confirm.clear();
    snapshot.commands[0].list = true;
    snapshot
}

fn port_rows() -> SubList {
    SubList {
        title: "Kill process on port".to_string(),
        rows: vec![
            ListRow {
                id: "50:3000".to_string(),
                title: "Port 3000".to_string(),
                detail: "node.exe".to_string(),
                confirm: "Press Enter again to end node.exe".to_string(),
                current: false,
            },
            ListRow {
                id: "60:5432".to_string(),
                title: "Port 5432".to_string(),
                detail: "postgres.exe".to_string(),
                confirm: String::new(),
                current: true,
            },
        ],
        empty: "No program listens".to_string(),
    }
}

#[test]
fn a_command_with_rows_shows_them_in_place_and_esc_goes_back() {
    let bridge = crate::core::ui_bridge::create();
    let (ctx, mut palette) = (egui::Context::default(), reply_palette());
    let (id, _) = plugin_row(40);
    let snapshot = list_snapshot(id);
    palette.set_query("kill".to_string());
    let outcome = palette.perform(Action::Command(id), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Stay));
    assert!(matches!(
        bridge.host_rx.try_recv().ok(),
        Some(HostRequest::OpenList(asked)) if asked == id
    ));
    assert_eq!(palette.query, "", "the box is free for filtering");
    palette.refresh(&snapshot, &ctx);
    assert_eq!(palette.results.items[0].title, "Loading\u{2026}");
    assert!(palette.results.items[0].disabled);

    palette.set_command_list(id, Some(port_rows()));
    palette.refresh(&snapshot, &ctx);
    assert_eq!(palette.results.items.len(), 2);
    assert_eq!(palette.selected, 1, "the row in use starts selected");
    assert_eq!(palette.results.items[1].badge.as_deref(), Some("In use"));

    palette.set_query("3000".to_string());
    palette.refresh(&snapshot, &ctx);
    assert_eq!(palette.results.items.len(), 1);
    assert_eq!(palette.results.items[0].title, "Port 3000");
    palette.set_query("nothing".to_string());
    palette.refresh(&snapshot, &ctx);
    assert_eq!(
        palette.results.items[0].title,
        "Nothing matches what you typed"
    );

    assert!(palette.leave_sub_list());
    assert_eq!(palette.query, "kill", "Esc puts the search back");
    assert!(!palette.leave_sub_list());
}

#[test]
fn a_risky_row_runs_on_the_second_enter_and_a_plain_one_at_once() {
    let bridge = crate::core::ui_bridge::create();
    let (ctx, mut palette) = (egui::Context::default(), reply_palette());
    let (id, _) = plugin_row(40);
    let snapshot = list_snapshot(id);
    palette.perform(Action::Command(id), &snapshot, &bridge.to_host);
    bridge.host_rx.try_recv().ok();
    palette.set_command_list(id, Some(port_rows()));
    palette.refresh(&snapshot, &ctx);
    palette.selected = 0;
    let pick = |row: &str| Action::ListPick {
        command: id,
        row: row.to_string(),
    };
    let outcome = palette.perform(pick("50:3000"), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Stay));
    assert!(bridge.host_rx.try_recv().is_err());
    assert_eq!(
        palette.armed_text(&snapshot).as_deref(),
        Some("Press Enter again to end node.exe")
    );
    let outcome = palette.perform(pick("50:3000"), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Hide));
    assert!(matches!(
        bridge.host_rx.try_recv().ok(),
        Some(HostRequest::ListPick { row, .. }) if row == "50:3000"
    ));
    assert!(palette.sub_list.is_none());

    palette.perform(Action::Command(id), &snapshot, &bridge.to_host);
    bridge.host_rx.try_recv().ok();
    palette.set_command_list(id, None);
    palette.refresh(&snapshot, &ctx);
    assert_eq!(
        palette.results.items[0].title,
        "This command has nothing to offer now"
    );
    palette.set_command_list(id, Some(port_rows()));
    let outcome = palette.perform(pick("60:5432"), &snapshot, &bridge.to_host);
    assert!(
        matches!(outcome, Outcome::Hide),
        "no confirm text, no second Enter"
    );
}

fn toggle_snapshot(id: crate::core::ui_bridge::CommandId) -> UiSnapshot {
    let mut snapshot = asking_snapshot(id);
    snapshot.commands[0].confirm.clear();
    snapshot.commands[0].label = "File Explorer: Toggle Hidden Files".to_string();
    snapshot.commands[0].subtitle = Some("Shows or hides hidden files".to_string());
    snapshot.commands[0].toggle = Some(ToggleLook {
        icon_on: "eye",
        icon_off: "eye-off",
        tile: [0xb8, 0x86, 0x2e],
    });
    snapshot
}

#[test]
fn enter_on_a_toggle_runs_it_and_the_palette_stays_open() {
    let bridge = crate::core::ui_bridge::create();
    let mut palette = reply_palette();
    let (id, _) = plugin_row(23);
    let snapshot = toggle_snapshot(id);
    let outcome = palette.perform(Action::Command(id), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Stay));
    assert!(matches!(
        bridge.host_rx.try_recv().ok(),
        Some(HostRequest::RunCommand(ran)) if ran == id
    ));
    let (plain, _) = plugin_row(2);
    let outcome = palette.perform(Action::Command(plain), &snapshot, &bridge.to_host);
    assert!(
        matches!(outcome, Outcome::Hide),
        "a normal command closes it"
    );
}

#[test]
fn enter_on_a_host_toggle_runs_it_and_the_palette_stays_open() {
    use crate::core::ui_bridge::HostCommand;
    let bridge = crate::core::ui_bridge::create();
    let mut palette = reply_palette();
    let id = CommandId::Host(HostCommand::DetailedLog);
    let snapshot = toggle_snapshot(id);
    let outcome = palette.perform(Action::Command(id), &snapshot, &bridge.to_host);
    assert!(matches!(outcome, Outcome::Stay));
    assert!(matches!(
        bridge.host_rx.try_recv().ok(),
        Some(HostRequest::RunCommand(ran)) if ran == id
    ));
    let mut item = titled("Detailed log");
    item.enter = Some(Choice {
        label: "Run".to_string(),
        action: Action::Command(id),
    });
    palette.set_toggle_states(vec![(id, ToggleState::on("Recording"))]);
    let shown = rows::row_toggle(&item, &snapshot, &palette.toggle_states).expect("a toggle");
    assert_eq!(shown.on(), Some(true));
}

#[test]
fn a_toggle_row_shows_the_state_the_host_sent_and_forgets_it_on_the_next_opening() {
    let mut palette = reply_palette();
    let (id, item) = plugin_row(23);
    let snapshot = toggle_snapshot(id);

    let waiting = rows::row_toggle(&item, &snapshot, &palette.toggle_states).expect("a toggle");
    assert_eq!(waiting.on(), None, "no switch before the host answers");
    assert_eq!(waiting.line("Shows or hides"), "Shows or hides");
    assert_eq!(waiting.icon(), Icon::EyeOff);

    palette.command_details = Some((id, Some(CommandDetails::default())));
    palette.set_toggle_states(vec![(id, ToggleState::on("Hidden files are shown"))]);
    assert!(
        palette.command_details.is_none(),
        "the inspector asks again after a flip"
    );
    let shown = rows::row_toggle(&item, &snapshot, &palette.toggle_states).unwrap();
    assert_eq!(shown.on(), Some(true));
    assert_eq!(shown.line("Shows or hides"), "Hidden files are shown");
    assert_eq!(shown.icon(), Icon::Eye);

    palette.set_toggle_states(vec![(id, ToggleState::unknown())]);
    let unknown = rows::row_toggle(&item, &snapshot, &palette.toggle_states).unwrap();
    assert_eq!(unknown.on(), None, "no switch when the state is unknown");
    assert_eq!(unknown.line(""), "State unknown");
    assert_eq!(unknown.icon(), Icon::EyeOff, "the tile shows off");

    palette.opened();
    assert!(
        palette.toggle_states.is_empty(),
        "read again on every opening"
    );

    let (plain, plain_item) = plugin_row(2);
    assert!(rows::row_toggle(&plain_item, &toggle_snapshot(plain), &[]).is_some());
    assert!(rows::row_toggle(&plain_item, &asking_snapshot(plain), &[]).is_none());
    assert!(rows::row_toggle(&titled("Calculator"), &snapshot, &[]).is_none());
}

#[test]
fn toggle_rows_draw_with_and_without_a_switch_in_every_theme_without_panicking() {
    let (id, item) = plugin_row(23);
    let snapshot = toggle_snapshot(id);
    let states = [
        None,
        Some(ToggleState::on("Hidden files are shown")),
        Some(ToggleState::off("Hidden files are not shown")),
        Some(ToggleState::without_switch("No window selected")),
    ];
    let themes = crate::core::config::ThemeName::ALL
        .into_iter()
        .flat_map(|name| {
            [
                crate::core::config::ThemeChoice::Light,
                crate::core::config::ThemeChoice::Dark,
            ]
            .map(|mode| (name, mode))
        });
    for ((name, mode), state) in themes.flat_map(|theme| states.iter().map(move |s| (theme, s))) {
        let ctx = drawing_context();
        theme::apply(&ctx, name, mode);
        let input = egui::RawInput {
            screen_rect: Some(window()),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let toggle = RowToggle {
                look: snapshot.commands[0].toggle.unwrap(),
                state: state.as_ref(),
            };
            let look = RowLook {
                needle: "",
                selected: true,
                interactive: true,
                column: false,
                marked: false,
                best: false,
                verb: Some("Run"),
                toggle: Some(toggle),
            };
            let mut icons = IconCache::new(ui.ctx());
            let row = result_row(ui, &item, &look, &mut icons);
            assert!(row.rect.height() >= 44.0);
        });
        output.textures_delta.clear();
    }
}

fn update_snapshot(tag: &str) -> UiSnapshot {
    UiSnapshot {
        version: "0.20.59".to_string(),
        update: crate::core::ui_bridge::UpdateView {
            available: Some(crate::core::ui_bridge::NewRelease {
                tag: tag.to_string(),
                notes: "- Toggle rows show their state.".to_string(),
                page: "https://github.com/OliverD25/wincraft/releases".to_string(),
            }),
            ..Default::default()
        },
        ..UiSnapshot::default()
    }
}

fn titles(palette: &Palette) -> Vec<&str> {
    palette
        .results
        .items
        .iter()
        .map(|item| item.title.as_str())
        .collect()
}

#[test]
fn the_update_row_sits_above_recent_and_the_first_recent_row_stays_selected() {
    let mut palette = windows_palette();
    let bridge = crate::core::ui_bridge::create();
    let ctx = drawing_context();
    let snapshot = update_snapshot("v0.20.60");
    frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
    assert_eq!(
        titles(&palette),
        ["Update WinCraft to 0.20.60", "Notes", "Inbox"]
    );
    assert_eq!(palette.selected, 1, "Enter still does what it did");
    assert_eq!(palette.pinned_spot(), actions::Pinned::Below);
    let hints =
        actions::pinned_hints(&palette.current_actions(), palette.pinned_spot(), "Close").unwrap();
    let words: Vec<&str> = hints.iter().map(|hint| hint.label.as_str()).collect();
    assert_eq!(words, ["Switch", "Select update", "Move", "Close"]);

    // Up selects it; its actions are the update's.
    frame(
        &mut palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![key(egui::Key::ArrowUp)],
    );
    assert_eq!(palette.selected, 0);
    assert_eq!(palette.pinned_spot(), actions::Pinned::On);
    let actions = palette.current_actions();
    assert_eq!(actions[0].label, "Update now");
    assert_eq!(actions::enter_verb(&actions[0].label), "Update");
    let keys: Vec<Key> = actions.iter().map(|action| action.key).collect();
    assert_eq!(
        keys,
        [Key::Enter, Key::CtrlO, Key::CtrlDelete],
        "no Ctrl+K without a hotkey id"
    );
    let hints = actions::pinned_hints(&actions, palette.pinned_spot(), "Close").unwrap();
    let words: Vec<&str> = hints.iter().map(|hint| hint.label.as_str()).collect();
    assert_eq!(
        words,
        ["Update", "Move", "Hide until next version", "Close"]
    );
}

#[test]
fn typing_ranks_as_before_without_the_pinned_row() {
    let mut palette = windows_palette();
    let bridge = crate::core::ui_bridge::create();
    let ctx = drawing_context();
    let snapshot = update_snapshot("v0.20.60");
    palette.type_in("no");
    frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
    assert!(!titles(&palette).contains(&"Update WinCraft to 0.20.60"));
    assert_eq!(palette.selected, 0);
    assert_eq!(palette.pinned_spot(), actions::Pinned::Away);

    // Back to an empty box: pinned again, the cursor under it.
    palette.set_query(String::new());
    frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
    assert_eq!(titles(&palette)[0], "Update WinCraft to 0.20.60");
    assert_eq!(palette.selected, 1);
}

#[test]
fn the_row_comes_only_with_the_next_opening_never_while_open() {
    let mut palette = windows_palette();
    let bridge = crate::core::ui_bridge::create();
    let ctx = drawing_context();
    frame(
        &mut palette,
        &ctx,
        &UiSnapshot::default(),
        &bridge,
        Vec::new(),
    );
    assert_eq!(titles(&palette), ["Notes", "Inbox"]);
    // The check started at this opening answers while the palette is open.
    let found = update_snapshot("v0.20.60");
    palette.invalidate();
    frame(&mut palette, &ctx, &found, &bridge, Vec::new());
    assert_eq!(
        titles(&palette),
        ["Notes", "Inbox"],
        "the list does not change"
    );
    palette.opened();
    frame(&mut palette, &ctx, &found, &bridge, Vec::new());
    assert_eq!(titles(&palette)[0], "Update WinCraft to 0.20.60");
}

#[test]
fn ctrl_del_hides_the_row_and_tells_the_host_its_tag() {
    let mut palette = windows_palette();
    let bridge = crate::core::ui_bridge::create();
    let ctx = drawing_context();
    let snapshot = update_snapshot("v0.20.60");
    frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
    frame(
        &mut palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![key(egui::Key::ArrowUp)],
    );
    frame(
        &mut palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![ctrl_key(egui::Key::Delete)],
    );
    let hidden = bridge.host_rx.try_iter().find_map(|request| match request {
        HostRequest::HideUpdate(tag) => Some(tag),
        _ => None,
    });
    assert_eq!(hidden.as_deref(), Some("v0.20.60"));
    frame(&mut palette, &ctx, &snapshot, &bridge, Vec::new());
    assert_eq!(titles(&palette), ["Notes", "Inbox"]);
    assert_eq!(palette.selected, 0);
}

#[test]
fn a_copy_that_cannot_update_or_a_hidden_release_pins_nothing() {
    for change in [0, 1] {
        let mut palette = windows_palette();
        let bridge = crate::core::ui_bridge::create();
        let mut snapshot = update_snapshot("v0.20.60");
        if change == 0 {
            snapshot.update.blocked = Some("a development build".to_string());
        } else {
            snapshot.update.hidden = true;
        }
        frame(
            &mut palette,
            &drawing_context(),
            &snapshot,
            &bridge,
            Vec::new(),
        );
        assert_eq!(titles(&palette), ["Notes", "Inbox"], "{change}");
        assert_eq!(palette.selected, 0);
    }
}

/// Presses at `from`, then moves the pointer 12 px: what the window is told.
fn press_and_move(palette: &mut Palette, from: egui::Pos2) -> WindowStep {
    let bridge = crate::core::ui_bridge::create();
    let ctx = drawing_context();
    let snapshot = UiSnapshot::default();
    let button = |pressed| egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(palette, &ctx, &snapshot, &bridge, Vec::new());
    frame(
        palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![egui::Event::PointerMoved(from)],
    );
    frame(palette, &ctx, &snapshot, &bridge, vec![button(true)]);
    assert_eq!(palette.take_window_step(), WindowStep::None, "not yet");
    frame(
        palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![egui::Event::PointerMoved(from + vec2(12.0, 0.0))],
    );
    palette.take_window_step()
}

#[test]
fn holding_blank_space_and_moving_starts_a_window_drag() {
    let mut palette = places_palette(&["x"], &new_store());
    let list_below_the_row = pos2(MARGIN + 200.0, MARGIN + 300.0);
    assert_eq!(
        press_and_move(&mut palette, list_below_the_row),
        WindowStep::StartDrag
    );
    let mut palette = places_palette(&["x"], &new_store());
    let footer = pos2(MARGIN + 450.0, MARGIN + PANEL_HEIGHT - 6.0);
    assert_eq!(press_and_move(&mut palette, footer), WindowStep::StartDrag);
}

#[test]
fn a_drag_on_the_search_box_or_a_row_does_not_move_the_window() {
    let mut palette = places_palette(&["x"], &new_store());
    let search_box = pos2(MARGIN + 100.0, MARGIN + SEARCH_HEIGHT / 2.0);
    assert_eq!(press_and_move(&mut palette, search_box), WindowStep::None);
    let mut palette = places_palette(&["x"], &new_store());
    let row = pos2(MARGIN + 200.0, MARGIN + SEARCH_HEIGHT + GROUP_HEADER + 12.0);
    assert_eq!(press_and_move(&mut palette, row), WindowStep::None);
}

#[test]
fn the_shadow_margin_is_not_blank_space() {
    let mut palette = places_palette(&["x"], &new_store());
    assert_eq!(
        press_and_move(&mut palette, pos2(MARGIN / 2.0, MARGIN + 300.0)),
        WindowStep::None
    );
}

#[test]
fn a_double_click_on_blank_space_asks_for_the_centre() {
    let mut palette = places_palette(&["x"], &new_store());
    let bridge = crate::core::ui_bridge::create();
    let ctx = drawing_context();
    let snapshot = UiSnapshot::default();
    let at = pos2(MARGIN + 200.0, MARGIN + 300.0);
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(
        &mut palette,
        &ctx,
        &snapshot,
        &bridge,
        vec![egui::Event::PointerMoved(at)],
    );
    for pressed in [true, false, true, false] {
        frame(
            &mut palette,
            &ctx,
            &snapshot,
            &bridge,
            vec![button(pressed)],
        );
    }
    assert_eq!(palette.take_window_step(), WindowStep::Centre);
}

#[test]
fn the_press_state_machine_needs_a_fresh_press_after_a_drag() {
    use drag::{BlankPress, Pointer, Step};
    let mut press = BlankPress::default();
    let at = pos2(10.0, 10.0);
    let down = |pos: egui::Pos2, pressed: bool| Pointer {
        pressed,
        down: true,
        pos: Some(pos),
        in_panel: true,
        ..Default::default()
    };
    assert_eq!(press.step(down(at, true)), Step::None);
    assert_eq!(press.step(down(at + vec2(2.0, 0.0), false)), Step::None);
    assert_eq!(
        press.step(down(at + vec2(9.0, 0.0), false)),
        Step::StartDrag
    );
    // egui never sees the release of a native drag: no second start.
    assert_eq!(press.step(down(at + vec2(40.0, 0.0), false)), Step::None);
    // A press that a widget took never starts one.
    let taken = Pointer {
        taken: true,
        ..down(at, true)
    };
    assert_eq!(press.step(taken), Step::None);
    assert_eq!(press.step(down(at + vec2(30.0, 0.0), false)), Step::None);
}
