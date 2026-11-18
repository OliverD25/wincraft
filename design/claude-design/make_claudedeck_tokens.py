"""Add section 16, tokens per chat, to ClaudeDeck.dc.html.

Usage: python -I make_claudedeck_tokens.py <in.html> <out.html>
Input is the v47 page. The section goes in before the "Rules" section.
It shows the Stats tab and the row token bar as built in 0.20.135, then
options for the per-chat graph (A1-A4) and the per-row indicator (B1-B4).
"""
import math
import re
import sys

src, dst = sys.argv[1], sys.argv[2]
page = open(src, encoding="utf-8").read()
assert "16. Tokens per chat" not in page, "section 16 is already in the page"


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
.st{padding:10px 12px 12px;font-size:13px}
.stbtns{display:flex;gap:6px}
.stbtn{height:24px;padding:0 10px;border-radius:4px;display:inline-flex;align-items:center;font-size:13px;color:var(--text);border:1px solid var(--border);background:var(--panel)}
.stbtn.on{background:var(--sel);border-color:var(--accent)}
.stnote{font-size:12px;color:var(--text2);margin:8px 0 2px}
.sth{font-size:12px;font-weight:600;color:var(--text2);margin:14px 0 4px}
.brow{height:32px;display:flex;flex-direction:column;justify-content:center;gap:4px}
.brow .bl{display:flex;align-items:baseline;gap:6px}
.brow .bn{font-size:13px;flex:1;min-width:0;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.brow .bs{font-size:12px;color:var(--text2)}
.track{height:6px;border-radius:3px;background:var(--border);display:flex;overflow:hidden}
.track i{display:block;height:100%}
.cols{display:flex;align-items:flex-end;gap:4px;height:56px}
.cols i{flex:1;background:var(--accent);border-radius:2px 2px 0 0;display:block}
.clab{display:flex;gap:4px;font-size:12px;color:var(--text2);height:18px;align-items:center}
.clab span{flex:1;text-align:center}
.stack{height:10px;border-radius:3px;display:flex;overflow:hidden}
.stack i{display:block;height:100%}
.leg{height:20px;display:flex;align-items:center;gap:6px;font-size:12px}
.leg .d{width:8px;height:8px;border-radius:2px;flex:none}
.leg .v{margin-left:auto;color:var(--text2)}
.tbar{position:absolute;left:12px;bottom:0;height:2px;background:var(--accent);opacity:.7}
.rowfill{position:absolute;left:0;top:0;bottom:0;background:var(--text2);opacity:.09;pointer-events:none}
.hchip{font-size:12px;padding:0 5px;height:16px;line-height:16px;border-radius:8px;flex:none}
.spark{display:flex;align-items:flex-end;gap:1px;flex:none}
.spark i{display:block;width:3px;background:var(--accent);border-radius:1px}
.spark i.z{background:var(--border)}
.sig{display:flex;align-items:flex-end;gap:1px;flex:none;width:13px;height:12px;margin-top:2px}
.sig i{display:block;width:2px;border-radius:1px;background:var(--border)}
.sig i.on{background:var(--accent)}
.tmap{position:relative;height:200px;margin-top:4px}
.tile{position:absolute;box-sizing:border-box;border:2px solid var(--win);border-radius:5px;padding:4px 6px;overflow:hidden;font-size:12px;line-height:1.3}
.tile b{display:block;font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.donut{display:flex;gap:14px;align-items:center;margin-top:4px}
.donut .lg{flex:1;min-width:0;display:block}
.donut .lg .leg{display:flex;width:auto}
.donut .leg .n{white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0}
.optcap{font-size:13px;font-weight:600;margin:0 0 4px}
"""
page = page.replace("</style>", CSS + "</style>", 1)

MODEL_COL = {"Opus 5.5": "var(--accent)", "Sonnet 5.5": "var(--ok)", "Haiku 5.5": "var(--warn)"}

# name, group, status, share %, {model: part of the chat}, tokens, 7 days (Mon..Today)
CHATS = [
    ("WinCraft Rust", "WinCraft", "working", 31, {"Opus 5.5": .86, "Sonnet 5.5": .14}, "41.2M", [5, 9, 12, 4, 0, 14, 20]),
    ("verdict-ai API", "verdict-ai", "done", 18, {"Opus 5.5": .62, "Sonnet 5.5": .38}, "27.9M", [12, 10, 0, 8, 9, 3, 0]),
    ("F1OS catalog", "F1OS", "stopped", 12, {"Sonnet 5.5": 1.0}, "38.4M", [0, 0, 6, 11, 9, 2, 0]),
    ("WinCraft design", "WinCraft", "asks", 9, {"Opus 5.5": 1.0}, "9.6M", [2, 0, 0, 3, 1, 4, 6]),
    ("newsHQ parser", "newsHQ", "done", 7, {"Haiku 5.5": .7, "Sonnet 5.5": .3}, "52.1M", [4, 4, 4, 4, 4, 3, 2]),
    ("Playwise - Kids Learn", "Playwise", "working", 6, {"Sonnet 5.5": 1.0}, "18.3M", [0, 0, 0, 0, 2, 5, 7]),
    ("UniComOS inbox", "UniComOS", "done", 5, {"Opus 5.5": .5, "Haiku 5.5": .5}, "8.7M", [3, 2, 2, 0, 0, 0, 1]),
    ("Claude config", "Other", "done", 4, {"Sonnet 5.5": .8, "Haiku 5.5": .2}, "7.9M", [1, 1, 1, 1, 1, 1, 1]),
]
TOP = CHATS[0][3]
DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Today"]


def tabs():
    return ('<div class="tabs"><span class="tab">Chats</span><span class="tab">News</span>'
            '<span class="tab on">Stats</span></div>')


def cap(text):
    return f'<div class="cap">{text}</div>'


def deck(theme, inner, width=360, tabbar=True):
    return (f'<div class="{theme} deck" style="width:{width}px">' + (tabs() if tabbar else "")
            + f'<div class="st">{inner}</div></div>')


def head_buttons():
    return ('<div class="stbtns"><span class="stbtn">Today</span><span class="stbtn on">7 days</span></div>'
            '<div class="stnote">Bars weigh each token by its model\'s API price.</div>')


# ---- the Stats tab as built in 0.20.135 ----
def bar_rows(items):
    out = ""
    for name, share in items:
        out += (f'<div class="brow"><div class="bl"><span class="bn">{name}</span><span class="bs">{share}%</span></div>'
                f'<div class="track"><i style="width:{share / items[0][1] * 100:.0f}%;background:var(--accent)"></i></div></div>')
    return out


def current_stats():
    groups = [("WinCraft", 40), ("verdict-ai", 18), ("F1OS", 12), ("newsHQ", 7), ("Not in a group", 23)]
    days = [27, 28, 25, 31, 17, 32, 37]
    hours = [1, 0, 0, 0, 0, 0, 0, 1, 3, 6, 8, 9, 7, 6, 9, 10, 11, 8, 5, 4, 6, 7, 5, 3]
    inner = head_buttons()
    inner += '<div class="sth">Top chats</div>' + bar_rows([(c[0], c[3]) for c in CHATS])
    inner += '<div class="sth">Groups</div>' + bar_rows(groups)
    m = max(days)
    inner += ('<div class="sth">Days</div><div class="cols">' + "".join(f'<i style="height:{d / m * 100:.0f}%"></i>' for d in days)
              + '</div><div class="clab">' + "".join(f"<span>{d}</span>" for d in DAYS) + "</div>")
    models = [("Opus 5.5", 61), ("Sonnet 5.5", 31), ("Haiku 5.5", 8)]
    inner += ('<div class="sth">Models</div><div class="stack">'
              + "".join(f'<i style="width:{v}%;background:{MODEL_COL[n]}"></i>' for n, v in models) + "</div>"
              + "".join(f'<div class="leg"><span class="d" style="background:{MODEL_COL[n]}"></span>{n}<span class="v">{v}%</span></div>'
                        for n, v in models))
    m = max(hours)
    inner += ('<div class="sth">Busiest hours</div><div class="cols" style="gap:2px">'
              + "".join(f'<i style="height:{max(h / m * 100, 3):.0f}%"></i>' for h in hours)
              + '</div><div class="clab" style="justify-content:space-between"><span style="flex:none">0</span>'
              '<span style="flex:none">6</span><span style="flex:none">12</span><span style="flex:none">18</span>'
              '<span style="flex:none;visibility:hidden">0</span></div>')
    return inner


# ---- options for the per-chat graph ----
def a1():
    out = ""
    for name, _, _, share, models, toks, _ in CHATS:
        parts = "".join(f'<i style="width:{share / TOP * p * 100:.1f}%;background:{MODEL_COL[mn]}"></i>'
                        for mn, p in models.items())
        out += (f'<div class="brow"><div class="bl"><span class="bn">{name}</span>'
                f'<span class="bs">{toks} tokens · {share}%</span></div><div class="track">{parts}</div></div>')
    legend = '<div style="display:flex;gap:12px;margin-top:6px">' + "".join(
        f'<span class="leg" style="height:auto"><span class="d" style="background:{c}"></span>{n}</span>'
        for n, c in MODEL_COL.items()) + "</div>"
    return '<div class="sth" style="margin-top:4px">Top chats</div>' + out + legend


def treemap(items, x, y, w, h):
    if len(items) == 1:
        return [(items[0], x, y, w, h)]
    total = sum(i[3] for i in items)
    acc, cut = 0, 1
    for k, it in enumerate(items[:-1], 1):
        acc += it[3]
        cut = k
        if acc >= total / 2:
            break
    left, right = items[:cut], items[cut:]
    f = sum(i[3] for i in left) / total
    if w >= h:
        return treemap(left, x, y, w * f, h) + treemap(right, x + w * f, y, w * (1 - f), h)
    return treemap(left, x, y, w, h * f) + treemap(right, x, y + h * f, w, h * (1 - f))


def a2():
    out = ""
    for it, x, y, w, h in treemap(CHATS, 0, 0, 336, 200):
        name, share = it[0], it[3]
        alpha = 0.18 + 0.5 * share / TOP
        label = f"<b>{name}</b>{share}%" if w > 60 and h > 34 else f"<b>{share}%</b>"
        out += (f'<div class="tile" style="left:{x:.1f}px;top:{y:.1f}px;width:{w:.1f}px;height:{h:.1f}px;'
                f'background:color-mix(in srgb, var(--accent) {alpha * 100:.0f}%, var(--win))">{label}</div>')
    return ('<div class="sth" style="margin-top:4px">Top chats</div><div class="tmap">' + out + "</div>"
            '<div class="stnote">The size of a tile is its share. Hover a tile for its name and counts.</div>')


def spark(days, w=3, hmax=14):
    m = max(days) or 1
    return '<span class="spark">' + "".join(
        f'<i class="{"z" if d == 0 else ""}" style="width:{w}px;height:{max(d / m * hmax, 2):.0f}px"></i>' for d in days) + "</span>"


def a3():
    out = ""
    for k, (name, _, _, share, _, _, days) in enumerate(CHATS, 1):
        out += (f'<div class="brow" style="flex-direction:row;align-items:center;gap:8px">'
                f'<span class="bs" style="width:14px;text-align:right">{k}</span>'
                f'<span class="bn">{name}</span>{spark(days)}'
                f'<span class="bs" style="width:30px;text-align:right">{share}%</span></div>')
    return ('<div class="sth" style="margin-top:4px">Top chats</div>' + out
            + '<div class="stnote">Each small chart is the chat\'s 7 days, Monday to today.</div>')


def a4():
    r, sw = 52, 13
    circ = 2 * math.pi * r
    cols = ["var(--accent)", "var(--ok)", "var(--info)", "var(--warn)", "var(--bad)", "var(--text2)"]
    top5 = CHATS[:5]
    rest = sum(c[3] for c in CHATS[5:]) + (100 - sum(c[3] for c in CHATS))
    items = [(c[0], c[3]) for c in top5] + [("Other chats", rest)]
    arcs, off = "", 0
    for (n, v), c in zip(items, cols):
        ln = circ * v / 100
        arcs += (f'<circle cx="60" cy="60" r="{r}" fill="none" stroke="{c}" stroke-width="{sw}" '
                 f'stroke-dasharray="{max(ln - 1.5, .5):.2f} {circ:.2f}" stroke-dashoffset="{-off:.2f}" '
                 f'transform="rotate(-90 60 60)"/>')
        off += ln
    svg = (f'<svg width="130" height="130" viewBox="-5 -5 130 130" role="img" aria-label="Share of tokens by chat">{arcs}'
           f'<text x="60" y="58" text-anchor="middle" font-size="16" font-weight="600" fill="var(--text)">7 days</text>'
           f'<text x="60" y="75" text-anchor="middle" font-size="12" fill="var(--text2)">205M tokens</text></svg>')
    leg = "".join(f'<div class="leg"><span class="d" style="background:{c}"></span><span class="n">{n}</span>'
                  f'<span class="v">{v}%</span></div>' for (n, v), c in zip(items, cols))
    return ('<div class="sth" style="margin-top:4px">Top chats</div><div class="donut">' + svg
            + f'<div class="lg">{leg}</div></div>')


# ---- the Chats rows and the per-row options ----
ROW_CHATS = [CHATS[0], CHATS[3], CHATS[5], CHATS[1], CHATS[2]]
BRANCH = {"WinCraft Rust": "main", "WinCraft design": "design", "Playwise - Kids Learn": "main",
          "verdict-ai API": "stage-2", "F1OS catalog": "main"}
TIME = {"WinCraft Rust": "2 min", "WinCraft design": "4 min", "Playwise - Kids Learn": "30 s",
        "verdict-ai API": "12 min", "F1OS catalog": "1 h"}
LINE2 = {"WinCraft Rust": "Running a command", "WinCraft design": "Which option should the open group use?",
         "Playwise - Kids Learn": "Reading the lesson screens", "verdict-ai API": "Tests pass. The import is fixed.",
         "F1OS catalog": "Stopped by you"}


def chip(share):
    a = 0.12 + 0.6 * share / TOP
    fg = "var(--win)" if a > 0.45 else "var(--text)"
    return (f'<span class="hchip" title="{share}% of the last 7 days" '
            f'style="background:color-mix(in srgb, var(--accent) {a * 100:.0f}%, transparent);color:{fg}">{share}%</span>')


def sig(share):
    lvl = 4 if share >= 25 else 3 if share >= 12 else 2 if share >= 6 else 1
    return '<span class="sig" title="{0}% of the last 7 days">'.format(share) + "".join(
        f'<i class="{"on" if k <= lvl else ""}" style="height:{3 * k}px"></i>' for k in range(1, 5)) + "</span>"


def row(chat, opt):
    name, _, status, share, _, _, days = chat
    right = f'<span class="tm">{TIME[name]}</span>'
    extra_l1, under, lead = "", "", ""
    if opt == "B1":
        right = f'<span class="rgt">{chip(share)}<span class="tm">{TIME[name]}</span></span>'
    elif opt == "B2":
        right = f'<span class="rgt">{spark(days, 2, 11)}<span class="tm">{TIME[name]}</span></span>'
    elif opt == "B3":
        lead = sig(share)
    elif opt == "B4":
        under = f'<span class="rowfill" style="width:{share / TOP * 100:.0f}%"></span>'
    elif opt == "now":
        under = f'<span class="tbar" style="width:calc((100% - 24px) * {share / TOP:.3f})"></span>'
    gap = lead if lead else '<span style="width:10px;flex:none"></span>'
    return (f'<div class="row two" style="padding-bottom:4px">{under}{gap}{MK[status]}<div class="body"><div class="l1">'
            f'<span class="nm">{name}</span><span class="tag">{BRANCH[name]}</span>{extra_l1}{right}</div>'
            f'<div class="t2">{LINE2[name]}</div></div></div>')


def rows_deck(theme, opt):
    rows = "".join(row(c, opt) for c in ROW_CHATS)
    return (f'<div class="{theme} deck" style="width:360px"><div class="tabs"><span class="tab on">Chats</span>'
            f'<span class="tab">News</span><span class="tab">Stats</span></div>{rows}<div style="height:6px"></div></div>')


REC = '<span class="rec">Recommended</span>'
CHOSEN = '<span class="rec">Chosen</span>'
KEPT = '<span class="rec" style="background:var(--text2)">Kept as a choice</span>'


def pair(title, dark, light, rec=""):
    return (f'<div style="margin-top:22px"><div class="optcap">{title} {rec}</div>'
            f'<div class="side"><div>{cap("Dark")}{dark}</div><div>{cap("Light")}{light}</div></div></div>')


A_OPTS = [
    ("A1. Bars split by model", a1,
     "Each bar is cut into its models, in the Models colours, with the token count beside the share. "
     "You see at once why a chat is big: many tokens, or an expensive model.", CHOSEN),
    ("A2. Tiles (a treemap)", a2,
     "One block, cut into tiles; the size of a tile is the chat's share. The biggest chats stand out at "
     "once, but small chats lose their names, and the narrow window leaves little room.", ""),
    ("A3. A ranked list with 7 days per chat", a3,
     "No long bars. Each chat gets its place, its share, and a small chart of its last 7 days, so you see "
     "when it was busy, not only how much.", ""),
    ("A4. A donut", a4,
     "The five biggest chats and \"Other chats\" as a ring with a legend. Good for the shape of the week; "
     "hard to compare two chats of near the same size.", ""),
]
B_OPTS = [
    ("B1. A share chip before the time", "B1",
     "A small rounded chip with the share, \"12%\", tinted stronger for bigger chats. The only option that shows "
     "a number without hovering. It takes the place at the right edge that ring option A of section 15 also wants.", ""),
    ("B2. A 7-day mini chart before the time", "B2",
     "Seven tiny columns, Monday to today. Shows when the chat worked; the size is only relative to the chat itself. "
     "Also takes room at the right edge.", ""),
    ("B3. Signal bars before the status mark", "B3",
     "Four small bars, like a Wi-Fi signal: 1 bar under 6%, 2 under 12%, 3 under 25%, 4 from 25%. "
     "Easy to scan down the list, uses the empty space at the left, and never clashes with the context ring.", KEPT),
    ("B4. A faint fill behind the row", "B4",
     "The row's background fills from the left, in grey at 9%, as far as its share. No extra item on the row; "
     "the grey keeps it apart from the amber selection colour.", KEPT),
]

def today_shares():
    tot = sum(c[6][-1] for c in CHATS)
    return {c[0]: round(c[6][-1] / tot * 100) for c in CHATS}


def line_deck(theme, mode):
    sh = today_shares() if mode == "today" else {c[0]: c[3] for c in CHATS}
    top = max(sh[c[0]] for c in ROW_CHATS) or 1
    rows = ""
    for c in ROW_CHATS:
        name, status = c[0], c[2]
        v = sh[name]
        when = "today" if mode == "today" else "the last 7 days"
        bar = (f'<span class="tbar" title="{v}% of {when}" '
               f'style="width:calc((100% - 24px) * {v / top:.3f})"></span>') if v else ""
        rows += (f'<div class="row two" style="padding-bottom:4px">{bar}<span style="width:10px;flex:none"></span>'
                 f'{MK[status]}<div class="body"><div class="l1"><span class="nm">{name}</span>'
                 f'<span class="tag">{BRANCH[name]}</span><span class="tm">{TIME[name]}</span></div>'
                 f'<div class="t2">{LINE2[name]}</div></div></div>')
    return (f'<div class="{theme} deck" style="width:360px"><div class="tabs"><span class="tab on">Chats</span>'
            f'<span class="tab">News</span><span class="tab">Stats</span></div>{rows}<div style="height:6px"></div></div>')


def qmenu(theme):
    ck = '<span class="mk2">✓</span>'
    no = '<span class="mk2"></span>'

    def mi(t, right, ind=False):
        pad = ' style="padding-left:24px"' if ind else ""
        return f'<div class="mi"{pad}><span>{t}</span>{right}</div>'

    def radio(t, on, ind=False):
        dot = ('<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true" style="flex:none">'
               '<circle cx="6" cy="6" r="5" fill="none" stroke="var(--text2)" stroke-width="1.2"/>'
               + ('<circle cx="6" cy="6" r="2.6" fill="var(--accent)"/>' if on else "") + "</svg>")
        pad = "padding-left:24px;" if ind else ""
        return (f'<div class="mi" style="{pad}justify-content:flex-start;gap:8px">{dot}<span>{t}</span></div>')
    return (f'<div class="{theme} menu" style="width:250px">'
            + mi("Job count", ck) + mi("Model name", no)
            + mi("Token mark", '<span class="mk2">✓ Win+Alt+Shift+T</span>')
            + radio("Line", True, True) + radio("Fill", False, True) + radio("Signal bars", False, True)
            + '<div class="msep"></div>'
            + radio("Count today", False) + radio("Count the last 7 days", True)
            + '<div class="msep"></div>' + mi("All settings…", no) + "</div>")


def dropdown(v):
    return ('<span style="display:inline-flex;align-items:center;gap:8px;padding:3px 10px;border:1px solid var(--border);'
            f'border-radius:5px;background:var(--raised);font-size:13px">{v}<svg width="10" height="10" viewBox="0 0 10 10">'
            '<path d="M2 3.5l3 3 3-3" fill="none" stroke="var(--text2)" stroke-width="1.4"/></svg></span>')


def settings(theme):
    return (f'<div class="{theme} settings" style="width:420px"><div class="shead">View</div>'
            '<div class="srow"><span>Model name</span><span class="sw"><span></span></span></div>'
            f'<div class="srow"><span>Token mark</span>{dropdown("Line")}</div>'
            '<div style="font-size:12px;color:var(--text2);margin:-2px 0 4px">Choices: Line, Fill, Signal bars, Off.</div>'
            f'<div class="srow"><span>Token mark counts</span>{dropdown("The last 7 days")}</div>'
            '<div style="font-size:12px;color:var(--text2);margin:-2px 0 4px">Choices: Today, The last 7 days.</div></div>')


DECIDED = (
    '<h3 style="margin-top:16px">Decided 2026-11-18</h3>'
    "<ul>"
    "<li><b>Stats tab:</b> A1. Each Top chats bar is cut by model, with the token count beside the share.</li>"
    "<li><b>Chats rows:</b> today's 2 px line stays the default. The setting <b>Token bar</b> becomes "
    "<b>Token mark</b> with four choices: <b>Line</b> (as now), <b>Fill</b> (B4), <b>Signal bars</b> (B3) "
    "and <b>Off</b>.</li>"
    "<li>A second setting, <b>Token mark counts</b>: <b>Today</b> or <b>The last 7 days</b> (the default). "
    "With Today the mark is the chat's share of today's tokens; a chat with none today shows no mark. "
    "The tooltip says which: “12% of today” or “12% of the last 7 days”.</li>"
    "<li>Hotkeys that work from any window (changed in 0.20.138: ClaudeDeck rarely has the keyboard, so "
    "Ctrl+B went to the Claude chat): <b>ClaudeDeck: Token mark on or off</b>, default <b>Win+Alt+Shift+T</b> "
    "(back to the style it had), and <b>ClaudeDeck: Token mark, today or 7 days</b>, with no default keys. "
    "Both are also palette commands. In the gear's <b>View</b> submenu the Token mark row shows the on/off "
    "hotkey's keys at the right (nothing when it has none); the grey note “{keys} switches the two.” "
    "shows under the Count lines only when the second hotkey has keys. Mini mode shows no mark.</li>"
    "<li>In the gear's <b>View</b> submenu the styles and the two Count lines are radio choices, with a dot "
    "on the one in use, as the Status mark choices are (changed in code for 0.20.137).</li>"
    "<li>Model colours: Opus accent, Sonnet success, Haiku warning. A second model of the same family gets the "
    "next free colour (info, then danger, then grey). The Models bar uses the same colours as the Top chats "
    "bars.</li></ul>"
    '<div class="side">'
    f'<div>{cap("Line, the last 7 days (the default)")}{line_deck("dark", "week")}</div>'
    f'<div>{cap("Line, counting today")}{line_deck("dark", "today")}</div>'
    '</div><div class="side" style="margin-top:20px">'
    f'<div>{cap("Quick settings (the gear) → View, dark")}{qmenu("dark")}</div>'
    f'<div>{cap("Quick settings → View, light")}{qmenu("light")}</div>'
    f'<div>{cap("Settings → Plugins → ClaudeDeck, under View")}{settings("dark")}</div>'
    "</div>"
)

section = (
    '<section><h2>16. Tokens per chat: the Stats tab and the rows (decided 2026-11-18)</h2>'
    "<p>Asked on 2026-11-18 in the WinCraft Rust chat: you do not like the look of the token views much. "
    "First the Stats tab and the row token bar as release 0.20.135 draws them, then four options for the graph "
    "of each chat on the Stats tab (A) and four options for the mark on each Chats row (B). Pick one A and one B. "
    "All shares weigh each token by its model's API price, as now; no dollars are shown.</p>"

    + DECIDED +
    '<h3 style="margin-top:28px">Now, in 0.20.135</h3>'
    "<p>The Stats tab: <b>Today</b> and <b>7 days</b>, the note line, then <b>Top chats</b> and <b>Groups</b> as "
    "6 px bars, <b>Days</b>, <b>Models</b> as one cut bar, and <b>Busiest hours</b>. On each Chats row, with "
    "<b>Token bar</b> on, a 2 px amber line at the bottom of the row as long as its share (the biggest chat "
    "fills it). The page shows 8 of the 10 top chats.</p>"
    '<div class="side">'
    f'<div>{cap("Stats tab, dark")}{deck("dark", current_stats())}</div>'
    f'<div>{cap("Stats tab, light")}{deck("light", current_stats())}</div>'
    '</div><div class="side" style="margin-top:20px">'
    f'<div>{cap("Chats rows with the token bar, dark")}{rows_deck("dark", "now")}</div>'
    f'<div>{cap("Chats rows with the token bar, light")}{rows_deck("light", "now")}</div></div>'

    '<h3 style="margin-top:28px">A. The graph of each chat on the Stats tab</h3>'
    "<p>Only <b>Top chats</b> changes; the other sections stay. You chose A1.</p>"
    + "".join(pair(t, deck("dark", f()), deck("light", f()), rec) .replace(
        '<div class="side">', f'<p style="margin:0 0 8px">{d}</p><div class="side">', 1) for t, f, d, rec in A_OPTS)
    + '<h3 style="margin-top:28px">B. The mark on each Chats row</h3>'
    "<p>Five chats of the same week. You kept today's line as the default, and B3 and B4 as choices "
    "of the new <b>Token mark</b> setting. B1 and B2 are not built.</p>"
    + "".join(pair(t, rows_deck("dark", o), rows_deck("light", o), rec).replace(
        '<div class="side">', f'<p style="margin:0 0 8px">{d}</p><div class="side">', 1) for t, o, d, rec in B_OPTS)
    + "</section>"
)

anchor = "<section><h2>Rules</h2>"
assert page.count(anchor) == 1
page = page.replace(anchor, section + anchor, 1)
open(dst, "w", encoding="utf-8", newline="").write(page)
print("written", dst, len(page.encode("utf-8")), "bytes")
