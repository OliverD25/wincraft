# Tasks on you: WinCraft

Updated: 2026-11-13. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 39
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 38. Install 0.20.122: ClaudeDeck in the tray, and "Job" on the WindowDeck board

- **State:** Wait: the 0.20.122 release is still building on GitHub.
- **Kind:** test · **Added:** 2026-11-14 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** you asked to hide ClaudeDeck from the taskbar and `Alt+Tab`, keep
  it in the tray, and show server jobs on the WindowDeck board. Only the
  start of the tray icon was seen in a test copy; clicks were not tried.
- **Known, do not report:** with **Live in the tray (no taskbar button)** on,
  ClaudeDeck reads the chats and the servers from WinCraft's start, also
  while its window was never opened, so the tray count is right after a
  reboot. The badge counts only chats that wait for you; new replies are
  only in the tooltip.
- **Agent can check:** nothing on screen.

**Steps**

1. Press `Alt+Space`, type `update`, pick **Update WinCraft**, press
   `Enter`. WinCraft restarts as 0.20.122.
2. Press `Win+Alt+X`. ClaudeDeck opens. The taskbar should have no
   ClaudeDeck button, and `Alt+Tab` should not list ClaudeDeck.
3. Find the new amber ClaudeDeck tile in the tray. If it is not next to the
   clock, click the `^` arrow beside the clock and drag the amber tile down
   next to the other icons.
4. Click the window's close (×) button. The window should hide, and the
   tray tile stays.
5. Left-click the amber tile. ClaudeDeck should come back at the same place,
   on the same tab. Left-click again: it hides.
6. Right-click the tile. The menu should have **Show ClaudeDeck** (or **Hide
   ClaudeDeck**), **Mini mode**, **Quick settings…**, **All settings…**.
7. When a chat waits for you, the tile should show a small red number. Rest
   the mouse on it: "ClaudeDeck — 1 chat waits for you".
8. While a PC chat runs a job on a server (as in task 37), open WindowDeck on the
   Claude app's taskbar button (`Win+Alt+A`, then its number) and press `B`
   for **Claude board**. That chat should be in the **Working** column, after the
   working chats, with the outlined server mark and "Job 1 h 48 m · home2".
9. Tell this chat "done 38", or what looks wrong (a screenshot helps).

**Done when:** steps 2 to 8 work as written.

