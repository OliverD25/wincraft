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

## User decisions that override the design

The design session is the source of truth for the look, but the user has
overruled some rules in chat. Every plan made from a design page must check
this list first and copy each matching override into the plan, so a coder never
rebuilds the old rule. Add a line here whenever the user overrules a design
rule, and send the user a prompt so the design page is fixed too.

| Area | Design said | The user decided | Date |
|---|---|---|---|
| Arrange strip ghosts | "Ghosts are not draggable" | Ghosts are draggable: inside the primary column a drag changes the window's taskbar place (it stays on its monitor); into another column or desktop it moves the window like its real card. Ctrl/Alt+←→ change its place. | 2026-10-26 |
| Arrange strip ghosts | Click selects the real card; arrow keys skip ghosts | A click opens the window; arrow keys land on ghosts like cards; Enter opens. | 2026-10-25 |
| Arrange strip keys | ←→ move the window, Shift+←→ next monitor | Arrows only select; Ctrl+←→ move, Ctrl+Shift+←→ next monitor, Ctrl+Shift+↑↓ next desktop. | 2026-10-25 |
| Compact view | Exactly 1 window on this desktop | 1 or 2 windows on this desktop. | 2026-10-26 |
| Themes picker | "Theme" dropdown in General | Tiles with a live hover preview, on their own Settings page "Themes". | 2026-10-26 |
| Palette Ask AI | Answer inside the palette from an AI provider | Opens the chosen AI website in the browser (Principle 5). No currency conversion. | 2026-10-26 |
| Hotkeys page | Page "Keyboard shortcuts"; hotkey status list on About | The page is called "Hotkeys" and says "hotkey" throughout; each row shows its live status (working / taken / plugin off) with a summary line; About has no hotkey list. | 2026-10-26 |
| Plugin tiles (1a) | Tile shows "4.1k users" above 100 users | No user count: WinCraft has no user data and collects none (Principle 5). Store "Get update" kept for plugins that need a newer WinCraft. | 2026-10-26 |
| Arrange strip name and keys | "Arrange strip", part of LayoutKeeper; digit in the strip jumps to taskbar button N; badges show at once after Win+Alt+A | Own plugin **WindowDeck**. In the strip a digit opens card N; Ctrl+digit jumps to taskbar button N. Badges appear only after 700 ms without a digit. Optional switch: Win+1…9 opens WindowDeck (off by default). | 2026-10-26 |
| Artboard desks | Windows drawn on a coloured desk with padding | The desk is background only: no outer frame, margin or extra glass around any window. | 2026-10-26 |

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
| Palette Main View.dc.html + v2 (palette with inspector, 8 additions) | 2026-10-26, via DesignSync (pages, no README) | plan `wincraft-palette-v2.md` |
| Plugins Grid v2.dc.html (option 1a, horizontal compact cards) | 2026-10-26, via DesignSync (page, no README) | plan `wincraft-plugins-grid-1a.md` |
| design_handoff_hotkey_editor (option 1c, one hotkey editor dialog) | 2026-10-26, via DesignSync (README) | plan `wincraft-hotkey-editor.md` |
