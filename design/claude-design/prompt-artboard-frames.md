A request about how the WinCraft design pages show windows.

On several pages (WinCraft Themes, Palette Main View, Palette Main View v2,
Keyboard Shortcuts, QuickJump), each window sits inside a large coloured
"desk" area: a gradient or a solid colour with padding around the window. That
area was meant only to show how the background affects the look, especially for
the translucent Fluent theme. The developer read it as part of the window, and
in a test build the palette got a blurred outer frame around it. It looked very
ugly.

Please change the pages so this cannot be misread:

1. **Mark the desk as background.** On every artboard where a window sits on a
   coloured desk, add a small label in the corner of the desk area:
   "Desktop background, not part of WinCraft". Keep it 12 px, text-disabled.
2. **Show the real window edge clearly.** The window itself ends at its 1 px
   border and its rounded corners. Nothing outside it belongs to WinCraft: no
   outer frame, no margin, no extra glass. The shadow is the system shadow that
   Windows draws.
3. **Fluent page (WinCraft Themes, Palette Main View v3 Fluent):** add one rule
   to the developer notes: "The acrylic fills only the window itself, edge to
   edge. The window is exactly the panel size, with Windows' own rounded corners
   and shadow. There is no transparent margin around it and no caption buttons
   (minimise, maximise, close) on the palette or the strips."
4. **Settings window:** if it is shown on a desk, show its real title bar (the
   normal Windows one) and say in the notes that the desk is background.
5. Keep everything else as it is.
