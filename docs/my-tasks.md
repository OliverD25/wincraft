# Tasks on you: WinCraft

Updated: 2026-11-13. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 34
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 33. Install 0.20.117: the News tab grouped by chat

- **State:** Ready to test (0.20.117 released 2026-11-13; wincraft.exe and SHA256SUMS.txt are in the release).
- **Kind:** test · **Added:** 2026-11-13 (PC, branch work/design/groups-fixes)
- **Why:** the News tab now shows one block per chat (design section 14,
  option B). Tests drew it, but nobody looked at it in the running app.
- **Known, do not report:** a waiting chat with no unseen reply has no amber
  count on its block line. That is on purpose: the counts add up to the
  count on the **News** tab.

**Steps**

1. Press `Alt+Space`, type `update`, pick **Update WinCraft**, press `Enter`.
   WinCraft restarts as 0.20.117.
2. Press `Win+Alt+X`, then click the **News** tab.
3. You should see each chat once, as a block line: its status mark, its name
   in bold, the project and, at the right, an amber count of its unseen
   replies. Its replies sit under it, two lines each, with the time at the
   right. A chat's name no longer repeats on every reply.
4. Find a chat with seen replies. Under its replies is an amber line
   **2 earlier replies** (or **1 earlier reply**). Click it. All of that
   chat's replies show.
5. Click one reply. It opens in place with the whole text and the button
   **Open the chat**.
6. Click the chat's name on a block line. The chat opens in the Claude app.
7. With the keyboard: press `Down` until a block line is picked, then
   `Left` folds the chat and `Right` unfolds it.
8. Tell any session of this project "done 33", or say which step failed.

**Done when:** steps 3 to 6 behave as written.
