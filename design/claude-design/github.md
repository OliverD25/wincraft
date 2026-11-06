repo: OliverD25/wincraft
branch: main
path: design/claude-design

## Last sync
date: 2026-11-05T23:52:45Z
### Updated in this project
- CODE-CHANGES entries of 2026-11-05 and 2026-11-06 brought into the design
- New pages: Shortcut Manager, QuickJump v2, Window Hint; Chord Tip 6a (5b as built)
- WindowDeck: neutral selected ring, Mark all as seen (Ctrl+M), 12 px minimum text

## Sync history
- 2026-11-01T18:04:02Z: WindowDeck larger thumbnails (Window Deck Options 3a)
- 2026-11-01T12:34:56Z: Open or switch 3a matched to the build; palette drag-to-move noted in Fluent rules
- 2026-10-29T06:40:38Z: Fluent glass 88 %, About check states and update panel, Detailed Log screen, WindowDeck key line and close animation
- 2026-10-28T19:00:40Z: first seven CODE-CHANGES entries applied
- 2026-10-28T14:31:38Z: QuickActions toggle rows and light-theme toggle rows

## Screen map
| Screen | Repo files |
| --- | --- |
| Toggle Rows Light Themes.dc.html | src/core/theme.rs, src/ui/widgets/toggle.rs, src/ui/palette/mod.rs, design/claude-design/CODE-CHANGES.md |
| Toggle States Options.dc.html | src/ui/palette/rows.rs, src/ui/widgets/toggle.rs, src/plugins/quick_actions/states.rs |
| design_handoff_quickactions_toggles/README.md | src/ui/palette/rows.rs, src/ui/widgets/toggle.rs, src/plugins/quick_actions/states.rs |
| Palette Main View v3 Fluent.dc.html | src/ui/palette/panel.rs, src/ui/palette/drag.rs, src/core/theme.rs |
| Shortcut Manager.dc.html | src/plugins/shortcut_detector/mod.rs, window.rs, answer.rs, rows.rs, nearby.rs, press.rs, owners.rs |
| Arrange Strip.dc.html | src/ui/arrange/strip.rs, window.rs, rows.rs, by_monitor/chrome.rs, routed.rs, slide.rs, closing.rs, motion.rs, seen.rs, cards.rs, by_monitor/draw_card.rs, compact.rs, src/core/host/seen.rs |
| About Updates.dc.html | src/ui/settings/about.rs, src/core/update_notice.rs, src/core/host/updates.rs |
| Detailed Log.dc.html | src/ui/palette/panel.rs, src/ui/settings/general.rs, src/core/tray.rs, src/core/host/detailed_log.rs |
| Open or Switch Options.dc.html | src/ui/palette/running.rs, nesting.rs, nested_row.rs, inspector.rs, view.rs |
| Window Deck Options.dc.html | src/ui/arrange/by_monitor.rs |
| Update Available Options.dc.html | src/ui/palette/pinned.rs, rows.rs, inspector.rs, actions.rs, state.rs, panel.rs |
| QuickJump v2.dc.html | src/plugins/quick_jump/pins.rs, keys.rs, wait.rs, notice.rs, mod.rs, list.rs, list/grid.rs, pictures.rs, picker.rs, spot.rs |
| Quick Access Options.dc.html | src/plugins/quick_jump/list.rs, list/grid.rs |
| Chord Tip Options.dc.html | src/core/chord_tip.rs, chord_tip/draw.rs, screen.rs, src/plugins/quick_jump/wait.rs, src/core/host/bindings.rs |
| Window Hint.dc.html | src/core/window_hint.rs, window_hint/draw.rs, window_hint/screen.rs, src/plugins/quick_jump/hint.rs |
