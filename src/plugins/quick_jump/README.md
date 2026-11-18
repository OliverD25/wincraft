# QuickJump

Keep up to ten windows in a numbered list and jump back to any of them
from anywhere, on any virtual desktop. It works like Harpoon in Neovim,
for windows.

## Keys

Press **Win+`** (the key left of 1), then one more key:

| Then | What it does |
|---|---|
| 1 to 9, 0 | Jump to the window at number 1 to 10 |
| ` (Win+` twice) | Open Quick access |
| A | Add the window in front at the first free number |
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

The list holds up to ten windows, one per number. A window keeps its
number until you move it, so a digit keeps reaching the same window. A
number can be empty:

- **A** adds the window in front at the lowest free number. With ten
  windows the tip says "Quick access is full", and nothing changes. A
  window that is in the list already keeps its number, and the tip says
  "Already number N".
- **Alt and a digit** puts the window in front at that number, also when
  fewer windows are pinned. A free number simply takes it. At a taken
  number the window there moves down one, and so do the taken numbers
  right after it, up to the first free number. When no number after it is
  free, those windows move up one instead, into the nearest free number
  before it. A window that is in the list already leaves its old number
  free first, so moving 2 to 9 leaves 2 empty.
- **U**, or Del in Quick access, unpins a window. Its number is free now,
  and no other window moves.

Every change shows a notice with **Undo**; Ctrl+Z works too while the
notice is on screen. The notice stays longer while the mouse rests on it.

Before 0.20.74 the pins sat in ten fixed slots. A file from then is read
with each pin at its old number and saved again in the new form; a copy of
the old file is kept as
`%LOCALAPPDATA%\WinCraft\plugins\quick_jump.state.before-0.20.74.json`.

## Jumping

A jump brings the window to the front and restores it when it is
minimized. When it is on another virtual desktop, Windows switches to that
desktop. Nothing is moved. A jump to the window that is in front already
does nothing.

When the window is not open, QuickJump starts the app and switches to its
first window once it opens. Each pin can turn this off in its details.

A pin of a Claude chat is different: starting the Claude app opens only its
main window, never the closed chat. So the jump opens the chat by its link
(`claude://code/continue?session=local_...`), and the Claude app shows it in
its main window. QuickJump presses nothing in the app. When the pin has no
link the app takes, it starts the app as before and says why in the log.

ClaudeDeck can pin a chat too: its right-click menu has **Give a Quick
access number** (with **First free number** and the numbers 1 to 9 and 0)
and **Remove the Quick access number**. The chat need not have a window
open; its pin then holds the chat's name as its title and opens the chat by
its link. The same rules as **Put at number** apply, with the same notice
and Undo. The two plugins do not call each other: QuickJump writes what
each number holds to `core::quick_pins` whenever its pins change and takes
ClaudeDeck's asks from there on its own window's thread. ClaudeDeck shows a
chat's number in a small box on its row.

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
app's id) and a part of the window's title. A pin of a Claude chat window
also remembers the chat (its session id), so a renamed chat is still found.
When the window closes and opens again, also after a restart, QuickJump
looks for it in this order:

1. for a pin of a Claude chat, the window that shows that chat,
2. the same window, while it is still open (for a chat pin, only while the
   window does not show another chat),
3. a window of the same app whose title contains the pin's text (the first
   one, if there are several),
4. the app's only window.

A chat pin skips 3 and 4 once WinCraft knows which chat each Claude window
shows, so another chat whose title holds the old name is never taken for
it. With no match the card says "not open", and a chat pin's card says
"opens the chat".

The title follows the window. While a pinned window is open and QuickJump
is sure it is that pin's window (the window was pinned, shows the pin's
chat, or is the only window whose title holds the pin's text), the pin's
title text becomes the window's current title. After a close it looks for
the latest title, not the one from the day you pinned it. It never learns
from rule 4, which can be another window. Empty titles, "Untitled" and
"New Tab" are not kept. A title you type in the details ("Title contains")
stays as you typed it. An older pin of a Claude chat learns its chat the
same way, while its window still shows it.

The pins are saved in
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
| Ctrl+Left, Ctrl+Right | Move the window to the number before or after; a window there takes its old number |
| Ctrl+Up, Ctrl+Down, drag | Move the window onto another card: it takes that card's number, and the cards between shift one |
| Del, middle click | Unpin the window; the others keep their numbers |
| F2, click the selected card | Open the details |
| R, on a card that is not open | Re-pin: the window that was in front when Quick access opened takes this number, with the card's label |
| Right click, Menu key, Shift+F10 | Open the card's menu |
| Esc | Close the menu, or stop a drag; then the details, then Quick access |

A dragged card lifts above the others and follows the mouse. The other
cards slide aside to open a gap where it will land, and it slides into the
gap when you let go. Esc during a drag puts everything back. Ctrl+arrow
moves slide the same way, and an unpinned card fades out while the cards
after it move up on screen; their numbers stay. Free numbers have no card,
so the numbers under the cards can skip, for example 1, 2, 5. With the
Windows setting "Animation effects" off, the cards change places at once.

The card's menu has "Jump to it", "Put at number" (1 to 10, free or
taken, with the card's own number marked; the same rule as Alt and a
digit), "Re-pin the window I was in" (R, only while the
window is not open), "Edit details" (F2), "Unpin" (Del) and, only while the
window is open, "Close the window". That one closes the real
window, as its own close button would, and only when you pick it. Every
move and unpin shows the notice with Undo (Ctrl+Z), whichever way you made
it.

Re-pin mends a pin whose window is gone for good. Go to the window, open
Quick access, pick the card that says "not open" and press R. The window
takes that number and keeps the card's label. A window that was pinned at
another number moves here and leaves that number free. The header shows the
hint "R Re-pin" only while the picked card is not open.

The details give a pin a label, which the tip and Quick access show instead
of the window's title, and set how QuickJump finds the window: the app, and
the text its title contains. A line under the title says how many open
windows match now. Changes take effect with Done; Esc drops them.

## The palette

Type "jump" in the palette for one row per pinned window, such as "Jump to
3 · Claude". "Add a window to Quick access…" lists the open windows to
choose from, and adds the one you pick at the first free number. "Pin this window…" and
"Open quick access" are there too.
