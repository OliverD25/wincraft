//! When the taskbar's hover popup may be hidden: the key rules that keep
//! Win+number and Alt+Tab working. Shared by the hover strip and by the
//! hide-popup test (`--test-hide-popup`), which proved them. Nothing here
//! calls Windows.

/// Explorer's hover popup. Its title is set only after it is shown, so the
/// class is what finds it.
pub const POPUP_CLASS: &str = "XamlExplorerHostIslandWindow";
pub const POPUP_TITLE: &str = "Task Switching";

/// A popup that opens this soon after Win+key was pressed was opened by
/// the keyboard. Repeated Win+number presses come well inside it.
pub const WIN_COMBO_MS: u64 = 1500;
/// After a Windows key is released, the popup it opened may still be
/// opening.
pub const WIN_RELEASE_MS: u64 = 500;
/// Any other key this recent means the keyboard may be driving the
/// taskbar (Win+T, then the arrow keys).
pub const RECENT_KEY_MS: u64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reason {
    WinKey,
    AltKey,
    RecentKey,
    MouseButton,
    NotOnButton,
}

impl Reason {
    pub fn text(self) -> &'static str {
        match self {
            Reason::WinKey => "left alone (Win+number)",
            Reason::AltKey => "left alone (Alt is held, maybe Alt+Tab)",
            Reason::RecentKey => "left alone (a key was pressed just before)",
            Reason::MouseButton => "left alone (a mouse button is down)",
            Reason::NotOnButton => "left alone (the cursor is not on a taskbar app button)",
        }
    }
}

/// What the keyboard and mouse were doing when the popup appeared. Times
/// are milliseconds ago, `None` when it never happened in this run.
#[derive(Debug, Clone, Copy, Default)]
pub struct Inputs {
    pub win_down: bool,
    pub since_win_combo: Option<u64>,
    pub since_win_up: Option<u64>,
    pub alt_down: bool,
    pub since_key: Option<u64>,
    pub mouse_button_down: bool,
    pub on_app_button: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Hide,
    LeaveAlone(Reason),
}

/// Hide only a popup that a mouse hover opened: the cursor rests on an
/// app button, no mouse button is down, and no key, least of all a
/// Windows key, had a part in it. Win+number shows the same popup and
/// must keep working, so the keyboard always wins.
pub fn decide(inputs: &Inputs) -> Decision {
    let within = |since: Option<u64>, limit: u64| since.is_some_and(|ms| ms <= limit);
    if inputs.win_down
        || within(inputs.since_win_combo, WIN_COMBO_MS)
        || within(inputs.since_win_up, WIN_RELEASE_MS)
    {
        return Decision::LeaveAlone(Reason::WinKey);
    }
    if inputs.alt_down {
        return Decision::LeaveAlone(Reason::AltKey);
    }
    if within(inputs.since_key, RECENT_KEY_MS) {
        return Decision::LeaveAlone(Reason::RecentKey);
    }
    if inputs.mouse_button_down {
        return Decision::LeaveAlone(Reason::MouseButton);
    }
    if !inputs.on_app_button {
        return Decision::LeaveAlone(Reason::NotOnButton);
    }
    Decision::Hide
}

/// What the keys did lately, from one look at them per tick. Times are
/// milliseconds on any clock that only goes forward.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyWatch {
    win_was_down: bool,
    last_win_combo: Option<u64>,
    last_win_up: Option<u64>,
    last_key: Option<u64>,
}

/// One look at the keys and mouse buttons.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyState {
    pub win_down: bool,
    pub digit_down: bool,
    pub alt_down: bool,
    /// Any key that moves around the taskbar from the keyboard: the arrows,
    /// Tab, Enter, Space or Esc.
    pub nav_down: bool,
    pub escape: bool,
    /// Any mouse button.
    pub mouse_button_down: bool,
    /// The middle button, which opens a new window from a taskbar button.
    pub middle_down: bool,
}

impl KeyWatch {
    pub fn see(&mut self, keys: KeyState, now_ms: u64) {
        if keys.win_down && keys.digit_down {
            self.last_win_combo = Some(now_ms);
        }
        if self.win_was_down && !keys.win_down {
            self.last_win_up = Some(now_ms);
        }
        if keys.nav_down {
            self.last_key = Some(now_ms);
        }
        self.win_was_down = keys.win_down;
    }

    /// A Windows key was down at the last look. It may have been let go
    /// since, before that release could be seen.
    pub fn win_was_down(&self) -> bool {
        self.win_was_down
    }

    /// The inputs `decide` reads, for keys seen now.
    pub fn inputs(&self, keys: KeyState, now_ms: u64, on_app_button: bool) -> Inputs {
        let since = |at: Option<u64>| at.map(|at| now_ms.saturating_sub(at));
        Inputs {
            win_down: keys.win_down,
            since_win_combo: since(self.last_win_combo),
            since_win_up: since(self.last_win_up),
            alt_down: keys.alt_down,
            since_key: since(self.last_key),
            mouse_button_down: keys.mouse_button_down,
            on_app_button,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hover_inputs() -> Inputs {
        Inputs {
            on_app_button: true,
            ..Inputs::default()
        }
    }

    #[test]
    fn a_hover_opened_popup_is_hidden() {
        assert_eq!(decide(&hover_inputs()), Decision::Hide);
        let old_keys = Inputs {
            since_win_combo: Some(5000),
            since_win_up: Some(5000),
            since_key: Some(5000),
            ..hover_inputs()
        };
        assert_eq!(decide(&old_keys), Decision::Hide);
    }

    #[test]
    fn a_windows_key_or_a_recent_win_number_leaves_it_alone() {
        let alone = Decision::LeaveAlone(Reason::WinKey);
        let down = Inputs {
            win_down: true,
            ..hover_inputs()
        };
        assert_eq!(decide(&down), alone);
        let combo = Inputs {
            since_win_combo: Some(1200),
            ..hover_inputs()
        };
        assert_eq!(decide(&combo), alone);
        let released = Inputs {
            since_win_up: Some(300),
            ..hover_inputs()
        };
        assert_eq!(decide(&released), alone);
        let with_button = Inputs {
            mouse_button_down: true,
            ..down
        };
        assert_eq!(
            decide(&with_button),
            alone,
            "the Windows key is named first"
        );
    }

    #[test]
    fn a_mouse_button_alt_or_another_key_leaves_it_alone() {
        let button = Inputs {
            mouse_button_down: true,
            ..hover_inputs()
        };
        assert_eq!(decide(&button), Decision::LeaveAlone(Reason::MouseButton));
        let alt = Inputs {
            alt_down: true,
            ..hover_inputs()
        };
        assert_eq!(decide(&alt), Decision::LeaveAlone(Reason::AltKey));
        let key = Inputs {
            since_key: Some(100),
            ..hover_inputs()
        };
        assert_eq!(decide(&key), Decision::LeaveAlone(Reason::RecentKey));
        assert_eq!(
            decide(&Inputs::default()),
            Decision::LeaveAlone(Reason::NotOnButton)
        );
    }

    #[test]
    fn the_key_watch_remembers_win_number_and_the_release() {
        let mut watch = KeyWatch::default();
        let quiet = KeyState::default();
        watch.see(quiet, 0);
        assert_eq!(decide(&watch.inputs(quiet, 0, true)), Decision::Hide);

        let combo = KeyState {
            win_down: true,
            digit_down: true,
            ..quiet
        };
        watch.see(combo, 1000);
        assert_eq!(
            decide(&watch.inputs(combo, 1000, true)),
            Decision::LeaveAlone(Reason::WinKey)
        );
        watch.see(quiet, 1100);
        assert_eq!(
            decide(&watch.inputs(quiet, 1400, true)),
            Decision::LeaveAlone(Reason::WinKey),
            "Win+digit was 0.4 s ago"
        );
        assert_eq!(
            decide(&watch.inputs(quiet, 2600, true)),
            Decision::Hide,
            "long enough after both"
        );

        let arrow = KeyState {
            nav_down: true,
            ..quiet
        };
        watch.see(arrow, 5000);
        assert_eq!(
            decide(&watch.inputs(quiet, 5200, true)),
            Decision::LeaveAlone(Reason::RecentKey)
        );
        assert_eq!(decide(&watch.inputs(quiet, 5600, true)), Decision::Hide);
    }

    #[test]
    fn a_win_tap_alone_counts_from_its_release() {
        let mut watch = KeyWatch::default();
        let win = KeyState {
            win_down: true,
            ..KeyState::default()
        };
        watch.see(win, 100);
        watch.see(KeyState::default(), 200);
        assert_eq!(
            decide(&watch.inputs(KeyState::default(), 600, true)),
            Decision::LeaveAlone(Reason::WinKey)
        );
        assert_eq!(
            decide(&watch.inputs(KeyState::default(), 800, true)),
            Decision::Hide
        );
    }
}
