"""Renders ClaudeDeck's window icon from its design SVG.

Usage (from the repo root):
    python tools/claudedeck-icon.py

Reads design/claude-design/claudedeck-icon/claudedeck-icon-b.svg (option B,
"Amber list", the creator's pick of 2026-11-12) and writes, into
src/plugins/claude_deck/icon/:
  - claudedeck.ico: 16, 20, 24, 32, 48 and 256 px, for the taskbar and the
    title (Windows picks the size it needs);
  - claudedeck-64.rgba: 64 x 64 raw RGBA, the picture egui gives the window.

Each size is drawn from the SVG's shapes at that size (eight times larger,
then averaged down), never scaled from another size. Every edge is first
moved to a whole pixel of that size, so 16 px stays sharp; only the
rounded corners are soft. Needs Pillow. The SVG
holds only <rect> elements (x, y, width, height, rx, fill, fill-opacity),
which is all this script reads.
"""
import math
import os
import re

from PIL import Image, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SVG = os.path.join(ROOT, "design", "claude-design", "claudedeck-icon", "claudedeck-icon-b.svg")
OUT = os.path.join(ROOT, "src", "plugins", "claude_deck", "icon")
GRID = 32.0
SIZES = [16, 20, 24, 32, 48, 256]
SUPER = 8


def rects(svg):
    for tag in re.findall(r"<rect\b[^>]*>", svg):
        attrs = dict(re.findall(r'([\w-]+)="([^"]*)"', tag))
        yield attrs


def colour(attrs):
    fill = attrs["fill"].lstrip("#")
    alpha = round(255 * float(attrs.get("fill-opacity", "1")))
    return tuple(int(fill[i : i + 2], 16) for i in (0, 2, 4)) + (alpha,)


def snap(value):
    """A shape's edge moved to the nearest whole pixel of the target size,
    so no edge falls half way through a pixel and blurs (the 16 px icon
    puts the bars' left edge, 13 on the grid, at 6.5 otherwise)."""
    return math.floor(value + 0.5)


def render(svg, size):
    big = size * SUPER
    scale = size / GRID
    image = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    for attrs in rects(svg):
        layer = Image.new("RGBA", (big, big), (0, 0, 0, 0))
        x, y = float(attrs["x"]), float(attrs["y"])
        w, h = float(attrs["width"]), float(attrs["height"])
        left, top = snap(x * scale) * SUPER, snap(y * scale) * SUPER
        right, bottom = snap((x + w) * scale) * SUPER, snap((y + h) * scale) * SUPER
        radius = float(attrs.get("rx", "0")) * scale * SUPER
        ImageDraw.Draw(layer).rounded_rectangle(
            [left, top, right - 1, bottom - 1],
            radius=round(radius),
            fill=colour(attrs),
        )
        image = Image.alpha_composite(image, layer)
    return image.resize((size, size), Image.Resampling.BOX)


def main():
    with open(SVG, encoding="utf-8") as f:
        svg = f.read()
    os.makedirs(OUT, exist_ok=True)
    images = [render(svg, size) for size in SIZES]
    largest = images[-1]
    largest.save(
        os.path.join(OUT, "claudedeck.ico"),
        format="ICO",
        sizes=[(s, s) for s in SIZES],
        append_images=images[:-1],
    )
    with open(os.path.join(OUT, "claudedeck-64.rgba"), "wb") as f:
        f.write(render(svg, 64).tobytes())
    print(f"wrote claudedeck.ico ({', '.join(map(str, SIZES))} px) and claudedeck-64.rgba to {OUT}")


if __name__ == "__main__":
    main()
