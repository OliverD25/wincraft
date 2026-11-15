# Tasks on you: WinCraft

Updated: 2026-11-14. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 47
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 46. Install 0.20.130: taskbar click opens WindowDeck, ClaudeDeck opens chats by link

- **State:** Ready to test
- **Kind:** test · **Added:** 2026-11-15 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** a click on the Chrome taskbar button sometimes showed Windows' own
  list; you chose that a click opens WindowDeck. ClaudeDeck's Open the chat
  pressed wrong buttons when the Claude app was on its Chat tab; you chose the
  app's own link and no button presses. Rename keeps pressing buttons but
  checks each step. All of it is checked only by unit tests.
- **Known, do not report:** the orange frames from before 0.20.129 stay until
  you press `Ctrl+M` in WindowDeck. A rename that stops with the notice
  "ClaudeDeck could not rename ..." is the safe way out, but tell me when it
  happens: it may mean one of the checks is too strict.
- **Agent can check:** the log lines "a click on ... opens WindowDeck" after step 2.

**Steps**

1. Press `Alt+Space`, type `update`, pick **Update WinCraft**, press
   `Enter`. WinCraft restarts as 0.20.130.
2. Click (do not hover) the Chrome button on the taskbar 5 times, with a
   pause between clicks. Each time WindowDeck should open, never Windows'
   own list of windows.
3. In the main Claude window, switch to the **Chat** tab. Then in
   ClaudeDeck (`Win+Alt+X`) double-click a chat that has no window of its
   own. The Claude window should switch to the Code view and show that chat,
   with no menus opening.
4. In ClaudeDeck, right-click a chat that has no window of its own and pick
   **Rename the chat…**. Type a new name and press `Enter`. The name should
   change in ClaudeDeck and in the Claude app's list. Then rename it back.
5. Tell this chat "done 46", or what went wrong (a screenshot helps).

**Done when:** steps 2 to 4 work as written.

