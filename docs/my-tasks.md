# Tasks on you: WinCraft

Updated: 2026-11-10. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

## Open

### 6. Review the WindowDeck groups design

- **State:** Ready to do. Page published 2026-11-11 (design artifact version 15).
- **Kind:** decision · **Added:** 2026-11-11 (PC, branch claude/gracious-panini-4299ca, design session)
- **Why:** the code work on groups waits until you accept the page or ask
  for changes.

**Steps**

1. Open https://claude.ai/artifact/S5fEimN5sS6oqHgwJVRxPn.
2. Click the page name at the top left and pick
   **Window Deck Groups Options**. You should see "WindowDeck: groups of
   related windows" as the page title.
3. Look at frames 1-7 and the "Rules" list under them. Check three rules
   the agent chose by itself: the orange frame shows on a stack when any
   window inside wants you; a group with one window left becomes a normal
   card with a small stack mark; the Flat view and the compact popup show
   stacks too.
4. Tell the agent "change the page: ..." with what to change, or
   "plan the build".

**Done when:** you accepted the page or asked for changes.

## Done

- **1. Install 0.20.90 and test the drop gap and the "waiting for you" marks**: done 2026-11-10. The user confirmed step 3 ("1.3 is passed") and steps 5-6 ("Orange, cleared after").
- **2. Install the new attention hook: plain-text questions turn orange too**: done 2026-11-10. Installed in config commit dfbc258; the user saw the orange card, and the log shows "Stop (reply asks)" marking and UserPromptSubmit clearing 4 of 4 targets.
- **4. Install the attention hook fix: next-step box questions stay orange**: done 2026-11-11. Installed in config commit c4ea72d; the user saw the card stay orange ("Stays orange"), and the log shows the mark cleared by the user's message.
- **3. Screenshot WindowDeck when a card is orange with no question waiting**: done 2026-11-11. The screenshot showed card "WinCraft Rust" (this chat); the log showed the hook marked it at 00:17:43 because the reply ended with the "On you:" line. Fixed in task 5.
- **5. Install the attention hook fix: the task reminder line no longer marks a window**: done 2026-11-11. Installed in config commit 6104161, identical to the tested copy; the test of the reminder line passed.
