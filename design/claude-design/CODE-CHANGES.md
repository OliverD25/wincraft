# UI changes made in code

Every change to a screen, layout, component, colour or on-screen wording that
did not come from a design pull gets an entry here, in the same commit. The
design session reads this file and brings the design in line, or says why not.

Status is `not yet in design` until a pull shows the design has it; then
`in design (pulled <date>)`.

| Date | What changed and why | Files | Status |
| --- | --- | --- | --- |
| 2026-10-27 | Fluent palette on glass: the window corner radius is 8 px, not the Fluent 12 px. Windows clips a glass window to its own 8 px corners, so a 12 px painted corner left a strip of bare glass at each corner. Without glass (battery saver, transparency off) Fluent keeps 12 px. | `src/ui/palette/panel.rs` | not yet in design |
| 2026-10-27 | ShortcutDetector window built from `Shortcut Detector.dc.html` with these differences: the native Windows title bar (no W mark, no custom 40 px buttons), as every other WinCraft window has; the shared search box (36 px, 14 px text, with a Ctrl+F chip) instead of 32 px / 13 px; at the default 760 px width Refresh and Copy list wrap to a second toolbar line, aligned right; the hook note moves to its own line when it does not fit; App rows keep "(Windows: …)" in Meaning; a sorted column shows ↑ or ↓ after its name; "Copied N rows." shows for 3 s in place of the hook note. | `src/plugins/shortcut_detector/window.rs`, `rows.rs` | not yet in design |
