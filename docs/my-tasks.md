# Tasks on you: WinCraft

Updated: 2026-11-10. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

## Open

### 7. Install 0.20.91 and test WindowDeck groups

- **State:** Ready to test. v0.20.91 released 2026-11-11 (build passed,
  wincraft.exe and SHA256SUMS.txt published).
- **Kind:** test · **Added:** 2026-11-11 (PC, branch claude/gracious-panini-4299ca)
- **Why:** these parts need a real mouse and the Ctrl key, which the coder's
  test copy could not use: picking cards, Ctrl+G, dropping a card onto a
  card, the close question, the right-click menus.
- **Do not test on real work you need open:** step 9 closes windows. Use
  three spare windows of one app (for example three Notepad windows).

**Steps**

1. Right-click the WinCraft icon in the tray and click **Settings…**.
2. Click **About** in the left list, then **Check for updates**, then
   **Update now**. WinCraft restarts; About shows 0.20.91.
3. Open three Notepad windows. Open WindowDeck (`Win+Alt+A`, then the
   number of Notepad's taskbar button).
4. Ctrl + click two Notepad cards (each gets a white check mark), then press
   `Ctrl+G`. A box **Group 2 windows** opens; type `Test` and click
   **Make the group**. The two cards become one stack named "Test" with a
   count chip "2" and the other window's name under it.
5. Select the stack with the arrow keys. A list opens under it with both
   windows and their numbers.
6. Drag the third Notepad card onto the middle of the stack. The stack
   shows **Drop to group**; drop it. The count chip shows "3".
7. Press the stack's number. The group opens in a frame with numbers 1, 2, 3;
   the other cards dim. Press `2`: the second window comes to the front.
8. Open WindowDeck again, right-click the stack. The menu shows
   **Open the group**, **Rename the group**, **Ungroup**,
   **Close 3 windows…**. Click **Ungroup**: three normal cards again.
9. Group the three again (step 4 with all three), select the stack and
   press `Ctrl+W`. A question **Close 3 windows in Test?** opens. Click
   **Cancel**: nothing closes. Press `Ctrl+W` again and click
   **Close 3 windows**: the three Notepads close.
10. Open Settings → WindowDeck and switch **Group related windows** off.
    WindowDeck shows every window as a normal card. Switch it on again.

**Done when:** steps 4-10 behave as written, or you report what did not.

### 9. Install 0.20.92 and test Custom WindowDeck on your Claude windows

- **State:** Ready to test. Release v0.20.92 is built and published (2026-11-11, with wincraft.exe and SHA256SUMS.txt).
- **Kind:** test · **Added:** 2026-11-11 (PC, away run)
- **Why:** the status chips, ages, counters and Board were checked only on a test instance with WindowNamer off. Your real WinCraft has WindowNamer names and your real chats.
- **Agent can check:** the WinCraft log (`%LOCALAPPDATA%\WinCraft\wincraft.log`).

**Steps**

1. Right-click the WinCraft icon in the tray and click **Settings…**.
2. Click **About** in the left list, then **Update now** (after the check finds 0.20.92). WinCraft restarts; About shows 0.20.92.
3. Open WindowDeck on the Claude app (hover its taskbar button, or Tray menu → **WindowDeck: Open**). Each Claude card should show a chip at the top left of its picture: **Waiting** (orange), **Working** (blue, turning ring) or **Idle**, and its age at the end of the title line.
4. Click the counter **Waiting** in the toolbar, or press `S`. Only the waiting cards stay bright; nothing moves. Press `Esc`: the highlight clears.
5. Press `B`. The **Claude board** opens with the columns **Waiting for you**, **Working**, **Idle** and **Cold**. `Esc` goes back to the deck as it was.
6. Tell the agent which cards show no chip or a wrong status. The main window titled "Claude" never shows one.

**Done when:** the chips match what your chats are doing, and S, B and Esc work as described.

## Done

- **1. Install 0.20.90 and test the drop gap and the "waiting for you" marks**: done 2026-11-10. The user confirmed step 3 ("1.3 is passed") and steps 5-6 ("Orange, cleared after").
- **2. Install the new attention hook: plain-text questions turn orange too**: done 2026-11-10. Installed in config commit dfbc258; the user saw the orange card, and the log shows "Stop (reply asks)" marking and UserPromptSubmit clearing 4 of 4 targets.
- **4. Install the attention hook fix: next-step box questions stay orange**: done 2026-11-11. Installed in config commit c4ea72d; the user saw the card stay orange ("Stays orange"), and the log shows the mark cleared by the user's message.
- **3. Screenshot WindowDeck when a card is orange with no question waiting**: done 2026-11-11. The screenshot showed card "WinCraft Rust" (this chat); the log showed the hook marked it at 00:17:43 because the reply ended with the "On you:" line. Fixed in task 5.
- **5. Install the attention hook fix: the task reminder line no longer marks a window**: done 2026-11-11. Installed in config commit 6104161, identical to the tested copy; the test of the reminder line passed.
- **6. Review the WindowDeck groups design**: done 2026-11-11. The user accepted the page ("ok build it") and added a settings toggle to switch groups off.
- **8. Review the design "Window Deck Claude Status"**: done 2026-11-11. The user approved all four styles: Classic, Quest board, Chat pets, Claude crabs (artifact version 19). (Numbered 6 on its own branch; renumbered 8 in the merge with the groups work, which also used 6.)
