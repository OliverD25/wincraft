# Tasks on you: WinCraft

Updated: 2026-11-23. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 63
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 62. Try notes on chats in ClaudeDeck

- **State:** Ready to test. 0.20.142 is released (checked 2026-11-23).
- **Kind:** test · **Added:** 2026-11-23 (PC)
- **Why:** you asked to pin a small note to a chat and remove it again.
  You approved design option A (a third line under the chat) with the ×
  on hover. Built in 0.20.142.
- **Agent can check:** `claude_deck.list.json` has the note under `notes`.

**Steps**

1. Update WinCraft: press `Alt+Space`, type "update", pick
   **Update WinCraft**, press `Enter`. The About page should say 0.20.142.
2. In ClaudeDeck, right-click a chat and pick **Add a note…**. A one-line
   box opens on the row. Type a short note and press `Enter`. A third line
   with the note shows under the chat, on a soft amber tint.
3. Pick another chat with the arrow keys and press `N`. Type a note, then
   press `Esc`. Nothing should be saved.
4. Rest the mouse on the first chat. Its whole note shows in the hover
   panel, and a small × shows at the end of the note line.
5. Double-click the note, change the text, press `Enter`. The new text
   shows.
6. Click the ×. The note line goes away.
7. Tell this chat "done 62", or what looked wrong.

**Done when:** you can add, change and remove a note, and it looks right
to you.
