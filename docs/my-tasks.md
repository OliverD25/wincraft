# Tasks on you: WinCraft

Updated: 2026-11-12. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 28
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

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
