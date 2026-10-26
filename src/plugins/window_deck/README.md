# WindowDeck

Shows all windows of an app in a row above the taskbar, with a live picture
of each, to pick, move and close them from the keyboard or the mouse. In
WinCraft 0.20.25 and earlier it was part of LayoutKeeper, as "the Arrange
strip". The first start after the update moves its settings and its hotkey from
LayoutKeeper to WindowDeck with the same values, and switches WindowDeck on
if you used the strip (its hotkey was set or the hover strip was on).

## Opening it

`Win+Alt+A` waits 3 seconds for a taskbar number. Press a number (with or
without Win and Alt still held) to open WindowDeck above that button, with
that button's window picked; with more than ten buttons, two digits typed
quickly make a two-digit number. If no number comes within 0.7 seconds, a
number shows over each button of the main taskbar, with a line at the
bottom of the screen; if you type the number sooner, nothing shows at all.
Press `Win+Alt+A` again to open WindowDeck for the app in front, or for the
app whose taskbar button is under the mouse. Esc, any other key, a click or
3 seconds without a key end the wait and nothing opens. The palette, the
tray menu and the button on this page open WindowDeck at once.

WindowDeck shows the windows LayoutKeeper sees, so LayoutKeeper must be on;
if it is off, WindowDeck says so. Switching WindowDeck off frees `Win+Alt+A`,
stops the hover strip and closes WindowDeck if it is open.

WindowDeck shows a live picture of every window in one app's taskbar group,
whatever the app, like the taskbar's own thumbnails. The list at the top left switches
to every other app that has windows, most windows first; the right end of
the toolbar counts the monitors, desktops and the app's windows. The
pictures are drawn by Windows itself (DWM thumbnails) and stay live while
the strip is open; windows on other desktops have pictures too. A window
Windows cannot picture shows its name instead.

When the app has only one or two windows on this desktop, the strip opens
as a small popup instead: the app's name (click it, or press Tab, to pick
another app), those windows in their monitor's shape, and a button such as
"+2 on Desktop 5" when the app has windows on other desktops. → from the
last window, or the button, opens those to the right, faded until picked,
under their desktop's name and monitor; ← from the first of them goes
back. Enter switches to the picked window, on its own desktop.

The switch next to the app list picks one of two views, and the strip keeps
the one you picked last (`arrange_view` in the settings file). Until you
pick one, it is **By monitor** with two or more monitors and **Flat** with
one.

Both views show **only this desktop** when they open, every time. Under its
windows, a small link reads "+ 2 other desktops" (the count is the other
desktops that have windows of the app; there is no link when there are
none). Click it, or press **D**, to show the other desktops; the link then
reads "Hide other desktops". The strip is shorter while they are hidden. A
window moved to a hidden desktop with Shift+↑ or Shift+↓ opens them, to show
where it went, and so does a picture dragged onto the link and held there for
a moment (then it can be dropped in one of their rows). They stay open until
you hide them or close the strip. The right-click **Move to desktop** works
as always.

### By monitor

**By monitor** is a map of your desk. There is one band per virtual
desktop: this desktop first, its name in the accent colour, then (when
shown) the others in Task View order, slightly faded. A desktop with none of the app's windows
is just its header. Inside a band there is one column per monitor, from left
to right as the monitors stand. A column is as wide as its windows need to sit
in one row (never narrower than 200 pixels and never wider than half of the
band); the column with the most windows takes all the space that is left, and
the main monitor's column wins a tie. The size of the monitors does not
matter. If the columns still do not fit, that band scrolls sideways. Monitors
stacked one above the other get a row of columns each. A column's label gives the monitor's place ("Left",
"Centre", "Right"; "Monitor 4" and so on with four or more), its resolution,
a **Primary** chip for the main monitor and how many windows it has; the
monitor's own name is its tooltip. A monitor with no windows, while other
monitors have some, folds into a narrow dashed rail (40 pixels) with its name
written upward and the word "empty". While you drag a picture, every rail
opens to a full column with a dashed box ("Drop a window here") and closes
again when you let go. A strip that shows one monitor only, or has no windows
at all, keeps ordinary columns.

- **Drag** a picture to another monitor's column to move the window there
  now, at the same size and place relative to the screen (a maximized
  window is maximized there). Dropping it in another desktop's band moves
  it to that desktop too. A 2-pixel accent bar shows where it will land, and
  the order goes to the taskbar and is saved at once.
- A window that is on another monitor than in the last saved layout has a
  **Moved** chip in its title line: you moved it since the last save (saves
  run every 30 seconds), or a restore has not put it back yet.
- A monitor that is unplugged but still has saved windows of the app keeps
  its column, marked **(disconnected)**, with those windows faded; they are
  on another monitor until it is back. Drag one to another column to give it
  a new place.
- When the main taskbar lists every window, the primary monitor's column
  also shows a dashed **stand-in** for each window on another monitor, at
  its taskbar number, so the column reads in taskbar order. A stand-in can
  be picked with the keys, clicked to switch to its window, and dragged like
  a picture. Dragged within the primary column, or moved with Ctrl+← and
  Ctrl+→, it only changes the window's taskbar number: the window stays on
  its monitor. Dragged to another monitor's column or another desktop's
  band, or moved with Ctrl+Shift and the arrows, it moves the window as
  dragging its own picture does.
- **Keyboard:** the arrow keys only pick a window: ← and → the one before
  or after, ↑ and ↓ the nearest one on the line above or below, across
  columns and desktops. Ctrl+← and Ctrl+→ move the picked window within its
  column; Ctrl+Shift+← and Ctrl+Shift+→ move it to the end of the next
  monitor's column (a rail counts, and it opens while the window lands on
  it); Ctrl+Shift+↑ and Ctrl+Shift+↓ move it to the next desktop, on the same
  monitor (or the main one when its own is unplugged). Enter switches to the
  window, a number switches to the window whose card shows that number
  (0 is 10), Ctrl and a number open WindowDeck for that taskbar button, D
  shows or hides the other desktops, Esc closes the strip, Alt+← and Alt+→ give it the next
  lower or higher number (see **Changing a window's number**). The footer
  lists the keys.

### Flat

**Flat** is the strip as it was before WinCraft 0.13: one row per virtual
desktop, this desktop first, then (when shown) the others under their names. A window
that is not on the primary monitor has a small label above its picture
saying which monitor it is on ("Left", "Right").

The pictures are small, about the size of the taskbar's own, and like the
taskbar's they keep each window's shape: every picture has the same height,
and its width follows the window, so a narrow window has a narrow card and a
wide one a wide card. The whole window is shown, never cut. A very narrow or
very wide window stops at a card 0.6 or 2.4 times as wide as it is tall, with
a margin beside or above its picture. A window Windows cannot picture gets a
16:9 card with its name. A card changes width only when its window changes
size. A desktop with many windows wraps onto more lines instead of running
off the screen. The strip takes at most 90 % of the screen's width and 60 % of
its height; beyond that the mouse wheel scrolls it. In both views:

- **The small number** at the start of each name is the window's place in
  the taskbar row of this app: 1, 2, 3 and so on, the same order that
  Win+number goes through. Windows shows one row for the whole app, so the
  By monitor columns cut that row into pieces; the numbers show how the
  pieces fit back together. Each desktop counts from 1 on its own, because
  the taskbar shows one desktop at a time. The numbers change at once when
  you drag a picture. To give a window another number without moving it to
  another monitor, see **Changing a window's number** below.
- **Rest the mouse** on a picture for a quarter of a second to see that
  window where it really is: its monitor goes dark and the window's live
  picture is drawn on top, even for a window on another desktop. The window
  itself is not raised or moved, and the peek goes away when the mouse
  leaves the picture. **Preview window on hover** turns this off.
- **Click** a picture to switch to that window; the strip closes.
- **Drag** a picture to a new place. Every change goes to the taskbar at once
  and is saved. Dropping it in another desktop's row moves the window to
  that desktop (show the other desktops first, or hold the picture over the
  link).
- **Right-click** for **Close window**, **Taskbar place** (see below) and
  **Move to desktop**. Closing is only offered here, never on a key.
- **An orange card** is a window that wants your attention, like an orange
  taskbar button: the app flashed it, for example for a new chat message.
  The card has a soft orange tint and an orange border until you switch to
  that window (click it, here or anywhere else); a window that only stops
  flashing keeps its mark. WinCraft only listens to what Windows tells every
  program about windows (it changes no window). It cannot know about a
  window that flashed before WinCraft started, so such a window shows no
  mark until it flashes again.
- **Keyboard (Flat):** ← and → pick a window, Ctrl+← and Ctrl+→ move it,
  Alt+← and Alt+→ change its number, D shows or hides the other desktops,
  Enter switches to it, a number switches to the window whose card shows it,
  Ctrl and a number open WindowDeck for that taskbar button, Esc closes the
  strip. The strip also closes when you click elsewhere.

### Changing a window's number

A drag in the By monitor view can move a window to another monitor. These
three ways change only the window's number, in both views. The window stays
on its monitor and on its desktop, and no other desktop's order changes. Each
one is the same change a drag makes: it shows at once, is saved at once and
is put on the taskbar for the desktop you are on. In By monitor the card
stays in its column and the column follows the numbers, so a window made
number 2 on the right monitor is still in the right column, now shown as 2.

- **Right-click, Taskbar place.** The submenu lists 1 to N, where N is how
  many windows the app has on that desktop. The window's own number is
  marked "(now)". Pick a number and the window takes it. The entry is not
  shown when the app has only one window on that desktop.
- **Alt+← and Alt+→** on the picked card make its number one lower or one
  higher. Nothing happens at number 1 or at the last number. The strip must
  have the keyboard (click once inside it, or open it with `Win+Alt+A` and a
  number). The
  hover strip normally closes when Alt goes down, so that Alt+Tab keeps
  working; once you have clicked into the strip, Alt no longer closes it.
  Alt+Tab then still works: it moves the keyboard to another window, and the
  strip closes because it lost the keyboard.
- **Ctrl while you drag.** Hold Ctrl, drag a picture and drop it on another
  picture of the same desktop: the dragged window takes that picture's
  number. Dragged up (to a lower number), it goes in front of the picture;
  dragged down, it takes the picture's place and the ones between move up. While Ctrl is held, the picture under
  the pointer gets an accent outline and a hint says "Change number only".
  Nothing is moved to another monitor or desktop, and dropping on empty
  space, on yourself or on a picture of another desktop (whose numbers are
  that desktop's own) does nothing. Without Ctrl a drag works as before.

The taskbar only shows the current desktop's buttons, and Windows cannot
rebuild a button without bringing its window to the current desktop. So a
new order for windows on other desktops is saved at once but reaches their
taskbar at the next restore; the strip says so while other desktops are
shown.

## WindowDeck instead of Windows' taskbar preview

This is off until you switch it on. Turn on **Show WindowDeck instead of
Windows' taskbar preview** in Settings → Plugins → WindowDeck, or use the
tick **WindowDeck on taskbar hover** in the tray menu, or the palette
command **WindowDeck: Switch taskbar hover on or off**. It works at
once, and switching it off gives you Windows' own preview back at once.

With it on:

- **Rest the mouse** on the taskbar button of an app with 2 or more windows
  (on any desktop). After the **Hover delay** (0.15 s unless you change it,
  from 0.05 to 1.5 s) WindowDeck opens above that button, on that
  monitor, with the same view as `Win+Alt+A`. Windows' own preview for that
  hover is hidden, also each time Windows tries to show it again.
- **A secondary monitor's taskbar lists only its own windows, and so does
  the strip.** Windows decides which windows each taskbar shows in Settings,
  Personalization, Taskbar, Multiple displays ("Show my taskbar apps on").
  WinCraft reads that choice (`MMTaskbarEnabled` and `MMTaskbarMode`) every
  time you rest the mouse on a button, and shows what that taskbar shows:
  - *All taskbars* (mode 0): every taskbar lists all windows, and so does
    the strip.
  - *Main taskbar and taskbar where window is open* (mode 1): the main
    taskbar lists all windows, a secondary taskbar only the windows on its
    own monitor.
  - *Taskbar where window is open* (mode 2): every taskbar, the main one
    too, lists only the windows on its own monitor.
  - *Only the main taskbar exists* (the "Show taskbar on all displays"
    switch is off): the strip lists all windows.

  A strip limited to one monitor shows that monitor's windows only: in By
  monitor a single column, in Flat the cards of those windows, and the other
  desktops link and the window count follow the same limit. The numbers on
  the cards stay the numbers of the whole row, so they can skip (7, then
  12): they are what Win+number goes through. Dragging inside such a strip
  reorders as always; windows that are not shown keep their places. A window
  cannot be dragged to another monitor there, because only one is shown.
  `Win+Alt+A` always shows everything.
- **Apps with one window** keep Windows' own preview. "One window" counts
  the windows that taskbar would list: an app with two windows on the main
  monitor and one on the right monitor keeps Windows' own preview on the
  right monitor's taskbar in mode 1 or 2. So do apps
  LayoutKeeper does not watch (see **Programs** and **Never touch** on
  LayoutKeeper's page).
- **Move the mouse up** into the strip: it stays open. It closes half a
  second after the mouse has left both the button and the strip. Moving to
  another app's button moves the strip to that app.
- **Click** a picture to switch to that window; the strip closes. Drag and
  right-click work as in the strip you open with `Win+Alt+A`.
- **Esc**, a click anywhere outside the strip, or a click on the taskbar
  button closes it. Clicking the button then does what it always does.
- The strip does not take the keyboard from the window you are working in.
  Click once inside the strip to use its keys (arrows, D, Enter).
- **Win+number and Alt+Tab work exactly as before.** Windows' preview is
  never hidden while a Windows key or Alt is down, within 1.5 s of
  Win+number, within half a second of letting go of a Windows key or of
  pressing an arrow key, Tab, Enter, Space or Esc, or while a mouse button is
  down. Pressing a Windows key or Alt also closes the strip, with one
  exception: after you click inside the strip it has the keyboard, and Alt
  no longer closes it, so Alt+← and Alt+→ can change a window's number.
  Windows' own preview is still left alone while Alt is down.

WinCraft only hides Windows' preview window, the way any program can; it
never moves it, changes Explorer or changes a setting. If WinCraft stops,
Windows shows its own preview again on the next hover.

It costs almost nothing while you do not use it: away from the taskbar,
WinCraft looks at the mouse position 10 times a second and does nothing
else. Only while the mouse is on a taskbar does it look more often, and only
once the mouse rests there does it ask Windows which button it is on.

A test copy of WinCraft (`WINCRAFT_INSTANCE`) opens the strip on hover but
never hides Windows' preview, so a test cannot touch your real taskbar; the
log says "hiding Windows' taskbar preview skipped". The flag
`--allow-hide-popup` lets a test copy hide it, for a test you watch.

## Hotkeys

| Action | Default |
|---|---|
| WindowDeck: Open | `Win+Alt+A` |

## Settings

| Setting | Default | Meaning |
|---|---|---|
| Preview window on hover | on | Resting the mouse on a picture in WindowDeck shows that window on its monitor. |
| Show WindowDeck instead of Windows' taskbar preview | off | Resting the mouse on the taskbar button of an app with 2 or more windows opens WindowDeck above it (see **WindowDeck instead of Windows' taskbar preview**). Also in the tray menu and the palette. |
| Hover delay (seconds) | 0.15 | How long the mouse rests on a taskbar button before WindowDeck opens, from 0.05 to 1.5. |

The view you pick last is kept as `arrange_view` in
`plugins\window_deck.json`.
