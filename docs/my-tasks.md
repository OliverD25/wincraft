# Tasks on you: WinCraft

Updated: 2026-11-12. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

## Open

### 23. Install 0.20.107: fork a chat in a terminal, and the waits on the server line

- **State:** Partly ready: steps 1 and 2 done (you said "done till step 3" on 2026-11-12); steps 3 and 4 are ready to test.
- **Kind:** test · **Added:** 2026-11-12 (PC)
- **Why:** you asked for both after testing 0.20.106. Opening a terminal and a forked chat can only be checked on your screen.

**Steps**

1. Press `Alt+Space` (your palette hotkey), type `update`, pick **Update WinCraft** and press `Enter`. The bottom right corner of the palette then shows "WinCraft v0.20.107" or newer.
2. Press `Win+Alt+X`. Right-click any PC chat row, for example **WinCraft Rust Master**, and pick **Fork in a terminal**. Windows Terminal opens in that chat's folder and Claude starts with the chat's history. Type `/status` or ask "what were we doing?" to see it knows the chat. Close the terminal window when done; the original chat is not changed.
3. Server chats: right-click a home1 or home2 chat whose line is NOT "Remote Control is ready" (those have no chat yet, so they have no **Fork in a terminal**). Pick **Fork in a terminal**. A terminal opens with ssh, and Claude starts there with the chat's history and Remote Control on. If every server chat says "Remote Control is ready", skip this step and say so.
4. In any Claude chat on this PC, ask: `run /c/Windows/System32/OpenSSH/ssh.exe rdehome2 sleep 60 in the foreground`. While it runs, the **home2** line shows "1 PC chat waits · 8" in blue instead of CPU and RAM. After it ends, CPU and RAM come back within 30 seconds.

**Done when:** steps 2 and 4 look as described, and step 3 does or was skipped for the reason above.

## Done

- **22. Pick the ClaudeDeck icon**: done 2026-11-12. The user picked "B. Amber list": the chat list on an amber tile. Source: design/claude-design/claudedeck-icon/claudedeck-icon-b.svg; the ClaudeDeck coding session builds the .ico from it.
- **21. Install 0.20.106 and check the homelab servers in ClaudeDeck**: done 2026-11-12. The user said "21 task. all done": three server lines, the health hover, terminals, and **Waits on home2** on the PC chat during a foreground ssh.
- **13. Install the attention hook fix: a question stays Waiting while other tools finish**: done 2026-11-12. The user said "13 is done" after the test. The hook log has 13 "ignored: the mark waits for" lines, so the hook keeps a question mark while other tools finish.
- **11. Check that a stack list row shows its window**: done 2026-11-12. The user said "11 done" after the test: resting the mouse on a row of a stack's list shows that window in front.
- **20. Install 0.20.105 and move the cursor with the mouse in the rename box**: done 2026-11-12. The user said "done 20" after the test on 0.20.105: a press on the name places the cursor and the palette stays where it is.
- **19. Install 0.20.104 and try the rename box and Rename in ClaudeDeck**: done 2026-11-12. The user said "done task #19" after the test on 0.20.104: the rename box starts with the current name, and **Rename the window…** and `F2` work in ClaudeDeck.
- **15. Check Rename, group renames, app updates and status chips in an open group**: done 2026-11-12. The user tested on 0.20.103: "all works as intended". Asked after it: the rename box should start with the current name, and ClaudeDeck should get Rename; both are in 0.20.104 (task 19).
- **16. Have the attention hook tell WinCraft when a chat needs approval**: done 2026-11-12. The user said "task 16 is done". Checked: the installed `claude-attention.py` adds `--kind approval` for a permission prompt, and its log shows "PermissionRequest (approval)" marks for this chat that WinCraft took (3 of 3 targets).
- **18. Install 0.20.103 and check the marked row and the Working status in ClaudeDeck**: done 2026-11-12. The user said "done 18" after the test on 0.20.103: the marked row follows the chat window in front, and a chat that gets a message in the middle of its turn keeps **Working**.
- **17. Install 0.20.101 and drag a chat in ClaudeDeck**: done 2026-11-12. The user said "done 17" after the retest on 0.20.101. The first test on 0.20.100 passed steps 4 to 7 and 9 to 13; the busy `Win+Alt+C` became the default `Win+Alt+X`. Found after: the close button lost the window's place, and the hover panel covered the right-click menu; both fixed in 0.20.102.
- **14. Review the design of the new ClaudeDeck plugin**: done 2026-11-12. The design session recorded the approval in commit c2afaca: status marks A "Shapes", forks A "Tree lines", the rest of the page "ClaudeDeck" approved as drawn (artifact version 24).
- **12. Try both ways an open group shows, and pick one**: done 2026-11-12. The user tested both on 0.20.97 ("all tests run successfully") and chose "keep both as a setting": **Open a group as** stays, with both options. The small fixes found while testing went to their own work (the group selection follows the mouse, a picture for the setting, Settings comes to the front, the hotkey row overlap).
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
