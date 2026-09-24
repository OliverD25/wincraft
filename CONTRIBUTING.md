# Adding a plugin to WinCraft

Everything a feature needs — the tray icon, the hidden window, global hotkeys,
config storage, logging, the settings page and the palette entries — already
belongs to the host. A plugin says what it wants and gets called back. It never
creates a tray icon, a message loop, or a line of UI code.

## Naming

A mini program inside WinCraft is a plugin. Use the word plugin in code, docs, UI and config. The word module is only for Rust mod files.

## The checklist

1. Create `src/plugins/<your_id>/mod.rs`.
2. Write `src/plugins/<your_id>/README.md`. It is loaded with `include_str!`,
   so a plugin without one does not compile. It is shown on the plugin's page
   and in the store.
3. Implement `WinCraftPlugin` (see below).
4. Add `settings_fields()` if your plugin has options.
5. Add one line to `load_active_plugins()` in
   [`src/plugins/mod.rs`](src/plugins/mod.rs).
6. Run `wincraft --write-plugin-index` from the repo root to regenerate
   `plugins.json`, and commit the result. A test compares the committed file
   with what your build produces, so forgetting this fails CI.
7. Open a pull request. CI must be green.

Use the same short lowercase id everywhere: the folder name, `id` in
`PluginMetadata`, and the file name under `plugins\`.

## A whole plugin

```rust
use crate::core::traits::{
    FieldKind, HostContext, PluginMetadata, SettingField, TrayAction, WinCraftPlugin,
};

#[derive(Default)]
pub struct Hello {
    greeting: String,
}

impl Hello {
    pub fn new() -> Self {
        Self::default()
    }
}

impl WinCraftPlugin for Hello {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            id: "hello",
            name: "Hello",
            description: "Says hello in the log.",
            author: "you",
            version: "1.0.0",
            readme: include_str!("README.md"),
        }
    }

    fn default_settings(&self) -> serde_json::Value {
        serde_json::json!({ "greeting": "hello" })
    }

    fn settings_fields(&self) -> Vec<SettingField> {
        vec![SettingField {
            key: "greeting",
            label: "Greeting",
            help: "What to write in the log.",
            kind: FieldKind::Text,
        }]
    }

    fn init(&mut self, ctx: &HostContext) -> Result<(), String> {
        self.greeting = ctx.settings["greeting"].as_str().unwrap_or("hello").to_string();
        Ok(())
    }

    fn on_settings_changed(&mut self, settings: &serde_json::Value) -> bool {
        self.greeting = settings["greeting"].as_str().unwrap_or("hello").to_string();
        true
    }

    fn tray_actions(&self) -> Vec<TrayAction> {
        vec![TrayAction { id: 1, label: "Say hello" }]
    }

    fn on_tray_action(&mut self, action_id: u32) {
        if action_id == 1 {
            log::info!("{}", self.greeting);
        }
    }
}
```

That plugin now has a settings page, a palette entry, a tray item and a config
file, without a single line of egui.

## Settings without UI code

Return a `SettingField` per option and WinCraft draws the control, saves the
value to your plugin's file, and hands you the whole settings object through
`on_settings_changed`. Return `true` if you applied the change live; return
`false` and the host stops and starts your plugin instead, which is always
correct and merely slower.

| `FieldKind` | Control |
|---|---|
| `Toggle` | Tick box |
| `Slider { min, max, step }` | Slider with the value shown |
| `Number { min, max }` | Drag-to-change number |
| `Text` | One-line text box |
| `Choice(&["a", "b"])` | Dropdown |
| `Path` | One-line text box for a file path |

The About page lists the kinds the build you are running can draw.

## What the host does for you

| Trait item | When the host calls it |
|---|---|
| `metadata` | Any time it needs your id, name, version or README. Keep it cheap. |
| `default_settings` | At startup, to fill missing keys in your config file. |
| `settings_fields` | When drawing your settings page. |
| `init` | When the plugin is switched on. Return `Err` and the host logs it, shows a tray notification and leaves the plugin off. |
| `on_settings_changed` | After the user changes one of your settings. |
| `hotkey_actions` | To register your hotkeys, and to list them in the palette and on your page. |
| `on_hotkey(id)` | When one of your hotkeys is pressed. |
| `tray_actions` | Every time the tray menu opens. |
| `on_tray_action(id)` | When your menu entry or its palette entry is chosen. |
| `palette_commands` | For entries that are neither a hotkey nor a tray item. |
| `on_palette_command(id)` | When one of those is chosen. |
| `status` | When building your page. One line under your description; call `host::plugin_changed()` when it changes. |
| `on_windows_message` | On `WM_DISPLAYCHANGE`, `WM_SETTINGCHANGE`, `WM_POWERBROADCAST`, `WM_TIMER`, `WM_QUERYENDSESSION` and `WM_ENDSESSION`. The two session messages arrive while every program is still open; the host answers `WM_QUERYENDSESSION` with TRUE itself. |
| `teardown` | When the plugin is switched off and at exit. Release every window and handle here. |

Hotkey and tray actions appear in the command palette automatically. You only
need `palette_commands` for something that has neither.

`host::notify(title, text)` shows a tray balloon. It is safe to call from any
callback: the balloon appears once the host's message loop comes round.

## If your plugin opens a window

`on_hotkey` and `on_tray_action` are called while the host has its own state
mutably borrowed. Inside them your plugin may **create** a window, but it must
not do anything that reads the host back — `host::registered_hotkeys()` is the
one that will catch you out.

The way round it is one line. Create the window, then post it a message and do
the real work when that message arrives:

```rust
fn open(&mut self) {
    let hwnd = create_my_window();
    unsafe { PostMessageW(hwnd, WM_APP_MY_WORK, 0, 0) };
    self.window = Some(hwnd);
}
```

By the time `WM_APP_MY_WORK` comes back through the message loop the host is no
longer borrowed. The same applies to `WM_CREATE`, which `CreateWindowExW`
delivers before it has even returned. ShortcutDetector does exactly this; copy
it if you need a window.

## Threads

There are two: the Win32 host loop on the main thread, and the egui UI thread.

- Your plugin lives on the **host thread** and is only ever called from its
  message loop. That is why the trait has no `Send + Sync` bound and why you
  may hold raw `HWND`s.
- **Never touch an egui or winit object from the host thread**, and never touch
  a plugin from the UI thread. The two sides exchange messages
  (`src/core/ui_bridge.rs`) and a read-only snapshot.
- If you spawn a thread, do not touch plugin state from it — post a message to
  the host window instead.
- Never block in a callback. `on_hotkey` and the rest run inside the message
  loop; anything slow freezes the tray menu and the palette.

## Rules

- **`core::hotkeys` knows the whole keyboard.** Letters, digits, `F1`–`F24`,
  the navigation and editing keys, the numeric keypad and the punctuation keys
  all parse and format. `hotkeys::all_keys()` returns every one, and it is the
  same list ShortcutDetector probes.
- **Hotkey defaults must start with `Win+Alt`.** That combination is nearly
  free on Windows, which keeps clashes rare and makes every WinCraft hotkey
  feel like one family.
- **No new crates without discussing it first.** WinCraft depends on
  `windows-sys`, `serde`, `serde_json`, `log`, `eframe`/`egui`,
  `egui_commonmark` and `ureq`, and that is meant to stay true. If you need a
  Win32 call that is not enabled yet, add the feature to the existing
  `windows-sys` entry in `Cargo.toml`.
- **Log with `log::info!`, `log::warn!`, `log::error!`.** They go to
  `%LOCALAPPDATA%\WinCraft\wincraft.log`. Use `log::debug!` for detail that
  only matters while debugging; it is off unless `WINCRAFT_DEBUG=1` is set.
- **Clean up in `teardown`.** A plugin can be switched off and on again from
  the settings window, so `init` must work a second time.
- **`cargo fmt`, `cargo test` and a warning-free `cargo build --release`.** CI
  runs all three and builds with `-D warnings`, so a warning fails the build.
  Do not reach for `#[allow]`; fix the cause.

## Ids

Three separate numbering spaces, all local to your plugin:

- **Hotkey action ids** (`HotkeyAction::id`) — any `u32`. ScreenDimmer uses
  1, 2, 3 for the monitors and 100 for "wake all", leaving room to add more.
- **Tray action ids** (`TrayAction::id`) — must be **below 100**. The host
  packs them into menu item ids with the plugin's position in the list.
- **Palette command ids** (`PaletteCommand::id`) — any `u32`.
