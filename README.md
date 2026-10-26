# WinCraft

A small, modular tray utility for Windows 11, in the style of Microsoft
PowerToys. One program owns the tray icon, the autostart entry, the global
hotkeys, a command palette and a settings window. Features are written as
**plugins** that plug into one Rust trait, so adding a feature never means
touching the Win32 plumbing or writing any UI code.

WinCraft ships seven plugins: **ScreenDimmer**, **ShortcutDetector**,
**LayoutKeeper**, **WindowDeck**, **LanguageIndicator**, **WindowNamer** and
**QuickJump**,
and a palette
that also finds apps, open windows, files and folders on every disk (through
Everything), your own bookmarks, sums, web searches and terminal commands.
The newest version is on the
[Releases page](https://github.com/OliverD25/wincraft/releases/latest), and
[wincraft.luminart.app](https://wincraft.luminart.app) shows it with
screenshots.

![The WinCraft command palette: a search box over WinCraft's own commands](docs/palette.png)

## What you get

- **Command palette** — `Win+Alt+P` opens a search box on the monitor your
  pointer is on. Type a few letters, press Enter. It finds WinCraft's commands,
  apps (Start menu, Store and installed), open windows, files and folders
  (through Everything) and your bookmarks, works out sums as you type, and
  with a prefix it browses folders, asks an AI website or searches the web.
  Beside the list, an inspector shows what the selected row is and what it
  can do. See
  [Command palette](#command-palette).
- **Settings window** — General, Plugins, Hotkeys, Layout,
  Bookmarks, Plugin Store and About, with a page per plugin: its options, its
  hotkeys, its config file and its README.
- **Hotkeys** — every command the palette lists can have a
  shortcut of one key press or two in a row (a chord, such as `Win+Alt+K`,
  then `S`). See [Hotkeys](#hotkeys).
- **Themes** — Classic (the default), Fluent, Terminal, Paper and High
  contrast, each in dark and light. In Fluent the palette uses Windows 11
  glass; it turns solid on battery saver or when transparency effects are
  off.
  Settings → Themes shows a tile with a small picture of each
  one. Point at a tile and every WinCraft window shows that
  theme; move away and it goes back; click to keep it. Mode (System, Dark or
  Light) is a separate choice. While Windows high contrast is on, WinCraft
  uses High contrast.
- **Plugin store** — what exists, what this copy has, and what needs a newer
  WinCraft. The Plugins page and the store show plugins as tiles that you can
  search (`Ctrl+F`) and filter by category; the arrow keys move between tiles,
  `Enter` opens one, and on the Plugins page `Space` switches it on or off.
- **Tray icon** — open the palette, open settings, one main action per
  enabled plugin, and quit. Everything else is in the settings window.

## Install

Download `wincraft.exe` from the
[latest release](https://github.com/OliverD25/wincraft/releases/latest/download/wincraft.exe)
and run it. There is no installer and nothing to unpack: it is a single file
that writes its settings to `%LOCALAPPDATA%\WinCraft`. The release also has
`SHA256SUMS.txt`, so you can check the download. WinCraft is not code-signed
yet, so Windows SmartScreen asks once before the first start. See the
[code signing policy](#code-signing-policy) and [privacy](#privacy).

To build it yourself instead, you need [rustup](https://rustup.rs) and the
Windows SDK (for `rc.exe`, which compiles the icon into the exe). The Rust
version is pinned by `rust-toolchain.toml`, so cargo fetches the right
compiler on first use:

```powershell
git clone https://github.com/OliverD25/wincraft; cd wincraft; cargo build --release
```

The result is `target\release\wincraft.exe`.

### Installing, once

Run this once, from a normal terminal (not one started from another app,
such as Claude Desktop, whose file redirection would hide the copy):

```
wincraft.exe --install
```

It copies the exe to `%LOCALAPPDATA%\Programs\WinCraft\wincraft.exe`, closes
the WinCraft that is running, adds WinCraft to the Start menu, points the
"Start with Windows" logon task at the new copy and starts it. Your settings
stay where they are. It prints each step and keeps them in
`%LOCALAPPDATA%\WinCraft\install.log`. It is safe to run again.
`wincraft.exe --uninstall` removes the shortcut and the logon task and leaves
the program and your settings.

### Updates

WinCraft asks GitHub once a day whether a newer release exists, and About's
"Check for updates" asks at once. A newer one shows a tray notification once,
an "Update to …" item in the tray menu, and its release notes on the About
page. Nothing changes until you click **Update now** or run the palette
command **Update WinCraft**. That command checks at once and, when a newer
release exists, installs it straight away, the same way Update now does. When
there is nothing newer, a notification says "WinCraft is up to date"; when
this copy cannot update itself, it says why.

Update now downloads `wincraft.exe` and `SHA256SUMS.txt` from the release,
checks the checksum, puts the new exe in place and restarts WinCraft. A bar
under the release on the About page shows how far it is: step 1 downloads,
step 2 installs. If the new version does not run for 30 seconds without
crashing, WinCraft ends it,
puts the old exe back, starts the old version again and says "Update to …
failed, kept …". Only the installed copy updates itself: a WinCraft run from
anywhere else says to run `--install` first. "Check for updates daily" on the
About page switches the daily check off.

## Plugins

### ScreenDimmer

Covers a monitor with a black overlay you can click straight through, which
turns partly see-through under your pointer, or turns it off over DDC/CI,
like its power button, without Windows moving your windows. A monitor that
ignores standby is dimmed to brightness 0 instead, and one without DDC/CI is
only covered. What the Shift key does to each monitor (Auto, Off, Dim,
Cover) is on the plugin's page. The mouse pointer cannot go onto a monitor
that is off, and quitting WinCraft brings every monitor back. Monitors are
counted from the left.
[Full README](src/plugins/screen_dimmer/README.md)

| Hotkey | What it does |
|---|---|
| `Win+Alt+F1` / `F2` / `F3` | Cover / uncover the left / centre / right monitor |
| `Win+Alt+Shift+F1` / `F2` / `F3` | Turn off / bring back the left / centre / right monitor |
| `Win+Alt+F12` | Bring back every monitor |

### ShortcutDetector

Shows which global key combinations are already taken on this PC, and tells you
about any combination you press before you assign it.
[Full README](src/plugins/shortcut_detector/README.md)

| Hotkey | What it does |
|---|---|
| `Win+Alt+S` | Open the shortcut detector |

It cannot see programs that read keys through a keyboard hook, such as
AutoHotkey scripts or PowerToys Keyboard Manager. Nothing can, short of
injecting code into every running process, and the window says so.

### LayoutKeeper

Remembers which virtual desktop and which monitor each window is on, the
order of the thumbnails in its taskbar group and which window is in front,
and puts them all back after a reboot. A window whose monitor is unplugged
stays where it is. It works for any app (Chrome, Claude, Telegram,
anything with a window); the **Never touch** setting leaves chosen apps
alone. Windows pinned to all desktops and windows Windows places on no
desktop are never moved between desktops. While it puts windows back after a
reboot, a small panel asks you to wait. The **Layout** page in the settings
window lists every saved window with its desktop, which you can change, and
shows LayoutKeeper's recent activity. Windows of one app that share a title
are marked, because a restore cannot tell them apart.
[Full README](src/plugins/layout_keeper/README.md)

| Hotkey | What it does |
|---|---|
| `Win+Alt+L` | Restore the saved layout |
| `Win+Alt+J` | Save the layout now |
| `Win+Alt+[` | Move the front window one place left in its taskbar group |
| `Win+Alt+]` | Move the front window one place right in its taskbar group |

Moving windows between desktops uses undocumented Windows interfaces, ported
from [MScholtes' VirtualDesktop](https://github.com/MScholtes/VirtualDesktop)
(MIT). They change between Windows builds; if they do not answer as expected,
LayoutKeeper turns desktop moves off for the session and says so on its page.

To reorder with the mouse, use WindowDeck. Dragging inside the
taskbar's thumbnail flyout is not possible from outside Explorer on Windows 11
24H2, because the flyout exposes no accessible thumbnail items (probed on
2026-09-25). The Windhawk mod "Taskbar Thumbnail Reorder" still works for hand
reorders, but WinCraft cannot see its changes, and the next restore replaces
them.

### WindowDeck

Shows all windows of an app in a row above the taskbar, with a live picture
of each, laid out by monitor like your desk; a link or `D` shows the other
desktops. Pick, move and close them from the keyboard, or drag them into
order, to another desktop or to another monitor. With LayoutKeeper on, the
order you set is saved with the layout. Until WinCraft 0.20.25 it
was LayoutKeeper's "Arrange strip"; the first start after the update moves
its settings and hotkey over with the same values.
[Full README](src/plugins/window_deck/README.md)

| Hotkey | What it does |
|---|---|
| `Win+Alt+A` | Wait 3 seconds for a taskbar number and open WindowDeck for that button; press `Win+Alt+A` again for the app in front. If no number comes within 0.7 s, a number shows over each taskbar button |

Inside WindowDeck, a number opens the window whose card shows it (0 is
10), and Ctrl and a number open WindowDeck for that taskbar button. Ctrl+W
closes the picked window, and Ctrl+N opens a new window of the app.

**Open WindowDeck with Win+1…9** (off until you switch it on in
WindowDeck's settings): Win and a number open WindowDeck for that taskbar
button, even for an app with one window; the same number again with Win
still held picks the next window. A pinned app that is not running starts
as before, and Win+Shift, Win+Ctrl and Win+Alt with a number stay with
Windows. With the switch off, Win+number works exactly as before.

**WindowDeck instead of Windows' taskbar preview** (off until you switch it
on in WindowDeck's settings, the tray menu or the palette): resting the
mouse on the taskbar button of an app with 2 or more windows opens
WindowDeck above that button instead of Windows' own preview, after 0.15 s
(the **Hover delay** setting, 0.05 to 1.5 s). Apps with one window keep
Windows' preview. Win+number and Alt+Tab work exactly as before. Switching it
off gives Windows' preview back at once.

### LanguageIndicator

Shows which input language you switched to, in the middle of the screen, the
way macOS does: switch from Ukrainian to English and a translucent panel shows
**UK → EN** with "English (United States) · US" under it, then fades out. It
never takes the focus and lets clicks through. It has no hotkeys; the palette
command **Show current input language** shows it on demand.
[Full README](src/plugins/language_indicator/README.md)

### WindowNamer

Gives any window a name of your own: three Chrome windows called "Mail",
"Research" and "Bank" instead of three page titles. The taskbar thumbnail,
Alt+Tab and the title bar show the name, and it stays when the app changes
its title. Names are saved and put back on the same window after WinCraft or
the app restarts. Rename from the hotkey, or with **Rename…** on a card in
WindowDeck; the palette command **Clear all names** takes every name
away. LayoutKeeper keeps working with the app's own title, so a rename never
makes it lose a window. [Full README](src/plugins/window_namer/README.md)

| Hotkey | What it does |
|---|---|
| `Win+Alt+E` | Rename the active window: the palette opens with the current name selected; Enter sets the name, Enter on an empty box clears it |

### QuickJump

Pins up to ten windows to the digits, like Harpoon in Neovim, and jumps back
to any of them from anywhere, on any virtual desktop. While it waits for the
digit, a strip at the bottom of the screen shows the ten slots. A jump
switches desktop when it has to and restores a minimized window; nothing is
moved. A pin finds its window again by app and title, also after a restart,
and starts the app when it is not open. Every change shows a notice with
Undo. The pin list (the tray menu, the palette, or the jump keys twice)
shows every slot with a live picture and lets you reorder, rename and tune
the pins. [Full README](src/plugins/quick_jump/README.md)

| Hotkey | What it does |
|---|---|
| `Win+Alt+Q`, then a digit | Jump to the window in that slot |
| `Win+Alt+Shift+Q`, then a digit | Pin the window in front to that slot |
| `Win+Alt+Q` twice | Open the pin list |

### Host hotkeys

| Hotkey | What it does |
|---|---|
| `Win+Alt+P` | Open the command palette |

### Hotkeys

Settings → Hotkeys lists every command the palette lists, grouped
by plugin. Click a row (or select it and press Enter or F2): the hotkey
dialog opens and records at once, so just press the keys. The same dialog
changes the palette hotkey on General and a plugin's hotkeys on its page;
there the keys in their box are the control (click it, or Tab to it and
press Enter or Space). In the dialog, Enter saves, Esc cancels, and
Backspace clears the keys; Backspace again removes the hotkey. A hotkey
needs Win, Ctrl or Alt with its key; F13 to F24 may be used alone. A
second key within 3 seconds makes a chord: `Win+Alt+K, S` means
Win+Alt+K, then S. While a chord waits, a small WinCraft line at the bottom
of the screen says so; the second key may come with the first press's
modifiers still held. Any other key ends the wait and goes to the app in
front, and so does Esc or 3 seconds without a key.

Before anything is saved, the dialog checks the keys in this order: keys
Windows keeps (Win+L, Ctrl+Alt+Delete, Ctrl+Shift+Esc, Win+number, Alt+Tab
and similar) are refused; a combination cannot be both a hotkey and the
start of a chord; keys another WinCraft command has can be taken with Move
here, which leaves that command without a hotkey; keys another app or a
Windows shortcut holds are refused. Nothing changes until Enter or Move
here. Search by keys finds what uses a combination. In the palette, a
command's hotkey shows at the right of its row, and Ctrl+K opens the same
dialog over the palette.

Each row with keys shows if they work: a green dot means working, a
warning triangle means another app (or another WinCraft command) holds
them, and a grey dot means the plugin is off. A line under the search box
sums it up, such as "12 working · 1 taken by another app".

## Command palette

The palette is a list with an inspector beside it. The inspector follows the
cursor: a live picture of a window, the row's title and words, what WinCraft
knows about it, every action with its key (the first is what Enter does), and
where the row comes from. → moves into the actions, ↑ ↓ pick one, Enter runs
it, ← or Esc goes back. On a monitor narrower than 1100 px the palette is the
list alone, and → opens the inspector over it.

With nothing typed, the list starts with **Recent and suggested**: up to five
open windows and apps you picked often. Open settings, the plugin store and
Update WinCraft are not listed there; type their names to find them, or use
the gear at the far left of the footer (Ctrl+,). Ctrl+P opens Settings →
Plugins.

Without a prefix, the palette searches five things at once and lists the best
matches first: WinCraft's own commands (every plugin action, every plugin
on/off switch, the host's items), your apps, the windows that are open on
any virtual desktop, your [bookmarks](#bookmarks) and the files and folders
that [Everything](#files-and-folders-with-everything) finds. Apps come from three places, and a program found in
more than one is listed once, from the first: the shortcuts in your Start menu,
the Start menu's app list (this is where Store apps such as Windows Terminal
are), and the programs Windows lists as installed that have no shortcut. The
second line of a row says which: the Start menu folder, "Store app" or
"Installed program". For a program with no shortcut, WinCraft picks its file
from the install folder, skipping uninstallers, updaters and helper programs.

A prefix typed at the start sends the search to one source only. It turns into
a chip before the search box; Backspace in the empty box removes it. Two
sources also have a short word: `w` and a space searches the open windows,
`a` and a space the apps (all of them, A to Z, while nothing else is typed).

| Prefix | What it searches | Enter | Shift+Enter | Tab |
|---|---|---|---|---|
| none | Commands, apps and open windows | Run, open or switch to | | Put the title in the box; on a File Explorer window, browse its folder under `/` |
| `<` | Open windows only, the ones Alt+Tab shows, by title or program; a window renamed with WindowNamer by its name or its app's own title | Switch to it, restoring it if minimized | | Put the title in the box; on a File Explorer window, browse its folder under `/` |
| `/` | Drives, then one folder: `/C:\Users\`. Typing after the folder also finds matches in all the folders below it (nearest first; hidden folders and links are skipped, and a very big tree is read only in part) | Open the file or folder | Show it in Explorer, selected | Go into the folder |
| `.` | Files and folders on every disk, through Everything (or the Windows index when it is not running): `.pswds` | Open it | Show it in Explorer, selected (Ctrl+Enter copies the path) | Go into the folder under `/` |
| `'` | Your bookmarks, all of them when nothing is typed | Open it | Show it in Explorer, selected (Ctrl+Enter copies the target) | Go into the folder under `/` |
| `=` | A sum: `+ - * / ^ %` and brackets; `×` and `÷` work too. Without `=`, a finished sum such as `12*3` is answered at the top of the list | Copy the answer (Ctrl+C copies the sum with it, Ctrl+Enter pastes the answer into the window you were in) | | Put the answer in the box |
| `>` | A command for the terminal shell chosen in settings (WSL bash unless you change it), then your earlier commands | Run it here, with its output in the palette | | Put the earlier command in the box |
| `!` | WinCraft's own commands only: the host's items, then each plugin's actions and on/off switches. All of them when nothing is typed | Run it | | Put the command's name in the box |
| `?` | Alone: this list of prefixes. With words: Ask AI, then a web search. The **Ask AI** pill in the search bar does the same | Pick the prefix, or open the AI website chosen in settings with your question (Ctrl+Enter searches the web instead) | | Pick the prefix |

Ask AI opens the website you choose in Settings → General → **Ask AI site**
(Claude unless you change it; ChatGPT, Perplexity, Copilot or an address of
your own) in your default browser, with the question filled in. WinCraft
sends the question nowhere itself and needs no account or key.

When nothing matches, the palette says so and offers **Try instead**: ask AI
about what you typed (Enter), or search the web for it (Ctrl+Enter).

In the calculator `%` after a number is a percentage (`200 * 15%` is 30) and
between two numbers is the remainder (`10 % 3` is 1). A comma works as the
decimal mark.

Results you pick often move up. Every time you run an app, a WinCraft
command, a window or a folder or file with Enter (Tab does not count), it
gains a point that halves every 14 days. The points only reorder results
that match about equally well: a clearly better match stays ahead and a
result named exactly what you typed stays first. With nothing typed, the
results you picked lead their own group. The calculator, web search and
terminal are not ranked this way.

WinCraft also remembers what you typed before you ran a result. Type the
same text again, or the start of it, and that result comes first under
**Best match**, marked "often picked", so Enter runs it at once. The
inspector says how often you picked it for that text this month and when
last; **Forget this pick** (Ctrl+Delete) stops it coming first for that text. If you picked two results for the same text, the one picked
more often and more recently wins. When a WinCraft command has your text at
the start of a word (`upd` finds "Update WinCraft"), the WinCraft group is
shown above the apps. The counts and the typed texts are kept in
`palette_usage.json` next to `config.json`; **Learn from my picks** in
Settings → General switches all of this off, and **Clear usage history** forgets
them.

The keys: ↑ ↓ move, Enter runs, Shift+Enter does the second action, Tab
completes, Esc closes. The selected row shows the word for what Enter does,
and the footer shows up to five keys for that row, Esc last. On a folder or
file row Ctrl+B bookmarks it, and Ctrl+Delete removes a bookmark. On a
command, Ctrl+K changes its hotkey.

On a window row Enter switches to it, Shift+→ moves it to the next monitor,
Ctrl+Enter opens it in WindowDeck and Ctrl+W asks it to close, the way
its own close button does. The inspector shows its program and process id,
its monitor and virtual desktop, how much memory its program uses, and a
**Show on all desktops** switch. Ctrl+Space, Shift+click or Ctrl+A mark
windows; the inspector then lists the marked ones and its actions apply to
all of them: arrange in WindowDeck, move to the next monitor, minimise
(Ctrl+M), close. Esc clears the marks before it closes the palette. Nothing
happens to a window until you press one of these keys.

### Files and folders with Everything

WinCraft does not index your disks. [Everything](https://www.voidtools.com)
(voidtools, free) already does, and answers in a few milliseconds, so WinCraft
asks it. Start Everything (version 1.4 or newer; the separate message
interface of the 1.5 alpha is not used yet) and type part of a name: without a prefix the palette adds up to six
rows, folders first, then files. After `.` the list is only files and
folders, up to 50 rows, with the best name match first. A name that equals
what you typed leads, then names that start with it, then the rest; rows
you pick often move up. Everything's own search words work too, such as
`ext:pdf invoice` or `*.log`.

Without Everything, WinCraft asks the Windows Search index instead, so file
search still works on a PC where Everything is not installed. It is slower
(a search that matches little can take most of a second, so the palette waits
up to 0.8 s) and it finds only what Windows Search has indexed, which by
default is your user folders, not whole disks. Each word you type must be in
the file name, and a word with a `\` in it is looked for in the whole path.
Rows from the index have "Windows index" at the start of the second line.
Everything's search words do not work there. If the Windows Search service is
off, `.` says "File search needs Everything or Windows Search" and the log has
one line about it.

Searches go out 120 ms after you stop typing, from a thread of their own, so
a slow Everything never freezes the palette. A search needs two letters.
Rows from the recycle bin and the Recent shortcuts folder are left out. If
Everything is not running, `.` says "Searching the Windows index" in one
row, and the blended list stays silent. **File search: Everything, then
Windows index** in Settings → General switches all file search off, and
**Use the Windows index when Everything is not running** switches only the
fallback off.

### Bookmarks

A bookmark is a short phrase for a folder, a file or a web address. Type the
phrase and its bookmark leads the list, above everything else; a phrase that
starts with what you typed comes before ordinary matches. `'` alone lists all
of them.

- **Add one:** press Ctrl+B on a folder or file row (from `/`, `.`, the
  blended list, or an open File Explorer window in the `<` list) or on a full
  web address typed after `?`. The palette asks for the phrase, with the name
  in lower case already filled in; Enter saves. A phrase that already opens
  something else asks you to press Enter again before it is replaced.
- **Add by hand:** the palette command **Add bookmark…** (type `add book`)
  asks for the place first, with the folder of the File Explorer window that
  was in front, then for the phrase. The Bookmarks page in the settings
  window has a form for it too, and lists every bookmark with Open, Edit and
  Delete. A place that does not exist is saved with a warning.
- **Use one:** Enter opens it, Shift+Enter shows it in Explorer, Ctrl+Enter
  copies its target, Tab goes into a folder bookmark under `/`.
- **Remove one:** Ctrl+Delete on it, then Ctrl+Delete again within five
  seconds. Any other key cancels.

They are kept in `bookmarks.json`, a plain list of `phrase`, `target` and
`created`.

Until you have a bookmark, a small tip over the palette's footer says so when
a folder or file row is selected: **Ctrl + B saves this folder as a bookmark.**
It retires after three openings, when you click its cross, or when you save
your first bookmark. After a save the palette stays open for four seconds (or
until your next key) and says what to type to open it. The Bookmarks page
explains the steps while it is empty. The **Show tips** switch on the General
page turns every tip off, and **Show all tips again** brings them back.

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
palette_usage.json     how often each palette result was picked
bookmarks.json         your palette bookmarks
plugins\
  screen_dimmer.json   one file per plugin
  shortcut_detector.json
  layout_keeper.json
  language_indicator.json
  window_namer.json
  quick_jump.json
  layout_keeper.state.json   the saved window layout, written by LayoutKeeper
  window_namer.names.json    the saved window names, written by WindowNamer
  quick_jump.state.json      the pinned windows, written by QuickJump
  layout_keeper.state.prev.json   the layout from before a browser crash or restart
  layout_keeper.state.shutdown.json   the layout at the last shutdown or quit
cache\                 the last plugin index fetched from GitHub
wincraft.log           what the program did in this run; an update adds to it
wincraft.prev.log      the log of the run before, kept at each new start
install.log            what the last --install or --uninstall did
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
  "theme": "classic",
  "mode": "system",
  "palette_hotkey": "Win+Alt+P",
  "search": {
    "prefixes": { "bookmarks": "'", "calc": "=", "commands": "!", "files": ".", "paths": "/", "terminal": ">", "web": "?", "windows": "<" },
    "web_url": "https://www.google.com/search?q={query}",
    "terminal_shell": "WSL bash",
    "terminal_custom": "",
    "terminal_history": true,
    "rank_by_use": true,
    "everything": true,
    "file_search_fallback": true
  },
  "updates": {
    "check_daily": true,
    "last_check": 1792260953,
    "latest": "v0.16.0"
  },
  "tips": {
    "show": true,
    "state": { "bookmark-ctrl-b": { "shown": 2, "done": false } }
  }
}
```

| Key | Meaning |
|---|---|
| `start_with_windows` | Mirrors the "Start with Windows" switch. It is a logon task in Task Scheduler; see below. |
| `theme` | `classic`, `fluent`, `terminal`, `paper` or `high-contrast`. An unknown word gives Classic. Before 0.20.11 this key held the mode; such a file is read as Classic with that mode. |
| `mode` | `system`, `light` or `dark`. An unknown word gives System. |
| `palette_hotkey` | What opens the command palette. |
| `shortcuts` | Shortcuts of palette commands that have no hotkey of their own, by command id, such as `"wincraft.open-settings": "Win+Alt+K, O"`. Only what you set is here. A plugin's own hotkeys stay in its file (`plugins/<id>.json`, `hotkeys`), where an empty text means no shortcut. |
| `search.prefixes` | The prefix of each palette search source. Change one to move it, set it to `""` to switch that prefix off. A source left out keeps its built-in prefix. Use symbols: a letter as a prefix would catch every search that starts with it. |
| `search.web_url` | The web search address, with `{query}` where the words go. It must start with `https://` or `http://`. |
| `search.terminal_shell` | What `>` runs commands in: `WSL bash`, `PowerShell 7`, `Windows PowerShell`, `Command Prompt` or `Custom`. |
| `search.terminal_custom` | For `Custom`: a command line with `{cmd}` where the command goes and optionally `{cwd}` for the folder, such as `C:\msys64\usr\bin\bash.exe -lc {cmd}`. Without `{cmd}` WSL bash is used. |
| `search.terminal_history` | Whether commands run from the palette are remembered. |
| `search.rank_by_use` | **Learn from my picks**: whether results you pick often move up, and picks and the text typed for them are kept in `palette_usage.json`. |
| `search.ask_ai_site` | The website Ask AI opens: `Claude`, `ChatGPT`, `Perplexity`, `Copilot` or `Custom URL`. |
| `search.ask_ai_custom` | For `Custom URL`: the address with `{query}` where the question goes. It must start with `https://` or `http://`. |
| `search.everything` | Whether files and folders are searched at all, in the blended list and after `.`: with Everything when it is running. |
| `search.file_search_fallback` | Whether the Windows Search index answers file searches while Everything is not running. |
| `updates.check_daily` | Whether GitHub is asked once a day for a newer release. |
| `updates.last_check` | When GitHub last answered, in seconds since 1970. WinCraft writes it. |
| `updates.latest` | The newest release tag seen; the tray says once per new tag that it is there. WinCraft writes it. |
| `tips.show` | Whether the small in-app tips show. It is the "Show tips" switch on the General page. |
| `tips.state` | What each tip has done, by id: `shown` times and `done` once it was dismissed or its feature used. WinCraft writes it; "Show all tips again" on the General page empties it. |

The prefixes are read when WinCraft starts, so restart it after changing
them. The other `search` keys are also on the General page of the settings
window and apply at once.

### Starting with Windows

The "Start with Windows" switch creates a logon task named `WinCraft` in
Task Scheduler, for your account only, with normal rights and a 5 second delay
after you sign in. It needs no administrator rights. Every time WinCraft starts
with the switch on, it checks the task in the background and makes it again if it
is missing or points at another `wincraft.exe`, for example after you moved the
folder. If Windows refuses to create the task, WinCraft writes the older
`WinCraft` value in `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` instead
and says so in the log. When the task exists, that value is removed.

To remove the task by hand, switch it off in Settings, or run this in any
terminal:

```
schtasks /Delete /TN WinCraft /F
```

With `WINCRAFT_INSTANCE=<suffix>` set, the task is named `WinCraft.<suffix>`.

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

Two presses joined by `, ` make a chord: `Win+Alt+K, S` or
`Win+Alt+K, Win+Alt+S`. One-press text reads exactly as before.

A bare key without modifiers is accepted too, such as `F13` or `Pause`. Keep
it to keys you never type with, because WinCraft takes that key from every
other program while it runs. If another program already owns a combination,
WinCraft keeps running, shows a tray notification and marks the hotkey
"taken by another app" on the plugin's page and on the Hotkeys page.

## Command line

| Option | What it does |
|---|---|
| `--open-palette` | Opens the command palette at startup. |
| `--open-arrange` | Opens WindowDeck at startup. |
| `--open-settings` | Opens the settings window at startup. |
| `--open-detector` | Opens the ShortcutDetector window at startup. |
| `--quit` | Asks the running WinCraft to shut down cleanly, as the tray's Quit does, and waits up to 10 s. Exit code 0: it closed; 1: none was running; 2: still running. |
| `--restarted` | Windows adds this itself when it starts WinCraft again after an update closed it. The log says so. |
| `--install` | Copies this exe to `%LOCALAPPDATA%\Programs\WinCraft`, adds the Start menu shortcut, points the logon task at it and starts it; see [Installing, once](#installing-once). Exit code 0: done; 1: failed; 2: refused inside another app's package. |
| `--uninstall` | Closes WinCraft and removes the shortcut and the logon task. The program and the settings stay. |
| `--updated-from <version>` | WinCraft's own update adds this when it starts the new version. |
| `--update-failed <version>` | Added when an update was rolled back and the old version starts again. |
| `--write-plugin-index` | Regenerates `plugins.json`. For contributors. |

Set `WINCRAFT_DEBUG=1` for a more detailed log.

## Contributing

WinCraft is open source, and contributions are welcome. Its goal and
principles are in [CONSTITUTION.md](CONSTITUTION.md); the second one is that
anyone should be able to add a useful feature in an afternoon.

- **New here?** Pick an issue labelled
  [good first issue](https://github.com/OliverD25/wincraft/labels/good%20first%20issue).
- **Have an idea or found a bug?** Open an issue; the form asks only what is
  needed.
- **Writing a plugin?** See [CONTRIBUTING.md](CONTRIBUTING.md). A plugin is one
  folder, one trait implementation, a README, and one line in the plugin list.
  You describe your settings; WinCraft draws and saves them.

Everyone follows the [Code of Conduct](CODE_OF_CONDUCT.md).

## Privacy

WinCraft sends nothing about you anywhere: no telemetry, no account, no
analytics. It goes on the network only in these cases:

- **The daily update check**, which asks GitHub
  (`api.github.com/repos/OliverD25/wincraft`) for the latest release. Switch it
  off in Settings → About ("Check for updates daily").
- **Update now**, which downloads `wincraft.exe` and `SHA256SUMS.txt` from
  this repository's releases, only when you click it.
- **The Plugin Store page**, which reads the plugin list from this repository
  when you open it.
- **Search and Ask AI in the palette**, which open your browser at the site
  you chose, with your text, only when you press Enter.

Your settings, layouts and palette history stay in `%LOCALAPPDATA%\WinCraft`
on your PC.

## Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io/),
certificate by [SignPath Foundation](https://signpath.org/). The application
is pending: until it is accepted, releases are unsigned, and each release's
`SHA256SUMS.txt` is the way to check a download.

- Every release is built from a git tag by GitHub Actions
  ([release workflow](.github/workflows/release.yml)); nobody builds a release
  by hand.
- Only WinCraft's own code is signed. It contains no third-party binaries;
  the Rust crates it uses are compiled from source in the same build.
- Team roles:
  - Committers and reviewers: [OliverD25](https://github.com/OliverD25)
  - Approvers (approve each signing request): [OliverD25](https://github.com/OliverD25)
- Changes from other contributors are reviewed in a pull request before they
  are merged.
- All team members use multi-factor authentication on GitHub and SignPath.

## Licence

MIT. See [LICENSE](LICENSE).

The interface uses Segoe UI from your own Windows installation, and the
Terminal theme uses Cascadia Mono (or Consolas) from it; nothing of them is
shipped. WinCraft bundles four fonts, each under the SIL Open Font License
1.1, with their licence files in [assets/fonts](assets/fonts):
[Selawik](https://github.com/microsoft/Selawik) by Microsoft, used when Segoe
UI is missing, [JetBrains Mono](https://github.com/JetBrains/JetBrainsMono)
for paths and code, and [Lora](https://github.com/cyrealtype/Lora-Cyrillic)
and [Cormorant](https://github.com/CatharsisFonts/Cormorant) for the Paper
theme, both from [Google Fonts](https://github.com/google/fonts).
