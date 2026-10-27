# Handoff: Arrange strip — grouped by monitor and desktop (Reorder plugin)

## Overview
Update to the Reorder plugin's Arrange view in WinCraft (Rust / egui). The flat per-desktop strip becomes a map of the desk: one band per virtual desktop, each band split into one column per physical monitor, laid out left-to-right in the monitor's real position. Window order, drag-to-reorder and restore behaviour stay; grouping and drag-across-monitor are new.

## About the design file
`Arrange Strip.dc.html` is a **visual reference built in HTML** (dark and light artboards at 1440 wide). Do not port the HTML — recreate in egui with the WinCraft tokens already in the codebase. All values are logical px at 100 % scale. High fidelity: match sizes and colours exactly.

## Tokens used (from the WinCraft sheet)
| Token | Dark | Light |
|---|---|---|
| window bg | `#1c1a19` | `#f3f2f2` |
| panel bg | `#262322` | `#eae9e9` |
| border | `#444141` | `#d7d3d3` |
| text primary | `#f3f2f2` | `#201f1d` |
| text secondary | `#bab6b6` | `#605d5d` |
| text disabled | `#7d7979` | `#9b9797` |
| accent | `#e1ad66` | `#b68235` |
| selection bg | accent @ 18 % | accent @ 16 % |

Font: system Segoe UI Variable Text (Selawik fallback). Sizes: header 13 semibold, band title 13 semibold, meta 12, monitor label 12 semibold, card title 13, footer 12, badge 11.

## Layout

### Toolbar (top, padding 16 20 12)
- App picker dropdown, 32 high, 1 px border r4, chevron-down 14 px.
- View switch, segmented 32 high: **By monitor** | **Flat**. Selected option = accent text + 1 px inset accent stroke. Default By monitor when ≥ 2 monitors, Flat when 1.
- Right-aligned summary, 13 px secondary: "3 monitors · 6 desktops · 27 windows".

### Desktop band (one per virtual desktop, 1 px top rule, padding 12 20 16)
- Header row: desktop name 13 semibold + meta 12 text-disabled ("Desktop 1 · 12 windows"). Current desktop first, name in accent, band at 100 % alpha; other desktops in Windows order at 85 % alpha.
- A desktop with no windows for the selected app collapses to its header row only.
- Body: CSS-grid-equivalent of monitor columns, gap 16.

### Monitor column
- Track width proportional to monitor width (`monitor.width / total width`), minimum 240. Example 1920 / 3440 / 1920 → `2fr 3fr 2fr`.
- 1 px left rule, 12 px left padding.
- Label row 12 px: Lucide `monitor` 12 px, name semibold secondary ("Left" / "Centre" / "Right" — derive from x order; use the device's friendly name if available), resolution text-disabled, optional "Primary" outlined chip 18 high, window count right-aligned.
- Empty column: dashed 1 px border box, 112 high, r4, centred 12 px text-disabled "Drop a window here to move it to this monitor".
- Cards wrap in rows, gap 8.

### Window card (172 wide)
- Padding 4, r4. Thumbnail 96 high, r3, 1 px border, 16:9 crop of the window capture. Title 13 px, one line, ellipsised, padding 0 2.
- **Selected:** selection bg fill + 2 px accent ring at 2 px offset (state-driven, visible without hover).
- **Hover:** hover bg (text primary @ 5 %). Never on touch.
- **Dragging:** card at 60 % alpha; a 2 px accent insertion bar between neighbour cards or at the edge of the target column.
- **Moved badge:** when a window's live monitor ≠ its saved slot, an 18-high chip top-right of the thumbnail: window bg fill, 1 px accent border, accent text "Moved". Clears when restore runs.

### Footer (1 px top rule, padding 10 20)
Left 12 px secondary: "Order on other desktops applies at the next restore." Right: key hints as panel-bg chips (1 px border r4, padding 1 6): `←→` Move within monitor · `Shift + ←→` Move to next monitor · `Shift + ↑↓` Move to next desktop.

## Behaviour
- Monitor order: `EnumDisplayMonitors`, sort by work-area `left`, ties by `top`. Monitors stacked vertically get a second row of columns under the same desktop header.
- Dropping a card in another column changes its target monitor in the saved layout; restore moves the window there, preserving relative size. Dropping into another band changes its virtual desktop.
- Keyboard: ↑↓ move selection across cards (wrapping columns), ←→ reorder within column, Shift+←→ move to adjacent monitor (appends at end), Shift+↑↓ move to adjacent desktop (same monitor if present, else primary). Enter = restore selected, Esc = close.
- Scroll: whole strip scrolls vertically; never horizontally. Columns wrap cards instead.
- Flat view = previous behaviour (one wrapping strip per desktop), unchanged.
- Data per window: hwnd, title, thumbnail, saved monitor id, live monitor id, desktop id, order index.

## Edge cases
- Monitor disconnected: its column stays, labelled "(disconnected)" in text-disabled, cards at 45 % alpha, restore falls back to primary.
- Monitor width below min track: clamp to 240 and let the row scroll horizontally only inside that band — the only horizontal scroll allowed.
- Very long titles: ellipsis; full title on hover tooltip after 600 ms.
- DPI 150 %+: snap 1 px rules and the 2 px ring to whole physical pixels.

## Files
- `Arrange Strip.dc.html` — visual reference (dark + light), requires the sibling `_ds/` folder and `support.js`.
- See the main WinCraft handoff README for the full token sheet and component rules this view inherits.
