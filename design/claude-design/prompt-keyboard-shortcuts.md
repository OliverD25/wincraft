Design a new page for WinCraft: **Keyboard shortcuts**, an editor where the user
can give any command a shortcut or a two-step chord, in the spirit of VS Code's
Keyboard Shortcuts editor (Ctrl+K Ctrl+S), but simpler and friendlier. It must be
very easy and intuitive for a person who has never edited shortcuts before.

## What WinCraft is
A small Windows 11 tray app that adds tools Microsoft forgot. It is built from
plugins (LayoutKeeper, ScreenDimmer, ShortcutDetector, LanguageIndicator,
WindowNamer, and community plugins from the Plugin Store). Every plugin has
commands, and every command can be reached from the command palette
(Win+Alt+P). Default hotkeys start with Win+Alt.

## Where the page lives
- A new page in the Settings window, in the side menu between "Plugins" and
  "Layout": **Keyboard shortcuts**. Same frame as the Plugins Grid page: 240 px
  side menu, 44 px side padding, 36 px top and bottom, title 30 px semibold +
  14 px subtitle.
- Also reachable from the command palette: every command row in the palette
  shows its shortcut on the right as keycaps, and a palette action "Change
  shortcut…" opens this page with that command already found and selected.
  Please show this palette row too.

## What the page must let the user do
1. **Find a command fast.** One search box (Ctrl+F) that matches the command
   name, the plugin name, or the keys. A "Search by keys" toggle (like VS Code's
   record-keys button): when on, pressing a key combination types it into the
   search as keycaps, to answer "what does Win+Alt+K do?".
2. **See everything in one list**, grouped by plugin (WinCraft core first, then
   each plugin with its icon). Each row: command name, a one-line description in
   plain words, the current shortcut as keycaps (or "Add shortcut" in
   text-disabled when it has none), and a small "Changed" marker when it differs
   from the default.
   Filter chips above the list: All · Changed · No shortcut · Conflicts.
3. **Change a shortcut by pressing it.** Click a row (or Enter on it) → the
   shortcut cell turns into a recording field: "Press the keys…". The user
   presses a combination; it shows as keycaps. If they press a second key within
   3 seconds it becomes a **chord** (shown as two keycap groups with a small
   "then" between them, e.g. [Win][Alt][K] then [S]). A countdown hint shows
   while it waits for the second key. Enter or a "Save" button saves, Esc
   cancels, Backspace clears.
4. **Understand problems before saving**, in plain words, under the field:
   - "Already used by <command> in <plugin>." with buttons "Replace" and "Cancel".
   - "Another app holds Win+Alt+X, so it may not reach WinCraft."
   - "Win+Alt+K is the start of a chord, so it cannot also be a shortcut on its
     own." (a combination cannot be both a plain shortcut and a chord start)
   - "Windows keeps Win+L for itself." (keys that can never be used)
5. **Undo easily:** a per-row "Reset to default" (small icon button, with a
   tooltip), and a page-level "Reset all…" with a confirm dialog.
6. **Remove a shortcut:** a per-row "Remove" in a "…" menu.
7. Optional small touch: a "?" link that explains chords in one sentence:
   "A chord is two key presses in a row, like Win+Alt+K, then S."

## Must keep
- Keyboard first: everything works without a mouse (Tab, arrows in the list,
  Enter to record, Esc to cancel).
- Windows 11 look, the same design system and tokens as the other WinCraft
  pages (Segoe UI Variable, the warm accent), light and dark themes.
- No text smaller than 12 px. Short, plain English labels, one idea per
  sentence; say what will happen, not how it works inside.
- WinCraft never surprises the user: nothing is saved until the user saves, and
  every change can be reset.

## Please show
- The page in dark and light, with: one plugin group expanded, a row with no
  shortcut, a "Changed" row, a chord row, and the search box with results.
- The recording state, waiting for the second key of a chord.
- A conflict message ("Already used by …") with Replace / Cancel.
- The "Search by keys" state.
- The palette row with a shortcut and the "Change shortcut…" action.
- Rules for the developer (sizes, spacing, states), like the Arrange Strip page.
