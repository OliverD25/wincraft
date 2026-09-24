# WinCraft — brief for the design agent

You are designing the visual layer of **WinCraft**, an open-source Windows 11
tray utility written in Rust. The code already works. Your job is a **design
handout**: screens, layout rules, colours, type, spacing, states, and icon
guidance that a developer can implement in the existing toolkit without
guessing. Do not redesign what the app does. Design how it looks and feels.

Deliverable: one document (with images or artboards) covering the screens in
section 4, plus the token sheet in section 5. Every measurement in logical
pixels at 100 % scale. Every colour as hex, one value for dark and one for light.

---

## 1. What WinCraft is

A small always-running tray program, in the family of Microsoft PowerToys,
Flow Launcher and macOS Spotlight. One host process owns a tray icon, global
hotkeys and a settings window. Features are **plugins**. Two exist today:

- **ScreenDimmer** — blacks out chosen monitors with click-through overlays.
  The overlay turns slightly see-through when the mouse is over that monitor.
  Hotkeys `Win+Alt+F1/F2/F3` toggle monitors 1–3, `Win+Alt+F12` wakes all.
- **ShortcutDetector** — scans about 1,500 key combinations and shows which
  are taken by other apps, by Windows, or by WinCraft, and lets the user press
  a combination to get an instant verdict. Hotkey `Win+Alt+Q`.

Target user: a power user on Windows 11 with one to three monitors. Values
speed, low memory, no clutter. Reads settings once, then lives on hotkeys.

Repo: https://github.com/OliverD25/wincraft (plugins live in
`src/modules/<id>/` with their own `README.md`).

## 2. Design goals

1. **Quiet and native.** Looks at home next to Windows 11 Settings and
   PowerToys. No gradients, no decoration for its own sake.
2. **Fast to scan.** The palette must be readable in one glance; the settings
   pages must show every option without scrolling on a 1080p screen where
   possible.
3. **One system for both windows and both themes.** Same tokens for the
   palette, the settings window and the store. Dark and light are equal
   citizens; the app follows the Windows theme by default.
4. **Plugin-agnostic.** Pages are built from typed fields a plugin declares
   (section 6). The design must work for a plugin you have never seen.
5. **Minimal.** Fewer components, reused everywhere. If a component appears
   once, question it.

## 3. Hard technical constraints (read before drawing)

The UI is drawn with **egui**, an immediate-mode Rust GUI. It is not HTML and
not WinUI. What it can and cannot do:

| Can | Cannot |
|---|---|
| Solid fills, strokes, rounded corners (per-widget radius) | Blur / acrylic / mica backgrounds |
| One font family per weight, any size; a proportional and a monospace family are loaded | Web fonts per element; variable fonts; more than ~3 families without bloating the exe |
| Simple opacity, simple colour transitions (a few hundred ms) | Springy or choreographed animations; motion blur; parallax |
| Icons as vector paths or small PNG/SVG rasterised at build time | Icon fonts; emoji rendering is unreliable, avoid emoji as UI icons |
| Custom-drawn widgets: toggles, sliders, chips, badges, list rows | Native Windows controls inside egui windows |
| Drop shadow on the borderless palette window (drawn inside the window) | Shadows outside the window bounds |
| Rounded window corners on Windows 11 (system-drawn) | Custom window shapes |
| Light / dark palettes switched at runtime | Per-monitor different themes |

Other facts that shape the design:

- **Palette window:** borderless, always on top, no taskbar button, fixed
  680×420 logical px, placed at the top-centre of the monitor under the mouse
  (18 % down from the top). It hides on Esc, on focus loss, and after a
  command runs. It can be transparent, so a soft shadow inside the window
  edge is possible.
- **Settings window:** a normal resizable window, default 960×640, minimum
  720×480, standard Windows title bar. Left navigation column plus a page.
- **Text and rows:** current palette row height is 34 px; current base font is
  the egui default at 14 px. You may change both; give exact values.
- **Existing accent:** dark `#4C8BF5`, light `#162D5C`. Selection background
  is the accent at 45 % opacity. Item spacing 8 px, button padding 10×6 px.
  These are placeholders; replace them with a considered set.
- **DPI:** everything scales with Windows scaling (100 % to 300 %). Design at
  100 % and note anything that must change at 150 % or higher.
- **Keyboard first.** The palette is used without a mouse: ↑/↓, Enter, Esc.
  Focus and selection states must be visible without hover.
- **No network for assets.** Everything ships inside an 8 MB exe.
- **ShortcutDetector's own window is native Win32** (a classic list view), not
  egui. Do not design it now; note only what it should inherit (colours, type)
  when it is moved to egui later.

## 4. Screens to design

For each screen: layout at default size, the empty / loading / error states
that apply, dark and light versions, and the keyboard focus state.

### 4.1 Command palette (`Win+Alt+P`)

- Search field on top, results below, grouped by plugin ("WinCraft",
  "ScreenDimmer", "ShortcutDetector") with a small group header.
- Each row: command label left, hotkey hint right (e.g. `Win+Alt+F1`), a
  selected state for the row under the keyboard cursor.
- Empty query shows all commands grouped. A query shows fuzzy matches; the
  matched characters may be highlighted.
- States: no results ("Nothing matches"), a command that is disabled because
  its plugin is off (show it dimmed with a hint, or hide it: decide and say
  why).
- Optional footer line with two or three key hints (Enter run, Esc close).
- Total height is fixed; if the list is longer, it scrolls inside.

### 4.2 Settings window: frame and navigation

- Left column: product name, version, then four items: General, Plugins,
  Plugin Store, About. Selected item state. Column width at default and at
  minimum window size.
- Page area: title, optional one-line description, then content.
- Decide whether pages use cards (PowerToys / Flow style rows with a
  description under the label) or a plain form grid. Pick one and use it on
  every page.

### 4.3 General page

Rows: Start with Windows (toggle), Theme (System / Light / Dark), Palette
hotkey (hotkey capture box, see 4.7), Config file path with "Open",
Log file path with "Open".

### 4.4 Plugins page and plugin detail page

- List: one row per plugin with name, version, author, one-line description
  and an enabled toggle. Clicking a row opens the detail page; design the
  "back to all plugins" affordance.
- Detail page sections, in this order: header (name, version, author,
  description, Enabled toggle); **Settings** (typed fields from section 6);
  **Hotkeys** (one row per action: label, current binding as a keycap-style
  chip, status text "registered" or "taken by another app" in a warning
  colour, a Reset button); **Files** (path to the plugin's JSON file, "Open
  file", "Reset to defaults"); **About this plugin** (the plugin's README
  rendered as Markdown: headings, paragraphs, lists, code, links).
- Provide the Markdown type scale for the README block.

### 4.5 Plugin Store page

- List of plugins from an online index. Each entry: name, version, author,
  description, and one badge: "Enabled", "Installed, disabled" (with an
  Enable button), or "Needs WinCraft x.y" (with a "Get update" button).
- Detail: README rendered plus "Open on GitHub".
- Status line at the bottom: "2 plugins listed", "offline, showing the cached
  copy", or "Loading…". Design all three.
- Refresh button.

### 4.6 About page

Version, build date, links (repository, issues), a "Check for updates" button
with three result states (up to date, update available with a link, could not
check). A small "For plugin authors" note listing the supported field kinds.

### 4.7 Shared components

- **Hotkey capture box.** An input that shows the current binding as keycaps
  (`Win` `Alt` `F1`). When focused it says "Press keys…" and shows the
  modifiers live as the user holds them. Below it a one-line verdict:
  "Free" (success colour), "Taken by another app" (warning), "Windows
  shortcut: Open File Explorer" (info), "Used by WinCraft: ScreenDimmer —
  Toggle monitor 1" (info). Apply and Cancel actions. Esc cancels.
- **Toggle**, **slider with numeric readout**, **number field**, **text
  field**, **choice (dropdown or segmented control, decide)**, **path field
  with a Browse button**. These six are the plugin field kinds.
- **Badges / chips** for statuses. **Buttons**: primary, secondary, danger
  (Reset to defaults). **Group header** text style. **Empty state** block.
- **Tray icon and app icon.** Current icon is a flat dark-blue rounded square
  with a white "W". Propose a refined version that reads at 16 px in the
  Windows 11 tray on both light and dark taskbars, and a 256 px app icon.
  Provide the tray icon as a monochrome (white, with alpha) variant too,
  because Windows 11 renders tray icons on both dark and light taskbars.
- **Tray context menu** is a native Windows menu; only specify the item order
  and wording if you want changes.

## 5. Token sheet to deliver

- Colours, dark and light: window background, panel/card background, elevated
  (palette) background, border, text primary, text secondary, text disabled,
  accent, accent hover, accent pressed, selection background, success,
  warning, danger, info, focus ring.
- Type: family (one proportional, one monospace; both must be redistributable
  under an open licence), sizes and weights for title, page heading, section
  heading, body, secondary, caption, keycap, code.
- Spacing scale (4-based recommended), corner radii (window, card, button,
  chip, keycap), border widths, shadow for the palette.
- Sizes: palette row height, settings nav width, toggle size, slider track
  and knob, button height, input height.
- Motion: which transitions exist (palette show/hide, toggle, hover) with
  durations; everything else is instant.

## 6. Plugin field kinds (the page builder's vocabulary)

A plugin declares a list of fields; the app draws them. Each field has a key,
a label, a one-line help text and one of these kinds:

| Kind | Meaning | Example |
|---|---|---|
| Toggle | on / off | "Show free combinations by default" |
| Slider { min, max, step } | numeric with a visual track and a readout | "Darkness 0.00–1.00 step 0.05" |
| Number { min, max } | typed number | "Scan timeout, ms" |
| Text | free text | "Label prefix" |
| Choice([options]) | one of a fixed list | "Position: Top / Centre / Bottom" |
| Path | file or folder path with Browse | "Sound file" |

Design each once; the developer reuses them.

## 7. What we need back, in this order

1. Token sheet (section 5) as a table, dark and light side by side.
2. Component sheet (4.7) with all states.
3. Screens 4.1 to 4.6 at default size, dark and light.
4. Icon set: app icon 256 px, tray icon 16/20/24 px colour and monochrome.
5. A one-page "implementation notes" list: anything that is easy to get wrong
   in an immediate-mode GUI (focus rings, hover on touch, minimum sizes,
   scaling at 150 % and 200 %).

Keep the handout short and exact. A table beats a paragraph. A measured
mock-up beats a mood board.
