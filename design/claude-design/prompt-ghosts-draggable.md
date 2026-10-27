Please update the WinCraft design so that ghost cards (the stand-ins in the
primary monitor's column for windows that live on another monitor) are
DRAGGABLE everywhere. This is my decision after testing the app. The developer
builds from these pages, and the rule "ghosts are not draggable" keeps coming
back in new features, so it must disappear from every page and file.

## Where to change it
1. `Arrange Strip.dc.html` — the "Rules for the developer" list.
2. `design_handoff_arrange_strip/README.md` — the section "Taskbar order and
   ghosts" and the "Behaviour" / keyboard text.
3. `Arrange Strip Ghost Options.dc.html` — add a note at the top: "Superseded:
   ghosts are draggable, see Arrange Strip.dc.html".
4. Any other page or note that mentions ghosts (search for "ghost",
   "stand-in", "not draggable", "skip ghosts").

## The new rules (replace the old ones word for word)
- **Ghosts are draggable**, like cards. Remove "Ghosts are not draggable" and
  "arrow-key navigation skips them" everywhere.
- **Drag inside the primary column:** the ghost moves between the cards and
  other ghosts. On drop it changes that window's taskbar position, relative to
  its new neighbours. The window itself stays on its own monitor.
- **Drag into another monitor's column or an empty-monitor rail:** the window
  moves to that monitor on restore, exactly as when its real card is dragged
  there. After the drop it shows as a normal card in that column (and as a
  ghost only where the taskbar lists all windows).
- **Drag onto another desktop's band:** same as dragging its real card there.
- **While dragging:** the ghost keeps its ghost look (dashed border, faded
  picture) at 60 % opacity; the usual 2 px accent landing bar shows between the
  neighbours; empty-monitor rails widen into drop boxes as for cards.
- **Click or Enter** on a ghost opens its window. Middle-click closes it.
  Right-click opens the usual card menu.
- **Keyboard:** arrow keys land on ghosts like on cards (focus ring 2 px accent,
  2 px offset, outside the ghost box). With a ghost selected: Ctrl+←→ and
  Alt+←→ change its taskbar position (the window stays on its monitor);
  Ctrl+Shift+←→ moves the window to the next monitor; Ctrl+Shift+↑↓ moves it to
  the next desktop — the same keys as for cards.
- **Not draggable:** nothing. The compact view (one or two windows on this
  desktop) has no ghosts, so it is unchanged.

## Please also show
- One artboard (dark) with a ghost being dragged inside the primary column:
  the ghost at 60 % under the cursor and the landing bar between two cards.
- One artboard (dark) with a ghost dragged over an empty-monitor rail that has
  widened into its drop box.

Keep everything else as it is.
