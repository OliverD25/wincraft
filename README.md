# WinCraft

A small, modular tray utility for Windows 11, in the style of Microsoft
PowerToys. One program owns the tray icon, the autostart entry, the global
hotkeys, a command palette and a settings window. Features are written as
**plugins** that plug into one Rust trait, so adding a feature never means
touching the Win32 plumbing or writing any UI code.

Version 0.6 ships three plugins: **ScreenDimmer**, **ShortcutDetector** and
**LayoutKeeper**.

## What you get

- **Command palette** — `Win+Alt+P` opens a search box on the monitor your
  pointer is on. Type a few letters, press Enter. It finds WinCraft's commands,
  Start menu apps and open windows, and with a prefix it browses folders,
  does sums and searches the web. See [Command palette](#command-palette).
- **Settings window** — General, Plugins, Plugin Store and About, with a page
  per plugin: its options, its hotkeys, its config file and its README.
- **Plugin store** — what exists, what this copy has, and what needs a newer
  WinCraft.
- **Tray icon** — open the palette, open settings, one main action per
  enabled plugin, and quit. Everything else is in the settings window.

<!-- Screenshots: docs/palette.png, docs/settings.png -->

## Install

No release is published yet, so for now you build WinCraft from source. You
need [rustup](https://rustup.rs) and the Windows SDK (for `rc.exe`, which
compiles the icon into the exe). The Rust version is pinned by
`rust-toolchain.toml`, so cargo fetches the right compiler on first use:

```powershell
git clone https://github.com/OliverD25/wincraft; cd wincraft; cargo build --release
```

The result is `target\release\wincraft.exe`. There is no installer and nothing
to unpack — it is a single file that writes its settings to
`%LOCALAPPDATA%\WinCraft`.

Once a release is published, you can instead download `wincraft.exe` from the
[Releases page](https://github.com/OliverD25/wincraft/releases) and run it.

## Plugins

### ScreenDimmer

Blacks out a monitor with an overlay you can click straight through. Moving
your pointer onto a blacked-out monitor makes it partly see-through so you can
still find things. [Full README](src/plugins/screen_dimmer/README.md)

| Hotkey | What it does |
|---|---|
| `Win+Alt+F1` | Black out / restore monitor 1 |
| `Win+Alt+F2` | Black out / restore monitor 2 |
| `Win+Alt+F3` | Black out / restore monitor 3 |
| `Win+Alt+F12` | Restore every monitor |

### ShortcutDetector

Shows which global key combinations are already taken on this PC, and tells you
about any combination you press before you assign it.
[Full README](src/plugins/shortcut_detector/README.md)

| Hotkey | What it does |
|---|---|
| `Win+Alt+Q` | Open the shortcut detector |

It cannot see programs that read keys through a keyboard hook, such as
AutoHotkey scripts or PowerToys Keyboard Manager. Nothing can, short of
injecting code into every running process, and the window says so.

### LayoutKeeper

Remembers which virtual desktop each Chrome window is on, the order of the
thumbnails in its taskbar group and which window is in front, and puts all
three back after a reboot. Any program can be watched, not only Chrome.
[Full README](src/plugins/layout_keeper/README.md)

| Hotkey | What it does |
|---|---|
| `Win+Alt+L` | Restore the saved layout |
| `Win+Alt+J` | Save the layout now |
| `Win+Alt+[` | Move the front window one place left in its taskbar group |
| `Win+Alt+]` | Move the front window one place right in its taskbar group |
| `Win+Alt+A` | Open the Arrange strip: live pictures of the program's windows, drag them into order |

Moving windows between desktops uses undocumented Windows interfaces, ported
from [MScholtes' VirtualDesktop](https://github.com/MScholtes/VirtualDesktop)
(MIT). They change between Windows builds; if they do not answer as expected,
LayoutKeeper turns desktop moves off for the session and says so on its page.

To reorder with the mouse, use the Arrange strip. Dragging inside the
taskbar's thumbnail flyout is not possible from outside Explorer on Windows 11
24H2, because the flyout exposes no accessible thumbnail items (probed on
2026-09-25). The Windhawk mod "Taskbar Thumbnail Reorder" still works for hand
reorders, but WinCraft cannot see its changes, and the next restore replaces
them.

### Host hotkeys

| Hotkey | What it does |
|---|---|
| `Win+Alt+P` | Open the command palette |

## Command palette

Without a prefix, the palette searches three things at once and lists the best
matches first: WinCraft's own commands (every plugin action, every plugin
on/off switch, the host's items), the programs in your Start menu, and the
windows that are open on any virtual desktop.

A prefix typed at the start sends the search to one source only. It turns into
a chip before the search box; Backspace in the empty box removes it.

| Prefix | What it searches | Enter | Shift+Enter | Tab |
|---|---|---|---|---|
| none | Commands, apps and open windows | Run, open or switch to | | Put the title in the box; on a File Explorer window, browse its folder under `/` |
| `<` | Open windows only, the ones Alt+Tab shows, by title or program | Switch to it, restoring it if minimized | | Put the title in the box; on a File Explorer window, browse its folder under `/` |
| `/` | Drives, then one folder at a time: `/C:\Users\` | Open the file or folder | Show it in Explorer, selected | Go into the folder |
| `=` | A sum: `+ - * / ^ %` and brackets; `×` and `÷` work too | Copy the answer | | Put the answer in the box |
| `>` | A command for the terminal shell chosen in settings (WSL bash unless you change it), then your earlier commands | Run it here, with its output in the palette | | Put the earlier command in the box |
| `?` | Alone: this list of prefixes. With words: a web search | Pick the prefix, or search in your default browser | | Pick the prefix |

In the calculator `%` after a number is a percentage (`200 * 15%` is 30) and
between two numbers is the remainder (`10 % 3` is 1). A comma works as the
decimal mark.

The keys: ↑ ↓ move, Enter runs, Shift+Enter does the second action, Tab
completes, Esc closes. The footer always shows what Enter, Shift+Enter,
Ctrl+Enter and Tab do on the selected row.

### Terminal commands with `>`

Typing after `>` never runs anything. Enter runs the command with no window
and streams its output into the palette, errors in the warning colour, with
a status line on top ("Running… 1.2 s", then "Exit 0 · 3.4 s"). The palette
stays open; Esc stops the command and everything it started, and a second
Esc closes the palette. A command keeps running when the palette closes, and
`>` shows its output again until the next command. Enter on an output line
copies it; Ctrl+Shift+C copies all of it. Ctrl+Enter opens the command in
Windows Terminal instead (a new tab that stays open), or in a console window
when Windows Terminal is not installed.

If you browsed into a folder with `/` just before, the command runs there;
otherwise WSL starts in its home folder and the Windows shells in your user
folder.

A command that can delete data or stop the PC (`rm -rf`, `mkfs`, `dd of=`,
`shutdown`, `git push --force`, `git reset --hard`, `format`, `diskpart`,
`Remove-Item -Recurse`, `del /s` and a few more) needs a second Enter
within 5 seconds. Any other key cancels it.

Commands you run are remembered in `terminal_history.json` (the last 100,
on this PC only) unless you switch that off in settings.

The shell is chosen on the General page of the settings window: WSL bash,
PowerShell 7, Windows PowerShell, Command Prompt, or a custom command line
with `{cmd}` where the command goes and `{cwd}` for the folder.

## Where your settings live

Everything is under `%LOCALAPPDATA%\WinCraft`, which is
`C:\Users\<you>\AppData\Local\WinCraft`.

```
config.json            the host's own settings
plugins\
  screen_dimmer.json   one file per plugin
  shortcut_detector.json
  layout_keeper.json
  layout_keeper.state.json   the saved window layout, written by LayoutKeeper
  layout_keeper.state.prev.json   the layout from before a browser crash or restart
cache\                 the last plugin index fetched from GitHub
wincraft.log           what the program did, emptied on every start
```

Splitting the files this way means resetting one plugin deletes one file, and a
broken edit costs one plugin rather than your whole configuration. A file
WinCraft cannot read is left exactly as it is: WinCraft runs on defaults, says
why in the log, and saves nothing over it until you fix it. WinCraft
0.2 kept everything in `config.json`; the first 0.3 start moves it and says so
in the log.

### config.json

```json
{
  "start_with_windows": false,
  "theme": "system",
  "palette_hotkey": "Win+Alt+P",
  "search": {
    "prefixes": { "calc": "=", "paths": "/", "terminal": ">", "web": "?", "windows": "<" },
    "web_url": "https://www.google.com/search?q={query}",
    "terminal_shell": "WSL bash",
    "terminal_custom": "",
    "terminal_history": true
  }
}
```

| Key | Meaning |
|---|---|
| `start_with_windows` | Mirrors the "Start with Windows" switch and the registry Run value. |
| `theme` | `system`, `light` or `dark`. |
| `palette_hotkey` | What opens the command palette. |
| `search.prefixes` | The prefix of each palette search source. Change one to move it, set it to `""` to switch that prefix off. A source left out keeps its built-in prefix. Use symbols: a letter as a prefix would catch every search that starts with it. |
| `search.web_url` | The web search address, with `{query}` where the words go. It must start with `https://` or `http://`. |
| `search.terminal_shell` | What `>` runs commands in: `WSL bash`, `PowerShell 7`, `Windows PowerShell`, `Command Prompt` or `Custom`. |
| `search.terminal_custom` | For `Custom`: a command line with `{cmd}` where the command goes and optionally `{cwd}` for the folder, such as `C:\msys64\usr\bin\bash.exe -lc {cmd}`. Without `{cmd}` WSL bash is used. |
| `search.terminal_history` | Whether commands run from the palette are remembered. |

The prefixes are read when WinCraft starts, so restart it after changing
them. The other `search` keys are also on the General page of the settings
window and apply at once.

### plugins/&lt;id&gt;.json

```json
{
  "enabled": true,
  "hotkeys": { "toggle_monitor_1": "Win+Alt+F1" },
  "settings": { "idle_opacity": 1.0, "hover_opacity": 0.7 }
}
```

WinCraft fills in every missing key on each start, so you never have to invent
a name: start it once, then open the file and change the values. Anything you
have already written is left alone.

### Hotkey format

Modifiers joined to one key with `+`, in any case:

- Modifiers: `Ctrl` (or `Control`), `Alt`, `Shift`, `Win`
- Keys: `A`–`Z`, `0`–`9`, `F1`–`F24`, `Space`, `Esc`, `Tab`, `Enter`,
  `Backspace`, `Insert`, `Delete`, `Home`, `End`, `PageUp`, `PageDown`, `Left`,
  `Right`, `Up`, `Down`, `PrintScreen`, `Pause`, `ScrollLock`, `NumLock`,
  `Numpad0`–`Numpad9`, `NumpadPlus`, `NumpadMinus`, `NumpadMultiply`,
  `NumpadDivide`, `NumpadDecimal`, and the punctuation keys `;` `=` `,` `-` `.`
  `/` `` ` `` `[` `\` `]` `'`

A bare key without modifiers is accepted too, such as `F13` or `Pause`. Keep
it to keys you never type with, because WinCraft takes that key from every
other program while it runs. If another program already owns a combination,
WinCraft keeps running, shows a tray notification and marks the hotkey
"taken by another app" on the plugin's page and on the About page.

## Command line

| Option | What it does |
|---|---|
| `--open-palette` | Opens the command palette at startup. |
| `--open-arrange` | Opens LayoutKeeper's Arrange strip at startup. |
| `--open-settings` | Opens the settings window at startup. |
| `--open-detector` | Opens the ShortcutDetector window at startup. |
| `--write-plugin-index` | Regenerates `plugins.json`. For contributors. |

Set `WINCRAFT_DEBUG=1` for a more detailed log.

## Writing a plugin

See [CONTRIBUTING.md](CONTRIBUTING.md). A plugin is one folder, one trait
implementation, a README, and one line in the plugin list. You describe your
settings; WinCraft draws and saves them.

## Licence

MIT. See [LICENSE](LICENSE).

The interface uses Segoe UI from your own Windows installation; nothing of it
is shipped. WinCraft bundles two fonts, each under the SIL Open Font License
1.1, with their licence files in [assets/fonts](assets/fonts):
[Selawik](https://github.com/microsoft/Selawik) by Microsoft, used when Segoe
UI is missing, and [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono)
for paths and code.
