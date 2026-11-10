# Tasks on you: WinCraft

Updated: 2026-11-10. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

## Open

### 2. Install the new attention hook: plain-text questions turn orange too

- **State:** Ready to do. The new hook is built and tested; this session may
  not write into `C:\Users\Admin\.claude\hooks\` (a guard blocks it).
- **Kind:** install · **Added:** 2026-11-10 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** a reply that asks you something in plain text (a command to run,
  a "?" at the end) did not mark its window. Only the clickable question
  card did. The new hook reads the end of the last reply when a turn ends.
- **Known, do not report:** a reply that only mentions open tasks on you
  ("On you: …") is marked too; the mark goes away when you send a message.

**Steps**

1. Open a WSL terminal and paste this block. It copies the new hook into
   place and commits it in the config repo:

   ```bash
   cp "/mnt/c/Users/Admin/AppData/Local/Temp/claude/9e02bdafa49d6b9c/db2a44a3-eed1-4ce2-8060-89ab0229d84b/scratchpad/claude-attention.py" /mnt/c/Users/Admin/.claude/hooks/claude-attention.py && cd /mnt/c/Users/Admin/.claude && git add hooks/claude-attention.py && git commit -m "claude-attention: mark a window when the last reply asks in plain text" -m "A reply that asked the user to run a command or answer a question in plain text left its window unmarked, because Stop only cleared marks. Stop now reads the end of the last reply and marks the window when it asks something." && git log --oneline -1
   ```

   You should see one new commit line.
2. Tell the agent "hook installed". The agent then asks a plain-text
   question in another window to test it.

**Done when:** a Claude window whose reply ends with a plain-text question
turns orange in WindowDeck, and clears after you reply.

## Done

- **1. Install 0.20.90 and test the drop gap and the "waiting for you" marks**: done 2026-11-10. The user confirmed step 3 ("1.3 is passed") and steps 5-6 ("Orange, cleared after").
