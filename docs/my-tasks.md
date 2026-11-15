# Tasks on you: WinCraft

Updated: 2026-11-14. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 52
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 48. Test that WinCraft no longer freezes while ClaudeDeck is in the tray

- **State:** Ready to test. Release 0.20.132 checked on GitHub 2026-11-15 (exe and checksums are there).
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

### 49. Take two ClaudeDeck pictures for the marketing page

- **State:** Ready to test. ClaudeDeck works now; nothing waits for a release.
- **Kind:** click · **Added:** 2026-11-15 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** the ClaudeDeck marketing page (marketing/features/claudedeck.md) and the
  Reddit post need a picture; posts without one get much less attention.
- **Agent can check:** the files `marketing/media/claudedeck-1.png` and
  `marketing/media/claudedeck-2.gif` exist.

**Steps**

1. Have 6-10 Claude Code chats open in the Claude app, with at least one waiting
   for your approval and one working.
2. Press `Win+Alt+X` to bring ClaudeDeck to the front.
3. Press `Win+Shift+S` (the Windows Snipping Tool), pick the window mode, and click
   ClaudeDeck's window. Save the picture as `claudedeck-1.png` in your `_io` folder
   (`E:\codespace\_claude_code\_rde\wincraft_windows_utilities_rust\_io`).
4. Optional: record a short GIF (for example with ScreenToGif) of a chat turning
   to "Needs approval", then `Win+Alt+X` and `Enter` bringing it to the front. Save
   it there as `claudedeck-2.gif`.
5. Tell this chat "pictures are in io". I move them into `marketing/media/` and
   check that no private chat names show.

**Done when:** both files are in `marketing/media/` and show no private names you
do not want public.
