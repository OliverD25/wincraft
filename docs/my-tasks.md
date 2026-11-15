# Tasks on you: WinCraft

Updated: 2026-11-14. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 53
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 52. Test dragging WindowDeck cards when an app has 2 windows

- **State:** Ready to test. Release 0.20.133 checked on GitHub 2026-11-15 (exe and checksums are there).
- **Kind:** test · **Added:** 2026-11-15 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** with 2 windows WindowDeck opens its small (compact) popup, whose cards took
  only clicks. 0.20.133 lets you drag them; only unit tests cover it so far.
- **Agent can check:** the log has the line "order changed by a drag: 2 windows".

**Steps**

1. Update WinCraft: press `Alt+Space`, type "update", pick **Update WinCraft**, press
   `Enter`. Settings → About should then show 0.20.133.
2. Rest the mouse on the taskbar button of an app with exactly 2 windows (Chrome, as
   in your screenshot). WindowDeck opens with two cards.
3. Drag card 2 onto card 1's place and let go. The cards should swap, and the
   numbers 1 and 2 should follow the new order.
4. Drag it back. A plain click on a card should still switch to that window.
5. Tell this chat "done 52", or what went wrong.

**Done when:** two cards swap by a drag, and a click still switches to the window.
