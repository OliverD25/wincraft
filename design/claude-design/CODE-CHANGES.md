# UI changes made in code

Every change to a screen, layout, component, colour or on-screen wording that
did not come from a design pull gets an entry here, in the same commit. The
design session reads this file and brings the design in line, or says why not.

Status is `not yet in design` until a pull shows the design has it; then
`in design (pulled <date>)`.

| Date | What changed and why | Files | Status |
| --- | --- | --- | --- |
| 2026-10-27 | Fluent palette on glass: the window corner radius is 8 px, not the Fluent 12 px. Windows clips a glass window to its own 8 px corners, so a 12 px painted corner left a strip of bare glass at each corner. Without glass (battery saver, transparency off) Fluent keeps 12 px. | `src/ui/palette/panel.rs` | not yet in design |
