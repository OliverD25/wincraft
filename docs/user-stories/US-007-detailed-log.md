# US-007: Record a detailed log and hand it over

Status: active
Area: detailed-log
Source: proposed by Claude from the features built, next-steps review 2026-10-29 (not the user's own draft)

**Story:** As a user who hit a bug, I want to switch on a detailed log, mark the moment and save a report, so that the bug can be found from one session's record.

## Acceptance criteria

- [ ] The Detailed log is a toggle row in the palette, a tray item and a switch on Settings > General; it is off by default.
  Test: src/core/host/tests.rs::the_detailed_log_toggle_says_its_state_in_words_and_keeps_its_title
- [ ] While it is on, the tray tooltip, a notice and a LOG tag in the palette footer say so.
- [ ] "Save a report" includes the log that is still being written.
  Test: src/core/detailed/tests.rs::a_report_copies_the_log_while_it_is_still_being_written
- [ ] "Delete the files" removes every detailed log and report after a second Enter.

## History

- 2026-10-29 — created as draft from the features built — approved by user: no
- 2026-10-29 — approved by the user in chat ("approve the stories") — Status: active
