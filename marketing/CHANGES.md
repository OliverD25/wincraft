# Marketing changes

Format: - <date> | <feature key> | <what changed> | <commit> | open
When /marketing update handles a line, `open` becomes `done <date>`.

- 2026-11-16 | quickjump | pins of Claude chats reopen the chat by its link; pins follow title changes; R re-pins | 3f63e03 | open
- 2026-11-18 | claudedeck | right-click a chat: Give a Quick access number (QuickJump `` Win+` `` + digit opens it from anywhere); the number shows on the chat's row | f1a9bf3 | open
- 2026-11-18 | claudedeck | Ctrl+W hides ClaudeDeck to the tray (or minimises it with Live in the tray off), in mini mode too | 265705a | open
- 2026-11-18 | claudedeck | new Stats tab: tokens per chat, group, day, model and hour (Today or 7 days, weighed by model price, PC chats only); Token bar under each chat row | 3ce21b1 | open
- 2026-11-18 | claudedeck | the marked row follows QuickJump jumps at once, also to a chat opened in the Claude app's main window, and stays right while ClaudeDeck is hidden in the tray | dea634c | open
- 2026-11-18 | claudedeck | a View group in Settings and a View submenu in Quick settings: show or hide the status mark, the text line, time, project, jobs, model, Quick access numbers (new switch) and the token bar | 2977373 | open
- 2026-11-18 | claudedeck | Stats tab: Top chats bars cut by model, with each chat's tokens and share; the Token bar becomes Token mark (Line, Fill, Signal bars or Off) with Token mark counts (today or the last 7 days), Ctrl+B and Ctrl+Shift+B in the window, two palette commands | 006a0d2 | open
- 2026-11-18 | claudedeck | the token mark now has hotkeys that work from anywhere (Win+Alt+Shift+T turns it on or off; a second command switches today and 7 days, keys set in Settings); the Ctrl+B and Ctrl+Shift+B window keys are gone | 7d57be5 | open
- 2026-11-18 | quickjump | fixed numbers: a pin keeps its number until you move it, Alt+9 (and Put at number, ClaudeDeck's number menu) puts a window at 9 even with fewer pins, unpinning leaves the number empty and moves nothing else | f3a55d3 | open
- 2026-11-20 | claudedeck | a chat of a new project always shows: it goes to an unfolded New group, also after you renamed New to a group of your own | 37aeda9 | open
- 2026-11-22 | claudedeck | a chat in the cold line can be dragged into a group; it shows there for a day | 758ed98 | open
- 2026-11-23 | claudedeck | a note on a chat: right-click → Add a note… (or N) puts one line of your own text under the row; the × on hover or Remove the note takes it away; the hover panel shows all of it | be9abb9 | open
- 2026-11-24 | palette | a folder path typed or pasted with no prefix lists that folder, and Enter opens it in File Explorer; a path that is not there offers the nearest folder that exists | e2bfefd | open
- 2026-11-26 | claudedeck | groups stand apart: a bigger group name and a line above each group; the cold chats sit in their own darker area under a Cold chats header with their count and age ("3 chats · 1 – 3 days"), which folds with a click or the arrow keys, Space and Enter | c26f69b | open
- 2026-11-27 | claudedeck | pinning or unpinning the window no longer makes it blink | ea3e6ee | open
- 2026-11-28 | claudedeck | new setting Cold after (hours), 48 by default: a chat with no activity that long goes to Cold chats (Chats and Servers tabs), and the idle mark turns blue then; the Settings toggle is now Fold idle chats, the quick settings row says the time ("Fold chats idle for over 2 days") | 675a1de | open
