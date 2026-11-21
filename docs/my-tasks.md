# Tasks on you: WinCraft

Updated: 2026-11-22. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 62
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 61. Check that a cold chat can be dragged into a ClaudeDeck group

- **State:** Ready to test. 0.20.141 is released (checked 2026-11-22).
- **Kind:** test · **Added:** 2026-11-22 (PC)
- **Why:** you could not drag chats from the cold chats line in
  ClaudeDeck. In 0.20.141 a cold chat can be dragged into a group.
- **Agent can check:** `claude_deck.list.json` has the dragged chat under
  `warm`.

**Steps**

1. Update WinCraft: press `Alt+Space`, type "update", pick
   **Update WinCraft**, press `Enter`. The About page should say 0.20.141.
2. In ClaudeDeck, click the grey line **8 cold chats** to list them.
3. Drag **Agentic Payment Test** onto a group, for example **Other
   Sessions**, and let go. It should show in that group, not in the cold
   line.
4. Tell this chat "done 61", or what looked wrong.

**Done when:** the dragged chat shows in the group you dropped it on. It
stays there for one day, then goes back to the cold line if it is still
idle.
