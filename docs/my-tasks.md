# Tasks on you: WinCraft

Updated: 2026-11-12. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 26
Done tasks: [my-tasks-done.md](my-tasks-done.md)

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

### 24. Install 0.20.108: a fork in a terminal shows in ClaudeDeck

- **State:** Ready to test (0.20.108 released 2026-11-12; the release has wincraft.exe and its checksum).
- **Kind:** test · **Added:** 2026-11-12 (PC)
- **Why:** you asked to see the terminal fork in ClaudeDeck. Finding the terminal window by its title can only be checked on your screen.

**Steps**

1. Press `Alt+Space` (your palette hotkey), type `update`, pick **Update WinCraft** and press `Enter`. The bottom right corner of the palette then shows "WinCraft v0.20.108" or newer.
2. Press `Win+Alt+X`. Right-click a PC chat, for example **WinCraft Rust Master**, and pick **Fork in a terminal**. A new Windows Terminal window opens. Its title is "Fork of WinCraft Rust Master · " and 8 letters and digits.
3. Look at ClaudeDeck. Within a few seconds a new row shows under **WinCraft Rust Master**, with a small terminal icon before its name.
4. Ask the fork something in the terminal. While it answers, its row shows **Working**; after it ends, **Idle**.
5. Rest the mouse on the row: the second line of the panel reads "Fork in a terminal · " and the folder. Click the row: the terminal window comes to the front.
6. Close the terminal window. Within a few seconds the row is gone from ClaudeDeck.

**Done when:** steps 2 to 6 look as described.

### 25. Install 0.20.109: ClaudeDeck's taskbar button, quick settings, and the drag between monitors

- **State:** Ready to test (0.20.109 released 2026-11-12; the release has wincraft.exe and its checksum).
- **Kind:** test · **Added:** 2026-11-12 (PC)
- **Why:** you asked for all of these on 2026-11-12. A test copy showed them with a real mouse, but your eyes on the three monitors are the last proof. One design change needs your yes (step 7).

**Steps**

1. Press `Alt+Space` (your palette hotkey), type `update`, pick **Update WinCraft** and press `Enter`. The bottom right corner of the palette then shows "WinCraft v0.20.109" or newer.
2. Press `Win+Alt+X`. The taskbar shows a button of its own for ClaudeDeck: an amber tile with three list rows, apart from the WinCraft "W" buttons. Click it: ClaudeDeck comes to the front.
3. Click the gear in ClaudeDeck's header, left of the pin. A menu opens. It lists the on/off settings with a check mark on each one that is on, and ends with **All settings…**.
4. Click **Fade when the mouse is away** so its check mark goes away. Press `Esc`. Move the mouse to another window and wait 10 seconds: ClaudeDeck stays solid.
5. In ClaudeDeck press `Ctrl+,`. The same menu opens with the first row marked. `↓` and `↑` move, `Enter` flips a row, `Esc` closes. Open it again and pick **All settings…**: Settings opens on the ClaudeDeck page.
6. Drag ClaudeDeck by its header to a monitor with another scale, then back. It follows the mouse without jumps. On the other monitor it looks the same size to the eye, and back on the first monitor it has its old size. Close it with the X on the other monitor and press `Win+Alt+X`: it opens there, in the same size.
7. Make ClaudeDeck as narrow as it goes. The header now hides the words **Only open windows** to make room for the gear; the switch itself stays and its tooltip names it. Tell me "keep" or "show the words".

**Done when:** steps 2 to 6 look as described, and you answered step 7.
