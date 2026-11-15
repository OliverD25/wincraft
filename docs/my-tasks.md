# Tasks on you: WinCraft

Updated: 2026-11-14. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 46
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 45. Install 0.20.128, then reboot: WinCraft starts by itself and keeps windows on their desktops

- **State:** Wait: the 0.20.128 release is still building on GitHub.
- **Kind:** test · **Added:** 2026-11-15 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** after the 2026-11-15 reboot WinCraft did not start (its sign-in
  task in Task Scheduler was damaged and had never run). When you started it
  by hand, its restore pulled 10 Chrome windows onto desktop 2. 0.20.127
  fixed the pull; 0.20.128 re-creates the task, checks that Windows can start
  it, and makes a first start by hand ask with Put back.
- **Known, do not report:** right after the update WinCraft starts the sign-in
  task once to test it; that second copy closes at once and you see nothing.
- **Agent can check:** steps 2 and 5 (the task's last run time and the log).

**Steps**

1. Press `Alt+Space`, type `update`, pick **Update WinCraft**, press
   `Enter`. WinCraft restarts as 0.20.128.
2. Tell this chat "45 step 2". The agent checks that the task was re-created
   and that Windows started it once.
3. Restart Windows (Start → Power → "Restart"). Sign in as usual.
4. Wait about 2 minutes. WinCraft's tray icon should be there without you
   starting it. Chrome windows should be on their own desktops: Home and
   Sport/etc on "Home", Reading and Study/Education on "Education&Reading",
   iGaming Research and Wildo.Agency on "PlayDuck".
5. Tell this chat "done 45", or what is wrong (a screenshot of WindowDeck
   helps). The agent then reads the log of the sign-in start.

**Done when:** WinCraft started by itself after the restart and no Chrome
window landed on the wrong desktop.

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

