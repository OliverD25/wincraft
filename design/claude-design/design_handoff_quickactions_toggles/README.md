# Handoff: QuickActions toggle rows

How a toggle command looks in the palette list. Reference: `Toggle States Options.dc.html`, block "Chosen" (b + a).

## Row (48 px min, radius 6, padding 0 12, gap 12)
1. **State tile** 28 × 28, radius 7, icon 16 px Lucide, stroke 2.
   - On: category colour fill, white icon of the on state.
   - Off: fill rgba(255,255,255,0.06), 1 px border #5c5858, icon #bab6b6 of the off state.
2. **Title** 15 px, never changes with state (search and recall stay stable).
3. **State line** 12 px text-secondary #bab6b6, says the current state in words.
4. **Switch** at the far right, 40 × 20, radius 10, knob 12.
   - On: accent #e1ad66 fill and border, knob #1c1a19 at the right.
   - Off: 1 px #bab6b6 border, no fill, knob #bab6b6 at the left.

Enter flips the toggle; the row updates in place and the palette stays open. Read the real state each time the palette opens; never assume it from the last action.

## Commands
| Title | On icon / Off icon | Tile | State line on / off |
| --- | --- | --- | --- |
| Theme: Toggle Dark Mode | moon / sun | #6b55b8 | Dark for apps and Windows / Light for apps and Windows |
| File Explorer: Toggle File Extensions | file-text / file | #b8862e | Endings like .txt are shown / hidden |
| File Explorer: Toggle Hidden Files | eye / eye-off | #b8862e | Hidden files are shown / not shown |
| Desktop: Show Desktop Icons | layout-grid / square + slash | #2f7f8f | Desktop icons are shown / hidden |
| Taskbar: Toggle Auto-Hide | panel-bottom dashed / panel-bottom | #3f6fb8 | Hides until you point at it / Taskbar always stays visible |
| Audio: Toggle Microphone Mute | mic-off / mic | on #b8483a | Microphone is muted / Microphone is on |
| Display: Toggle Keep Awake | coffee / cup without steam | #a8673a | PC stays awake / PC sleeps as usual |
| Window: Toggle Always on Top | pin / pin-off | #5a6578 | <window> stays on top / <window> is not on top |

## Notes
- **Desktop icons** is renamed from "Toggle Desktop Icons" so on = icons visible. Keep the old name as a search alias.
- **Microphone:** on means muted, so its on tile is red instead of the category colour.
- **Always on Top** is per window: state and state line refer to the window that had focus when the palette opened. With no window, hide the switch and show "No window selected".
- If a state can't be read, show the tile as off, hide the switch, and set the state line to "State unknown".
- The same pattern applies to any future toggle command.
