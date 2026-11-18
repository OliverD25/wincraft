# Tasks on you: WinCraft

Updated: 2026-11-18. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 59
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

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
