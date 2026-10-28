# US-003: Pick any of up to 20 windows in WindowDeck by its number

Status: draft
Area: window-deck
Source: proposed by Claude from the features built, next-steps review 2026-10-29 (not the user's own draft)

**Story:** As a user with many windows of one app, I want to open a card by the number it shows, so that one key press reaches the window I want.

## Acceptance criteria

- [ ] A digit 1–9 opens the card with that number; 0 opens card 10.
  Test: src/ui/arrange/tests.rs::a_digit_opens_the_card_with_that_number_on_this_desktop
- [ ] Shift and a digit opens the card ten higher: Shift+1 is 11, Shift+0 is 20.
  Test: src/ui/arrange/tests.rs::shift_adds_ten_to_a_typed_digit
- [ ] A number no card has does nothing and closes nothing.
- [ ] The footer names the Shift key only when this desktop has more than ten cards.

## History

- 2026-10-29 — created as draft from the features built — approved by user: no
