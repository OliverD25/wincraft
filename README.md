# WinCraft

A small, modular tray utility for Windows 11, in the style of Microsoft
PowerToys. One program owns the tray icon, the autostart entry, the global
hotkeys and a hidden message window. Features are written as **modules** that
plug into one Rust trait, so adding a feature never means touching the Win32
plumbing.

Version 0.2 ships two modules: **ScreenDimmer** and **ShortcutDetector**.

## ScreenDimmer

Blacks out a monitor with an overlay you can click straight through. The
overlay never takes keyboard focus, so the window you were typing in stays
active.

When your mouse pointer moves onto a blacked-out monitor, the overlay becomes
partly see-through, so you can still find things there. Move the pointer away
and it goes fully black again.

| Hotkey | What it does |
|---|---|
| `Win+Alt+F1` | Black out / restore monitor 1 |
| `Win+Alt+F2` | Black out / restore monitor 2 |
| `Win+Alt+F3` | Black out / restore monitor 3 |
| `Win+Alt+F12` | Restore every monitor |

Monitors are numbered in the order Windows enumerates them. Monitor 1 is
normally your primary display.

## ShortcutDetector

Shows which global key combinations are already taken on this PC, and tells you
about any combination you press before you assign it to anything.

Open it with `Win+Alt+K`, or from the tray menu ("Shortcut detector"). It scans
when it opens and again whenever you press Refresh or F5. Nothing runs in the
background while the window is closed.

### The table

Each row is one combination. The columns are:

| Column | What it tells you |
|---|---|
| Shortcut | The combination, e.g. `Win+Alt+F1`. |
| Status | `Taken by an app`, `Windows`, `WinCraft`, or `Free`. |
| Owner / meaning | What the Windows shortcut does, or which WinCraft module and action holds it. |
| Source | How WinCraft knows: `probe`, `windows list`, or `wincraft`. |

By default only taken combinations are listed. Tick **Show free** to add the
rest. Type in **Search** to filter on the shortcut, the owner or the status.
Use the **Modifiers** dropdown to see one modifier set at a time, for example
only the `Win+Alt` combinations. Click a column heading to sort by it; click it
again to reverse the order.

**Copy** puts the rows you can currently see on the clipboard as tab-separated
text with a header row, so it pastes straight into Notepad or Excel.

### Checking one shortcut

Click the **Check a shortcut** box and press the combination you are thinking
of using. The box shows what you pressed and the line next to it gives the
answer:

| Verdict | Meaning |
|---|---|
| Free | Nothing holds it. |
| Taken by another app | Some other program registered it. |
| Windows shortcut: … | Windows itself uses it, and for what. |
| Reserved by Windows: … | Windows keeps it and no program can ever have it, such as `Win+L`. |
| Used by WinCraft: … | One of your own WinCraft modules holds it. |

The key press is swallowed while the box has focus, so testing `Win+E` tells
you about File Explorer instead of opening it. Press `Esc` to clear the box.

### How it finds out, and what it cannot see

For every combination WinCraft asks Windows for it and immediately gives it
back. If Windows refuses because someone else already has it, the combination
is taken. On top of that there is a built-in list of Windows 11 desktop
shortcuts, which explains the ones Windows keeps for the shell.

**The limitation:** a program that reads keys through a keyboard hook instead
of registering a hotkey cannot be detected by this method, or by any other
method that does not inject code into every running process. AutoHotkey scripts
and PowerToys Keyboard Manager work that way. If a combination shows as free
but does not work for you, that is usually why.

One more thing worth knowing: most Windows shortcuts also fail the probe,
because the shell holds them. Those rows say `Taken by an app` with the Windows
meaning in brackets, for example `(Windows: Open File Explorer)`. That is the
honest answer — the combination really is unavailable — and the bracket tells
you who to blame.

### Settings

| Setting | Meaning |
|---|---|
| `show_free_by_default` | Start with the "Show free" box already ticked. |

## Build

WinCraft needs a stable Rust toolchain for `x86_64-pc-windows-msvc` and the
Windows SDK (for `rc.exe`, which compiles the icon into the exe).

```powershell
cd "E:\codespace\_claude_code\_rde\wincraft_windows_utilities_rust"; cargo build --release
```

The result is `target\release\wincraft.exe`. It is a single file with no
runtime dependencies; copy it anywhere you like.

## Running it

Start `wincraft.exe`. There is no window — look for the WinCraft icon in the
tray (the notification area next to the clock). Starting it a second time does
nothing: the second copy sees the first one and exits.

Right-click (or left-click) the tray icon for the menu:

```
[x] ScreenDimmer               module on/off, the check mark shows the state
        Wake all monitors      an action the module offers
─────
[ ] Start with Windows
    Edit config                opens config.json in Notepad
    Open log                   opens wincraft.log in Notepad
    About WinCraft             version, hotkeys, and which ones failed
─────
    Exit
```

**Start with Windows** writes a `WinCraft` value under
`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`. Unticking it removes the
value again.

If another program already owns one of the hotkeys, WinCraft keeps running.
It shows a tray notification, writes a `WARN` line to the log, and the About
box marks that hotkey `NOT REGISTERED`.

## Files

Both live in `%LOCALAPPDATA%\WinCraft`, which is
`C:\Users\<you>\AppData\Local\WinCraft`.

| File | What it is |
|---|---|
| `config.json` | Your settings. Safe to edit by hand. |
| `wincraft.log` | What the program did. Emptied every time WinCraft starts. |

Set the environment variable `WINCRAFT_DEBUG=1` before starting WinCraft to get
more detail in the log.

## config.json

WinCraft writes this file on every start, filling in any key that is missing.
So you never have to invent a key name: start WinCraft once, then open the file
and change the values.

```json
{
  "start_with_windows": false,
  "modules": {
    "screen_dimmer": {
      "enabled": true,
      "hotkeys": {
        "toggle_monitor_1": "Win+Alt+F1",
        "toggle_monitor_2": "Win+Alt+F2",
        "toggle_monitor_3": "Win+Alt+F3",
        "wake_all": "Win+Alt+F12"
      },
      "settings": {
        "hover_opacity": 0.7,
        "idle_opacity": 1.0
      }
    },
    "shortcut_detector": {
      "enabled": true,
      "hotkeys": {
        "open_window": "Win+Alt+K"
      },
      "settings": {
        "show_free_by_default": false
      }
    }
  }
}
```

| Key | Meaning |
|---|---|
| `start_with_windows` | Mirrors the tray tick box and the registry Run value. |
| `modules.<id>.enabled` | Turn a module off without removing it. |
| `modules.<id>.hotkeys` | Hotkey per named action. See the format below. |
| `modules.<id>.settings` | Options that belong to that module. |

ScreenDimmer settings:

| Setting | Meaning |
|---|---|
| `idle_opacity` | How solid the overlay is normally. `1.0` is fully black. |
| `hover_opacity` | How solid it is while your pointer is on that monitor. `0.7` lets a little through. |

Both are between `0.0` and `1.0`. Values outside that range are pulled back
into it.

WinCraft reads this file only at startup. Edit it, then exit and start WinCraft
again.

### Hotkey format

Modifiers joined to one key with `+`, in any case:

- Modifiers: `Ctrl` (or `Control`), `Alt`, `Shift`, `Win`
- Keys: `A`–`Z`, `0`–`9`, `F1`–`F24`, `Space`, `Esc`, `Tab`, `Enter`,
  `Backspace`, `Insert`, `Delete`, `Home`, `End`, `PageUp`, `PageDown`, `Left`,
  `Right`, `Up`, `Down`, `PrintScreen`, `Pause`, `ScrollLock`, `NumLock`,
  `Numpad0`–`Numpad9`, `NumpadPlus`, `NumpadMinus`, `NumpadMultiply`,
  `NumpadDivide`, `NumpadDecimal`, and the punctuation keys `;` `=` `,` `-` `.`
  `/` `` ` `` `[` `\` `]` `'`

Examples: `Win+Alt+F1`, `Ctrl+Shift+D`, `Win+Space`.

At least one modifier is needed — Windows will not hand a bare key to a
background program. A line WinCraft cannot read is reported in the log and the
module's built-in default is used instead.

## Command line

`wincraft.exe` takes one option:

| Option | What it does |
|---|---|
| `--open-detector` | Opens the ShortcutDetector window as soon as WinCraft starts. |

## Adding a feature

See [CONTRIBUTING.md](CONTRIBUTING.md). A module is one folder, one trait
implementation, and one line in the module list.

## Licence

Not decided yet.
