# Tasks on you: WinCraft

Updated: 2026-11-12. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 29
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 28. Install 0.20.111: open a chat that has no window of its own

- **State:** Ready to test (0.20.111 released 2026-11-12; the release has wincraft.exe and its checksum).
- **Kind:** test · **Added:** 2026-11-12 (PC)
- **Why:** a double-click on a chat shown only in the main Claude window did nothing. Now WinCraft presses that chat in the Claude sidebar; only your screen shows that it works.

**Steps**

1. Press `Alt+Space` (your palette hotkey), type `update`, pick **Update WinCraft** and press `Enter`. The bottom right corner of the palette then shows "WinCraft v0.20.111" or newer.
2. Press `Win+Alt+X` and switch **Only open windows** off, so chats without a window of their own show too.
3. Double-click a chat that has no window of its own and whose name you can see in the main Claude window's sidebar (the list on the left), for example **Windows PC Setup**. The main Claude window comes to the front and shows that chat.
4. Try a chat whose group is folded in the Claude sidebar. A WinCraft tray message says it could not find the chat in the Claude app's list.

**Done when:** steps 3 and 4 look as described.
