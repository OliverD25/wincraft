# Tasks on you: WinCraft

Updated: 2026-11-14. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 48
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 47. Test that ClaudeDeck opens without a white flash

- **State:** Wait: release 0.20.131 is being built.
- **Kind:** test · **Added:** 2026-11-15 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** ClaudeDeck now stays covered until it has drawn itself (0.20.131). Only
  your eyes can see a flash; the agent can only check the log.
- **Agent can check:** the line "ClaudeDeck: its window was shown before it had
  drawn itself" should not be in `%LOCALAPPDATA%\WinCraft\wincraft.log`.

**Steps**

1. Update WinCraft: press `Alt+Space`, type "update", pick **Update WinCraft**, press
   `Enter`. Settings → About should then show 0.20.131.
2. Press `Win+Alt+X`. ClaudeDeck should appear at its place with no white
   rectangle first.
3. Press `Win+Alt+X` again (it hides to the tray), then once more. It should come
   back with no white flash. Do this 3-4 times.
4. Right-click ClaudeDeck's own tray icon and pick **Hide ClaudeDeck**, then click the
   tray icon. Again no flash.
5. Click the pin in ClaudeDeck's header (**Stay on top**) twice. ClaudeDeck should
   stay on screen and keep no taskbar button.
6. Press `Ctrl+M` in ClaudeDeck (mini mode). Rest the mouse on a mark: the hover
   panel should appear with no flash. Press `Ctrl+,`: the small quick settings
   window should appear with no flash. Press `Esc`, then `Ctrl+M` to go back.
7. Tell this chat "done 47", or say which step still flashed.

**Done when:** no step shows a white flash and ClaudeDeck behaves as before.
