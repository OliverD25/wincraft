# Project notes

- WindowDeck thumbnail size is being tested in production (Window Deck Options 3a, Nov 2026). The previous view is the rollback point: THUMB_HEIGHT 96, TITLE 18, TITLE_GAP 6, CARD_PAD 4, CARD_GAP 8, GHOST_WIDTH 84, GHOST_STRIP 20, GHOST_PILL 14, GHOST_CHIP 16, EMPTY 112, MIN_PANEL 680 (src/ui/arrange/by_monitor.rs). Full table: design_handoff_window_deck_size/README.md. If the user wants to go back, restore these.
- Quick access (pinned windows) chosen: Quick Access Options 4a. Cards 300 px, picture 188 px (16:10), title 18 px, 4 per row then wrap, keycap number under each card, numbers follow pin order.
