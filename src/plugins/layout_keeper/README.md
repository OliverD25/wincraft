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
before a restart loses nothing.

## Naming windows

Chrome can give a window a name: right-click the tab strip and choose
**Name window…**. A named window keeps the same title whatever tab is open,
which is the surest way for LayoutKeeper to recognise it after a restart.
Unnamed windows are recognised by title and position, which works less well
when many windows are maximized.

## Hotkeys

| Action | Default |
|---|---|
| Save layout now | `Win+Alt+J` |
