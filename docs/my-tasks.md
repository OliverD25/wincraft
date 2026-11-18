# Tasks on you: WinCraft

Updated: 2026-11-18. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 57
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 55. Test Quick access numbers from ClaudeDeck and Ctrl+W

- **State:** Wait: deploy 0.20.135. The release is building.
- **Kind:** test · **Added:** 2026-11-18 (PC)
- **Why:** Notion card "Quickswitch Mechanics" waits for it; only unit tests cover the menu, the number box and Ctrl+W.

**Steps**

1. Update WinCraft: press `Alt+Space`, type "update", pick **Update WinCraft**, press `Enter`. Wait until WinCraft restarts.
2. Press `Win+Alt+X` to open ClaudeDeck.
3. Right-click a Claude chat → **Give a Quick access number** → **First free number**. You should see a small outlined number box before the chat's name.
4. Rest the mouse on the box. The tooltip should say "Win+` then N jumps here".
5. Click another window (any app), then press `` Win+` `` and the chat's digit. The chat's window should come to the front (or, if it has no window, the Claude app shows it).
6. In ClaudeDeck, right-click the same chat → **Quick access number N** → **Remove the Quick access number**. The box should go away.
7. With ClaudeDeck in front, press `Ctrl+W`. ClaudeDeck should hide to the tray (its tray icon stays).
8. Tell any session of this project "done 55", or describe what looked wrong.

**Done when:** steps 3-7 work as described.

### 56. Look at the ClaudeDeck Stats tab and the token bars

- **State:** Wait: deploy 0.20.135. The release is building.
- **Kind:** test · **Added:** 2026-11-18 (PC)
- **Why:** Notion card "Stats Tab in ClaudeDeck" waits for it; nobody has seen the tab or the bars on screen yet.

**Steps**

1. After the update of task 55, press `Win+Alt+X` to open ClaudeDeck.
2. Press `Ctrl+Tab` until the **Stats** tab is picked (it is the last tab). For a few seconds it may say "Counting… N of M files".
3. You should see **Top chats**, **Groups**, **Days**, **Models** and **Busiest hours**. Press `←` for **Today** and `→` for **7 days**.
4. Rest the mouse on a top chat bar. The tooltip should show **Subagents**, **Cache read** and **Context now**.
5. Go back to the **Chats** tab. Each PC chat row should have a thin accent bar under it; the heaviest chat has the longest one.
6. Tell any session of this project "done 56", or say what is wrong or what you want changed.

**Done when:** the tab and the bars show and the numbers look believable to you.
