//! The wait after QuickJump's shortcut (Win+`) and what the next key does
//! with the window in front: jump, add, put at a number, unpin, or open
//! Quick access. The host's chord tip shows the keys that can follow while
//! it waits, and how the wait ended.

use std::time::Instant;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN};
use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::core::chord_tip::{self, Content, End, Row};
use crate::core::{chord, hotkeys, mask_key, shortcuts};

use super::keys::{self, Key};
use super::matching::{Live, SlotView};
use super::notice;
use super::pins::{self, Pinned};
use super::{live, Engine, Waiting, WM_APP_KEY};

/// Which shortcut started the wait.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The jump shortcut: a digit jumps.
    Jump,
    /// The pin shortcut, when the user gave it keys: a digit puts the
    /// window in front at that number.
    Pin,
}

/// What a key asks for, once the mode is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Jump(usize),
    PutAt(usize),
    Add,
    Unpin,
    Access,
    Cancel,
    Unassigned,
    Nothing,
}

pub fn act(mode: Mode, key: Key) -> Act {
    let slot = |digit: u8| pins::slot_of_digit(digit).unwrap_or(0);
    match (mode, key) {
        (Mode::Jump, Key::Digit(digit)) => Act::Jump(slot(digit)),
        (Mode::Pin, Key::Digit(digit)) | (_, Key::AltDigit(digit)) => Act::PutAt(slot(digit)),
        (_, Key::Add) => Act::Add,
        (_, Key::Unpin) => Act::Unpin,
        (_, Key::Access) => Act::Access,
        (_, Key::Cancel) => Act::Cancel,
        (_, Key::Other) => Act::Unassigned,
        (_, Key::Again) => Act::Nothing,
    }
}

/// The keys the tip lists: each pinned window by its number while the
/// jump shortcut waits, then what the other keys do.
pub fn rows(mode: Mode, views: &[SlotView]) -> Vec<Row> {
    let mut rows = Vec::new();
    match mode {
        Mode::Jump => {
            for view in views {
                rows.push(Row::new(view.digit.to_string(), view.title_line()));
            }
            rows.push(Row::new("`", "Open quick access"));
            rows.push(Row::new("A", "Add this window"));
            rows.push(Row::new("Alt+1\u{2013}0", "Put this window at a number"));
        }
        Mode::Pin => {
            rows.push(Row::new("1\u{2013}0", "Put this window at a number"));
            rows.push(Row::new("A", "Add this window"));
            rows.push(Row::new("`", "Open quick access"));
        }
    }
    rows.push(Row::new("U", "Unpin this window"));
    rows
}

/// How many of the tip's first rows are the pinned windows, which the tip
/// draws as a grid of their own.
pub fn pin_rows(mode: Mode, views: &[SlotView]) -> usize {
    match mode {
        Mode::Jump => views.len(),
        Mode::Pin => 0,
    }
}

/// The keycaps of a shortcut's text, both steps of a chord in a row.
pub fn caps(keys: &str) -> Vec<String> {
    keys.split(',')
        .flat_map(|step| step.split('+'))
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string)
        .collect()
}

/// The modifiers of a shortcut's last press, which the user may still
/// hold when the next key comes.
fn held(keys: &str) -> u32 {
    shortcuts::parse(keys)
        .map(|keys| keys.second.unwrap_or(keys.first).modifiers)
        .unwrap_or(0)
        & (MOD_WIN | MOD_ALT | MOD_CONTROL | MOD_SHIFT)
}

impl Engine {
    fn keys_of(&self, mode: Mode) -> &str {
        match mode {
            Mode::Jump => &self.jump_keys,
            Mode::Pin => &self.pin_keys,
        }
    }

    /// The jump or pin shortcut came: wait for the next key, with the
    /// window in front as the one the keys act on.
    pub(super) fn open_wait(&mut self, mode: Mode) {
        self.close_wait(false);
        self.close_notice();
        let foreground = unsafe { GetForegroundWindow() };
        self.refresh();
        let front = foreground as isize;
        // From the palette the window in front is WinCraft's own, so the
        // one under it is meant.
        let target = self.target_of(foreground);
        let listening = chord::listen(
            self.hwnd as isize,
            WM_APP_KEY,
            shortcuts::CHORD_WAIT,
            keys::rule(&self.jump_keys, &self.pin_keys),
        );
        if !listening {
            self.notify(notice::no_keys(), None);
            self.open_list(None);
            return;
        }
        let views = self.views(front);
        let keys = self.keys_of(mode).to_string();
        chord_tip::show(
            self.hwnd as isize,
            Content {
                keys: caps(&keys),
                rows: rows(mode, &views),
                pins: pin_rows(mode, &views),
            },
            shortcuts::CHORD_WAIT,
        );
        log::info!(
            "QuickJump: waiting 3 s for the next key ({})",
            match mode {
                Mode::Jump => "jump",
                Mode::Pin => "pin",
            }
        );
        self.waiting = Some(Waiting {
            mode,
            front,
            target,
        });
        self.undo_keys = false;
    }

    /// The window the keys act on: the one in front, or from WinCraft's own
    /// window (the palette) the one under it.
    fn target_of(&self, foreground: windows_sys::Win32::Foundation::HWND) -> Option<Live> {
        let front = foreground as isize;
        let own = live::is_own(foreground);
        self.live
            .iter()
            .find(|window| window.hwnd == front)
            .or_else(|| own.then(|| self.live.first()).flatten())
            .cloned()
    }

    /// The palette's "Add this window": the same as Win+`, then A.
    pub(super) fn add_front_window(&mut self) {
        let foreground = unsafe { GetForegroundWindow() };
        self.refresh();
        match self.target_of(foreground) {
            Some(window) => self.add_window(&window),
            None => self.notify(notice::no_window(), None),
        }
    }

    /// Ends the wait. `by_key` says another key closed it, which a second
    /// jump press right after still counts as "twice".
    pub(super) fn close_wait(&mut self, by_key: bool) {
        if let Some(waiting) = self.waiting.take() {
            chord::stop_for(self.hwnd as isize);
            if by_key {
                self.closed_by_key = Some((Instant::now(), waiting.mode));
            }
        }
        self.stop_ticks_if_idle();
    }

    pub(super) fn on_keys(&mut self) {
        let news = chord::take_for(self.hwnd as isize);
        if news
            .taken
            .iter()
            .any(|stroke| stroke.modifiers & MOD_ALT != 0)
        {
            // Alt went down with a key Windows never saw: without a key in
            // between, letting Alt go would open the menu of the app in
            // front.
            mask_key::send("QuickJump");
        }
        if self.undo_keys {
            if !news.taken.is_empty() {
                self.undo();
            }
            if news.ended || !news.taken.is_empty() {
                self.undo_keys = false;
                chord::stop_for(self.hwnd as isize);
            }
            return;
        }
        let Some(mode) = self.waiting.as_ref().map(|waiting| waiting.mode) else {
            return;
        };
        let held = held(self.keys_of(mode));
        for stroke in news.taken {
            let act = act(mode, keys::kind(stroke, &[]));
            if act == Act::Nothing {
                continue;
            }
            let label = chord_tip::key_label(stroke, held);
            if let Some(waiting) = self.waiting.take() {
                chord::stop_for(self.hwnd as isize);
                self.run(act, &label, waiting);
            }
            self.stop_ticks_if_idle();
            return;
        }
        if news.ended {
            // Another wait took the keyboard, and shows its own tip.
            self.close_wait(true);
            chord_tip::hide(self.hwnd as isize);
        }
    }

    /// The wait's time ran out.
    pub(super) fn wait_timed_out(&mut self) {
        if self.waiting.is_some() {
            log::info!("QuickJump: no key came, the wait ended");
            chord_tip::end(self.hwnd as isize, End::Cancelled);
        }
        self.close_wait(false);
    }

    fn tip(&self, end: End) {
        chord_tip::end(self.hwnd as isize, end);
    }

    fn refuse(&self, key: &str, text: String) {
        self.tip(End::Refused {
            key: key.to_string(),
            text,
        });
    }

    fn run(&mut self, act: Act, key: &str, waiting: Waiting) {
        let owner = self.hwnd as isize;
        match act {
            Act::Jump(slot) => match self.jump(slot, waiting.front) {
                Some(name) => self.tip(End::Matched {
                    key: key.to_string(),
                    text: name,
                }),
                None => chord_tip::hide(owner),
            },
            Act::Access => {
                self.tip(End::Matched {
                    key: key.to_string(),
                    text: "Quick access".to_string(),
                });
                self.open_list(None);
            }
            Act::Cancel => self.tip(End::Cancelled),
            Act::Unassigned => {
                log::info!("QuickJump: nothing is assigned to {key}");
                self.tip(End::Unassigned {
                    key: key.to_string(),
                });
            }
            Act::Add | Act::PutAt(_) | Act::Unpin => {
                let Some(target) = waiting.target else {
                    self.refuse(key, "There is no window to pin".to_string());
                    return;
                };
                self.change(act, key, &target);
            }
            Act::Nothing => {}
        }
    }

    /// Adds, places or unpins `target`. A change shows the notice with
    /// Undo in the tip's place; no change says why in the tip.
    fn change(&mut self, act: Act, key: &str, target: &Live) {
        let owner = self.hwnd as isize;
        if act == Act::Unpin {
            match self.slots.slot_of_window(target.hwnd) {
                Some(slot) => {
                    chord_tip::hide(owner);
                    self.unpin(slot);
                }
                None => self.refuse(key, "Not in Quick access".to_string()),
            }
            return;
        }
        let done = self.place(act, target);
        match done {
            Pinned::New { .. } | Pinned::Moved { .. } => chord_tip::hide(owner),
            Pinned::Same { at } => self.refuse(key, notice::already(at)),
            Pinned::Full => self.refuse(key, notice::FULL.to_string()),
        }
    }

    fn place(&mut self, act: Act, window: &Live) -> Pinned {
        let (done, undo) = match act {
            Act::PutAt(slot) => self.slots.pin(slot, window.to_pin()),
            _ => self.slots.add(window.to_pin()),
        };
        if !done.changed() {
            return done;
        }
        let name = match done {
            Pinned::New { at } | Pinned::Moved { to: at, .. } => self
                .slots
                .get(at)
                .map_or_else(|| window.app_name(), |pin| pin.name().to_string()),
            _ => window.app_name(),
        };
        let message = notice::pinned(&name, &done);
        log::info!("QuickJump: {}", message.text);
        self.save();
        self.publish();
        let text = message.text.clone();
        self.notify(message, Some((undo, text)));
        done
    }

    /// "Pin a window…" chose this window: it goes to the end of the list.
    pub(super) fn add_window(&mut self, window: &Live) {
        let done = self.place(Act::Add, window);
        if !done.changed() {
            self.notify(notice::pinned(&window.app_name(), &done), None);
        }
    }

    /// Takes the pin at `slot` out; the gap closes.
    pub(super) fn unpin(&mut self, slot: usize) {
        if let Some((pin, undo)) = self.slots.remove(slot) {
            let message = notice::removed(pin.name(), slot);
            log::info!("QuickJump: {}", message.text);
            self.save();
            self.publish();
            let text = message.text.clone();
            self.notify(message, Some((undo, text)));
        }
    }

    /// The jump shortcut came while QuickJump waits for its next key, or
    /// right after another key ended the wait: open Quick access.
    pub(super) fn twice(&mut self) -> bool {
        let again = match (&self.waiting, self.closed_by_key) {
            (Some(waiting), _) => waiting.mode == Mode::Jump,
            (None, Some((at, Mode::Jump))) => {
                let chord = shortcuts::parse(&self.jump_keys).is_some_and(|keys| keys.is_chord());
                at.elapsed()
                    <= if chord {
                        super::AGAIN_CHORD
                    } else {
                        super::AGAIN_PLAIN
                    }
            }
            _ => false,
        };
        if !again {
            return false;
        }
        log::info!("QuickJump: the jump shortcut twice opens Quick access");
        let key = shortcuts::parse(&self.jump_keys)
            .map(|keys| hotkeys::format(keys.second.unwrap_or(keys.first)))
            .unwrap_or_default();
        let key = caps(&key).last().cloned().unwrap_or_default();
        self.tip(End::Matched {
            key,
            text: "Quick access".to_string(),
        });
        self.close_wait(false);
        self.closed_by_key = None;
        self.open_list(None);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::matching::{slot_views, tests::window};
    use super::super::pins::tests::list;
    use super::*;

    #[test]
    fn each_key_after_the_jump_shortcut_has_its_job() {
        assert_eq!(act(Mode::Jump, Key::Digit(1)), Act::Jump(0));
        assert_eq!(act(Mode::Jump, Key::Digit(0)), Act::Jump(9));
        assert_eq!(act(Mode::Jump, Key::AltDigit(3)), Act::PutAt(2));
        assert_eq!(act(Mode::Jump, Key::AltDigit(0)), Act::PutAt(9));
        assert_eq!(act(Mode::Jump, Key::Add), Act::Add);
        assert_eq!(act(Mode::Jump, Key::Unpin), Act::Unpin);
        assert_eq!(act(Mode::Jump, Key::Access), Act::Access);
        assert_eq!(act(Mode::Jump, Key::Cancel), Act::Cancel);
        assert_eq!(act(Mode::Jump, Key::Other), Act::Unassigned);
        assert_eq!(act(Mode::Jump, Key::Again), Act::Nothing);
    }

    #[test]
    fn after_the_pin_shortcut_a_digit_puts_the_window_there() {
        assert_eq!(act(Mode::Pin, Key::Digit(4)), Act::PutAt(3));
        assert_eq!(act(Mode::Pin, Key::AltDigit(4)), Act::PutAt(3));
        assert_eq!(act(Mode::Pin, Key::Add), Act::Add);
    }

    #[test]
    fn the_tip_lists_each_pinned_window_then_the_other_keys() {
        let mut slots = list(&["Wincraft", "telemetrix"]);
        if let Some(pin) = slots.get_mut(1) {
            pin.label = Some("Dashboards".to_string());
        }
        let views = slot_views(&slots, &[window(1, "Wincraft", "Wincraft")], 0);
        let rows = rows(Mode::Jump, &views);
        let pairs: Vec<(&str, &str)> = rows
            .iter()
            .map(|row| (row.key.as_str(), row.text.as_str()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("1", "Wincraft"),
                ("2", "Dashboards"),
                ("`", "Open quick access"),
                ("A", "Add this window"),
                ("Alt+1\u{2013}0", "Put this window at a number"),
                ("U", "Unpin this window"),
            ]
        );
        assert_eq!(
            pin_rows(Mode::Jump, &views),
            2,
            "the pins are the first rows"
        );
        assert_eq!(pin_rows(Mode::Pin, &views), 0);
        let pin = rows_for_pin();
        assert_eq!(pin[0].key, "1\u{2013}0");
        assert_eq!(pin.last().map(|row| row.key.as_str()), Some("U"));
    }

    fn rows_for_pin() -> Vec<Row> {
        rows(Mode::Pin, &[])
    }

    #[test]
    fn the_keycaps_cover_both_steps_of_a_chord() {
        assert_eq!(caps("Win+`"), ["Win", "`"]);
        assert_eq!(caps("Win+Alt+K, J"), ["Win", "Alt", "K", "J"]);
        assert!(caps("").is_empty());
        assert_eq!(
            held("Win+`"),
            windows_sys::Win32::UI::Input::KeyboardAndMouse::MOD_WIN
        );
        assert_eq!(held(""), 0);
    }
}
