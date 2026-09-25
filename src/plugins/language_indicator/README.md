# LanguageIndicator

Shows which input language you switched to, in the middle of the screen, the
way macOS does. Switch from Ukrainian to English and a translucent panel shows
**UK → EN** with "English (United States) · US" under it, then fades out.

## When it appears

- When the input language of the window you are typing in changes, however
  you switch: Alt+Shift, Win+Space, Ctrl+Shift or the language button on the
  taskbar.
- Not when you move to another window that happens to use another language,
  unless you turn on **Show on focus change**.
- The palette command **Show current input language** shows it at any time,
  with the current language alone.

A new switch while the panel is up replaces the text and starts the time
again.

## Where and how long

It appears on the monitor of the window you are typing in: in the centre, or
in the upper or lower third (**Where it appears**). It stays for 900 ms by
default (**How long it stays**, 300 to 5000) and fades out over a quarter of
a second.

The panel never takes the keyboard focus, lets clicks through to what is
under it, and never shows in the taskbar or in Alt+Tab. It follows
WinCraft's light or dark theme.

## Names

The big line uses the two-letter language codes (ISO 639): EN, UK, DE. The
line under it is the language's name in that language, such as "українська
(Україна)", and the keyboard layout, such as "Ukrainian (Enhanced)". Turn off
**Show the layout name** to see the language name alone.

## How it notices a switch

Windows does not tell ordinary programs when the input language changes. The
plugin looks at the keyboard layout of the window in front eight times a
second, which costs three quick system calls, and also listens to the shell's
own language message. Whichever notices a switch first shows it; the other
one is ignored, so every switch is shown once.
