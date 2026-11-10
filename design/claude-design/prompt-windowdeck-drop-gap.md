# Prompt for the design chat: WindowDeck drop gap

Paste everything below the line into the design chat of the artifact
"Wincraft Windows Utilities Design" (https://claude.ai/artifact/S5fEimN5sS6oqHgwJVRxPn).
Decided by the user on 2026-11-10.

---

Please update the WindowDeck pages (Arrange Strip, Window Deck Options, and
any page that shows a card being dragged) with a new way to show where a
dragged window will land.

**The problem today.** In WindowDeck's "By monitor" view, while you drag a
window card, a 2 px accent bar sits in the 8 px space between two cards to
mark the landing place. A window that wants attention has an orange frame,
drawn as a ring just outside its card, in that same space. In the Classic
theme the accent (#f0a640) and the attention orange (#e88a4a) look almost
the same. When the bar sits next to an orange card, the user cannot see the
bar.

**The user's decision: open a gap.** While a card is dragged, the other
cards slide apart and leave an empty slot where the window will land. The
bar goes away. This is how the Quick access window already works since
2026-11-08: cards slide aside in 200 ms, ease-out; on release the dragged
card slides into the gap; Esc slides everything back.

Please design:

1. **The slot.** It is as wide and as tall as the dragged card. It must not
   look like an orange attention frame. It must also not look like a
   "ghost" (the narrow card with a dashed outline that stands for a window
   on another monitor). A suggested start: a soft neutral hole, fill
   `text_primary` at about 6-8 %, the card's corner radius, and no outline
   or a thin neutral one. Show it in the light theme, the dark theme and
   Classic.
2. **The dragged card.** It lifts and follows the mouse, as in Quick
   access: drawn above the others, `palette_shadow` 8 px down with a 24 px
   blur, the selected look. Its old place does not keep a faded copy any
   more, because the gap now shows the landing place.
3. **Line breaks.** A monitor column wraps its cards into several lines.
   Show what happens when the gap pushes the last card of a line to the
   next line: it slides there too.
4. **Another monitor.** When the drag moves over another monitor's
   column, the gap opens there, and the first column closes its hole.
   The empty-monitor drop box ("Drop a window here to move it to this
   monitor") stays as it is.
5. **Ghosts.** A ghost card can be dragged too. Show the gap when a ghost
   is the card being dragged, and the ghosts moving aside like other
   cards.
6. One frame each: rest, the drag between two cards next to an orange
   card (the case that failed), the drop sliding in.

Keep every rule of the design brief: no text under 12 px, theme tokens
only, Windows 11 look. When it is done, reply with the page names you
changed, so the code session can pull them with /design-pull.
