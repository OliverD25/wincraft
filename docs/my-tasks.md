# Tasks on you: WinCraft

Updated: 2026-11-10. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

## Open

### 6. Review the design "Window Deck Claude Status" and approve it or ask for changes

- **State:** Ready to do. The page is published in the WinCraft design artifact (version 17).
- **Kind:** decision · **Added:** 2026-11-11 (PC, branch of worktree optimistic-benz-67acd6)
- **Why:** the Claude status view for WindowDeck is built only after you approve the design.

**Steps**

1. Open https://claude.ai/artifact/S5fEimN5sS6oqHgwJVRxPn in your browser.
2. Click the title "Wincraft Windows Utilities Design" at the top left. The page list opens.
3. Click "Window Deck Claude Status". You should see four frames: "At rest",
   "Counter Waiting pressed", "Board (B)" and "What the marks mean".
4. Tell the agent "approve", or say what to change. Also answer the two open
   points at the end of the page: is the Claude view on by default, and does
   Flat view show the chips too.

**Done when:** you approved the page, or the agent has your list of changes.

## Done

- **1. Install 0.20.90 and test the drop gap and the "waiting for you" marks**: done 2026-11-10. The user confirmed step 3 ("1.3 is passed") and steps 5-6 ("Orange, cleared after").
- **2. Install the new attention hook: plain-text questions turn orange too**: done 2026-11-10. Installed in config commit dfbc258; the user saw the orange card, and the log shows "Stop (reply asks)" marking and UserPromptSubmit clearing 4 of 4 targets.
- **4. Install the attention hook fix: next-step box questions stay orange**: done 2026-11-11. Installed in config commit c4ea72d; the user saw the card stay orange ("Stays orange"), and the log shows the mark cleared by the user's message.
- **3. Screenshot WindowDeck when a card is orange with no question waiting**: done 2026-11-11. The screenshot showed card "WinCraft Rust" (this chat); the log showed the hook marked it at 00:17:43 because the reply ended with the "On you:" line. Fixed in task 5.
- **5. Install the attention hook fix: the task reminder line no longer marks a window**: done 2026-11-11. Installed in config commit 6104161, identical to the tested copy; the test of the reminder line passed.
