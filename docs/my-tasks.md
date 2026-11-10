# Tasks on you: WinCraft

Updated: 2026-11-10. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

## Open

### 1. Install 0.20.90 and test the drop gap and the "waiting for you" marks

- **State:** Ready to test. 0.20.90 installed and the hook events registered
  2026-11-10; steps 5-6 passed with two windows ("Orange, cleared after").
  Left: step 3, the drop gap.
- **Kind:** test · **Added:** 2026-11-10 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** two features of 0.20.90 need a check on the real windows: the
  WindowDeck drop gap, and the orange mark on a Claude window whose session
  asks a question.
- **Known, do not report:** a window that waited before the update is not
  marked; only new questions are.
- **Agent can check:** the WinCraft log (`%LOCALAPPDATA%\WinCraft\wincraft.log`,
  lines `attention: Claude session`) and `~/.claude/claude-attention.log`.

**Steps**

1. Right-click the WinCraft icon in the tray and click **Settings…**.
2. Click **About** in the left list, then **Check for updates**, then
   **Update now**. WinCraft restarts; About shows 0.20.90.
3. Open WindowDeck and drag a window card next to a card with an orange
   frame. You should see the other cards slide apart and an empty slot
   with a thin grey line where the card will land. Esc puts it back.
4. Tell the agent "installed". The agent registers the hook events.
5. In one Claude window, ask the session something that makes it ask you
   a question back. Switch to another Claude window before the question
   appears. You should see the waiting window's WindowDeck card turn
   orange and its taskbar button flash.
6. Answer the question. The orange mark should go away.

**Done when:** the gap shows in WindowDeck, and a Claude window that asks a
question turns orange and clears after the answer.

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
