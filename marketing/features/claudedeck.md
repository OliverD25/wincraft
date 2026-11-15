# ClaudeDeck

Feature: claudedeck
Status: draft
Updated: 2026-11-15
Relies on: `Win+Alt+X`, `Open ClaudeDeck`, `News`, `Mini mode`, `Show in the Claude app`, `Rename the chat…`, `Fork in a terminal`, `Connect to Claude Code`, `Live in the tray (no taskbar button)`, `Show a count on the tray icon`, `Servers (ssh names)`

## The problem
You run five or ten Claude Code chats at once in the Claude desktop app.
One of them waits for your approval, and you only notice it ten minutes
later. A finished answer in another chat goes unread.

## What you gain
One small window shows every chat and what it is doing, so you see at a
glance which chat needs you, and you get there with one key.

## Benefits
- See at once which chat needs approval, asks you a question, works, or
  finished while you were away.
- Jump to the chat that waited longest with `Win+Alt+X` and `Enter`.
- Read every reply of the last 24 hours on the News tab, so no answer gets
  lost.
- See how full each chat's context is before it compacts.
- Keep your chats in your own groups and order; rows never jump around.
- Watch Claude chats on your own servers too, over ssh.

## Proof points
- Status marks: needs approval, asks you, done not seen, working, waits on
  a server, server job, background, idle, stopped, failed, unknown
  (source: src/plugins/claude_deck/README.md, "Status marks").
- `Win+Alt+X` brings it forward with the chat that waits longest selected;
  `Enter` opens it (source: docs/ui-map.md, ClaudeDeck "Keyboard").
- News tab: every reply finished in the last 24 hours, newest first
  (source: src/plugins/claude_deck/README.md, "News").
- Context ring: fills as the context grows; full is the point where Claude
  Code compacts (source: README, "The context ring").
- Rows move only by your drag or `Alt+Up` / `Alt+Down` (source: README,
  intro list).
- Mini mode: a 30 px column of status marks (source: docs/ui-map.md,
  ClaudeDeck "Mini mode").
- Tray icon with a red badge counting the chats that wait for you (source:
  docs/ui-map.md, "ClaudeDeck's own tray icon").
- Servers: lists Claude chats on the ssh names you enter; nothing on a
  server is changed (source: docs/ui-map.md, ClaudeDeck "Servers" tab).
- Free and open source, MIT, no telemetry (source: CONSTITUTION.md,
  principles 2 and 5).
- QUESTION: how many chats do you run at once on a normal day? A real
  number ("I run 8") makes the post stronger, but it must be yours.

## Do not claim
- "It only reads": **Rename the chat…** renames the chat in the Claude
  app, **Show in the Claude app** switches the app's main window to that
  chat, **Fork in a terminal** starts a new chat, and **Connect to Claude
  Code** writes a hook into your Claude Code `settings.json`.
- "It never touches the Claude app": the rename presses the app's own
  menu, with guards before every step (source: README, "Privacy").
- "It works with no setup": **Needs approval** and **Asks you** show only
  after **Connect to Claude Code** (source: README, "How WinCraft tells
  each status").
- "It works with chats in any terminal": it lists the chats of the Claude
  desktop app on this PC, and chats on the servers you name.
- "It sends you notifications" or "it makes a sound": it makes no sound;
  the list, the tray badge and the tooltip are its only signals.
- "Works on Windows 10": WinCraft targets Windows 11 and is not tested on
  Windows 10.
- Any speed number: none is measured for ClaudeDeck yet.

## Shots needed
- media/claudedeck-1.png: the full window with 6-10 chats in two or three
  groups, at least one "Needs approval" and one "Working" mark, the header
  counts visible.
- media/claudedeck-2.gif: a chat turns to "Needs approval", then
  `Win+Alt+X` and `Enter` bring that chat to the front.
