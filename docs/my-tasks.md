# Tasks on you: WinCraft

Updated: 2026-11-12. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 31
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 30. Install 0.20.114: a fast open from ClaudeDeck, and Background for a woken agent

- **State:** Ready to test (0.20.114 released 2026-11-12; the release has wincraft.exe and its checksum).
- **Kind:** test · **Added:** 2026-11-12 (PC)
- **Why:** you found the open slow and a chat with a working background agent shown as Idle. Measured live before the release: the search now takes 0.1-0.3 s instead of 3-4 s.

**Steps**

1. Press `Alt+Space` (your palette hotkey), type `update`, pick **Update WinCraft** and press `Enter`. The bottom right corner of the palette then shows "WinCraft v0.20.114" or newer.
2. Press `Win+Alt+X`, switch **Only open windows** off, and double-click a chat that has no window of its own. Its line says "Opening in the Claude app…" at once, and the chat shows in the Claude app within about a second.
3. Right-click a chat: the menu has no Archive item any more, and `Delete` does nothing.
4. When a chat gives a background agent a new job (as this WinCraft chat does with its coder), its row shows **Bg** while the agent works, not **Idle**.

**Done when:** steps 2 to 4 look as described.
