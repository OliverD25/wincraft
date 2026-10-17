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
WinCraft's own windows are always left out, and so are Windows' own helper
processes that can show a titled window but are not apps: `rundll32.exe`
(the Sound control panel, for one), `dllhost.exe`, `ShellExperienceHost.exe`,
`StartMenuExperienceHost.exe`, `SearchHost.exe`, `TextInputHost.exe`,
`LockApp.exe` and `ShellHost.exe`, whatever the two settings say. Settings
and other Store apps are still tracked. Settings from before 0.8 that
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
  within 3 minutes, it was a restart; saving stays held until the number of
  windows has not changed for 30 seconds. If they do not come back, you
  closed them on purpose, and saving goes on as normal.
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
taskbar group is restored on its own, and each group has its own clock. The
clock starts when the group's first window appears, not when WinCraft
starts. The group is restored once its number of windows has not changed for
15 seconds. If the number keeps changing, the group is restored 4 minutes
after its first window with the windows that are open then.

- A group that has opened no window is waited for 30 minutes after WinCraft
  starts. Chrome is the usual reason: Windows can start it without a window,
  and it opens its windows only when you first open it.
- After a group is restored it is watched for 5 more minutes. If more of its
  windows come in during that time and their number holds still, the group is
  restored a second time. The second pass moves the new windows to their
  saved desktops and rebuilds the taskbar order. Windows the first pass
  already placed stay on the desktop they are on then, so a window you moved
  yourself is not taken back, and the front-to-back order is not touched.
  Then the group is left alone.
- An app you open later than that is left where you put it. Apps with
  nothing saved are not touched.

For each group, LayoutKeeper:

1. puts the taskbar thumbnails back in the saved order,
2. moves every window back to its saved virtual desktop,
3. puts the restored windows back in their saved front-to-back order.

The desktop you are on and the window you are using stay as they are. A
notification says how many windows were found, and the log lists the ones
that were not. There is one notification for each restore run, for its first
restored group; the other groups and the second passes are in the log and in
the status line on this page. The log also says which apps never opened a
window and which were restored only because of the 4-minute limit.
**Restore layout** runs the same steps at any time.

### The please-wait panel

While windows are being put back, a small panel at the bottom right of the
main monitor asks you to wait, so you do not start moving windows at the same
time. It lists each app that has opened windows and is not done yet ("Chrome:
12 of 22 opened, waiting for the rest", "Claude: moving 4 windows"), and
which apps are done. It never stays for an app that has opened no window. It
cannot be clicked and never takes the focus. When nothing is waiting any more
it says "Layout restored: 22 Chrome windows, 6 apps" for 4 seconds and
fades. If some apps never opened a window within the wait, it says
"Layout restored, 2 apps skipped", and the tray icon's tooltip names them.

While a restore runs, the tray icon's tooltip shows the same progress, for
example "WinCraft - restoring layout: Chrome 12/22 settling; waiting for 3
apps to open". With **Show restore progress** on, the panel's last line takes
the place of the notification balloon; with it off there is no panel and no
tooltip, and the balloon stays.

**Never moved between desktops:** a window pinned to all desktops, and a
window Windows does not place on any desktop (WhatsApp's, for example). They
still get their place in the taskbar.

While a group waits for its restore, and during the 5 minutes after it,
saves keep its saved windows, so the scrambled windows of a fresh start never
overwrite the layout they are about to be put back into. Everything else is
saved as normal.

## The Layout page

**Layout** in the settings window (or **Saved window layout** in the
palette) lists everything LayoutKeeper has saved, grouped by app with the
same names as the Arrange strip. For each saved window it shows:

- its title (the WindowNamer name, when an open window has one),
- whether it is **open now**, or **closed since** the last day it was seen
  open,
- the monitor chip (Left, Right) when it is not on the main monitor,
- the virtual desktop it comes back to, in a dropdown, and
- **Forget**, for a closed window. An open window is saved again at once, so
  forgetting it would do nothing; the button is off for it.

Choosing another desktop saves it at once and applies at the next restore. If
the window is open, it also moves there now, the same way **Move to desktop**
does in the strip. A window saved on a desktop that has since been removed
shows **desktop removed** in amber. A restore leaves such a window where it
is, and says so in the log; pick a desktop to fix the entry.

The header has **Restore now** (the same as `Win+Alt+L`), **Save now** (the
same as `Win+Alt+J`) and the time of the last save. The list refreshes every
two seconds while the page is open.

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
the taskbar with a live picture of every window in one app's taskbar group,
whatever the app, like the taskbar's own thumbnails. It opens on the app
whose taskbar button is under the mouse, on any monitor's taskbar, else on
the active window's app: point at a button and press the hotkey to arrange
that app without switching to it first. A list at
the top switches to every other app that has windows, most windows first. There is one row per virtual
desktop: this desktop first, then the others under their names. The pictures
are drawn by Windows itself (DWM thumbnails) and stay live while the strip
is open; windows on other desktops have pictures too. A window Windows
cannot picture shows its name instead. A window that is not on the primary
monitor has a small label above its picture saying which monitor it is on
("Left", "Right"; "Monitor 4" and so on with four or more).

The pictures are small (16:9, about the size of the taskbar's own), and a
desktop with many windows wraps onto more lines instead of running off the
screen. The strip takes at most 90 % of the screen's width and 60 % of its
height; beyond that the mouse wheel scrolls it.

- **Rest the mouse** on a picture for a quarter of a second to see that
  window where it really is: its monitor goes dark and the window's live
  picture is drawn on top, even for a window on another desktop. The window
  itself is not raised or moved, and the peek goes away when the mouse
  leaves the picture. **Preview window on hover** turns this off.
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
| Never touch | (empty) | Exe names to leave alone, separated by commas. Windows' helper processes are always left alone. |
| Restore when WinCraft starts | on | Restore once the windows have settled after start. |
| Wait for windows (seconds) | 15 | How long a group's window count must hold still. |
| Wait for apps to open (minutes) | 30 | How long to wait for an app that has opened no window yet. |
| Save every (seconds) | 30 | How often the layout is saved when it changed. |
| Show restore progress | on | The please-wait panel and the tray tooltip while windows are put back. |
| Restore which window is in front | on | Also restore the front-to-back order. |
| Preview window on hover | on | Resting the mouse on a picture in the Arrange strip shows that window on its monitor. |
