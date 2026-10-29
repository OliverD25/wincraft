# US-002: Win+number opens and switches WindowDeck

Status: active
Area: window-deck
Source: proposed by Claude from the features built, next-steps review 2026-10-29 (not the user's own draft)

**Story:** As a user with many windows, I want Win+1…9 to open WindowDeck for that taskbar app and Win+another number to switch to that app at once, so that I can work fast from the keyboard.

## Acceptance criteria

- [ ] With "Open WindowDeck with Win+1…9" on, Win+N opens WindowDeck for the app at taskbar button N.
- [ ] Releasing Win and pressing Win+M switches the open deck to app M, every time.
- [ ] Pressing Win+N again while Win is held selects the next card, round to the first.
  Test: src/ui/arrange/tests.rs::win_and_the_same_number_again_picks_the_next_card_round_to_the_first
- [ ] WinCraft's keyboard hook keeps working while a WinCraft window is in front (the raw keyboard registration is removed).
  Test: src/core/raw_input.rs::the_keyboard_registration_is_found_and_removed

## History

- 2026-10-29 — created as draft from the features built — approved by user: no
- 2026-10-29 — approved by the user in chat ("approve the stories") — Status: active
