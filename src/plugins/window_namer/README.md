# WindowNamer

Give any window a name of your own. Three Chrome windows called "Mail",
"Research" and "Bank" instead of three page titles; two terminals called
"Server" and "Build". The name is what the taskbar thumbnail shows when you
rest the mouse on the button, what Alt+Tab shows and what the title bar
shows.

## Naming a window

- Press **Win+Alt+E** (Rename the active window) in the window you want to
  name. The palette opens with a box for the name; the name the window has
  now is filled in and selected. **Enter** sets the name, **Enter** on an
  empty box takes the name away, **Esc** changes nothing.
- In the Arrange strip, right-click a window's card: **Rename…** opens the
  same box for that window, and **Clear name** takes its name away.
- The palette command **Clear all names** takes every name away and forgets
  the saved ones.

WinCraft's own windows are never renamed.

## Apps that change their title

Most apps set their title again all the time: Chrome on every tab change, a
terminal on every command. WindowNamer notices each change within a moment,
remembers the app's new title, and puts the name back. If an app answers
every name with a title of its own more than five times a second,
WindowNamer lets it be for ten seconds and then tries again, so the two
never fight without end.

**Title shows** chooses what a renamed window's title says: the name alone
(the default), or the name followed by the app's own title, like
"Mail — Inbox - Gmail - Google Chrome".

## What the rest of WinCraft sees

- The Arrange strip and the palette's windows list (`<`) show the name. The
  palette finds a renamed window by its name and by the app's own title,
  which it shows under the name.
- LayoutKeeper keeps working with the app's own title, so renaming a window
  does not make it forget where that window belongs.

## After a restart

With **Keep names after restart** on (the default) every name is saved in
`window_namer.names.json` next to the plugin's settings, with the app it
belongs to and the app's latest title. When WinCraft starts, and whenever a
window opens, a saved name goes back on:

1. the window of the same app with the same title, or still showing the
   name itself;
2. for a browser window, the window showing the same page;
3. else the app's only window, when the app has exactly one saved name.

A name is never guessed: when two windows fit it equally, neither gets it.
A name that has not been on any window for 30 days is forgotten. When you
close a renamed window its name is kept, but in the same session only a
window with the same title gets it back, not the next window of that app.

When WindowNamer is switched off, or WinCraft quits, every renamed window
gets its app's own title back.

## What it cannot rename

- An app that runs as administrator, while WinCraft does not: Windows does
  not let one app change the other's title. WindowNamer says so.
- An app that ignores a title set from outside, or that is not responding.
