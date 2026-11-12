"""Writes the ClaudeDeck icon options (SVG) and the HTML of section 12 of the ClaudeDeck page.

Usage: make_claudedeck_icons.py <svg out dir> <section html out> <wincraft-64.rgba>
All icons are drawn on a 32 x 32 grid with even coordinates, so they stay sharp at 16 px.
"""
import base64
import os
import struct
import sys
import zlib

OUT_DIR, SECTION_OUT, RGBA = sys.argv[1], sys.argv[2], sys.argv[3]

TILE = "#1c1a19"
WHITE = "#f3f2f2"
AMBER = "#e1ad66"
WARN = "#e88a4a"
INFO = "#8fb0d0"
OK = "#8fbf7f"

RING = f'<rect x="1.5" y="1.5" width="29" height="29" rx="6.5" fill="{TILE}" stroke="{AMBER}" stroke-width="1.5"/>'


def rows(colours, widths, ink=WHITE, x0=6, bar_x=13):
    out = []
    for row, (c, w) in zip((8, 16, 24), zip(colours, widths)):
        out.append(f'<rect x="{x0}" y="{row - 2}" width="4" height="4" rx="1.2" fill="{c}"/>')
        out.append(f'<rect x="{bar_x}" y="{row - 2}" width="{w}" height="4" rx="1.2" fill="{ink}"/>')
    return "".join(out)


ICONS = {
    "a": ("A. The list", "WinCraft's dark tile and amber ring, with three chat rows: a status mark and a name line each.",
          RING + rows([WARN, INFO, OK], [13, 9, 11])),
    "b": ("B. Amber list", "The same list on an amber tile, so it never looks like the dark WinCraft icon next to it.",
          f'<rect x="1" y="1" width="30" height="30" rx="7" fill="{AMBER}"/>'
          + rows([TILE, TILE, TILE], [13, 9, 11], ink=TILE)
          + f'<rect x="6" y="6" width="4" height="4" rx="1.2" fill="{WHITE}"/>'),
    "c": ("C. A deck of chats", "Two cards: an amber one behind, a dark one in front with an orange status mark. A deck, as in the name.",
          f'<rect x="8" y="2" width="22" height="20" rx="5" fill="{AMBER}"/>'
          f'<rect x="2" y="9" width="22" height="21" rx="5" fill="{TILE}" stroke="{AMBER}" stroke-width="1.5"/>'
          f'<rect x="6" y="14" width="4" height="4" rx="1.2" fill="{WARN}"/><rect x="13" y="14" width="8" height="4" rx="1.2" fill="{WHITE}"/>'
          f'<rect x="6" y="22" width="15" height="4" rx="1.2" fill="{WHITE}" fill-opacity=".55"/>'),
    "d": ("D. A chat bubble", "A dark speech bubble with the amber ring, holding two chat lines with marks. Its shape is not a square.",
          f'<path d="M7 2.75h18a4.25 4.25 0 0 1 4.25 4.25v13a4.25 4.25 0 0 1-4.25 4.25H13l-7 5v-5.1A4.25 4.25 0 0 1 2.75 20V7A4.25 4.25 0 0 1 7 2.75z" '
          f'fill="{TILE}" stroke="{AMBER}" stroke-width="1.5" stroke-linejoin="round"/>'
          f'<rect x="7" y="8" width="4" height="4" rx="1.2" fill="{WARN}"/><rect x="14" y="8" width="11" height="4" rx="1.2" fill="{WHITE}"/>'
          f'<rect x="7" y="15" width="4" height="4" rx="1.2" fill="{OK}"/><rect x="14" y="15" width="8" height="4" rx="1.2" fill="{WHITE}"/>'),
}

os.makedirs(OUT_DIR, exist_ok=True)
svgs = {}
for key, (title, _, body) in ICONS.items():
    svg = f'<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32">{body}</svg>\n'
    svgs[key] = svg
    with open(os.path.join(OUT_DIR, f"claudedeck-icon-{key}.svg"), "w", encoding="utf-8", newline="\n") as f:
        f.write(svg)


def png_data_uri(path):
    d = open(path, "rb").read()
    w = h = 64
    raw = b"".join(b"\0" + d[y * w * 4:(y + 1) * w * 4] for y in range(h))

    def ch(t, b):
        return struct.pack(">I", len(b)) + t + b + struct.pack(">I", zlib.crc32(t + b) & 0xffffffff)

    png = (b"\x89PNG\r\n\x1a\n" + ch(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
           + ch(b"IDAT", zlib.compress(raw, 9)) + ch(b"IEND", b""))
    return "data:image/png;base64," + base64.b64encode(png).decode()


WINCRAFT = png_data_uri(RGBA)


def icon(key, px):
    return (f'<svg width="{px}" height="{px}" viewBox="0 0 32 32" aria-hidden="true" style="flex:none;display:block">'
            f'{ICONS[key][2]}</svg>')


def strip(key, light):
    bg, fg, line = ("#f3f3f3", "#1a1a1a", "#d6d6d6") if light else ("#202020", "#f3f2f2", "#3a3a3a")
    sizes = "".join(f'<div style="display:flex;flex-direction:column;align-items:center;gap:4px">{icon(key, px)}'
                    f'<span style="font-size:12px;color:{fg};opacity:.7">{px}</span></div>' for px in (16, 24, 32, 48))
    # A taskbar: WinCraft and ClaudeDeck side by side at the real 24 px, the ClaudeDeck button active.
    bar = (f'<div style="display:flex;align-items:center;gap:4px;padding:4px 8px;border-left:1px solid {line};margin-left:4px">'
           f'<span style="width:40px;height:40px;display:flex;align-items:center;justify-content:center;border-radius:4px">'
           f'<img src="{WINCRAFT}" width="24" height="24" alt="" style="display:block"></span>'
           f'<span style="width:40px;height:40px;display:flex;flex-direction:column;align-items:center;justify-content:center;'
           f'border-radius:4px;background:{"rgba(0,0,0,.06)" if light else "rgba(255,255,255,.07)"};position:relative">{icon(key, 24)}'
           f'<span style="position:absolute;bottom:2px;width:16px;height:3px;border-radius:2px;background:{AMBER}"></span></span></div>')
    return (f'<div style="display:flex;align-items:flex-end;gap:14px;padding:10px 14px;background:{bg};border:1px solid {line};'
            f'border-radius:8px">{sizes}{bar}</div>')


def option(key):
    title, why, _ = ICONS[key]
    return (f'<div style="display:flex;flex-direction:column;gap:10px;padding:16px;border:1px solid var(--border);border-radius:10px;'
            f'background:var(--panel)">'
            f'<div style="font-size:16px;font-weight:600;color:var(--text)">{title}</div>'
            f'<div style="font-size:14px;color:var(--text2);max-width:520px">{why}</div>'
            f'<div style="display:flex;gap:20px;align-items:flex-start;flex-wrap:wrap">'
            f'<div style="display:flex;flex-direction:column;gap:8px">{strip(key, True)}{strip(key, False)}'
            f'<div style="font-size:12px;color:var(--text2)">Sizes 16 – 48, then the taskbar: WinCraft, then ClaudeDeck (open).</div></div>'
            f'<div style="display:flex;flex-direction:column;align-items:center;gap:4px">{icon(key, 256)}'
            f'<span style="font-size:12px;color:var(--text2)">256</span></div></div>'
            f'<div style="font-size:12px;color:var(--text2)">Source: design/claude-design/claudedeck-icon/claudedeck-icon-{key}.svg</div>'
            f'</div>')


section = (
    '<section><h2>12. The ClaudeDeck icon (pick one)</h2>'
    '<p>ClaudeDeck gets its own icon for its taskbar button, its window title and its row on the Plugins and Store pages. '
    'Four ideas, each at 16, 24, 32, 48 and 256 px, on a light and a dark taskbar, next to the WinCraft icon. '
    'All four use the WinCraft colours (the dark tile, the amber ring, the status mark colours) and plain shapes. '
    'They are drawn on a 32 px grid, so 16 px stays sharp. None uses the Claude or Anthropic logo.</p>'
    '<div style="display:grid;grid-template-columns:1fr;gap:16px">'
    + "".join(option(k) for k in ICONS) + '</div></section>')

with open(SECTION_OUT, "w", encoding="utf-8", newline="\n") as f:
    f.write(section)
print("ok", len(section))
