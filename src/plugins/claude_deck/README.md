# ClaudeDeck

A small window that lists your Claude Code chats from the Claude desktop
app, so you see at a glance which chat asks you something, which one works,
and which one is idle.

- Open it with `Win+Alt+X`, from the tray menu (**ClaudeDeck**), or from the
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
- Click a chat to bring its window to the front. A chat with no open window
  does nothing.
- The header shows how many chats need approval, ask you, work, or are done.
  **Only open windows** lists only the chats that have a window open. The pin
  keeps ClaudeDeck above other windows. The arrow button switches to mini
  mode.
- Drag the header to move the window, and its edges to resize it. It opens
  again at the same place and size.

## Status marks

Each status has its own colour and its own shape, so it reads without colour
too:

- **Needs approval** (triangle): Claude waits for your OK to use a tool.
- **Asks you** (speech bubble): Claude asked you a question.
- **Done, not seen** (tick): the chat finished while you were not looking.
- **Working** (turning ring): Claude is busy on its turn.
- **Waits on a server** (two stacked boxes): the chat runs an ssh command
  to one of your servers and waits until it ends (see "Homelab servers").
- **Background** (dashed ring): the turn is over, background jobs still run.
- **Idle** (hollow circle): nothing happens. Grey when fresh, lighter after
  an hour, blue after a day, a snowflake after three days.
- **Stopped** (square): you stopped the chat.
- **Failed** (cross): its last request failed.
- **Unknown** (dashed "?"): WinCraft cannot tell.

The setting **Status mark** adds a short word after each shape, or hides
the marks.

How WinCraft tells each status:

- **Needs approval** and **Asks you**: a Claude Code hook said the chat
  waits (`wincraft.exe --waiting`). The hook says `--kind approval` for a
  permission prompt; without it every wait is "Asks you". The hook's change
  is in `docs/claude-deck-hook-kind.md`.
- **Working**, **Background**, **Idle**: the same rules as WindowDeck.
  One addition: a chat that was active in the last hour and whose
  transcript ends in a message or a tool shows **Working**, because the
  Claude app marks a chat idle when you send a message in the middle of a
  turn.
- The marked row follows you: when a chat's window comes to the front, its
  row becomes the marked one, so the list shows which chat you work in.
- **Done, not seen**: the chat's turn ended while ClaudeDeck was open, and
  since then you have not had its window in front. A turn that ended while
  ClaudeDeck was closed shows as Idle, because WinCraft cannot know whether
  you saw it.
- **Stopped**: the chat's last record is your stop. **Failed**: its last
  record is a failed request. Both only for chats active in the last day.
- **Unknown**: the session file does not say enough.

## The keyboard

- `Win+Alt+X` brings ClaudeDeck to the front with the keyboard in it. The
  chat that has waited longest (Needs approval or Asks you) is selected;
  with none waiting, the row you used last. When that chat is inside a
  folded group or under a folded chat, the folded line is selected; it
  does not unfold by itself.
- `Up` and `Down` move, `Home` and `End` go to the first and the last line.
- `1` to `9` pick the chat with that digit. While you use the keyboard, the
  first nine chat rows on screen show their digit in a small box.
- `Left` folds the picked group (or the picked chat's forks), `Right`
  unfolds it.
- `Enter` opens the picked chat, or folds the picked group.
- `Esc` gives the keyboard back to the window that was in front when you
  pressed `Win+Alt+X`. It closes nothing. While you rename a group, drag a
  line or have a menu open, `Esc` first ends that.
- `Ctrl+M` switches mini mode. The hotkey command **ClaudeDeck mini mode**
  does the same from anywhere; it has no keys until you set some in
  Settings → Hotkeys.
- The arrow keys go on from the PC chats to the server lines and server
  chats. `Enter` on a server line opens a terminal in the server's home
  folder; on a server chat, in its folder. `Ctrl+C` copies the name.
- The palette command **ClaudeDeck: Open a terminal on a server** lists
  your servers ("ClaudeDeck: Open a terminal on home2"); `Enter` opens a
  terminal there, in the home folder.
- `Alt+Up` and `Alt+Down` move the picked chat or group, `F2` renames the
  picked group, `Ctrl+C` copies the picked chat's name, `F2` on a picked chat opens the rename box for its window (WindowNamer keeps the name, and ClaudeDeck shows it), and the Menu key or
  `Shift+F10` opens its menu.

## Mini mode

Click the arrow button in the header (or press `Ctrl+M`) and the window
becomes a narrow column of status marks: one per chat, in list order, with
a thin line between groups. A folded group shows one mark, its most urgent.
Cold and hidden chats are left out.

- Click a mark to bring that chat's window to the front.
- Rest the mouse on a mark to see the hover panel beside the column.
- Drag the strip at the top to move the column.
- The button at the top (**Full list**) brings the full list back, at the
  place and size it had.

Mini mode is remembered when WinCraft starts again.

## Your own order

- Drag a group's header to move the group. Drag a chat to another place or
  another group; a blue line shows where it lands. A chat takes its forks
  along. A drag that starts on a fork moves the whole family: the fork stays
  under its chat. Cold chats and hidden chats cannot be dragged. With the keyboard, pick a chat or a group with the arrow keys and
  press `Alt+Up` or `Alt+Down`.
- Double-click a group's header, or pick it and press `F2`, to rename it:
  `Enter` saves, `Esc` puts the old name back. Scrolling the group out of
  sight saves the new name too.
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

## The line of text and the hover panel

With **A line of text (the question, or what it does)** on, each row has a
second line: the question when the chat waits, what it does while it works
("Running a command"), or the first sentence of its last reply. Rest the
mouse on a chat to see the full question or reply, the chat's project and
branch, and "No open window" when a click would do nothing.

The text is only what Claude showed in the chat, at most 600 characters.
Code blocks are left out, and words that look like keys or tokens show as
"[hidden]". A tool's input, a command, a file or a tool's result is never
shown; a tool shows only as plain words for its name. The one exception is
a foreground ssh command to one of your servers, for **Waits on a server**:
its first 120 characters, with the same hiding.

## Settings

- **Stay on top**: keep ClaudeDeck above other windows (the pin does the
  same).
- **Only open windows**: list only the chats that have a window open.
- **Fade when the mouse is away**: 3 seconds after the mouse leaves,
  ClaudeDeck turns half see-through. It is solid again when the mouse comes
  back, when it has the keyboard, or when a chat starts to wait.
- **Fold chats idle for over 1 day**: old idle chats go into one grey line
  at the end.
- **Show a count on the tray icon**: while ClaudeDeck is open, the tray
  icon's tooltip says how many chats wait for you, for example
  "WinCraft — 2 chats wait for you". The icon itself does not change.
- Under **On each row**: **Status mark**, **A line of text (the question,
  or what it does)**, **Time in the status**, **Project and branch**,
  **Job count**, **Model name**.

## What ClaudeDeck never does

- It never changes a chat: no archive, close, rename or message. A click
  only brings the chat's own window to the front.
- It never moves, closes or changes another window, and never moves the
  mouse pointer. `Esc` only gives the keyboard back to the window that had
  it before the hotkey.
- It makes no sound. The list, the tray tooltip and the fade are its only
  signals.
- Nothing runs while its window is closed.

## Homelab servers

List your servers' ssh names in **Servers (ssh names)** (under
**Homelab**), for example `rdehome1, rdehome2`. Empty, the default, turns
this off: nothing is read and no server group shows.

- Each server is a group under the PC chats: a server icon, its name
  without a leading `rde`, a green dot when it answers, and CPU, RAM and the
  number of chats. A server that does not answer shows "Offline · last seen
  3 h ago" and keeps its chats as last known, drawn see-through. Rest the
  mouse on the server's line for details, its health ("Up 12 days ·
  Restart needed", "Disk 120 GB free of 512 GB", "Busiest: dockerd 85%")
  and the PC chats that wait on it.
- Its chats are the Remote Control chats (`claude-rc@<folder>` units) and
  every other chat whose transcript changed in the last 24 hours. Click one
  to open Windows Terminal with ssh in its folder; right-click for **Open a
  terminal there**, pin, hide, **Copy the name** and **Copy the folder
  path**.
- A PC chat whose running command is a foreground `ssh` to one of your
  servers (by any name on its `Host` line in `~\.ssh\config`) shows
  **Waits on a server**: "Waits on home2 · docker compose up", and the
  command in the hover panel. A background ssh job stays **Background**.
- Server statuses come from the end of each transcript: **Asks you** for an
  open question, **Working** for an open tool call or a write in the last
  minute, **Idle** after that, **Stopped** when neither a Remote Control
  unit nor a `claude` process runs in its folder. **Needs approval** cannot
  be seen on a server (on the PC a hook says it, and nothing is installed
  on the servers), so such a chat shows **Working**. A chat whose Remote
  Control service failed or keeps restarting shows **Failed** (red) with
  "Remote Control stopped". A Remote Control service that has not started
  a chat yet (it writes nothing until you do) still shows as a row:
  **Idle**, "Remote Control is ready", in `~/projects/<name>`.
  A chat whose Remote Control service runs is never folded into the cold
  line: after a day without activity it shows "Remote Control is ready",
  with the time of its last activity.

When two chats work on the same repo at the same time (told by the last
part of the folder, a git worktree counting as its repo), on this PC or a
server in any mix, both rows start with "Also busy on home2" (or "Also busy
on this PC"), and the hover panel says "Two chats work on this repo at the
same time. Their changes can conflict." Only chats that work or wait on a
server count; idle ones do not. It shows on the line of text.

How it reads: while the ClaudeDeck window is open, and only then, one
`ssh <name> sh -s` call per server every 30 s (the first at once), with
Windows' own ssh, `BatchMode=yes` (never a password prompt), a 6 s connect
time, and a stop after 20 s. The script it sends only reads: the clock, CPU,
memory and uptime from `/proc`, whether a restart is needed, the free disk
of `/`, the busiest process, the running Remote Control units, the folders of
running `claude` processes, and the last 64 KB of each recent transcript
under `~/.claude/projects` (a transcript that did not change is not sent
again). Nothing is installed, written or changed on a server.

Limits: mini mode shows only the PC chats. A pinned or hidden server chat
is forgotten after 30 days, like a PC chat that is gone.

## Privacy

**Fork in a terminal.** The right-click menu of a chat (on this PC or on a
server) can start a fork of it in a new terminal:
`claude --resume <session id> --fork-session`. The fork is a new chat with
the whole history; the chat it came from is not changed. On this PC it uses
Windows Terminal in the chat's folder (a console window when Windows
Terminal is missing). On a server it uses ssh and adds `--remote-control`,
so the fork also shows in the Claude app. A Remote Control service with no
chat yet has nothing to fork.

ClaudeDeck only reads the Claude app's own session files on this PC
(`%APPDATA%\Claude\claude-code-sessions`) and the transcripts of the chats
it lists (those with a window, or active in the last day), in steps: only
what was added since the last read. Nothing leaves the PC, and nothing
runs while the window is closed. With servers listed, it also reads your
servers over ssh as described under "Homelab servers"; what it reads stays
on this PC.
