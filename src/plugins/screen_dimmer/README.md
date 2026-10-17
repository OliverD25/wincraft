# ScreenDimmer

Covers a monitor with a black overlay you can click straight through, or
turns it off the way its power button does, without Windows treating it as
unplugged: your windows stay where they are. A monitor that cannot be turned
off this way is dimmed to brightness 0 or covered instead.

Useful when one screen is showing something you do not want to look at, or
do not want other people to look at, but you still want the windows on it
running.

## What it does

Each monitor has two hotkeys, counted from the left: `Win+Alt+F1` is the
left monitor, `F2` the centre one, `F3` the right one (with two monitors, F1
is the left and F2 the right one).

- The **plain key** (`Win+Alt+F1`) covers the monitor with the overlay and
  nothing else. Press it again and the cover goes. On a monitor that is off
  or dimmed it brings the monitor back.
- The **Shift key** (`Win+Alt+Shift+F1`) turns the monitor off, in the mode
  chosen for it below. On a covered monitor it goes on to off; on a monitor
  that is off or dimmed it brings the monitor back.

What "off" means depends on the monitor:

| Mode | What happens |
|---|---|
| **Off** | WinCraft asks the monitor for standby over DDC/CI, the command channel inside the monitor cable. The screen goes dark like with its power button. A black overlay stays underneath, so nothing shows if the monitor wakes by itself. |
| **Dim** | A black overlay, and the monitor's brightness set to 0. The old brightness comes back when you press the hotkey again. |
| **Cover** | Only the black overlay. The monitor keeps running. |

Each monitor's Shift-key mode is **Auto** until you choose another one in
the settings. The first time an Auto monitor is turned off, WinCraft asks it for
standby and watches its answer for 2 seconds. If it went into standby, it is
Off from then on. If it keeps saying it is on, nothing more is sent and it is
dimmed instead, or only covered when its brightness cannot be set either. A
monitor that does not answer DDC/CI at all is covered. WinCraft remembers
what it found; **Test monitors again** on the plugin's page makes it check
each monitor again the next time it is turned off.

While a monitor is off, the mouse pointer cannot go onto it: it would be
lost where you cannot see it. Bring the monitor back with either of its
hotkeys or with `Win+Alt+F12`; moving the mouse does not wake it. A covered
or dimmed monitor does not hold the pointer back. If every monitor is off,
the pointer stays on the primary one.

Waking can take a few seconds: some monitors do not answer at all while they
start up. WinCraft asks again every second for up to 8 seconds and removes
the overlay once the monitor says it is on.

The overlay never takes keyboard focus, so whatever you were typing in stays
active, and clicks go through to the windows underneath. On a covered or
dimmed monitor the overlay becomes partly see-through while your mouse
pointer is on it, so you can still find things there. On a monitor that is
off it stays solid.

## Hotkeys

| Action | Default |
|---|---|
| Cover left monitor | `Win+Alt+F1` |
| Turn left monitor off | `Win+Alt+Shift+F1` |
| Cover centre monitor | `Win+Alt+F2` |
| Turn centre monitor off | `Win+Alt+Shift+F2` |
| Cover right monitor | `Win+Alt+F3` |
| Turn right monitor off | `Win+Alt+Shift+F3` |
| Wake all monitors | `Win+Alt+F12` |

Monitors are counted from the left, whatever number Windows gives them. With
two monitors the actions are named left and right; with four or more they
are numbered from the left. The palette shows each monitor's state and place
under both of its commands, such as "off (standby) · left".

## Settings

| Setting | Meaning |
|---|---|
| `idle_opacity` | How solid the overlay is normally. `1.0` is fully black. |
| `hover_opacity` | How solid it is while your pointer is on that monitor. `0.7` lets a little through. |
| `monitors.<name>.mode` | What the Shift key does to one monitor: Auto, Off, Dim or Cover. The plugin's page has a choice per connected monitor ("Shift key turns the left monitor") and says what testing found. The plain key always only covers. |

A monitor's name is the one it reports itself, such as `PHL 323E7`. Two
monitors of the same model get their number from the left after it.
Settings take effect immediately; a new mode applies the next time the
monitor is turned off.

## Quitting and crashes

When WinCraft quits, when you switch ScreenDimmer off and when Windows shuts
down, every monitor WinCraft turned off is asked back on and every dimmed one
gets its brightness back. WinCraft writes down that a monitor is in standby,
or what its brightness was, before it changes anything, in
`plugins\screen_dimmer.state.json`. So if WinCraft is stopped without a
chance to clean up, the next start wakes the monitor and restores the
brightness.

## Notes

WinCraft only ever asks for standby (power mode 4), never for "off" (power
mode 5): after mode 5 some monitors stop listening to DDC/CI until you press
their power button.

DDC/CI can be switched off in a monitor's own menu. A monitor connected
through some docks, adapters or KVM switches does not get the commands
either; such a monitor is covered.

If you change your display layout or unplug a monitor while a screen is
dark, the overlays are rebuilt and each monitor keeps its state. The hotkeys
then follow the new order from the left.
