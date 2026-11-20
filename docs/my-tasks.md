# Tasks on you: WinCraft

Updated: 2026-11-18. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 61
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 60. Check that a chat of a new project shows in ClaudeDeck

- **State:** Wait: 0.20.140 is being built.
- **Kind:** test · **Added:** 2026-11-20 (PC)
- **Why:** the fix for "I opened a new session in a new window and it did
  not appear in ClaudeDeck" (0.20.140). The chat went into your renamed,
  folded group **Litpolibat**, which was still ClaudeDeck's **New** group.
- **Agent can check:** `claude_deck.list.json` shows the chat outside
  Litpolibat.

**Steps**

1. Update WinCraft: press `Alt+Space`, type "update", pick
   **Update WinCraft**, press `Enter`. The About page should say 0.20.140.
2. In ClaudeDeck, unfold **Litpolibat** (click its header). The chat
   **Agentic Payment Test** is there: it went in before the fix.
3. Right-click **Agentic Payment Test**, pick **New group with this chat**.
   A new group with this chat should show, unfolded.
4. Open a new Claude Code chat in a folder no ClaudeDeck group holds yet.
   Within a few seconds it should show in an unfolded **New** group at the
   top. **Litpolibat** keeps only its own chats.
5. Tell this chat "done 60", or what looked wrong.

**Done when:** the new chat shows in **New** without unfolding anything.
