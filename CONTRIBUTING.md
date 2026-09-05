# Adding a module to WinCraft

Everything a feature needs — the tray icon, the hidden window, global hotkeys,
config storage, logging — already belongs to the host. A module says what it
wants and gets called back. It never creates a tray icon or a message loop of
its own.

## The three steps

### 1. Create `src/modules/<your_id>/mod.rs`

Use the same short lowercase id everywhere: the folder name, the `id` in
`ModuleMetadata`, and the key in `config.json`.

### 2. Implement `WinCraftModule`

The trait is in [`src/core/traits.rs`](src/core/traits.rs). Only `metadata` and
`init` are required; everything else has a default that does nothing.

```rust
use crate::core::traits::{HostContext, ModuleMetadata, TrayAction, WinCraftModule};

#[derive(Default)]
pub struct Hello {
    greeting: String,
}

impl Hello {
    pub fn new() -> Self {
        Self::default()
    }
}

impl WinCraftModule for Hello {
    fn metadata(&self) -> ModuleMetadata {
        ModuleMetadata {
            id: "hello",
            name: "Hello",
            description: "Says hello in the log.",
            author: "you",
            version: "1.0.0",
        }
    }

    fn default_settings(&self) -> serde_json::Value {
        serde_json::json!({ "greeting": "hello" })
    }

    fn init(&mut self, ctx: &HostContext) -> Result<(), String> {
        self.greeting = ctx.settings["greeting"].as_str().unwrap_or("hello").to_string();
        Ok(())
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

### 3. Add one line to `load_active_modules()`

In [`src/modules/mod.rs`](src/modules/mod.rs):

```rust
pub mod hello;

pub fn load_active_modules() -> Vec<Box<dyn WinCraftModule>> {
    vec![
        Box::new(screen_dimmer::ScreenDimmer::new()),
        Box::new(hello::Hello::new()),
    ]
}
```

Build and start WinCraft. Your module is in the tray menu and in About, and its
section appears in `config.json` with every default filled in.

## What the host does for you

| Trait item | When the host calls it |
|---|---|
| `metadata` | Any time it needs your id, name or version. Keep it cheap. |
| `default_settings` | At startup, to fill missing keys in `config.json`. |
| `init` | When the module is switched on. Return `Err` and the host logs it, shows a tray notification and leaves the module off. |
| `hotkey_actions` | At startup and when switched on, to register your hotkeys. |
| `on_hotkey(id)` | When one of your hotkeys is pressed. |
| `tray_actions` | Every time the tray menu opens. |
| `on_tray_action(id)` | When your menu entry is chosen. |
| `on_windows_message` | On `WM_DISPLAYCHANGE`, `WM_SETTINGCHANGE`, `WM_POWERBROADCAST` and `WM_TIMER`. |
| `teardown` | When the module is switched off and at exit. Release every window and handle here. |

`HostContext` gives you the hidden host window and your own `settings` object
from `config.json`.

## Rules

- **No new crates without discussing it first.** WinCraft depends on
  `windows-sys`, `serde`, `serde_json` and `log`, and that is meant to stay
  true. If you need a Win32 call that is not enabled yet, add the feature to
  the existing `windows-sys` entry in `Cargo.toml`.
- **Hotkey defaults must start with `Win+Alt`.** That combination is nearly
  free on Windows, which keeps clashes with other programs rare, and it makes
  every WinCraft hotkey feel like one family.
- **Log with `log::info!`, `log::warn!`, `log::error!`.** They go to
  `%LOCALAPPDATA%\WinCraft\wincraft.log`. Use `log::debug!` for detail that
  only matters while debugging; it is off unless `WINCRAFT_DEBUG=1` is set.
- **The host is single-threaded on purpose.** Your module is only ever called
  from the message loop, which is why the trait has no `Send + Sync` bound and
  why you may hold raw `HWND`s. If you spawn a thread, do not touch module
  state from it — post a message to the host window instead.
- **Never block in a callback.** `on_hotkey` and the rest run inside the
  message loop; anything slow freezes the tray menu.
- **Clean up in `teardown`.** A module can be switched off and on again from
  the tray, so `init` must work a second time.
- **`cargo build --release` must finish with no warnings**, and
  `cargo test` must pass.

## Ids

Two separate numbering spaces, both local to your module:

- **Hotkey action ids** (`HotkeyAction::id`) — any `u32`. ScreenDimmer uses
  1, 2, 3 for the monitors and 100 for "wake all", leaving room to add
  monitors 4 and up.
- **Tray action ids** (`TrayAction::id`) — must be **below 100**. The host
  packs them into menu item ids together with the module's position in the
  list.
