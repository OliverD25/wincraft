# US-005: Flip a Windows setting from the palette and see its state

Status: draft
Area: quick-actions
Source: proposed by Claude from the features built, next-steps review 2026-10-29 (not the user's own draft)

**Story:** As a user, I want to switch dark mode, hidden files, mic mute and similar settings from the palette and see their current state, so that I change them in one keystroke and know what I changed.

## Acceptance criteria

- [ ] Each of the eight toggles shows a state tile, a state line and a switch, read from Windows each time the palette opens.
  Test: src/core/host/tests.rs::the_palette_gets_every_toggles_state_and_again_after_a_flip
- [ ] Enter flips the toggle and the palette stays open, with the new state shown.
- [ ] Every flip shows a WinCraft notice with Undo, and Undo puts the setting back.
  Test: src/plugins/quick_actions/actions.rs::a_toggle_switches_the_setting_and_undo_puts_it_back
- [ ] A state that cannot be read shows "State unknown" and no switch.
  Test: src/core/ui_bridge/tests.rs::a_toggle_state_says_on_off_or_nothing_to_switch

## History

- 2026-10-29 — created as draft from the features built — approved by user: no
