# WinCraft

## The Constitution comes first

[CONSTITUTION.md](CONSTITUTION.md) is the highest authority in this project. It
is loaded into every session below. Every design, code and feature decision
must be consistent with it.

- **Never edit CONSTITUTION.md without explicit approval from the creator in
  chat.** This covers wording, formatting, order and content. Approval is per
  change. Propose the change as an exact before/after text, then wait for a
  clear yes.
- **If a request conflicts with a principle, stop and say so.** Name the
  principle by number and title (for example "Principle 1, Never surprise the
  user"), explain the conflict in one or two sentences, and ask how to
  proceed. Do not quietly work around a principle.
- **Cite principles when they drive a decision.** When a design choice follows
  from a principle, say which one.
- **When in doubt about scope, default to Principle 3.** A new tool is a
  plugin in its own folder, not a change to the core.

## The other documents

- [CONTRIBUTING.md](CONTRIBUTING.md): how to write a plugin, the code rules,
  the checks, and how to test next to a running WinCraft without touching it.
  Read its "Rules" and "Testing next to your own WinCraft" sections before
  changing code.
- [docs/design-brief.md](docs/design-brief.md): the look of WinCraft and the
  hard visual limits.
- [README.md](README.md): what WinCraft does, for users.

@CONSTITUTION.md

## Working cheaply (rules for every session and coder agent)

Tokens go on what a session reads, and every later turn pays for it again.
Measured on 2026-10-27: test output and screenshots were a small part; whole
big files read into the context and long runs of turns were most of it.

- **Read with ranges, not whole files.** Find the place with `grep -n`, then
  read 40–150 lines around it with `sed -n 'a,bp'` or Read with offset and
  limit. Never read a whole file over 800 lines. A test module lives in its
  own `tests.rs`; read it only when you change tests.
- **Make a big mechanical edit with a small script**, not by printing the file
  and rewriting it.
- **Use the three check levels** of `tools\check.ps1`:
  - `-Fast` while you work (fmt and the tests of the changed modules);
  - `-Quick` once when the change is finished (all tests and clippy);
  - the full run only once, right before a release. The main session does
    it, so a coder agent does not run the full check.
- **Filter command output.** For example
  `cargo test 2>&1 | grep -E "^test result|FAILED|panicked|^error"`, or
  `| tail -20`. Never print a full build log.
- **Three tries, then report.** If the same failure survives three different
  fixes, stop and report the error text instead of trying more.
