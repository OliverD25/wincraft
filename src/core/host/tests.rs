use super::*;

// The plugin and hotkey bookkeeping below runs with fake plugins, a fake
// keyboard instead of RegisterHotKey, and settings files in a scratch
// folder. No test registers a real hotkey or touches the real profile.

use std::rc::Rc;

use crate::core::traits::{
    HotkeyAction, PaletteCommand, StripLook, TrayAction, TrayToggle, WindowGroups,
};
use crate::core::ui_bridge::{ArrangeView, ArrangeWindow, LayoutAction};

#[test]
fn the_strip_hears_of_windows_that_come_go_or_take_the_front() {
    for code in [1, 2, 4, 0x8004] {
        assert!(window_list_changed(code), "{code:#x}");
    }
    // A flash, a redraw and a title change leave the list as it is.
    for code in [0x8006, 6, 0] {
        assert!(!window_list_changed(code), "{code:#x}");
    }
}

/// Stands in for RegisterHotKey: keys another app holds, and keys
/// already registered here, are refused, as Windows refuses them.
#[derive(Default)]
struct Keyboard {
    taken_by_others: Vec<Hotkey>,
    registered: BTreeMap<i32, Hotkey>,
    freed: Vec<i32>,
}

#[derive(Clone, Default)]
struct FakeKeys(Rc<RefCell<Keyboard>>);

impl Registrar for FakeKeys {
    fn register(&mut self, id: i32, hotkey: Hotkey) -> bool {
        let mut keyboard = self.0.borrow_mut();
        let taken = keyboard.taken_by_others.contains(&hotkey)
            || keyboard.registered.contains_key(&id)
            || keyboard.registered.values().any(|held| *held == hotkey);
        if !taken {
            keyboard.registered.insert(id, hotkey);
        }
        !taken
    }

    fn unregister(&mut self, id: i32) {
        let mut keyboard = self.0.borrow_mut();
        keyboard.registered.remove(&id);
        keyboard.freed.push(id);
    }
}

impl FakeKeys {
    fn held(&self) -> Vec<String> {
        self.0
            .borrow()
            .registered
            .values()
            .map(|hotkey| hotkeys::format(*hotkey))
            .collect()
    }
}

fn keys(text: &str) -> Hotkey {
    hotkeys::parse(text).unwrap()
}

type Diary = Rc<RefCell<Vec<String>>>;

/// A plugin that writes down what the host asks of it.
struct FakePlugin {
    id: &'static str,
    name: &'static str,
    /// (action id, name, label, default keys)
    actions: Vec<(u32, &'static str, &'static str, &'static str)>,
    fails_with: Option<&'static str>,
    takes_settings_live: bool,
    /// Keeps a window order for the strip, as LayoutKeeper does.
    groups: bool,
    /// Decides when the strip opens, as WindowDeck does.
    look: bool,
    diary: Diary,
}

fn fake(id: &'static str, name: &'static str, default: &'static str, diary: &Diary) -> FakePlugin {
    FakePlugin {
        id,
        name,
        actions: vec![(1, "go", "Go", default)],
        fails_with: None,
        takes_settings_live: true,
        groups: false,
        look: false,
        diary: Rc::clone(diary),
    }
}

impl FakePlugin {
    fn note(&self, what: String) {
        self.diary.borrow_mut().push(what);
    }
}

impl WinCraftPlugin for FakePlugin {
    fn metadata(&self) -> crate::core::traits::PluginMetadata {
        crate::core::traits::PluginMetadata {
            id: self.id,
            name: self.name,
            description: "A plugin for tests.",
            author: "test",
            version: "1.0.0",
            readme: "",
            category: "Test",
            icon: None,
            thumbnail: None,
        }
    }

    fn default_settings(&self) -> Value {
        serde_json::json!({ "level": 1 })
    }

    fn init(&mut self, ctx: &HostContext) -> Result<(), String> {
        self.note(format!("init {} level {}", self.id, ctx.settings["level"]));
        match self.fails_with {
            Some(why) => Err(why.to_string()),
            None => Ok(()),
        }
    }

    fn hotkey_actions(&self) -> Vec<HotkeyAction> {
        self.actions
            .iter()
            .map(|(id, name, label, default)| HotkeyAction {
                id: *id,
                name,
                label,
                default: keys(default),
            })
            .collect()
    }

    fn on_hotkey(&mut self, action_id: u32) {
        self.note(format!("hotkey {} {action_id}", self.id));
    }

    fn tray_actions(&self) -> Vec<TrayAction> {
        vec![
            TrayAction { id: 1, label: "Go" },
            TrayAction {
                id: 2,
                label: "Open",
            },
        ]
    }

    fn on_tray_action(&mut self, action_id: u32) {
        self.note(format!("tray {} {action_id}", self.id));
    }

    fn tray_toggles(&self) -> Vec<TrayToggle> {
        vec![TrayToggle {
            id: 5,
            label: "Feature".into(),
            on: true,
        }]
    }

    fn palette_commands(&self) -> Vec<PaletteCommand> {
        vec![PaletteCommand {
            id: 7,
            label: "Tidy up",
            hint: "tidy",
            words: "",
            confirm: if self.id == "fake_confirm" {
                "Press Enter again to tidy up"
            } else {
                ""
            },
            list: self.id == "fake_list",
            toggle: self.id.starts_with("fake_toggle").then_some(
                crate::core::ui_bridge::ToggleLook {
                    icon_on: "eye",
                    icon_off: "eye-off",
                    tile: [1, 2, 3],
                },
            ),
        }]
    }

    fn toggle_state(&mut self, kind: ActionKind, action_id: u32) -> Option<ToggleState> {
        self.note(format!("toggle state {} {action_id}", self.id));
        (kind == ActionKind::Palette && self.id == "fake_toggle")
            .then(|| ToggleState::on("Tidy now"))
    }

    fn command_list(
        &mut self,
        kind: ActionKind,
        action_id: u32,
    ) -> Option<crate::core::ui_bridge::SubList> {
        (kind == ActionKind::Palette && action_id == 7).then(|| crate::core::ui_bridge::SubList {
            title: "Tidy".to_string(),
            rows: vec![crate::core::ui_bridge::ListRow {
                id: "desk".to_string(),
                title: "The desk".to_string(),
                ..Default::default()
            }],
            empty: String::new(),
        })
    }

    fn on_list_pick(&mut self, _kind: ActionKind, action_id: u32, row: &str) {
        self.note(format!("pick {} {action_id} {row}", self.id));
    }

    fn on_palette_command(&mut self, id: u32) {
        self.note(format!("palette {} {id}", self.id));
    }

    fn command_details(
        &mut self,
        kind: ActionKind,
        action_id: u32,
    ) -> Option<crate::core::ui_bridge::CommandDetails> {
        (kind == ActionKind::Palette && action_id == 7).then(|| {
            crate::core::ui_bridge::CommandDetails {
                status: format!("{} is untidy", self.id),
                lines: vec![("Mess".to_string(), "a lot".to_string())],
            }
        })
    }

    fn on_settings_changed(&mut self, settings: &Value) -> bool {
        self.note(format!("settings {} {settings}", self.id));
        self.takes_settings_live
    }

    fn on_prompt_answer(&mut self, prompt_id: u32, _target: isize, text: &str) {
        self.note(format!("answer {} {prompt_id} {text}", self.id));
    }

    fn on_layout_action(&mut self, _action: &LayoutAction) {
        self.note(format!("layout {}", self.id));
    }

    fn window_groups(&mut self) -> Option<WindowGroups> {
        self.groups.then(|| WindowGroups {
            groups: vec![ArrangeGroup {
                key: "Chrome.UserData.Profile3".to_string(),
                label: "Chrome".to_string(),
                windows: vec![ArrangeWindow {
                    hwnd: 0x7fff_fff0,
                    label: "one".to_string(),
                    desktop: String::new(),
                    monitor: None,
                    on_monitor: None,
                    saved_monitor: None,
                    renamed: false,
                    same_title: false,
                    attention: false,
                }],
            }],
            desktops: Vec::new(),
            focus: 0,
            watched: vec!["*".to_string()],
            monitors: Vec::new(),
        })
    }

    fn on_arrange_action(&mut self, _action: &ArrangeAction) {
        self.note(format!("arrange {}", self.id));
    }

    fn strip_look(&self) -> Option<StripLook> {
        self.look.then_some(StripLook {
            preview: false,
            view: ArrangeView::Flat,
            hints: true,
        })
    }

    fn teardown(&mut self) {
        self.note(format!("teardown {}", self.id));
    }
}

/// A data folder of its own, removed afterwards.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("wincraft-host-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn saved(&self, id: &str) -> PluginConfig {
        PluginConfig::load_in(&self.0, id)
    }

    fn has_file(&self, id: &str) -> bool {
        self.0.join("plugins").join(format!("{id}.json")).exists()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn plugin_set(plugins: Vec<FakePlugin>, keyboard: &FakeKeys, data: &Scratch) -> PluginSet {
    let plugins = plugins
        .into_iter()
        .map(|plugin| Box::new(plugin) as Box<dyn WinCraftPlugin>)
        .collect();
    boxed_set(plugins, keyboard, data)
}

fn boxed_set(
    plugins: Vec<Box<dyn WinCraftPlugin>>,
    keyboard: &FakeKeys,
    data: &Scratch,
) -> PluginSet {
    let mut set = PluginSet::new(
        std::ptr::null_mut(),
        plugins,
        Box::new(keyboard.clone()),
        data.0.clone(),
    );
    set.load_configs();
    set
}

fn diary() -> Diary {
    Rc::new(RefCell::new(Vec::new()))
}

fn read(diary: &Diary) -> Vec<String> {
    std::mem::take(&mut *diary.borrow_mut())
}

#[test]
fn a_plugin_starts_with_its_saved_settings_and_registers_its_hotkeys() {
    let data = Scratch::new("start");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut set = plugin_set(
        vec![
            fake("fake_a", "Fake A", "Win+Alt+F1", &log),
            fake("fake_b", "Fake B", "Win+Alt+F2", &log),
        ],
        &keyboard,
        &data,
    );
    assert_eq!(set.wanted(), [0, 1], "no file says a plugin is off");
    for index in set.wanted() {
        set.enable(index);
    }
    assert_eq!(read(&log), ["init fake_a level 1", "init fake_b level 1"]);
    assert_eq!(keyboard.held(), ["Win+Alt+F1", "Win+Alt+F2"]);
    assert!(set.warnings.is_empty());
    let saved = data.saved("fake_a");
    assert!(saved.enabled);
    assert_eq!(saved.hotkeys["go"], "Win+Alt+F1");
    assert_eq!(saved.settings["level"], 1);
    assert_eq!(
        set.registered(),
        [
            ("Fake A".to_string(), "Go".to_string(), keys("Win+Alt+F1")),
            ("Fake B".to_string(), "Go".to_string(), keys("Win+Alt+F2")),
        ]
    );
    assert_eq!(set.hotkey_target(1), Some((0, 1)));
    assert_eq!(set.hotkey_target(2), Some((1, 1)));
    assert_eq!(set.hotkey_target(3), None);
    // Asked again, a running plugin is not started twice.
    set.enable(0);
    assert!(read(&log).is_empty());
}

#[test]
fn a_plugin_saved_as_off_is_not_started() {
    let data = Scratch::new("saved-off");
    let off = PluginConfig {
        enabled: false,
        ..PluginConfig::default()
    };
    off.save_in(&data.0, "fake_b").unwrap();
    let log = diary();
    let set = plugin_set(
        vec![
            fake("fake_a", "Fake A", "Win+Alt+F1", &log),
            fake("fake_b", "Fake B", "Win+Alt+F2", &log),
        ],
        &FakeKeys::default(),
        &data,
    );
    assert_eq!(set.wanted(), [0]);
    // Its file got the defaults it was missing.
    assert_eq!(data.saved("fake_b").hotkeys["go"], "Win+Alt+F2");
}

#[test]
fn a_plugin_whose_start_fails_stays_off_and_keeps_its_saved_flag() {
    let data = Scratch::new("crash");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut broken = fake("broken", "Broken", "Win+Alt+F1", &log);
    broken.fails_with = Some("no device");
    let mut set = plugin_set(vec![broken], &keyboard, &data);
    set.enable(0);
    assert!(!set.slots[0].enabled);
    assert!(set.slots[0].registered.is_empty());
    assert!(keyboard.held().is_empty());
    assert_eq!(set.next_hotkey_id, 1, "no hotkey id was used up");
    assert_eq!(set.warnings, ["Broken failed to start: no device"]);
    assert!(data.saved("broken").enabled, "the next start tries again");
    assert_eq!(set.wanted(), [0]);
    // Turning it off now changes nothing: it is not running.
    set.disable(0);
    assert!(data.saved("broken").enabled);
    assert_eq!(read(&log), ["init broken level 1"]);
}

#[test]
fn turning_a_plugin_off_frees_its_hotkeys_and_saves_it_off() {
    let data = Scratch::new("off");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut set = plugin_set(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    set.enable(0);
    set.disable(0);
    assert!(!set.slots[0].enabled);
    assert!(keyboard.held().is_empty());
    assert_eq!(read(&log), ["init fake_a level 1", "teardown fake_a"]);
    assert!(!data.saved("fake_a").enabled);
    assert!(set.wanted().is_empty());
    set.disable(0);
    assert!(read(&log).is_empty(), "no second teardown");
    // Back on with a fresh hotkey id, saved on again.
    set.enable(0);
    assert_eq!(set.slots[0].registered[0].global_id, 2);
    assert!(data.saved("fake_a").enabled);
}

#[test]
fn quitting_stops_every_plugin_but_saves_none_of_them_as_off() {
    let data = Scratch::new("quit");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut set = plugin_set(
        vec![
            fake("fake_a", "Fake A", "Win+Alt+F1", &log),
            fake("fake_b", "Fake B", "Win+Alt+F2", &log),
        ],
        &keyboard,
        &data,
    );
    set.register_palette("Win+Alt+Space");
    set.enable(0);
    set.enable(1);
    read(&log);
    set.stop_all();
    assert_eq!(read(&log), ["teardown fake_a", "teardown fake_b"]);
    assert!(
        keyboard.held().is_empty(),
        "the palette's keys are free too"
    );
    assert!(set.slots.iter().all(|slot| !slot.enabled));
    assert!(data.saved("fake_a").enabled && data.saved("fake_b").enabled);
    assert!(set.configs["fake_a"].enabled);
    assert_eq!(set.wanted(), [0, 1]);
}

#[test]
fn a_hotkey_another_app_holds_is_reported_and_never_freed() {
    let data = Scratch::new("taken");
    let (log, keyboard) = (diary(), FakeKeys::default());
    keyboard
        .0
        .borrow_mut()
        .taken_by_others
        .push(keys("Win+Alt+F1"));
    let mut a = fake("fake_a", "Fake A", "Win+Alt+F1", &log);
    a.actions.push((2, "stop", "Stop", "Win+Alt+F4"));
    let mut set = plugin_set(vec![a], &keyboard, &data);
    set.enable(0);
    assert!(set.slots[0].enabled, "the plugin runs without that hotkey");
    assert_eq!(set.warnings, ["Win+Alt+F1 is used by another app"]);
    let infos = set.hotkey_infos(0);
    let shown: Vec<(&str, &str, bool)> = infos
        .iter()
        .map(|info| (info.action.as_str(), info.binding.as_str(), info.registered))
        .collect();
    assert_eq!(
        shown,
        [("go", "Win+Alt+F1", false), ("stop", "Win+Alt+F4", true)]
    );
    assert_eq!(
        set.registered().len(),
        1,
        "only keys it holds count as its own"
    );
    set.disable(0);
    assert_eq!(
        keyboard.0.borrow().freed,
        [2],
        "the refused id is not freed"
    );
}

#[test]
fn two_plugins_that_want_the_same_keys_get_them_in_turn() {
    let data = Scratch::new("clash");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut set = plugin_set(
        vec![
            fake("fake_a", "Fake A", "Win+Alt+F1", &log),
            fake("fake_b", "Fake B", "Win+Alt+F1", &log),
        ],
        &keyboard,
        &data,
    );
    set.enable(0);
    set.enable(1);
    assert!(set.slots[0].registered[0].ok);
    assert!(!set.slots[1].registered[0].ok);
    assert_eq!(
        set.warnings,
        ["Win+Alt+F1 is used by Go (Fake A) in WinCraft"]
    );
    // Once the first lets go, the second gets them on its next start.
    set.disable(0);
    set.set_hotkey("fake_b", "go", "Win+Alt+F1");
    assert!(set.slots[1].registered[0].ok);
}

#[test]
fn the_palette_hotkey_clashes_like_any_other_and_can_be_rebound() {
    let data = Scratch::new("palette");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut set = plugin_set(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    set.register_palette("Win+Alt+F1");
    assert_eq!(set.palette_hotkey, Some((keys("Win+Alt+F1"), true)));
    set.enable(0);
    assert!(!set.slots[0].registered[0].ok, "the palette was first");
    assert_eq!(
        set.warnings,
        ["Win+Alt+F1 is used by Open the palette in WinCraft"]
    );

    assert_eq!(
        set.rebind_palette("Win+Alt+F9"),
        Ok("Win+Alt+F9".to_string())
    );
    assert_eq!(keyboard.held(), ["Win+Alt+F9"]);
    assert!(set.rebind_palette("Win+Nothing").is_err());
    assert_eq!(set.palette_hotkey, Some((keys("Win+Alt+F9"), true)));

    // A palette hotkey another app holds is not freed when rebound.
    keyboard
        .0
        .borrow_mut()
        .taken_by_others
        .push(keys("Win+Alt+F8"));
    set.rebind_palette("Win+Alt+F8").unwrap();
    assert_eq!(set.palette_hotkey, Some((keys("Win+Alt+F8"), false)));
    let freed = keyboard.0.borrow().freed.len();
    set.rebind_palette("Win+Alt+F7").unwrap();
    assert_eq!(keyboard.0.borrow().freed.len(), freed);
}

#[test]
fn a_palette_hotkey_that_does_not_parse_falls_back_to_the_default() {
    let data = Scratch::new("palette-default");
    let keyboard = FakeKeys::default();
    let mut set = plugin_set(Vec::new(), &keyboard, &data);
    set.register_palette("Ctrl+Nonsense");
    assert_eq!(
        set.palette_hotkey,
        Some((keys(DEFAULT_PALETTE_HOTKEY), true))
    );
}

#[test]
fn a_rebound_hotkey_is_saved_and_the_plugin_restarted_with_it() {
    let data = Scratch::new("rebind");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut set = plugin_set(
        vec![
            fake("fake_a", "Fake A", "Win+Alt+F1", &log),
            fake("fake_b", "Fake B", "Win+Alt+F2", &log),
        ],
        &keyboard,
        &data,
    );
    set.enable(0);
    read(&log);
    set.set_hotkey("fake_a", "go", "win+alt+f3");
    assert_eq!(data.saved("fake_a").hotkeys["go"], "Win+Alt+F3");
    assert_eq!(keyboard.held(), ["Win+Alt+F3"]);
    assert_eq!(read(&log), ["teardown fake_a", "init fake_a level 1"]);
    assert_eq!(set.hotkey_infos(0)[0].default_binding, "Win+Alt+F1");

    set.set_hotkey("fake_a", "go", "Win+Banana");
    assert_eq!(data.saved("fake_a").hotkeys["go"], "Win+Alt+F3");
    assert!(read(&log).is_empty(), "nothing restarted");

    // A plugin that is off only gets its file changed.
    set.set_hotkey("fake_b", "go", "Win+Alt+F5");
    assert_eq!(data.saved("fake_b").hotkeys["go"], "Win+Alt+F5");
    assert!(read(&log).is_empty());
    assert_eq!(set.hotkey_infos(1)[0].binding, "Win+Alt+F5");
}

#[test]
fn a_saved_binding_that_does_not_parse_gives_the_plugin_default() {
    let data = Scratch::new("bad-binding");
    let mut file = PluginConfig::default();
    file.hotkeys.insert("go".to_string(), "Hyper+Q".to_string());
    file.save_in(&data.0, "fake_a").unwrap();
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut set = plugin_set(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    let infos = set.hotkey_infos(0);
    assert_eq!(
        (infos[0].binding.as_str(), infos[0].registered),
        ("Win+Alt+F1", true)
    );
    set.enable(0);
    assert_eq!(keyboard.held(), ["Win+Alt+F1"]);
    assert_eq!(
        data.saved("fake_a").hotkeys["go"],
        "Hyper+Q",
        "the file keeps it"
    );
}

#[test]
fn a_setting_the_plugin_cannot_take_live_restarts_it() {
    let data = Scratch::new("settings");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut stubborn = fake("fake_b", "Fake B", "Win+Alt+F2", &log);
    stubborn.takes_settings_live = false;
    let mut set = plugin_set(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log), stubborn],
        &keyboard,
        &data,
    );
    set.enable(0);
    set.enable(1);
    read(&log);
    set.set_setting("fake_a", "level", serde_json::json!(2));
    assert_eq!(read(&log), [r#"settings fake_a {"level":2}"#]);
    set.set_setting("fake_b", "/monitors/LG", serde_json::json!(true));
    assert_eq!(
        read(&log),
        [
            r#"settings fake_b {"level":1,"monitors":{"LG":true}}"#,
            "teardown fake_b",
            "init fake_b level 1",
        ]
    );
    assert_eq!(data.saved("fake_b").settings["monitors"]["LG"], true);
    // A plugin that is off is not asked.
    set.disable(0);
    read(&log);
    set.set_setting("fake_a", "level", serde_json::json!(3));
    assert!(read(&log).is_empty());
    assert_eq!(data.saved("fake_a").settings["level"], 3);
}

#[test]
fn a_reset_deletes_the_file_of_a_plugin_that_is_off_and_reads_its_defaults() {
    let data = Scratch::new("reset");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut set = plugin_set(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    set.set_setting("fake_a", "level", serde_json::json!(5));
    set.set_hotkey("fake_a", "go", "Win+Alt+F6");
    assert!(set.reset("fake_a"));
    let saved = data.saved("fake_a");
    assert_eq!(saved.settings["level"], 1);
    assert_eq!(saved.hotkeys["go"], "Win+Alt+F1");
    assert!(read(&log).is_empty(), "it stays off");
    assert!(!set.reset("nobody"), "no such plugin");
    assert!(!data.has_file("nobody"));
}

#[test]
fn a_reset_of_a_running_plugin_restarts_it() {
    let data = Scratch::new("reset-on");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut set = plugin_set(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    set.enable(0);
    set.set_setting("fake_a", "level", serde_json::json!(5));
    read(&log);
    assert!(set.reset("fake_a"));
    assert!(set.slots[0].enabled && data.saved("fake_a").enabled);
    assert_eq!(read(&log)[0], "teardown fake_a");
}

#[test]
fn a_reset_gives_a_running_plugin_its_defaults_in_memory_on_disk_and_on_the_keyboard() {
    let data = Scratch::new("reset-defaults");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut set = plugin_set(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    set.enable(0);
    set.set_setting("fake_a", "level", serde_json::json!(5));
    set.set_hotkey("fake_a", "go", "Win+Alt+F6");
    assert_eq!(keyboard.held(), ["Win+Alt+F6"]);
    read(&log);

    assert!(set.reset("fake_a"));

    assert_eq!(
        read(&log),
        ["teardown fake_a", "init fake_a level 1"],
        "the plugin starts again from the defaults, not from level 5"
    );
    assert_eq!(set.settings_of("fake_a")["level"], 1);
    assert_eq!(set.config("fake_a").unwrap().hotkeys["go"], "Win+Alt+F1");
    let saved = data.saved("fake_a");
    assert!(saved.enabled);
    assert_eq!(saved.settings["level"], 1);
    assert_eq!(saved.hotkeys["go"], "Win+Alt+F1");
    assert_eq!(keyboard.held(), ["Win+Alt+F1"]);
    assert_eq!(
        set.registered(),
        [("Fake A".to_string(), "Go".to_string(), keys("Win+Alt+F1"))]
    );
}

#[test]
fn a_reset_of_a_plugin_that_is_off_leaves_it_off_with_defaults_on_disk() {
    let data = Scratch::new("reset-off-defaults");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut set = plugin_set(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    set.enable(0);
    set.set_setting("fake_a", "level", serde_json::json!(5));
    set.disable(0);
    read(&log);

    assert!(set.reset("fake_a"));

    assert!(read(&log).is_empty(), "it is not started or stopped");
    assert!(!set.slots[0].enabled);
    assert!(keyboard.held().is_empty());
    assert_eq!(set.settings_of("fake_a")["level"], 1);
    assert_eq!(data.saved("fake_a").settings["level"], 1);
    assert!(
        !data.saved("fake_a").enabled,
        "it stays off after the next start"
    );
}

#[test]
fn the_palette_lists_each_plugins_commands_dimmed_while_it_is_off() {
    let data = Scratch::new("entries");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut set = plugin_set(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    let plugins = |set: &PluginSet| -> Vec<PluginInfo> {
        vec![PluginInfo {
            id: "fake_a".to_string(),
            name: "Fake A".to_string(),
            version: String::new(),
            author: String::new(),
            description: String::new(),
            readme: String::new(),
            category: String::new(),
            icon: None,
            enabled: set.slots[0].enabled,
            config_path: PathBuf::new(),
            hotkeys: set.hotkey_infos(0),
            fields: Vec::new(),
            status: None,
            page_action: None,
        }]
    };
    let off = set.palette_entries(&plugins(&set));
    let mine: Vec<(&str, &str, bool)> = off
        .iter()
        .filter(|entry| entry.group == "Fake A")
        .map(|entry| (entry.label.as_str(), entry.hint.as_str(), entry.disabled))
        .collect();
    // The tray's "Go" is the hotkey's "Go" already, so it is listed once.
    assert_eq!(
        mine,
        [
            ("Turn Fake A on", "", false),
            ("Go", "Win+Alt+F1", true),
            ("Open", "", true),
            ("Tidy up", "tidy", true),
        ]
    );
    assert_eq!(
        off.iter().filter(|entry| entry.group == "WinCraft").count(),
        13,
        "nine commands and the detailed log's four"
    );
    set.enable(0);
    let on = set.palette_entries(&plugins(&set));
    let toggle = on.iter().find(|entry| entry.group == "Fake A").unwrap();
    assert_eq!(toggle.label, "Turn Fake A off");
    assert_eq!(toggle.id, CommandId::Host(HostCommand::TogglePlugin(0)));
    assert!(on
        .iter()
        .filter(|entry| entry.group == "Fake A")
        .all(|entry| !entry.disabled));
}

#[test]
fn the_palette_has_one_update_command_whatever_is_known() {
    let data = Scratch::new("update-entries");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let set = plugin_set(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    let updates: Vec<PaletteEntry> = set
        .palette_entries(&[])
        .into_iter()
        .filter(|entry| entry.group == "WinCraft" && entry.label.contains("pdate"))
        .collect();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].label, "Update WinCraft");
    assert_eq!(updates[0].id, CommandId::Host(HostCommand::UpdateWinCraft));
    assert_eq!(
        updates[0].subtitle.as_deref(),
        Some("Checks now and installs a newer version at once")
    );
}

#[test]
fn a_known_release_badges_the_update_entry_unless_updates_are_blocked() {
    let data = Scratch::new("update-badge");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let set = plugin_set(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    let update_of = |view: &UpdateView| {
        let mut entries = set.palette_entries(&[]);
        announce_update(&mut entries, view);
        entries
            .into_iter()
            .find(|entry| entry.id == CommandId::Host(HostCommand::UpdateWinCraft))
            .unwrap()
    };
    let plain = update_of(&UpdateView::default());
    assert!(plain.badge.is_none() && plain.quiet);

    let newer = UpdateView {
        available: Some(NewRelease {
            tag: "v0.20.20".to_string(),
            notes: String::new(),
            page: String::new(),
        }),
        ..UpdateView::default()
    };
    let badged = update_of(&newer);
    assert_eq!(badged.badge.as_deref(), Some("0.20.20 available"));
    assert_eq!(
        badged.subtitle.as_deref(),
        Some("Installs WinCraft 0.20.20 now")
    );
    assert!(
        badged.quiet,
        "found by typing; the palette pins its own row under an empty box"
    );

    let blocked = update_of(&UpdateView {
        blocked: Some("a development build".to_string()),
        ..newer
    });
    assert_eq!(blocked.badge, plain.badge);
    assert_eq!(blocked.subtitle, plain.subtitle);
    assert!(blocked.quiet);
}

#[test]
fn update_wincraft_waits_for_a_running_update_and_says_why_it_cannot() {
    let data = Scratch::new("update-commands");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (mut host, ui) = test_host(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    host.updates.newer = Some(release("v0.17.9"));
    host.publish();
    let listed = snapshots(&ui).pop().unwrap().commands;
    assert!(listed.iter().any(|entry| entry.label == "Update WinCraft"));
    assert!(!listed
        .iter()
        .any(|entry| entry.label.starts_with("Update to")));

    // Busy: no second download and no second check.
    host.updates.busy = true;
    host.handle_request(HostRequest::RunCommand(CommandId::Host(
        HostCommand::UpdateWinCraft,
    )));
    assert!(!host.updates.checking && !host.updates.auto_update);
    assert_eq!(host.updates.status, UpdateStatus::Idle);

    // A copy that cannot update itself checks nothing.
    host.updates.busy = false;
    host.updates.blocked = Some("a development build".to_string());
    host.handle_request(HostRequest::RunCommand(CommandId::Host(
        HostCommand::UpdateWinCraft,
    )));
    assert!(!host.updates.checking && !host.updates.auto_update && !host.updates.report);
}

#[test]
fn cancel_stops_only_a_download_and_keeps_the_old_version() {
    let data = Scratch::new("update-cancel");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (mut host, _ui) = test_host(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    // Nothing is downloading: Cancel does nothing.
    host.handle_request(HostRequest::CancelUpdate);
    assert!(!host.updates.cancel.load(Ordering::Relaxed));

    // The restart is under way: too late to cancel.
    host.updates.busy = true;
    host.updates.status = UpdateStatus::Installing;
    host.handle_request(HostRequest::CancelUpdate);
    assert!(!host.updates.cancel.load(Ordering::Relaxed));

    // A download: the flag is set, the download thread stops at its
    // next block.
    host.updates.status = UpdateStatus::Downloading(Some(40));
    host.handle_request(HostRequest::CancelUpdate);
    assert!(host.updates.cancel.load(Ordering::Relaxed));

    // The download stopped by itself: no error, the release stays so
    // Update now is offered again.
    host.updates.newer = Some(release("v0.17.0"));
    host.downloaded(Err(update::CANCELLED.to_string()));
    assert_eq!(host.updates.status, UpdateStatus::Idle);
    assert!(!host.updates.busy && !host.updates.cancel.load(Ordering::Relaxed));
    assert!(host.updates.newer.is_some());
}

#[test]
fn a_cancel_that_comes_after_the_last_byte_still_deletes_the_file() {
    let data = Scratch::new("update-cancel-late");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (mut host, _ui) = test_host(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    let file = std::env::temp_dir().join(format!("wincraft-cancel-late-{}", std::process::id()));
    std::fs::write(&file, b"partly").unwrap();
    host.updates.busy = true;
    host.updates.status = UpdateStatus::Downloading(Some(100));
    host.updates.cancel.store(true, Ordering::Relaxed);
    host.downloaded(Ok(file.clone()));
    assert!(!file.exists(), "the downloaded file is deleted");
    assert_eq!(host.updates.status, UpdateStatus::Idle);
    assert!(!host.updates.busy);
}

#[test]
fn an_update_that_does_not_happen_leaves_nothing_to_reopen() {
    let data = Scratch::new("update-reopen");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (mut host, _ui) = test_host(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    // About's Update now on a copy that cannot update itself.
    host.updates.blocked = Some("a development build".to_string());
    host.handle_request(HostRequest::UpdateNow);
    assert_eq!(host.updates.reopen, None);

    // A check that finds nothing newer forgets where it was asked from.
    host.updates.blocked = None;
    host.updates.reopen = Some(update::Reopen::Palette);
    let version = env!("CARGO_PKG_VERSION");
    host.checked(Ok(Checked::UpToDate(format!("v{version}"))));
    assert_eq!(host.updates.reopen, None);

    // So does a download that fails.
    host.updates.reopen = Some(update::Reopen::About);
    host.downloaded(Err("offline".to_string()));
    assert_eq!(host.updates.reopen, None);
}

#[test]
fn update_wincraft_says_every_answer_and_installs_a_newer_one() {
    let asked = || Updates {
        manual: true,
        auto_update: true,
        report: true,
        ..Updates::default()
    };
    let mut config = UpdateConfig::default();
    let version = env!("CARGO_PKG_VERSION");

    let mut updates = asked();
    let after = updates.checked(
        Ok(Checked::UpToDate(format!("v{version}"))),
        &mut config,
        100,
    );
    assert_eq!(
        after.notice,
        Some(format!("WinCraft is up to date ({version})"))
    );
    assert!(!after.update_now);

    let mut updates = asked();
    let after = updates.checked(Ok(Checked::Newer(release("v9.9.9"))), &mut config, 100);
    assert!(after.update_now, "installed at once");
    assert_eq!(
        after.notice.as_deref(),
        Some("Updating WinCraft to 9.9.9. It restarts when the new version is in place.")
    );

    let mut updates = asked();
    let after = updates.checked(Err("no network".to_string()), &mut config, 100);
    assert_eq!(
        after.notice.as_deref(),
        Some("WinCraft could not check for updates. Is the PC online?")
    );

    let mut updates = asked();
    let after = updates.checked(Ok(Checked::NoRelease), &mut config, 100);
    assert_eq!(
        after.notice.as_deref(),
        Some("No WinCraft release is published yet")
    );
    assert!(!updates.report, "said once");

    // A daily check stays quiet when nothing is new.
    let mut daily = Updates::default();
    let after = daily.checked(
        Ok(Checked::UpToDate(format!("v{version}"))),
        &mut config,
        200,
    );
    assert_eq!(after.notice, None);
}

fn release(tag: &str) -> Release {
    Release {
        tag: tag.to_string(),
        notes: String::new(),
        page: String::new(),
        assets: Vec::new(),
    }
}

#[test]
fn a_failed_update_check_says_offline_only_when_asked_for() {
    let mut updates = Updates::default();
    let mut config = UpdateConfig::default();
    updates.checking = true;
    let after = updates.checked(Err("no network".to_string()), &mut config, 100);
    assert_eq!(after, AfterCheck::default());
    assert_eq!(updates.status, UpdateStatus::Idle);
    assert!(!updates.checking);
    assert_eq!(config.last_check, 0, "nothing answered");

    updates.manual = true;
    updates.auto_update = true;
    updates.checked(Err("no network".to_string()), &mut config, 100);
    assert_eq!(updates.status, UpdateStatus::Offline);
    assert!(!updates.manual && !updates.auto_update);
}

#[test]
fn an_answered_check_records_what_github_said() {
    let mut updates = Updates::default();
    let mut config = UpdateConfig::default();
    let after = updates.checked(Ok(Checked::NoRelease), &mut config, 100);
    assert!(after.answered && after.notice.is_none() && !after.reload_notes);
    assert_eq!(updates.status, UpdateStatus::NoRelease);
    assert_eq!(config.last_check, 100);

    updates.newer = Some(release("v9.9.9"));
    let after = updates.checked(
        Ok(Checked::UpToDate("v0.16.6".to_string())),
        &mut config,
        200,
    );
    assert!(after.answered && after.reload_notes);
    assert_eq!(updates.status, UpdateStatus::UpToDate);
    assert!(updates.newer.is_none());
    assert_eq!(
        (config.last_check, config.latest.as_str()),
        (200, "v0.16.6")
    );
}

#[test]
fn a_newer_release_is_announced_once_and_installed_only_after_update_now() {
    let mut updates = Updates::default();
    let mut config = UpdateConfig::default();
    let after = updates.checked(Ok(Checked::Newer(release("v0.17.0"))), &mut config, 100);
    assert_eq!(
        after.notice.as_deref(),
        Some("WinCraft 0.17.0 is available \u{2014} open About to update")
    );
    assert!(!after.update_now);
    assert_eq!(config.latest, "v0.17.0");
    assert_eq!(
        updates.newer.as_ref().map(|r| r.tag.as_str()),
        Some("v0.17.0")
    );

    updates.auto_update = true;
    let after = updates.checked(Ok(Checked::Newer(release("v0.17.0"))), &mut config, 200);
    assert_eq!(after.notice, None, "the same tag is not announced again");
    assert!(after.update_now && !updates.auto_update);
}

#[test]
fn the_healthy_report_goes_once_and_tidies_only_a_copy_that_updates_itself() {
    let mut updates = Updates::default();
    assert_eq!(updates.healthy_report(false), None);
    updates.healthy_pending = Some("0.16.5".to_string());
    assert_eq!(
        updates.healthy_report(false),
        Some(("0.16.5".to_string(), true))
    );
    assert_eq!(updates.healthy_report(false), None, "told once");
    updates.healthy_pending = Some("0.16.5".to_string());
    assert_eq!(
        updates.healthy_report(true),
        Some(("0.16.5".to_string(), false))
    );
    updates.healthy_pending = Some("0.16.5".to_string());
    updates.blocked = Some("a development build".to_string());
    assert_eq!(
        updates.healthy_report(false),
        Some(("0.16.5".to_string(), false))
    );
}

/// A host with no window and no tray icon. What it sends the UI waits
/// in the returned receiver.
fn test_host(
    plugins: Vec<FakePlugin>,
    keyboard: &FakeKeys,
    data: &Scratch,
) -> (Host, Receiver<UiCommand>) {
    let bridge = ui_bridge::create();
    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let mut host = Host {
        hwnd: std::ptr::null_mut(),
        config: Config::default(),
        plugins: plugin_set(plugins, keyboard, data),
        tray: Tray::new(std::ptr::null_mut(), hinstance),
        to_ui: bridge.to_ui,
        host_rx: bridge.host_rx,
        palette_hwnd: std::ptr::null_mut(),
        balloon_click: BalloonClick::Nothing,
        arrange_focus: None,
        peek_card: None,
        arrange_anchor: None,
        digits: taskbar_number::Digits::default(),
        buttons: Vec::new(),
        buttons_at: None,
        buttons_cache: taskbar_cache::Cache::default(),
        buttons_coming: None,
        snapshot_coming: None,
        prepared: prepared::Prepared::default(),
        clock: std::time::Instant::now(),
        wait_hint: None,
        reveal: taskbar_number::Reveal::default(),
        reveal_line: String::new(),
        updates: Updates::default(),
        update_notice: None,
        autostart_note: None,
        shortcut_keys: Vec::new(),
        chord_wait: None,
        win_digit_opened: None,
        deck_app: None,
    };
    for index in host.plugins.wanted() {
        host.enable_slot(index);
    }
    (host, bridge.ui_rx)
}

fn snapshots(ui: &Receiver<UiCommand>) -> Vec<UiSnapshot> {
    ui.try_iter()
        .filter_map(|command| match command {
            UiCommand::Snapshot(snapshot) => Some(*snapshot),
            _ => None,
        })
        .collect()
}

#[test]
fn the_settings_window_switches_plugins_and_gets_a_fresh_snapshot() {
    let data = Scratch::new("request-enable");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (mut host, ui) = test_host(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    host.handle_request(HostRequest::SetPluginEnabled {
        id: "fake_a".to_string(),
        enabled: false,
    });
    let sent = snapshots(&ui);
    assert_eq!(sent.len(), 1);
    assert!(!sent[0].plugins[0].enabled);
    assert!(sent[0]
        .commands
        .iter()
        .any(|entry| entry.label == "Turn Fake A on"));
    assert!(!data.saved("fake_a").enabled);

    host.handle_request(HostRequest::SetPluginEnabled {
        id: "nobody".to_string(),
        enabled: true,
    });
    assert!(snapshots(&ui).is_empty(), "nothing changed");
    host.handle_request(HostRequest::RunCommand(CommandId::Host(
        HostCommand::TogglePlugin(0),
    )));
    assert!(host.plugins.slots[0].enabled);
    assert_eq!(snapshots(&ui).len(), 1);
}

#[test]
fn the_tray_menu_shows_the_first_action_and_the_ticked_toggles() {
    let data = Scratch::new("tray-toggles");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (host, _ui) = test_host(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    let entries: Vec<(u32, String, bool)> = host
        .menu_items()
        .into_iter()
        .filter_map(|item| match item {
            MenuItem::Entry { id, label, checked } => Some((id, label, checked)),
            MenuItem::Separator => None,
        })
        .collect();
    let base = MENU_PLUGIN_ACTION_BASE;
    assert!(entries.contains(&(base + 1, "Go".to_string(), false)));
    assert!(entries.contains(&(base + 5, "Feature".to_string(), true)));
    assert!(
        !entries.iter().any(|(_, label, _)| label == "Open"),
        "only the first tray action"
    );
}

#[test]
fn a_plugin_can_open_the_strip_on_a_button_close_it_and_save_a_setting() {
    open_arrange_at("Chrome.UserData.Profile3".to_string(), [1, 2, 3, 4]);
    close_arrange();
    save_setting("fake_a", "level", serde_json::json!(4));
    let requests = PluginRequests::take();
    assert!(requests.arrange);
    assert_eq!(
        requests.anchor,
        Some(("Chrome.UserData.Profile3".to_string(), [1, 2, 3, 4]))
    );
    assert!(requests.close);
    assert_eq!(
        requests.settings,
        [(
            "fake_a".to_string(),
            "level".to_string(),
            serde_json::json!(4)
        )]
    );
    let again = PluginRequests::take();
    assert!(!again.arrange && !again.close && again.anchor.is_none());
    assert!(again.settings.is_empty());
}

#[test]
fn requests_reach_a_plugin_only_while_it_is_on() {
    let data = Scratch::new("request-off");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (mut host, ui) = test_host(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    read(&log);
    let ask = || HostRequest::PromptAnswer {
        plugin: "fake_a".to_string(),
        id: 3,
        target: 0,
        text: "yes".to_string(),
    };
    let run = || {
        HostRequest::RunCommand(CommandId::Plugin {
            index: 0,
            kind: ActionKind::Hotkey,
            action: 1,
        })
    };
    let layout = || HostRequest::Layout {
        plugin: "fake_a".to_string(),
        action: LayoutAction::SaveNow,
    };
    host.handle_request(ask());
    host.handle_request(run());
    host.handle_request(layout());
    host.handle_request(HostRequest::RunCommand(CommandId::Plugin {
        index: 0,
        kind: ActionKind::Tray,
        action: 2,
    }));
    assert_eq!(
        read(&log),
        [
            "answer fake_a 3 yes",
            "hotkey fake_a 1",
            "layout fake_a",
            "tray fake_a 2"
        ]
    );
    let layouts = ui
        .try_iter()
        .filter(|command| matches!(command, UiCommand::LayoutUpdate(None)))
        .count();
    assert_eq!(layouts, 1, "the Layout page hears there is no layout");

    host.plugins.disable(0);
    read(&log);
    host.handle_request(ask());
    host.handle_request(run());
    host.handle_request(layout());
    host.handle_request(HostRequest::Arrange {
        plugin: "fake_a".to_string(),
        action: ArrangeAction::Activate(0),
    });
    assert!(read(&log).is_empty());
    host.handle_request(HostRequest::RunCommand(CommandId::Plugin {
        index: 9,
        kind: ActionKind::Hotkey,
        action: 1,
    }));
}

#[test]
fn a_rename_from_the_strip_reaches_windownamer_only_while_it_is_on() {
    let data = Scratch::new("request-rename");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (mut host, _ui) = test_host(
        vec![
            fake("fake_a", "Fake A", "Win+Alt+F1", &log),
            fake("window_namer", "Namer", "Win+Alt+F2", &log),
        ],
        &keyboard,
        &data,
    );
    read(&log);
    let rename = || HostRequest::Arrange {
        plugin: "fake_a".to_string(),
        action: ArrangeAction::Rename {
            hwnd: 5,
            name: Some("Work".to_string()),
        },
    };
    host.handle_request(rename());
    assert_eq!(
        read(&log),
        ["arrange window_namer"],
        "not the strip's plugin"
    );
    host.plugins.disable(1);
    read(&log);
    host.handle_request(rename());
    assert!(read(&log).is_empty(), "WindowNamer is off");
}

#[test]
fn a_test_instance_renames_no_window_from_the_strip() {
    let rename = ArrangeAction::Rename {
        hwnd: 5,
        name: None,
    };
    assert!(skipped_in_test_instance(&rename, true));
    assert!(!skipped_in_test_instance(&rename, false));
    assert!(!skipped_in_test_instance(&ArrangeAction::Close(5), true));
}

#[test]
fn settings_requests_save_and_publish() {
    let data = Scratch::new("request-settings");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (mut host, ui) = test_host(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    host.handle_request(HostRequest::SetHotkey {
        plugin: "fake_a".to_string(),
        action: "go".to_string(),
        binding: "Win+Alt+F2".to_string(),
    });
    host.handle_request(HostRequest::SetSetting {
        plugin: "fake_a".to_string(),
        key: "level".to_string(),
        value: serde_json::json!(4),
    });
    host.handle_request(HostRequest::ResetPlugin("fake_a".to_string()));
    assert_eq!(snapshots(&ui).len(), 3);

    host.handle_request(HostRequest::SetShortcuts(vec![ShortcutChange {
        id: "wincraft.palette".to_string(),
        binding: Some("win+alt+f9".to_string()),
    }]));
    assert_eq!(host.config.palette_hotkey, "Win+Alt+F9");
    assert!(keyboard.held().contains(&"Win+Alt+F9".to_string()));
    host.handle_request(HostRequest::SetHostSetting(HostSetting::CheckUpdatesDaily(
        false,
    )));
    let saved = Config::load_in(&data.0);
    assert_eq!(saved.palette_hotkey, "Win+Alt+F9");
    assert!(!saved.updates.check_daily);
    host.handle_request(HostRequest::ClearPaletteUsage);
    assert!(ui
        .try_iter()
        .any(|command| matches!(command, UiCommand::ClearPaletteUsage)));
}

fn change(id: &str, binding: Option<&str>) -> ShortcutChange {
    ShortcutChange {
        id: id.to_string(),
        binding: binding.map(str::to_string),
    }
}

fn row<'a>(snapshot: &'a UiSnapshot, id: &str) -> &'a ShortcutInfo {
    snapshot
        .shortcuts
        .iter()
        .find(|info| info.id == id)
        .unwrap_or_else(|| panic!("no row {id}"))
}

#[test]
fn every_palette_command_is_a_shortcut_row_with_a_stable_id() {
    let data = Scratch::new("shortcut-rows");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (host, _ui) = test_host(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    let snapshot = host.snapshot();
    let ids: Vec<&str> = snapshot
        .shortcuts
        .iter()
        .map(|info| info.id.as_str())
        .collect();
    for id in [
        "wincraft.palette",
        "wincraft.open-settings",
        "wincraft.quit",
        "fake_a.turn-on-off",
        "fake_a.go",
        "fake_a.tray-2",
        "fake_a.command-7",
    ] {
        assert!(ids.contains(&id), "{id} in {ids:?}");
    }
    let go = row(&snapshot, "fake_a.go");
    assert_eq!(
        (go.binding.as_str(), go.default_binding.as_str()),
        ("Win+Alt+F1", "Win+Alt+F1")
    );
    assert_eq!(
        row(&snapshot, "wincraft.palette").binding,
        DEFAULT_PALETTE_HOTKEY
    );
    assert!(row(&snapshot, "wincraft.palette").one_press);
    assert_eq!(row(&snapshot, "fake_a.command-7").binding, "");
    let entry = snapshot
        .commands
        .iter()
        .find(|entry| entry.shortcut_id == "fake_a.go")
        .unwrap();
    assert_eq!(entry.shortcut, "Win+Alt+F1");
}

#[test]
fn shortcuts_are_saved_as_overrides_and_reset_takes_them_away() {
    let data = Scratch::new("shortcut-overrides");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (mut host, ui) = test_host(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );

    host.handle_request(HostRequest::SetShortcuts(vec![change(
        "fake_a.command-7",
        Some("win+alt+k, t"),
    )]));
    assert_eq!(host.config.shortcuts["fake_a.command-7"], "Win+Alt+K, T");
    assert_eq!(
        Config::load_in(&data.0).shortcuts["fake_a.command-7"],
        "Win+Alt+K, T"
    );
    assert!(keyboard.held().contains(&"Win+Alt+K".to_string()));
    let last = snapshots(&ui).pop().unwrap();
    assert_eq!(row(&last, "fake_a.command-7").binding, "Win+Alt+K, T");

    // Removing a plugin's hotkey writes the empty text and frees the keys.
    host.handle_request(HostRequest::SetShortcuts(vec![change(
        "fake_a.go",
        Some(""),
    )]));
    assert_eq!(data.saved("fake_a").hotkeys["go"], "");
    assert!(!keyboard.held().contains(&"Win+Alt+F1".to_string()));
    assert_eq!(row(&host.snapshot(), "fake_a.go").binding, "");

    // Reset gives the default back, and takes an override away.
    host.handle_request(HostRequest::SetShortcuts(vec![
        change("fake_a.go", None),
        change("fake_a.command-7", None),
    ]));
    assert_eq!(data.saved("fake_a").hotkeys["go"], "Win+Alt+F1");
    assert!(keyboard.held().contains(&"Win+Alt+F1".to_string()));
    assert!(!host.config.shortcuts.contains_key("fake_a.command-7"));
    assert!(!keyboard.held().contains(&"Win+Alt+K".to_string()));
    assert!(Config::load_in(&data.0).shortcuts.is_empty());
}

#[test]
fn replace_moves_a_plugin_hotkey_to_another_command() {
    let data = Scratch::new("shortcut-replace");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (mut host, _ui) = test_host(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    let keys = hotkeys::parse_shortcut("Win+Alt+F1").unwrap();
    host.handle_request(HostRequest::SetShortcuts(shortcuts::replace(
        "wincraft.quit",
        &keys,
        "fake_a.go",
    )));
    let snapshot = host.snapshot();
    assert_eq!(row(&snapshot, "fake_a.go").binding, "");
    assert!(shortcuts::is_changed(row(&snapshot, "fake_a.go")));
    assert_eq!(row(&snapshot, "wincraft.quit").binding, "Win+Alt+F1");
    assert!(
        row(&snapshot, "wincraft.quit").registered,
        "the keys were free again"
    );
    assert_eq!(host.shortcut_keys.len(), 1);
    assert!(host.shortcut_keys[0].2);
}

#[test]
fn a_plugin_chord_waits_for_its_second_key_and_runs_the_hotkey() {
    let data = Scratch::new("shortcut-chord");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (mut host, _ui) = test_host(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    host.handle_request(HostRequest::SetShortcuts(vec![change(
        "fake_a.go",
        Some("Win+Alt+K, G"),
    )]));
    assert_eq!(keyboard.held(), ["Win+Alt+K"]);
    let press = host.shortcut_keys[0].1.clone();
    assert_eq!(press.seconds.len(), 1);
    read(&log);

    // What the hook would have taken: a key that is not the second
    // press is never taken, so only the matching one comes here.
    host.chord_wait = Some(press.clone());
    host.wait_news(chord::News {
        taken: vec![chord::Stroke {
            vk: 0x47,
            modifiers: 0,
        }],
        ended: false,
    });
    assert_eq!(read(&log), ["hotkey fake_a 1"]);
    assert!(host.chord_wait.is_none());

    host.chord_wait = Some(press.clone());
    host.wait_news(chord::News {
        taken: vec![chord::Stroke {
            vk: 0x1B,
            modifiers: 0,
        }],
        ended: false,
    });
    assert!(read(&log).is_empty(), "Esc runs nothing");
    assert!(host.chord_wait.is_none());

    host.chord_wait = Some(press);
    host.wait_news(chord::News {
        taken: Vec::new(),
        ended: true,
    });
    assert!(host.chord_wait.is_none(), "another key or the time ends it");

    // The plugin's page shows the chord as its binding.
    assert_eq!(host.plugins.hotkey_infos(0)[0].binding, "Win+Alt+K, G");
    // Turning the plugin off frees the chord's first press.
    host.plugins.disable(0);
    host.publish();
    assert!(keyboard.held().iter().all(|keys| keys != "Win+Alt+K"));
}

#[test]
fn an_old_config_without_shortcuts_keeps_every_default_hotkey() {
    let data = Scratch::new("shortcut-old");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (host, _ui) = test_host(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    assert_eq!(keyboard.held(), ["Win+Alt+F1"]);
    assert!(host.shortcut_keys.is_empty());
    assert!(host.config.shortcuts.is_empty());
}

#[test]
fn a_failed_start_from_the_settings_window_warns_with_a_balloon_that_opens_the_detector() {
    let data = Scratch::new("request-crash");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut broken = fake("broken", "Broken", "Win+Alt+F1", &log);
    broken.fails_with = Some("no device");
    let (host, _ui) = test_host(vec![broken], &keyboard, &data);
    assert_eq!(host.balloon_click, BalloonClick::Detector);
    assert!(host.plugins.warnings.is_empty(), "the balloon was shown");
    assert!(!host.plugins.slots[0].enabled);
}

#[test]
fn the_strip_opens_only_while_the_plugin_that_decides_its_look_is_on() {
    let data = Scratch::new("strip-look");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut keeper = fake("fake_keeper", "Keeper", "Win+Alt+F1", &log);
    keeper.groups = true;
    let mut deck = fake("fake_deck", "Deck", "Win+Alt+F2", &log);
    deck.look = true;
    let (mut host, ui) = test_host(vec![keeper, deck], &keyboard, &data);
    let shown = |ui: &Receiver<UiCommand>| {
        ui.try_iter()
            .filter_map(|command| match command {
                UiCommand::ShowArrange(snapshot) => Some(snapshot),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert!(host.send_arrange_for(true, false));
    let sent = shown(&ui);
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].plugin, "fake_keeper",
        "the windows come from the keeper"
    );
    assert_eq!(
        sent[0].look_plugin, "fake_deck",
        "a new view is saved there"
    );
    assert!(!sent[0].preview);
    assert_eq!(sent[0].view, ArrangeView::Flat);

    host.handle_request(HostRequest::SetPluginEnabled {
        id: "fake_deck".to_string(),
        enabled: false,
    });
    assert!(!host.send_arrange_for(true, false), "the deck is off");
    assert!(shown(&ui).is_empty());
}

#[test]
fn the_keeper_gives_the_strip_its_windows_and_the_deck_lists_them_without_it() {
    let data = Scratch::new("strip-source");
    let (log, keyboard) = (diary(), FakeKeys::default());
    // The deck comes first, so the keeper does not win by its place.
    let mut deck = fake("fake_deck", "Deck", "Win+Alt+F2", &log);
    deck.look = true;
    deck.groups = true;
    let mut keeper = fake("fake_keeper", "Keeper", "Win+Alt+F1", &log);
    keeper.groups = true;
    let (mut host, ui) = test_host(vec![deck, keeper], &keyboard, &data);
    let sent_by = |ui: &Receiver<UiCommand>| {
        ui.try_iter()
            .filter_map(|command| match command {
                UiCommand::ShowArrange(snapshot) => Some((snapshot.plugin, snapshot.keeps_order)),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert!(host.send_arrange_for(true, false));
    assert_eq!(
        sent_by(&ui),
        [("fake_keeper".to_string(), true)],
        "a saved order: the note on other desktops shows"
    );

    host.handle_request(HostRequest::SetPluginEnabled {
        id: "fake_keeper".to_string(),
        enabled: false,
    });
    assert!(host.send_arrange_for(true, false), "the deck lists them");
    assert_eq!(sent_by(&ui), [("fake_deck".to_string(), false)]);
    // The strip's requests go to the plugin that listed the windows.
    host.arrange_action("fake_deck", &ArrangeAction::Close(0x7fff_fff0));
    assert!(log.borrow().iter().any(|line| line == "arrange fake_deck"));
}

#[test]
fn with_no_plugin_to_list_windows_the_strip_stays_shut_without_a_balloon() {
    let data = Scratch::new("strip-none");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut deck = fake("fake_deck", "Deck", "Win+Alt+F2", &log);
    deck.look = true;
    let (mut host, ui) = test_host(vec![deck], &keyboard, &data);
    let _ = PluginRequests::take();
    assert!(!host.send_arrange_for(true, false));
    assert!(ui
        .try_iter()
        .all(|command| !matches!(command, UiCommand::ShowArrange(_))));
    assert!(PluginRequests::take().notices.is_empty());
}

#[test]
fn win_number_opens_the_strip_for_its_button_or_gives_the_keys_back() {
    let data = Scratch::new("win-number");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut keeper = fake("fake_keeper", "Keeper", "Win+Alt+F1", &log);
    keeper.groups = true;
    let mut deck = fake("fake_deck", "Deck", "Win+Alt+F2", &log);
    deck.look = true;
    let (mut host, ui) = test_host(vec![keeper, deck], &keyboard, &data);
    let button = |app: &str, left: i32| taskbar::Button {
        app: app.to_string(),
        rect: [left, 1040, left + 48, 1088],
    };
    // Read moments ago, so no test reads the real taskbar.
    host.buttons = vec![
        button("Microsoft.WindowsTerminal", 0),
        button("Chrome.UserData.Profile3", 48),
    ];
    host.buttons_at = Some(std::time::Instant::now());
    let opened = |ui: &Receiver<UiCommand>| {
        ui.try_iter()
            .filter_map(|command| match command {
                UiCommand::ShowArrange(_) => Some(0),
                UiCommand::ArrangeSelect(place) => Some(place),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    // Win+2: Chrome has a window, even only one, so the strip opens on it.
    assert_eq!(host.win_digit(2, false), WinDigit::Opened);
    assert_eq!(opened(&ui), [0, 1]);
    // The same digit again before the UI has the strip's window: the
    // next card, not a second strip.
    assert_eq!(host.win_digit(2, true), WinDigit::NextCard);
    assert!(opened(&ui).is_empty());
    // Win let go and Win+2 again while the strip is open on Chrome: the
    // next card too, not a strip built again.
    assert_eq!(host.win_digit(2, false), WinDigit::NextCard);
    assert!(opened(&ui).is_empty());
    // Open on another app: Win+2 moves the open strip to Chrome.
    host.deck_app = Some("Microsoft.WindowsTerminal".to_string());
    assert_eq!(host.win_digit(2, false), WinDigit::Opened);
    assert_eq!(opened(&ui), [0, 1]);
    assert_eq!(host.deck_app.as_deref(), Some("Chrome.UserData.Profile3"));
    // Win let go alone with no strip on screen does nothing.
    host.win_released(true);
    assert!(ui
        .try_iter()
        .all(|command| !matches!(command, UiCommand::HideArrange)));
    // Win+1: a pinned app that is not running goes back to Windows.
    assert_eq!(host.win_digit(1, false), WinDigit::GiveBack);
    assert!(opened(&ui).is_empty());
    assert!(host.arrange_anchor.is_none());
    // Win+5: no such button.
    assert_eq!(host.win_digit(5, false), WinDigit::GiveBack);
    // With WindowDeck off nothing opens, and Windows keeps the keys.
    host.handle_request(HostRequest::SetPluginEnabled {
        id: "fake_deck".to_string(),
        enabled: false,
    });
    assert_eq!(host.win_digit(2, false), WinDigit::GiveBack);
    assert!(opened(&ui).is_empty());
}

#[test]
fn a_win_number_posted_from_another_thread_reaches_the_queue() {
    let hwnd = 0x7fff_fff4 as HWND;
    unsafe { wnd_proc(hwnd, WM_APP_WIN_DIGIT, 4, 1) };
    assert_eq!(PluginRequests::take().win_digits, [(4, true)]);
}

#[test]
fn a_hook_queues_win_number_for_the_host() {
    open_taskbar_button(3, false);
    open_taskbar_button(3, true);
    assert_eq!(PluginRequests::take().win_digits, [(3, false), (3, true)]);
    assert!(PluginRequests::take().win_digits.is_empty());
}

#[test]
fn windowdeck_takes_the_strip_settings_over_from_layoutkeeper_once() {
    let data = Scratch::new("take-over");
    let keeper = PluginConfig {
        enabled: true,
        hotkeys: [
            ("arrange".to_string(), "Win+Alt+K, A".to_string()),
            ("go".to_string(), "Win+Alt+F1".to_string()),
        ]
        .into_iter()
        .collect(),
        settings: serde_json::json!({
            "level": 2,
            "taskbar_hover_strip": true,
            "hover_delay_seconds": 0.4,
            "arrange_view": "flat",
        }),
    };
    keeper.save_in(&data.0, "layout_keeper").unwrap();
    let log = diary();
    let keyboard = FakeKeys::default();
    let plugins = || -> Vec<Box<dyn WinCraftPlugin>> {
        vec![
            Box::new(fake("layout_keeper", "LayoutKeeper", "Win+Alt+F1", &log)),
            Box::new(crate::plugins::window_deck::WindowDeck::new()),
        ]
    };
    let set = boxed_set(plugins(), &keyboard, &data);
    let deck = data.saved("window_deck");
    assert!(deck.enabled);
    assert_eq!(deck.hotkeys["arrange"], "Win+Alt+K, A", "the chord stays");
    assert_eq!(deck.settings["taskbar_hover_strip"], true);
    assert_eq!(deck.settings["arrange_view"], "flat");
    assert_eq!(
        deck.settings["hover_delay_seconds"], 0.15,
        "its own migration runs after the take-over"
    );
    assert_eq!(
        deck.settings["preview_on_hover"], true,
        "a default filled in"
    );
    let left = data.saved("layout_keeper");
    assert!(left.settings.get("taskbar_hover_strip").is_none());
    assert!(left.settings.get("arrange_view").is_none());
    assert!(!left.hotkeys.contains_key("arrange"));
    assert_eq!(left.settings["level"], 2);
    assert_eq!(
        set.config("layout_keeper").map(|c| c.hotkeys.len()),
        Some(1)
    );
    drop(set);

    // A later start finds WindowDeck's own file and takes nothing again.
    let mut changed = data.saved("window_deck");
    changed.settings["arrange_view"] = serde_json::json!("by_monitor");
    changed.save_in(&data.0, "window_deck").unwrap();
    let _again = boxed_set(plugins(), &keyboard, &data);
    assert_eq!(
        data.saved("window_deck").settings["arrange_view"],
        "by_monitor"
    );
}

#[test]
fn windowdeck_off_registers_no_hotkey() {
    let data = Scratch::new("deck-off");
    PluginConfig {
        enabled: false,
        ..PluginConfig::default()
    }
    .save_in(&data.0, "window_deck")
    .unwrap();
    let keyboard = FakeKeys::default();
    let mut set = boxed_set(
        vec![Box::new(crate::plugins::window_deck::WindowDeck::new())],
        &keyboard,
        &data,
    );
    assert!(set.wanted().is_empty());
    assert!(keyboard.held().is_empty());
    set.enable(0);
    assert_eq!(keyboard.held(), ["Win+Alt+A"]);
    set.disable(0);
    assert!(keyboard.held().is_empty(), "off frees Win+Alt+A");
}

#[test]
fn a_busy_host_keeps_only_the_newest_display_change() {
    let mut queue = Vec::new();
    queue_again(&mut queue, WM_DISPLAYCHANGE, 32, 1);
    queue_again(&mut queue, WM_HOTKEY, 3, 0);
    queue_again(&mut queue, WM_DISPLAYCHANGE, 32, 2);
    assert_eq!(queue, [(WM_HOTKEY, 3, 0), (WM_DISPLAYCHANGE, 32, 2)]);
    for _ in 0..RETRY_LIMIT * 2 {
        queue_again(&mut queue, WM_HOTKEY, 1, 0);
    }
    assert_eq!(queue.len(), RETRY_LIMIT);
}

#[test]
fn a_plugin_request_is_queued_once_and_survives_a_full_queue() {
    let mut queue = Vec::new();
    for _ in 0..RETRY_LIMIT * 2 {
        queue_again(&mut queue, WM_HOTKEY, 1, 0);
    }
    queue_again(&mut queue, WM_APP_PLUGIN, 0, 0);
    queue_again(&mut queue, WM_APP_PLUGIN, 0, 0);
    assert_eq!(queue.len(), RETRY_LIMIT + 1);
    assert_eq!(queue.last(), Some(&(WM_APP_PLUGIN, 0, 0)));
    let mut queue = Vec::new();
    queue_again(&mut queue, WM_APP_PLUGIN, 0, 0);
    queue_again(&mut queue, WM_HOTKEY, 3, 0);
    queue_again(&mut queue, WM_APP_PLUGIN, 0, 0);
    assert_eq!(queue, [(WM_APP_PLUGIN, 0, 0), (WM_HOTKEY, 3, 0)]);
}

/// The host stands in as a list of the notices it showed. The handler
/// takes the notices inside the closure, as WM_APP_PLUGIN does.
fn deliver_plugin_requests(host: &RefCell<Option<Vec<(String, String)>>>) -> bool {
    run_unless_busy(host, |shown| shown.extend(PluginRequests::take().notices))
}

#[test]
fn a_notice_that_finds_the_host_busy_is_shown_once_after_it_is_free() {
    let host = RefCell::new(Some(Vec::new()));
    NOTICES.with(|queue| {
        let mut queue = queue.borrow_mut();
        queue.push(("first".to_string(), "1".to_string()));
        queue.push(("second".to_string(), "2".to_string()));
    });
    let mut retry = Vec::new();

    let busy_guard = host.borrow_mut();
    assert!(deliver_plugin_requests(&host));
    queue_again(&mut retry, WM_APP_PLUGIN, 0, 0);
    assert_eq!(NOTICES.with(|queue| queue.borrow().len()), 2);
    drop(busy_guard);

    // A third notice and its message arrive while the first two wait.
    NOTICES.with(|queue| {
        queue
            .borrow_mut()
            .push(("third".to_string(), "3".to_string()))
    });
    queue_again(&mut retry, WM_APP_PLUGIN, 0, 0);
    assert_eq!(retry.len(), 1);

    for _ in std::mem::take(&mut retry) {
        assert!(!deliver_plugin_requests(&host));
    }
    // The fresh message the third notice posted finds nothing left.
    assert!(!deliver_plugin_requests(&host));
    let titles: Vec<String> = host
        .borrow()
        .as_ref()
        .unwrap()
        .iter()
        .map(|(title, _)| title.clone())
        .collect();
    assert_eq!(titles, ["first", "second", "third"]);
}

#[test]
fn a_tooltip_and_a_prompt_wait_for_a_free_host_too() {
    let host: RefCell<Option<Vec<(String, String)>>> = RefCell::new(Some(Vec::new()));
    TRAY_TIP.with(|cell| *cell.borrow_mut() = Some(Some("tip".to_string())));
    ARRANGE_WANTED.with(|cell| cell.set(true));
    let busy_guard = host.borrow_mut();
    assert!(deliver_plugin_requests(&host));
    drop(busy_guard);
    let requests = PluginRequests::take();
    assert_eq!(requests.tip, Some(Some("tip".to_string())));
    assert!(requests.arrange);
    assert!(PluginRequests::take().tip.is_none());
}

#[test]
fn no_flags_means_a_shutdown_or_restart() {
    assert_eq!(end_session_words(0), "no flags (shutdown or restart)");
}

#[test]
fn closeapp_means_an_installer_or_update_is_closing_apps() {
    let words = end_session_words(ENDSESSION_CLOSEAPP as LPARAM);
    assert!(
        words.starts_with("ENDSESSION_CLOSEAPP (an installer or update"),
        "{words}"
    );
    assert!(!words.contains("shutdown"), "{words}");
}

#[test]
fn logoff_and_critical_are_told_apart_and_can_come_together() {
    assert_eq!(
        end_session_words(ENDSESSION_LOGOFF as LPARAM),
        "ENDSESSION_LOGOFF (the user is signing out)"
    );
    let words = end_session_words((ENDSESSION_LOGOFF | ENDSESSION_CRITICAL) as LPARAM);
    assert!(
        words.contains("ENDSESSION_CRITICAL (Windows ends the session now"),
        "{words}"
    );
    assert!(
        words.ends_with("ENDSESSION_LOGOFF (the user is signing out)"),
        "{words}"
    );
}

#[test]
fn the_high_bit_reads_the_same_sign_extended_or_not() {
    let plain = end_session_words(0x8000_0000_u32 as LPARAM);
    let extended = end_session_words(0xFFFF_FFFF_8000_0000_u64 as LPARAM);
    assert_eq!(plain, extended);
    assert!(plain.contains("ENDSESSION_LOGOFF"));
}

#[test]
fn the_palette_after_an_update_waits_for_the_app_index_but_not_longer_than_the_cap() {
    use std::time::Duration;
    assert!(!palette_ready_to_reopen(false, Duration::from_millis(0)));
    assert!(!palette_ready_to_reopen(false, Duration::from_millis(1900)));
    assert!(palette_ready_to_reopen(false, Duration::from_secs(2)));
    assert!(palette_ready_to_reopen(true, Duration::from_millis(0)));
}

#[test]
fn switching_the_strip_plugin_off_frees_its_kept_window() {
    let data = Scratch::new("strip-free");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut deck = fake("fake_deck", "Deck", "Win+Alt+F2", &log);
    deck.look = true;
    let other = fake("fake_other", "Other", "Win+Alt+F3", &log);
    let (mut host, _ui) = test_host(vec![deck, other], &keyboard, &data);
    let _ = PluginRequests::take();
    host.handle_request(HostRequest::SetPluginEnabled {
        id: "fake_other".to_string(),
        enabled: false,
    });
    assert!(!PluginRequests::take().free, "a plugin without the strip");
    assert!(host.strip_plugin_on());
    host.handle_request(HostRequest::SetPluginEnabled {
        id: "fake_deck".to_string(),
        enabled: false,
    });
    assert!(PluginRequests::take().free);
    assert!(!host.strip_plugin_on());
}

#[test]
fn windows_gathered_when_win_goes_down_serve_the_next_opening() {
    let data = Scratch::new("strip-prepared");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let mut keeper = fake("fake_keeper", "Keeper", "Win+Alt+F1", &log);
    keeper.groups = true;
    let mut deck = fake("fake_deck", "Deck", "Win+Alt+F2", &log);
    deck.look = true;
    let (mut host, ui) = test_host(vec![keeper, deck], &keyboard, &data);
    let now = std::time::Instant::now;
    host.prepare_from(no_windows());
    assert!(host.prepared.is_fresh(now()));
    assert!(host.send_arrange_for(true, false));
    assert!(!host.prepared.is_fresh(now()), "taken by the opening");
    assert!(ui
        .try_iter()
        .any(|command| matches!(command, UiCommand::ShowArrange(_))));
    // A window that comes or goes throws them away.
    host.prepare_from(no_windows());
    host.buttons_changed();
    assert!(!host.prepared.is_fresh(now()));
}

fn no_windows() -> crate::core::window_snapshot::Snapshot {
    crate::core::window_snapshot::Snapshot {
        windows: Vec::new(),
        taken: std::time::Instant::now(),
    }
}

fn snapshot_host(
    data: &Scratch,
    keyboard: &FakeKeys,
) -> (Host, std::sync::mpsc::Receiver<UiCommand>) {
    let log = diary();
    let mut keeper = fake("fake_keeper", "Keeper", "Win+Alt+F1", &log);
    keeper.groups = true;
    let mut deck = fake("fake_deck", "Deck", "Win+Alt+F2", &log);
    deck.look = true;
    test_host(vec![keeper, deck], keyboard, data)
}

#[test]
fn windows_read_on_their_own_thread_are_built_into_groups_when_they_come() {
    let data = Scratch::new("strip-snapshot");
    let keyboard = FakeKeys::default();
    let (mut host, _ui) = snapshot_host(&data, &keyboard);
    let now = std::time::Instant::now;
    // Still being read: the message from an older thread changes nothing.
    let (send, receive) = std::sync::mpsc::channel();
    host.snapshot_coming = Some((receive, now()));
    host.snapshot_arrived();
    assert!(host.snapshot_coming.is_some() && !host.prepared.is_fresh(now()));
    // Read: the groups are ready for the digit.
    send.send(no_windows()).unwrap();
    host.snapshot_arrived();
    assert!(host.snapshot_coming.is_none() && host.prepared.is_fresh(now()));
    // A thread that could not read sends nothing; the strip reads itself.
    host.prepared.forget();
    let (send, receive) = std::sync::mpsc::channel();
    drop(send);
    host.snapshot_coming = Some((receive, now()));
    host.snapshot_arrived();
    assert!(host.snapshot_coming.is_none() && !host.prepared.is_fresh(now()));
}

#[test]
fn windows_read_too_long_ago_are_not_built_into_groups() {
    let data = Scratch::new("strip-snapshot-old");
    let keyboard = FakeKeys::default();
    let (mut host, _ui) = snapshot_host(&data, &keyboard);
    let mut old = no_windows();
    old.taken = std::time::Instant::now() - prepared::FRESH;
    host.prepare_from(old);
    assert!(!host.prepared.is_fresh(std::time::Instant::now()));
}

#[test]
fn a_strip_that_opens_before_the_windows_are_back_waits_for_them() {
    let data = Scratch::new("strip-snapshot-wait");
    let keyboard = FakeKeys::default();
    let (mut host, ui) = snapshot_host(&data, &keyboard);
    let (send, receive) = std::sync::mpsc::channel();
    host.snapshot_coming = Some((receive, std::time::Instant::now()));
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(20));
        let _ = send.send(no_windows());
    });
    assert!(host.send_arrange_for(true, false));
    assert!(host.snapshot_coming.is_none());
    assert!(ui
        .try_iter()
        .any(|command| matches!(command, UiCommand::ShowArrange(_))));
}

#[test]
fn a_window_that_comes_or_goes_drops_the_windows_being_read() {
    let data = Scratch::new("strip-snapshot-drop");
    let keyboard = FakeKeys::default();
    let (mut host, _ui) = snapshot_host(&data, &keyboard);
    let (send, receive) = std::sync::mpsc::channel();
    host.snapshot_coming = Some((receive, std::time::Instant::now()));
    host.buttons_changed();
    assert!(host.snapshot_coming.is_none());
    let _ = send.send(no_windows());
    host.snapshot_arrived();
    assert!(!host.prepared.is_fresh(std::time::Instant::now()));
}

#[test]
fn the_inspector_asks_a_running_plugin_about_its_command_and_gets_an_answer() {
    let data = Scratch::new("command-details");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (mut host, ui) = test_host(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    let tidy = CommandId::Plugin {
        index: 0,
        kind: ActionKind::Palette,
        action: 7,
    };
    ui.try_iter().count();
    host.handle_request(HostRequest::CommandDetails(tidy));
    let answers: Vec<_> = ui
        .try_iter()
        .filter_map(|command| match command {
            UiCommand::CommandDetails { id, details } => Some((id, details)),
            _ => None,
        })
        .collect();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].0, tidy);
    let details = answers[0].1.as_ref().expect("details");
    assert_eq!(details.status, "fake_a is untidy");
    assert!(host.confirm_of(tidy).is_none());

    host.plugins.disable(0);
    host.handle_request(HostRequest::CommandDetails(tidy));
    let off = ui.try_iter().find_map(|command| match command {
        UiCommand::CommandDetails { details, .. } => Some(details),
        _ => None,
    });
    assert_eq!(off, Some(None), "a plugin that is off says nothing");
    host.handle_request(HostRequest::CommandDetails(CommandId::Host(
        HostCommand::OpenSettings,
    )));
}

#[test]
fn a_hotkey_on_a_command_that_asks_first_opens_the_palette_on_it() {
    let data = Scratch::new("confirm-key");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (mut host, ui) = test_host(
        vec![
            fake("fake_a", "Fake A", "Win+Alt+F1", &log),
            fake("fake_confirm", "Fake C", "Win+Alt+F2", &log),
        ],
        &keyboard,
        &data,
    );
    read(&log);
    ui.try_iter().count();
    let plain = CommandId::Plugin {
        index: 0,
        kind: ActionKind::Palette,
        action: 7,
    };
    let asks = CommandId::Plugin {
        index: 1,
        kind: ActionKind::Palette,
        action: 7,
    };
    assert_eq!(
        host.confirm_of(asks),
        Some((
            "Tidy up".to_string(),
            "Press Enter again to tidy up".to_string()
        ))
    );
    host.run_from_key(plain);
    assert_eq!(read(&log), ["palette fake_a 7"]);
    host.run_from_key(asks);
    assert!(read(&log).is_empty(), "it does not run from the key");
    let sent: Vec<UiCommand> = ui.try_iter().collect();
    assert!(sent.iter().any(|c| matches!(c, UiCommand::ShowPalette(_))));
    assert!(sent
        .iter()
        .any(|c| matches!(c, UiCommand::TypeInPalette(text) if text == "!Tidy up")));
    let snapshot = host.snapshot();
    let entry = snapshot
        .commands
        .iter()
        .find(|entry| entry.id == asks)
        .expect("its row");
    assert_eq!(entry.confirm, "Press Enter again to tidy up");
    host.handle_request(HostRequest::RunCommand(asks));
    assert_eq!(
        read(&log),
        ["palette fake_confirm 7"],
        "the palette's own run goes through"
    );
}

#[test]
fn a_command_with_rows_answers_with_them_and_hears_the_pick() {
    let data = Scratch::new("sub-list");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (mut host, ui) = test_host(
        vec![fake("fake_list", "Fake L", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    read(&log);
    ui.try_iter().count();
    let tidy = CommandId::Plugin {
        index: 0,
        kind: ActionKind::Palette,
        action: 7,
    };
    host.handle_request(HostRequest::OpenList(tidy));
    let list = ui.try_iter().find_map(|command| match command {
        UiCommand::CommandList { id, list } if id == tidy => Some(list),
        _ => None,
    });
    let list = list.flatten().expect("rows");
    assert_eq!(list.rows[0].id, "desk");
    host.handle_request(HostRequest::ListPick {
        id: tidy,
        row: "desk".to_string(),
    });
    assert_eq!(read(&log), ["pick fake_list 7 desk"]);

    host.run_from_key(tidy);
    assert!(
        read(&log).is_empty(),
        "a key opens the palette on it instead"
    );
    assert!(ui
        .try_iter()
        .any(|c| matches!(c, UiCommand::TypeInPalette(text) if text == "!Tidy up")));

    host.plugins.disable(0);
    read(&log);
    host.handle_request(HostRequest::OpenList(tidy));
    let off = ui.try_iter().find_map(|command| match command {
        UiCommand::CommandList { list, .. } => Some(list),
        _ => None,
    });
    assert_eq!(off, Some(None));
    host.handle_request(HostRequest::ListPick {
        id: tidy,
        row: "desk".to_string(),
    });
    assert!(read(&log).is_empty());
}

#[test]
fn the_palette_gets_every_toggles_state_and_again_after_a_flip() {
    let data = Scratch::new("toggle-states");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (mut host, ui) = test_host(
        vec![
            fake("fake_a", "Fake A", "Win+Alt+F1", &log),
            fake("fake_toggle", "Fake T", "Win+Alt+F2", &log),
            fake("fake_toggle_quiet", "Fake Q", "Win+Alt+F3", &log),
        ],
        &keyboard,
        &data,
    );
    read(&log);
    ui.try_iter().count();
    let id = |index| CommandId::Plugin {
        index,
        kind: ActionKind::Palette,
        action: 7,
    };
    let states = |ui: &Receiver<UiCommand>| -> Vec<Vec<(CommandId, ToggleState)>> {
        ui.try_iter()
            .filter_map(|command| match command {
                UiCommand::ToggleStates(states) => Some(states),
                _ => None,
            })
            .collect()
    };

    let log_switch = (
        CommandId::Host(HostCommand::DetailedLog),
        detailed_log::toggle_state(crate::core::detailed::on()),
    );
    host.send_toggle_states();
    let sent = states(&ui);
    assert_eq!(
        sent,
        vec![vec![
            (id(1), ToggleState::on("Tidy now")),
            (id(2), ToggleState::unknown()),
            log_switch.clone(),
        ]],
        "a plain command has no state; a plugin with no answer is unknown;          the host's own toggle comes last"
    );
    assert_eq!(
        read(&log),
        [
            "toggle state fake_toggle 7",
            "toggle state fake_toggle_quiet 7"
        ]
    );
    let snapshot = host.snapshot();
    let look = |index| {
        snapshot
            .commands
            .iter()
            .find(|entry| entry.id == id(index))
            .and_then(|entry| entry.toggle)
    };
    assert!(look(0).is_none());
    assert_eq!(look(1).map(|look| look.tile), Some([1, 2, 3]));

    host.handle_request(HostRequest::RunCommand(id(1)));
    assert_eq!(states(&ui).len(), 1, "a flip reads the states again");
    host.handle_request(HostRequest::RunCommand(id(0)));
    assert!(states(&ui).is_empty(), "a plain command does not");
    assert!(
        host.is_toggle(CommandId::Host(HostCommand::DetailedLog)),
        "a host toggle also gets its state read again after a flip"
    );
    assert!(!host.is_toggle(CommandId::Host(HostCommand::DetailedLogMark)));
    read(&log);

    host.plugins.disable(1);
    host.send_toggle_states();
    assert_eq!(
        states(&ui),
        vec![vec![(id(2), ToggleState::unknown()), log_switch]]
    );
    assert_eq!(ToggleState::unknown().line, "State unknown");
    assert_eq!(ToggleState::unknown().on, None);
    assert_eq!(ToggleState::off("x").on, Some(false));
}

#[test]
fn ctrl_del_hides_a_release_until_a_newer_tag_comes() {
    assert!(!view::update_hidden("v0.21.0", ""), "nothing hidden");
    assert!(
        view::update_hidden("v0.21.0", "v0.21.0"),
        "the same tag stays hidden"
    );
    assert!(
        view::update_hidden("v0.20.9", "v0.21.0"),
        "an older tag too"
    );
    assert!(
        !view::update_hidden("v0.21.1", "v0.21.0"),
        "a newer tag shows"
    );

    let data = Scratch::new("update-hide");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (mut host, ui) = test_host(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    host.updates.newer = Some(release("v0.21.0"));
    assert!(!host.update_view().hidden);
    host.handle_request(HostRequest::HideUpdate("v0.21.0".to_string()));
    assert!(host.update_view().hidden);
    assert_eq!(
        Config::load_in(&data.0).updates.hidden_version,
        "v0.21.0",
        "kept across restarts"
    );
    assert!(
        snapshots(&ui).last().is_some_and(|s| s.update.hidden),
        "the palette hears it"
    );
    host.updates.newer = Some(release("v0.21.1"));
    assert!(!host.update_view().hidden, "the next version shows again");
}

#[test]
fn download_ahead_is_kept_only_in_the_installed_copy_and_goes_when_switched_off() {
    let data = Scratch::new("update-ahead");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (mut host, _ui) = test_host(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    assert!(host.update_view().download_ahead, "on by default");
    // A copy without a folder of its own (every test host) starts nothing.
    host.updates.newer = Some(release("v9.9.9"));
    host.stage_ahead();
    assert!(!host.updates.staging);

    let stage = data.0.join("install");
    std::fs::create_dir_all(&stage).unwrap();
    std::fs::write(stage.join(update_stage::STAGED_EXE), b"kept").unwrap();
    std::fs::write(stage.join(update_stage::RECORD), "{}").unwrap();
    host.updates.stage_dir = Some(stage.clone());
    host.staged("v9.9.9".to_string(), Ok(()));
    assert!(host.update_view().downloaded);
    host.staged("v9.9.9".to_string(), Err("metered".to_string()));
    assert!(!host.update_view().downloaded, "a failed one is not ready");

    host.updates.staged = Some("v9.9.9".to_string());
    host.handle_request(HostRequest::SetHostSetting(
        HostSetting::DownloadUpdatesAhead(false),
    ));
    assert!(!Config::load_in(&data.0).updates.download_ahead);
    assert!(!stage.join(update_stage::STAGED_EXE).exists());
    assert!(!stage.join(update_stage::RECORD).exists());
    assert!(!host.update_view().downloaded);
}

#[test]
fn the_detailed_log_commands_are_in_the_palette_and_deleting_asks_twice() {
    let data = Scratch::new("detailed-entries");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let set = plugin_set(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    let entries = set.palette_entries(&[]);
    let find = |command: HostCommand| {
        entries
            .iter()
            .find(|entry| entry.id == CommandId::Host(command))
            .unwrap_or_else(|| panic!("{command:?} missing"))
    };
    assert_eq!(find(HostCommand::DetailedLog).label, "Detailed log");
    assert_eq!(
        find(HostCommand::DetailedLogMark).label,
        "Detailed log: Mark this moment"
    );
    assert_eq!(
        find(HostCommand::DetailedLogReport).label,
        "Detailed log: Save a report"
    );
    let delete = find(HostCommand::DetailedLogDelete);
    assert_eq!(delete.label, "Detailed log: Delete the files");
    assert!(delete.confirm.starts_with("Press Enter again"));
    assert!(find(HostCommand::DetailedLog).confirm.is_empty());
    assert_eq!(
        find(HostCommand::DetailedLog).toggle,
        Some(detailed_log::toggle_look())
    );
    for plain in [
        HostCommand::DetailedLogMark,
        HostCommand::DetailedLogReport,
        HostCommand::DetailedLogDelete,
    ] {
        assert_eq!(find(plain).toggle, None, "{plain:?} is a plain command");
    }
    assert_eq!(arrange::host_slug(HostCommand::DetailedLog), "detailed-log");
}

#[test]
fn the_detailed_log_toggle_says_its_state_in_words_and_keeps_its_title() {
    let on = detailed_log::toggle_state(true);
    assert_eq!(on.on, Some(true));
    assert_eq!(
        on.line,
        "Recording keys, window titles and palette text on this PC"
    );
    let off = detailed_log::toggle_state(false);
    assert_eq!(off.on, Some(false));
    assert_eq!(off.line, "Off. Nothing is recorded");
    let look = detailed_log::toggle_look();
    assert_eq!((look.icon_on, look.icon_off), ("circle-dot", "circle"));
    assert_eq!(look.tile, [0xb8, 0x48, 0x3a]);
    assert_eq!(detailed_log::TOGGLE_TITLE, "Detailed log");
    assert_eq!(detailed_log::subtitle(HostCommand::OpenSettings), None);
    assert!(detailed_log::host_toggle_look(HostCommand::DetailedLogMark).is_none());
    // The host sends this toggle's state with the plugins' states.
    let states = detailed_log::host_toggle_states();
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].0, CommandId::Host(HostCommand::DetailedLog));
}

#[test]
fn the_old_words_for_the_detailed_log_still_find_it() {
    let data = Scratch::new("detailed-words");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let set = plugin_set(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    let entries = set.palette_entries(&[]);
    let entry = entries
        .iter()
        .find(|entry| entry.id == CommandId::Host(HostCommand::DetailedLog))
        .expect("the toggle");
    for query in ["turn on", "turn off", "toggle", "debug", "logging"] {
        let found = crate::ui::fuzzy::score_command(query, &entry.group, &entry.label)
            .max(crate::ui::fuzzy::score_words(query, &entry.words));
        assert!(found.is_some(), "\"{query}\" does not find it");
    }
}

#[test]
fn a_host_message_gets_a_line_when_it_is_slow_or_not_a_quick_timer() {
    use detailed_log::message_line;
    assert_eq!(
        message_line(WM_HOTKEY, "WM_HOTKEY", 0.4).as_deref(),
        Some("host: WM_HOTKEY 0.40 ms")
    );
    assert_eq!(message_line(WM_TIMER, "WM_TIMER (badges)", 1.0), None);
    assert_eq!(
        message_line(WM_TIMER, "WM_TIMER (badges)", 7.0).as_deref(),
        Some("host: WM_TIMER (badges) 7.00 ms")
    );
    assert_eq!(
        message_line(WM_APP_PLUGIN, "WM_APP_PLUGIN", 73.2).as_deref(),
        Some("host busy 73 ms in WM_APP_PLUGIN")
    );
    assert_eq!(
        message_line(WM_TIMER, "WM_TIMER", detailed_log::BUSY_MS).as_deref(),
        Some("host busy 50 ms in WM_TIMER")
    );
}

#[test]
fn the_tray_menu_has_the_detailed_log_check_item_before_quit() {
    let data = Scratch::new("detailed-menu");
    let (log, keyboard) = (diary(), FakeKeys::default());
    let (host, _ui) = test_host(
        vec![fake("fake_a", "Fake A", "Win+Alt+F1", &log)],
        &keyboard,
        &data,
    );
    let entries: Vec<(u32, String, bool)> = host
        .menu_items()
        .into_iter()
        .filter_map(|item| match item {
            MenuItem::Entry { id, label, checked } => Some((id, label, checked)),
            MenuItem::Separator => None,
        })
        .collect();
    let at = entries.len() - 2;
    assert_eq!(
        entries[at],
        (MENU_DETAILED_LOG, "Detailed log".to_string(), false),
        "unticked: no test turns the detailed log on"
    );
    assert_eq!(entries[at + 1].0, MENU_QUIT);
}

#[test]
fn a_plugin_call_line_says_how_long_and_marks_a_slow_one() {
    use detailed_log::plugin_line;
    assert_eq!(
        plugin_line("window_deck", "hotkey action 1", 0.5, 0.0).as_deref(),
        Some("plugin window_deck: hotkey action 1 0.50 ms")
    );
    assert_eq!(plugin_line("dimmer", "WM_TIMER", 1.0, 5.0), None);
    assert_eq!(
        plugin_line("dimmer", "WM_TIMER", 25.0, 5.0).as_deref(),
        Some("plugin dimmer: WM_TIMER 25.00 ms SLOW")
    );
}
