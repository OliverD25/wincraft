# Tasks on you: WinCraft

Updated: 2026-11-27. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 66
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 65. Check that the ClaudeDeck pin no longer blinks

- **State:** Ready to test. 0.20.145 is released (checked 2026-11-27).
- **Kind:** test · **Added:** 2026-11-27 (PC)
- **Why:** you saw the ClaudeDeck window blink (vanish and come back)
  each time you pinned or unpinned it. The pin changed the window's
  style, and WinCraft hid and showed the window to fix the style. Now the
  pin changes only whether the window stays on top. Fixed in 0.20.145.
- **Agent can check:** nothing; only you can see the window.

**Steps**

1. Update WinCraft: press `Alt+Space`, type "update", pick
   **Update WinCraft**, press `Enter`. The palette's bottom line should
   say v0.20.145.
2. Press `Win+Alt+X` to bring ClaudeDeck to the front.
3. Click the pin in the ClaudeDeck header (tooltip **Stay on top**)
   a few times. The window should stay in place with no blink.
4. With the pin on, click another window. ClaudeDeck stays above it.
   With the pin off, the other window covers ClaudeDeck.
5. Tell this chat "done 65", or what looked wrong.

**Done when:** the pin works and the window does not blink.

### 64. Look at the new ClaudeDeck groups and cold chats

- **State:** Ready to test. 0.20.144 is released (checked 2026-11-26).
- **Kind:** test · **Added:** 2026-11-26 (PC)
- **Why:** you approved design option A for clearer groups and a
  foldable cold chats area. Built in 0.20.144. Nobody has seen it on a
  real screen yet.
- **Agent can check:** nothing; only you can see the window.

**Steps**

1. Update WinCraft: press `Alt+Space`, type "update", pick
   **Update WinCraft**, press `Enter`. The palette's bottom line should
   say v0.20.144.
2. Press `Win+Alt+X` to bring ClaudeDeck to the front. Group names are
   larger and white (not grey), with a thin line between groups.
3. At the end of the list, find the darker area with the snowflake and
   **Cold chats** in blue, and "N chats · a – b days" at the right.
4. Click that header. The cold chats open under it, with grey names.
   Click it again. They fold.
5. Use `↓` to reach the **Cold chats** header. Press `→` (opens),
   `←` (folds), then `Space` (opens again).
6. Drag one cold chat onto a group. It moves into that group.
7. Tell this chat "done 64", or what looked wrong.

**Done when:** the groups and the cold area look right to you, and
folding works with the mouse and the keys.


