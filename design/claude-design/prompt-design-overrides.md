Please update the WinCraft design pages with decisions I made while testing the
app. The developer builds from these pages, so every rule that still says the
old thing comes back in new features.

1. **Arrange Strip (page and design_handoff_arrange_strip/README.md), ghosts:**
   replace "Ghosts are not draggable. Click / Enter selects the real card in its
   own column … arrow keys skip ghosts" with:
   - Ghosts are draggable. Inside the primary column, a drag changes that
     window's taskbar position; the window stays on its own monitor. Dropped
     into another monitor's column or another desktop's band, the window moves
     there, like its real card.
   - A click or Enter on a ghost opens its window. Arrow keys land on ghosts
     like on cards. Ctrl+←→ and Alt+←→ on a selected ghost change its taskbar
     position.
2. **Arrange Strip keys (footer and README):** arrows only move the selection.
   Moving a window: Ctrl+←→ within the monitor, Ctrl+Shift+←→ to the next
   monitor, Ctrl+Shift+↑↓ to the next desktop. Alt+←→ changes the taskbar number
   without moving the window. Enter opens the selected window.
3. **Compact view:** it shows when the app has 1 or 2 windows on the current
   desktop (not only exactly 1). With 2, the two cards sit side by side.
4. **WinCraft Themes:** the theme choice is a row of tiles with a small preview,
   on its own Settings page "Themes" (after General). Pointing at a tile shows
   that theme in the whole app; a click keeps it. "Mode" (System / Dark / Light)
   sits under the tiles.
5. **Palette (v2):** "Ask AI" opens the chosen AI website (Claude, ChatGPT, …) in
   the browser with the question; the answer is not shown inside the palette.
   No currency conversion.

Keep everything else as it is.
