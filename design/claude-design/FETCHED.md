# Claude Design fetch record

Project: "Wincraft Windows Utilities Design" (design project)
Id: 2799b359-d98e-4741-afbe-67e08618bcf1
Link: https://claude.ai/design/p/2799b359-d98e-4741-afbe-67e08618bcf1

## 2026-10-17

Fetched (read-only, byte for byte):
- `design_handoff_arrange_strip/README.md` (≈5.4 KB) — new: spec for the Arrange strip grouped by monitor and desktop.
- `design_handoff_arrange_strip/Arrange Strip.dc.html` (≈11 KB) — new: visual reference, dark + light artboards.

Left out:
- `_ds/…`, `support.js`, the `design_handoff_*/_ds` copies — the Classical design-system bundle the page needs to render; not needed for the spec.
- `design_handoff_wincraft_ui/*`, `WinCraft Handout*.dc.html` — handouts 1 and 2, already received as zips on 2026-09-24/25 (`Wincraft Windows Utilities Design*.zip`) and built.
- `uploads/*.png` — two pasted screenshots (binary), not needed.

## 2026-10-17 (evening)

Fetched (read-only):
- `design_handoff_wincraft_ui/README.md` (≈14 KB) — changed since handout 2: section 4.1 now says
  "Fuzzy query: highlight only the main run — the first contiguous match of ≥ 3 characters — in
  accent colour, no underline. Scattered single-character hits stay plain text." Built in 0.16.1
  (`fuzzy::main_run`); queries shorter than 3 letters highlight when the whole query is unbroken.
  Not copied byte for byte this time: only the changed bullet matters, and the full README is in
  the project.

## 2026-10-25

Read (read-only); not copied byte for byte — the substance went into plan files:
- `Plugins Grid.dc.html` — new: option 3a, Plugins and Plugin Store as a tile grid with optional
  plugin thumbnails, dark + light. User chose to build it → `~/.claude/plans/wincraft-plugins-grid.md`.
- `Arrange Strip Width Options.dc.html` — new: "Turn 2", column widths 2a / 2b (recommended) / 2c.
  User chose 2b → `~/.claude/plans/wincraft-strip-widths.md`.

Seen, not read:
- `Plugins Grid Options.dc.html` (the variants behind 3a), `Match Highlight Options.dc.html`
  (likely the source of the 0.16.1 highlight change), root `Arrange Strip.dc.html` (copy of the
  handout page), 4 new `uploads/*.png` screenshots.

## 2026-10-25 (later)

Read (read-only); not copied byte for byte — the substance went into a plan file:
- `Arrange Strip.dc.html` — changed by the user: the primary monitor's column now shows
  "ghosts" for windows on other monitors at their taskbar position (84 px, edge strip with
  arrow + monitor name, faded picture, number, title), and the column width cap became
  "min(content, proportional share)". The user said this page is the chosen version
  → `~/.claude/plans/wincraft-strip-ghosts.md`.
- `Arrange Strip Ghost Options.dc.html` — new: the variants (4a–4c, 5a–5c) behind it. The user
  said not to build from this page.

Seen, not read:
- `uploads/pasted-1792950433432-0.png` — a new pasted screenshot (binary), not needed.

## 2026-10-25 (night)

Read (read-only); substance went into plan files:
- `Arrange Strip.dc.html` — changed again: new ghost layout (20 px name strip with monitor pill,
  vertical title, number chip; full-height picture) → `wincraft-strip-ghosts.md` "Update 2".
- `WinCraft Themes.dc.html` — new: five themes (Classic default, Fluent, Terminal, Paper, High
  contrast) with all token values → `~/.claude/plans/wincraft-themes.md`. User chose tiles with
  live hover preview instead of the page's dropdown; Fluent in step 2.

Seen, not read: `Palette Main View*.dc.html` (4 pages), `Palette Suggestions.dc.html`, new uploads.

## 2026-10-26

Read (read-only); substance went into a plan file:
- `Keyboard Shortcuts.dc.html` — new: the shortcuts editor (search, search by keys, chips, grouped
  list, recording with chords, four checks, Reset all, palette "Change shortcut…")
  → `~/.claude/plans/wincraft-shortcuts-editor.md` (replaces wincraft-chords.md as the build plan).

- `QuickJump.dc.html` — new, read later the same day: strips, notices, pin list, details,
  picker, palette → `~/.claude/plans/wincraft-quickjump.md`.

- `Palette Main View.dc.html` (6a base) and `Palette Main View v2.dc.html` (6a + eight additions) —
  read 2026-10-26 → `~/.claude/plans/wincraft-palette-v2.md`. Ask AI changed to "open the AI website"
  (Principle 5); currency conversion dropped by the user.

## 2026-10-26 (later)

Read: `design_handoff_arrange_strip/README.md` — Badge layout (chips on the thumbnail, icons on narrow cards) → built in 0.20.19; Compact view reorder (header, button, hints, card with title row first and thumbnail last) → being built. The README still says compact = exactly 1 window; the user decided 1 or 2 (override list).
- Same day: README "Monitor column" rewritten — one row first (content counts ghosts, real card widths; spare to the last column), else the widest content takes 1fr. Being built.

## 2026-10-26
- `Plugins Grid v2.dc.html` (~17 KB): option 1a for Plugins and Plugin Store. Not copied; substance in ~/.claude/plans/wincraft-plugins-grid-1a.md.

## 2026-10-26 (2)
- `design_handoff_hotkey_editor/README.md` (~2.6 KB): hotkey editor dialog, option 1c. Page `Hotkey Editor Options.dc.html` not fetched (README is the spec).

## 2026-10-27
- `About Updates.dc.html` (~8 KB): About page with Updates on top, one state-following button, no-wrap rule. Not copied; substance in ~/.claude/plans/wincraft-about-updates.md.

## 2026-10-27 (2)
- `design_handoff_arrange_strip/README.md` (~14 KB): Footer section gained Ctrl+H to hide/show the hint strip (setting showHints, default on; "Ctrl + H Hints" in the header while hidden; last chip "Ctrl + H Hide hints"). Being built. The README's older footer key list stays overridden by the overrides table (Ctrl+arrows, Ctrl+W, Ctrl+N, F2).

## 2026-10-27 (3)
- `Shortcut Detector.dc.html` (~11 KB): new page, first fetch. A redesign of the ShortcutDetector window in the WinCraft look (today it is plain Win32 controls). Copied to design/claude-design/ (first pull with copies in git); build plan `wincraft-shortcut-detector.md`. The copies moved here from the git-ignored _dev_docs/claude-design/ the same day; design_handoff_arrange_strip/ is the 2026-10-26 copy and is refreshed at its next pull.

## 2026-10-28
- `design_handoff_quickactions_toggles/README.md` (2490 bytes): new. Toggle rows in the palette: state tile + title + state line + switch ("Chosen", b + a), the eight toggle commands with icons, tile colours and state wording, the "Show Desktop Icons" rename, and three rules (real state read on each palette open; Always on Top with no window says "No window selected"; unreadable state says "State unknown", no switch).
- `design_handoff_quickactions_toggles/Toggle States Options.dc.html` (14720 bytes): new, the options page (a, b, c per toggle plus the Chosen block).
- Left out: `support.js` (renderer), `_ds/` bundles, `uploads/`. Other pages unchanged since their last pull were not fetched (this pull is narrowed to the new handoff).
