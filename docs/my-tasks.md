# Tasks on you: WinCraft

Updated: 2026-11-14. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 49
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 48. Test that WinCraft no longer freezes while ClaudeDeck is in the tray

- **State:** Wait: release 0.20.132 is being built.
- **Kind:** test · **Added:** 2026-11-15 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** 0.20.131 froze WinCraft at 16:44 while ClaudeDeck was hidden in the tray
  (two of its threads waited for each other). 0.20.132 fixes it; only a real run
  over some minutes shows that the freeze is gone.
- **Agent can check:** the log `%LOCALAPPDATA%\WinCraft\wincraft.log` has no
  "arrived while the host was busy" lines after the update, and the
  "saved N windows (timer)" lines keep coming every 30 s.

**Steps**

1. Update WinCraft: press `Alt+Space`, type "update", pick **Update WinCraft**, press
   `Enter`. Settings → About should then show 0.20.132.
2. Press `Win+Alt+X` to open ClaudeDeck, then press `Win+Alt+X` again to hide it to
   the tray.
3. Work as usual for at least 5 minutes. Use WinCraft now and then: `Alt+Space` for
   the palette, `Win+Alt+X` to show and hide ClaudeDeck. Nothing should freeze.
4. Click ClaudeDeck's own tray icon. ClaudeDeck should come back at once, with no
   white flash.
5. Tell this chat "done 48", or the time it froze if it did.

**Done when:** WinCraft stays responsive with ClaudeDeck in the tray, and the log
shows no "host was busy" lines.
