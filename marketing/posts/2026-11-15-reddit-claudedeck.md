# reddit: claudedeck

Date: 2026-11-15
Channel: reddit r/ClaudeAI
Built from: features/claudedeck.md (approved 2026-11-15), voice.md (draft)
Rules checked: no, ask the user (the agent's browser cannot open reddit.com).
Read r/ClaudeAI's rules and sidebar for self-promotion and the flair to pick.
Posted: no

---

Title: I made a small Windows window that shows which of my Claude Code chats needs me

I run many Claude Code chats at once in the Claude desktop app. Too often one
of them sat waiting for my approval, and I noticed it much later. So I built
ClaudeDeck, a small window that lists every chat and what it is doing.

What it gives me:

- A status mark on every chat: needs approval, asks you, working, or done
  while I was away.
- `Win+Alt+X` jumps to the chat that has waited longest. `Enter` opens it.
- A News tab with every reply from the last 24 hours, so no answer gets lost.
- A small ring per chat that shows how full its context is before it compacts.
- My own groups and order. Rows never jump around by themselves.

Honest limits: it is Windows 11 only, it lists the chats of the Claude
desktop app (and of my own servers over ssh), and the "needs approval" mark
needs a one-click Claude Code hook that it sets up for you.

ClaudeDeck is one plugin of WinCraft, a free, open source (MIT) tray app
with no telemetry: https://github.com/OliverD25/wincraft

What would you want a tool like this to show?
