# QuickJump

Keep up to ten windows in a numbered list and jump back to any of them
from anywhere, on any virtual desktop. It works like Harpoon in Neovim,
for windows.

## Keys

Press **Win+`** (the key left of 1), then one more key:

| Then | What it does |
|---|---|
| 1 to 9, 0 | Jump to window 1 to 10 in the list |
| ` (Win+` twice) | Open Quick access |
| A | Add the window in front to the end of the list |
| Alt+1 to Alt+9, Alt+0 | Put the window in front at that number |
| U | Unpin the window in front |
| Esc | Cancel |

0 is number 10, as with Win+number. You can keep Win held while you press
the next key, or let it go first. QuickJump waits 3 seconds for it.

While it waits, a tip at the bottom of the screen shows the keys you
pressed, the windows in the list with their numbers, and what the other
keys do. A thin bar along its bottom runs out with the time. The tip never
takes the keyboard from the app in front, and clicks go through it. When
the wait ends, the tip says how: the window you jumped to, "Nothing
assigned to X" for a key with no job (that key does not reach the app
either), or "Cancelled".

Every shortcut can be changed in Settings, Hotkeys. "Pin this window…"
has no keys at first; give it keys there and then a digit puts the window
in front at that number. "Open quick access" has a row there too, with no
keys at first.

### When Win+` does nothing

Windows Terminal uses Win+` to show its quake window on many PCs, and only
one app can own a shortcut. WinCraft then warns that the key is taken.
WinCraft does not take the key from Windows Terminal. To free it:

1. Open Windows Terminal.
2. Press Ctrl+, to open "Settings".
3. Click "Actions" in the list on the left.
4. Find "Toggle quake mode" (it may also be called "Show/Hide the
   Terminal window" or "globalSummon"), with the keys "Win+`".
5. Click the pencil next to it, then the bin, to delete the keys.
6. Click "Save" at the bottom right.
7. Turn QuickJump off and on again in WinCraft's Settings, Plugins, or
   restart WinCraft. The warning should not come back.

Or give QuickJump other keys in WinCraft's Settings, Hotkeys.

## The list

The list holds up to ten windows, in order. A window's number is its place
in the list:

- **A** adds the window in front to the end. With ten windows the tip says
  "Quick access is full", and nothing changes. A window that is in the list
  already keeps its number, and the tip says "Already number N".
- **Alt and a digit** puts the window in front at that number: the window
  that had the number and the ones after it move down one. A window that is
  in the list already moves from its old place. A number past the end of
  the list puts the window at the end.
- **U**, or Del in Quick access, unpins a window. The windows after it move
  up one, so there is never a gap.

Every change shows a notice with **Undo**; Ctrl+Z works too while the
notice is on screen. The notice stays longer while the mouse rests on it.

Before 0.20.74 the pins sat in ten fixed slots. The first start of 0.20.74
turns them into the list, in slot order without the empty slots, and keeps
a copy of the old file as
`%LOCALAPPDATA%\WinCraft\plugins\quick_jump.state.before-0.20.74.json`.

## Jumping

A jump brings the window to the front and restores it when it is
minimized. When it is on another virtual desktop, Windows switches to that
desktop. Nothing is moved. A jump to the window that is in front already
does nothing.

When the window is not open, QuickJump starts the app and switches to its
first window once it opens. Each pin can turn this off in its details.

### On another monitor

When the window comes up on another monitor than the one you were using,
QuickJump points to it. A soft glow in the theme's accent colour shows
around the window for under a second, and a small pill on your monitor
says where it went, at the edge that faces the other monitor: "Telegram ·
Right monitor →". Both fade out by themselves. Clicks go through them, and
they never take the keyboard. The monitor you were using is the one of
the window in front before the jump, of Quick access when you jumped from
it, or of the mouse when the desktop was in front. A window that QuickJump
has to start first gets no hint. With the Windows setting "Animation
effects" off, the hint shows without fading.

To turn it off:

1. Open WinCraft's Settings, click "Plugins", then click "QuickJump".
2. Turn off "Point to a window on another monitor".

## How a pin finds its window

A pin remembers the app (its program, and for Store apps and web apps the
app's id) and a part of the window's title. When the window closes and
opens again, also after a restart, QuickJump looks for it in this order:

1. the same window, while it is still open,
2. a window of the same app whose title contains the pin's text (the first
   one, if there are several),
3. the app's only window.

With no match the card says "not open". The pins are saved in
`%LOCALAPPDATA%\WinCraft\plugins\quick_jump.state.json`.

## Quick access

Open it with Win+` twice, from the tray menu or from the palette. It shows
the pinned windows as cards in rows of four, in list order, each with a
live picture of the window, its title and app, and the key that reaches it
under the card.

It opens in the middle of the monitor under the mouse. To open it where you
last left it instead:

1. Open WinCraft's Settings, click "Plugins", then click "QuickJump".
2. Turn off "Open Quick access in the middle of the screen".
3. Open Quick access, drag it by its title bar to where you want it, and
   close it. It opens there from now on.

The spot is kept in `%LOCALAPPDATA%\WinCraft\plugins\quick_jump.spot.json`.
A spot on a monitor that is gone or smaller is moved onto the nearest screen.

| Key | What it does |
|---|---|
| ↑ ↓ ← → | Select a card |
| Enter, double-click, 1 to 0 | Jump, and close Quick access |
| Ctrl+arrows, drag | Move the window to another place |
| Del | Unpin the window; the ones after it move up |
| F2, click the selected card | Open the details |
| Esc | Close the details, then Quick access |

The details give a pin a label, which the tip and Quick access show instead
of the window's title, and set how QuickJump finds the window: the app, and
the text its title contains. A line under the title says how many open
windows match now. Changes take effect with Done; Esc drops them.

## The palette

Type "jump" in the palette for one row per pinned window, such as "Jump to
3 · Claude". "Add a window to Quick access…" lists the open windows to
choose from, and adds the one you pick to the end. "Pin this window…" and
"Open quick access" are there too.
