# Adding a plugin to WinCraft

Before you start, read [CONSTITUTION.md](CONSTITUTION.md): one page with
WinCraft's goal and seven principles. A change that fits them is easy to
review and merge. A short path for a first contribution:

1. Pick an issue labelled `good first issue`, or open one to describe your
   idea first.
2. Fork the repo, then turn on the pre-push check once:
   `git config core.hooksPath .githooks`.
3. Make the change, and run `tools\check.ps1` (or `tools/check.sh`).
4. Open a pull request; its template lists what to check.

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

## Category and picture

The Plugins page and the Plugin Store show every plugin as a tile, with chips
that filter the tiles by category. Three fields of `PluginMetadata` feed them:

- `category` is one short word, such as `Monitors`, `Keyboard`, `Windows` or
  `Input`. Reuse a word another plugin already uses when it fits, so the chip
  row stays short. An empty category is shown as `Other`.
- `icon` is `None`, or the lucide name of the icon on the plugin's tile and
  on its group on the Hotkeys page, such as `Some("pin")`. The names WinCraft
  draws are in `Icon::NAMED` in `src/ui/widgets/icons.rs`. With `None`, or a
  name not in that list, the category's icon shows. It is written to
  `plugins.json` for the store.
- `thumbnail` is `None`, or the path of a 16:9 picture inside the repository.
  It is written to `plugins.json` for the store. The tiles do not draw it yet,
  because WinCraft has no image decoder; they show the plugin's icon.

Both fields are copied into `plugins.json` by `wincraft --write-plugin-index`.
An index file without them still reads, and its plugins land under `Other`.

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
            category: "Input",
            icon: None,
            thumbnail: None,
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
| `Toggle` | On/off switch |
| `Slider { min, max, step }` | Slider with the value shown |
| `Number { min, max }` | Box you type a number into, kept between `min` and `max` |
| `Text` | One-line text box |
| `Choice(&["a", "b"])` | Segmented control for up to 4 options, a dropdown for 5 or more |
| `Path` | One-line text box for a file path, with a Browse… button |

The About page lists the kinds the build you are running can draw.

## What the host does for you

| Trait item | When the host calls it |
|---|---|
| `metadata` | Any time it needs your id, name, version or README. Keep it cheap. |
| `default_settings` | At startup, to fill missing keys in your config file. |
| `migrate_settings(settings)` | At startup, right after missing keys were filled in, to rewrite values an older version stored. The host saves the file afterwards. |
| `settings_fields` | When drawing your settings page. |
| `dynamic_fields` | When drawing your settings page, after `settings_fields`. For settings that depend on what the plugin sees, such as one per connected monitor: the key is a JSON pointer into your settings (`/monitors/PHL 323E7/mode`, each step made with `config::pointer_token`), and you give the current value. The host writes the chosen value at that place. |
| `migrate_hotkeys(hotkeys, settings)` | At startup, right before `migrate_settings`, to move stored keys an older version wrote, such as an old default that another plugin now uses. Keep a note in `settings` if it must happen only once. |
| `init` | When the plugin is switched on. Return `Err` and the host logs it, shows a tray notification and leaves the plugin off. |
| `on_settings_changed` | After the user changes one of your settings. |
| `hotkey_actions` | To register your hotkeys, and to list them in the palette and on your page. |
| `on_hotkey(id)` | When one of your hotkeys is pressed. |
| `on_hotkeys_bound(keys)` | Right after `init`, with the keys each of your hotkey actions has, as the user set them (`"Win+Alt+Q"`, `"Win+Alt+K, J"`, or empty for none). Keep them if you name your keys on screen; a change starts your plugin again. |
| `on_taskbar_created` | When Explorer restarts and builds a new taskbar. It is still empty then, so wait a few seconds before touching its buttons. |
| `tray_actions` | Every time the tray menu opens. Only your **first** tray action goes into the tray menu; every tray action appears in the palette. |
| `on_tray_action(id)` | When your menu entry or its palette entry is chosen. |
| `tray_toggles` | Every time the tray menu opens. Ticked on/off entries, shown after your first tray action, for a setting people switch often. Choosing one calls `on_tray_action` with its id, so keep it apart from your tray action ids (and below 100). Call `host::save_setting` to keep the new value. |
| `palette_commands` | For entries that are neither a hotkey nor a tray item. `words` holds more words the palette finds the entry by, split by spaces, such as `"night light"` for a dark-mode switch. `confirm`, when not empty, makes the palette ask for Enter twice within 5 s and shows that text before the second; use it for a command that cannot be undone. A hotkey on such a command opens the palette on it instead of running it. |
| `command_list(kind, id)` / `on_list_pick(kind, id, row)` | For a palette command with `list: true`: when the user presses Enter on it, its rows (a `SubList` of `ListRow`s, each with an id, a title, a detail, an optional confirm text for Enter twice, and a `current` mark). The palette shows them in place, filters them as the user types, and calls `on_list_pick` with the picked row's id. |
| `toggle_state(kind, id)` | For a palette command with `toggle` set (its on and off icons, as Lucide names `Icon::named` knows, and its tile colour when on): its state now, as a `ToggleState` with on, off or no switch and a line of words such as "Hidden files are shown". Asked each time the palette opens and right after the command ran from the palette, never in the background; read it fresh every time. The row shows a state tile and a switch, and Enter flips it with the palette left open. None shows "State unknown". |
| `command_details(kind, id)` | When one of your command rows becomes selected in the palette, once per selection, never in the background. A status line ("Now: Dark · Enter switches to Light") and name/value lines for the inspector. Keep it quick: it runs on the host thread while the user waits. |
| `on_palette_command(id)` | When one of those is chosen. |
| `search_providers` | Once at startup, whether the plugin is on or not. Extra sources of palette results; see below. |
| `windows` | Once at startup, whether the plugin is on or not. Windows your plugin draws itself with egui; see below. |
| `status` | When building your page. One line under your description; call `host::plugin_changed()` when it changes. |
| `page_action` | When building your page. One of your hotkey actions, or failing that one of your palette commands, shown as a button under the status line. |
| `palette_subtitle(kind, id)` | Each time the palette's list is rebuilt. A second line under one of your palette entries, such as the state the command would change; `kind` says whether `id` is a hotkey, tray or palette id, because those numbers can overlap. Return `None` for no subtitle, and call `host::plugin_changed()` when the answer changes. |
| `window_groups` | When the Arrange strip (WindowDeck) opens (`host::open_arrange()`) and after every change made in it. A plugin that keeps a window order (LayoutKeeper) is asked first; while none is on, the plugin that decides when the strip opens (WindowDeck) lists the open windows itself. |
| `strip_look` | When the strip opens and after every change made in it: whether a rest on a card previews the window, and the view. Only for the plugin that decides when the strip opens (WindowDeck); while no such plugin is on, the strip never opens. The strip saves the view the user picks as that plugin's `arrange_view` setting. |
| `split_from` / `take_over(old, new)` | Once, at the first start of a plugin that was split out of another one (it has no settings file yet): `take_over` moves its settings and hotkeys out of the other plugin's stored config and decides whether it starts on. WindowDeck uses it for what it took from LayoutKeeper. |
| `on_arrange_action(action)` | When the user does something in the Arrange strip: activates a window, drags windows into a new order inside a taskbar group, moves a window to another desktop, or closes it. Rename and Clear name on a card go to WindowNamer instead, whichever plugin draws the strip. |
| `layout_snapshot` | While the Layout page is open, every two seconds: the plugin's saved windows, grouped by app. Only for plugins that keep a saved layout. |
| `on_layout_action(action)` | When the user does something on the Layout page: picks another desktop for a saved window, forgets a closed one, or asks to restore or save now. |
| `on_prompt_answer(id, target, text)` | When the user answers a question the plugin asked with `host::ask`: see below. |
| `on_windows_message` | On `WM_DISPLAYCHANGE`, `WM_SETTINGCHANGE`, `WM_POWERBROADCAST`, `WM_TIMER`, `WM_QUERYENDSESSION` and `WM_ENDSESSION`. The two session messages arrive while every program is still open; the host answers `WM_QUERYENDSESSION` with TRUE itself. |
| `teardown` | When the plugin is switched off and at exit. Release every window and handle here. |

Hotkey and tray actions appear in the command palette automatically. You only
need `palette_commands` for something that has neither. The tray menu stays
short on purpose: it shows each plugin's first tray action, and the rest are
reached through the palette.

`host::notify(title, text)` shows a tray balloon. It is safe to call from any
callback: the balloon appears once the host's message loop comes round.

`host::save_setting(plugin_id, key, value)` changes one of your own settings
and saves it, as if the user had changed it on your page; you hear of it
through `on_settings_changed`. Use it for a tray toggle or a palette command
that flips a setting.

`host::open_arrange_at(app, button)` opens the Arrange strip on an app,
centred above its taskbar button, without taking the keyboard from the
window in front; `host::close_arrange()` closes the strip. WindowDeck's
hover strip uses them.

`host::set_tray_tip(Some(text))` sets the tray icon's tooltip (Windows keeps
the first 127 characters) and `host::set_tray_tip(None)` puts the plain name
back. It is safe to call from any callback too.

`host::ask(prompt)` opens the palette as a one-line text box: a `Prompt`
names your plugin, your own number for the question, what it is about (a
`target`, such as a window handle), the chip before the box, the grey text in
the empty box, the text to start with (selected, so typing replaces it) and
what Enter does with some text and with none. When the user presses Enter,
the text comes back through `on_prompt_answer`; Esc sends nothing.
WindowNamer's rename box works this way. Like `notify`, it is safe to call
from any callback.

## A window of your own

Most plugins need no window: settings, the tray, the palette and the
notices are the host's. When yours keeps a list that people browse and
edit, such as QuickJump's pin list, return a `PluginWindow` from
`windows()`. Like a search provider it is made once at startup and moved to
the UI thread, so share data with your plugin through an `Arc<Mutex<…>>`
and wake your plugin with a message to a window of its own.
`host::show_window(plugin_id, window_id)` opens it from any callback, and
the host closes every window of a plugin that is switched off. Its `show`
runs every frame while `is_open` says so, and draws its own viewport with
`ctx.show_viewport_deferred`; `WindowFrame` gives it the app icon, the
palette's handle (to find its own window with `ui::settings::find_window`)
and the channel to the host, for `HostRequest::FocusWindow`. The widgets in
`ui::widgets`, the theme tokens and `ui::dwm_thumbs` are there to reuse.

## Window titles

WindowNamer can put a user's name on another app's window. Read titles
through the two functions made for it, never with `GetWindowTextW`:

- `windows_list::window_text(hwnd)` is the app's own title, also for a
  renamed window. Use it for anything that identifies or matches windows:
  saved state, rules, lookups.
- `window_names::display_title(hwnd)` is what the user should read: the name
  when there is one. Use it for labels.

## Adding a source of palette results

Commands are for things your plugin *does*. When it has things to *find* — a
list of bookmarks, recent files, saved layouts — give it a search provider.
The palette asks every provider on each keystroke and merges the rows; a
provider can also own a prefix so that typing it sends the search there alone.
WinCraft's own apps, windows, folders, calculator and web search are providers
too, in `src/search/providers/`; read one of them before writing yours.

```rust
use crate::search::{Action, Choice, Context, Query, ResultItem, SearchProvider};
use crate::ui::fuzzy;

struct Bookmarks {
    urls: Vec<(String, String)>,
}

impl SearchProvider for Bookmarks {
    fn id(&self) -> &'static str { "bookmarks" }
    fn name(&self) -> &'static str { "Bookmarks" }
    fn description(&self) -> &'static str { "Open a saved address" }
    fn default_prefix(&self) -> Option<&'static str> { Some("*") }

    fn query(&mut self, query: &Query, _context: &Context) -> Vec<ResultItem> {
        self.urls
            .iter()
            .filter_map(|(name, url)| {
                let score = fuzzy::score(&query.text, name)?;
                Some(ResultItem {
                    group: "Bookmarks".to_string(),
                    title: name.clone(),
                    subtitle: url.clone(),
                    score,
                    enter: Some(Choice {
                        label: "Open".to_string(),
                        action: Action::OpenUrl(url.clone()),
                    }),
                    ..Default::default()
                })
            })
            .take(query.limit)
            .collect()
    }
}

// In your plugin:
fn search_providers(&self) -> Vec<Box<dyn SearchProvider>> {
    vec![Box::new(Bookmarks { urls: load_bookmarks() })]
}
```

What to know:

- **The provider lives on the UI thread**, not with your plugin. It is moved
  there at startup, which is why the trait needs `Send`. It cannot call your
  plugin; give it the data it needs when you create it, or share it through an
  `Arc<Mutex<…>>` that your plugin updates.
- **`query` runs on every keystroke** and must answer in a millisecond or two.
  Answer from memory. If you have to read something slow, do it on your own
  thread and let `query` read the result, as the apps provider does with the
  Start menu.
- **It is only asked while your plugin is on.**
- **A row's keys** are `enter`, `shift_enter`, `ctrl_enter` and `tab`. The
  `Action`s a row can carry are listed in `src/search/mod.rs`; the host runs
  most of them. `Action::Provider` comes back to your provider's `act()` on
  the UI thread, for work only it can do, such as starting a command whose
  output it then shows.
- **Work in the background** by answering `waiting()` while it runs and
  `has_news()` once it has something; the palette then asks `query` again.
  The drives list and the terminal work this way.
- **`blended()`** returns true to also answer searches typed without a prefix.
  Keep that for things people look for all the time, and limit yourself to
  `query.limit` rows so you do not bury the commands.
- **Rows can rank higher once picked.** Give a row a `usage_key` that
  stays the same for the same thing (a path, an id; never a title that
  changes) and the palette counts Enter on it and moves it up next time.
  The router adds your provider's id in front, so keys only need to be
  unique within your provider. Leave it `None` for rows that are answers
  rather than things, like a sum or a web search.
- **A provider can ask a question.** Return `Reply::Ask(Ask { .. })` from
  `act()` and the palette turns into a one-line box (the Ctrl+B bookmark box
  is one). The answer comes back to your `answered(payload, text, context)`,
  which returns another `Ask` when it needs more, such as a second Enter to
  confirm. `payload` is yours: it carries what the answer needs. The
  question is answered in the palette, with no trip through the host.
- **More keys on a row:** `bookmark` is the action for Ctrl+B, `delete` for
  Ctrl+Delete and `copy` for Ctrl+C (when nothing is selected in the box).
  The inspector lists every key a row has, the first being Enter, and the
  footer shows at most five of them with Esc last, so give each choice a
  short `label`: its first word is the Enter pill's verb ("Switch", "Copy").
- **An answer leads the list.** Set `answer` on a row that is worked out
  from the text, as the calculator's is: it goes first and its title is
  drawn large.
- **A scope word** is a short word that sends the search to you when a space
  follows it, such as `w` for windows. Return it from `scope()`; a word that
  is taken is not given twice, and the log says so.
- **Suggestions under an empty box.** `suggestions()` returns rows for
  "Recent and suggested". The palette shows the three picked most, and a row
  with `open` set even when it was never picked; keep `open` for open
  windows.
- **Users can move or switch off your prefix** in config.json under
  `search.prefixes`, keyed by your provider's `id`. If your default prefix is
  already taken, your provider gets none and the log says so.

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
  feel like one family. `wincraft --hotkey-verdict=Win+Alt+Shift+F1,Win+Alt+K`
  prints ShortcutDetector's verdict on each combination before you pick one;
  a key your own running WinCraft holds shows as taken by another app.
- **Record hotkeys only through `ui::widgets::hotkey_capture`.** Its hook
  sends an unassigned mask key (`0xE8`) when it hides a key while Alt or Win
  is held, and it hides that key's release too. Its guard on the recording
  window drops `SC_KEYMENU` and character messages. Keep all three: without
  them Alt+Space opens the window's system menu, Alt+F4 closes the window and
  Win+<key> opens the Start menu while the user is only naming a hotkey.
  A place where the user changes a hotkey opens the one hotkey dialog,
  `ui::widgets::hotkey_dialog`, which records through that hook.
- **No new crates without discussing it first.** WinCraft depends on
  `windows-sys`, `serde`, `serde_json`, `log`, `eframe`/`egui`,
  `egui_commonmark`, `ureq`, `winit` and `raw-window-handle`, and that is
  meant to stay true. `winit` and `raw-window-handle` are the versions eframe
  already uses, named directly: `winit` lets the egui event loop run on the UI
  thread instead of the main thread, and `raw-window-handle` gives the host the
  palette window's `HWND`. If you need a
  Win32 call that is not enabled yet, add the feature to the existing
  `windows-sys` entry in `Cargo.toml`.
- **Fonts.** The UI text is Segoe UI, read from `%WINDIR%\Fonts` when WinCraft
  starts; it is never bundled or committed. If it is missing, the bundled
  Selawik (a free, metric-compatible Segoe UI substitute) is used instead.
  JetBrains Mono is bundled for code and file paths, and Lora and Cormorant
  for the Paper theme. The Terminal theme reads Cascadia Mono (or Consolas)
  from `%WINDIR%\Fonts`. Every bundled font is under the SIL Open Font
  License 1.1, and its licence file sits next to it in `assets/fonts/`. Use
  the helpers in `core::theme` (`regular`, `semibold`, `mono`) rather than
  naming a font: the theme decides which face they give.
- **Themes are token sets.** Colours, corner radii, the palette row height,
  the border width and the fonts come from `core::theme::Tokens`
  (`Tokens::get(ctx)`), never from a number in a widget. Use
  `theme::line(ctx, colour)` for a border, so High contrast can make it
  2 px. A new theme is one more token set in `core/theme.rs` and one more
  `ThemeName`.
- **Log with `log::info!`, `log::warn!`, `log::error!`.** They go to
  `%LOCALAPPDATA%\WinCraft\wincraft.log`. Use `log::debug!` for detail that
  only matters while debugging; it is off unless `WINCRAFT_DEBUG=1` is set.
- **Write what a bug hunt needs to the detailed log.** The user's "Detailed
  log" switch (`core::detailed`) records into
  `%LOCALAPPDATA%\WinCraft\detailed\`, and also gets every `log` line,
  debug lines included. A plugin writes a line with
  `host::detailed(format_args!("deck: ..."))`, which formats nothing while
  the log is off; gather anything costly only when `host::detailed_on()` is
  true. Core code uses `crate::detailed!(...)`. A keyboard hook builds a
  `host::DetailedKey` (or `core::detailed::KeyEvent`) only when the log is
  on, after its decision, and never writes a file itself: the line goes to
  the writer thread through a channel. Start a line with a short word for
  its kind (`key`, `front`, `host`, `deck`, `palette`, `plugin`, `call`),
  which `tools\detailed-log.ps1` sorts by. That script sums a detailed log
  up (`powershell -NoProfile -ExecutionPolicy Bypass -File
  tools\detailed-log.ps1 <file>`); a test runs it on
  `tools\samples\detailed-sample.log`, so a change to a line's form that
  the script reads needs the sample changed with it.
- **Use the shared Win32 helpers in `src/core/`** instead of writing another
  copy: `com` (the owned COM pointer), `windows_list` (the Alt+Tab window
  filter, window text, a process's program path), `desktop_manager` (which
  virtual desktop a window is on), `clipboard`, `monitors` (left, centre,
  right), `wide` / `from_wide_ptr` for UTF-16 strings,
  `window_identity` (finding a window again by its program and title),
  `live_windows` (the app windows open now, with their taskbar group),
  `window_snapshot` (the same windows read on a thread of their own, which
  `live_windows` answers from while one is in use),
  `app_names` (what to call an app's group), `monitor_list` (the monitors
  connected now, for the strip), `taskbar_order` (a group's thumbnail order
  and the calls that set it), `strip_groups` (the strip's data from windows
  in order), `window_actions` (switch to, close or move one window),
  `chord` (waiting for the key after a hotkey), `canvas` (drawing an
  on-screen panel for `core::layered`) and `notice` (the notice with Undo
  that QuickJump and QuickActions show after a change).
- **Clean up in `teardown`.** A plugin can be switched off and on again from
  the settings window, so `init` must work a second time.
- **`cargo fmt`, `cargo test`, `cargo clippy --all-targets` and a
  warning-free `cargo build --release`.** CI runs `cargo fmt --check`, the
  tests, clippy and the release build, with clippy and the build treating
  warnings as errors, so a formatting slip, a lint or a warning fails CI.
  Do not reach for `#[allow]`; fix the cause. CI runs on pull requests and by
  hand (Actions → CI → Run workflow), not on pushes to `main`: run the same
  four checks locally before you push. Releases build on `v*` tags.
- **`tools\check.ps1` runs all of it.** `tools\check.ps1` (or `tools/check.sh`
  from Git Bash or WSL) runs fmt, the tests, clippy, the release build with
  warnings as errors in `target\verify`, and the line-coverage floor in
  `tools\coverage-floor.txt` (needs `cargo install cargo-llvm-cov`), and stops
  at the first failure. It has three levels:
  - `-Fast`: fmt and only the tests of the modules you changed since the
    last commit (all tests when the change is wide, or touches `main.rs`,
    `build.rs` or `Cargo.toml`). A few seconds. Use it while you work.
  - `-Quick`: fmt, every test and clippy. The pre-push hook runs this.
  - no switch: everything, with the release build and the coverage run.
    Run it once, before a pull request or a release.
  `-Smoke` also starts every plugin in a test instance (see
  `tools\smoke.ps1`). Raise the floor when coverage grows; never lower it to
  make a change pass.
- **Remove a feature completely, in one commit.** When you remove a feature,
  setting, flag, view or screenshot scene, remove its tests, its docs (README
  lines, plugin README, this file) and its scene in `tools\screenshots.ps1` in
  the same commit. A test of something that no longer exists gives
  a wrong feeling of safety. The one exception is a test written for a bug that a user saw: it
  stays, next to the code that still has the behaviour. Then run
  `tools\test-map.ps1`. It regenerates `docs\test-map.md` (tests per feature,
  thin features, suspects, who uses each flag) and you commit the result. The
  full `tools\check.ps1` prints the suspect count as a report; it never fails
  the check.
- **A pre-push hook runs `check.ps1 -Quick`.** Turn it on once in each clone
  with `git config core.hooksPath .githooks`. `git push --no-verify` skips it
  for one push.

## Testing next to your own WinCraft

A test build can run while your everyday WinCraft keeps running, as long as
it is a separate instance:

- **`WINCRAFT_INSTANCE=<suffix>`** adds `.<suffix>` to everything that tells
  one WinCraft from another: the single-instance mutex, the host window class
  and the autostart logon task (`WinCraft.<suffix>`, and the Run value when a
  task cannot be made). With it set, a test instance
  starts beside yours, `--quit` only reaches the instance with the same
  suffix, and the autostart setting cannot touch yours. If a test leaves a task
  behind, `schtasks /Delete /TN WinCraft.<suffix> /F` removes it (for your own
  WinCraft it is `schtasks /Delete /TN WinCraft /F`). Unset, every name is
  exactly as in a normal install. Only letters, digits, `-` and `_` count.
- **Point `LOCALAPPDATA` at a scratch folder** so the test instance has its
  own config, log and plugin files. Write a `plugins\<id>.json` with
  `{"enabled": false}` for every plugin you are not testing: with no file, a
  plugin starts enabled with its defaults, and LayoutKeeper would then watch
  and restore your real windows.
- **A test instance writes its detailed log to `detailed.<suffix>`** under
  its data folder, never into `detailed`, so it cannot touch yours.
- **A test instance is read-only towards real windows.** Its LayoutKeeper
  watches, saves into the scratch folder and WindowDeck shows the strip, but never
  restores, reorders taskbar buttons, moves, raises, switches to or closes a
  real window; each such step is logged as "read-only test instance: ...
  skipped". Its hover strip never keeps keys from the window in front
  either: no keyboard hook goes in, and the log says when it would have.
- **Scene flags open one view without any input**, and only in a test
  instance (a normal WinCraft logs that it ignored them):

  | Flag | Opens |
  |---|---|
  | `--open-palette=<query>` | The palette with the query typed in, prefix and all: `/`, `=2+2*3`, `<chrome`, `?`. Nothing runs. |
  | `--open-settings=<page>` | `general`, `plugins`, `hotkeys` (or `shortcuts`), `layout`, `store`, `about`, or a plugin's page: `plugin:layout_keeper`. |
  | `--open-hotkey-dialog=<id>[:<keys>]` | The Hotkeys page with the hotkey dialog open on that command (`wincraft.palette`, `screen_dimmer.toggle_monitor_1`), and the keys after the colon shown as pressed: `wincraft.palette:Win+L`. No keyboard hook runs and nothing is saved. |
  | `--open-arrange=<group>` | WindowDeck on a group, by its place (`0` is the first) or its key. |
  | `--peek-card=<n>` | The strip with the hover peek on card `n` (from 0), or `chip` for the first card of a window that is not on the primary monitor. |
  | `--attention=<hwnd>` | Marks that window as wanting attention at start, as if it had flashed its taskbar button, so its card in the strip is orange. The window itself is not touched. |
  | `--theme=light` or `dark` | That theme for this run. |
  | `--ask-name=<hwnd>` | The palette's rename box for that window (WindowNamer). Nothing is renamed without Enter. |
  | `--show-tip=<id>` | Lets that in-app tip show. A test instance shows no tip otherwise, so no other scene carries one. `bookmark-ctrl-b` is the hint over a folder or file row (pair it with a query that selects one, such as `--open-palette=/C:\Windows\`); `bookmark-saved` opens the palette on the confirmation that follows saving a bookmark, with no Enter needed. |
  | `--show-restore-panel` | LayoutKeeper's please-wait panel with sample progress, and nothing else. A flag without a value. |
  | `--allow-hide-popup` | Lets WindowDeck's hover strip hide Windows' taskbar preview. Without it a test instance opens the strip on hover but never hides the preview, and logs "hiding Windows' taskbar preview skipped". Use it only for a test you watch: it acts on your real taskbar. A flag without a value. |
  | `--ddc-test=<what>` | ScreenDimmer's DDC/CI check on real monitors, printed step by step, then exit; no tray icon, no plugins. `list` only reads each monitor's power mode and brightness. `left`, `centre` or `right` turns that monitor off the way Auto does, keeps it so for 10 s and brings it back; `wake-left` and so on only wakes it. Run it only with the person at the PC: it really turns a screen off. |
  | `--show-update-available` | A sample newer release (v9.9.9), shown as if this copy could install it: About lists it and the palette pins its "Update ready" row above Recent. Every real update stays refused. A flag without a value. |
  | `--select-update` | With the one above and `--open-palette`: the palette opens with the "Update ready" row selected, so its inspector shows. Nothing runs. A flag without a value. |
  | `--show-update-progress` | About and the update panel at the bottom right half way through a sample download of v9.9.9 (42 %). Nothing is downloaded. A flag without a value. |
  | `--update-now` | Checks for an update at once and installs what it finds, as if Update now were clicked. With `WINCRAFT_RELEASES_URL` pointing at a fake releases API on this PC and `WINCRAFT_INSTALL_DIR` (or a scratch `LOCALAPPDATA`) giving the test instance an install folder of its own, the whole update and its rollback run without the internet. A flag without a value. |
  | `--rename-window=<hwnd>:<name>` | Renames that window through WindowNamer at start, for a live test on a window the test owns. Never point it at a window you did not create. |

  A test instance's WindowNamer renames only on request and puts saved
  names back by title only, never on an app's only window, so a name saved
  in a test cannot land on one of your own windows.

  Each scene is a fresh test instance, started with its flags and stopped
  with `--quit`, so no scene can leave state behind for the next.
- **Installs and updates in a test instance** never reach yours.
  `--install` gives the shortcut the suffix too (`WinCraft.<suffix>.lnk`) and
  starts the copy directly, not through the logon task, which would start it
  without the test environment. `WINCRAFT_INSTALL_DIR` moves a test
  instance's install folder, and `WINCRAFT_RELEASES_URL` points its update
  check at a fake of `releases/latest`; a test instance may download from
  `http://127.0.0.1:` only then. `WINCRAFT_RELEASES_PAGE_URL` points the
  fallback for GitHub's request limit at a fake of the releases page, which
  answers with a redirect to `/releases/tag/<tag>`; without it a test
  instance never asks the real page. Without that fake a test instance never
  checks on its own, and it only logs the update balloons. `--install` and
  `--uninstall` refuse inside another app's package, so from a shell started
  by Claude Desktop use a scratch `LOCALAPPDATA` under
  `%LOCALAPPDATA%\Temp`, which is not redirected.
- **`wincraft.exe --quit`** asks the running instance to shut down the way
  the tray's Quit does and waits up to 10 seconds. It prints one line and
  exits with 0 when WinCraft closed, 1 when none was running and 2 when it was
  still running after 10 seconds. It uses `WINCRAFT_INSTANCE` too.

### One scene, now

`tools\scene-shot.ps1` takes one picture of one scene at once, to look at a UI
change before a release: a test instance (`WINCRAFT_INSTANCE=sceneshot`, its
own `LOCALAPPDATA` under `%LOCALAPPDATA%\Temp`, every plugin off unless
`-Plugins` names it) opens the scene, its largest window is captured from the
screen, and the instance quits. It uses the newest of `target\debug`,
`targeterifyelease` and `targetelease`.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools\scene-shot.ps1 -Scene '--open-palette=toggle' -Plugins quick_actions -Theme fluent
```

It does not wait for you to be away: the scene shows for a few seconds and
may take the keyboard, and the window that was in front gets it back at the
end. For every scene in both themes, use the script below.

### Screenshots while you are away

`tools\screenshots.ps1` takes a PNG of every scene (the palette empty and
with `/`, `=2+2*3`, `<`, `?` and `toggle`; settings General, Plugins, Hotkeys, the
hotkey dialog waiting, with free keys, a conflict and a Windows shortcut, LayoutKeeper's
page and About; the empty Bookmarks page and the palette's two bookmark tips; the strip in the view Auto picks and in the Flat view; the
strip with a peek on a card of a window off the primary monitor; WindowNamer's rename box for the window that was in front; the strip
with a renamed window, which is a minimized window the harness opens
itself and closes at the end), in the dark and the light theme, from test
instances only. It is
built for a PC someone is using: it starts only after `-IdleSeconds` (180 by
default) without keyboard or mouse input and only while the desktop is
unlocked, it sends no input itself, and it checks the last input time before
and after every scene. Any input stops it at once: the test instance quits,
the window that was in front gets the focus back, `ABORTED.txt` names the
scene it reached, and it exits with 3. Your own WinCraft is never touched;
the report says whether its process was the same before and after.

```powershell
cargo build --release --target-dir target\verify
powershell -NoProfile -ExecutionPolicy Bypass -File tools\screenshots.ps1 -DryRun
powershell -NoProfile -ExecutionPolicy Bypass -File tools\screenshots.ps1
```

`-DryRun` checks the conditions and prints the plan without starting
anything. The pictures, `report.txt` and the test instance's scratch
folder go to `target\shots\<date-time>\` (or `-Out`). `-Exe` picks another
build; it refuses an exe that is already running. `-SimulateInputAfterScene
N` acts as if you came back after scene N, to test the abort. `-Query no` adds a
palette scene with that text typed, `-UsageFile <file>` starts the test
instances with that `palette_usage.json`, and `-Only <scene>` runs just the
scenes named. Exit codes: 0
done, 1 error, 2 refused to start, 3 aborted.

The palette pictures include the shadow margin around the panel, so they
show a little of whatever is behind it, and the peek pictures show the real
windows on that monitor.

## Ids

Three separate numbering spaces, all local to your plugin:

- **Hotkey action ids** (`HotkeyAction::id`) — any `u32`. ScreenDimmer uses
  1, 2, 3 for covering the monitors from the left, 11, 12, 13 for turning
  them off and 100 for "wake all", leaving room to add more.
  A `page_action` id is looked up among these first, then among the palette
  command ids, so give a page button a number no hotkey action uses.
- **Tray action ids** (`TrayAction::id`) — must be **below 100**. The host
  packs them into menu item ids with the plugin's position in the list.
- **Palette command ids** (`PaletteCommand::id`) — any `u32`.
