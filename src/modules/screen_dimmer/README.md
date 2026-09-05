# ScreenDimmer

Blacks out a monitor with an overlay you can click straight through.

Useful when one screen is showing something you do not want to look at, or do
not want other people to look at, but you still want the windows on it running.

## What it does

Press the hotkey for a monitor and it goes black. Press it again and it comes
back. The overlay never takes keyboard focus, so whatever you were typing in
stays active, and clicks go through to the windows underneath as if the overlay
were not there.

When your mouse pointer moves onto a blacked-out monitor the overlay becomes
partly see-through, so you can still find things on it. Move the pointer away
and it goes fully black again.

## Hotkeys

| Action | Default |
|---|---|
| Toggle monitor 1 | `Win+Alt+F1` |
| Toggle monitor 2 | `Win+Alt+F2` |
| Toggle monitor 3 | `Win+Alt+F3` |
| Wake all monitors | `Win+Alt+F12` |

Monitors are numbered in the order Windows enumerates them. Monitor 1 is
normally your primary display.

## Settings

| Setting | Meaning |
|---|---|
| `idle_opacity` | How solid the overlay is normally. `1.0` is fully black. |
| `hover_opacity` | How solid it is while your pointer is on that monitor. `0.7` lets a little through. |

Both take effect immediately — you do not have to restart WinCraft.

## Notes

If you change your display layout or unplug a monitor while a screen is dimmed,
the overlays are rebuilt and the ones that were dimmed come back dimmed.

The overlay is a layered, click-through, no-activate window per monitor. It
covers the whole monitor rectangle in physical pixels, so mixed-DPI setups work.
