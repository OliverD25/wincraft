"""Add section 15, the context ring on each chat row, to ClaudeDeck.dc.html.

Usage: python -I make_claudedeck_context_ring.py <in.html> <out.html>
Input is the v45 page. The section goes in before the "Rules" section.
"""
import math
import re
import sys

src, dst = sys.argv[1], sys.argv[2]
page = open(src, encoding="utf-8").read()
assert "15. Context ring" not in page, "section 15 is already in the page"


def mark(needle):
    m = re.search(r'<svg class="mk"(?:(?!</svg>).)*' + re.escape(needle) + r'(?:(?!</svg>).)*</svg>', page)
    assert m, needle
    return m.group(0)


MK = {
    "working": mark('stroke="var(--info)" stroke-opacity=".3"'),
    "done": mark('fill="var(--ok)"/><path d="M4 7.2'),
    "asks": mark("M7 1.5a5.3"),
    "stopped": mark('rx="1.5" fill="var(--dis)"'),
}

CSS = """
.cring{flex:none;display:block}
.l1 .cring{margin-left:2px}
.l1 .tm + .cring, .l1 .cring + .tm{margin-left:0}
.l1 .rgt{margin-left:auto;display:flex;align-items:center;gap:6px;padding-left:6px}
.l1 .rgt .tm{margin-left:0;padding-left:0}
.bands{display:flex;gap:18px;flex-wrap:nowrap;padding:12px 14px}
.band{display:flex;align-items:center;gap:7px;font-size:12px;color:var(--text2)}
.band b{color:var(--text);font-weight:600}
.hpctx{font-size:12px;color:var(--text2);margin:-2px 0 6px;display:flex;gap:6px;align-items:center}
"""
page = page.replace("</style>", CSS + "</style>", 1)


def band(pct):
    if pct is None:
        return None
    if pct < 15:
        return "var(--text2)"
    if pct < 45:
        return "var(--ok)"
    if pct < 75:
        return "var(--accent)"
    return "var(--bad)"


def ring(pct, size=12):
    c = size / 2
    r = c - 1.6
    circ = 2 * math.pi * r
    track = (f'<circle cx="{c}" cy="{c}" r="{r:.2f}" fill="none" stroke="var(--text2)" '
             f'stroke-opacity=".28" stroke-width="2"/>')
    arc = ""
    if pct is not None:
        fill = circ * max(pct, 3) / 100
        arc = (f'<circle cx="{c}" cy="{c}" r="{r:.2f}" fill="none" stroke="{band(pct)}" stroke-width="2" '
               f'stroke-dasharray="{fill:.2f} {circ:.2f}" transform="rotate(-90 {c} {c})"/>')
    title = "Context size not known" if pct is None else f"Context {pct}%"
    return (f'<svg class="cring" width="{size}" height="{size}" viewBox="0 0 {size} {size}" '
            f'role="img" aria-label="{title}">{track}{arc}</svg>')


# name, branch, model, status, time, context %, line 2
CHATS = [
    ("WinCraft Rust", "main", "Opus 5.5", "working", "2 min", 41, "Running a command"),
    ("WinCraft design", "design", "Opus 5.5", "asks", "4 min", 8, "Which option should the open group use?"),
    ("Playwise - Kids Learn", "main", "Sonnet 5.5", "working", "30 s", 62, "Reading the lesson screens"),
    ("verdict-ai API", "stage-2", "Opus 5.5", "done", "12 min", 88, "Tests pass. The import is fixed."),
    ("newsHQ parser", "main", "Haiku 5.5", "done", "25 min", 27, "Saved 140 new articles"),
    ("F1OS catalog", "main", "Sonnet 5.5", "stopped", "1 h", None, "Stopped by you"),
]


def row(chat, place):
    name, branch, model, status, tm, pct, line2 = chat
    rg = ring(pct)
    if place == "A":
        l1 = (f'<span class="nm">{name}</span><span class="tag">{branch}</span><span class="tag">{model}</span>'
              f'<span class="rgt">{rg}<span class="tm">{tm}</span></span>')
    else:
        l1 = (f'<span class="nm">{name}</span><span class="tag">{branch}</span><span class="tag">{model}</span>'
              f'{rg}<span class="tm">{tm}</span>')
    return (f'<div class="row two"><span style="width:10px;flex:none"></span>{MK[status]}'
            f'<div class="body"><div class="l1">{l1}</div><div class="t2">{line2}</div></div></div>')


GHEAD = ('<div class="ghead"><svg width="10" height="10" viewBox="0 0 10 10" style="transform:rotate(0deg)" '
         'aria-hidden="true"><path d="M2 3.5l3 3 3-3" fill="none" stroke="var(--text2)" stroke-width="1.4"/></svg>'
         '<span class="gdot" style="background:var(--accent)"></span><span class="gname">{g}</span>'
         '<span class="gcount">{n}</span></div>')
REC = '<span class="rec">Recommended</span>'


def deck(theme, place):
    rows = "".join(row(c, place) for c in CHATS[:3])
    rows2 = "".join(row(c, place) for c in CHATS[3:])
    return (f'<div class="{theme} deck" style="width:360px">' + GHEAD.format(g="WinCraft", n=3) + rows
            + GHEAD.format(g="Other projects", n=3) + rows2 + '<div style="height:6px"></div></div>')


def bands(theme):
    items = [
        (5, "Fresh", "under 15%"),
        (30, "Fine", "15 to 45%"),
        (60, "Getting full", "45 to 75%"),
        (88, "Near compaction", "over 75%"),
        (None, "Not known", "no size yet"),
    ]
    inner = "".join(f'<span class="band">{ring(p, 14)}<b>{t}</b> {d}</span>' for p, t, d in items)
    return f'<div class="{theme} deck" style="display:inline-block"><div class="bands">{inner}</div></div>'


def hover(theme, chat, ctx_text, pct):
    name, branch, model, status, tm, _, body = chat
    project = {"WinCraft Rust": "wincraft", "verdict-ai API": "verdict-ai", "F1OS catalog": "fabrica-os"}[name]
    word = {"working": "working", "done": "done, not seen", "asks": "asks you", "stopped": "stopped"}[status]
    return (f'<div class="{theme} hoverpanel" style="margin:0">'
            f'<div class="hp1">{MK[status]}{name} · {word} · {tm}</div>'
            f'<div class="hp1">{project} · {branch}</div>'
            f'<div class="hpctx">{ring(pct, 12)}{ctx_text}</div>'
            f'{body}</div>')


def cap(text):
    return f'<div class="cap">{text}</div>'


section = (
    '<section><h2>15. Context ring on each chat (for review 2026-11-14)</h2>'
    '<p>Asked on 2026-11-14 in the WinCraft Rust chat. Each chat row gets a small ring that shows how full the '
    "chat's context is. 100% is the point where Claude Code compacts the chat: 777,000 tokens on your PC "
    "(autoCompactWindow), about 160,000 on a 200k model. The ring fills clockwise from the top over a faint "
    'track. Its colour changes in four bands. There is no setting to hide it, and mini mode shows no ring.</p>'

    '<h3 style="margin-top:16px">The ring and its bands</h3>'
    '<p>Two things differ from the brief. Yellow is the amber accent colour: the theme has no yellow, and its '
    '"warning" orange already means "needs you" on the status marks. A chat with no known size shows the empty '
    "track only, so it does not look like a fresh chat. Even 1% shows a short arc, so a fresh chat never looks "
    'empty.</p>'
    '<div class="side" style="flex-direction:column;gap:12px">'
    f'<div>{cap("Dark")}{bands("dark")}</div><div>{cap("Light")}{bands("light")}</div></div>'

    '<h3 style="margin-top:24px">Where it sits in the row: two options</h3>'
    '<p>The same six chats in both options, with <b>Model name</b> on. '
    "A: the ring sits at the right edge, just before the time. B: the ring follows the model name.</p>"
    f'<p>{REC} A. The status mark at the left is already a ring for "Working", so a second ring next to it '
    "would read as one more status. At the right edge the two are apart. All rings also line up in one column, "
    "so you can scan for the red one from top to bottom. In B the ring moves with the length of each name, "
    "and a long name pushes it out of view.</p>"
    '<div class="side">'
    f'<div>{cap("Option A, dark: before the time " + REC)}{deck("dark", "A")}</div>'
    f'<div>{cap("Option B, dark: after the model name")}{deck("dark", "B")}</div>'
    '</div><div class="side" style="margin-top:20px">'
    f'<div>{cap("Option A, light")}{deck("light", "A")}</div>'
    f'<div>{cap("Option B, light")}{deck("light", "B")}</div>'
    '</div>'

    '<h3 style="margin-top:24px">The hover panel</h3>'
    "<p>One new line, after the project and branch line. It names the size, the limit and the percentage. "
    '"Compacted 2 times" shows only when the chat was compacted at least once. The same ring stands at '
    "the start of the line.</p>"
    '<div class="side">'
    f'<div>{cap("Dark, a compacted chat")}'
    f'{hover("dark", CHATS[0], "Context 318k of 777k (41%) · compacted 2 times", 41)}</div>'
    f'<div>{cap("Light, a 200k model, never compacted")}'
    f'{hover("light", CHATS[3], "Context 141k of 160k (88%)", 88)}</div>'
    f'<div>{cap("Dark, size not known yet")}'
    f'{hover("dark", CHATS[5], "Context size not known yet", None)}</div>'
    '</div></section>'
)

anchor = "<section><h2>Rules</h2>"
assert page.count(anchor) == 1
page = page.replace(anchor, section + anchor, 1)
open(dst, "w", encoding="utf-8", newline="").write(page)
print("written", dst, len(page.encode("utf-8")), "bytes")
