# US-004: Use the hover deck from the keyboard without clicking

Status: draft
Area: window-deck
Source: proposed by Claude from the features built, next-steps review 2026-10-29 (not the user's own draft)

**Story:** As a user who points at a taskbar button, I want the deck's keys to work while the mouse is on the deck, so that I do not have to click into it first.

## Acceptance criteria

- [ ] While the mouse is inside an unfocused hover deck, digits, Shift/Ctrl+digit, arrows, Enter, Esc, F2, D, Ctrl+W and Ctrl+N go to the deck.
  Test: src/plugins/window_deck/hover_keys/tests.rs::enter_esc_f2_d_ctrl_w_and_ctrl_n_go_to_the_strip
- [ ] All other typing stays with the app the user works in.
  Test: src/plugins/window_deck/hover_keys/tests.rs::typing_stays_with_the_app
- [ ] Win and Alt combinations stay with Windows, so Win+number and Alt+Tab work as before.
  Test: src/plugins/window_deck/hover_keys/tests.rs::win_and_alt_with_anything_stay_with_windows
- [ ] No key sticks: a routed key's release is swallowed too, even after routing stopped.
  Test: src/plugins/window_deck/hover_keys/tests.rs::a_routed_key_keeps_its_release_after_routing_stopped
- [ ] A test copy of WinCraft never catches real keys.
  Test: src/plugins/window_deck/hover_keys/tests.rs::a_read_only_test_instance_never_puts_the_hook_in

## History

- 2026-10-29 — created as draft from the features built — approved by user: no
