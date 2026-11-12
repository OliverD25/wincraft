# ClaudeDeck: a number on the tray icon (proposal, not built)

The design (`design/claude-design/ClaudeDeck.dc.html`, section 10) shows an
orange number on WinCraft's tray icon when chats wait, and the tooltip
"WinCraft — 2 chats wait for you".

## What is built

The setting **Show a count on the tray icon** sets the tooltip only. It uses
`host::set_tray_tip`, which plugins already have, so the host did not change.
The window tells the plugin the new text through `host::post_to_plugins`,
and the plugin sets it on the host thread. With no waiting chat, or with
ClaudeDeck closed, the tooltip is the plain "WinCraft" again. The count is
there only while ClaudeDeck is open: closed, ClaudeDeck reads nothing
(Principle 5).

## What is not built, and why

A number on the icon means drawing a new icon each time the count changes:
the WinCraft icon, plus a small rounded badge with the digits, made into an
`HICON` and handed to `Shell_NotifyIconW(NIM_MODIFY, NIF_ICON)`. The host's
`Tray` keeps its own icon today (`refresh_icon` reloads it for the theme), so
a plugin icon would have to fit into that, and a mistake there touches the
icon every WinCraft user sees. That is more than a small, safe host change.

## The smallest host change that would do it

1. `traits.rs`: an optional `fn tray_badge(&self) -> Option<u32> { None }` on
   the plugin trait, asked when a plugin calls `host::plugin_changed()`.
2. `core/tray.rs`: when any enabled plugin reports a badge, draw the normal
   small icon into a 32-bit DIB, paint a filled circle and the number (up to
   "9+") at the top right, make an `HICON` with `CreateIconIndirect`, and set
   it with `NIM_MODIFY`. When no plugin reports one, set the normal icon
   again, exactly as `refresh_icon` does today. Free the old badge icon after
   each change.
3. Tests: the badge text for 0, 1, 9, 10 and more; the plain icon comes back
   when the count goes to 0 or the plugin is switched off.

Other plugins change nothing, and with no badge the icon stays as it is.
