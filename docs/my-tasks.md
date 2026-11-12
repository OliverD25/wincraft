# Tasks on you: WinCraft

Updated: 2026-11-12. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 30
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 28. Install 0.20.113: open, archive and rename chats from ClaudeDeck, Minimize, the pinned icon

- **State:** Ready to test (0.20.113 released 2026-11-12; the release has wincraft.exe and its checksum). Your results on 0.20.112 (2026-11-12): step 2 worked on the second click; step 3 (rename) and step 5 (archive) did nothing. 0.20.113 fixes all three.
- **Kind:** test · **Added:** 2026-11-12 (PC)
- **Why:** WinCraft now works the Claude app's own chat list and chat menu through Windows accessibility, which tests cannot reach. The 0.20.111 open did not work for you; this version fixes it.
- **Known, do not report:** a chat in a folded section of the Claude list may still not be found; WinCraft then says so in a tray message.

**Steps**

1. Press `Alt+Space` (your palette hotkey), type `update`, pick **Update WinCraft** and press `Enter`. The bottom right corner of the palette then shows "WinCraft v0.20.113" or newer.
2. Press `Win+Alt+X` and switch **Only open windows** off. Double-click a chat that has no window of its own, for example **Windows PC Setup**. The main Claude window comes forward, its chat list opens if it was hidden, and it shows that chat.
3. Right-click a chat you can rename for a test, pick **Rename the chat…**, type a new name and press `Enter`. The name changes in the Claude app's list and in ClaudeDeck. Rename it back the same way.
4. Rename a chat in the Claude app itself (right-click it in the app's list, **Rename**). Within a few seconds ClaudeDeck shows the new name.
5. Pick a chat you no longer need, press `Delete` (or right-click, **Archive the chat**). A WinCraft tray message says it was archived, the row leaves ClaudeDeck, and the chat is under Archived in the Claude app.
6. Click the new **Minimize** button in the ClaudeDeck header (a short line, left of the close button). ClaudeDeck goes to its taskbar button; click that button to bring it back.
7. If you pinned ClaudeDeck to the taskbar before: right-click the pinned icon, **Unpin from taskbar**, open ClaudeDeck, then right-click its taskbar button and **Pin to taskbar** again. The pin now shows the ClaudeDeck icon.

**Done when:** steps 2 to 7 look as described.
