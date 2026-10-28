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
| **Desktop: Toggle Desktop Icons** | Hides the icons on the desktop, for a clean screen to share, or shows them again. Your files stay where they are. |
| **Taskbar: Toggle Auto-Hide** | Hides the taskbar until the mouse reaches the screen edge, or keeps it on screen. |
| **File Explorer: Toggle Hidden Files** | Shows or hides hidden files in File Explorer. |
| **File Explorer: Toggle File Extensions** | Shows or hides the end of file names, such as `.txt`. |

The window actions change the window you were in before you opened the
palette. They never touch WinCraft's own windows. A maximised window is
made normal first. Sizes are real screen pixels of the visible window, so
a 1280 × 720 window records as 1280 × 720 on any scale. A size larger than
the screen is made smaller to fit, and the notice says so.

## Undo

Every change shows a notice from WinCraft with an **Undo** button, for
8 seconds (longer while the mouse rests on it). Undo puts back exactly what
was there before: the window's old place and size, maximised or not, or
the setting's old value. Running a switch again also switches it back.

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
