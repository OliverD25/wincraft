# US-001: Open the palette and run anything from the keyboard

Status: active
Area: palette
Source: proposed by Claude from the features built, next-steps review 2026-10-29 (not the user's own draft)

**Story:** As a keyboard user, I want one search box that finds commands, apps, windows and files, so that I never reach for the mouse to start something.

## Acceptance criteria

- [ ] The palette hotkey opens the palette on the monitor the pointer is on, with the cursor already in the search box.
- [ ] Typing a few letters lists matching commands, apps, open windows, bookmarks and files; Enter runs the selected row.
- [ ] Up and Down move through the list without moving the text cursor in the search box.
  Test: src/ui/palette/tests.rs::up_and_down_move_the_list_and_leave_the_search_cursor_alone
- [ ] Esc closes the palette and gives the keyboard back to the window that had it.

## History

- 2026-10-29 — created as draft from the features built — approved by user: no
- 2026-10-29 — approved by the user in chat ("approve the stories") — Status: active
