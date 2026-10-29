# US-008: Move the palette where I want it

Status: active
Area: palette
Source: proposed by Claude from the features built, next-steps review 2026-10-29 (not the user's own draft)

**Story:** As a user, I want to drag the palette by any blank part and have it open there next time, so that it does not cover what I am working on.

## Acceptance criteria

- [ ] Press and hold on blank space (including the search box right of the text) and move: the window moves.
  Test: src/ui/palette/tests.rs::holding_blank_space_and_moving_starts_a_window_drag
- [ ] A drag on typed text, a row or a button does not move the window.
  Test: src/ui/palette/tests.rs::a_drag_on_typed_text_or_a_row_does_not_move_the_window
- [ ] The palette reopens where it was left, per monitor, also after a restart.
- [ ] Double-click on blank space, or "Palette: Move back to the centre", puts it back.

## History

- 2026-10-29 — created as draft from the features built — approved by user: no
- 2026-10-29 — approved by the user in chat ("approve the stories") — Status: active
