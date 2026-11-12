# Tasks on you: WinCraft

Updated: 2026-11-12. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 28
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 26. Install 0.20.110: the header switch and the top edge of ClaudeDeck

- **State:** Ready to test (0.20.110 released 2026-11-12; the release has wincraft.exe and its checksum).
- **Kind:** test · **Added:** 2026-11-12 (PC)
- **Why:** you found the **Only open windows** switch badly aligned, and the top edge moved the window instead of resizing it. Both can only be seen on your screen.

**Steps**

1. Press `Alt+Space` (your palette hotkey), type `update`, pick **Update WinCraft** and press `Enter`. The bottom right corner of the palette then shows "WinCraft v0.20.110" or newer.
2. Press `Win+Alt+X`. Look at the **Only open windows** switch in the header. The round knob sits in the middle of the switch, with the same gap above, below and at its end. Click the switch off and on: the knob looks centred both times.
3. Move the mouse to the very top edge of the ClaudeDeck window, until the pointer becomes a two-headed up-down arrow. Press and drag up: the window gets taller and does not move.
4. Press in the middle of the header (not on a button) and drag: the window moves as before.

**Done when:** steps 2 to 4 look as described.

### 27. Two quick checks for ClaudeDeck's open and archive features

- **State:** Partly ready: step 3 answered on 2026-11-12 (the link only brought the Claude app forward on a new empty chat); step 4 waits for your answer.
- **Kind:** test · **Added:** 2026-11-12 (PC)
- **Why:** a double-click on a chat with no window of its own does nothing, and you want an archive item. Both depend on what the Claude app accepts, which only your screen shows.

**Steps**

1. Look at the Claude app window, so you can see which chat it shows.
2. Paste this into a WSL terminal and press `Enter`:
   `cmd.exe /c start "" "claude://code/continue?session=local_b578d8a3-56d7-42b3-8543-d301982de0ae"`
3. Tell the chat what the Claude window did: switched to the chat "Fix the flaky settings log test in WinCraft", only came to the front, or nothing.
4. Tell the chat whether that same chat (its fix is merged; nothing is left in it) may be the test chat for archiving: the session sets its archive flag and you watch whether the Claude app hides it.

**Done when:** you answered steps 3 and 4.
