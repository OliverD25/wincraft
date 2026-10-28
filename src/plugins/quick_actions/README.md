# QuickActions

Windows tools that Settings hides several clicks deep, one search away in
the palette. Open the palette (**Win+Alt+P**), type a few letters such as
`dark`, `center` or `awake`, and press **Enter**.

Every action is called "Area: Verb", so typing the area lists its actions:
`window` shows all window actions, `explorer` both File Explorer switches.
Each action is also found by other words people use for it: `night` finds
dark mode, `caffeine` finds keep awake, `pin` finds always on top.

## The actions

| Action | What it does |
|---|---|
| **Window: Toggle Always on Top** | Keeps the window you were in above all other windows, or stops it. |
| **Window: Center** | Moves the window to the middle of its screen, same size. |
| **Window: Resize to 1280 × 720** | Makes the window 1280 by 720 pixels, in the middle of its screen. |
| **Window: Resize to 1920 × 1080** | Makes the window 1920 by 1080 pixels, in the middle of its screen. |
| **Window: Resize to 80 % of the Monitor** | Makes the window 80 % of its screen's width and height, in the middle. |
| **Display: Toggle Keep Awake** | Keeps the PC and the screen from sleeping until you run it again. |
| **Theme: Toggle Dark Mode** | Switches apps and Windows between dark and light, together. |
| **Desktop: Show Desktop Icons** | Hides the icons on the desktop, for a clean screen to share, or shows them again. Your files stay where they are. Typing `toggle desktop icons` still finds it. |
| **Taskbar: Toggle Auto-Hide** | Hides the taskbar until the mouse reaches the screen edge, or keeps it on screen. |
| **File Explorer: Toggle Hidden Files** | Shows or hides hidden files in File Explorer. |
| **File Explorer: Toggle File Extensions** | Shows or hides the end of file names, such as `.txt`. |
| **Network: Copy Local IP** | Copies this PC's address on your network, such as `192.168.1.20`. |
| **Network: Flush DNS Cache** | Makes Windows look up web addresses afresh, as `ipconfig /flushdns` does. |
| **Clipboard: Convert to Plain Text** | Keeps only the text on the clipboard, without fonts, colours or links. |
| **Clipboard: Copy New UUID** | Copies a new random UUID (version 4). |
| **Audio: Toggle Microphone Mute** | Mutes the default microphone, or turns it on again. |
| **System: Empty Recycle Bin** | Deletes everything in the Recycle Bin for good. Asks for **Enter** twice. |
| **Settings: Open Environment Variables** | Opens the window where you edit `PATH` and other variables. |
| **Developer: Kill Process on Port** | Lists the programs that listen on a TCP port; ends the one you pick. Asks for **Enter** twice. |
| **Audio: Switch Output Device** | Lists the speakers and headphones; sound plays on the one you pick. |
| **Window: Resize…** | Lists the window sizes, with your own sizes from the plugin's page. |
| **System: Eject USB Drive** | Lists the USB drives and memory cards; ejects the one you pick so you can unplug it. |
| **Display: Keep Awake for…** | Keeps the PC awake for 30 minutes, 1 hour, 2 hours, or until you turn it off. |

## Switches in the palette

The eight actions that switch something on and off (always on top, keep
awake, dark mode, desktop icons, taskbar auto-hide, hidden files, file
extensions and the microphone) show their state in the palette. A small
tile shows the state with its icon, a line under the name says it in
words, such as "Hidden files are not shown", and a switch at the right
shows on or off. WinCraft reads the state from Windows each time the
palette opens.

**Enter** flips the switch and the palette stays open, so you see the new
state at once. The notice with **Undo** still appears. A hotkey on one of
these actions runs it without the palette.

Always on top is about the window you were in. With no such window the
line says "No window selected" and there is no switch. When Windows does
not say how a setting is, the line says "State unknown".

## Actions with a list

The last five actions show a list in the palette instead of running at
once. Type to filter it, press **Enter** to pick a row, or **Esc** to go
back to your search.

- **Kill Process on Port** shows each port with the program and its
  process number (PID). Typing a number such as `3000` finds the program
  on that port. Ending a program cannot be undone, so it asks for
  **Enter** twice. WinCraft never offers itself or the Windows system
  processes. If Windows refuses, for example because the program runs as
  administrator, the notice says so.
- **Switch Output Device** marks the device in use. Windows has no public
  way for apps to switch it, so WinCraft uses the same hidden interface as
  Sound settings. If that fails, the notice says so; switch it in Sound
  settings then. **Undo** puts the old device back.
- **Resize…** offers 1280 × 720, 1920 × 1080, 80 % of the monitor, and
  the sizes in **More sizes for Window: Resize…** on the plugin's page,
  such as `1600x900, 1366x768`.
- **Eject USB Drive** first makes sure no program has a file open on the
  drive. If one has, the notice says the drive is in use and nothing
  happens.
- **Keep Awake for…** shows when each choice ends. While it runs, the
  tray item says "Keep awake until 15:20", and a notice tells you when the
  time is up.

A hotkey on one of these actions opens the palette on its list, because
picking a row needs the palette.

## What the palette shows before you press Enter

Select an action in the palette, and the panel on the right (the
inspector) shows how things are now and what **Enter** will do. WinCraft
reads this only while the row is selected, and reading it changes nothing.

- A switch says its state: "Now: Dark · Enter switches to Light".
- A window action names the window, its size now, the screen's size and
  the size it will get: "Enter makes it 1280 × 720".
- Keep awake says how long it has been on: "On for 12 min".
- Copy Local IP lists the addresses by connection (Wi-Fi, Ethernet, WSL);
  **Enter** copies the first one. It only reads the PC's own network
  adapters and asks no outside service.
- Convert to Plain Text shows the start of the text and its length.
- Copy New UUID shows the UUID; **Enter** copies exactly that one.
- Toggle Microphone Mute names the microphone and says whether it is muted.
- Empty Recycle Bin shows how many items it holds and how much space they
  take.

## Enter twice for what cannot be undone

Emptying the Recycle Bin cannot be undone, so the first **Enter** only
shows "Press Enter again to empty the Recycle Bin". A second **Enter**
within 5 seconds empties it; any other key cancels. If you give it a
hotkey, the hotkey opens the palette on it instead of emptying the bin, so
one key press alone never deletes anything.

The window actions change the window you were in before you opened the
palette. They never touch WinCraft's own windows. A maximised window is
made normal first. Sizes are real screen pixels of the visible window, so
a 1280 × 720 window records as 1280 × 720 on any scale. A size larger than
the screen is made smaller to fit, and the notice says so.

## Undo

Every change shows a notice from WinCraft with an **Undo** button, for
8 seconds (longer while the mouse rests on it). Undo puts back exactly what
was there before: the window's old place and size, maximised or not, the
setting's old value, or the microphone as it was. Running a switch again
also switches it back. Copying to the clipboard, clearing the DNS cache and
emptying the Recycle Bin have no Undo; the notice still says what happened.

If Windows does not take a change, for example when the window belongs to
an app that runs as administrator, the notice says so and the log
(`%LOCALAPPDATA%\WinCraft\wincraft.log`) has the reason. Nothing fails
silently.

## Keep awake

Keep awake stays on until you run it again, switch QuickActions off, or
quit WinCraft. The tray menu has a **Keep awake** item with a tick while it
is on, and the plugin's page says how long it has been on. It does not
change your power settings; Windows simply waits while WinCraft asks it to.

## Hotkeys

QuickActions has no hotkeys of its own, so it takes no keys from other
apps. To give an action a hotkey, select it in the palette and press
**Ctrl+K**.

## Privacy

QuickActions does everything on your PC. It sends nothing anywhere.
