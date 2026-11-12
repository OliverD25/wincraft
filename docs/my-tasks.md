# Tasks on you: WinCraft

Updated: 2026-11-13. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 33
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 32. Install 0.20.116: Show in the Claude app from ClaudeDeck

- **State:** Ready to test (0.20.116 released 2026-11-13; wincraft.exe and SHA256SUMS.txt are in the release).
- **Kind:** test · **Added:** 2026-11-13 (PC, branch claude/cranky-franklin-03f5d5)
- **Why:** the new command moves the main Claude window to a chat. The link
  it uses was tested by hand, but the menu item and the key were not tried
  in the running app.
- **Known, do not report:** the item is missing on a server chat, a fork in
  a terminal, and a folded old chat. That is on purpose.

**Steps**

1. Press `Alt+Space`, type `update`, pick **Update WinCraft**, press `Enter`.
   WinCraft restarts as 0.20.116.
2. Press `Win+Alt+X`. ClaudeDeck comes to the front.
3. Right-click a chat that has its own window. The menu shows **Show in the
   Claude app** right under **Open the chat**. Click it.
4. You should see the main Claude window come forward and show that chat.
   The chat's own window stays as it was.
5. Pick another chat with the arrow keys and press `Ctrl+Enter`. The main
   Claude window should change to that chat.
6. Press `Enter` on a chat with its own window. Only that window should come
   forward, as before; the main Claude window should not change.
7. Optional: press `Alt+Space`, type `show the picked`, pick **ClaudeDeck:
   Show the picked chat in the Claude app**, press `Enter`. The main Claude
   window shows the chat picked in ClaudeDeck.
8. Tell any session of this project "done 32", or say which step failed.

**Done when:** steps 4, 5 and 6 behave as written.
