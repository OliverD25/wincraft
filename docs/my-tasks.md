# Tasks on you: WinCraft

Updated: 2026-11-14. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 55
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 53. Test that QuickJump pins survive a closed window

- **State:** Ready to test. Release 0.20.134 checked on GitHub 2026-11-16 (exe and checksums are there).
- **Kind:** test · **Added:** 2026-11-16 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** 0.20.134 lets a pinned Claude chat reopen its chat by its link, makes pin
  titles follow their window, and adds `R` to re-pin. Only unit tests cover it so far.
- **Agent can check:** the log line "Opening ... in the Claude app" after a jump, and
  `claude_session` fields in `%LOCALAPPDATA%\WinCraft\plugins\quick_jump.state.json`.

**Steps**

1. Update WinCraft: press `Alt+Space`, type "update", pick **Update WinCraft**, press
   `Enter`. Settings → About should then show 0.20.134.
2. Open each pinned Claude chat's window once (so its pin learns which chat it is).
   Then close one of those chat windows.
3. Press `` Win+` `` twice to open Quick access. That card should say
   **Claude · opens the chat** instead of **not open**.
4. Press its number. The Claude app should show that chat.
5. Rename a pinned chat in the Claude app. Its pin should still find it.
6. Open a window you want to pin (any app), press `` Win+` `` twice, pick a card that
   says **not open**, and press `R`. The card should now hold that window, with the
   same number.
7. Tell this chat "done 53", or which step went wrong.

**Done when:** a closed chat pin opens its chat, a renamed chat keeps its pin, and `R`
re-pins.

### 54. Test that ClaudeDeck stays out of Win+Tab

- **State:** Ready to test. Release 0.20.134 checked on GitHub 2026-11-16 (exe and checksums are there).
- **Kind:** test · **Added:** 2026-11-16 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** pressing ClaudeDeck's pin dropped its tray window style, so it showed in
  Win+Tab. 0.20.134 puts the style back by itself.
- **Agent can check:** ClaudeDeck's window has the tool-window flag
  (`WS_EX_TOOLWINDOW`) and not the app-window flag; the log may say "ClaudeDeck:
  Windows lost the tray window style; put back".

**Steps**

1. Update WinCraft: press `Alt+Space`, type "update", pick **Update WinCraft**, press
   `Enter`. Settings → About should then show 0.20.134.
2. Press `Win+Alt+X` to open ClaudeDeck. Click the pin in its header (tooltip
   **Stay on top**) twice.
3. Press `Win+Tab` and then `Alt+Tab`. ClaudeDeck should be in neither.
4. Press `Win+Alt+X` to hide it to the tray. Open Settings on ClaudeDeck's page and
   switch **Stay on top** off and on. ClaudeDeck should stay hidden.
5. Click ClaudeDeck's tray icon. It should appear with no white flash, and still not
   be in `Win+Tab`.
6. Tell this chat "done 54", or which step went wrong.

**Done when:** ClaudeDeck never shows in Win+Tab or Alt+Tab, and a pin change while
hidden keeps it hidden.
