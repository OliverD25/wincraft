# WinCraft — Project Constitution

> **Status: protected.** This document sets the goal and the principles of
> WinCraft. No part of it, wording or content, may be changed without explicit
> approval from the project creator.
>
> Adopted: 2026-10-21

## Main goal

"One small tray app that adds the Windows 11 tools Microsoft forgot: fast,
keyboard-first, and built from plugins anyone can write."

Every feature, design and code decision must fit this goal and the principles
below. When two principles pull in different directions, the one with the
lower number wins.

## 1. Never surprise the user

WinCraft never moves, closes, hides or changes a window, a file or a setting
that the user did not ask it to touch. When it must act on its own, it says so
on screen and can be undone or stopped.

In practice:

- LayoutKeeper restores windows by itself only right after sign-in, never
  after an update or a restart.
- A failed update puts the old version back by itself.
- Test copies of WinCraft only read the user's real windows; they never move,
  close or rename them.
- A message on screen always names WinCraft, so the user knows who is talking.

## 2. Free, open source, and easy to contribute to

WinCraft is free and open source under the MIT licence, and stays that way.
It is built so that a person who has never seen the code can add a useful
feature in an afternoon.

In practice:

- [CONTRIBUTING.md](CONTRIBUTING.md) explains everything a contributor needs:
  how to add a plugin, the rules, and how to test next to their own WinCraft.
- One command, `tools\check.ps1`, runs every check a pull request must pass.
- Issues labelled `good first issue` are small, well described and ready to
  take.
- Pull requests from anyone get a polite, clear review. Everyone follows the
  [Code of Conduct](CODE_OF_CONDUCT.md).
- Code, comments, docs and messages are written in plain English, so people
  who are not native speakers can read them.

## 3. Plugin first

A new tool is a plugin. A plugin lives in its own folder under `src/plugins/`
and talks to the rest of WinCraft only through the host's plugin interface. A
contributor can add, change or remove a plugin without touching the core.

In practice:

- The host gives plugins hotkeys, settings pages, the tray, notices and the
  palette for free, so a plugin holds only its own idea.
- Settings are declared as fields; the settings window draws and saves them.
- A plugin can be switched off and on again at any time, and leaves nothing
  behind when it is off.
- Code that two plugins need goes into a shared helper in `src/core/`, not
  into a copy.

## 4. Keyboard first

Every feature can be reached from the keyboard. The mouse is welcome but never
required.

In practice:

- Default hotkeys start with `Win+Alt`. This combination is nearly free on
  Windows, and it makes every WinCraft hotkey feel like one family.
- The command palette (`Win+Alt+P`) reaches every command, setting and plugin.
- Every hotkey can be changed, and WinCraft warns when another app already
  holds it.

## 5. Free and light

WinCraft costs nothing to use and nothing to run. It is one small exe that
starts fast and uses little memory while it waits in the tray.

In practice:

- No paid service, account, server or telemetry (data sent home about the
  user).
- Few dependencies: a new crate needs a discussion first.
- No installer is required; the exe can install and update itself.
- Nothing runs in the background that the user did not switch on.

## 6. Plain and native

WinCraft looks and feels like a part of Windows 11, and it talks to the user
in short, plain English.

In practice:

- Windows 11 fonts and colours, a light and a dark theme, and the rules of the
  design brief ([docs/design-brief.md](docs/design-brief.md)).
- No text in the app is smaller than 12 px.
- Labels and messages use common words and one idea per sentence. They say
  what will happen, not how it works inside.

## 7. Tested before it ships

A release must not break what worked before. Every change passes the same
checks on the contributor's PC that a release passes.

In practice:

- Formatting, tests, clippy and the release build all pass with zero
  warnings; `#[allow]` is not used to hide a warning.
- The line-coverage floor in `tools/coverage-floor.txt` only goes up.
- A bug that reached a user gets a test that would have caught it.
- Each release is built from a tag by GitHub Actions, with a checksum the
  updater verifies before it installs anything.
