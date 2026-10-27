Design a new WinCraft plugin: **QuickJump**. It is "Harpoon for windows"
(Harpoon is a Neovim plugin: you pin files to numbered slots and jump to them by
number). In QuickJump the user pins chosen windows to slots 1–9 (0 = 10) and
jumps to them from anywhere with the keyboard.

## How it works
- **Jump:** Win+Alt+Q, then a digit → the window in that slot comes to the front.
  If it is on another virtual desktop, Windows switches to that desktop. If the
  window is already in front, nothing happens.
- **Pin:** Win+Alt+Shift+Q, then a digit → the window in front is pinned to that
  slot. A full slot is replaced, with Undo.
- **Pin list:** Win+Alt+Q pressed twice → a window with all pins.
- After the first key, WinCraft waits 3 seconds for the digit.
- **A pin remembers its window** (app + title), also after a restart. A pin whose
  window is closed shows as "not open"; jumping to it starts the app.
- One list for all virtual desktops.

## Please design these screens, dark and light
1. **Jump slot strip** (shown for 3 s after Win+Alt+Q): a small horizontal strip,
   bottom centre of the active monitor. Slots 1–9 and 0, each with the digit as a
   keycap, the app icon, and a short window name. States: open, not open (dimmed,
   "not open"), on another desktop (small desktop badge, e.g. "D3"), empty
   ("empty"), the window that is in front now (marked). A hint line:
   "Press a number to jump · Win+Alt+Q again for the list · Esc".
2. **Pin slot strip** (after Win+Alt+Shift+Q): the same strip in "pin" mode. It
   shows which window is about to be pinned (icon + title, at the left), empty
   slots invite ("Pin here"), full slots say "Replace". Hint: "Press a number to
   pin Claude · Esc".
3. **Notices** (short WinCraft messages, a few seconds, one idea each):
   "Pinned Claude to 3", "Pinned Claude to 3 (was Chrome) · Undo",
   "Slot 5 is empty", "Starting Claude for slot 2…", "Removed slot 4 · Undo".
4. **Pin list window** (Win+Alt+Q twice, also from the palette and the tray):
   one row per slot 1–9, 0: digit keycap, window thumbnail, app name, window
   title, desktop and monitor, state (open / not open / empty).
   Actions: jump (Enter or click), reorder (drag, or Ctrl+↑↓), move to another
   slot by typing its digit, remove (Del, with Undo), "Pin a window…" on an
   empty slot. A footer with the key hints. An empty first-use state that
   teaches the two shortcuts in two lines.
5. **"Pin a window…" picker** (from an empty slot in the list): a searchable list
   of open windows with icons and thumbnails; Enter pins the chosen one.
6. **Pin details / matching** (a side panel or a small dialog in the list): how
   this pin finds its window again: app + "title contains …" (editable), with a
   live "Matches 1 open window" check, and a custom label for the slot.
7. **Palette rows**: commands "QuickJump: Jump to 3 · Claude" with the shortcut
   as keycaps, "QuickJump: Pin this window…", "QuickJump: Open pin list".

## Must keep
- Same design system and tokens as the other WinCraft pages (it now has themes:
  design in Classic; the token rules from "WinCraft Themes" apply).
- Windows 11 look, keyboard first (everything works without a mouse).
- No text smaller than 12 px. Short plain English, one idea per sentence; every
  on-screen message names WinCraft or QuickJump, so the user knows who talks.
- Nothing moves or closes without the user asking; every change can be undone.
- Rules for the developer (sizes, spacing, states), like the Arrange Strip page.
