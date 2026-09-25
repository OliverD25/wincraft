# LayoutKeeper

Remembers where your windows were and puts them back after a reboot. It
works for any app: Chrome, Claude, Telegram, anything with a window.

Apps that bring their windows back after a restart (Chrome with "Continue
where you left off", Telegram and many others that start with Windows) do it
in random order: the thumbnails in their taskbar group are scrambled,
windows land on the wrong virtual desktop, and the wrong one is in front.
LayoutKeeper records the layout while you work and restores it.

## Which windows

Every app window, the way Alt+Tab counts them: visible windows with a
title, not tool windows, dialogs or windows an app keeps hidden. Windows on
other virtual desktops count too.

**Programs** is `*` (every app) by default; it can also be a list of exe
names, such as `chrome.exe, telegram.exe`. **Never touch** lists apps to
leave alone, such as `telegram.exe` if Telegram should stay where it opens.
WinCraft's own windows are always left out. Settings from before 0.8 that
still had the old default `chrome.exe` were changed to `*` once; any other
list you wrote was kept.

## What it records

For every window:

- its name or title, size and position,
- the virtual desktop it is on,
- the monitor it is on (the one it returns to when minimized),
- its place in the taskbar group,
- its place in the front-to-back order, across all apps.

The layout is saved every 30 seconds when it changed, when Windows shuts down
or restarts, and when you press **Save layout now**. It lives in
`%LOCALAPPDATA%\WinCraft\plugins\layout_keeper.state.json`; the palette command
**Open state file** shows it. Its `saved` field is your local time with its
offset from UTC, such as `2026-09-25T02:48:11+03:00`. Files written before
WinCraft 0.6.1 have UTC time (`...Z`) there and are still read.

If an app is closed, its last saved windows are kept, so closing Chrome or
Telegram before a restart loses nothing. At shutdown, programs close their windows
while the layout is being saved, so a list that got shorter at that moment
keeps the complete one from before.

### After a browser crash or restart during the day

When Chrome crashes or restarts, its windows come back in a scrambled order.
LayoutKeeper does not save that scrambled order over the good one:

- If a taskbar group loses more than half of its windows, or all of them,
  timer saves keep that group's saved windows. If the windows come back
  within the restore time (3 minutes by default), it was a restart; saving
  stays held until the number of windows has not changed for six times the
  settle time (30 seconds by default). If they do not come back, you closed
  them on purpose, and saving goes on as normal.
- If more than a third of a group's windows are replaced by new windows
  between two saves, saving is held the same way until the number is steady.
- Before the new windows of a held group are first written into the file,
  the file is copied to `layout_keeper.state.prev.json`, next to it. To go
  back to the layout from before the crash, quit WinCraft, copy that file
  over `layout_keeper.state.json` and start WinCraft again.

Other groups are saved as normal during a hold, and **Save layout now** is
never held. The log says when a hold starts and ends.

## Restoring

Apps open their windows at their own pace after you sign in, so each app's
taskbar group is restored on its own: once that group's number of windows
has not changed for 5 seconds, as long as that happens within 3 minutes of
WinCraft's start. An app that starts a little later, such as Telegram, is
still covered; an app you open an hour later is left where you put it. Apps
with nothing saved are not touched. For each group, LayoutKeeper:

1. puts the taskbar thumbnails back in the saved order,
2. moves every window back to its saved virtual desktop,
3. puts the restored windows back in their saved front-to-back order.

The desktop you are on and the window you are using stay as they are. A
notification says how many windows were found, and the log lists the ones
that were not. **Restore layout** runs the same steps at any time.

**Never moved between desktops:** a window pinned to all desktops, and a
window Windows does not place on any desktop (WhatsApp's, for example). They
still get their place in the taskbar.

While a group waits for its restore, saves keep its saved windows, so the
scrambled windows of a fresh start never overwrite the layout they are about
to be put back into. Everything else is saved as normal.

## Naming windows

Chrome can give a window a name: right-click the tab strip and choose
**Name window…**. A named window keeps the same title whatever tab is open,
which is the surest way for LayoutKeeper to recognise it after a restart.
Unnamed windows are recognised by title and position, which works less well
when many windows are maximized. An app with a single window in its taskbar
group, such as Telegram whose title shows the open chat, is recognised by
being the only one.

## Virtual desktops

Windows lets a program read which desktop any window is on, but only lets it
move its own windows. To move other apps' windows, LayoutKeeper uses the same
undocumented interfaces as
[MScholtes' VirtualDesktop](https://github.com/MScholtes/VirtualDesktop)
(MIT), for Windows 11 24H2 and later. Microsoft changes them between builds,
so LayoutKeeper checks them against the desktop list in the registry the
first time it needs them. If anything does not match, desktop moves are off
for the session, the plugin page says why, and everything else still works.

## Taskbar order

Windows has no setting for the order of thumbnails inside a taskbar group,
and no way to read it. LayoutKeeper keeps its own list of the wanted order for
each taskbar group and makes the taskbar match it. New windows join at the end, as
Windows adds them; a window that stays closed for one save interval is
dropped from the list.

### Groups

Windows groups taskbar buttons by app, not by program: Chrome's installed web
apps (Gemini, for example) get buttons of their own, next to Chrome's. They
do because each window carries an app ID, and LayoutKeeper groups windows by
that same ID, so each taskbar group keeps its own order. A window without an
ID is grouped by its program's path, as Windows does. **Programs** and
**Never touch** take exe names, so `chrome.exe` covers Chrome and all its web
apps. A group is named by the app's own name ("Telegram", "Claude"), else
the description in its program file, else the file name.

### Arrange windows

`Win+Alt+A` (or the palette, or the button on this page) opens a strip above
the taskbar with a live picture of every window in the front window's
taskbar group, whatever the app, like the taskbar's own thumbnails. A list at
the top switches to every other app that has windows, most windows first. There is one row per virtual
desktop: this desktop first, then the others under their names. The pictures
are drawn by Windows itself (DWM thumbnails) and stay live while the strip
is open; windows on other desktops have pictures too. A window Windows
cannot picture shows its name instead.

The pictures are small (16:9, about the size of the taskbar's own), and a
desktop with many windows wraps onto more lines instead of running off the
screen. The strip takes at most 90 % of the screen's width and 60 % of its
height; beyond that the mouse wheel scrolls it.

- **Click** a picture to switch to that window; the strip closes.
- **Drag** a picture to a new place. Every change goes to the taskbar at once
  and is saved. Dropping it in another desktop's row moves the window to
  that desktop.
- **Right-click** for **Close window** and **Move to desktop**. Closing is
  only offered here, never on a key.
- **Keyboard:** ← and → pick a window, Ctrl+← and Ctrl+→ move it, Enter
  switches to it, Esc closes the strip. The strip also closes when you click
  elsewhere.

The taskbar only shows the current desktop's buttons, and Windows cannot
rebuild a button without bringing its window to the current desktop. So a
new order for windows on other desktops is saved at once but reaches their
taskbar at the next restore; the strip says so when more than one desktop is
shown.

### Without the mouse

Without the mouse, bring a window to the front and press **Move window
left in taskbar** or **Move window right in taskbar**. The group's thumbnails
are rebuilt in the new order, which is visible for a moment.

The taskbar shows one desktop's windows at a time, so a move swaps the window
with its neighbour on the same desktop. Windows cannot rebuild a button
without also bringing its window to the current desktop, so a move only
rebuilds the buttons of the desktop you are on. A restore rebuilds them all
and then sends every window back to its desktop.

### Why not drag in the taskbar itself

The mouse way to reorder is the Arrange windows strip. Dragging a thumbnail
inside the taskbar's own flyout is not possible from outside Explorer on
Windows 11 24H2: the flyout exposes no accessible thumbnail items, so another
program cannot tell which thumbnail is under the pointer (probed on
2026-09-25). The Windhawk mod "Taskbar Thumbnail Reorder" still works for
reordering by hand, but it changes the order inside Explorer where WinCraft
cannot see it, so the next restore replaces its changes with LayoutKeeper's
saved order.

## Hotkeys

| Action | Default |
|---|---|
| Restore layout | `Win+Alt+L` |
| Save layout now | `Win+Alt+J` |
| Move window left in taskbar | `Win+Alt+[` |
| Move window right in taskbar | `Win+Alt+]` |
| Arrange windows | `Win+Alt+A` |

## Settings

| Setting | Default | Meaning |
|---|---|---|
| Programs | `*` | `*` for every app, or exe names separated by commas. |
| Never touch | (empty) | Exe names to leave alone, separated by commas. |
| Restore when WinCraft starts | on | Restore once the windows have settled after start. |
| Wait for windows (seconds) | 5 | How long the window count must hold still. |
| Give up after (minutes) | 3 | Stop waiting if it never does. |
| Save every (seconds) | 30 | How often the layout is saved when it changed. |
| Restore which window is in front | on | Also restore the front-to-back order. |
