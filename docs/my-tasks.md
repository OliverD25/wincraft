# Tasks on you: WinCraft

Updated: 2026-11-14. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 45
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 44. Install 0.20.127: "Show on all desktops" in WindowDeck's card menu

- **State:** Ready to test
- **Kind:** test · **Added:** 2026-11-15 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** you asked for a "show on all displays" item in the card's
  right-click menu; you chose the WindowDeck card menu and one window, like
  Windows' "Show this window on all desktops". The agent checked it only
  with unit tests, not on screen.
- **Agent can check:** nothing on screen.

**Steps**

1. Press `Alt+Space`, type `update`, pick **Update WinCraft**, press
   `Enter`. WinCraft restarts as 0.20.127.
2. Hover the Chrome button on the taskbar so WindowDeck opens.
3. Right-click a window card. After **Move to desktop** there is a check
   item **Show on all desktops**, not checked.
4. Click it. Switch to another desktop with `Ctrl+Win+Right`: the window
   is there too.
5. Open WindowDeck again and right-click the same card: the item now has a
   check mark. Click it again: the window stays only on the desktop you are
   on now.
6. Tell this chat "done 44", or what looks wrong (a screenshot helps).

**Done when:** steps 3 to 5 work as written.

