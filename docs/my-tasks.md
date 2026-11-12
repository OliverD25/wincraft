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

### 13. Install the attention hook fix: a question stays Waiting while other tools finish

- **State:** Ready to test. The fixed hook is tested and waits in the scratchpad.
- **Kind:** test · **Added:** 2026-11-12 (PC)
- **Why:** on 2026-11-12 the VerdictAI chat showed a question, but its WindowDeck card said Idle. The hook cleared the mark when any tool finished; now only the tool that waits clears it. This chat cannot write to the hooks folder.
- **Agent can check:** the installed hook matches the scratchpad copy, and `~/.claude/claude-attention.log` shows "ignored: the mark waits for" lines.

**Steps**

1. Open the WSL terminal and paste this block:

   ```bash
   cd /mnt/c/Users/Admin/.claude && cp "/mnt/c/Users/Admin/AppData/Local/Temp/claude/9e02bdafa49d6b9c/db2a44a3-eed1-4ce2-8060-89ab0229d84b/scratchpad/claude-attention.py" hooks/claude-attention.py && git add hooks/claude-attention.py && git commit -m "claude-attention: only the waiting tool's PostToolUse clears a mark" && git log --oneline -1
   ```

   It prints one commit line.
2. Next time a Claude chat shows a question card while it also has other work going (an away run, background agents), open WindowDeck. Its card shows **Waiting** with an orange frame until you answer.

**Done when:** step 1 printed a commit, and a waiting question card stays **Waiting** in WindowDeck.

## Done

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
