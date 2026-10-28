use super::*;
use crate::core::live_windows::{is_system_helper, known_desktop, SYSTEM_HELPERS};
use crate::core::window_identity::program_name;

#[test]
fn every_app_is_watched_except_the_excluded_ones_and_wincraft() {
    let watch = Watch::new("*", " Telegram.exe, claude.exe");
    assert!(watch.covers("chrome.exe"));
    assert!(watch.covers("CHARMAP.EXE"));
    assert!(!watch.covers("telegram.exe"));
    assert!(!watch.covers("claude.exe"));
    assert!(!watch.covers("wincraft.exe"));
    assert!(!watch.covers(""));
    assert_eq!(watch.to_string(), "any app except telegram.exe, claude.exe");

    let listed = Watch::new("chrome.exe, charmap.exe", "");
    assert!(listed.covers("charmap.exe"));
    assert!(!listed.covers("telegram.exe"));
    assert_eq!(listed.to_string(), "chrome.exe, charmap.exe");
    // The exclude list wins over a list that names the same program.
    assert!(!Watch::new("chrome.exe", "chrome.exe").covers("chrome.exe"));
}

#[test]
fn windows_helpers_are_matched_by_file_name_in_any_case() {
    for helper in SYSTEM_HELPERS {
        assert!(is_system_helper(helper), "{helper}");
        assert!(is_system_helper(&helper.to_lowercase()), "{helper}");
        assert!(is_system_helper(&helper.to_uppercase()), "{helper}");
    }
    assert!(is_system_helper(r"C:\Windows\System32\rundll32.exe"));
    assert!(is_system_helper(
        r"c:\windows\systemapps\shellexperiencehost_cw5n1h2txyewy\ShellExperienceHost.exe"
    ));
    assert!(is_system_helper("C:/Windows/System32/DllHost.exe"));
    // Only the whole file name counts.
    assert!(!is_system_helper("rundll32.exe.bak"));
    assert!(!is_system_helper("myrundll32.exe"));
    assert!(!is_system_helper(r"C:\Tools\rundll32\app.exe"));
    assert!(!is_system_helper("rundll32"));
    assert!(!is_system_helper(""));
}

#[test]
fn real_apps_are_not_helpers() {
    for app in [
        "SystemSettings.exe",
        "ApplicationFrameHost.exe",
        "explorer.exe",
        "WindowsTerminal.exe",
        "chrome.exe",
        "notepad.exe",
        "Taskmgr.exe",
    ] {
        assert!(!is_system_helper(app), "{app}");
        assert!(Watch::new("*", "").covers(app), "{app}");
    }
}

#[test]
fn helpers_are_skipped_whatever_the_program_list_says() {
    let every = Watch::new("*", "");
    let listed = Watch::new("rundll32.exe, dllhost.exe, chrome.exe", "");
    for watch in [&every, &listed] {
        assert!(!watch.covers("rundll32.exe"));
        assert!(!watch.covers("RUNDLL32.EXE"));
        assert!(!watch.covers("dllhost.exe"));
        assert!(!watch.covers("SearchHost.exe"));
        assert!(watch.covers("chrome.exe"));
    }
    assert!(!listed.covers("notepad.exe"));
}

#[test]
fn the_users_own_exclude_list_still_applies_next_to_the_helpers() {
    let watch = Watch::new("*", "telegram.exe");
    assert!(!watch.covers("telegram.exe"));
    assert!(!watch.covers("rundll32.exe"));
    assert!(watch.covers("viber.exe"));
    // Naming a helper in the exclude list changes nothing either way.
    let both = Watch::new("*", "rundll32.exe");
    assert!(!both.covers("rundll32.exe"));
    assert!(both.covers("chrome.exe"));
    assert_eq!(watch.to_string(), "any app except telegram.exe");
}

fn saved(group: &str) -> state::SavedWindow {
    state::SavedWindow {
        name: Some(group.to_string()),
        title: group.to_string(),
        rect: [0, 0, 800, 600],
        maximized: false,
        desktop: None,
        desktop_name: None,
        group: group.to_string(),
        taskbar_index: 0,
        z_index: 0,
        monitor: None,
        seen: None,
    }
}

fn old_file() -> Programs {
    let mut programs = Programs::new();
    for (exe, group) in [
        ("rundll32.exe", r"c:\windows\system32\rundll32.exe"),
        ("chrome.exe", "Chrome.UserData.Profile3"),
        ("dllhost.exe", r"c:\windows\system32\dllhost.exe"),
    ] {
        programs.insert(
            exe.to_string(),
            state::ProgramState {
                windows: vec![saved(group)],
            },
        );
    }
    programs
}

#[test]
fn a_helper_in_an_old_state_file_is_left_out_of_the_restore() {
    let mut keeper = LayoutKeeper {
        watch: Watch::new("*", ""),
        ..LayoutKeeper::default()
    };
    keeper.writer = state::Writer::new(
        std::env::temp_dir().join("wincraft-test-never-written.json"),
        Some(old_file()),
    );
    assert_eq!(keeper.saved_groups(), ["Chrome.UserData.Profile3"]);
}

#[test]
fn a_helper_in_an_old_state_file_is_dropped_at_the_next_save() {
    let watch = Watch::new("*", "");
    let merged = state::merge(Some(&old_file()), Programs::new(), false, &|exe| {
        watch.covers(exe)
    });
    assert_eq!(merged.keys().collect::<Vec<_>>(), ["chrome.exe"]);
}

fn read_only() -> LayoutKeeper {
    LayoutKeeper {
        read_only: true,
        watch: Watch::new("*", ""),
        ..LayoutKeeper::default()
    }
}

// A handle no window has: if a guard were missing, the call would reach
// Win32 with it and the refusal count would not move.
const NO_WINDOW: HWND = 0x7fff_fff0 as HWND;

#[test]
fn a_normal_instance_is_not_read_only() {
    // The flag follows instance::is_test, which reads WINCRAFT_INSTANCE
    // from the shell; this test must pass whatever the shell has set.
    assert_eq!(LayoutKeeper::default().read_only, instance::is_test());
    let mut keeper = LayoutKeeper {
        read_only: instance::is_test_suffix(""),
        ..LayoutKeeper::default()
    };
    assert!(!keeper.read_only);
    assert!(!keeper.refuses("anything"));
    assert_eq!(keeper.refused, 0);
}

#[test]
fn a_read_only_instance_starts_no_restore() {
    let mut keeper = read_only();
    keeper.writer = state::Writer::new(
        std::env::temp_dir().join("wincraft-test-never-written.json"),
        Some(old_file()),
    );
    keeper.start_restore(false);
    assert_eq!(keeper.refused, 1);
    assert!(!keeper.restore.is_running());
}

#[test]
fn a_read_only_instance_never_touches_the_taskbar_or_desktops() {
    let mut keeper = read_only();
    keeper.apply_group("Chrome.UserData.Profile3", true);
    assert!(keeper.taskbar.is_none(), "no ITaskbarList was made");
    keeper.move_to_desktop(NO_WINDOW, DesktopId(7), &[], "w");
    assert!(matches!(keeper.mover, Mover::Untried));
    keeper.shift_front(1);
    keeper.shift_front(-1);
    assert_eq!(keeper.refused, 4);
}

#[test]
fn a_read_only_instance_does_not_finish_a_restore() {
    let mut keeper = read_only();
    keeper.finish_restore(Finish {
        moves: vec![(NO_WINDOW, DesktopId(7), "w".to_string())],
        stack: vec![(0, NO_WINDOW)],
        foreground: NO_WINDOW,
        summary: "Restored 1 of 1 test windows".to_string(),
        announce: true,
        targets: Vec::new(),
        orders: Vec::new(),
    });
    assert_eq!(keeper.refused, 1);
    assert!(keeper.last_restore.is_none());
    assert!(keeper.checks.is_empty(), "nothing to check either");
}

#[test]
fn every_strip_action_is_turned_down_in_a_read_only_instance() {
    let mut keeper = read_only();
    let hwnd = NO_WINDOW as isize;
    let actions = [
        ArrangeAction::Activate(hwnd),
        ArrangeAction::Reorder {
            group: "Chrome.UserData.Profile3".to_string(),
            order: vec![hwnd],
        },
        ArrangeAction::MoveToDesktop {
            hwnd,
            desktop: "{00000000-0000-0000-0000-000000000007}".to_string(),
        },
        ArrangeAction::MoveToMonitor {
            hwnd,
            monitor: r"\\.\DISPLAY2".to_string(),
        },
        ArrangeAction::Close(hwnd),
    ];
    for (done, action) in actions.iter().enumerate() {
        keeper.on_arrange_action(action);
        assert_eq!(keeper.refused as usize, done + 1, "{action:?}");
    }
    assert!(keeper.reapply.is_empty());
    assert!(!keeper.arrange_stale);
}

#[test]
fn the_hotkeys_that_move_windows_are_turned_down_too() {
    let mut keeper = read_only();
    keeper.on_hotkey(ACTION_RESTORE);
    keeper.on_hotkey(ACTION_MOVE_LEFT);
    keeper.on_hotkey(ACTION_MOVE_RIGHT);
    keeper.on_tray_action(ACTION_RESTORE);
    assert_eq!(keeper.refused, 4);
    let status = keeper.status().unwrap();
    assert!(
        status.contains("Read-only test instance: 4 changes"),
        "{status}"
    );
}

#[test]
fn only_the_old_default_becomes_every_app() {
    let mut old = serde_json::json!({ "programs": "chrome.exe", "settle_seconds": 5 });
    assert!(migrate_programs(&mut old));
    assert_eq!(old["programs"], "*");
    assert_eq!(old["settle_seconds"], 5);

    for kept in ["chrome.exe, msedge.exe", "msedge.exe", "*"] {
        let mut own = serde_json::json!({ "programs": kept });
        assert!(!migrate_programs(&mut own), "{kept}");
        assert_eq!(own["programs"], kept);
    }
}

#[test]
fn only_the_old_restore_defaults_move_to_the_new_ones() {
    let mut old = serde_json::json!({
        "settle_seconds": 5,
        "restore_window_minutes": 3,
        "snapshot_interval_seconds": 5,
    });
    keeper_migrate(&mut old);
    assert_eq!(old["settle_seconds"], 3);
    assert_eq!(old["restore_window_minutes"], 30);
    // 15 was the default until windows were placed as they open.
    let mut before = serde_json::json!({ "settle_seconds": 15 });
    keeper_migrate(&mut before);
    assert_eq!(before["settle_seconds"], 3);
    assert_eq!(old["snapshot_interval_seconds"], 5);

    let mut own = serde_json::json!({ "settle_seconds": 8, "restore_window_minutes": 10 });
    keeper_migrate(&mut own);
    assert_eq!(own["settle_seconds"], 8);
    assert_eq!(own["restore_window_minutes"], 10);

    let mut new = LayoutKeeper::default().default_settings();
    keeper_migrate(&mut new);
    assert_eq!(new["settle_seconds"], 3);
    assert_eq!(new["restore_window_minutes"], 30);

    // A float written by the settings page counts the same as an integer.
    let mut float = serde_json::json!({ "settle_seconds": 5.0 });
    assert!(migrate_default(&mut float, "settle_seconds", 5, 15));
    assert_eq!(float["settle_seconds"], 15);
}

fn keeper_migrate(settings: &mut serde_json::Value) {
    LayoutKeeper::default().migrate_settings(settings);
}

#[test]
fn the_restore_runs_with_the_new_defaults() {
    let keeper = LayoutKeeper::default();
    assert_eq!(keeper.restore.settle_seconds, 3);
    assert_eq!(keeper.restore.give_up_seconds, 30 * 60);
    let mut applied = LayoutKeeper::default();
    applied.apply_settings(&keeper.default_settings());
    assert_eq!(applied.restore.settle_seconds, 3);
    assert_eq!(applied.restore.give_up_seconds, 30 * 60);
}

#[test]
fn monitors_are_restored_by_default() {
    let mut keeper = LayoutKeeper {
        restore_monitors: false,
        ..LayoutKeeper::default()
    };
    keeper.apply_settings(&LayoutKeeper::default().default_settings());
    assert!(keeper.restore_monitors);
    keeper.apply_settings(&serde_json::json!({ "restore_monitors": false }));
    assert!(!keeper.restore_monitors);
}

#[test]
fn the_strip_settings_and_hotkey_are_windowdecks_now() {
    let keeper = LayoutKeeper::default();
    let settings = keeper.default_settings();
    for key in [
        "taskbar_hover_strip",
        "hover_delay_seconds",
        "preview_on_hover",
        "arrange_view",
    ] {
        assert!(settings.get(key).is_none(), "{key}");
    }
    assert!(keeper.hotkey_actions().iter().all(|a| a.name != "arrange"));
    assert!(keeper.tray_toggles().is_empty());
    assert!(keeper.page_action().is_none());
}

#[test]
fn an_app_counts_its_windows_on_every_desktop() {
    let identity = |title: &str| identity::WindowIdentity {
        exe: "chrome.exe".to_string(),
        name: None,
        title: title.to_string(),
        rect: [0, 0, 800, 600],
        maximized: false,
        group: "Chrome.UserData.Profile3".to_string(),
    };
    let mut groups = BTreeMap::new();
    groups.insert(
        "Chrome.UserData.Profile3".to_string(),
        OrderModel::from_matched(vec![
            (identity("one"), Some(1)),
            (identity("two"), Some(2)),
            (identity("closed"), None),
        ]),
    );
    groups.insert(
        "Telegram.TelegramDesktop".to_string(),
        OrderModel::from_matched(vec![(identity("chat"), Some(3))]),
    );
    let census = census(&groups);
    assert_eq!(census.len(), 3, "closed windows are not counted");
    let count = |app| window_census::count_of(&census, app);
    assert_eq!(count("Chrome.UserData.Profile3"), 2);
    assert_eq!(count("Appid: chrome.userdata.profile3"), 2);
    assert_eq!(count("Telegram.TelegramDesktop"), 1);
    assert_eq!(count("md.obsidian"), 0);
}

#[test]
fn a_store_app_shown_on_all_desktops_counts_all_its_windows() {
    let claude = r"d:\windowsapps\claude_3.6556.2.0_x64__pzs8sxrjxfjjc\app\claude.exe";
    let identity = |title: &str| identity::WindowIdentity {
        exe: claude.to_string(),
        name: None,
        title: title.to_string(),
        rect: [0, 0, 800, 600],
        maximized: false,
        group: claude.to_string(),
    };
    let mut groups = BTreeMap::new();
    groups.insert(
        claude.to_string(),
        OrderModel::from_matched(
            (1..=10)
                .map(|n| (identity(&format!("chat {n}")), Some(n)))
                .collect(),
        ),
    );
    let census = census(&groups);
    assert_eq!(
        window_census::count_of(&census, "Claude_pzs8sxrjxfjjc!Claude"),
        10
    );
    assert_eq!(
        window_census::count_of(&census, "Appid: Claude_pzs8sxrjxfjjc!Claude"),
        10
    );

    let registry = vec![Desktop {
        id: DesktopId::parse("{11111111-2222-3333-4444-555555555555}").expect("a GUID"),
        name: "Desktop 1".to_string(),
    }];
    assert!(known_desktop(DesktopId::ALL, &registry), "all desktops");
    assert!(known_desktop(registry[0].id, &registry));
    let gone = DesktopId::parse("{99999999-2222-3333-4444-555555555555}").expect("a GUID");
    assert!(!known_desktop(gone, &[]));
}

fn saved_window(title: &str, group: &str, desktop: &str, seen: Option<&str>) -> SavedWindow {
    SavedWindow {
        title: title.to_string(),
        name: None,
        desktop: Some(desktop.to_string()),
        seen: seen.map(str::to_string),
        ..saved(group)
    }
}

const DESKTOP_A: &str = "{11111111-1111-1111-1111-111111111111}";
const DESKTOP_B: &str = "{22222222-2222-2222-2222-222222222222}";

fn layout_file() -> Programs {
    let mut programs = Programs::new();
    programs.insert(
        "chrome.exe".to_string(),
        ProgramState {
            windows: vec![
                saved_window(
                    "Inbox - Google Chrome",
                    "Chrome",
                    DESKTOP_A,
                    Some("2026-10-12"),
                ),
                saved_window("Docs - Google Chrome", "Chrome", DESKTOP_B, None),
            ],
        },
    );
    programs.insert(
        "viber.exe".to_string(),
        ProgramState {
            windows: vec![saved_window("Viber", "Viber.Desktop", DESKTOP_A, None)],
        },
    );
    programs
}

/// A keeper whose saved layout lives in its own scratch file.
fn keeper_with(programs: Programs, name: &str) -> (LayoutKeeper, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!(
        "wincraft-test-layout-{name}-{}.json",
        std::process::id()
    ));
    let mut keeper = LayoutKeeper {
        watch: Watch::new("*", ""),
        ..LayoutKeeper::default()
    };
    keeper.writer = state::Writer::new(path.clone(), Some(programs));
    (keeper, path)
}

fn entry_of(keeper: &mut LayoutKeeper, title: &str) -> LayoutRef {
    let snapshot = keeper.layout_snapshot().unwrap();
    snapshot
        .apps
        .iter()
        .flat_map(|app| &app.windows)
        .find(|window| window.entry.title == title)
        .map(|window| window.entry.clone())
        .unwrap()
}

#[test]
fn the_layout_page_lists_saved_windows_by_app_with_their_state() {
    let (mut keeper, _) = keeper_with(layout_file(), "list");
    let snapshot = keeper.layout_snapshot().unwrap();
    // The busiest app comes first.
    assert_eq!(snapshot.apps.len(), 2);
    assert_eq!(snapshot.apps[0].windows.len(), 2);
    assert_eq!(snapshot.apps[1].windows.len(), 1);
    let inbox = &snapshot.apps[0].windows[0];
    assert_eq!(inbox.title, "Inbox - Google Chrome");
    assert_eq!(inbox.hwnd, None, "no such window is open");
    assert_eq!(inbox.closed_since.as_deref(), Some("2026-10-12"));
    assert_eq!(snapshot.apps[0].windows[1].closed_since, None);
    // The desktops in the file are not the PC's own, so they count as gone.
    assert!(inbox.desktop_removed);
    assert_eq!(inbox.desktop, DESKTOP_A);
}

#[test]
fn a_saved_window_is_matched_to_the_open_one_it_names() {
    let program = ProgramState {
        windows: vec![saved_window("Docs", "Chrome", DESKTOP_A, None)],
    };
    assert_eq!(
        LayoutKeeper::open_matches("chrome.exe", &program, &[]),
        [None]
    );
    let live = windows::LiveWindow {
        hwnd: 4242 as HWND,
        identity: identity::WindowIdentity::new("chrome.exe", "Docs", [0, 0, 800, 600], false)
            .in_group("Chrome"),
        group: "Chrome".to_string(),
        app_name: None,
        exe_path: r"c:\chrome.exe".to_string(),
    };
    assert_eq!(
        LayoutKeeper::open_matches("chrome.exe", &program, &[live]),
        [Some(4242)]
    );
}

#[test]
fn a_new_desktop_is_saved_in_the_file() {
    let (mut keeper, path) = keeper_with(layout_file(), "desktop");
    let entry = entry_of(&mut keeper, "Docs - Google Chrome");
    // Desktop ids need to exist on this PC to be saved, so use the
    // saved id the file already has: the write itself is what is tested.
    keeper.on_layout_action(&LayoutAction::SetDesktop {
        entry: entry.clone(),
        desktop: DESKTOP_A.to_string(),
        hwnd: None,
    });
    let text = std::fs::read_to_string(&path).unwrap();
    let file: state::StateFile = serde_json::from_str(&text).unwrap();
    let docs = &file.programs["chrome.exe"].windows[1];
    assert_eq!(docs.desktop.as_deref(), Some(DESKTOP_A));
    assert_eq!(file.reason, "layout page");
    assert_eq!(keeper.writer.last(), Some(&file.programs));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn an_edit_from_a_stale_list_is_ignored() {
    let (mut keeper, path) = keeper_with(layout_file(), "stale");
    let mut entry = entry_of(&mut keeper, "Docs - Google Chrome");
    entry.title = "Something else".to_string();
    keeper.on_layout_action(&LayoutAction::Forget(entry.clone()));
    keeper.on_layout_action(&LayoutAction::SetDesktop {
        entry,
        desktop: DESKTOP_A.to_string(),
        hwnd: None,
    });
    assert!(!path.exists(), "nothing was written");
    assert_eq!(keeper.writer.last(), Some(&layout_file()));
}

#[test]
fn a_closed_window_can_be_forgotten_and_an_empty_app_goes_with_it() {
    let (mut keeper, path) = keeper_with(layout_file(), "forget");
    let docs = entry_of(&mut keeper, "Docs - Google Chrome");
    keeper.on_layout_action(&LayoutAction::Forget(docs));
    let text = std::fs::read_to_string(&path).unwrap();
    let file: state::StateFile = serde_json::from_str(&text).unwrap();
    assert_eq!(file.programs["chrome.exe"].windows.len(), 1);
    assert_eq!(
        file.programs["chrome.exe"].windows[0].title,
        "Inbox - Google Chrome"
    );

    let viber = entry_of(&mut keeper, "Viber");
    keeper.on_layout_action(&LayoutAction::Forget(viber));
    assert!(!keeper.writer.last().unwrap().contains_key("viber.exe"));
    // A forgotten window is no longer waited for by a restore.
    assert_eq!(keeper.saved_groups(), ["Chrome"]);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn the_seen_day_round_trips_and_older_files_have_none() {
    let window = saved_window("A", "G", DESKTOP_A, Some("2026-10-17"));
    let text = serde_json::to_string(&window).unwrap();
    assert!(text.contains(r#""seen":"2026-10-17""#), "{text}");
    let back: SavedWindow = serde_json::from_str(&text).unwrap();
    assert_eq!(back, window);
    let none = serde_json::to_string(&saved_window("A", "G", DESKTOP_A, None)).unwrap();
    assert!(!none.contains("seen"), "{none}");
}

#[test]
fn a_saved_group_without_a_live_window_still_gets_a_name() {
    let (mut keeper, _) = keeper_with(Programs::new(), "label");
    assert_eq!(
        keeper.saved_label("chrome.exe", "Chrome.UserData.Profile3"),
        "Chrome \u{b7} Profile3"
    );
    assert_eq!(
        keeper.saved_label("charmap.exe", r"c:\windows\charmap.exe"),
        "Charmap"
    );
}

fn live_window(hwnd: isize, title: &str, group: &str) -> windows::LiveWindow {
    windows::LiveWindow {
        hwnd: hwnd as HWND,
        identity: identity::WindowIdentity::new("app.exe", title, [0, 0, 1, 1], false)
            .in_group(group),
        group: group.to_string(),
        app_name: None,
        exe_path: r"c:\app.exe".to_string(),
    }
}

#[test]
fn saved_windows_share_a_title_only_inside_one_app() {
    let mut programs = Programs::new();
    let windows = |titles: &[&str], group: &str| ProgramState {
        windows: titles
            .iter()
            .map(|title| saved_window(title, group, DESKTOP_A, None))
            .collect(),
    };
    programs.insert(
        "browser.exe".to_string(),
        windows(&["Inbox", "inbox ", "Docs"], "Browser"),
    );
    // The same word in another app is not a clash.
    programs.insert("chat.exe".to_string(), windows(&["Inbox"], "Chat"));
    let (mut keeper, _) = keeper_with(programs, "same-title");
    let snapshot = keeper.layout_snapshot().unwrap();
    let flags = |label: &str| -> Vec<(String, bool)> {
        snapshot
            .apps
            .iter()
            .find(|app| app.label == label)
            .unwrap()
            .windows
            .iter()
            .map(|window| (window.title.clone(), window.same_title))
            .collect()
    };
    assert_eq!(
        flags("Browser"),
        [
            ("Inbox".to_string(), true),
            ("inbox ".to_string(), true),
            ("Docs".to_string(), false)
        ]
    );
    assert_eq!(flags("Chat"), [("Inbox".to_string(), false)]);
}

#[test]
fn shared_titles_are_noted_when_the_set_changes_and_not_before() {
    let (mut keeper, _) = keeper_with(Programs::new(), "noted");
    let pair = [
        live_window(1, "New Tab", "Browser"),
        live_window(2, "new tab", "Browser"),
        live_window(3, "New Tab", "Other"),
    ];
    keeper.note_shared_titles(&pair);
    let key = ("Browser".to_string(), "new tab".to_string());
    assert_eq!(keeper.shared_titles.len(), 1);
    assert_eq!(keeper.shared_titles[&key], 2);

    // The same windows again change nothing; a third one changes the count.
    keeper.note_shared_titles(&pair);
    assert_eq!(keeper.shared_titles[&key], 2);
    let three = [
        live_window(1, "New Tab", "Browser"),
        live_window(2, "new tab", "Browser"),
        live_window(4, "New tab", "Browser"),
    ];
    keeper.note_shared_titles(&three);
    assert_eq!(keeper.shared_titles[&key], 3);

    // Giving one a different name ends it.
    let named = [
        live_window(1, "New Tab", "Browser"),
        live_window(2, "Work", "Browser"),
    ];
    keeper.note_shared_titles(&named);
    assert!(keeper.shared_titles.is_empty());
}

#[test]
fn the_progress_panel_follows_its_setting() {
    let mut keeper = LayoutKeeper::default();
    assert!(keeper.show_progress);
    keeper.apply_settings(&serde_json::json!({ "show_restore_progress": false }));
    assert!(!keeper.show_progress);
    keeper.apply_settings(&LayoutKeeper::default().default_settings());
    assert!(keeper.show_progress);
}

#[test]
fn a_group_without_a_window_still_has_a_readable_name() {
    let mut programs = Programs::new();
    programs.insert(
        "claude.exe".to_string(),
        ProgramState {
            windows: vec![saved_window(
                "Claude",
                r"d:\windowsapps\claude_2.4_x64__pzs8sxrjxfjjc\app\claude.exe",
                DESKTOP_A,
                None,
            )],
        },
    );
    programs.insert(
        "chrome.exe".to_string(),
        ProgramState {
            windows: vec![saved_window(
                "Inbox",
                "Chrome.UserData.Profile3",
                DESKTOP_A,
                None,
            )],
        },
    );
    let (mut keeper, _) = keeper_with(programs, "names");
    assert_eq!(
        keeper.group_name(r"d:\windowsapps\claude_2.4_x64__pzs8sxrjxfjjc\app\claude.exe"),
        "Claude"
    );
    assert_eq!(
        keeper.group_name("Chrome.UserData.Profile3"),
        "Chrome \u{b7} Profile3"
    );
    // A group the layout does not know is still not shown as a path.
    assert_eq!(
        keeper.group_name(r"c:\program files\windowsapps\x\windowsterminal.exe"),
        "Windowsterminal"
    );
    assert_eq!(
        keeper.group_name("Telegram.TelegramDesktop"),
        "TelegramDesktop"
    );
    assert_eq!(friendly_key("Pkg_8wekyb!App"), "App");
}

#[test]
fn a_label_seen_on_a_live_window_is_used_for_the_panel() {
    let (mut keeper, _) = keeper_with(Programs::new(), "seen");
    let mut window = live_window(7, "Docs", "Chrome.UserData.Profile3");
    window.identity.exe = "chrome.exe".to_string();
    window.app_name = Some("Dmytro (Oliver) - Chrome".to_string());
    keeper.remember_label(&window);
    assert_eq!(
        keeper.group_name("Chrome.UserData.Profile3"),
        "Chrome \u{b7} Dmytro (Oliver)"
    );
}

#[test]
fn apps_are_named_by_what_they_call_themselves() {
    assert_eq!(
        app_label(Some("Telegram"), Some("Telegram Desktop"), "telegram.exe"),
        "Telegram"
    );
    assert_eq!(
        app_label(None, Some("Character Map"), "charmap.exe"),
        "Character Map"
    );
    assert_eq!(app_label(None, None, "claude.exe"), "Claude");
    assert_eq!(app_label(Some("  "), None, "code.exe"), "Code");
    assert_eq!(program_name("msedge.exe"), "Edge");
}

#[test]
fn pinned_and_untracked_windows_are_never_moved() {
    let one = DesktopId(1);
    let two = DesktopId(2);
    let known = [one, two];
    let saved_two = two.to_string();
    // A window on desktop 1 that was saved on desktop 2 goes back there.
    assert_eq!(
        desktop_target(Some(one), Some(&saved_two), &known),
        Some(two)
    );
    // No desktop at all ("not tracked"), or pinned to every desktop.
    assert_eq!(desktop_target(None, Some(&saved_two), &known), None);
    assert_eq!(
        desktop_target(Some(DesktopId::ALL), Some(&saved_two), &known),
        None
    );
    assert_eq!(
        desktop_target(Some(DesktopId(9)), Some(&saved_two), &known),
        None
    );
    // Saved as pinned, or on a desktop that is gone: only undo the pull.
    let gone = DesktopId(9).to_string();
    assert_eq!(desktop_target(Some(one), Some(&gone), &known), Some(one));
    assert_eq!(desktop_target(Some(one), None, &known), Some(one));
    assert_eq!(desktop_target(Some(one), Some(""), &known), Some(one));
}

#[test]
fn the_program_list_is_trimmed_lowercased_and_deduplicated() {
    assert_eq!(
        parse_programs(" Chrome.exe, ,msedge.exe,chrome.exe"),
        ["chrome.exe", "msedge.exe"]
    );
}

#[test]
fn numbers_are_rounded_and_clamped() {
    let settings = serde_json::json!({ "a": 29.6, "b": 1.0, "c": "x" });
    assert_eq!(number(&settings, "a", 5, 600), Some(30));
    assert_eq!(number(&settings, "b", 5, 600), Some(5));
    assert_eq!(number(&settings, "c", 5, 600), None);
    assert_eq!(number(&settings, "missing", 5, 600), None);
}

#[test]
fn switches_default_to_on() {
    let settings = serde_json::json!({ "off": false });
    assert!(!flag(&settings, "off"));
    assert!(flag(&settings, "missing"));
}

#[test]
fn a_label_made_before_the_index_was_ready_is_made_again_when_it_is() {
    let (mut keeper, _) = keeper_with(Programs::new(), "unresolved");
    let mut window = live_window(7, "Host", "Some.Unlisted.App");
    window.identity.exe = "unlisted.exe".to_string();
    keeper.remember_label(&window);
    // Nothing builds the index in a test, so it is not final.
    assert!(keeper.unresolved.contains("Some.Unlisted.App"));
    assert!(keeper.labels.contains_key("Some.Unlisted.App"));
}

// Saves and restores below run over fake windows, desktops and monitors.
// Nothing in them reaches a real window: the handles are made up and the
// only writes go to scratch files.

const D1: DesktopId = DesktopId(1);
const D2: DesktopId = DesktopId(2);
/// A desktop the PC no longer has.
const D9: DesktopId = DesktopId(9);

fn two_desktops() -> Vec<Desktop> {
    vec![
        Desktop {
            id: D1,
            name: "Desktop 1".to_string(),
        },
        Desktop {
            id: D2,
            name: "Work".to_string(),
        },
    ]
}

fn screen(device: &str, name: &str, position: usize) -> SavedMonitor {
    SavedMonitor {
        device: format!(r"\\.\{device}"),
        name: name.to_string(),
        position,
    }
}

/// DELL on the left, LG right of it at x 1920.
fn two_monitors() -> Vec<SavedMonitor> {
    vec![screen("DISPLAY1", "DELL", 0), screen("DISPLAY2", "LG", 1)]
}

fn by_left_edge(rect: Rect) -> Option<usize> {
    Some(usize::from(rect[0] >= 1920))
}

fn open(hwnd: isize, exe: &str, title: &str, group: &str, rect: Rect) -> windows::LiveWindow {
    windows::LiveWindow {
        hwnd: hwnd as HWND,
        identity: identity::WindowIdentity::new(exe, title, rect, false).in_group(group),
        group: group.to_string(),
        app_name: None,
        exe_path: format!(r"c:\apps\{exe}"),
    }
}

fn chrome(hwnd: isize, title: &str, left: i32) -> windows::LiveWindow {
    open(
        hwnd,
        "chrome.exe",
        title,
        "Chrome",
        [left, 10 * hwnd as i32, left + 700, 500],
    )
}

fn kept(
    title: &str,
    desktop: Option<DesktopId>,
    monitor: Option<SavedMonitor>,
    place: usize,
) -> SavedWindow {
    SavedWindow {
        name: Some(title.to_string()),
        title: title.to_string(),
        rect: [5000 + place as i32, 0, 5600, 400],
        desktop: desktop.map(|id| id.to_string()),
        monitor,
        taskbar_index: place,
        z_index: place,
        ..saved("Chrome")
    }
}

fn programs(entries: Vec<(&str, Vec<SavedWindow>)>) -> Programs {
    entries
        .into_iter()
        .map(|(exe, windows)| (exe.to_string(), ProgramState { windows }))
        .collect()
}

fn read_file(path: &std::path::Path) -> state::StateFile {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn titles(file: &state::StateFile, exe: &str) -> Vec<String> {
    file.programs[exe]
        .windows
        .iter()
        .map(|window| window.title.clone())
        .collect()
}

/// The titles whatever their taskbar order, which a restart scrambles.
fn title_set(path: &std::path::Path, exe: &str) -> BTreeSet<String> {
    titles(&read_file(path), exe).into_iter().collect()
}

fn set(titles: &[&str]) -> BTreeSet<String> {
    titles.iter().map(|title| title.to_string()).collect()
}

/// Saves `live` the way a timer or a hotkey would, against two desktops
/// and two monitors, with window 1 on the Work desktop.
fn save_seen(
    keeper: &mut LayoutKeeper,
    live: &[windows::LiveWindow],
    reason: &str,
) -> (bool, usize) {
    let desktops = two_desktops();
    let monitors = two_monitors();
    let world = World {
        desktops: &desktops,
        monitors: &monitors,
        desktop_of: &|hwnd| (hwnd as isize == 1).then_some(D2),
        monitor_of: &by_left_edge,
        today: Some("2026-10-18".to_string()),
    };
    let force = reason == "manual";
    keeper.write_seen(live, &world, reason, force).unwrap()
}

fn clean_up(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("prev.json"));
    let _ = std::fs::remove_file(state::snapshot_path(path));
}

#[test]
fn a_save_writes_each_window_with_its_desktop_monitor_order_and_day() {
    let (mut keeper, path) = keeper_with(Programs::new(), "save-fresh");
    let live = [
        chrome(1, "Inbox", 2000),
        chrome(2, "Docs", 0),
        open(3, "viber.exe", "Viber", "Viber.Desktop", [0, 0, 400, 400]),
    ];
    assert_eq!(save_seen(&mut keeper, &live, "manual"), (true, 3));
    let file = read_file(&path);
    assert_eq!(file.reason, "manual");
    // A group seen for the first time is in taskbar order bottom first.
    assert_eq!(titles(&file, "chrome.exe"), ["Docs", "Inbox"]);
    let docs = &file.programs["chrome.exe"].windows[0];
    let inbox = &file.programs["chrome.exe"].windows[1];
    assert_eq!(inbox.desktop, Some(D2.to_string()));
    assert_eq!(inbox.desktop_name.as_deref(), Some("Work"));
    assert_eq!(inbox.monitor, Some(screen("DISPLAY2", "LG", 1)));
    assert_eq!((inbox.z_index, docs.z_index), (0, 1));
    assert_eq!(docs.desktop, None);
    assert_eq!(docs.monitor, Some(screen("DISPLAY1", "DELL", 0)));
    assert_eq!(docs.seen.as_deref(), Some("2026-10-18"));
    assert_eq!(file.programs["viber.exe"].windows[0].z_index, 2);

    // The same windows again: a timer save writes nothing.
    assert_eq!(save_seen(&mut keeper, &live, "timer"), (false, 3));
    clean_up(&path);
}

#[test]
fn a_save_keeps_closed_apps_and_drops_the_ones_no_longer_watched() {
    let before = programs(vec![
        ("viber.exe", vec![kept("Viber", None, None, 0)]),
        ("telegram.exe", vec![kept("Telegram", None, None, 0)]),
    ]);
    let (mut keeper, path) = keeper_with(before, "save-closed");
    keeper.watch = Watch::new("*", "telegram.exe");
    save_seen(&mut keeper, &[chrome(2, "Docs", 0)], "timer");
    let file = read_file(&path);
    assert_eq!(
        file.programs.keys().collect::<Vec<_>>(),
        ["chrome.exe", "viber.exe"]
    );
    assert_eq!(titles(&file, "viber.exe"), ["Viber"]);
    clean_up(&path);
}

#[test]
fn a_window_saved_on_an_unplugged_monitor_keeps_it_until_moved_in_the_strip() {
    let samsung = screen("DISPLAY3", "SAMSUNG", 2);
    let before = programs(vec![(
        "chrome.exe",
        vec![kept("Mail", None, Some(samsung.clone()), 0)],
    )]);
    let (mut keeper, path) = keeper_with(before, "save-unplugged");
    let live = [chrome(2, "Mail", 0)];
    save_seen(&mut keeper, &live, "manual");
    let mail = &read_file(&path).programs["chrome.exe"].windows[0];
    assert_eq!(mail.monitor, Some(samsung));

    keeper.released.insert(2);
    save_seen(&mut keeper, &live, "manual");
    let mail = &read_file(&path).programs["chrome.exe"].windows[0];
    assert_eq!(mail.monitor, Some(screen("DISPLAY1", "DELL", 0)));
    assert!(keeper.released.is_empty(), "a save uses the release up");
    clean_up(&path);
}

fn four_tabs() -> Vec<windows::LiveWindow> {
    ["A", "B", "C", "D"]
        .iter()
        .enumerate()
        .map(|(index, title)| chrome(index as isize + 1, title, 0))
        .collect()
}

/// Chrome restarted: two new windows in place of the four saved ones.
fn after_restart() -> Vec<windows::LiveWindow> {
    vec![chrome(11, "X", 0), chrome(12, "Y", 0)]
}

#[test]
fn a_crash_burst_keeps_the_saved_windows_and_a_manual_save_copies_the_file_first() {
    let (mut keeper, path) = keeper_with(Programs::new(), "save-burst");
    let prev = path.with_extension("prev.json");
    save_seen(&mut keeper, &four_tabs(), "timer");

    keeper.ticks = 30;
    assert_eq!(
        save_seen(&mut keeper, &after_restart(), "timer"),
        (false, 4)
    );
    assert_eq!(title_set(&path, "chrome.exe"), set(&["A", "B", "C", "D"]));
    assert!(!prev.exists(), "nothing was written over yet");

    assert_eq!(
        save_seen(&mut keeper, &after_restart(), "manual"),
        (true, 2)
    );
    assert_eq!(title_set(&path, "chrome.exe"), set(&["X", "Y"]));
    assert_eq!(title_set(&prev, "chrome.exe"), set(&["A", "B", "C", "D"]));

    // The copy was made once; later writes leave it alone.
    std::fs::remove_file(&prev).unwrap();
    keeper.ticks = 90;
    let more = [chrome(11, "X", 0), chrome(12, "Y", 0), chrome(13, "Z", 0)];
    assert_eq!(save_seen(&mut keeper, &more, "manual"), (true, 3));
    assert!(!prev.exists());
    clean_up(&path);
}

#[test]
fn the_first_timer_save_after_the_hold_ends_copies_the_file_first() {
    let (mut keeper, path) = keeper_with(Programs::new(), "save-release");
    let prev = path.with_extension("prev.json");
    save_seen(&mut keeper, &four_tabs(), "timer");
    keeper.ticks = 30;
    save_seen(&mut keeper, &after_restart(), "timer");
    keeper.ticks = 45;
    assert_eq!(
        save_seen(&mut keeper, &after_restart(), "timer"),
        (false, 4)
    );
    assert!(!prev.exists());

    keeper.ticks = 60;
    assert_eq!(save_seen(&mut keeper, &after_restart(), "timer"), (true, 2));
    assert_eq!(title_set(&prev, "chrome.exe"), set(&["A", "B", "C", "D"]));
    assert_eq!(title_set(&path, "chrome.exe"), set(&["X", "Y"]));
    clean_up(&path);
}

#[test]
fn groups_still_waiting_for_their_restore_keep_their_saved_windows_in_any_save() {
    let before = programs(vec![(
        "chrome.exe",
        vec![kept("A", None, None, 0), kept("B", None, None, 1)],
    )]);
    let (mut keeper, path) = keeper_with(before, "save-pending");
    keeper.restore.start(0, vec!["Chrome".to_string()]);
    let live = [chrome(1, "B", 0), chrome(2, "A", 0), chrome(3, "New", 0)];
    assert_eq!(save_seen(&mut keeper, &live, "manual"), (true, 2));
    assert_eq!(titles(&read_file(&path), "chrome.exe"), ["A", "B"]);
    clean_up(&path);
}

#[test]
fn a_group_still_being_checked_after_its_restore_keeps_its_saved_windows() {
    let before = programs(vec![(
        "chrome.exe",
        vec![kept("A", None, None, 0), kept("B", None, None, 1)],
    )]);
    let (mut keeper, path) = keeper_with(before, "save-checking");
    keeper.checks.push(check_of(vec![verify::Target {
        hwnd: 1,
        group: "Chrome".to_string(),
        label: "B".to_string(),
        desktop: Some(D1),
        monitor: None,
    }]));
    let live = [chrome(1, "B", 0), chrome(3, "New", 0)];
    assert_eq!(
        save_seen(&mut keeper, &live, "timer"),
        (false, 2),
        "the saved A and B stay as they were"
    );
    keeper.checks.clear();
    assert_eq!(save_seen(&mut keeper, &live, "timer"), (true, 2));
    assert_eq!(set(&["B", "New"]), title_set(&path, "chrome.exe"));
    clean_up(&path);
}

#[test]
fn a_save_plan_copies_aside_only_when_asked_and_something_changes() {
    let watch = Watch::new("*", "");
    let mut guard = guard::Guard::default();
    let timing = guard::Timing {
        stable_seconds: 30,
        refill_seconds: 180,
    };
    let look = |handles: &[Handle]| BTreeMap::from([("Chrome".to_string(), handles.to_vec())]);
    guard.observe(0, &look(&[1, 2, 3, 4]), timing);
    guard.observe(30, &look(&[]), timing);
    let saved = programs(vec![("chrome.exe", vec![kept("A", None, None, 0)])]);

    let same = plan_save(
        Some(&saved),
        saved.clone(),
        "manual",
        &watch,
        &guard,
        vec![],
    );
    assert!(same.backup && !same.copy_aside, "nothing would be lost");

    let other = programs(vec![("chrome.exe", vec![kept("B", None, None, 0)])]);
    let manual = plan_save(
        Some(&saved),
        other.clone(),
        "manual",
        &watch,
        &guard,
        vec![],
    );
    assert!(manual.backup && manual.copy_aside);
    assert_eq!(manual.programs, other);

    let timer = plan_save(Some(&saved), other, "timer", &watch, &guard, vec![]);
    assert!(!timer.backup && !timer.copy_aside);
    assert_eq!(timer.programs, saved, "the held group keeps its windows");
    assert_eq!(timer.count, 1);
}

fn due(group: &str, pass: restore::Pass) -> Vec<restore::Due> {
    vec![restore::Due {
        group: group.to_string(),
        pass,
    }]
}

/// Docs on desktop 1 of the left monitor, Inbox on Work on the right one,
/// Old on a desktop that is gone, Lost no longer open.
fn saved_chrome() -> ProgramState {
    let dell = screen("DISPLAY1", "DELL", 0);
    let lg = screen("DISPLAY2", "LG", 1);
    ProgramState {
        windows: vec![
            kept("Inbox", Some(D2), Some(lg), 1),
            kept("Docs", Some(D1), Some(dell.clone()), 0),
            kept("Old", Some(D9), Some(dell), 2),
            kept("Lost", Some(D1), None, 3),
        ],
    }
}

/// Every window was pulled to desktop 1 on the left monitor.
fn reopened() -> Vec<windows::LiveWindow> {
    vec![
        chrome(10, "Inbox", 0),
        chrome(11, "Docs", 0),
        chrome(12, "Old", 0),
        open(20, "viber.exe", "Viber", "Viber.Desktop", [0, 0, 300, 300]),
    ]
}

struct Plan {
    plans: Vec<GroupPlan>,
    groups: BTreeMap<String, OrderModel>,
    placed: BTreeSet<Handle>,
    stack: Vec<(usize, HWND)>,
}

fn plan_for(
    program: &ProgramState,
    live: &[windows::LiveWindow],
    due: &[restore::Due],
    placed: BTreeSet<Handle>,
    can_move: bool,
    restore_monitors: bool,
) -> Plan {
    let view = ExeView::new("chrome.exe", program, live, &|_| Some(D1), &by_left_edge);
    let monitors = two_monitors();
    let cx = RestoreContext {
        live,
        known: &[D1, D2],
        monitors: &monitors,
        can_move,
        restore_monitors,
        ticks: 0,
        snapshot_seconds: 30,
    };
    let mut plan = Plan {
        plans: Vec::new(),
        groups: BTreeMap::new(),
        placed,
        stack: Vec::new(),
    };
    plan.plans = plan_restore(
        &view,
        due,
        &cx,
        &mut plan.groups,
        &mut plan.placed,
        &mut plan.stack,
    );
    plan
}

fn hwnd(handle: isize) -> HWND {
    handle as HWND
}

#[test]
fn a_restore_plans_monitor_desktop_and_taskbar_moves_for_each_matched_window() {
    let plan = plan_for(
        &saved_chrome(),
        &reopened(),
        &due("Chrome", restore::Pass::Settled),
        BTreeSet::new(),
        true,
        true,
    );
    let [group] = plan.plans.as_slice() else {
        panic!("{:?}", plan.plans)
    };
    assert_eq!((group.matched, group.saved), (3, 4));
    assert_eq!(group.lost, ["Lost"]);
    let targets: Vec<(isize, monitors::Return)> = group
        .monitors
        .iter()
        .map(|step| (step.hwnd as isize, step.target))
        .collect();
    assert_eq!(
        targets,
        [
            (11, monitors::Return::Stay),
            (10, monitors::Return::To(1)),
            (12, monitors::Return::Stay),
        ]
    );
    // Every window gets a desktop move, if only to undo the pull of
    // its re-added button; a window from a desktop that is gone stays.
    let moves: Vec<(isize, DesktopId)> = group
        .moves
        .iter()
        .map(|(hwnd, target, _)| (*hwnd as isize, *target))
        .collect();
    assert_eq!(moves, [(10, D2), (11, D1), (12, D1)]);
    assert_eq!(group.gone, [("Old".to_string(), D9)]);
    // The taskbar gets the saved order.
    assert_eq!(plan.groups["Chrome"].pending(), Some(vec![11, 10, 12]));
    assert_eq!(plan.placed, BTreeSet::from([10, 11, 12]));
    assert_eq!(plan.stack, [(0, hwnd(11)), (1, hwnd(10)), (2, hwnd(12))]);
}

fn placements_for(
    live: &[windows::LiveWindow],
    pending: &[&str],
    placed: BTreeSet<Handle>,
) -> Vec<Placement> {
    let program = saved_chrome();
    // Desktop 1 for every open window, as after sign-in.
    let view = ExeView::new("chrome.exe", &program, live, &|_| Some(D1), &by_left_edge);
    let monitors = two_monitors();
    let cx = RestoreContext {
        live,
        known: &[D1, D2],
        monitors: &monitors,
        can_move: true,
        restore_monitors: true,
        ticks: 0,
        snapshot_seconds: 30,
    };
    let pending: Vec<String> = pending.iter().map(|group| group.to_string()).collect();
    plan_placements(&view, &pending, &placed, &cx)
}

#[test]
fn a_window_that_opens_is_placed_at_once_without_waiting_for_the_others() {
    // Only Inbox and Docs are open so far; Old and Lost have not come.
    let live = [chrome(10, "Inbox", 0), chrome(11, "Docs", 0)];
    let mut placements = placements_for(&live, &["Chrome"], BTreeSet::new());
    placements.sort_by_key(|placement| placement.hwnd);
    assert_eq!(
        placements,
        [
            Placement {
                hwnd: 10,
                group: "Chrome".to_string(),
                label: "Inbox".to_string(),
                monitor: Some((0, 1)),
                desktop: Some(D2),
            },
            Placement {
                hwnd: 11,
                group: "Chrome".to_string(),
                label: "Docs".to_string(),
                monitor: None,
                desktop: None,
            },
        ],
        "Docs is in place already and is only counted as placed"
    );
}

#[test]
fn a_placed_window_or_one_of_a_group_not_in_the_restore_is_left_alone() {
    let live = [chrome(10, "Inbox", 0), chrome(11, "Docs", 0)];
    let placements = placements_for(&live, &["Chrome"], BTreeSet::from([10, 11]));
    assert!(placements.is_empty(), "each window is placed once");
    let placements = placements_for(&live, &["Viber.Desktop"], BTreeSet::new());
    assert!(placements.is_empty(), "Chrome is not in the restore");
    // A window that matches no saved one is not placed.
    let stranger = [chrome(12, "Something new", 9000)];
    let saved_one = placements_for(&stranger, &["Chrome"], BTreeSet::new());
    assert!(saved_one.iter().all(|placement| placement.hwnd != 12) || saved_one.len() <= 1);
}

#[test]
fn a_second_pass_moves_only_the_windows_that_came_late() {
    let mut program = saved_chrome();
    program.windows.push(kept(
        "Later",
        Some(D2),
        Some(screen("DISPLAY2", "LG", 1)),
        4,
    ));
    let mut live = reopened();
    live.push(chrome(13, "Later", 0));
    let plan = plan_for(
        &program,
        &live,
        &due("Chrome", restore::Pass::Late),
        BTreeSet::from([10, 11, 12]),
        true,
        true,
    );
    let [group] = plan.plans.as_slice() else {
        panic!("{:?}", plan.plans)
    };
    assert!(group.late);
    let stepped: Vec<isize> = group
        .monitors
        .iter()
        .map(|step| step.hwnd as isize)
        .collect();
    assert_eq!(stepped, [13], "placed windows stay on their monitor");
    let moves: Vec<(isize, DesktopId)> = group
        .moves
        .iter()
        .map(|(hwnd, target, _)| (*hwnd as isize, *target))
        .collect();
    // Inbox was put on Work by the first pass and moved back here since:
    // it only gets its pull undone, and only Later counts as moved.
    assert_eq!(moves, [(10, D1), (11, D1), (12, D1), (13, D2)]);
    assert_eq!(group.moved_late, 1);
    assert!(group.gone.is_empty() && group.lost.is_empty());
    assert!(plan.stack.is_empty(), "nothing is raised again");
    assert!(plan.placed.contains(&13));
}

#[test]
fn the_second_order_after_a_sign_in_rebuilds_the_taskbar_and_moves_nothing() {
    let plan = plan_for(
        &saved_chrome(),
        &reopened(),
        &due("Chrome", restore::Pass::Again),
        BTreeSet::from([10, 11, 12]),
        true,
        true,
    );
    let [group] = plan.plans.as_slice() else {
        panic!("{:?}", plan.plans)
    };
    assert!(group.again && group.late);
    assert!(
        group.monitors.is_empty(),
        "placed windows stay on their monitor"
    );
    let targets: Vec<DesktopId> = group.moves.iter().map(|(_, target, _)| *target).collect();
    assert_eq!(
        targets,
        [D1, D1, D1],
        "each only undoes the pull of its button"
    );
    assert_eq!(plan.groups["Chrome"].pending(), Some(vec![11, 10, 12]));
    assert!(plan.stack.is_empty(), "nothing is raised again");

    let mut keeper = read_only();
    let mut done = Restored::default();
    keeper.carry_out(plan.plans, true, &[], &mut done);
    assert!(
        done.parts.is_empty() && done.late_parts.is_empty(),
        "no summary line"
    );
}

#[test]
fn without_the_desktop_mover_or_monitor_restore_those_moves_are_left_out() {
    let plan = plan_for(
        &saved_chrome(),
        &reopened(),
        &due("Chrome", restore::Pass::Settled),
        BTreeSet::new(),
        false,
        false,
    );
    let group = &plan.plans[0];
    assert!(group.moves.is_empty() && group.gone.is_empty());
    assert!(group.monitors.is_empty());
    assert_eq!(group.matched, 3);
}

#[test]
fn a_group_not_open_or_not_due_is_not_planned() {
    let plan = plan_for(
        &saved_chrome(),
        &[open(
            20,
            "viber.exe",
            "Viber",
            "Viber.Desktop",
            [0, 0, 300, 300],
        )],
        &due("Chrome", restore::Pass::Settled),
        BTreeSet::new(),
        true,
        true,
    );
    assert!(plan.plans.is_empty());
    let plan = plan_for(
        &saved_chrome(),
        &reopened(),
        &due("Viber.Desktop", restore::Pass::Settled),
        BTreeSet::new(),
        true,
        true,
    );
    assert!(plan.plans.is_empty(), "Viber is not chrome.exe's group");
}

#[test]
fn only_watched_programs_with_a_due_group_are_restored() {
    let saved = programs(vec![
        ("chrome.exe", saved_chrome().windows),
        ("telegram.exe", vec![kept("Chat", None, None, 0)]),
        (
            "viber.exe",
            vec![SavedWindow {
                group: "Viber.Desktop".to_string(),
                ..kept("Viber", None, None, 0)
            }],
        ),
    ]);
    let watch = Watch::new("*", "telegram.exe");
    let due = due("Chrome", restore::Pass::Settled);
    assert_eq!(restore_exes(&saved, &due, &watch), ["chrome.exe"]);
}

#[test]
fn the_summary_names_each_group_and_what_was_not_found() {
    let mut done = Restored::default();
    assert_eq!(summary(&done), None);
    done.parts = vec![
        "2 of 3 Chrome windows".to_string(),
        "1 of 1 Viber windows".to_string(),
    ];
    done.missing = 1;
    assert_eq!(
        summary(&done).as_deref(),
        Some("Restored 2 of 3 Chrome windows, 1 of 1 Viber windows (1 not found)")
    );
    done.late_parts = vec!["second pass: moved 1 late Chrome windows".to_string()];
    assert_eq!(
        summary(&done).as_deref(),
        Some(
            "Restored 2 of 3 Chrome windows, 1 of 1 Viber windows (1 not found); second pass: moved 1 late Chrome windows"
        )
    );
    done.parts.clear();
    done.missing = 0;
    assert_eq!(
        summary(&done).as_deref(),
        Some("second pass: moved 1 late Chrome windows")
    );
}

fn finish(stack: Vec<(usize, HWND)>, foreground: HWND) -> Finish {
    Finish {
        moves: vec![
            (hwnd(10), D2, "Inbox".to_string()),
            (hwnd(11), D1, "Docs".to_string()),
        ],
        stack,
        foreground,
        summary: String::new(),
        announce: false,
        targets: Vec::new(),
        orders: Vec::new(),
    }
}

#[test]
fn the_second_half_skips_windows_on_the_current_desktop_and_raises_back_to_front() {
    let done = finish(vec![(1, hwnd(10)), (0, hwnd(11)), (2, hwnd(12))], hwnd(99));
    let (moves, raises) = finish_steps(&done, Some(D1), true);
    let moved: Vec<isize> = moves.iter().map(|(hwnd, _, _)| *hwnd as isize).collect();
    assert_eq!(moved, [10]);
    let raised: Vec<isize> = raises.iter().map(|hwnd| *hwnd as isize).collect();
    assert_eq!(raised, [12, 10, 11, 99], "the old front window ends on top");

    let (_, raises) = finish_steps(&done, Some(D1), false);
    assert!(raises.is_empty(), "the front order is not restored");
    let late = finish(Vec::new(), hwnd(99));
    let (moves, raises) = finish_steps(&late, None, true);
    assert_eq!(moves.len(), 2, "with no current desktop every move runs");
    assert!(raises.is_empty(), "a second pass raises nothing");
    let (_, raises) = finish_steps(
        &finish(vec![(0, hwnd(10))], std::ptr::null_mut()),
        None,
        true,
    );
    assert_eq!(raises, [hwnd(10)]);
}

#[test]
fn a_read_only_instance_carries_out_no_planned_move_but_still_reports() {
    let mut keeper = read_only();
    let plans = vec![GroupPlan {
        group: "Chrome".to_string(),
        monitors: vec![MonitorStep {
            hwnd: NO_WINDOW,
            label: "Inbox".to_string(),
            saved: None,
            now: Some(0),
            target: monitors::Return::To(1),
        }],
        moves: vec![(NO_WINDOW, D2, "Inbox".to_string())],
        matched: 1,
        saved: 2,
        lost: vec!["Lost".to_string()],
        ..GroupPlan::default()
    }];
    let mut done = Restored::default();
    keeper.carry_out(plans, true, &[], &mut done);
    assert_eq!(keeper.refused, 2, "the monitor move and the taskbar order");
    assert_eq!(done.parts, ["1 of 2 Chrome windows"]);
    assert_eq!((done.missing, done.moves.len()), (1, 1));

    let late = vec![GroupPlan {
        group: "Chrome".to_string(),
        late: true,
        moved_late: 1,
        ..GroupPlan::default()
    }];
    keeper.carry_out(late, false, &[], &mut done);
    assert_eq!(
        done.late_parts,
        ["second pass: moved 1 late Chrome windows"]
    );
}

#[test]
fn the_check_wants_each_window_on_its_move_desktop_and_step_monitor() {
    let plan = GroupPlan {
        group: "Chrome".to_string(),
        monitors: vec![
            MonitorStep {
                hwnd: hwnd(10),
                label: "Inbox".to_string(),
                saved: None,
                now: Some(0),
                target: monitors::Return::To(1),
            },
            MonitorStep {
                hwnd: hwnd(11),
                label: "Docs".to_string(),
                saved: None,
                now: Some(0),
                target: monitors::Return::Stay,
            },
            MonitorStep {
                hwnd: hwnd(12),
                label: "Old".to_string(),
                saved: None,
                now: None,
                target: monitors::Return::Unplugged,
            },
        ],
        moves: vec![
            (hwnd(10), D2, "Inbox".to_string()),
            (hwnd(13), D1, "Late".to_string()),
            (hwnd(14), DesktopId::ALL, "Pinned".to_string()),
        ],
        ..GroupPlan::default()
    };
    let device = |index: usize| Some(format!("DISPLAY{}", index + 1));
    let wanted: Vec<(isize, Option<DesktopId>, Option<String>)> =
        check_targets_with(&plan, &device)
            .into_iter()
            .map(|target| (target.hwnd, target.desktop, target.monitor))
            .collect();
    assert_eq!(
        wanted,
        [
            (10, Some(D2), Some("DISPLAY2".to_string())),
            (11, None, Some("DISPLAY1".to_string())),
            (13, Some(D1), None),
        ],
        "an unplugged monitor and a pinned window are not checked"
    );
}

fn check_of(targets: Vec<verify::Target>) -> Check {
    Check {
        due: 0,
        orders: vec![(
            "Chrome".to_string(),
            targets.iter().map(|target| target.hwnd).collect(),
        )],
        targets,
        moved_again: None,
    }
}

#[test]
fn a_window_out_of_place_is_moved_once_more_and_then_reported() {
    let mut keeper = read_only();
    let target = verify::Target {
        hwnd: 10,
        group: "Chrome".to_string(),
        label: "Inbox".to_string(),
        desktop: Some(D2),
        monitor: None,
    };
    // Without a desktop reader the window's desktop cannot be read, so
    // it is not on the wanted one.
    let live = [chrome(10, "Inbox", 0)];
    keeper.check_restore(check_of(vec![target]), &live, &[], &two_desktops());
    assert_eq!(keeper.refused, 1, "the second desktop move");
    let [again] = keeper.checks.as_slice() else {
        panic!("the check comes back")
    };
    assert_eq!(again.moved_again, Some(vec![0]));
    assert_eq!(again.due, CHECK_SECONDS);

    let check = keeper.checks.pop().unwrap();
    keeper.check_restore(check, &live, &[], &two_desktops());
    assert!(keeper.checks.is_empty(), "the second look is the last");
    assert_eq!(keeper.refused, 1, "nothing is moved a third time");
}

#[test]
fn a_check_of_windows_that_closed_since_moves_nothing() {
    let mut keeper = read_only();
    let target = verify::Target {
        hwnd: 10,
        group: "Chrome".to_string(),
        label: "Inbox".to_string(),
        desktop: Some(D2),
        monitor: Some("DISPLAY1".to_string()),
    };
    keeper.check_restore(check_of(vec![target]), &[], &[], &two_desktops());
    assert!(keeper.checks.is_empty());
    assert_eq!(keeper.refused, 0);
    // A check that is not due yet waits.
    keeper.checks.push(Check {
        due: 5,
        ..check_of(Vec::new())
    });
    keeper.run_checks();
    assert_eq!(keeper.checks.len(), 1);
}

#[test]
fn a_monitor_step_that_stays_or_is_unplugged_moves_nothing() {
    let mut keeper = read_only();
    for target in [monitors::Return::Stay, monitors::Return::Unplugged] {
        keeper.return_to_monitor(
            &MonitorStep {
                hwnd: NO_WINDOW,
                label: "Inbox".to_string(),
                saved: Some(screen("DISPLAY3", "SAMSUNG", 2)),
                now: Some(0),
                target,
            },
            &[],
        );
    }
    assert_eq!(keeper.refused, 0, "no move was even tried");
}

#[test]
fn timer_saves_come_every_snapshot_interval() {
    assert!(save_due(30, 30) && save_due(60, 30) && !save_due(31, 30));
    // A zero interval cannot come from the settings, but never divides by it.
    assert!(save_due(7, 0));
}

/// A keeper that watches a program nobody runs, so its saves see no
/// window of the user's at all.
fn watching_nothing(name: &str) -> (LayoutKeeper, std::path::PathBuf) {
    let before = programs(vec![("viber.exe", vec![kept("Viber", None, None, 0)])]);
    let (mut keeper, path) = keeper_with(before, name);
    keeper.watch = Watch::new("wincraft-test-nothing.exe", "");
    (keeper, path)
}

#[test]
fn a_tick_saves_on_the_interval() {
    let (mut keeper, path) = watching_nothing("tick-save");
    keeper.snapshot_seconds = 3;
    keeper.tick();
    keeper.tick();
    assert!(!path.exists());
    keeper.tick();
    let file = read_file(&path);
    assert_eq!(file.reason, "timer");
    assert!(
        file.programs.is_empty(),
        "viber.exe is not watched any more"
    );
    assert!(keeper.last_saved.is_some());
    clean_up(&path);
}

#[test]
fn a_tick_finishes_a_restore_first_and_saves_nothing_then() {
    let (mut keeper, path) = watching_nothing("tick-finish");
    keeper.read_only = true;
    keeper.watch = Watch::new("*", "");
    keeper.snapshot_seconds = 1;
    keeper.finishing = Some(finish(Vec::new(), std::ptr::null_mut()));
    keeper.tick();
    assert!(keeper.finishing.is_none());
    assert_eq!(keeper.refused, 1);
    assert!(!path.exists(), "the half-moved windows were not saved");
    // While a restore is between its halves no save runs at all.
    keeper.finishing = Some(finish(Vec::new(), std::ptr::null_mut()));
    keeper.save("timer");
    assert!(!path.exists());
    keeper.finishing = None;
    keeper.tick();
    assert!(path.exists());
    clean_up(&path);
}

#[test]
fn a_tick_rebuilds_the_groups_a_desktop_move_changed() {
    let (mut keeper, _) = watching_nothing("tick-reapply");
    keeper.read_only = true;
    keeper.watch = Watch::new("*", "");
    keeper.reapply = vec!["Chrome".to_string()];
    keeper.arrange_stale = true;
    keeper.tick();
    assert!(keeper.reapply.is_empty() && !keeper.arrange_stale);
    assert_eq!(keeper.refused, 1, "the taskbar reorder was turned down");
}

#[test]
fn a_restart_of_explorer_starts_the_wait_unless_the_instance_is_read_only() {
    let (mut keeper, _) = watching_nothing("explorer-restart");
    keeper.read_only = false;
    keeper.ticks = 40;
    keeper.on_taskbar_created();
    assert!(!keeper.explorer.due(44));
    assert!(keeper.explorer.due(45));

    let (mut test_copy, _) = watching_nothing("explorer-restart-read-only");
    test_copy.read_only = true;
    test_copy.on_taskbar_created();
    assert!(!test_copy.explorer.due(1000));
    assert_eq!(test_copy.refused, 1);
}

#[test]
fn the_end_of_a_session_keeps_a_snapshot_but_an_installer_closing_apps_does_not() {
    let (mut keeper, path) = watching_nothing("end-session");
    let snapshot = state::snapshot_path(&path);
    keeper.on_windows_message(WM_QUERYENDSESSION, 0, ENDSESSION_CLOSEAPP as LPARAM);
    assert_eq!(read_file(&path).reason, "shutdown");
    assert!(!snapshot.exists(), "Windows keeps running after this");
    keeper.on_windows_message(WM_ENDSESSION, 0, 0);
    assert!(!snapshot.exists(), "the session did not end after all");
    keeper.on_windows_message(WM_QUERYENDSESSION, 0, 0);
    let kept = state::load(&snapshot).expect("a snapshot");
    assert_eq!(kept.reason, "shutdown");
    assert_eq!(Some(&kept.programs), keeper.writer.last());
    clean_up(&path);

    // Between the two halves of a restore neither file is written.
    let (mut keeper, path) = watching_nothing("end-session-restore");
    keeper.finishing = Some(finish(Vec::new(), std::ptr::null_mut()));
    keeper.on_windows_message(WM_ENDSESSION, 1, 0);
    assert!(!path.exists() && !state::snapshot_path(&path).exists());
    clean_up(&path);
}

fn saved_at(programs: Programs, saved: &str, reason: &str) -> state::StateFile {
    state::StateFile {
        version: state::VERSION,
        saved: saved.to_string(),
        reason: reason.to_string(),
        programs,
    }
}

#[test]
fn the_start_layout_is_the_snapshot_unless_something_was_saved_after_it() {
    let at_shutdown = programs(vec![("chrome.exe", vec![kept("Shutdown", None, None, 0)])]);
    let later = programs(vec![("chrome.exe", vec![kept("Later", None, None, 0)])]);
    let snapshot = saved_at(at_shutdown.clone(), "2026-10-22T21:20:00+03:00", "shutdown");

    let unchanged = saved_at(later.clone(), "2026-10-22T18:00:00+03:00", "timer");
    let chosen = start_layout(Some(unchanged), Some(snapshot.clone())).unwrap();
    assert_eq!(chosen.programs, at_shutdown);

    let crashed = saved_at(later.clone(), "2026-10-23T09:00:00+03:00", "timer");
    let chosen = start_layout(Some(crashed), Some(snapshot.clone())).unwrap();
    assert_eq!(
        chosen.programs, later,
        "the session after it ended in a crash"
    );

    assert_eq!(
        start_layout(None, Some(snapshot)).unwrap().programs,
        at_shutdown
    );
    assert!(start_layout(None, None).is_none());
}

#[test]
fn init_starts_from_the_snapshot_when_it_is_the_newest() {
    let path = std::env::temp_dir().join(format!(
        "wincraft-test-layout-init-snapshot-{}.json",
        std::process::id()
    ));
    let mut writer = state::Writer::new(path.clone(), None);
    writer
        .write(
            programs(vec![("viber.exe", vec![kept("Older", None, None, 0)])]),
            "timer",
            "2026-10-22T18:00:00+03:00".to_string(),
            true,
        )
        .unwrap();
    let snapshot = state::Writer::new(path.clone(), Some(layout_file()));
    snapshot
        .write_snapshot("shutdown", "2026-10-22T21:20:00+03:00".to_string())
        .unwrap();
    let mut keeper = LayoutKeeper {
        state_path: path.clone(),
        ..LayoutKeeper::default()
    };
    let settings = serde_json::json!({
        "programs": "wincraft-test-nothing.exe",
        "restore_on_start": false,
    });
    let ctx = HostContext {
        hwnd: std::ptr::null_mut(),
        settings: &settings,
    };
    keeper.init(&ctx).unwrap();
    assert_eq!(keeper.writer.last(), Some(&layout_file()));
    keeper.host_hwnd = None;
    keeper.teardown();
    clean_up(&path);
}

#[test]
fn a_run_where_no_app_opened_says_so() {
    let mut run = Restore::new(15, 30 * 60);
    assert_eq!(
        nothing_restored(&run).as_deref(),
        Some("None of the saved apps opened a window within 30 minutes, so nothing was restored")
    );
    run.start(0, vec!["Chrome".to_string()]);
    assert_eq!(nothing_restored(&run), None, "Chrome is still waited for");
}

#[test]
fn saved_groups_start_with_their_saved_taskbar_order() {
    let mut viber = kept("Viber", None, None, 0);
    viber.group = "Viber.Desktop".to_string();
    let mut old = kept("From version 1", None, None, 0);
    old.group = String::new();
    let saved = programs(vec![
        (
            "chrome.exe",
            vec![kept("B", None, None, 1), kept("A", None, None, 0), old],
        ),
        ("viber.exe", vec![viber]),
    ]);
    let mut models = saved_models(Some(&saved));
    assert_eq!(
        models.keys().collect::<Vec<_>>(),
        ["Chrome", "Viber.Desktop"]
    );
    let chrome = models.get_mut("Chrome").unwrap();
    let live = [chrome_handle(1, "B"), chrome_handle(2, "A")];
    chrome.refresh(&live, 0, 30);
    assert_eq!(chrome.handles(), [2, 1], "A was saved first");
    assert!(saved_models(None).is_empty());
}

fn chrome_handle(handle: Handle, title: &str) -> (Handle, identity::WindowIdentity) {
    let window = chrome(handle, title, 0);
    (handle, window.identity)
}

#[test]
fn init_reads_the_saved_layout_and_teardown_saves_and_forgets() {
    let path = std::env::temp_dir().join(format!(
        "wincraft-test-layout-init-{}.json",
        std::process::id()
    ));
    let mut writer = state::Writer::new(path.clone(), None);
    writer
        .write(
            layout_file(),
            "manual",
            "2026-10-18T09:00:00".to_string(),
            true,
        )
        .unwrap();
    let mut keeper = LayoutKeeper {
        state_path: path.clone(),
        ..LayoutKeeper::default()
    };
    let settings = serde_json::json!({
        "programs": "wincraft-test-nothing.exe",
        "restore_on_start": false,
    });
    // No window: the timer is the thread's own, and goes with it.
    let ctx = HostContext {
        hwnd: std::ptr::null_mut(),
        settings: &settings,
    };
    keeper.init(&ctx).unwrap();
    assert_eq!(keeper.writer.last(), Some(&layout_file()));
    assert_eq!(
        keeper.groups.keys().collect::<Vec<_>>(),
        ["Chrome", "Viber.Desktop"]
    );
    assert!(keeper.host_hwnd.is_some() && keeper.started_once);
    assert!(!keeper.restore.is_running());

    keeper
        .labels
        .insert("Other".to_string(), "Other".to_string());
    keeper.teardown();
    assert_eq!(read_file(&path).reason, "teardown");
    assert!(keeper.host_hwnd.is_none() && keeper.reader.is_none());
    assert!(keeper.groups.is_empty() && keeper.labels.is_empty());

    // Switched on again, it reads what teardown wrote. Torn down in the
    // middle of a restore, it drops the restore and saves nothing.
    keeper.init(&ctx).unwrap();
    assert!(keeper.groups.is_empty(), "nothing watched was open");
    let written = std::fs::metadata(&path).unwrap().modified().unwrap();
    keeper.finishing = Some(finish(Vec::new(), std::ptr::null_mut()));
    keeper.teardown();
    assert!(keeper.finishing.is_none());
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        written
    );
    clean_up(&path);
}

/// Where the strip's windows cost their time on this PC. Read only: it
/// lists the real windows and changes none. Run by hand with
/// `cargo test strip_timing -- --ignored --nocapture`.
#[test]
#[ignore = "reads the real PC; run by hand"]
fn strip_timing_on_the_real_desktop() {
    use crate::core::windows_list;
    use std::time::Instant;
    use windows_sys::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
    let ms = |at: Instant| at.elapsed().as_secs_f64() * 1000.0;
    let mut keeper = LayoutKeeper::new();
    keeper.read_only = true;
    keeper.watch = Watch::new("*", "");
    keeper.reader = desktops::Reader::new().ok();
    let own_reader = desktops::Reader::new().ok();
    for round in 0..4 {
        let at = Instant::now();
        let top = windows_list::top_windows();
        let top_ms = ms(at);
        let at = Instant::now();
        let reader = own_reader.as_ref();
        let registry = desktops::list();
        let known = |hwnd: HWND| {
            reader
                .and_then(|r| r.read(hwnd))
                .is_some_and(|id| known_desktop(id, &registry))
        };
        let records = windows_list::app_window_records(&known);
        let records_ms = ms(at);
        let at = Instant::now();
        for (hwnd, _) in &records {
            let _ = appid::read(*hwnd);
        }
        let appid_ms = ms(at);
        let at = Instant::now();
        for (hwnd, _) in &records {
            let mut pid = 0;
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
                    *hwnd, &mut pid,
                )
            };
            let _ = windows::exe_path(pid);
        }
        let exe_ms = ms(at);
        let at = Instant::now();
        let live = keeper.live_windows();
        let live_ms = ms(at);
        let at = Instant::now();
        keeper.track(&live);
        let track_ms = ms(at);
        // In the app the Start menu index is ready after a moment, so each
        // label is looked up once; here it never is, so pretend it is.
        keeper.unresolved.clear();
        let at = Instant::now();
        let _ = desktops::list();
        let _ = desktops::current();
        let desk_ms = ms(at);
        let at = Instant::now();
        let mons = monitors::list();
        let mons_ms = ms(at);
        let at = Instant::now();
        for window in &live {
            let _ = strip_groups::desktop_of(reader, window.hwnd as Handle);
        }
        let desktop_of_ms = ms(at);
        let at = Instant::now();
        let _ = windows::describe(unsafe { GetForegroundWindow() });
        let front_ms = ms(at);
        let at = Instant::now();
        let groups = keeper.window_groups();
        let whole_ms = ms(at);
        eprintln!(
            "round {round}: {} top, {} app windows, {} watched, {} groups, {} monitors\n  \
             top {top_ms:.1} ms, records {records_ms:.1}, appid {appid_ms:.1}, exe {exe_ms:.1}, \
             live_windows {live_ms:.1}, track {track_ms:.1}, desktops {desk_ms:.1}, \
             monitors {mons_ms:.1}, desktop_of {desktop_of_ms:.1}, front {front_ms:.1}, \
             whole window_groups {whole_ms:.1}",
            top.len(),
            records.len(),
            live.len(),
            groups.map_or(0, |g| g.groups.len()),
            mons.len(),
        );
    }
}
