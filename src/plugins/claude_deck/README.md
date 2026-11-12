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
what was added since the last read. Nothing leaves the PC, and nothing runs while the window
is closed.
