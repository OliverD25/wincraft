Please update the WinCraft design pages with decisions I made while testing the
app. The developer builds from these pages, so every rule that still says the
old thing comes back in new features.

1. **New name: WindowDeck.** The "Arrange strip" is now its own plugin called
   WindowDeck (it is no longer part of LayoutKeeper). Rename it on every page
   and in `design_handoff_arrange_strip/README.md`: titles, labels, footers,
   palette commands ("WindowDeck: Open", "WindowDeck: Switch taskbar hover on
   or off"), tray entries ("WindowDeck on taskbar hover"). LayoutKeeper's own
   pages no longer mention the strip.
2. **Digits inside WindowDeck:** a plain digit 1–9 (0 = 10) opens the window
   whose card shows that number chip. **Ctrl+digit** jumps to the app at that
   taskbar button. Update the footer key hints: "1–9 Open by number" and
   "Ctrl + 1–9 Taskbar button".
3. **Win+Alt+A then a digit:** the number badges under the taskbar buttons do
   NOT show at once. They fade in only if no digit was typed within 0.7 s, as a
   help for when the user does not know the number. The same for the small
   wait line. Please show one artboard state "0–0.7 s: nothing on screen" and
   one "after 0.7 s: badges".
4. **New WindowDeck setting** (on its plugin page, Settings tab): a switch
   "Open WindowDeck with Win+1…9", off by default, with the line "Win+Shift,
   Win+Ctrl and Win+Alt with a number stay with Windows." When it is on,
   Win+digit always opens WindowDeck for that taskbar button, even for an app
   with one window; it stays open after Win is released; pressing the digit
   again while Win is held selects the next card.
5. **Plugins Grid v2 (option 1a):** please add the 1a grid to a README handoff
   (it has none yet). One decision: the tile shows **no user count** — WinCraft
   collects no usage data at all, so there is no number to show. A Store
   plugin that needs a newer WinCraft shows a "Get update" button in the place
   of Install.

Keep everything else as it is.
