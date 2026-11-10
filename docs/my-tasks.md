# Tasks on you: WinCraft

Updated: 2026-11-10. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

## Open

### 3. Screenshot WindowDeck when a card is orange with no question waiting

- **State:** Wait: until a Claude window's card is orange while its chat is
  not waiting for you.
- **Kind:** report · **Added:** 2026-11-10 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** you saw this chat's card stay orange. The WinCraft log shows no
  mark on that window after 22:31:50, so the cause is not found yet.
- **Agent can check:** the WinCraft log (`%LOCALAPPDATA%\WinCraft\wincraft.log`,
  lines `attention:`) and `~/.claude/claude-attention.log`, at the time you give.

**Steps**

1. When you see an orange frame on a Claude window's card in WindowDeck, and
   that chat does not wait for an answer, press `Win+Shift+S` and take a
   screenshot of WindowDeck.
2. Paste the screenshot into this chat and write the time you saw it.

**Done when:** the agent has found which mark set the orange frame, from the
log at that time.

### 4. Install the attention hook fix: next-step box questions stay orange

- **State:** Ready to test. Installed 2026-11-10 (config commit c4ea72d);
  left: see a card stay orange while its next-step box question waits.
- **Kind:** install · **Added:** 2026-11-10 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** a question in the next-step box (buttons like "It's gone" /
  "Still blocks") marked the window, but a late tool event from the same
  session cleared it in the same second (23:08:45, newsHQ). Now only your
  message or the next turn end clears such a mark.

**Steps**

1. Open a WSL terminal and paste this block. It copies the fixed hook into
   place and commits it in the config repo:

   ```bash
   cp "/mnt/c/Users/Admin/AppData/Local/Temp/claude/9e02bdafa49d6b9c/db2a44a3-eed1-4ce2-8060-89ab0229d84b/scratchpad/claude-attention.py" /mnt/c/Users/Admin/.claude/hooks/claude-attention.py && cd /mnt/c/Users/Admin/.claude && git add hooks/claude-attention.py && git commit -m "claude-attention: keep a turn-end mark until the user answers" -m "A late PostToolUse from the same session cleared the mark in the same second it was set, so next-step box questions never stayed orange. A turn-end mark now ignores PostToolUse. The transcript read also stops only at the user's own message, not at Stop hook feedback or background task results." && git log --oneline -1
   ```

   You should see one new commit line.
2. Tell the agent "fix installed".

**Done when:** a window whose reply ends with a next-step box question stays
orange in WindowDeck until you answer.

## Done

- **1. Install 0.20.90 and test the drop gap and the "waiting for you" marks**: done 2026-11-10. The user confirmed step 3 ("1.3 is passed") and steps 5-6 ("Orange, cleared after").
- **2. Install the new attention hook: plain-text questions turn orange too**: done 2026-11-10. Installed in config commit dfbc258; the user saw the orange card, and the log shows "Stop (reply asks)" marking and UserPromptSubmit clearing 4 of 4 targets.
