"""Add section 18, clearer groups and a foldable cold-chats area, to ClaudeDeck.dc.html.

Usage: python -I make_claudedeck_sections.py <in.html> <out.html>
Input is the v59 page. The section goes in before the "Rules" section.
"""
import re
import sys

src, dst = sys.argv[1], sys.argv[2]
page = open(src, encoding="utf-8").read()
assert "18. Groups and cold chats" not in page, "section 18 is already in the page"


def mark(needle):
    m = re.search(r'<svg class="mk"(?:(?!</svg>).)*' + re.escape(needle) + r'(?:(?!</svg>).)*</svg>', page)
    assert m, needle
    return m.group(0)


MK = {
    "working": mark('stroke="var(--info)" stroke-opacity=".3"'),
    "done": mark('fill="var(--ok)"/><path d="M4 7.2'),
    "asks": mark("M7 1.5a5.3"),
    "stopped": mark('rx="1.5" fill="var(--dis)"'),
    "cold": mark("M7 1.5v11M2.2 4.2l9.6 5.6"),
}

CSS = """
.gh2{height:30px;display:flex;align-items:center;gap:7px;padding:0 10px;font-size:14px;font-weight:600;color:var(--text)}
.gh2 .gc{margin-left:auto;font-size:12px;font-weight:400;color:var(--text2)}
.gh2.ln{border-top:1px solid var(--border);margin-top:8px}
.gh2.band{background:var(--panel);border-top:1px solid var(--border);border-bottom:1px solid var(--border);margin-top:6px}
.gh2.band.first{border-top:0;margin-top:0}
.shelf{margin-top:12px;border-top:2px solid var(--border);background:var(--bg);padding-bottom:4px}
.shelf .ch{height:30px;display:flex;align-items:center;gap:7px;padding:0 10px;font-size:13px;font-weight:600;color:var(--info)}
.shelf .ch .gc{margin-left:auto;font-size:12px;font-weight:400;color:var(--text2)}
.shelf .row .nm{color:var(--text2)}
.cdiv{display:flex;align-items:center;gap:8px;margin:12px 10px 2px;font-size:12px;color:var(--info);font-weight:600}
.cdiv::before,.cdiv::after{content:"";flex:1;border-top:1px solid var(--border)}
.cdiv .gc{font-weight:400;color:var(--text2)}
"""
page = page.replace("</style>", CSS + "</style>", 1)


def chev(open_, size=12):
    rot = "0" if open_ else "-90"
    return (f'<svg width="{size}" height="{size}" viewBox="0 0 10 10" style="transform:rotate({rot}deg);flex:none" '
            'aria-hidden="true"><path d="M2 3.5l3 3 3-3" fill="none" stroke="var(--text2)" stroke-width="1.4"/></svg>')


TREE = ('<span class="ind tree" style="width:14px"><span class="vl" style="left:6px;top:0;bottom:calc(100% - 11px)"></span>'
        '<span class="hl" style="left:6px;top:11px;width:7px"></span></span>')

# group name, [(name, branch, status, time, last line, fork)]
GROUPS = [
    ("F1os-Agentic Systems", [
        ("F1os Fable Session", "f1-code-review", "working", "3 min", "Last: bringing the home1 work back to the PC", False),
        ("F1OS design", "vibrant-gates", "done", "1 day", "Last: I fixed the task list in docs/my-tasks.md", True),
        ("My tasks - Temp Session", "zen-turing", "stopped", "5 h", "Last: task 109, a hand count of one campaign", True),
    ]),
    ("verdict_ai_code", [
        ("VerdictAI - UA Legal AI platform", "main", "working", "4 days", "Runs 4 jobs on home1", False),
        ("VerdictAI - Design Chat", "main", "done", "1 day", "Last: the design pages are up to date", True),
    ]),
    ("Other Sessions", [
        ("NewsroomCMS", "folder-move", "working", "3 h", "Thinking", False),
        ("WinCraft design", "design", "asks", "2 min", "Which option should the groups use?", False),
    ]),
]
COLD = [
    ("AgentSKU design", "main", "1 day", "Last: the product photos need no new work"),
    ("Projects Administration", "main", "3 days", "Last: all 4 domains are active on Cloudflare"),
    ("Agentic Shop Test", "main", "3 days", "Last: stock and prices in the apple feed"),
]


def row(name, branch, status, tm, last, fork=False):
    lead = (TREE if fork else "") + '<span style="width:10px;flex:none"></span>' + MK[status]
    return (f'<div class="row two">{lead}<div class="body" style="min-width:0;flex:1"><div class="l1">'
            f'<span class="nm">{name}</span><span class="tag">{branch}</span><span class="tm">{tm}</span></div>'
            f'<div class="t2">{last}</div></div></div>')


def groups(style):
    out = []
    for i, (gname, chats) in enumerate(GROUPS):
        if style == "A":
            cls = "gh2" + ("" if i == 0 else " ln")
        else:
            cls = "gh2 band" + (" first" if i == 0 else "")
        out.append(f'<div class="{cls}">{chev(True)}<span>{gname}</span><span class="gc">{len(chats)}</span></div>')
        out += [row(*c) for c in chats]
    return "".join(out)


def cold(style, open_):
    count = f"{len(COLD)} chats · 1 – 3 days"
    rows = "".join(row(n, b, "cold", t, l) for n, b, t, l in COLD) if open_ else ""
    if style == "A":
        return (f'<div class="shelf"><div class="ch">{chev(open_)}{MK["cold"]}<span>Cold chats</span>'
                f'<span class="gc">{count}</span></div>{rows}</div>')
    return f'<div class="cdiv">{chev(open_)}<span>Cold chats</span><span class="gc">{count}</span></div>{rows}'


def deck(theme, gstyle, cstyle, open_, width=380):
    return (f'<div class="{theme} deck" style="width:{width}px"><div class="tabs"><span class="tab on">Chats</span>'
            f'<span class="tab">News</span><span class="tab">Servers</span><span class="tab">Stats</span></div>'
            f'<div class="list">{groups(gstyle)}{cold(cstyle, open_)}</div></div>')


def cap(text):
    return f'<div class="cap">{text}</div>'


REC = '<span class="rec">Recommended</span>'

section = (
    '<section><h2>18. Groups and cold chats: clearer sections (options for review 2026-11-26)</h2>'
    "<p>Asked on 2026-11-26: (1) set the cold chats clearly apart from the active chats, (2) make the cold chats "
    "foldable, (3) a clear line between groups, with bigger group names. Today a group header is 12 px grey text with "
    "only 4 px of space above it, so groups run into each other, and the cold chats start after one dashed line that "
    "is easy to miss. The same chats in every drawing: three groups from the real list and three cold chats.</p>"

    + f'<h3 style="margin-top:24px">Option A: a line between groups and a cold shelf {REC}</h3>'
    "<p><b>Groups:</b> the name is 14 px, semibold, in normal text colour (rows stay 13 px, so the name is the biggest "
    "text in the list). The header is 30 px high. Above every group but the first: 8 px of space and a full-width 1 px "
    "line. The count stays small and grey at the right.</p>"
    "<p><b>Cold chats:</b> after the last group, 12 px of space and a 2 px line, then a darker area (the window's "
    "background colour) that holds the header and the cold rows. The header has the fold arrow, the snowflake mark, "
    "<b>Cold chats</b> in the blue of the cold status, and at the right the count and the age range. Cold chat names "
    "are grey, so the eye goes to the active chats first.</p>"
    '<div class="side">'
    f'<div>{cap("Dark, cold chats open")}{deck("dark", "A", "A", True)}</div>'
    f'<div>{cap("Light, cold chats open")}{deck("light", "A", "A", True)}</div>'
    f'<div>{cap("Dark, cold chats folded")}{deck("dark", "A", "A", False)}</div>'
    "</div>"

    + '<h3 style="margin-top:28px">Option B: header bands and a cold divider</h3>'
    "<p><b>Groups:</b> the same 14 px name, on a full-width band in the panel colour, with a line above and below. "
    "Stronger than A, but the bands are heavy when there are many small groups.</p>"
    "<p><b>Cold chats:</b> one centred divider, “── Cold chats · 3 chats ──”, with the fold arrow. Lighter than the shelf "
    "of A: the cold rows look like the other rows, so the split is clear only at the divider.</p>"
    '<div class="side">'
    f'<div>{cap("Dark, cold chats open")}{deck("dark", "B", "B", True)}</div>'
    f'<div>{cap("Light, cold chats folded")}{deck("light", "B", "B", False)}</div>'
    "</div>"

    + '<h3 style="margin-top:28px">Folding the cold chats (the same for both options)</h3><ul>'
    "<li>A click on the cold header folds or opens it, like a group. The arrow points down when open and right when "
    "folded.</li>"
    "<li>Keyboard, as on a group header: <b>←</b> folds, <b>→</b> opens, <b>Space</b> or <b>Enter</b> does either.</li>"
    "<li>Folded or open is remembered after a restart. The first time it is folded, as today.</li>"
    "<li>Folded, the header still shows the count and the age range, so nothing is lost.</li>"
    "<li>Dragging a cold chat onto a group still works and puts it there for a day, as today.</li>"
    "<li>The setting <b>Fold chats idle for over 1 day</b> keeps its job: off, there is no cold area at all.</li>"
    "<li>Hidden chats keep their own line after the cold area. Mini mode does not change: it leaves cold chats out.</li>"
    "<li>The Servers tab uses the same cold header in each server card, so the look is one family.</li></ul>"

    + '<h3 style="margin-top:28px">My recommendation</h3>'
    "<p>Option A. The line and the bigger name split the groups without making the list taller by much (about 10 px "
    "per group). The darker shelf makes the cold chats a separate place you can fold away, so the top of the list is "
    "only the chats that are alive.</p></section>"
)

anchor = "<section><h2>Rules</h2>"
assert page.count(anchor) == 1
page = page.replace(anchor, section + anchor, 1)
open(dst, "w", encoding="utf-8", newline="").write(page)
print("written", dst, len(page.encode("utf-8")), "bytes")
