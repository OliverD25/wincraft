# The attention hook: tell WinCraft when a chat waits for approval

This note is for the session that edits the global Claude config
(`C:\Users\Admin\.claude`). The WinCraft repo cannot change that folder.

## Why

ClaudeDeck shows two different statuses for a chat that waits:

- **Needs approval**: a permission prompt blocks a tool (a yellow triangle).
- **Asks you**: Claude asked a question in the chat (a speech bubble).

Today the hook sends the same `wincraft.exe --waiting "<target>"` for both,
so every wait shows as **Asks you**. Since WinCraft 0.20.x (ClaudeDeck stage
2), `wincraft.exe` takes an optional argument after the target:

```
wincraft.exe --waiting "<target>" --kind approval
wincraft.exe --waiting "<target>" --kind question
```

No `--kind` means a question, exactly as before. WindowDeck shows both kinds
the same way, so nothing changes there.

## The change

File: `C:\Users\Admin\.claude\hooks\claude-attention.py`.

1. A wait is an approval when the hook event is one of these:
   - `PermissionRequest`;
   - `Notification` with `notification_type` equal to `permission_prompt`.

   Every other wait stays a question: `PreToolUse` for `AskUserQuestion`,
   a `Stop` whose reply asks, and the other notifications.
2. In `main()`, where it calls `wincraft("--waiting", targets)`, pass the
   kind for those two events. For example, give `wincraft()` an `extra`
   list and build the command as `[WINCRAFT, flag, target] + extra`, with
   `extra = ["--kind", "approval"]` for an approval and `[]` otherwise.
3. Keep `--answered` exactly as it is. It clears both kinds.
4. Optional: write the kind into the state file next to `"by"`, so the log
   line says which kind was sent.

## Safe with older versions

- An older `wincraft.exe` reads the target and skips `--kind approval`, so
  it still marks the chat as waiting (as a question).
- A wrong word after `--kind` makes `wincraft.exe` exit with code 2 and an
  error line, and marks nothing. Only `approval` and `question` are taken.

## How to check it worked

1. Open ClaudeDeck (`Win+Alt+C`).
2. In a Claude Code chat with the "Ask" permission mode, ask Claude to run a
   command that needs approval.
3. The chat's row shows the yellow triangle and, with **Shapes and words**,
   the word **Approve**. Before the change it showed the speech bubble.
