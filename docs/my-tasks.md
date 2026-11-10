# Tasks on you: WinCraft

Updated: 2026-11-10. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

## Open

### 5. Install the attention hook fix: the task reminder line no longer marks a window

- **State:** Ready to do. The fix is built and tested; this session may not
  write into `C:\Users\Admin\.claude\hooks\` (a guard blocks it).
- **Kind:** install · **Added:** 2026-11-11 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** a reply that ended with the "On you: N tasks" reminder line kept
  its window orange with nothing to answer (card "WinCraft Rust",
  00:17-00:35). A request inside that line ("tell me ...") still marks it.

**Steps**

1. Open a WSL terminal and paste this block. It copies the fixed hook into
   place and commits it in the config repo:

   ```bash
   cp "/mnt/c/Users/Admin/AppData/Local/Temp/claude/9e02bdafa49d6b9c/db2a44a3-eed1-4ce2-8060-89ab0229d84b/scratchpad/claude-attention.py" /mnt/c/Users/Admin/.claude/hooks/claude-attention.py && cd /mnt/c/Users/Admin/.claude && git add hooks/claude-attention.py && git commit -m "claude-attention: the open-tasks reminder line is not a question" -m "A reply that ended with the 'On you: N tasks' line kept its window orange although nothing waited for an answer. 'on you:' is no longer an asking phrase; a real request in that line still has 'tell me'." && git log --oneline -1
   ```

   You should see one new commit line.
2. Tell the agent "installed".

**Done when:** the hook is installed and a reply that ends with only the
task reminder line leaves its card without an orange frame.

## Done

- **1. Install 0.20.90 and test the drop gap and the "waiting for you" marks**: done 2026-11-10. The user confirmed step 3 ("1.3 is passed") and steps 5-6 ("Orange, cleared after").
- **2. Install the new attention hook: plain-text questions turn orange too**: done 2026-11-10. Installed in config commit dfbc258; the user saw the orange card, and the log shows "Stop (reply asks)" marking and UserPromptSubmit clearing 4 of 4 targets.
- **4. Install the attention hook fix: next-step box questions stay orange**: done 2026-11-11. Installed in config commit c4ea72d; the user saw the card stay orange ("Stays orange"), and the log shows the mark cleared by the user's message.
- **3. Screenshot WindowDeck when a card is orange with no question waiting**: done 2026-11-11. The screenshot showed card "WinCraft Rust" (this chat); the log showed the hook marked it at 00:17:43 because the reply ended with the "On you:" line. Fixed in task 5.
