# ShortcutDetector

Shows which global key combinations are already taken on this PC, and tells you
about any combination you press before you assign it to anything.

## What it does

Open the window with `Win+Alt+Q` or from the tray menu. It scans when it opens,
and again whenever you press Refresh or F5. Nothing runs in the background while
the window is closed.

The table lists one row per combination:

| Column | What it tells you |
|---|---|
| Shortcut | The combination, e.g. `Win+Alt+F1`. |
| Status | `Taken by an app`, `Windows`, `WinCraft`, or `Free`. |
| Owner / meaning | What the Windows shortcut does, or which WinCraft plugin and action holds it. |
| Source | How WinCraft knows: `probe`, `windows list`, or `wincraft`. |

Only taken combinations are shown by default. Tick **Show free** to add the
rest, use **Search** to filter, and the **Modifiers** dropdown to see one
modifier set at a time. Click a column heading to sort. **Copy** puts the rows
you can see on the clipboard as tab-separated text.

## Checking one shortcut

Click the **Check a shortcut** box and press the combination you have in mind.
The key press is swallowed while the box has focus, so testing `Win+E` tells you
about File Explorer instead of opening it. Press `Esc` to clear the box.

| Verdict | Meaning |
|---|---|
| Free | Nothing holds it. |
| Taken by another app | Some other program registered it. |
| Windows shortcut: … | Windows itself uses it, and for what. |
| Reserved by Windows: … | Windows keeps it and no program can ever have it, such as `Win+L`. |
| Used by WinCraft: … | One of your own WinCraft plugins holds it. |

## Hotkeys

| Action | Default |
|---|---|
| Open shortcut detector | `Win+Alt+Q` |

## Settings

| Setting | Meaning |
|---|---|
| `show_free_by_default` | Start with the "Show free" box already ticked. |

## How it finds out, and what it cannot see

For every combination WinCraft asks Windows for it and immediately gives it
back. If Windows refuses because someone else already has it, the combination is
taken. On top of that there is a built-in list of Windows 11 desktop shortcuts,
which explains the ones Windows keeps for the shell.

**The limitation:** a program that reads keys through a keyboard hook instead of
registering a hotkey cannot be detected by this method, or by any method that
does not inject code into every running process. AutoHotkey scripts and
PowerToys Keyboard Manager work that way. If a combination shows as free but
does not work for you, that is usually why.

Most Windows shortcuts also fail the probe, because the shell holds them. Those
rows say `Taken by an app` with the Windows meaning in brackets, for example
`(Windows: Open File Explorer)`. The combination really is unavailable, and the
bracket tells you who to blame.
