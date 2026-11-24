# Tasks on you: WinCraft

Updated: 2026-11-24. Kept by the my-tasks skill. `/my-tasks` shows this
list; say "done 1" to close task 1. Secret values never go here, only names.

Next number: 64
Done tasks: [my-tasks-done.md](my-tasks-done.md)

## Open

### 63. Try typing a folder path in the palette

- **State:** Wait: 0.20.143 is being built on GitHub.
- **Kind:** test · **Added:** 2026-11-24 (PC)
- **Why:** you pasted a folder path into the palette and it showed only
  Ask AI and Search the web. Now a full path lists that folder, and a
  path that is not there offers the nearest folder that exists.
- **Agent can check:** nothing; only you can see the palette.

**Steps**

1. Update WinCraft: press `Alt+Space`, type "update", pick
   **Update WinCraft**, press `Enter`. The palette's bottom line should
   say v0.20.143.
2. Press `Alt+Space` and paste a path to a real folder, for example
   `E:\codespace\_claude_code`. The first row is that folder.
   Press `Enter`. File Explorer opens it.
3. Press `Alt+Space` and paste a path that does not exist, for example
   `E:\codespace\_claude_code\_rde_content_websites\_NewsroomCMS\_io`.
   One row shows the nearest folder that exists, with the line
   "“…” was not found. This is the nearest folder that exists."
   Press `Enter`. File Explorer opens that folder.
4. Tell this chat "done 63", or what looked wrong.

**Done when:** both paths open a folder in File Explorer.

