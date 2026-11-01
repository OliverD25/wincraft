use super::*;
use crate::core::theme;
use crate::core::ui_bridge::UiSnapshot;
use crate::search::providers::apps::Source;
use crate::search::{Context, Router};
use crate::ui::palette::{Outcome, Palette, HEIGHT, WIDTH};

const NOTEPAD: &str = r"c:\program files\notepad\notepad.exe";

fn app(name: &str, exe: Option<&str>, aumid: Option<&str>) -> App {
    let target = PathBuf::from(format!(r"C:\Start\{name}.lnk"));
    App {
        name: name.to_string(),
        icon: target.clone(),
        usage_key: target.to_string_lossy().to_lowercase(),
        target,
        subtitle: "Start menu".to_string(),
        source: Source::StartMenu,
        exe: exe.map(str::to_string),
        aumid: aumid.map(str::to_string),
        install_dir: String::new(),
    }
}

fn notepad() -> App {
    app("Notepad", Some(NOTEPAD), None)
}

fn window(hwnd: isize, title: &str, exe: &str, group: &str) -> OpenWindow {
    OpenWindow {
        hwnd,
        title: title.to_string(),
        app_title: None,
        exe_path: PathBuf::from(exe),
        program: "notepad".to_string(),
        other_desktop: false,
        explorer: false,
        group: group.to_string(),
    }
}

/// `count` Notepad windows, the one used last first, hwnd 100, 101, …
fn notepads(titles: &[&str]) -> Vec<OpenWindow> {
    titles
        .iter()
        .enumerate()
        .map(|(at, title)| window(100 + at as isize, title, NOTEPAD, NOTEPAD))
        .collect()
}

fn many(count: usize) -> Vec<OpenWindow> {
    let titles: Vec<String> = (0..count)
        .map(|at| format!("file{at}.txt \u{2013} Notepad"))
        .collect();
    let titles: Vec<&str> = titles.iter().map(String::as_str).collect();
    notepads(&titles)
}

fn running(windows: Vec<OpenWindow>, filter: &str) -> RunningApp {
    RunningApp::new(&notepad(), "apps:notepad".to_string(), windows, filter)
}

fn app_row(app: &App) -> ResultItem {
    ResultItem {
        group: "Apps".to_string(),
        title: app.name.clone(),
        subtitle: "Store app".to_string(),
        provider: "apps".to_string(),
        usage_key: Some(format!("apps:{}", app.usage_key)),
        enter: Some(Choice {
            label: "Open".to_string(),
            action: Action::Open(app.target.clone()),
        }),
        ..Default::default()
    }
}

fn plain(title: &str) -> ResultItem {
    ResultItem {
        group: "Rows".to_string(),
        title: title.to_string(),
        ..Default::default()
    }
}

#[test]
fn a_window_belongs_by_program_app_id_or_store_package() {
    let mine = window(1, "a", NOTEPAD, NOTEPAD);
    assert!(belongs(Some(NOTEPAD), None, &mine));
    let other = window(2, "b", r"c:\x\other.exe", r"c:\x\other.exe");
    assert!(!belongs(Some(NOTEPAD), None, &other));
    let by_id = window(
        3,
        "c",
        r"c:\x\frame.exe",
        "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App",
    );
    assert!(belongs(
        None,
        Some("microsoft.windowscalculator_8wekyb3d8bbwe!app"),
        &by_id
    ));
    let packaged = window(
        4,
        "d",
        r"C:\Program Files\WindowsApps\Microsoft.WindowsNotepad_11.2409.9.0_x64__8wekyb3d8bbwe\Notepad\Notepad.exe",
        "whatever",
    );
    assert!(belongs(
        None,
        Some("microsoft.windowsnotepad_8wekyb3d8bbwe!app"),
        &packaged
    ));
    assert!(!belongs(
        None,
        Some("microsoft.windowsstore_8wekyb3d8bbwe!app"),
        &packaged
    ));
    assert!(!belongs(None, None, &mine));
}

#[test]
fn only_the_first_row_nests_and_only_when_it_is_an_app_with_windows() {
    let pad = notepad();
    let index = vec![pad.clone()];
    let open = notepads(&["Untitled \u{2013} Notepad"]);
    let found = find(&[app_row(&pad)], "note", true, &index, &open).unwrap();
    assert_eq!(found.row, None);
    assert_eq!(found.app.windows.len(), 1);
    // An app further down never nests.
    assert_eq!(
        find(
            &[plain("Notes"), app_row(&pad)],
            "note",
            true,
            &index,
            &open
        ),
        None
    );
    // No window open: nothing nests.
    assert_eq!(find(&[app_row(&pad)], "note", true, &index, &[]), None);
    // An empty box nests nothing.
    assert_eq!(find(&[app_row(&pad)], "", true, &index, &open), None);
}

#[test]
fn a_tray_only_app_has_no_window_in_the_list_so_it_does_not_nest() {
    // The window list is the Alt+Tab list: a process with only a tray icon
    // has no row in it, so its app finds no window.
    let pad = notepad();
    let others = vec![window(9, "Chat", r"c:\x\chat.exe", r"c:\x\chat.exe")];
    assert_eq!(
        find(
            &[app_row(&pad)],
            "note",
            true,
            std::slice::from_ref(&pad),
            &others
        ),
        None
    );
}

#[test]
fn words_after_the_app_name_filter_its_window_titles() {
    let pad = notepad();
    let index = vec![pad.clone()];
    let open = notepads(&[
        "Untitled \u{2013} Notepad",
        "release-notes.md \u{2013} Notepad",
        "todo.txt \u{2013} Notepad",
        "release-plan.txt \u{2013} Notepad",
    ]);
    let found = find(&[plain("Something else")], "note rel", true, &index, &open).unwrap();
    assert_eq!(found.app.filter, "rel");
    assert_eq!(found.app.shown, vec![1, 3]);
    let row = found.row.clone().unwrap();
    assert_eq!(row.provider, "apps");
    assert_eq!(
        row.usage_key.as_deref(),
        Some("apps:c:\\start\\notepad.lnk")
    );
    // Under a prefix the words are the provider's, not a filter.
    assert_eq!(find(&[plain("x")], "note rel", false, &index, &open), None);
    // When the whole query names the app, it is the app.
    let code = app("Visual Studio Code", Some(NOTEPAD), None);
    assert_eq!(
        find(&[plain("x")], "visual studio", true, &[code], &open),
        None
    );
}

#[test]
fn nesting_puts_the_app_first_with_its_windows_under_it_and_drops_its_old_row() {
    let pad = notepad();
    let mut items = vec![plain("Notes"), app_row(&pad), plain("Other")];
    let found = Found {
        app: running(many(3), ""),
        row: Some(app_row(&pad)),
    };
    let app = nest(&mut items, found, false);
    let titles: Vec<&str> = items.iter().map(|item| item.title.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Notepad",
            "file0.txt \u{2013} Notepad",
            "file1.txt \u{2013} Notepad",
            "file2.txt \u{2013} Notepad",
            "Notes",
            "Other"
        ]
    );
    assert_eq!(items[1].nested, Some(Nested::Window { order: 0 }));
    assert_eq!(items[1].window(), Some(100));
    assert_eq!(items[1].group, items[0].group);
    assert_eq!(app.windows.len(), 3);
}

#[test]
fn three_windows_then_n_more_but_all_of_them_under_five() {
    for (count, rows) in [(1, 1), (3, 3), (4, 4)] {
        let shown = nested_rows(&running(many(count), ""), false, "Apps");
        assert_eq!(shown.len(), rows, "{count} windows");
        assert!(shown
            .iter()
            .all(|row| matches!(row.nested, Some(Nested::Window { .. }))));
    }
    let rows = nested_rows(&running(many(15), ""), false, "Apps");
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[3].nested, Some(Nested::More { hidden: 12 }));
    assert_eq!(rows[3].title, "12 more windows");
    assert_eq!(
        rows[3].subtitle,
        "Enter to expand \u{b7} or keep typing to filter"
    );
    let rows = nested_rows(&running(many(5), ""), false, "Apps");
    assert_eq!(rows[3].nested, Some(Nested::More { hidden: 2 }));
    // Expanded: every window, no "more" row.
    let rows = nested_rows(&running(many(15), ""), true, "Apps");
    assert_eq!(rows.len(), 15);
    assert!(rows
        .iter()
        .all(|row| row.nested != Some(Nested::More { hidden: 12 })));
}

#[test]
fn alt_keycaps_go_to_the_first_nine_on_screen() {
    let rows = nested_rows(&running(many(12), ""), true, "Apps");
    assert_eq!(rows[0].hint, "Alt+1");
    assert_eq!(rows[8].hint, "Alt+9");
    assert_eq!(rows[9].hint, "");
    let app = running(many(12), "");
    assert_eq!(app.alt_target(1), Some(100));
    assert_eq!(app.alt_target(9), Some(108));
    assert_eq!(app.alt_target(0), None);
    assert_eq!(app.alt_target(10), None);
    // After filtering, Alt+1 is the first match on screen.
    let app = running(notepads(&["a.txt", "rel1.txt", "b.txt", "rel2.txt"]), "rel");
    assert_eq!(app.alt_target(1), Some(101));
    assert_eq!(app.alt_target(2), Some(103));
    assert_eq!(app.alt_target(3), None);
}

#[test]
fn the_filter_works_collapsed_and_expanded_and_says_when_nothing_matches() {
    let titles: Vec<String> = (0..12).map(|at| format!("rel{at}.txt")).collect();
    let titles: Vec<&str> = titles.iter().map(String::as_str).collect();
    let mut open = notepads(&titles);
    open.push(window(500, "other.txt", NOTEPAD, NOTEPAD));
    let app = running(open, "rel");
    let collapsed = nested_rows(&app, false, "Apps");
    assert_eq!(collapsed.len(), 4);
    assert_eq!(collapsed[3].title, "9 more windows");
    assert_eq!(collapsed[3].subtitle, "Enter to expand");
    assert_eq!(nested_rows(&app, true, "Apps").len(), 12);

    let none = running(many(3), "zzz");
    let rows = nested_rows(&none, false, "Apps");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].nested, Some(Nested::NoMatch));
    assert_eq!(
        rows[0].title,
        "No Notepad window matches \u{201c}zzz\u{201d}"
    );
}

#[test]
fn the_app_row_says_how_many_match_and_shift_enter_goes_to_the_top_one() {
    let mut row = app_row(&notepad());
    let app = running(notepads(&["a.txt", "rel1.txt", "b.txt", "rel2.txt"]), "rel");
    decorate(&mut row, &app);
    assert_eq!(
        row.subtitle,
        "Store app \u{b7} 2 of 4 windows match \u{201c}rel\u{201d}"
    );
    assert_eq!(
        row.shift_enter.as_ref().map(|choice| &choice.action),
        Some(&Action::Activate(101))
    );
    // Enter still opens a new instance.
    assert!(matches!(
        row.enter.as_ref().map(|choice| &choice.action),
        Some(Action::Open(_))
    ));

    let mut row = app_row(&notepad());
    decorate(&mut row, &running(many(15), ""));
    assert_eq!(row.subtitle, "Store app \u{b7} 15 windows open");
    assert_eq!(
        row.shift_enter.as_ref().map(|choice| &choice.action),
        Some(&Action::Activate(100))
    );
}

#[test]
fn an_app_that_keeps_one_window_switches_on_enter() {
    let settings = app(
        "Settings",
        None,
        Some("windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel"),
    );
    let app = RunningApp::new(&settings, "apps:settings".to_string(), many(1), "");
    assert!(app.single);
    let mut row = app_row(&settings);
    decorate(&mut row, &app);
    assert_eq!(
        row.enter.as_ref().map(|choice| &choice.action),
        Some(&Action::Activate(100))
    );
    assert!(matches!(
        row.shift_enter.as_ref().map(|choice| &choice.action),
        Some(Action::Open(_))
    ));
}

#[test]
fn down_enters_the_windows_and_leaves_past_the_last_and_up_goes_back() {
    let mut items = vec![app_row(&notepad()), plain("Next")];
    nest(
        &mut items,
        Found {
            app: running(many(2), ""),
            row: None,
        },
        false,
    );
    // App, window 1, window 2, Next.
    assert_eq!(step(&items, 0, true), 1);
    assert_eq!(step(&items, 2, true), 3);
    assert_eq!(step(&items, 1, false), 0);
    assert_eq!(step(&items, 3, true), 0);
}

#[test]
fn the_no_match_line_is_skipped_by_the_arrows() {
    let mut items = vec![app_row(&notepad()), plain("Next")];
    nest(
        &mut items,
        Found {
            app: running(many(2), "zzz"),
            row: None,
        },
        false,
    );
    assert_eq!(items[1].nested, Some(Nested::NoMatch));
    assert_eq!(step(&items, 0, true), 2);
    assert_eq!(step(&items, 2, false), 0);
}

#[test]
fn page_keys_move_eight_windows_and_stay_on_the_windows() {
    let mut items = vec![app_row(&notepad()), plain("Next")];
    nest(
        &mut items,
        Found {
            app: running(many(20), ""),
            row: None,
        },
        true,
    );
    assert_eq!(page(&items, 1, true), 9);
    assert_eq!(page(&items, 17, true), 20);
    assert_eq!(page(&items, 9, false), 1);
    assert_eq!(page(&items, 0, true), 0);
}

fn desktops() -> Vec<Desktop> {
    (1..=3)
        .map(|at| Desktop {
            id: DesktopId(at),
            name: format!("Desktop {at}"),
        })
        .collect()
}

#[test]
fn the_desktop_summary_puts_this_desktop_first_and_counts_each() {
    let places = [
        Some(DesktopId(2)),
        Some(DesktopId(1)),
        Some(DesktopId(2)),
        Some(DesktopId::ALL),
        Some(DesktopId(3)),
    ];
    let summary = summary(&places, &desktops(), Some(DesktopId(2)));
    let lines: Vec<(&str, usize, bool)> = summary
        .lines
        .iter()
        .map(|line| (line.name.as_str(), line.count, line.current))
        .collect();
    assert_eq!(
        lines,
        [
            ("Desktop 2 \u{b7} this one", 2, true),
            ("Desktop 1", 1, false),
            ("Desktop 3", 1, false),
            ("All desktops", 1, false),
        ]
    );
    assert_eq!(summary.title(), "5 windows on 3 desktops");
    assert_eq!(summary.lines[0].windows, vec![0, 2]);
}

#[test]
fn with_one_desktop_every_window_is_on_this_one() {
    let summary = summary(&[None, None], &[], None);
    assert_eq!(summary.lines.len(), 1);
    assert_eq!(summary.lines[0].name, "Desktop 1 \u{b7} this one");
    assert_eq!(summary.title(), "2 windows on 1 desktop");
}

#[test]
fn ctrl_d_lists_the_windows_under_one_header_per_desktop() {
    let app = running(many(4), "");
    let places = [
        Some(DesktopId(1)),
        Some(DesktopId(2)),
        Some(DesktopId(1)),
        Some(DesktopId(2)),
    ];
    let summary = summary(&places, &desktops(), Some(DesktopId(2)));
    let rows = by_desktop_rows(&app, &summary, "");
    let seen: Vec<(&str, isize)> = rows
        .iter()
        .map(|row| (row.group.as_str(), row.window().unwrap()))
        .collect();
    assert_eq!(
        seen,
        [
            ("Desktop 2 \u{b7} this one \u{b7} 2", 101),
            ("Desktop 2 \u{b7} this one \u{b7} 2", 103),
            ("Desktop 1 \u{b7} 2", 100),
            ("Desktop 1 \u{b7} 2", 102),
        ]
    );
    assert_eq!(rows[0].hint, "Alt+2");
    assert!(matches!(
        rows[0].ctrl_enter.as_ref().map(|choice| &choice.action),
        Some(Action::Open(_))
    ));
    let typed = by_desktop_rows(&app, &summary, "file3");
    assert_eq!(typed.len(), 1);
}

#[test]
fn the_footer_reads_as_the_design_says() {
    let keys = |hints: Vec<Hint>| -> Vec<String> {
        hints
            .into_iter()
            .map(|hint| format!("{} {}", hint.key, hint.label))
            .collect()
    };
    let app = running(many(3), "");
    assert_eq!(
        keys(hints(Spot::App, &app, false, "Close")),
        [
            "\u{21B5} Open new",
            "Shift+\u{21B5} Switch",
            "\u{2193} Open windows",
            "Ctrl+D By desktop",
            "Esc Close"
        ]
    );
    assert_eq!(
        keys(hints(Spot::Window, &app, false, "Close")),
        [
            "\u{21B5} Switch",
            "\u{2191} Back to app",
            "Shift+\u{2192} Next monitor",
            "Ctrl+W Close window",
            "Esc Close"
        ]
    );
    let expanded = keys(hints(Spot::Window, &app, true, "Close"));
    assert!(expanded.contains(&"\u{2190} Collapse".to_string()));
    assert_eq!(expanded.last().unwrap(), "Esc Collapse");
    let by_desktop = hints(Spot::ByDesktop, &app, false, "Close");
    assert!(by_desktop[0].accent);
    assert_eq!(
        keys(by_desktop),
        [
            "Backspace Back",
            "\u{21B5} Switch",
            "Ctrl+\u{21B5} Open new",
            "Ctrl+D Back",
            "Esc Back"
        ]
    );
    let filtered = keys(hints(Spot::App, &running(many(3), "file"), false, "Close"));
    assert_eq!(filtered[1], "Shift+\u{21B5} Switch to match");
}

#[test]
fn enter_on_n_more_expands() {
    let actions = adjust(Vec::new(), Spot::More, &running(many(9), ""));
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].key, Key::Enter);
    assert_eq!(actions[0].run, Run::Expand);
}

// The palette itself, driven by keys over fake apps and windows. Nothing
// here reads or touches a real window.

struct AppRows(Vec<ResultItem>);

impl crate::search::SearchProvider for AppRows {
    fn id(&self) -> &'static str {
        "apps"
    }
    fn name(&self) -> &'static str {
        "Apps"
    }
    fn description(&self) -> &'static str {
        "test apps"
    }
    fn blended(&self) -> bool {
        true
    }
    fn query(&mut self, _query: &crate::search::Query, _context: &Context) -> Vec<ResultItem> {
        self.0.clone()
    }
}

fn palette_with(windows: Vec<OpenWindow>) -> Palette {
    let pad = notepad();
    let mut row = app_row(&pad);
    row.provider = String::new();
    row.usage_key = Some(pad.usage_key.clone());
    let mut palette = Palette::new(Router::new(
        vec![(None, Box::new(AppRows(vec![row, plain("Next")])))],
        &Default::default(),
    ));
    palette.opened();
    palette.running.fake = Some((vec![pad], windows));
    palette.type_in("note");
    palette
}

fn press(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn frame(palette: &mut Palette, ctx: &egui::Context, events: Vec<egui::Event>) -> Outcome {
    frame_with(palette, ctx, &crate::core::ui_bridge::create(), events)
}

fn frame_with(
    palette: &mut Palette,
    ctx: &egui::Context,
    bridge: &crate::core::ui_bridge::Bridge,
    events: Vec<egui::Event>,
) -> Outcome {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(WIDTH, HEIGHT),
        )),
        events,
        ..Default::default()
    };
    let mut outcome = Outcome::Stay;
    let mut output = ctx.run_ui(input, |ui| {
        outcome = palette.show(ui, &UiSnapshot::default(), &bridge.to_host, 1.0, true);
    });
    output.textures_delta.clear();
    outcome
}

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
    theme::apply(
        &ctx,
        crate::core::config::ThemeName::Classic,
        crate::core::config::ThemeChoice::Dark,
    );
    ctx
}

const NONE: egui::Modifiers = egui::Modifiers::NONE;

#[test]
fn the_palette_nests_the_windows_and_the_arrows_walk_them() {
    let ctx = context();
    let mut palette = palette_with(many(2));
    frame(&mut palette, &ctx, Vec::new());
    assert_eq!(palette.results.items.len(), 4);
    assert_eq!(palette.spot(), Some(Spot::App));
    frame(&mut palette, &ctx, vec![press(egui::Key::ArrowDown, NONE)]);
    assert_eq!(palette.selected, 1);
    assert_eq!(palette.spot(), Some(Spot::Window));
    frame(&mut palette, &ctx, vec![press(egui::Key::ArrowUp, NONE)]);
    assert_eq!(palette.selected, 0);
    // Up and Down never move the text cursor (US-001): the box keeps "note".
    assert_eq!(palette.query, "note");
}

#[test]
fn n_more_expands_and_esc_or_left_collapse_before_esc_closes() {
    let ctx = context();
    let mut palette = palette_with(many(15));
    frame(&mut palette, &ctx, Vec::new());
    assert_eq!(palette.results.items.len(), 1 + 4 + 1);
    palette.selected = 4;
    assert_eq!(palette.spot(), Some(Spot::More));
    frame(&mut palette, &ctx, vec![press(egui::Key::Enter, NONE)]);
    frame(&mut palette, &ctx, Vec::new());
    assert!(palette.expanded());
    assert_eq!(palette.results.items.len(), 1 + 15 + 1);
    assert_eq!(palette.spot(), Some(Spot::Window));
    // Esc folds the list; the palette stays.
    let outcome = frame(&mut palette, &ctx, vec![press(egui::Key::Escape, NONE)]);
    assert!(matches!(outcome, Outcome::Stay));
    frame(&mut palette, &ctx, Vec::new());
    assert!(!palette.expanded());
    assert_eq!(palette.selected, 4);
    // Expand again, then ← folds it too.
    frame(&mut palette, &ctx, vec![press(egui::Key::Enter, NONE)]);
    frame(&mut palette, &ctx, Vec::new());
    palette.selected = 9;
    frame(&mut palette, &ctx, vec![press(egui::Key::ArrowLeft, NONE)]);
    frame(&mut palette, &ctx, Vec::new());
    assert!(!palette.expanded());
    assert_eq!(palette.selected, 4);
    // Now Esc closes.
    let outcome = frame(&mut palette, &ctx, vec![press(egui::Key::Escape, NONE)]);
    assert!(matches!(outcome, Outcome::Hide));
}

#[test]
fn ctrl_d_opens_the_view_by_desktop_and_ctrl_d_or_backspace_return() {
    let ctx = context();
    let mut palette = palette_with(many(3));
    frame(&mut palette, &ctx, Vec::new());
    frame(
        &mut palette,
        &ctx,
        vec![press(egui::Key::D, egui::Modifiers::COMMAND)],
    );
    frame(&mut palette, &ctx, Vec::new());
    assert_eq!(palette.spot(), Some(Spot::ByDesktop));
    assert_eq!(palette.query, "");
    assert_eq!(
        palette.by_desktop_chip().as_deref(),
        Some("Notepad \u{b7} by desktop")
    );
    assert_eq!(palette.results.items.len(), 3);
    assert_eq!(
        palette.results.items[0].group,
        "Desktop 1 \u{b7} this one \u{b7} 3"
    );
    frame(
        &mut palette,
        &ctx,
        vec![press(egui::Key::D, egui::Modifiers::COMMAND)],
    );
    frame(&mut palette, &ctx, Vec::new());
    assert_eq!(palette.query, "note");
    assert_eq!(palette.spot(), Some(Spot::App));

    frame(
        &mut palette,
        &ctx,
        vec![press(egui::Key::D, egui::Modifiers::COMMAND)],
    );
    frame(&mut palette, &ctx, vec![press(egui::Key::Backspace, NONE)]);
    frame(&mut palette, &ctx, Vec::new());
    assert_eq!(palette.query, "note");
    assert_eq!(palette.spot(), Some(Spot::App));
}

#[test]
fn shift_enter_and_alt_digits_pick_the_windows_in_screen_order() {
    let ctx = context();
    let mut palette = palette_with(many(3));
    frame(&mut palette, &ctx, Vec::new());
    let actions = palette.current_actions();
    let shift = actions
        .iter()
        .find(|action| action.key == Key::ShiftEnter)
        .unwrap();
    assert_eq!(shift.run, Run::Action(Action::Activate(100)));
    assert_eq!(shift.short, "Switch");
    assert_eq!(actions[0].label, "Open new window");
    let bridge = crate::core::ui_bridge::create();
    let outcome = frame_with(
        &mut palette,
        &ctx,
        &bridge,
        vec![press(egui::Key::Num2, egui::Modifiers::ALT)],
    );
    assert!(matches!(outcome, Outcome::Hide));
    let sent: Vec<_> = bridge.host_rx.try_iter().collect();
    assert!(sent.iter().any(|request| matches!(
        request,
        crate::core::ui_bridge::HostRequest::RunAction(Action::Activate(101))
    )));
}
