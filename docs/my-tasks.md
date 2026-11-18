# Tasks on you: WinCraft

Updated: 2026-11-18. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 59
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 57. Test the ClaudeDeck cursor after a jump, and the View settings

- **State:** Ready to test. 0.20.136 is released (checked 2026-11-18).
- **Kind:** test · **Added:** 2026-11-18 (PC)
- **Why:** the cursor fix and the View group are covered only by unit tests.

**Steps**

1. Update WinCraft: press `Alt+Space`, type "update", pick **Update WinCraft**, press `Enter`. Wait until WinCraft restarts.
2. Press `Win+Alt+X` to open ClaudeDeck, and leave it open beside your other windows.
3. Press `` Win+` `` and the digit of a pinned Claude chat. Within a moment, ClaudeDeck's marked row should move to that chat.
4. Jump to another pinned chat the same way. The marked row should move again.
5. In ClaudeDeck, click the gear in the header, then **View**. You should see a grey "Status mark" heading with **Shapes**, **Shapes and words**, **Off**, then the toggles ending in **Quick access numbers** and **Token bar**.
6. Click **Quick access numbers**. The number boxes on the rows should go away; click it again to bring them back.
7. Tell any session of this project "done 57", or say what looked wrong.

**Done when:** steps 3-6 work as described.

### 58. Look at the new token views in ClaudeDeck

- **State:** Ready to test. 0.20.137 is released (checked 2026-11-18).
- **Kind:** test · **Added:** 2026-11-18 (PC)
- **Why:** the design you picked (graph A1, Token mark) is only checked by unit tests; nobody has seen it on screen.

**Steps**

1. Update WinCraft: press `Alt+Space`, type "update", pick **Update WinCraft**, press `Enter`. Wait until WinCraft restarts.
2. Press `Win+Alt+X`, then `Ctrl+Tab` until the **Stats** tab shows. Each Top chats bar should be split into model colours, with "{tokens} tokens · {n}%" at the right and a model legend under the list.
3. Go to the **Chats** tab. Click the gear in the header → **View**. Under **Token mark**, click **Fill**, then **Signal bars**, then **Line**. The rows should change each time.
4. Click **Count today**. The marks should now show today's share; chats with no tokens today show none.
5. Close the menu. Press `Ctrl+B`: the marks go away; press it again: they come back in the last style. Press `Ctrl+Shift+B`: it switches between today and 7 days.
6. Tell any session of this project "done 58", or say what you want changed.

**Done when:** steps 2-5 work and the looks match what you picked.
