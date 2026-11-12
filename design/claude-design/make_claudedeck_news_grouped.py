"""Adds section 14 (News grouped by chat, for approval) to ClaudeDeck.dc.html.

Usage: make_claudedeck_news_grouped.py <in.html> <out.html>
Needs the page with section 13 (its CSS classes .tabs, .nitem, .ndot, .tabn are reused).
"""
import re
import sys

src, dst = sys.argv[1], sys.argv[2]
s = open(src, encoding="utf-8").read()
assert "<h2>13." in s and "<h2>14." not in s

CSS = (
    ".ngrp{padding:5px 0 6px}"
    ".ngrp+.ngrp{border-top:1px solid var(--hover)}"
    ".nghead{display:flex;align-items:center;gap:7px;padding:0 10px;height:24px;white-space:nowrap}"
    ".nghead .nm{font-weight:600;overflow:hidden;text-overflow:ellipsis}"
    ".nghead .tag{max-width:130px;overflow:hidden;text-overflow:ellipsis}"
    ".nghead .tabn{margin-left:auto}"
    ".nrep{display:flex;gap:7px;padding:3px 10px 3px 27px;align-items:flex-start}"
    ".nrep:hover{background:var(--hover)}"
    ".nrep .nlead{width:8px;padding-top:2px}"
    ".nrep .ndot{margin-top:2px}"
    ".nrep .ntext{flex:1;min-width:0;margin-top:0;-webkit-line-clamp:2}"
    ".nrep .tm{flex:none;margin-left:6px;font-size:12px;line-height:16px;color:var(--text2)}"
    ".nrep.unseen .ntext{color:var(--text)}"
    ".nrep.sel{background:var(--sel);box-shadow:inset 0 0 0 1px var(--accent)}"
    ".nmore{display:block;padding:2px 10px 0 42px;font-size:12px;color:var(--accent)}"
    ".ngrp.kf .nghead{box-shadow:inset 0 0 0 1px var(--accent);border-radius:4px;background:var(--sel)}"
)
s = s.replace("</style>", CSS + "</style>", 1)

i1 = s.find("<h2>1.")
HDR = re.compile(r'<div class="hdr">.*?</button></div>', re.S).search(s, i1).group(0)
APPROVAL, ASK, WORKING, DONE = re.findall(r'<svg class="mk".*?</svg>', HDR, re.S)[:4]
j = s.find(">1 day<", s.find("<h2>11."))
IDLE = re.search(r'<svg class="mk".*?</svg>', s[s.rfind('<div class="row', 0, j):j], re.S).group(0)
SRV = ('<svg width="12" height="12" viewBox="0 0 14 14" aria-hidden="true" style="flex:none"><rect x="2" y="1.5" width="10" height="4.6" '
       'rx="1.2" fill="none" stroke="var(--text2)" stroke-width="1.3"/><rect x="2" y="7.9" width="10" height="4.6" rx="1.2" fill="none" '
       'stroke="var(--text2)" stroke-width="1.3"/><path d="M4.3 3.8h1.6M4.3 10.2h1.6" stroke="var(--text2)" stroke-width="1.3" '
       'stroke-linecap="round"/></svg>')


def chev(open_=True):
    rot = "0" if open_ else "-90"
    return (f'<svg width="10" height="10" viewBox="0 0 10 10" style="transform:rotate({rot}deg);flex:none" aria-hidden="true">'
            f'<path d="M2 3.5l3 3 3-3" fill="none" stroke="var(--text2)" stroke-width="1.4"/></svg>')


def tabs(count):
    pill = f'<span class="tabn">{count}</span>' if count else ""
    return f'<div class="tabs"><span class="tab">Chats</span><span class="tab on">News{pill}</span></div>'


def deck(theme, body, count=6, width=360):
    return (f'<div class="{theme} deck" style="width:{width}px">{HDR}{tabs(count)}'
            f'<div class="list" style="padding-top:0">{body}</div></div>')


def head(text, n):
    return (f'<div class="ghead">{chev()}<span class="gname">{text}</span><span class="gcount">{n}</span></div>')


def rep(tm, text, unseen, sel=False):
    dot = '<span class="ndot" title="Not seen yet"></span>' if unseen else ""
    c = (" unseen" if unseen else "") + (" sel" if sel else "")
    return (f'<div class="nrep{c}"><span class="nlead">{dot}</span><div class="ntext">{text}</div>'
            f'<span class="tm">{tm}</span></div>')


def group(chat, mode="all", sel=None, kf=False):
    """mode: "all" lists every reply; "fold" lists the unseen ones (or the newest) and folds the rest."""
    name, proj, mk, server, reps = chat
    unseen = sum(1 for r in reps if r[2])
    if mode == "all":
        shown, rest = reps, []
    else:
        shown = [r for r in reps if r[2]] or reps[:1]
        rest = [r for r in reps if r not in shown]
    open_ = mode == "all" or not rest
    srv = f'<span class="nsrv">{SRV}{server}</span>' if server else ""
    pill = f'<span class="tabn" title="{unseen} not seen yet">{unseen}</span>' if unseen else ""
    more = (f'<span class="nmore">{len(rest)} earlier {"reply" if len(rest) == 1 else "replies"}</span>' if rest else "")
    body = "".join(rep(*r, sel=(sel == k)) for k, r in enumerate(shown))
    return (f'<div class="ngrp{" kf" if kf else ""}"><div class="nghead">{chev(open_)}{mk}<span class="nm">{name}</span>'
            f'<span class="tag">{proj}</span>{srv}{pill}</div>{body}{more}</div>')


WAITING = [
    ("ClaudeDeck plugin", "wincraft", APPROVAL, None, [
        ("2 min", "Waits for your OK to run a command.", True),
    ]),
    ("Verdict - AI", "verdict_ai_code", ASK, None, [
        ("6 min", "Which court list should the search use first: the 2025 list or the full archive?", True),
        ("12 min", "The customs reader passed its test run. The dry run of the customs queue is still going.", True),
        ("26 min", "The hand-check tests still run in the merge worktree, so the merge waits for them. I will merge and deploy "
                   "both together.", False),
        ("56 min", "Closed 27: the statistics key is set on the newsHQ Worker. The newsHQ chat checks that its drafts get real numbers.",
         False),
    ]),
]
LAST = [
    ("WinCraft homelab", "wincraft_windows_utilities_rust", IDLE, None, [
        ("24 s", "Task 30 is closed. Task 31 still waits for your test.", True),
        ("43 min", "WinCraft 0.20.115 is released, with the News tab and the fix for the hotkey on a minimized window. It passed the "
                   "full local check and the GitHub build.", False),
        ("49 min", "Task 31 is written. I wait for the GitHub build of 0.20.115 before I mark it ready.", False),
    ]),
    ("March import", "oriondb", IDLE, "home1", [
        ("38 min", "The March import finished: 41 210 rows, 12 skipped for a missing date. Should I retry them with the file date?",
         True),
    ]),
    ("Law news first run", "_newsHQ", WORKING, None, [
        ("56 min", "The statistics key works now. I made one live request from home2, the same way the drafting agent makes it.",
         True),
        ("2 h", "The drafts could not get statistics: Verdict AI refused the old key.", False),
    ]),
    ("Playwise - Kids Learn", "playwise", IDLE, None, [
        ("2 h", "The new lesson screen works at phone size. The sound button is too small at 360 px.", False),
        ("5 h", "The parent page needs the new colours. I made a list of the 9 places.", False),
    ]),
    ("Deploy check", "verdict-ai", IDLE, "home2", [
        ("7 h", "The deploy on home2 is done and the health page answers. The search index is rebuilt.", True),
    ]),
]


def older_head(n):
    return f'<div class="ghead">{chev(False)}<span class="gname">Older</span><span class="gcount">{n}</span></div>'


def body(mode, sel_chat=None, sel=None, kf_chat=None, waiting=True):
    out = ""
    if waiting:
        out += head("Waits for you", len(WAITING))
        out += "".join(group(c, mode, sel if k == sel_chat else None, k == kf_chat) for k, c in enumerate(WAITING))
    out += head("Last 24 hours", len(LAST))
    off = len(WAITING) if waiting else 0
    out += "".join(group(c, mode, sel if k + off == sel_chat else None, k + off == kf_chat) for k, c in enumerate(LAST))
    return out + older_head(4)


UNSEEN = sum(1 for c in WAITING + LAST for r in c[4] if r[2])
REC = ' <span class="rec">Recommended</span>'


def cap(text, chip=""):
    return f'<div class="cap">{text}{chip}</div>'


one_open = list(LAST[0])
section = (
    '<section><h2>14. News grouped by chat (for approval)</h2>'
    '<p>Asked on 2026-11-13, after a look at 0.20.115: in the first News tab one busy chat fills the list. The same name and the same '
    'long folder name repeat on every reply, five times in a row for one chat, and each reply takes three lines. Here every chat '
    'gets one block. Its name, folder, server and status mark show once, on the block line. Under it, each reply is one or two lines '
    'of text with its time at the right. Blocks are ordered by their newest reply; a chat that waits for you stays pinned at the top. '
    'The amber count on the block line is the number of unseen replies in it.</p>'
    '<div class="side">'
    f'<div>{cap("Option A: every reply listed under its chat")}{deck("dark", body("all"), UNSEEN)}</div>'
    f'<div>{cap("Option B: unseen replies listed, seen ones folded", REC)}{deck("dark", body("fold"), UNSEEN)}</div>'
    '</div>'
    '<p style="margin-top:14px">Option A shows everything, so the list is still long on a busy evening. Option B shows only what you '
    'have not read: every unseen reply of a chat, or its newest reply when all are seen, and one line "N earlier replies" for the '
    'rest. A click on that line, or on the arrow of the block line, unfolds the chat; ClaudeDeck remembers it per chat until the '
    'next new reply. Option B is recommended: the list stays short, and nothing unread is ever folded away.</p>'
    '<h3 style="margin-top:22px">Option B, unfolded and in light</h3>'
    '<p>"WinCraft homelab" unfolded shows all three replies. A click on a reply opens it in place, as in section 13 '
    '(the whole text, then "Open the chat"); a click on the block line opens the chat in the Claude app. The Older group stays as '
    'drawn in section 13: one block per chat, its newest older reply.</p>'
    '<div class="side">'
    f'<div>{cap("A chat unfolded")}{deck("dark", head("Last 24 hours", len(LAST)) + group(LAST[0], "all") + "".join(group(c, "fold") for c in LAST[1:]) + older_head(4), UNSEEN)}</div>'
    f'<div>{cap("Light")}{deck("light", body("fold"), UNSEEN)}</div>'
    '</div>'
    '<h3 style="margin-top:22px">The keyboard</h3>'
    '<p>↑ ↓ move over block lines and replies. On a block line, → unfolds the chat and ← folds it; Enter opens the chat. On a reply, '
    'Space opens it in place and Enter opens the chat. Home and End, Ctrl+Tab and Esc work as in section 13.</p>'
    '<div class="side">'
    f'<div>{cap("The focus on a block line")}{deck("dark", body("fold", kf_chat=3, waiting=False), UNSEEN)}</div>'
    f'<div>{cap("The focus on a reply")}{deck("dark", body("fold", sel_chat=1, sel=1), UNSEEN)}</div>'
    '</div>'
    '</section>'
)

anchor = "<section><h2>Rules</h2>"
assert s.count(anchor) == 1
s = s.replace(anchor, section + anchor, 1)
open(dst, "w", encoding="utf-8", newline="\n").write(s)
print("ok", len(s), len(section), "unseen", UNSEEN)
