# ClaudeDeck

A small window that lists your Claude Code chats from the Claude desktop
app, so you see at a glance which chat asks you something, which one works,
and which one is idle.

- Open it with `Win+Alt+C`, from the tray menu (**ClaudeDeck**), or from the
  palette (**Open ClaudeDeck**). Press the hotkey again while it is in front
  to close it.
- Chats are grouped by project folder. A chat in a git worktree belongs to
  its repository's group. A chat with no known folder goes to **New** at the
  top.
- A forked chat, or a chat started by another chat, hangs under it with thin
  tree lines. Click the small arrow to fold a chat's forks; folded, it shows
  the most urgent status of the family. When the chat a fork came from was
  archived, a grey stand-in keeps the tree whole.
- Click a group's header to fold it. Chats idle for over a day go into one
  grey line at the end; click it to see them.
- Click a chat, or pick it with the arrow keys and press `Enter`, to bring
  its window to the front. A chat with no open window does nothing.
- The header shows how many chats need approval, ask you, work, or are done.
  **Only open windows** lists only the chats that have a window open. The pin
  keeps ClaudeDeck above other windows.
- Drag the header to move the window, and its edges to resize it. It opens
  again at the same place and size.

## Status marks

Each status has its own colour and its own shape, so it reads without colour
too: a triangle for approval, a speech bubble for a question, a tick for
done, a turning ring for working, a dashed ring for background jobs, a
hollow circle for idle (grey when fresh, lighter after an hour, blue after a
day, a snowflake after three days), a square for stopped, a cross for
failed, and a dashed "?" when WinCraft cannot tell. The setting
**Status mark** adds a short word after each shape, or hides the marks.

Today WinCraft tells apart: asks you (a Claude Code hook said so), working,
background jobs, idle, and unknown. The other statuses come in a later
version.

## Privacy

ClaudeDeck only reads the Claude app's own session files on this PC
(`%APPDATA%\Claude\claude-code-sessions`) and the chat transcripts for
background jobs. Nothing leaves the PC, and nothing runs while the window
is closed.
