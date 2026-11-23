"""Add section 17, a note pinned to a chat (options), to ClaudeDeck.dc.html.

Usage: python -I make_claudedeck_notes.py <in.html> <out.html>
Input is the v54 page. The section goes in before the "Rules" section.
"""
import re
import sys

src, dst = sys.argv[1], sys.argv[2]
page = open(src, encoding="utf-8").read()
assert "17. A note on a chat" not in page, "section 17 is already in the page"


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
.nt{display:flex;align-items:center;gap:5px;margin:4px 0 2px;padding:1px 6px;border-radius:4px;max-width:100%;
  box-sizing:border-box;width:fit-content;background:color-mix(in srgb,var(--accent) 14%,transparent);font-size:12px;color:var(--text)}
.nt span{white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0}
.nt .nx{margin-left:6px;color:var(--text2);flex:none;display:flex}
.nchip{display:inline-flex;align-items:center;gap:4px;max-width:130px;flex:0 1 auto;min-width:0;padding:0 6px;height:16px;
  border-radius:4px;background:color-mix(in srgb,var(--accent) 14%,transparent);font-size:12px;color:var(--text)}
.nchip span{white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0}
.t2.nl{color:var(--text);display:flex;align-items:center;gap:5px}
.t2.nl span{white-space:nowrap;overflow:hidden;text-overflow:ellipsis;min-width:0}
.nbox{display:flex;align-items:center;gap:8px;margin:4px 0 4px;padding:2px 6px;border:1px solid var(--accent);border-radius:4px;
  background:var(--panel);font-size:12px;color:var(--text)}
.nbox .ph{color:var(--dis)}
.nbox .cr{width:1px;height:13px;background:var(--text);margin-left:-6px}
.nbox .nh{margin-left:auto;color:var(--text2);white-space:nowrap}
.nicon{flex:none;display:block}
"""
page = page.replace("</style>", CSS + "</style>", 1)

ICON = ('<svg class="nicon" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="var(--accent)" stroke-width="2.2" '
        'stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15.5 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14'
        'a2 2 0 0 0 2-2V8.5z"/><path d="M15 3v5a1 1 0 0 0 1 1h5"/></svg>')
X = ('<span class="nx" title="Remove the note"><svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">'
     '<path d="M3 3l6 6M9 3l-6 6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg></span>')
TREE = ('<span class="ind tree" style="width:14px"><span class="vl" style="left:6px;top:0;bottom:calc(100% - 11px)"></span>'
        '<span class="hl" style="left:6px;top:11px;width:7px"></span></span>')

LONG = "Ask about the Stats tab colours, then check the light theme screenshots again before the merge"
# name, branch, status, time, last line, note, fork
CHATS = [
    ("WinCraft Rust", "main", "working", "2 min", "Last: started the release build", "Test the tray after 0.20.141 is out", False),
    ("WinCraft design", "design", "asks", "4 min", "Which option should the note use?", "", False),
    ("Playwise - Kids Learn", "main", "done", "12 min", "Last: the lesson screens are fixed", LONG, False),
    ("Fan layout try", "fan", "stopped", "1 h", "Last: the fan hides most of each picture", "Throwaway, delete after Friday", True),
]


def row(chat, opt, hover_x=False, edit=None):
    name, branch, status, tm, last, note, fork = chat
    lead = (TREE if fork else "") + '<span style="width:10px;flex:none"></span>' + MK[status]
    chip = ""
    if opt == "B" and note:
        chip = f'<span class="nchip" title="{note}">{ICON}<span>{note}</span></span>'
    l1 = f'<span class="nm">{name}</span>{chip}<span class="tag">{branch}</span><span class="tm">{tm}</span>'
    if opt == "C" and note:
        l2 = f'<div class="t2 nl">{ICON}<span>{note}</span></div>'
    else:
        l2 = f'<div class="t2">{last}</div>'
    l3 = ""
    if edit is not None:
        text, hint = edit
        inner = (f'<span>{text}</span><span class="cr"></span>' if text else
                 '<span class="cr" style="margin-left:0"></span><span class="ph">A note for this chat</span>')
        l3 = f'<div class="nbox">{ICON}{inner}<span class="nh">{hint}</span></div>'
    elif opt == "A" and note:
        l3 = f'<div class="nt">{ICON}<span>{note}</span>{X if hover_x else ""}</div>'
    style = ' style="background:var(--hover)"' if hover_x else ""
    return (f'<div class="row two"{style}>{lead}<div class="body" style="min-width:0;flex:1"><div class="l1">{l1}</div>'
            f'{l2}{l3}</div></div>')


def deck(theme, opt, rows=None):
    body = rows if rows is not None else "".join(row(c, opt) for c in CHATS)
    return (f'<div class="{theme} deck" style="width:360px"><div class="tabs"><span class="tab on">Chats</span>'
            f'<span class="tab">News</span><span class="tab">Stats</span></div>{body}<div style="height:6px"></div></div>')


def cap(text):
    return f'<div class="cap">{text}</div>'


def menu(theme, has_note):
    def mi(t, k=""):
        return f'<div class="mi"><span>{t}</span><span class="mk2">{k}</span></div>'
    note_items = (mi("Edit the note…", "N") + mi("Remove the note")) if has_note else mi("Add a note…", "N")
    return (f'<div class="{theme} menu" style="width:250px">' + mi("Open the chat", "Enter")
            + mi("Show in the Claude app", "Ctrl+Enter") + mi("Pin to the top") + note_items
            + mi("Give a Quick access number ›") + mi("Hide this chat") + mi("Copy the name", "Ctrl+C")
            + mi("Rename the chat…", "F2") + '<div class="msep"></div>' + mi("Move to group ›") + "</div>")


def hover(theme):
    return (f'<div class="{theme} hoverpanel" style="margin:0">'
            f'<div class="hp1">{MK["done"]}Playwise - Kids Learn · done, not seen · 12 min</div>'
            '<div class="hp1">playwise · main</div>'
            f'<div class="hp1" style="align-items:flex-start;gap:6px;color:var(--text)"><span style="margin-top:3px">{ICON}</span>'
            f'<span>{LONG}</span></div>'
            'Last: the lesson screens are fixed. All 14 tests pass.</div>')


REC = '<span class="rec">Recommended</span>'


def pair(title, opt, text, rec=""):
    return (f'<h3 style="margin-top:24px">{title} {rec}</h3><p>{text}</p><div class="side">'
            f'<div>{cap("Dark")}{deck("dark", opt)}</div><div>{cap("Light")}{deck("light", opt)}</div></div>')


edit_rows_new = row(CHATS[1], "A", edit=("", "Enter saves · Esc cancels")) + row(CHATS[0], "A")
edit_rows_old = row(CHATS[0], "A", edit=("Test the tray after 0.20.141 is out", "34 / 120"))
hover_rows = row(CHATS[0], "A", hover_x=True) + row(CHATS[2], "A")

section = (
    '<section><h2>17. A note on a chat (options for review 2026-11-23)</h2>'
    "<p>Asked on 2026-11-23: pin a small note to a chat on the Chats tab, and remove it again. One note per chat, "
    "plain text, one line, up to 120 characters. It is saved with the list, so it stays after a restart, when the chat "
    "goes cold, and when it moves to another group. The same four chats in each option: a short note, no note, a long "
    "note cut with “…”, and a fork with a note. The note colour is the amber accent at 14% behind normal text, with a "
    "small note icon; the theme has no yellow.</p>"

    + pair("Option A: a third line under the row", "A",
           "Only a chat with a note grows by one line. The name, the branch, the time and the “Last:” line stay as they "
           "are. A long note is cut with “…”; the hover panel shows all of it. Easy to see, and it hides nothing. "
           "The cost: a row with a note is taller.", REC)
    + pair("Option B: a chip after the name", "B",
           "The row keeps its height. The chip takes at most 130 px and pushes the branch out first. In the narrow "
           "window most notes show only two or three words, so you rest the mouse on the chip to read it.")
    + pair("Option C: the note takes the place of the “Last:” line", "C",
           "The row keeps its height and the note is easy to read. The last message moves to the hover panel, so a "
           "chat with a note no longer shows what it did last at a glance.")

    + '<h3 style="margin-top:28px">Adding, editing and removing (the same for every option)</h3><ul>'
    "<li><b>Add:</b> right-click a chat → <b>Add a note…</b>, or pick the chat with ↑ ↓ and press <b>N</b> "
    "(N is free in the ClaudeDeck window today). A one-line box opens on the row, where the note will show, "
    "with the keyboard in it.</li>"
    "<li><b>In the box:</b> <b>Enter</b> saves, <b>Esc</b> cancels. With text in the box, the hint at the right "
    "becomes a count, “34 / 120”; typing stops at 120. Enter on an empty box removes the note.</li>"
    "<li><b>Edit:</b> the menu says <b>Edit the note…</b> (still <b>N</b>), and the box opens with the text "
    "selected. A double-click on the note also opens it.</li>"
    "<li><b>Remove:</b> <b>Remove the note</b> in the menu. A small × shows at the end of the note only while the "
    "mouse is on the row, so it is never in the way and is hard to hit by chance. I recommend keeping it: it is the "
    "quickest way for the mouse.</li>"
    "<li>A fork can have its own note. A stand-in (a hidden or not-found fork) has no note items.</li></ul>"
    '<div class="side">'
    f'<div>{cap("Right-click a chat with no note")}{menu("dark", False)}</div>'
    f'<div>{cap("Right-click a chat with a note")}{menu("dark", True)}</div>'
    '</div><div class="side" style="margin-top:20px">'
    f'<div>{cap("N on a chat with no note: the empty box")}{deck("dark", "A", edit_rows_new)}</div>'
    f'<div>{cap("Editing a note, light")}{deck("light", "A", edit_rows_old)}</div>'
    f'<div>{cap("Mouse on the row: the × shows")}{deck("dark", "A", hover_rows)}</div>'
    "</div>"

    + '<h3 style="margin-top:28px">Where else the note shows</h3><ul>'
    "<li><b>Hover panel:</b> yes, the whole note, after the project line. This is how you read a long note.</li>"
    "<li><b>Mini mode:</b> no. The column shows status marks only, as with the ring and the token mark.</li>"
    "<li><b>Board, News, Stats:</b> no, to keep it small. The note is about the row on the Chats tab.</li></ul>"
    f'<div class="side"><div>{cap("Hover panel, dark")}{hover("dark")}</div><div>{cap("Hover panel, light")}{hover("light")}</div></div>'

    + '<h3 style="margin-top:28px">My recommendation</h3>'
    "<p>Option A, with <b>N</b>, the menu items and the × on hover, and the note in the hover panel only. Notes are "
    "rare, so most rows stay as they are, and the one row with a note shows it in full width without hiding the "
    "“Last:” line.</p></section>"
)

anchor = "<section><h2>Rules</h2>"
assert page.count(anchor) == 1
page = page.replace(anchor, section + anchor, 1)
open(dst, "w", encoding="utf-8", newline="").write(page)
print("written", dst, len(page.encode("utf-8")), "bytes")
