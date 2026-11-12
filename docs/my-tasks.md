# Tasks on you: WinCraft

Updated: 2026-11-13. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 35
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 34. Install 0.20.118: fold any News chat with its arrow

- **State:** Ready to test (0.20.118 released 2026-11-13; wincraft.exe and SHA256SUMS.txt are in the release).
- **Kind:** test · **Added:** 2026-11-13 (PC, branch work/design/groups-fixes)
- **Why:** you asked that a chat with unread replies can be folded too.
  Tests checked it, but nobody tried it in the running app.

**Steps**

1. Press `Alt+Space`, type `update`, pick **Update WinCraft**, press `Enter`.
   WinCraft restarts as 0.20.118.
2. Press `Win+Alt+X`, then click the **News** tab.
3. Find a chat with an amber count on its block line (it has unread
   replies). Click the arrow at the left of its block line.
4. You should see only the block line of that chat. The amber count stays.
   The arrow points right.
5. Click the arrow again. The unread replies show again, still unread.
6. Press `Down` until that block line is picked. `Left` folds the chat,
   `Right` shows its replies.
7. Tell any session of this project "done 34", or say which step failed.

**Done when:** steps 4 and 5 behave as written.
