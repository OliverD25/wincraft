# Tasks on you: WinCraft

Updated: 2026-11-13. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 36
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 35. Install 0.20.119: move windows inside an open WindowDeck group

- **State:** Wait: the 0.20.119 release is still building on GitHub.
- **Kind:** test · **Added:** 2026-11-13 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** you asked to change the order of the windows in a group. Only
  you can test it: a test copy of WinCraft must never move your real
  windows (Principle 1).
- **Known, do not report:** the new order also changes the order on the
  Windows taskbar. You chose that in the Q&A.
- **Agent can check:** the log line "applied the order of N ... windows"
  in the WinCraft log after a move.

**Steps**

1. Press `Alt+Space`, type `update`, pick **Update WinCraft**, press
   `Enter`. WinCraft restarts as 0.20.119.
2. Open WindowDeck on an app with a group of 3 or more windows, and
   click the group's stack. The group opens over the row.
3. Drag the first window's card to the right, between the second and the
   third card. You should see its picture fade, a blue line in the gap,
   and the header text "Drop between windows to move it, outside to take
   it out".
4. Release the mouse. The window should now be second, and the Windows
   taskbar should show the same new order.
5. Press `Alt + ←` and `Alt + →`. The selected window should move one place
   each time, and stay selected.
6. Set Settings → WindowDeck → **Open a group as** to **A fan**, open the
   group again, and repeat steps 3 and 4. The fan should reorder the same way.
7. Drag one window's card out of the group's frame. The header should read
   "Release to take it out of the group". Release: the window leaves the
   group and shows as its own card in the row.
8. Put it back in the group if you want: drag its card onto the stack.
9. Tell this chat "done 35", or what did not work.

**Done when:** steps 3 to 7 work as written.

