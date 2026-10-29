# US-006: Update WinCraft and see what happens

Status: active
Area: updates
Source: proposed by Claude from the features built, next-steps review 2026-10-29 (not the user's own draft)

**Story:** As a user, I want to update WinCraft from the palette or About and see each step, so that I know it is working and my version is safe if it fails.

## Acceptance criteria

- [ ] A newer release shows an "Update ready" row above Recent; Enter on it updates.
- [ ] A panel shows each step: checking, downloading with the percent, checking the download, restarting.
  Test: src/core/update_notice/tests.rs::each_stage_says_one_thing_and_names_wincraft
- [ ] A release WinCraft already knows installs at once, without asking GitHub again.
  Test: src/core/host/tests.rs::update_now_with_a_known_release_downloads_without_a_check
- [ ] When GitHub limits requests, WinCraft uses the releases page and says so honestly, never "offline".
  Test: src/core/host/tests.rs::github_s_limit_sends_checks_to_the_releases_page_and_pauses_automatic_ones
- [ ] A failed update keeps the current version and says why.
  Test: src/core/update_notice/tests.rs::a_failed_update_stays_eight_seconds
- [ ] Ctrl+Del on the update row hides it until a newer version comes.
  Test: src/core/host/tests.rs::ctrl_del_hides_a_release_until_a_newer_tag_comes

## History

- 2026-10-29 — created as draft from the features built — approved by user: no
- 2026-10-29 — approved by the user in chat ("approve the stories") — Status: active
