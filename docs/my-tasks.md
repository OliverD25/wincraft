# Tasks on you: WinCraft

Updated: 2026-11-13. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 37
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 36. Install 0.20.120: the Servers tab in ClaudeDeck

- **State:** Ready to test. Release 0.20.120 is published with wincraft.exe and SHA256SUMS.txt (checked 2026-11-13).
- **Kind:** test · **Added:** 2026-11-13 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** you asked for a third ClaudeDeck tab with the servers, their
  chats and processes. It reads real servers, so only you can see it on
  screen. The ssh part was checked once on home1 by the agent.
- **Known, do not report:** "Busiest" CPU % is an average over each
  process's whole life, not the live value htop shows. Stopped systemd
  services are left out on purpose (on home1 they are 26 timer jobs).
- **Agent can check:** nothing on screen; the server read itself was
  checked on home1.

**Steps**

1. Press `Alt+Space`, type `update`, pick **Update WinCraft**, press
   `Enter`. WinCraft restarts as 0.20.120.
2. Press `Win+Alt+X` to open ClaudeDeck. Under the header you should see
   three tabs: **Chats**, **News**, **Servers**.
3. On **Chats**, under the PC chats, each server should be one short line
   (name, dot, counts such as "1 waits for you"), not a long group.
4. Click a server's line. ClaudeDeck should switch to **Servers** with that
   server's card open.
5. On a card, check the header (CPU, RAM, disk free, up time) and the
   sections **Chats**, **Claude processes**, **Busiest**, **Services**.
   On home1, **Services** should show `verdict-ramp-day.service` as
   **failed** if it is still failed.
6. Rest the mouse on the **Servers** tab name. A red count, if any, should
   list the problems ("home1: verdict-ramp-day.service failed", and so on).
7. Press `Ctrl+Tab` a few times. It should go round Chats, News, Servers.
8. On the Servers tab press `↓` and `↑` to move over the rows, `←` and `→`
   on a card header to fold and unfold it, `Enter` on a header to open a
   terminal on that server.
9. Tell this chat "done 36", or what looks wrong (a screenshot helps).

**Done when:** steps 2 to 8 work as written.
