# WindowDeck: larger thumbnails (test)

Design: `Window Deck Options.dc.html`, option **3a**. Only sizes change; layout, colours and behaviour stay as they are.
File: `src/ui/arrange/by_monitor.rs`

## Test values

| Constant | Current (keep for rollback) | Test |
| --- | --- | --- |
| THUMB_HEIGHT | 96.0 | 160.0 |
| TITLE | 18.0 | 22.0 |
| TITLE_GAP | 6.0 | 8.0 |
| CARD_PAD | 4.0 | 6.0 |
| CARD_GAP | 8.0 | 12.0 |
| GHOST_WIDTH | 84.0 | 120.0 |
| GHOST_STRIP | 20.0 | 30.0 |
| GHOST_PILL | 14.0 | 20.0 |
| GHOST_CHIP | 16.0 | 22.0 |
| EMPTY | 112.0 | 180.0 |
| MIN_PANEL | 680.0 | 900.0 |

Unchanged: everything else in by_monitor.rs, including CARD_HEIGHT (it is derived), RAIL, GHOST_OPACITY, the toolbar and the footer.

Card width keeps coming from `picture_width` (window aspect × THUMB_HEIGHT), so a 16:9 window on Centre is about 284 px wide and a window on the portrait Right monitor is narrower. The mock-up uses a taller picture on Right; the code uses one THUMB_HEIGHT for all monitors, which is fine for the test.

If the panel would be wider than the work area, the thumbnails should shrink back towards 96 px, never below.

## Rollback

Put the "Current" column back. No other file changes.
