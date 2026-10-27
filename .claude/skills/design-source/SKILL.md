---
name: design-source
description: Where WinCraft's design lives and how it reaches the code. The Claude Design project "Wincraft Windows Utilities Design" is the source of truth for how WinCraft looks; copies sit in design/claude-design/. Use before building or changing any screen, theme, layout or wording on screen, when the user mentions the design, Claude Design or a handout, or runs /design-pull.
---

# WinCraft design source

The workflow (pull, compare, apply, check, the quick check before UI work, the
code-changes log) is in the global `design-pull` skill. Tool and login facts
are in `claude-design-connect`. This file holds only WinCraft's facts and the
user's decisions.

- **Source of truth:** Claude Design project "Wincraft Windows Utilities
  Design", id `2799b359-d98e-4741-afbe-67e08618bcf1`,
  https://claude.ai/design/p/2799b359-d98e-4741-afbe-67e08618bcf1. Type:
  design project. Design system it uses: "classical" (the `_ds/` bundle).
- **Copies:** `design/claude-design/`, committed (user's decision,
  2026-10-27; until then they sat git-ignored in `_dev_docs/claude-design/`).
  Record in `design/claude-design/FETCHED.md`. UI changes made in code go into
  `design/claude-design/CODE-CHANGES.md`. The repo is public: never put a
  private path, a name or an e-mail address into a copy or a prompt file.
- **Handouts:** folders named `design_handoff_*` hold a `README.md` spec plus
  one `.dc.html` page. The README is the spec; a page is a visual reference,
  never code to port.
- **Skip when pulling:** `_ds/` bundles, `support.js`, `image-slot.js`,
  `.thumbnail`, `uploads/`, and the option pages whose choice is made
  (`* Options.dc.html`), unless a task needs them.
- **Product notes:** colours come from theme tokens only (`src/core/theme.rs`),
  for all five themes, light and dark; design pages show fixed hex values of
  the Classic dark theme. No text under 12 px (Principle 6). A plugin's window
  talks to the host only through the plugin interface (Principle 3).
- **Prompt files** for the design session (`prompt-*.md`) sit next to the
  copies.

## Page-to-code map

| Design page | Built in |
| --- | --- |
| `Arrange Strip.dc.html`, `design_handoff_arrange_strip/README.md` | `src/ui/arrange.rs`, `src/ui/arrange/`, `src/plugins/window_deck/` |
| `Arrange Strip Compact Options.dc.html` (8a chosen) | `src/ui/arrange/compact.rs` |
| `About Updates.dc.html` | `src/ui/settings/about.rs`, `src/ui/widgets/row.rs` |
| `Hotkey Editor Options.dc.html` (1c), `design_handoff_hotkey_editor/README.md` | `src/ui/widgets/hotkey_dialog.rs`, `src/ui/widgets/hotkey_capture.rs` |
| `Keyboard Shortcuts.dc.html` | `src/ui/settings/shortcuts.rs`, `src/ui/settings/hotkey_status.rs` |
| `Palette Main View*.dc.html`, `Palette Suggestions.dc.html`, `Match Highlight Options.dc.html` | `src/ui/palette/`, `src/search/` |
| `Plugins Grid v2.dc.html` (1a), `design_handoff_plugins_grid/README.md` | `src/ui/settings/plugins.rs`, `src/ui/settings/store.rs`, `src/ui/settings/catalog.rs`, `src/ui/widgets/tile.rs` |
| `QuickJump.dc.html` | `src/plugins/quick_jump/` |
| `Shortcut Detector.dc.html` | `src/plugins/shortcut_detector/window.rs` (Win32 controls today; being rebuilt in egui) |
| `WinCraft Themes.dc.html` | `src/core/theme.rs`, `src/ui/settings/themes.rs`, `src/ui/widgets/theme_tile.rs` |
| `WinCraft Handout.dc.html`, `design_handoff_wincraft_ui/README.md` | `src/core/theme.rs`, `src/ui/widgets/` |

Copies in the repo so far: `design_handoff_arrange_strip/`, `Shortcut
Detector.dc.html`. The other pages are copied at their next pull.

## Your decisions that override the design

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
| WindowDeck keys | (not designed) | Ctrl+W closes the selected window, Ctrl+N opens a new window of the app, F2 renames the selected window on its card (Enter saves, Esc or a click elsewhere cancels; saved as WindowNamer's name). Hover counts only after the mouse moves; hover and keys share one selection. | 2026-10-27 |
| Artboard desks | Windows drawn on a coloured desk with padding | The desk is background only: no outer frame, margin or extra glass around any window. | 2026-10-26 |

## Pulls so far

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
| Shortcut Detector.dc.html (window redesign in the WinCraft look; first pull with copies in git) | 2026-10-27, via DesignSync (page, no README) | plan `wincraft-shortcut-detector.md` |
