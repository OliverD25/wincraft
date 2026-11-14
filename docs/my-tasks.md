# Tasks on you: WinCraft

Updated: 2026-11-14. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 43
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 42. Install 0.20.125: see the context ring on each ClaudeDeck chat

- **State:** Ready to test (0.20.125 is released).
- **Kind:** test · **Added:** 2026-11-14 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** you asked for a picture of each chat's context size, and you
  approved the design (artifact v47, placement A). The agent saw green and
  grey rings in a test copy; the hover line, the light theme and the amber
  and red rings were checked only by unit tests.
- **Known, do not report:** right after WinCraft starts, all rings show only
  the empty track for 15-30 seconds, until the chat files are read. Chats
  without an open window and with no activity in the last 24 hours always
  show the empty track. Mini mode has no rings. Server chats may show a too
  low "compacted" count.
- **Agent can check:** nothing on screen.

**Steps**

1. Press `Alt+Space`, type `update`, pick **Update WinCraft**, press
   `Enter`. WinCraft restarts as 0.20.125.
2. Press `Win+Alt+X` and wait about 30 seconds. Each chat row should have a
   small ring just before the time, all rings in one column.
3. The colours: grey for a fresh chat (under 15% of 777k), green up to 45%,
   amber up to 75%, red above. A long chat like this WinCraft one should be
   green or amber.
4. Rest the mouse on a chat. The hover panel should have a line like
   "Context 318k of 777k (41%)", with "· compacted 2 times" when the chat
   was compacted.
5. Tell this chat "done 42", or what looks wrong (a screenshot helps).

**Done when:** steps 2 to 4 work as written.

