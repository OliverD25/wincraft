# Tasks on you: WinCraft

Updated: 2026-11-13. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 41
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 40. Install 0.20.123: connect WinCraft's own Claude Code hook, remove the old Python hook

- **State:** Ready to test (0.20.123 is released).
- **Kind:** test · **Added:** 2026-11-14 (PC, branch claude/wincraft-design-artifact-move-4b267d)
- **Why:** you chose to ship the "asks you" hook inside WinCraft, so other
  users get it with one click, and to replace your Python hook
  `claude-attention.py` with it. The button was never clicked by an agent,
  and your real settings.json was not touched. Step 3 was tested on a copy
  of your settings.json: it removed 6 entries and kept your other hooks.
- **Known, do not report:** a chat counts as asking only for a question
  card, a permission prompt, the app's next-step question, or a reply whose
  last paragraph ends with "?". A question in the middle of a long reply is
  not marked. The ClaudeDeck header no longer has the **Only open windows**
  switch; it is in the gear menu.
- **Agent can check:** `grep -c -- "--claude-hook" /c/Users/Admin/.claude/settings.json`
  prints 6, and `grep -c claude-attention` on the same file prints 0.

**Steps**

1. Press `Alt+Space`, type `update`, pick **Update WinCraft**, press
   `Enter`. WinCraft restarts as 0.20.123.
2. Open ClaudeDeck's settings: right-click the amber ClaudeDeck tile in the
   tray, pick **All settings…**. Under the heading **Claude Code**, the row
   **Claude Code hook** should say "Another hook already marks chats
   (claude-attention.py)". Click **Connect to Claude Code**. A balloon
   says "WinCraft connected to Claude Code. New chats show when they ask
   you."
3. Remove the old Python hook. In a WSL terminal, run this (it keeps a copy
   named `settings.json.before-drop-attention`):

   ```bash
   python3 - /mnt/c/Users/Admin/.claude/settings.json <<'EOF'
   import json, sys, shutil
   p = sys.argv[1]
   shutil.copy(p, p + ".before-drop-attention")
   d = json.load(open(p, encoding="utf-8"))
   n = 0
   for ev, groups in list(d.get("hooks", {}).items()):
       for g in groups:
           before = len(g.get("hooks", []))
           g["hooks"] = [h for h in g.get("hooks", []) if "claude-attention" not in h.get("command", "")]
           n += before - len(g["hooks"])
       d["hooks"][ev] = [g for g in groups if g.get("hooks")]
       if not d["hooks"][ev]:
           del d["hooks"][ev]
   json.dump(d, open(p, "w", encoding="utf-8"), indent=2, ensure_ascii=False)
   open(p, "a", encoding="utf-8").write("\n")
   print(f"removed {n} entries")
   EOF
   ```

   It should print `removed 6 entries`.
4. Back on ClaudeDeck's settings page, the row should now say "Connected to
   Claude Code".
5. In any chat, ask Claude something that makes it ask you back with a
   question card. Within a second, ClaudeDeck shows that chat as **Asks**,
   and the amber tray tile shows a red 1. Answer it: the mark goes.
6. Let a chat finish a turn with a reply that only reports work (no
   question). It should show **Done** or **Idle**, never **Asks**.
7. Tell this chat "done 40", or what looks wrong (a screenshot helps).

**Done when:** steps 2 to 6 work as written.

