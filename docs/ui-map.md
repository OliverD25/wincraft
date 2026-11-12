# UI map: WinCraft

Updated: 2026-11-12. Exact labels are in **bold**, copied from the code.
Before telling the user where to click, find the label here. A label that
is missing here: grep the code, then add it here in the same commit.

## Tray menu
Opened by: left-click or right-click the WinCraft icon in the tray (the
hidden-icons arrow `^` in the taskbar, if the icon is not shown).
- **Open palette**: opens the command palette.
- **Settings…**: opens the settings window.
- **Update to {version}**: only after an update check found a newer version. This is the only place in the tray menu that updates WinCraft; the **Update now** button is on Settings → About.
- One primary item per enabled plugin (see "Tray items of the plugins" below).
- **Detailed log**: a check mark; switches the detailed log on or off.
- **Quit WinCraft**: closes the app.

### Tray items of the plugins
Shown only while the plugin is switched on, between two separators.
- ScreenDimmer: **Wake all monitors**.
- LayoutKeeper: **Restore layout**.
- WindowDeck: **WindowDeck: Open**, and a check mark item **WindowDeck on taskbar hover**.
- QuickJump: **Open quick access**.
- ShortcutManager: **Shortcut manager**.
- ClaudeDeck: **ClaudeDeck**.
- QuickActions: a check mark item **Keep awake**; while keep awake is on with an end time it reads **Keep awake until {time}**.
- LanguageIndicator and WindowNamer have no tray item.

## Command palette
Opened by: the palette hotkey (default `Win+Alt+P`; changed in Settings → General → **Palette hotkey**), or Tray menu → **Open palette**.
- Search box: type to search; a prefix key at the start filters (`<` windows, `/` folders, `.` files, `'` bookmarks, `=` calculator, `>` terminal, `!` WinCraft commands, `?` prefix list and **Ask AI**).
- **Recent and suggested**: the group shown under an empty box.
- **Best match**: the top group while typing.
- **Update ready**: a row pinned at the top under an empty box, only after an update check found a newer version (and it was not hidden).
  - **Update now**: `Enter` on that row installs it.
  - **Release page**: `Ctrl+O` opens the release page.
  - **Hide until next version**: `Ctrl+Del` hides the row until a newer release.
- **Update WinCraft**: command (type it): checks now and installs a newer version at once. It carries a badge "{version} available" when one is known.
- **Ask AI**: row for a question; opens the site chosen in Settings → General → **Ask AI site**.
- **Nothing matches**: shown when the search finds nothing.
- Footer: a gear icon opens Settings (`Ctrl+,`); `Ctrl+P` opens Plugins.
- Row actions in the inspector (the panel beside the list): **Switch to window**, **Move to next monitor**, **Close window**, **Forget this pick**, **Arrange marked in strip**, **Open release page**.

### Commands under WinCraft in the palette
- **Open settings**, **Plugin store**, **Saved window layout** (opens Settings → Layout), **About WinCraft** (opens Settings → About).
- **Open the config file**, **Open the log file**, **Update WinCraft**.
- **Detailed log**, **Detailed log: Mark this moment**, **Detailed log: Save a report**, **Detailed log: Delete the files**.
- **Palette: Move back to the centre**, **Mark all windows as seen**, **Quit WinCraft**.

### Commands of the plugins in the palette
- LanguageIndicator: **Show current input language**.
- LayoutKeeper: **Restore layout**, **Save layout now**, **Move window left in taskbar**, **Move window right in taskbar**.
- ScreenDimmer: **Test monitors again**, **Wake all monitors**.
- WindowDeck: **WindowDeck: Open**, **WindowDeck: Switch taskbar hover on or off**.
- WindowNamer: **Rename the active window**, **Clear all names**.
- QuickJump: **Jump to a pinned window**, **Pin this window…**.
- ShortcutManager: **Open shortcut manager**, **Show free combinations**.
- ClaudeDeck: **Open ClaudeDeck**.
- QuickActions (named Area: Verb):
  - Window: **Window: Toggle Always on Top**, **Window: Center**, **Window: Resize to 1280 × 720**, **Window: Resize to 1920 × 1080**, **Window: Resize to 80 % of the Monitor**, **Window: Resize…**.
  - Display: **Display: Toggle Keep Awake**, **Display: Keep Awake for…**.
  - Switches: **Theme: Toggle Dark Mode**, **Desktop: Show Desktop Icons**, **Taskbar: Toggle Auto-Hide**, **File Explorer: Toggle Hidden Files**, **File Explorer: Toggle File Extensions**, **Audio: Toggle Microphone Mute**.
  - One-step: **Network: Copy Local IP**, **Network: Flush DNS Cache**, **Clipboard: Convert to Plain Text**, **Clipboard: Copy New UUID**, **System: Empty Recycle Bin**, **Settings: Open Environment Variables**, **Developer: Kill Process on Port**, **Audio: Switch Output Device**, **System: Eject USB Drive**.

## Settings window
Opened by: Tray menu → **Settings…**, or the palette command **Open settings**, or `Ctrl+,` in the palette.
The left list has these pages, in this order: **General**, **Themes**, **Plugins**, **Hotkeys**, **Layout**, **Bookmarks**, **Plugin Store**, **About**.

## Settings → General
Opened by: Settings window → **General** in the left list.
- **Start with Windows**: toggle; starts WinCraft when you sign in.
- **Palette hotkey**: the key that opens the palette.
- **Config file**, **Log file**: rows with an **Open** button.
- **Palette search** (section):
  - **Terminal shell**: the shell for the `>` prefix; **Custom command** shows when **Custom** is picked.
  - **Web search address**: where Search the web goes.
  - **File search: Everything, then Windows index**: toggle.
  - **Use the Windows index when Everything is not running**: toggle.
  - **Remember terminal history**: toggle.
  - **Ask AI site**: pick the AI site; **Custom AI address** shows for a custom one.
  - **Learn from my picks**: toggle.
  - **Usage history**: row with a **Clear usage history** button.
- **Show tips**: toggle. **Tips seen**: row with a **Show all tips again** button.
- **Detailed log**: toggle, same as the tray item.

## Settings → Themes
Opened by: Settings window → **Themes** in the left list.
- **Theme**: pick a theme (point at one to try it).
- **Mode**: Light, Dark, or System (follows the Windows app mode).

## Settings → Plugins
Opened by: Settings window → **Plugins** in the left list.
- **Search installed plugins**: filter box.
- A tile per plugin with an on/off toggle. Click the tile to open the plugin's page.
- **No plugins installed**, **No plugin matches**: empty states.

### Settings → Plugins → a plugin's page
Opened by: Settings → **Plugins** → click the plugin's tile.
- **‹ All plugins**: link back to the list.
- **Enabled**: toggle next to the title.
- A page button for plugins that have one (for example **WindowDeck: Open**).
- **Settings** (section): the plugin's own fields, listed per plugin below.
- **Hotkeys** (section): the plugin's hotkeys; click a row to change it.
- **Files** (section): **Plugin settings** row with **Open file** and **Reset to defaults** buttons.
- **About this plugin**: the plugin's README text.

### Fields of each plugin's Settings section
- LayoutKeeper: **Programs**, **Never touch**, **Restore on start**, **Wait for windows (seconds)**, **Wait for apps to open (minutes)**, **Save every (seconds)**, **Restore which window is in front**, **Restore monitors**, **Show restore progress**, **Offer to put windows back after an app restarts**. Also **Open state file** (a file button).
- ScreenDimmer: **Darkness**, **Darkness under the pointer**.
- WindowDeck: **Preview window on hover**, **Show key hints**, **Show the toolbar on other monitors**, **Group related windows**, **Open a group as** (**A row centred on the stack** or **A fan**; under the row a picture of each option, with its name, and a click on a picture also picks it), **Show WindowDeck instead of Windows' taskbar preview**, **Hover delay (seconds)**, **Open WindowDeck with Win+1…9**. Under the heading **Custom WindowDeck**: **Claude card style** (**Classic** or **Off**) and **Animate the Claude cards**.
- QuickJump: **Open Quick access in the middle of the screen**, **Point to a window on another monitor**.
- WindowNamer: **Keep names after restart**, **Title shows**.
- ShortcutManager: **Show free combinations**.
- ClaudeDeck: **Stay on top**, **Only open windows**, **Fold chats idle for over 1 day**. Under the heading **On each row**: **Status mark** (**Shapes**, **Shapes and words** or **Off**), **A line of text (the question, or what it does)**, **Time in the status**, **Project and branch**, **Job count**, **Model name**.
- LanguageIndicator: **How long it stays**, **Where it appears**, **Show on focus change**, **Show the layout name**.
- QuickActions: no fields.

## Settings → Hotkeys
Opened by: Settings window → **Hotkeys** in the left list.
- **Search by name, plugin or keys**: filter box.
- **Search by keys**: button; then press keys to find what uses them (**Press keys to find what uses them**).
- Columns: **Command**, **Hotkey**.
- Click a row to record a new hotkey: **Change hotkey**; `Enter` saves, `Esc` cancels, `Backspace` clears.
  - **Remove hotkey**, **Reset to default**, **Cancel**.
  - **Move here**: shown when another WinCraft command holds the keys; moves them to this command.
  - **Taken by another app**, **Windows shortcut: {name}**: warnings.
- **Reset all…**: asks **Reset all hotkeys?** and then **Reset all**.
- **What is a chord?**: a short help text.

## Settings → Layout
Opened by: Settings window → **Layout** in the left list, or the palette command **Saved window layout**.
- **Restore now**: puts the saved layout back now (same as `Win+Alt+L`).
- **Save now**: saves the layout now (same as `Win+Alt+J`).
- **Last saved {time}**, **Not saved yet this session**: the state line.
- A list of saved windows per app; per window **Rename…** and **Forget**.
- **Nothing is saved yet**, **No saved layout to show**: empty states.
- **Recent activity** (section): the LayoutKeeper log, with **Refresh** and **Open log file**.

## Settings → Bookmarks
Opened by: Settings window → **Bookmarks** in the left list.
- **Add a bookmark** (section): **Phrase**, **Place**, **Add**.
- **Add bookmark…**: opens the form.
- Each bookmark row: **Open**, **Edit**, **Delete**; while editing **Save** and **Cancel**.
- **No bookmarks yet**: empty state.

## Settings → Plugin Store
Opened by: Settings window → **Plugin Store** in the left list, or the palette command **Plugin store**.
- **Search the store**: filter box.
- **Refresh**: loads the index again.
- A tile per plugin; **Installed** marks one you have; **Open on GitHub** opens its page.
- **The plugin index could not be loaded**: error state.

## Settings → About
Opened by: Settings window → **About** in the left list, or the palette command **About WinCraft**.
- **Version**: the installed version.
- **Updates** (section):
  - **Check for updates**: shown when no newer version is known.
  - **Update now**: shown instead, once a newer version is known.
  - **Cancel**: shown while a download runs.
  - **Release page**: link to the release.
  - **Check for updates automatically**: toggle. **Download updates ahead**: toggle.
  - Status lines: **Not checked yet**, **Checking…**, **Update failed**.
- **What's new in {version}**: the release notes.
- **Links**: **Repository**, **Report an issue**. **Fonts**. **For plugin authors**.

## WindowDeck strip
Opened by: the hotkey `Win+Alt+A` and then a taskbar number, or Tray menu → **WindowDeck: Open**, or by hovering a taskbar button when **Show WindowDeck instead of Windows' taskbar preview** is on, or `Win+1…9` when **Open WindowDeck with Win+1…9** is on.
- Toolbar: the app list, and the view switch **By monitor** and **Flat**.
- **No windows to arrange**: shown when nothing is open.
- **+ {n} other desktops**: link that shows windows of other desktops (`D`).
- **Hide other desktops**: the same link while they are shown.
- **Hide hints**: `Ctrl+H`; hides the key hints (**Show key hints** in the plugin settings brings them back).
- **Mark all as seen**: `Ctrl+M`; clears the orange marks. **Undo**: `Ctrl+Z`, for a few seconds after.
- **No windows on this desktop**.

### Custom WindowDeck: the Claude desktop app's cards
Shown when WindowDeck is on the Claude desktop app and **Claude card style** is **Classic** (the default). Other apps look as before.
- Each Claude card: a status chip in the top left corner of the picture (**Waiting** with the wait time, **Working** with the run time and a turning ring, **Background** with the time of the oldest job, "· 1 job" or "· 2 jobs" and a still empty ring, or **Idle**), and the chat's age at the end of the title line ("now", "12 min", "5 h", "2 days"; a snowflake from 3 days).
- Toolbar, after **By monitor** and **Flat**: the counters **Waiting**, **Working** (also counts the **Background** chats), **Idle**, **Cold**. A click on one, or `S`, lights up its cards in place and dims the others; `S` steps Waiting → Working → Idle → Cold → none. While one is lit the arrows move only between lit cards, and the toolbar says "Showing: Waiting · S next · Esc clears" (with the lit status). `Esc` clears it.
- **Board** button (`B`): opens **Claude board** over the deck, with the columns **Waiting for you**, **Working** (the **Background** chats come after the working ones), **Idle** and **Cold**. Arrows move between rows and columns, `Enter` or a click opens the chat, `Esc` goes back to the deck as it was.

### WindowDeck card, right-click menu
Opened by: right-click a window card in the strip.
- **Rename…**: only with WindowNamer on. **Clear name**: only for a window that has a name.
- **Close window**.
- **Taskbar place**: submenu; only when the app has more than one window in the row.
- **Move to desktop**: submenu with the desktops.
- **Add to group**: submenu with the app's groups and their open window counts, then **New group…**; only with **Group related windows** on and when the app has a group.
- **New group…  (Ctrl + G)**: the name box for this card and the picked ones (Ctrl + click picks).

### WindowDeck stack, right-click menu
Opened by: right-click a stack card (a closed group), with **Group related windows** on.
- **Open the group** (`Enter`), **Rename the group** (`F2`), **Ungroup** (`Ctrl+G`).
- **Close {n} windows…** (`Ctrl+W`, or a middle click on the stack): asks first.

### WindowDeck open group
Opened by: a click on a stack, its number, or **Open the group** (`Enter`).
- It opens on a layer over the row, on the stack's place; no card moves. The setting **Open a group as** picks a row centred on the stack or a fan.
- Frame header: **{group} · 1 – {n} opens a window · Esc closes the group** (row), or **{group} · 1 – {n} or ← → · Esc closes** (fan).
- Footer hints: **Window in the group** (`1 – 9`), **Close the group** (`Esc`), **Take out of the group** (`Ctrl + G`).
- A click outside the frame closes the group; WindowDeck stays open.
- Right-click on a window in the group: the card menu (**Rename…** and **Clear name** with WindowNamer on, **Close window**, **Taskbar place**, **Move to desktop**), then the group items.

### WindowDeck group dialogs
- Name box for a new group: title **Group {n} windows**, buttons **Cancel** and **Make the group**. Enter makes it, Esc cancels.
- After `F2` on a stack: **Rename the group**, buttons **Cancel** and **Rename**.
- Before closing a group's windows: title **Close {n} windows in {group}?**, buttons **Cancel** and **Close {n} windows**. Esc is Cancel.
- While a card is dragged over the middle of another card: **Drop to group**.

### WindowDeck compact popup
Opened by: the hover popup on a taskbar button, when **Show WindowDeck instead of Windows' taskbar preview** is on (an app with two or more windows).
- **Open**, **More** (`→`), **Mark all as seen**, **Back to this desktop**, **This desktop**, **Switch to {desktop}**.
- **Close**: the window close button on a card. **Back**: `Esc` goes back.

## QuickJump Quick access
Opened by: `` Win+` `` twice, or Tray menu → **Open quick access**.
- Title bar: **QuickJump · Quick access**.
- Per window: **Unpin** (`Del`), and a details area **How QuickJump finds this window**: **App**, **Title contains**, **Label (optional)**.
- **Done**: closes the details.
- Card right-click menu (also the Menu key or `Shift+F10`): **Jump to it**, **Put at number** (submenu 1 to 10), **Edit details** (`F2`), **Unpin** (`Del`), **Close the window** (only while the window is open). A middle click on a card unpins it too.
- **Add a window to Quick access**: opens the picker **QuickJump · Add a window** with **Search open windows**; **No open window matches** when empty.
- The chord tip after `` Win+` `` shows: **Add this window**, **Put this window at a number**, **Unpin this window**, **Open quick access**.
- Notices: **Not in Quick access**, **There is no window to pin**.

## Shortcut manager
Opened by: `Win+Alt+S`, Tray menu → **Shortcut manager**, or the palette command **Open shortcut manager**.
- Title bar: **WinCraft · Shortcut manager**.
- **Search shortcuts, meaning or owner**: filter box.
- **Check a shortcut**: press keys to see what holds them (**Click, then press keys**, **Press keys…**).
- **Show free**: lists free combinations. **Free close to it**: free combinations near a taken one.
- **Copy list**, **Refresh** (`F5`).
- Answer panel: **Press it for real**, **Change**, **Remove key**, **Save**, **Cancel**, **Back to the full list**.
- **No shortcuts match.**, **Scanning…**.

## ClaudeDeck
Opened by: `Win+Alt+C`, Tray menu → **ClaudeDeck**, or the palette command **Open ClaudeDeck**. `Win+Alt+C` again while it is in front closes it.
- The window has no Windows title bar. Drag its header to move it; drag an edge or a corner to resize it. It opens again at the same place and size.
- Header, left to right: a status mark and a count for the chats that need approval, ask you, work, or are done (only the statuses that have chats); the switch **Only open windows** (click the switch or its label); the pin (tooltip **Stay on top**), filled while the window stays on top; the close button (tooltip **Close**).
- A group header per project folder: click it to fold or unfold the group. Folded, it shows the most urgent mark and **{n} chats**. **New**: the group at the top for chats with no known folder.
- A chat row: the status mark, the name, the branch, **{n} jobs** while background jobs run, and the time in the status at the right edge. Click the row (or pick it with `↑` `↓` and press `Enter`) to bring the chat's window to the front; a chat with no open window does nothing.
- The line of text under the name (when **A line of text (the question, or what it does)** is on): the question when the chat asks you; **Waits for your OK to {run a command, read a file, edit a file, …}** (or **Waits for your OK**) when it needs approval; what it does while it works (**Running a command**, **Reading a file**, **Editing a file**, **Searching the files**, **Running a subagent**, **Looking on the web**, **Updating its plan**, **Using a skill**, **Using a connector**, **Using {tool}**, or **Thinking**); the first sentence of its last reply when done, or **Last: {sentence}** when idle or running background jobs; **You stopped it**; the error, or **The last request failed**; **WinCraft cannot read this chat's state** for an unknown status. A folded chat whose fork is the most urgent reads **A fork needs approval** and the fork's own line after a colon (also **A fork asks you**, **A fork is done**, **A fork works**, **A fork runs jobs**, **A fork is idle**, **A fork was stopped**, **A fork failed**).
- Rest the mouse on a chat for half a second: a panel shows **{name} · {status} · {time}** (status words: **needs approval**, **asks you**, **done, not seen**, **working**, **background**, **idle**, **stopped**, **failed**, **unknown**), then **{project} · {branch}**, then the full question or last reply, and **No open window** when the chat has no window.
- Forks hang under their chat with tree lines. The small arrow before a chat folds its forks; folded, the row shows the most urgent mark of the family and **{n} forks**. A grey dashed stand-in **{title} · archived** (or **Chat not found**) holds forks whose chat is gone; click it to fold them.
- The grey line **{n} cold chats** at the end holds the chats idle for over 1 day (when **Fold chats idle for over 1 day** is on): click it to list them under it.
- Empty: **No Claude chats yet.** **Chats show here when Claude Code runs.** With **Only open windows** on and no chat window open: **No chat has a window open.**

## Notices
Opened by: they show by themselves above the taskbar after a change. They take the mouse but never the keyboard.
- **Undo**: the button on QuickJump and QuickActions notices (`Ctrl+Z` works while the notice shows).
- **Put back**: the button on the LayoutKeeper offer "{app} restarted. Put its {n} windows back in order?" (apps that restart together share one notice: "{app} and {app} restarted. Put their {n} windows back in order?"); only when **Offer to put windows back after an app restarts** is on.
- **Layout saved: {n} windows in {m} apps.**: LayoutKeeper, after `Win+Alt+J` or Settings → Layout → **Save now**.
- **WinCraft · Putting your windows back — please wait**: the restore progress notice, when **Show restore progress** is on.

## Hotkeys
Defaults. Every one can be changed in Settings → Hotkeys.

| Action | Default | Change it in |
|---|---|---|
| **Open palette** | `Win+Alt+P` | Settings → General → **Palette hotkey** |
| **Restore layout** | `Win+Alt+L` | Settings → Hotkeys |
| **Save layout now** | `Win+Alt+J` | Settings → Hotkeys |
| **Move window left in taskbar** | `Win+Alt+[` | Settings → Hotkeys |
| **Move window right in taskbar** | `Win+Alt+]` | Settings → Hotkeys |
| **WindowDeck: Open** | `Win+Alt+A` | Settings → Hotkeys |
| **Jump to a pinned window** | `` Win+` `` | Settings → Hotkeys |
| **Pin this window…** | none | Settings → Hotkeys |
| **Rename the active window** | `Win+Alt+E` | Settings → Hotkeys |
| **Open shortcut manager** | `Win+Alt+S` | Settings → Hotkeys |
| **Open ClaudeDeck** | `Win+Alt+C` | Settings → Hotkeys |
| **Cover the monitor** (also **Cover left monitor**, **Cover right monitor**, **Cover monitor 2**, **Cover monitor 3**) | `Win+Alt+F1`, `F2`, `F3` | Settings → Hotkeys |
| **Turn the monitor off** (also **Turn left monitor off**, **Turn right monitor off**, **Turn monitor 2 off**, **Turn monitor 3 off**) | `Win+Alt+Shift+F1`, `F2`, `F3` | Settings → Hotkeys |
| **Wake all monitors** | `Win+Alt+F12` | Settings → Hotkeys |
| QuickActions, LanguageIndicator | none | Settings → Hotkeys |
