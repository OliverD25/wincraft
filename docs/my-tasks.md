# Tasks on you: WinCraft

Updated: 2026-11-10. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

## Open

### 1. Install 0.20.90 and test the drop gap and the "waiting for you" marks

- **State:** Ready to test. 0.20.90 installed and the hook events registered
  2026-11-10; the live test in this window marked and cleared it. Left: step 3
  and steps 5-6 with two windows.
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

## Done
