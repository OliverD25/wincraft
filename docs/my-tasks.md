# Tasks on you: WinCraft

Updated: 2026-11-18. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 60
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 59. Check that QuickJump keeps a window at number 9

- **State:** Ready to test. 0.20.139 is released (checked 2026-11-18).
- **Kind:** test · **Added:** 2026-11-18 (PC)
- **Why:** the fix for "could not place the window to number 9" (0.20.139)
  needs a check on your real pins.
- **Agent can check:** nothing; only you see the jump.

**Steps**

1. Update WinCraft: press `Alt+Space`, type "update", pick
   **Update WinCraft**, press `Enter`. The About page should say 0.20.139.
2. Click a window that is not pinned yet. Press `` Win+` ``, then `Alt+9`.
   The notice should say it is number 9, even with fewer than nine pins.
3. Press `` Win+` ``, then `9`. That window should come to the front.
4. Press `` Win+` `` twice to open **QuickJump · Quick access**. The card
   should show number 9, and your other pins keep their old numbers.
5. Unpin a lower card there (`Del`). The card at 9 should still say 9.
6. Tell this chat "done 59", or what looked wrong.

**Done when:** the window stays at 9 and the other numbers do not move.
