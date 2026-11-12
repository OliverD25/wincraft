# Tasks on you: WinCraft

Updated: 2026-11-12. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 32
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 31. Install 0.20.115: the News tab, and the hotkey on a minimized ClaudeDeck

- **State:** Ready to test (0.20.115 released 2026-11-12; the release has wincraft.exe and its checksum).
- **Kind:** test · **Added:** 2026-11-12 (PC)
- **Why:** the News tab you approved (design section 13) is built; only its drawing in tests was checked, not on screen. The hotkey fix for a minimized ClaudeDeck could not run in a test copy.
- **Known, do not report:** **Older** lists only chats whose history ClaudeDeck already reads (chats with an open window, or active in the last 24 hours).

**Steps**

1. Press `Alt+Space` (your palette hotkey), type `update`, pick **Update WinCraft** and press `Enter`. The bottom right corner of the palette then shows "WinCraft v0.20.115" or newer.
2. Press `Win+Alt+X`. Under the header are two tabs, **Chats** and **News**; **News** has an amber count when there are replies you have not seen, and chat rows with such a reply have an amber dot after the name.
3. Click **News** (or press `Ctrl+Tab`). On top: **Waits for you** (chats that ask you or wait for your OK), then **Last 24 hours**, newest first, then **Older**, folded. Server chats show the server icon and name.
4. Click one item: it opens in place with the full reply, its amber dot goes, and the **News** count drops by one. A long reply ends with **Show all · N lines**. Click **Open the chat**: the chat opens.
5. Try **Only unseen** and **Mark all seen** at the right of the **Last 24 hours** line, and click **Older** to unfold it.
6. Click the **Minimize** button in the header, then press `Win+Alt+X`: ClaudeDeck comes back at the same place.

**Done when:** steps 2 to 6 look as described.
