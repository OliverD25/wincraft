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

## Done

- **1. Install 0.20.90 and test the drop gap and the "waiting for you" marks**: done 2026-11-10. The user confirmed step 3 ("1.3 is passed") and steps 5-6 ("Orange, cleared after").
- **2. Install the new attention hook: plain-text questions turn orange too**: done 2026-11-10. Installed in config commit dfbc258; the user saw the orange card, and the log shows "Stop (reply asks)" marking and UserPromptSubmit clearing 4 of 4 targets.
