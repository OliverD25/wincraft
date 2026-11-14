# Tasks on you: WinCraft

Updated: 2026-11-14. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 44
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 43. Install 0.20.126: the homelab admin chat no longer shows "Job"

- **State:** Wait: the 0.20.126 release is still building on GitHub.
- **Kind:** test · **Added:** 2026-11-15 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** the "Ubuntu 2 server homelab admin" chat showed "Job, runs 6
  jobs on home1, home2", but it ran no job. Old ssh calls, copied by
  Claude Code into lines with no time, claimed long-running services. The
  agent checked the fix on that chat's real transcript and the live job
  lists: 6 false jobs before, none after.
- **Agent can check:** nothing on screen.

**Steps**

1. Press `Alt+Space`, type `update`, pick **Update WinCraft**, press
   `Enter`. WinCraft restarts as 0.20.126.
2. Press `Win+Alt+X` and wait about 30 seconds.
3. Find "Ubuntu 2 server homelab admin" under ubuntu-homelab-server-setup.
   It should show "Idle", not "Job", and no "Runs 6 jobs" line.
4. On the **Servers** tab, the home1 and home2 lines should count fewer
   jobs from PC chats than before (it was "8 jobs from 3 PC chats" and
   "6 jobs from 4 PC chats").
5. Tell this chat "done 43", or what looks wrong (a screenshot helps).

**Done when:** step 3 shows Idle.

