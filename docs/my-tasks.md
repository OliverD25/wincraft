# Tasks on you: WinCraft

Updated: 2026-11-11. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

## Open

### 11. Check that a stack list row shows its window

- **State:** Ready to test (0.20.96 released 2026-11-11).
- **Kind:** test · **Added:** 2026-11-11 (PC)
- **Why:** the window comes forward only on a real screen with a real mouse; tests cannot see it.

**Steps**

1. Right-click the WinCraft icon in the tray and click **Settings…**.
2. Click **About** in the left list, then **Check for updates**, then **Update now**. WinCraft restarts; About shows 0.20.96.
3. Open WindowDeck on an app that has a group (for example the Notepad group from task 7).
4. Move the mouse onto the stack. Its list of windows opens.
5. Move the mouse down into the list and rest it on a row. After a short moment that window shows in front, as when you rest the mouse on a card.
6. Move to another row: that row's window shows instead. Move the mouse out of the list: the window goes back.

**Done when:** steps 5 and 6 look as described.

### 12. Try both ways an open group shows, and pick one

- **State:** Ready to test. Release v0.20.97 is built and published (2026-11-11, with wincraft.exe and SHA256SUMS.txt).
- **Kind:** test + decision · **Added:** 2026-11-11 (PC)
- **Why:** you asked that opening a group moves nothing, and you wanted to try options B and C of the design page "Window Deck Group Opens In Place". Both are built; the setting **Open a group as** switches between them. Only a real screen shows which one feels right.

**Steps**

1. Right-click the WinCraft icon in the tray and click **Settings…**.
2. Click **About** in the left list, then **Check for updates**, then **Update now**. WinCraft restarts; About shows 0.20.97.
3. Open WindowDeck on an app that has a group (for example your Claude windows with the NewsHQ group).
4. Click the stack. Its windows show on a layer over the row, as a row centred on the stack (option B). No other card moves, nothing wraps to a second line, and the WindowDeck window keeps its size.
5. Press a number from 1 to 3, or click a window on the layer: that window comes to the front. Open the group again, then press `Esc` or click outside the layer: the group closes and every card is where it was.
6. Open Settings… → **Plugins** → click the **WindowDeck** tile. In the **Settings** section, set **Open a group as** to **A fan**.
7. Open WindowDeck and click the stack again. The windows now overlap like a fan on the stack's place (option C). `←` and `→` bring another window to the front; resting the mouse on a window's strip does too.
8. Tell the agent which one to keep: "keep the row", "keep the fan", or "keep both as a setting".

**Done when:** you said which option to keep, or what looked wrong.

## Done

- **10. Install 0.20.94 and check the Background chip on a Claude chat**: done 2026-11-12. The user checked steps 4-6: the chip showed "Background 24 s · 1 job" with a still ring, the chat sat in the Working column of the board, and it went back to Idle after the job ended.
- **7. Release and test WindowDeck groups with the fixes**: done 2026-11-11. The user tested steps 1-10 on 0.20.93 and 0.20.95; the hidden "Drop to group" mark was fixed in 0.20.95. Follow-up: list rows show their window (task 11).
- **9. Install 0.20.92 and test Custom WindowDeck on your Claude windows**: done 2026-11-11. The user said "task 9 done". Found while testing: a chat with a background task still running shows "Idle"; that is new work, not a failed test.
- **1. Install 0.20.90 and test the drop gap and the "waiting for you" marks**: done 2026-11-10. The user confirmed step 3 ("1.3 is passed") and steps 5-6 ("Orange, cleared after").
- **2. Install the new attention hook: plain-text questions turn orange too**: done 2026-11-10. Installed in config commit dfbc258; the user saw the orange card, and the log shows "Stop (reply asks)" marking and UserPromptSubmit clearing 4 of 4 targets.
- **4. Install the attention hook fix: next-step box questions stay orange**: done 2026-11-11. Installed in config commit c4ea72d; the user saw the card stay orange ("Stays orange"), and the log shows the mark cleared by the user's message.
- **3. Screenshot WindowDeck when a card is orange with no question waiting**: done 2026-11-11. The screenshot showed card "WinCraft Rust" (this chat); the log showed the hook marked it at 00:17:43 because the reply ended with the "On you:" line. Fixed in task 5.
- **5. Install the attention hook fix: the task reminder line no longer marks a window**: done 2026-11-11. Installed in config commit 6104161, identical to the tested copy; the test of the reminder line passed.
- **6. Review the WindowDeck groups design**: done 2026-11-11. The user accepted the page ("ok build it") and added a settings toggle to switch groups off.
- **8. Review the design "Window Deck Claude Status"**: done 2026-11-11. The user approved all four styles: Classic, Quest board, Chat pets, Claude crabs (artifact version 19). (Numbered 6 on its own branch; renumbered 8 in the merge with the groups work, which also used 6.)
