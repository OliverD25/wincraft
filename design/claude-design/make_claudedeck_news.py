"""Adds section 13 (the News tab) to ClaudeDeck.dc.html. Usage: make_news_section.py <in.html> <out.html>"""
import re
import sys

src, dst = sys.argv[1], sys.argv[2]
s = open(src, encoding="utf-8").read()
assert "<h2>13." not in s

CSS = (
    ".tabs{display:flex;gap:2px;padding:0 6px;height:30px;align-items:flex-end;border-bottom:1px solid var(--border);background:var(--win)}"
    ".tab{height:29px;padding:0 10px;display:inline-flex;align-items:center;gap:6px;font-size:13px;color:var(--text2);border-bottom:2px solid transparent}"
    ".tab.on{color:var(--text);font-weight:600;border-bottom-color:var(--accent)}"
    ".tab.kf{box-shadow:inset 0 0 0 1px var(--accent);border-radius:4px 4px 0 0}"
    ".tabn{min-width:18px;height:16px;padding:0 5px;border-radius:8px;background:var(--accent);color:var(--win);font-size:12px;font-weight:700;line-height:16px;text-align:center}"
    ".nitem{display:flex;gap:7px;padding:6px 10px 7px 10px;position:relative}"
    ".nitem+.nitem{border-top:1px solid var(--hover)}"
    ".nitem:hover{background:var(--hover)}"
    ".nlead{width:14px;flex:none;display:flex;justify-content:center;padding-top:1px}"
    ".ndot{width:8px;height:8px;border-radius:50%;background:var(--accent);margin-top:3px;flex:none}"
    ".ntext{font-size:12px;line-height:16px;color:var(--text2);display:-webkit-box;-webkit-line-clamp:3;-webkit-box-orient:vertical;overflow:hidden;margin-top:3px;white-space:normal}"
    ".nitem.unseen .nm{font-weight:600}.nitem.unseen .ntext{color:var(--text)}"
    ".nitem.sel{background:var(--sel);box-shadow:inset 0 0 0 1px var(--accent)}"
    ".nitem.open{background:var(--sel)}"
    ".nitem.open .ntext{display:block;color:var(--text)}"
    ".ntext p{margin:0 0 6px;font-size:12px;line-height:16px;color:inherit;max-width:none}"
    ".ncut{display:-webkit-box!important;-webkit-line-clamp:11;-webkit-box-orient:vertical;overflow:hidden}"
    ".nscroll{max-height:176px;overflow-y:auto;padding-right:6px}"
    ".nact{display:flex;gap:10px;align-items:center;margin-top:8px}"
    ".nbtn{height:24px;padding:0 10px;border:0;border-radius:4px;background:var(--accent);color:var(--win);font:600 12px 'Segoe UI',system-ui,sans-serif}"
    ".nlink{font-size:12px;color:var(--accent)}"
    ".nsrv{display:inline-flex;align-items:center;gap:3px;font-size:12px;color:var(--text2);white-space:nowrap}"
    ".nitem.card{margin:6px 8px 0;border:1px solid var(--border);border-radius:6px;background:var(--panel);padding:7px 10px 8px}"
    ".nitem.card+.nitem.card{border-top:1px solid var(--border)}"
    ".nitem.card.unseen{border-left:3px solid var(--accent);padding-left:8px}"
    ".prop{padding:1px 8px;border-radius:10px;border:1px solid #e1ad66;color:#e1ad66;font-size:12px;font-weight:600}"
)
s = s.replace("</style>", CSS + "</style>", 1)


def first(pattern, start=0):
    m = re.compile(pattern, re.S).search(s, start)
    assert m, pattern
    return m.group(0)


i1 = s.find("<h2>1.")
HDR = first(r'<div class="hdr">.*?</button></div>', i1)
marks = re.findall(r'<svg class="mk".*?</svg>', HDR, re.S)
APPROVAL, ASK, WORKING, DONE = marks[:4]
j = s.find(">1 day<", s.find("<h2>11."))
v0 = s.rfind('<div class="row', 0, j)
IDLE = re.search(r'<svg class="mk".*?</svg>', s[v0:j], re.S).group(0)
SRV = ('<svg width="12" height="12" viewBox="0 0 14 14" aria-hidden="true" style="flex:none"><rect x="2" y="1.5" width="10" height="4.6" '
       'rx="1.2" fill="none" stroke="var(--text2)" stroke-width="1.3"/><rect x="2" y="7.9" width="10" height="4.6" rx="1.2" fill="none" '
       'stroke="var(--text2)" stroke-width="1.3"/><path d="M4.3 3.8h1.6M4.3 10.2h1.6" stroke="var(--text2)" stroke-width="1.3" '
       'stroke-linecap="round"/></svg>')
CHEV = ('<svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M2 3.5l3 3 3-3" fill="none" stroke="var(--text2)" '
        'stroke-width="1.4"/></svg>')


def tabs(on="news", count=4, kf=False):
    pill = f'<span class="tabn">{count}</span>' if count else ""
    c = ' on' if on == "chats" else ''
    n = ' on' if on == "news" else ''
    k = ' kf' if kf else ''
    return (f'<div class="tabs"><span class="tab{c}">Chats</span>'
            f'<span class="tab{n}{k}">News{pill}</span></div>')


def deck(theme, body, on="news", count=4, width=360, kf=False):
    return (f'<div class="{theme} deck" style="width:{width}px">{HDR}{tabs(on, count, kf)}'
            f'<div class="list" style="padding-top:0">{body}</div></div>')


def head(text, n=None, right=""):
    cnt = f'<span class="gcount">{n}</span>' if n is not None and not right else ""
    return (f'<div class="ghead">{CHEV}<span class="gname">{text}</span>{cnt}{right}</div>')


def item(name, proj, tm, text, unseen=False, mk=None, server=None, cls="", after=""):
    lead = mk if mk else ('<span class="ndot" title="Not seen yet"></span>' if unseen else "")
    srv = f'<span class="nsrv">{SRV}{server}</span>' if server else ""
    c = " unseen" if unseen else ""
    return (f'<div class="nitem{c} {cls}"><span class="nlead">{lead}</span><div class="body"><div class="l1">'
            f'<span class="nm">{name}</span><span class="tag">{proj}</span>{srv}<span class="tm">{tm}</span></div>'
            f'<div class="ntext">{text}</div>{after}</div></div>')


WAIT = [
    ("ClaudeDeck plugin", "wincraft", "2 min", "Waits for your OK to run a command.", APPROVAL, None),
    ("Verdict - AI", "verdict-ai", "6 min", "Which court list should the search use first: the 2025 list or the full archive?", ASK, None),
]
NEWS = [
    ("WinCraft homelab", "wincraft", "4 min", "Released 0.20.114. ClaudeDeck opens faster, Archive is gone, and resumed agents show as Background. "
     "Ready to test: open ClaudeDeck with Win+Alt+X and check the server groups.", True, None),
    ("March import", "oriondb", "38 min", "The March import finished: 41 210 rows, 12 skipped for a missing date. The skipped rows are in "
     "_io/skipped-march.csv. Should I retry them with the file date?", True, "home1"),
    ("Playwise - Kids Learn", "playwise", "2 h", "The new lesson screen works at phone size. Two things are left: the sound button is too "
     "small at 360 px, and the parent page needs the new colours.", True, None),
    ("WinCraft homelab", "wincraft", "3 h", "The server probe now reads CPU and RAM every 30 s over one ssh connection. It stays quiet "
     "when a server does not answer.", False, None),
    ("Deploy check", "verdict-ai", "7 h", "The deploy on home2 is done and the health page answers. The search index is rebuilt; it "
     "took 14 minutes.", True, "home2"),
    ("Luminart site", "luminart.app", "19 h", "Published the new gallery page. The old links still work through redirects.", False, None),
]


def news_body(card=False, wait=True, n=None):
    cls = "card" if card else ""
    out = ""
    if wait:
        out += head("Waits for you", len(WAIT))
        out += "".join(item(a, b, c, d, True, mk, sv, cls) for a, b, c, d, mk, sv in WAIT)
    rows = NEWS if n is None else NEWS[:n]
    out += head("Last 24 hours", len(rows))
    out += "".join(item(a, b, c, d, u, None, sv, cls) for a, b, c, d, u, sv in rows)
    if n is None:
        out += older_head(4, False)
    return out


def row(mk, name, tag, tm, line, dot=False):
    d = '<span class="ndot" title="Unseen news" style="margin-top:0"></span>' if dot else ""
    return (f'<div class="row two"><span style="width:10px;flex:none"></span>{mk}<div class="body"><div class="l1">'
            f'<span class="nm">{name}</span><span class="tag">{tag}</span>{d}<span class="tm">{tm}</span></div>'
            f'<div class="t2">{line}</div></div></div>')


def ghead(name, n, dot="var(--accent)", server=False):
    lead = (SRV.replace('width="12" height="12"', 'width="14" height="14"') if server
            else f'<span class="gdot" style="background:{dot}"></span>')
    ok = '<span style="width:7px;height:7px;border-radius:50%;background:var(--ok);flex:none"></span>' if server else ""
    return f'<div class="ghead">{CHEV}{lead}<span class="gname">{name}</span>{ok}<span class="gcount">{n}</span></div>'


CHATS = (ghead("WinCraft", 2)
         + row(APPROVAL, "ClaudeDeck plugin", "claudedeck", "2 min", "Waits for your OK to run a command")
         + row(IDLE, "WinCraft homelab", "main", "4 min", "Last: released 0.20.114", dot=True)
         + ghead("Other", 2, "var(--info)")
         + row(ASK, "Verdict - AI", "main", "6 min", "Asks you: which court list first")
         + row(IDLE, "Playwise - Kids Learn", "main", "2 h", "Last: the lesson screen works at phone size", dot=True)
         + ghead("home1", 1, server=True)
         + row(IDLE, "March import", "main", "38 min", "Last: the March import finished", dot=True))

LONG = ("<p>Released 0.20.114. Three changes in ClaudeDeck:</p>"
        "<p>1. It opens faster. The list is drawn from the last known state first, and the servers are asked after that, so the "
        "window shows at once even when home2 is slow.</p>"
        "<p>2. The Archive item is gone from the chat menu. Archived chats were never shown, so the item only confused.</p>"
        "<p>3. A resumed agent now shows as Background, not as Working, because the chat itself waits for you.</p>"
        "<p>Ready to test now: open ClaudeDeck with Win+Alt+X. Check that the home1 and home2 groups show within two seconds, and that "
        "the chat menu has no Archive item. Task 23 in docs/my-tasks.md has the full steps.</p>"
        "<p>Not done: the News tab. It waits for the design to be approved.</p>")
ACT = ('<div class="nact"><button type="button" class="nbtn">Open the chat</button><span class="mk2">or Enter, or a double-click</span></div>')


def open_item(text, extra_cls="", more=""):
    body = (f'<div class="nitem open"><span class="nlead"></span><div class="body"><div class="l1"><span class="nm">WinCraft homelab</span>'
            f'<span class="tag">wincraft</span><span class="tm">4 min</span></div><div class="ntext {extra_cls}">{text}</div>{more}{ACT}</div></div>')
    return body


SHORT = "<p>The deploy on home2 is done and the health page answers. The search index is rebuilt; it took 14 minutes.</p>"
open_short = ('<div class="nitem open"><span class="nlead"></span><div class="body"><div class="l1"><span class="nm">Deploy check</span>'
              f'<span class="tag">verdict-ai</span><span class="nsrv">{SRV}home2</span><span class="tm">7 h</span></div>'
              f'<div class="ntext">{SHORT}</div>{ACT}</div></div>')


def plain(body):
    return f'<div class="list" style="padding:0">{body}</div>'


def frame(theme, inner, width=360):
    return f'<div class="{theme} deck" style="width:{width}px">{inner}</div>'


def cap(text, *chips):
    return f'<div class="cap">{text}{"".join(chips)}</div>'


REC = ' <span class="rec">Chosen</span>'
PROP = REC
OLDER = [
    ("Luminart site", "luminart.app", "1 day", "Published the redirects for the old gallery links. All 38 old links answer.", False, None),
    ("Backup check", "homelab", "2 days", "The nightly backup to the NAS worked for 7 days in a row. The oldest copy is 30 days old.", True, "home2"),
    ("Playwise - Kids Learn", "playwise", "3 days", "The parent page is moved to the new colours. The sound button size waits for your pick.", False, None),
    ("Verdict - AI", "verdict-ai", "5 days", "The court list import is done for 2024. 2025 needs the new export from the court site.", False, None),
]
def older_head(n, open_):
    rot = "0" if open_ else "-90"
    return (f'<div class="ghead"><svg width="10" height="10" viewBox="0 0 10 10" style="transform:rotate({rot}deg)" aria-hidden="true">'
            f'<path d="M2 3.5l3 3 3-3" fill="none" stroke="var(--text2)" stroke-width="1.4"/></svg><span class="gname">Older</span>'
            f'<span class="gcount">{n}</span></div>')
OLDER_OPEN = (head("Last 24 hours", 2) + "".join(item(a, b, c, d, u, None, sv) for a, b, c, d, u, sv in NEWS[4:6])
              + older_head(len(OLDER), True) + "".join(item(a, b, c, d, u, None, sv) for a, b, c, d, u, sv in OLDER))

MINI = ('<div class="dark mini"><div title="Full list" style="height:18px;display:flex;align-items:center;justify-content:center">'
        '<svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M4.5 2v8M7.5 4l2 2-2 2" fill="none" '
        'stroke="var(--text2)" stroke-width="1.2"/></svg></div>'
        '<span class="tabn" title="4 unseen replies: a click opens the News tab">4</span><span class="sepm"></span>'
        + APPROVAL + ASK + IDLE + IDLE + "</div>")

TRAY = ('<div class="tray"><svg width="20" height="20" viewBox="0 0 20 20"><rect x="2" y="2" width="16" height="16" rx="4" fill="#e1ad66"/>'
        '</svg><span style="color:#bab6b6;font-size:12px">WinCraft — 2 chats wait for you, 4 new replies</span></div>')

EMPTY = deck("dark", '<div class="empty">No replies in the last 24 hours.<br>Each reply a chat finishes shows here.</div>', count=0)

kbd_body = (head("Last 24 hours", 3)
            + item(*NEWS[0][:4], True)
            + item(*NEWS[1][:4], True, None, "home1", "sel")
            + item(*NEWS[2][:4], True))

prop_right = ('<span class="sp"></span><span class="sw" title="Only unseen"><span></span></span><span class="swl" style="font-weight:400">'
              'Only unseen</span><span class="nlink" style="font-weight:400;margin-left:8px">Mark all seen</span>')
prop_body = (f'<div class="ghead">{CHEV}<span class="gname">Last 24 hours</span>{prop_right}</div>'
             + "".join(item(a, b, c, d, u, None, sv) for a, b, c, d, u, sv in NEWS[:3]))

section = (
    '<section><h2>13. News: the latest replies of every chat (approved 2026-11-12)</h2>'
    '<p>With many chats at once, a chat can go Idle while its last reply matters and nobody read it. The News tab sits next to the chat '
    'list and shows every reply a chat finished in the last 24 hours, newest first, several per chat. Chats that wait for your OK or ask '
    'you a question are pinned at the top. Server chats are in it too, with the server icon and name. Each item shows the chat, its '
    'project, the time and the first three lines of the reply.</p>'
    '<p>A reply is unseen when the chat finished it after you last opened that chat in the Claude app. Opening the chat makes it seen, and '
    'so does opening the item here. An unseen item has an amber dot, a bold name and full-strength text; a seen one is calmer.</p>'
    '<div class="side">'
    f'<div>{cap("The News tab, dark")}{deck("dark", news_body())}</div>'
    f'<div>{cap("The News tab, light")}{deck("light", news_body())}</div>'
    '</div>'
    '<h3 style="margin-top:22px">The look of an item: two options</h3>'
    '<p>Chosen on 2026-11-12: option 1. Option 1 keeps the list flat, like the chat rows, with a thin line between items; it fits the most replies. Option 2 puts each '
    'reply on its own card with an amber edge while unseen; it separates replies more but fits about a third fewer.</p>'
    '<div class="side">'
    f'<div>{cap("Option 1: rows", REC)}{deck("dark", news_body(wait=False, n=4))}</div>'
    f'<div>{cap("Option 2: cards")}{deck("dark", news_body(card=True, wait=False, n=4) + "<div style=height:6px></div>")}</div>'
    '</div>'
    '<h3 style="margin-top:22px">Signals while you are on the Chats tab</h3>'
    '<p>The News tab shows how many replies are unseen. Each chat row with an unseen reply gets the same amber dot after its name. The tray '
    'tooltip counts unseen replies too. In mini mode the list stays Chats-only; the unseen count sits under the Full list button, and a '
    'click on it opens the full window on the News tab.</p>'
    '<div class="side">'
    f'<div>{cap("The Chats tab: the count on News, a dot on rows")}{deck("dark", CHATS, on="chats")}</div>'
    f'<div>{cap("Tray icon: the tooltip")}{TRAY}'
    f'<div class="cap" style="margin-top:22px">Mini mode: the count under Full list{PROP}</div>{MINI}</div>'
    '</div>'
    '<h3 style="margin-top:22px">Open an item</h3>'
    '<p>The first click opens the item in place with the whole reply and makes it seen. "Open the chat" (or Enter, or a double-click) '
    'opens the chat in the Claude app. A second click on the item closes it. A long reply is cut after about eleven lines with '
    '"Show all", so the list never has a scroll area inside a scroll area; option B is an inner scroll area instead. Chosen on 2026-11-12: A.</p>'
    '<div class="side">'
    f'<div>{cap("A short reply, open")}{frame("dark", plain(open_short))}</div>'
    f'<div>{cap("A long reply, A: cut, then Show all", REC)}'
    f'{frame("dark", plain(open_item(LONG, "ncut", chr(10) + "<div class=nact style=margin-top:4px><span class=nlink>Show all · 24 lines</span></div>")))}</div>'
    f'<div>{cap("A long reply, B: an inner scroll area")}{frame("dark", plain(open_item(LONG, "nscroll")))}</div>'
    '</div>'
    '<h3 style="margin-top:22px">Older replies</h3>'
    '<p>Replies older than 24 hours are not hidden. Below "Last 24 hours" sits the group "Older", folded at first, with its count. '
    'Unfolded, it shows the latest older reply of each chat, newest first, as the same rows; unseen ones keep the amber dot. '
    'A click on the group line, or ← and → on it, folds and unfolds it, and ClaudeDeck remembers the choice.</p>'
    '<div class="side">'
    f'<div>{cap("Older, folded (as at first)")}{deck("dark", head("Last 24 hours", 2) + "".join(item(a, b, c, d, u, None, sv) for a, b, c, d, u, sv in NEWS[4:6]) + older_head(len(OLDER), False))}</div>'
    f'<div>{cap("Older, unfolded")}{deck("dark", OLDER_OPEN)}</div>'
    '</div>'
    '<h3 style="margin-top:22px">Empty, and the keyboard</h3>'
    '<p>Ctrl+Tab and Ctrl+Shift+Tab switch between Chats and News. On News, ↑ ↓ move between items, Space or → opens an item, ← closes it, '
    'Enter opens the chat, Home and End go to the first and the last item, Esc gives the focus back to the window before. The digits 1 – 9 '
    'stay on the Chats tab. The pinned items stay when there is no reply, so the empty text shows only when nothing at all happened.</p>'
    '<div class="side">'
    f'<div>{cap("No replies in 24 hours")}{EMPTY}</div>'
    f'<div>{cap("The keyboard: ↓ moved the focus to the second item")}{deck("dark", kbd_body)}</div>'
    '</div>'
    '<h3 style="margin-top:22px">Two small extras</h3>'
    '<p>"Mark all seen" makes every reply in the list seen at once, without opening the chats in the Claude app. "Only unseen" hides the '
    'seen replies. Both sit at the right of the "Last 24 hours" line. Both chosen on 2026-11-12, and the count in mini mode too.</p>'
    '<div class="side">'
    f'<div>{cap("Mark all seen and Only unseen", PROP)}{deck("dark", prop_body)}</div>'
    '</div>'
    '</section>'
)

anchor = "<section><h2>Rules</h2>"
assert s.count(anchor) == 1
s = s.replace(anchor, section + anchor, 1)
open(dst, "w", encoding="utf-8", newline="\n").write(s)
print("ok", len(s), len(section))
