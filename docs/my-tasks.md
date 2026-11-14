# Tasks on you: WinCraft

Updated: 2026-11-13. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 40
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 39. Fix the attention hook so ClaudeDeck stops showing "asks you" for chats that ask nothing

- **State:** Ready to test
- **Kind:** edit · **Added:** 2026-11-14 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** the global hook `claude-attention.py` marks a chat as asking when
  its last reply contains the word "click ". Replies to the ui-map Stop hook
  say "my reply had no click steps", so "F1os - New Suppliers" and
  "VerdictAI (fork) - Fable Research and Plans" showed **Asks** with nothing
  to answer. WinCraft sessions may not edit the global config, so the edit
  is yours. The command was tested on a copy of the file.
- **Agent can check:** `grep -c 'click \*\*' /c/Users/Admin/.claude/hooks/claude-attention.py` prints 1.

**Steps**

1. In a WSL terminal, run this one command (it changes one line):

   ```bash
   sed -i 's/"you need to", "click ", "your answer"/"you need to", "click **", "click the ", "click on ", "click \\"", "click `", "your answer"/' /mnt/c/Users/Admin/.claude/hooks/claude-attention.py && grep -c 'click \*\*' /mnt/c/Users/Admin/.claude/hooks/claude-attention.py
   ```

   It should print `1`.
2. The two chats keep their old **Asks** mark until you send each one a
   message. A reply from a chat clears it.
3. Tell this chat "done 39".

**Done when:** the command printed `1`, and no new chat shows **Asks** after
a reply that only says "no click steps".

