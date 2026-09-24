# LayoutKeeper

Remembers where your windows were and puts them back after a reboot.

Chrome with "Continue where you left off" brings every window back after a
restart, but in random order: the thumbnails in its taskbar group are
scrambled, windows land on the wrong virtual desktop, and the wrong one is in
front. LayoutKeeper records the layout while you work and restores it.

## What it records

For every window of the programs you list (Chrome by default):

- its name or title, size and position,
- the virtual desktop it is on,
- its place in the taskbar group,
- its place in the front-to-back order.

The layout is saved every 30 seconds when it changed, when Windows shuts down
or restarts, and when you press **Save layout now**. It lives in
`%LOCALAPPDATA%\WinCraft\plugins\layout_keeper.state.json`; the palette command
**Open state file** shows it.

If a program is closed, its last saved windows are kept, so closing Chrome
before a restart loses nothing. At shutdown, programs close their windows
while the layout is being saved, so a list that got shorter at that moment
keeps the complete one from before.

## Restoring

When WinCraft starts, LayoutKeeper waits until the watched programs have
opened their windows: the restore begins once the number of windows has not
changed for 5 seconds, and it gives up after 3 minutes. Then it:

1. puts each program's taskbar thumbnails back in the saved order,
2. moves every window back to its saved virtual desktop,
3. puts the windows back in their saved front-to-back order.

The desktop you are on and the window you are using stay as they are. A
notification says how many windows were found, and the log lists the ones
that were not. **Restore layout** runs the same steps at any time.

Nothing is saved while a restore is waiting, so the scrambled windows of a
fresh start never overwrite the layout they are about to be put back into.

## Naming windows

Chrome can give a window a name: right-click the tab strip and choose
**Name window…**. A named window keeps the same title whatever tab is open,
which is the surest way for LayoutKeeper to recognise it after a restart.
Unnamed windows are recognised by title and position, which works less well
when many windows are maximized.

## Virtual desktops

Windows lets a program read which desktop any window is on, but only lets it
move its own windows. To move Chrome's windows, LayoutKeeper uses the same
undocumented interfaces as
[MScholtes' VirtualDesktop](https://github.com/MScholtes/VirtualDesktop)
(MIT), for Windows 11 24H2 and later. Microsoft changes them between builds,
so LayoutKeeper checks them against the desktop list in the registry the
first time it needs them. If anything does not match, desktop moves are off
for the session, the plugin page says why, and everything else still works.

## Taskbar order

Windows has no setting for the order of thumbnails inside a taskbar group,
and no way to read it. LayoutKeeper keeps its own list of the wanted order for
each program and makes the taskbar match it. New windows join at the end, as
Windows adds them; a window that stays closed for one save interval is
dropped from the list.

The easiest way to change the order is **Arrange windows** (`Win+Alt+A`, the
palette, or the button on this page): drag a window up or down the list, or
select it and use **Move up** and **Move down**. Every change goes to the
taskbar at once and is saved.

Without the mouse, bring a window to the front and press **Move window
left in taskbar** or **Move window right in taskbar**. The group's thumbnails
are rebuilt in the new order, which is visible for a moment.

The taskbar shows one desktop's windows at a time, so a move swaps the window
with its neighbour on the same desktop. Windows cannot rebuild a button
without also bringing its window to the current desktop, so a move only
rebuilds the buttons of the desktop you are on. A restore rebuilds them all
and then sends every window back to its desktop.

### Why not drag in the taskbar itself

The mouse way to reorder is the Arrange windows list. Dragging a thumbnail
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
| Programs | `chrome.exe` | Exe names to watch, separated by commas. |
| Restore when WinCraft starts | on | Restore once the windows have settled after start. |
| Wait for windows (seconds) | 5 | How long the window count must hold still. |
| Give up after (minutes) | 3 | Stop waiting if it never does. |
| Save every (seconds) | 30 | How often the layout is saved when it changed. |
| Restore which window is in front | on | Also restore the front-to-back order. |
