# Tasks on you: WinCraft

Updated: 2026-11-13. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 38
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 37. Install 0.20.121: see server jobs started by PC chats in ClaudeDeck

- **State:** Wait: the 0.20.121 release is still building on GitHub.
- **Kind:** test · **Added:** 2026-11-13 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** you asked to see when PC chats make the homelab servers work.
  Nothing was checked on screen yet; the server read itself ran on all
  three servers.
- **Known, do not report:** a job a timer or the boot starts shows only
  when a chat's ssh command named that unit. A job matched only by its
  repo folder shows on the server line and the Servers tab, but does not
  change any chat's status. The WindowDeck Claude board is not changed.
- **Agent can check:** the probe output on the servers (read-only ssh).

**Steps**

1. Press `Alt+Space`, type `update`, pick **Update WinCraft**, press
   `Enter`. WinCraft restarts as 0.20.121.
2. In a PC chat, ask Claude to start a long job on a server in the
   background, for example: "on rdehome2, run `sleep 900` with
   systemd-run --user --unit=cc-test-sleep, then end your turn".
3. Press `Win+Alt+X` to open ClaudeDeck. That chat's row should show the
   outlined server mark, the word **job on a server**, and the line
   "Runs on home2: cc-test-sleep · …".
4. Rest the mouse on the row. The panel should list the job: server, name,
   time, CPU, RAM, and the command.
5. On **Chats**, home2's line should read "1 job from 1 PC chat · CPU …"
   instead of "No chat working".
6. Open **Servers**. home2's card should start with the section
   **Work from the PC**, with that chat's name and the job. Press `Enter`
   on the row: ClaudeDeck goes to that chat on **Chats**.
7. After 15 minutes (or after you stop the job), the row should stay in
   **Work from the PC**, greyed, with "ran 15 min · ended well".
8. Tell this chat "done 37", or what looks wrong (a screenshot helps).

**Done when:** steps 3 to 7 work as written.

