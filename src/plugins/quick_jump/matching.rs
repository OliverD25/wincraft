//! Which open window a pin stands for, and what each slot shows. The open
//! windows come in as plain data, so the rules are tested without Windows.

use crate::core::window_identity::{bare_title, exe_key, program_name, title_contains};

use super::pins::{digit_of_slot, Pin, Slots, SLOTS};

/// One open app window, as read for matching and for the screens.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Live {
    pub hwnd: isize,
    /// The program's full path.
    pub exe: String,
    pub app_id: Option<String>,
    /// The app's own title, which pins are matched by.
    pub title: String,
    /// What the user reads, a WindowNamer name among them.
    pub shown: String,
    /// The desktop's number from 1, in Task View order; None with one
    /// desktop or for a window shown on all of them.
    pub desktop: Option<usize>,
    /// The window is on a desktop other than the one on screen.
    pub other_desktop: bool,
    /// "Left", "Centre" or "Right"; None with one monitor.
    pub monitor: Option<String>,
    pub minimized: bool,
}

impl Live {
    /// "Claude" for claude.exe.
    pub fn app_name(&self) -> String {
        program_name(&exe_key(&self.exe))
    }

    /// "Desktop 2 · Centre", leaving out what there is only one of.
    pub fn place(&self) -> String {
        place(self.desktop, self.monitor.as_deref())
    }

    /// A pin for this window, which matches it by its title for now.
    pub fn to_pin(&self) -> Pin {
        Pin {
            exe: self.exe.clone(),
            app_id: self.app_id.clone(),
            app_name: self.app_name(),
            title: bare_title(&self.title).to_string(),
            label: None,
            start: true,
            hwnd: Some(self.hwnd),
        }
    }
}

pub fn place(desktop: Option<usize>, monitor: Option<&str>) -> String {
    let desktop = desktop.map(|number| format!("Desktop {number}"));
    [desktop.as_deref(), monitor]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" \u{b7} ")
}

/// Whether the window belongs to the pin's app: the same program file and,
/// when both have one, the same AppUserModelID, which keeps a browser's
/// web apps apart from the browser.
pub fn same_app(pin: &Pin, live: &Live) -> bool {
    exe_key(&pin.exe) == exe_key(&live.exe)
        && match (&pin.app_id, &live.app_id) {
            (Some(pinned), Some(open)) => pinned == open,
            _ => true,
        }
}

/// The app's windows whose title holds the pin's text, in the order given
/// (top of the z-order first).
pub fn candidates<'a>(pin: &Pin, live: &'a [Live]) -> Vec<&'a Live> {
    live.iter()
        .filter(|window| same_app(pin, window))
        .filter(|window| {
            title_contains(&window.title, &pin.title) || title_contains(&window.shown, &pin.title)
        })
        .collect()
}

/// The pin's window: the one it pinned while that lives, else the first of
/// the app's windows whose title holds the pin's text, else the app's only
/// window.
pub fn find<'a>(pin: &Pin, live: &'a [Live]) -> Option<&'a Live> {
    if let Some(hwnd) = pin.hwnd {
        if let Some(window) = live
            .iter()
            .find(|window| window.hwnd == hwnd && same_app(pin, window))
        {
            return Some(window);
        }
    }
    if let Some(first) = candidates(pin, live).first() {
        return Some(first);
    }
    let mut app = live.iter().filter(|window| same_app(pin, window));
    match (app.next(), app.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

/// What the line under "Title contains" says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckTone {
    Success,
    Warning,
    Quiet,
}

pub fn check_line(count: usize) -> (String, CheckTone) {
    match count {
        0 => ("No open window matches".to_string(), CheckTone::Quiet),
        1 => ("Matches 1 open window".to_string(), CheckTone::Success),
        many => (
            format!("Matches {many} windows, the first one is used"),
            CheckTone::Warning,
        ),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Empty,
    NotOpen,
    Open,
    /// The pin's window is the one in front.
    InFront,
}

/// One slot as the strip, the list and the palette show it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SlotView {
    pub slot: usize,
    pub digit: u8,
    pub state: State,
    /// The label, else the app's name; empty for an empty slot.
    pub name: String,
    pub app_name: String,
    /// The open window's title, else the pin's title text.
    pub title: String,
    pub exe: String,
    pub app_id: Option<String>,
    /// "Desktop 2 · Centre" for an open window.
    pub place: String,
    pub hwnd: Option<isize>,
    /// "D3" when the window is on another desktop.
    pub badge: Option<String>,
}

impl SlotView {
    pub fn is_pinned(&self) -> bool {
        self.state != State::Empty
    }
}

/// Every slot, in order, with the window each pin stands for now.
pub fn slot_views(slots: &Slots, live: &[Live], front: isize) -> Vec<SlotView> {
    (0..SLOTS)
        .map(|slot| {
            let digit = digit_of_slot(slot);
            let Some(pin) = slots.get(slot) else {
                return SlotView {
                    slot,
                    digit,
                    ..SlotView::default()
                };
            };
            let found = find(pin, live);
            let state = match found {
                Some(window) if window.hwnd == front => State::InFront,
                Some(_) => State::Open,
                None => State::NotOpen,
            };
            SlotView {
                slot,
                digit,
                state,
                name: pin.name().to_string(),
                app_name: pin.app_name.clone(),
                title: found.map_or_else(|| pin.title.clone(), |window| window.shown.clone()),
                exe: pin.exe.clone(),
                app_id: pin.app_id.clone(),
                place: found.map(Live::place).unwrap_or_default(),
                hwnd: found.map(|window| window.hwnd),
                badge: found
                    .filter(|window| window.other_desktop)
                    .and_then(|window| window.desktop)
                    .map(|number| format!("D{number}")),
            }
        })
        .collect()
}

/// "7 of 10 slots pinned".
pub fn pinned_line(count: usize) -> String {
    format!("{count} of {SLOTS} slots pinned")
}

#[cfg(test)]
pub mod tests {
    use super::super::pins::tests::pin;
    use super::*;

    pub fn window(hwnd: isize, app: &str, title: &str) -> Live {
        Live {
            hwnd,
            exe: format!(r"C:\Apps\{app}.exe"),
            app_id: None,
            title: title.to_string(),
            shown: title.to_string(),
            desktop: None,
            other_desktop: false,
            monitor: None,
            minimized: false,
        }
    }

    #[test]
    fn the_live_handle_wins_then_the_title_then_the_only_window() {
        let live = vec![
            window(1, "Chrome", "Mail - Google Chrome"),
            window(2, "Chrome", "Docs - Google Chrome"),
            window(3, "Claude", "Claude"),
        ];
        let mut docs = pin("Chrome", Some(2));
        docs.title = "Mail".to_string();
        assert_eq!(find(&docs, &live).map(|w| w.hwnd), Some(2), "handle");
        docs.hwnd = Some(99);
        assert_eq!(find(&docs, &live).map(|w| w.hwnd), Some(1), "title");
        docs.title = "Sheets".to_string();
        assert_eq!(find(&docs, &live), None, "two windows, none matches");
        let mut claude = pin("Claude", None);
        claude.title = "Something else".to_string();
        assert_eq!(find(&claude, &live).map(|w| w.hwnd), Some(3), "only one");
        assert_eq!(find(&pin("Code", None), &live), None);
    }

    #[test]
    fn a_web_app_is_not_its_browser() {
        let mut gemini = window(1, "Chrome", "Gemini");
        gemini.app_id = Some("Chrome._crx_gemini".to_string());
        let mut browser = window(2, "Chrome", "Gemini - Google Chrome");
        browser.app_id = Some("Chrome".to_string());
        let mut pinned = pin("Chrome", None);
        pinned.app_id = Some("Chrome._crx_gemini".to_string());
        pinned.title = "Gemini".to_string();
        let live = vec![browser, gemini];
        assert_eq!(find(&pinned, &live).map(|w| w.hwnd), Some(1));
    }

    #[test]
    fn a_renamed_window_matches_by_its_name_too() {
        let mut named = window(4, "Chrome", "Inbox - Google Chrome");
        named.shown = "Work mail".to_string();
        let mut pinned = pin("Chrome", None);
        pinned.title = "work mail".to_string();
        assert_eq!(candidates(&pinned, &[named]).len(), 1);
    }

    #[test]
    fn a_new_pin_drops_the_browser_name_from_the_title() {
        let mut live = window(5, "chrome", "Inbox - Gmail - Google Chrome");
        live.app_id = Some("Chrome".to_string());
        let made = live.to_pin();
        assert_eq!(made.title, "Inbox - Gmail");
        assert_eq!(made.app_name, "Chrome");
        assert_eq!(made.hwnd, Some(5));
        assert!(made.start);
    }

    #[test]
    fn the_check_line_says_how_many_windows_match() {
        assert_eq!(check_line(0).1, CheckTone::Quiet);
        assert_eq!(check_line(1).0, "Matches 1 open window");
        assert_eq!(
            check_line(3),
            (
                "Matches 3 windows, the first one is used".to_string(),
                CheckTone::Warning
            )
        );
    }

    #[test]
    fn each_slot_knows_whether_it_is_open_in_front_closed_or_empty() {
        let mut slots = Slots::default();
        slots.pin(0, pin("Claude", None));
        slots.pin(1, pin("Chrome", None));
        slots.pin(2, pin("Code", None));
        let mut far = window(2, "Chrome", "Docs");
        far.desktop = Some(3);
        far.other_desktop = true;
        far.monitor = Some("Left".to_string());
        let live = vec![window(1, "Claude", "Claude"), far];
        let views = slot_views(&slots, &live, 1);
        assert_eq!(views.len(), SLOTS);
        assert_eq!(views[0].state, State::InFront);
        assert_eq!(views[1].state, State::Open);
        assert_eq!(views[1].badge.as_deref(), Some("D3"));
        assert_eq!(views[1].place, "Desktop 3 \u{b7} Left");
        assert_eq!(views[2].state, State::NotOpen);
        assert_eq!(views[2].hwnd, None);
        assert_eq!(views[3].state, State::Empty);
        assert!(!views[3].is_pinned());
        assert_eq!(views[9].digit, 0);
        assert_eq!(views[0].badge, None, "this desktop needs no badge");
    }

    #[test]
    fn the_place_leaves_out_what_there_is_one_of() {
        assert_eq!(place(None, None), "");
        assert_eq!(place(Some(2), None), "Desktop 2");
        assert_eq!(place(None, Some("Centre")), "Centre");
        assert_eq!(pinned_line(7), "7 of 10 slots pinned");
    }
}
