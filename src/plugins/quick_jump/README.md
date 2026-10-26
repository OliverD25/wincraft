# QuickJump

Pin up to ten windows to the number keys and jump back to any of them from
anywhere, on any virtual desktop. It works like Harpoon in Neovim, for
windows.

## Keys

- **Win+Alt+Shift+Q, then a digit** pins the window in front to that slot.
- **Win+Alt+Q, then a digit** jumps to the window in that slot.
- **Win+Alt+Q twice** opens the pin list.

1 to 9 are slots 1 to 9, and 0 is slot 10, as with Win+number. You can keep
Win and Alt held while you press the digit, or let them go first. QuickJump
waits 3 seconds for the digit. Esc, a click, or any other key stops the wait
and changes nothing.

Every shortcut can be changed in Settings, Keyboard shortcuts. The pin list
has a row there too, "Open pin list", with no keys at first.

## The strip

While QuickJump waits for the digit, a strip at the bottom of the screen
shows the ten slots: the app's icon and name, and a line that says what the
digit would do. A thin line along its top shrinks while the time runs out.
A slot whose window is on another desktop shows that desktop, such as "D3".

The strip never takes the keyboard from the app in front, and clicks go
through it.

## Jumping

A jump brings the window to the front and restores it when it is
minimized. When it is on another virtual desktop, Windows switches to that
desktop. Nothing is moved. A jump to the window that is in front already
does nothing.

When the window is not open, QuickJump starts the app and switches to its
first window once it opens. Each pin can turn this off in its details.

## Pinning

A digit pins at once and a short notice says what happened. Pinning to a
full slot replaces what was there, and a window that is pinned to another
slot moves. Every change shows a notice with **Undo**; Ctrl+Z works too
while the notice is on screen. The notice stays longer while the mouse rests
on it.

## How a pin finds its window

A pin remembers the app (its program, and for Store apps and web apps the
app's id) and a part of the window's title. When the window closes and
opens again, also after a restart, QuickJump looks for it in this order:

1. the same window, while it is still open,
2. a window of the same app whose title contains the pin's text (the first
   one, if there are several),
3. the app's only window.

With no match the slot shows "not open". The pins are saved in
`%LOCALAPPDATA%\WinCraft\plugins\quick_jump.state.json`.

## The pin list

Open it with Win+Alt+Q twice, from the tray menu or from the palette. It
shows all ten slots with a live picture of each window.

| Key | What it does |
|---|---|
| ↑ ↓ | Select a slot |
| Enter, double-click | Jump, and close the list; on an empty slot, pick a window |
| Ctrl+↑ ↓, drag | Move the pin up or down |
| 1 to 0 | Move the pin to that slot, swapping with a pin there |
| Del | Remove the pin |
| F2 | Open the details |
| Esc | Close the details, then the list |

The details give a pin a label, which the strip and the list show instead of
the app's name, and set how QuickJump finds the window: the app, and the
text its title contains. A line under the title says how many open windows
match now. Changes take effect with Done; Esc drops them.

**Pin a window…** on an empty slot lists the open windows to choose from.

## The palette

Type "jump" in the palette for one row per pinned slot, such as "Jump to 3 ·
Claude". "Pin this window…" and "Open pin list" are there too.
