# ClaudeDeck

A small window that lists your Claude Code chats from the Claude desktop
app, so you see at a glance which chat asks you something, which one works,
and which one is idle.

- Open it with `Win+Alt+C`, from the tray menu (**ClaudeDeck**), or from the
  palette (**Open ClaudeDeck**). Press the hotkey again while it is in front
  to close it.
- The first time, chats are grouped by project folder. A chat in a git
  worktree belongs to its repository's group. After that the groups are
  yours (see "Your own order" below). A new chat goes to the end of the
  group that already holds chats of its project folder; a fork hangs under
  its chat; a chat of a project no group holds goes to **New** at the top.
- A forked chat, or a chat started by another chat, hangs under it with thin
  tree lines. Click the small arrow to fold a chat's forks; folded, it shows
  the most urgent status of the family. When the chat a fork came from was
  archived, a grey stand-in keeps the tree whole.
- Click a group's header to fold it. Chats idle for over a day go into one
  grey line at the end; click it to see them.
- Rows never move by themselves. Only your drag, `Alt+Up` or `Alt+Down`
  changes the order.
- Click a chat, or pick it with the arrow keys and press `Enter`, to bring
  its window to the front. A chat with no open window does nothing.
- The header shows how many chats need approval, ask you, work, or are done.
  **Only open windows** lists only the chats that have a window open. The pin
  keeps ClaudeDeck above other windows.
- Drag the header to move the window, and its edges to resize it. It opens
  again at the same place and size.

## Your own order

- Drag a group's header to move the group. Drag a chat to another place or
  another group; a blue line shows where it lands. A chat takes its forks
  along. A fork cannot be dragged out from under its chat: it stays in the
  tree. With the keyboard, pick a chat or a group with the arrow keys and
  press `Alt+Up` or `Alt+Down`.
- Double-click a group's header, or pick it and press `F2`, to rename it:
  `Enter` saves, `Esc` puts the old name back.
- Right-click a chat (or press the Menu key or `Shift+F10`) to open it, pin
  it to the top of its group, hide it, copy its name, fold its forks, move
  it to another group or start a new group with it.
- Right-click a group to rename it, fold it (or all groups), give it a
  colour, make a new group, move it up or down, or ungroup it.
  **Ungroup (chats go back to their folder)** sends each chat to the group
  that holds its project folder's chats, or makes that group again; a chat
  with no folder goes to **New**. No chat is ever lost.
- A hidden chat leaves the list, with its forks. The line
  **N hidden chats** at the end lists them; right-click one and pick
  **Show this chat** to bring it back.

All of this changes only ClaudeDeck's own list. Hiding a chat hides its row
in ClaudeDeck; it never archives, closes or renames the real chat, and
nothing in the Claude app changes.

The order is kept in `%LOCALAPPDATA%\WinCraft\plugins\claude_deck.list.json`
(next to `claude_deck.place.json`, the window's place). It is written
through a temporary file, so a crash cannot leave half a file. A file that
cannot be read is kept as `claude_deck.list.json.bad` and the list starts
again from project folders; a file from a newer WinCraft is used but never
written. A chat that is no longer listed keeps its place for 30 days, so it
comes back where it was; after that it is forgotten.

## Status marks

Each status has its own colour and its own shape, so it reads without colour
too: a triangle for approval, a speech bubble for a question, a tick for
done, a turning ring for working, a dashed ring for background jobs, a
hollow circle for idle (grey when fresh, lighter after an hour, blue after a
day, a snowflake after three days), a square for stopped, a cross for
failed, and a dashed "?" when WinCraft cannot tell. The setting
**Status mark** adds a short word after each shape, or hides the marks.

How WinCraft tells each status:

- **Needs approval** and **Asks you**: a Claude Code hook said the chat
  waits (`wincraft.exe --waiting`). The hook says `--kind approval` for a
  permission prompt; without it every wait is "Asks you". The hook's change
  is in `docs/claude-deck-hook-kind.md`.
- **Working**, **Background**, **Idle**: the same rules as WindowDeck.
- **Done, not seen**: the chat's turn ended while ClaudeDeck was open, and
  since then you have not had its window in front. A turn that ended while
  ClaudeDeck was closed shows as Idle, because WinCraft cannot know whether
  you saw it.
- **Stopped**: the chat's last record is your stop. **Failed**: its last
  record is a failed request. Both only for chats active in the last day.
- **Unknown**: the session file does not say enough.

## The line of text and the hover panel

With **A line of text (the question, or what it does)** on, each row has a
second line: the question when the chat waits, what it does while it works
("Running a command"), or the first sentence of its last reply. Rest the
mouse on a chat to see the full question or reply, the chat's project and
branch, and "No open window" when a click would do nothing.

The text is only what Claude showed in the chat, at most 600 characters.
Code blocks are left out, and words that look like keys or tokens show as
"[hidden]". A tool's input, a command, a file or a tool's result is never
shown; a tool shows only as plain words for its name.

## Privacy

ClaudeDeck only reads the Claude app's own session files on this PC
(`%APPDATA%\Claude\claude-code-sessions`) and the transcripts of the chats
it lists (those with a window, or active in the last day), in steps: only
what was added since the last read. Nothing leaves the PC, and nothing
runs while the window is closed.
