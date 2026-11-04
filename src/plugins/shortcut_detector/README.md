# ShortcutDetector

Shows which global key combinations are already taken on this PC, and tells you
about any combination you press before you assign it to anything.

## What it does

Open the window with `Win+Alt+S` or from the tray menu. It scans when it opens,
and again whenever you press Refresh or F5. Nothing runs in the background while
the window is closed.

The table lists one row per combination:

| Column | What it tells you |
|---|---|
| Shortcut | The combination, e.g. `Win+Alt+F1`. |
| Status | A short tag: `App` (another app holds it), `Windows`, `WinCraft`, or `Free`. |
| Meaning | What the shortcut does, when that is known. |
| Owner | Who holds it: `Windows`, `WinCraft`, an app, or `unknown app`. |
| Source | How WinCraft knows: `probe`, `windows list`, `WinCraft`, `shortcut file`, `known apps`, or `press test`. |

Only taken combinations are shown by default. Switch on **Show free** to add
the rest. Type in the search box (`Ctrl+F`) to filter by shortcut, meaning or
owner.
Pick **Ctrl**, **Alt**, **Shift** or **Win** to see only the combinations that
use that key, or **All**. Click a column heading to sort, and again to turn the
order round. `↑` and `↓` move through the rows. **Copy list** (or `Ctrl+C`)
puts the rows you can see on the clipboard as tab-separated text. `Esc` closes
the window.

The line at the bottom says how many combinations were checked and how many
are taken.

## Checking one shortcut

Click the box next to **Check a shortcut** (or reach it with `Tab` and press
`Enter` or `Space`), then press the combination you have in mind. The box
listens until you click somewhere else, so you can try several. The keys are
held back from Windows while it listens, so testing `Win+E` tells you about
File Explorer instead of opening it. Press `Esc` to stop listening and clear
the box.

| Answer | Meaning |
|---|---|
| Free · … | Nothing holds it. |
| App · Explorer shortcut: Notepad++ — Open Notepad++ | A shortcut file in the Start menu or on the desktop has this key. |
| App · ShareX, probably — Capture a region | A running app uses this key by default. You may have changed it there. |
| App · unknown app | Some other program registered it, and WinCraft cannot tell which. |
| Windows · … | Windows itself uses it, and for what. |
| Windows · reserved, no app can have it: … | Windows keeps it and no program can ever have it, such as `Win+L`. |
| WinCraft · … | One of your own WinCraft plugins holds it. |

## Hotkeys

| Action | Default |
|---|---|
| Open shortcut detector | `Win+Alt+S` |

Before WinCraft 0.20.16 the default was `Win+Alt+Q`, which QuickJump uses
now. A stored `Win+Alt+Q` is moved to `Win+Alt+S` once, at the first start;
keys you chose yourself stay as they are.

## Settings

| Setting | Meaning |
|---|---|
| `show_free_by_default` | Start with the "Show free" switch already on. |

## How it finds out, and what it cannot see

For every combination WinCraft asks Windows for it and immediately gives it
back. If Windows refuses because someone else already has it, the combination is
taken. On top of that there is a built-in list of Windows 11 desktop shortcuts,
which explains the ones Windows keeps for the shell.

Windows has no way to say which program holds a key. So the Owner comes
from two places, read only when the window scans:

- **Shortcut files.** A shortcut in the Start menu or on the desktop can have
  a "Shortcut key" (right-click it, Properties). Explorer registers that key,
  so this answer is certain. Source `shortcut file`.
- **Known apps.** A short list of apps and the keys they use out of the box
  (`known_apps.rs`). An app is named only while it runs, and only as
  "probably", because you may have changed the key in that app. Source
  `known apps`.

**The limitation:** a program that reads keys through a keyboard hook instead of
registering a hotkey cannot be detected by this method, or by any method that
does not inject code into every running process. AutoHotkey scripts and
PowerToys Keyboard Manager work that way. If a combination shows as free but
does not work for you, that is usually why.

Most Windows shortcuts also fail the probe, because the shell holds them. Those
rows say `Windows`, with Owner `Windows` and the plain meaning, for example
`Open File Explorer`. The combination really is unavailable. When a shortcut
file or a running known app holds a key from the Windows list instead (a
launcher on `Alt+Space`, say), the row says `App` and names it.
