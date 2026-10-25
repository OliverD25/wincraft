---
name: design-handouts
description: Fetch WinCraft design handouts from the user's Claude Design project "Wincraft Windows Utilities Design" read-only into _dev_docs/claude-design/. Use when the user says "fetch the design", "take the handout from Claude Design", "the design agent made something new", or names the Arrange strip / WinCraft UI design.
---

# WinCraft design handouts from Claude Design

Generic rules and the tool live in the global `claude-design-connect` skill.
This file holds WinCraft's own facts and decisions.

- **Project:** "Wincraft Windows Utilities Design", a design project (not a
  design system), id `2799b359-d98e-4741-afbe-67e08618bcf1`, owned by the user.
- **Trigger:** fetch only on the user's word, never on a schedule.
- **What counts as a handout:** folders named `design_handoff_*` (a `README.md`
  spec plus one `.dc.html` visual reference). The README is the spec; the page
  is a visual reference, never code to port.
- **Where files go:** `_dev_docs/claude-design/<path as in the project>`.
  `_dev_docs/` is git-ignored (WinCraft has no `_io` folder; this is its
  exchange folder). Never copy handout files or their private paths into the
  repository; the substance goes into a plan in `~/.claude/plans/` and a desk card.
- **Record:** every fetch gets a dated entry in `_dev_docs/claude-design/FETCHED.md`.
- **Skip:** `_ds/` bundles, `support.js`, `uploads/` images, unless a task needs them.
- **Truncated file (> 256 KiB):** ask the user for the page's share link.

## Handouts so far

| Handout | Arrived | Became |
|---|---|---|
| design_handoff_wincraft_ui (handout 1, Classical) | 2026-09-24, as zip | v0.4 visual pass |
| design_handoff_wincraft_ui (handout 2, readability) | 2026-09-25, as zip | v0.6.0 design v2 |
| design_handoff_arrange_strip (strip by monitor and desktop) | 2026-10-17, via DesignSync | plan `wincraft-arrange-by-monitor.md` |
| Plugins Grid.dc.html (option 3a, Plugins + Store tile grid) | 2026-10-25, via DesignSync (page, no README) | plan `wincraft-plugins-grid.md` |
| Arrange Strip Width Options.dc.html (2b, rails for empty monitors) | 2026-10-25, via DesignSync (page, no README) | plan `wincraft-strip-widths.md` |
| Arrange Strip.dc.html (ghosts for other monitors in the primary column) | 2026-10-25, via DesignSync (page, no README) | plan `wincraft-strip-ghosts.md` |
| WinCraft Themes.dc.html (5 themes; tiles + hover preview chosen by the user) | 2026-10-25, via DesignSync (page, no README) | plan `wincraft-themes.md` |
| Keyboard Shortcuts.dc.html (shortcuts editor + chords) | 2026-10-26, via DesignSync (page, no README) | plan `wincraft-shortcuts-editor.md` |
| QuickJump.dc.html (QuickJump plugin: strips, notices, pin list, picker) | 2026-10-26, via DesignSync (page, no README) | plan `wincraft-quickjump.md` |
