# Tasks on you: WinCraft

Updated: 2026-11-14. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 53
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 49. Take two ClaudeDeck pictures for the marketing page

- **State:** Ready to test. ClaudeDeck works now; nothing waits for a release.
- **Kind:** click · **Added:** 2026-11-15 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** the ClaudeDeck marketing page (marketing/features/claudedeck.md) and the
  Reddit post need a picture; posts without one get much less attention.
- **Agent can check:** the files `marketing/media/claudedeck-1.png` and
  `marketing/media/claudedeck-2.gif` exist.

**Steps**

1. Have 6-10 Claude Code chats open in the Claude app, with at least one waiting
   for your approval and one working.
2. Press `Win+Alt+X` to bring ClaudeDeck to the front.
3. Press `Win+Shift+S` (the Windows Snipping Tool), pick the window mode, and click
   ClaudeDeck's window. Save the picture as `claudedeck-1.png` in your `_io` folder
   (`E:\codespace\_claude_code\_rde\wincraft_windows_utilities_rust\_io`).
4. Optional: record a short GIF (for example with ScreenToGif) of a chat turning
   to "Needs approval", then `Win+Alt+X` and `Enter` bringing it to the front. Save
   it there as `claudedeck-2.gif`.
5. Tell this chat "pictures are in io". I move them into `marketing/media/` and
   check that no private chat names show.

**Done when:** both files are in `marketing/media/` and show no private names you
do not want public.

### 52. Test dragging WindowDeck cards when an app has 2 windows

- **State:** Wait: release 0.20.133 is being built.
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
