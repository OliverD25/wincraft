//! What the hover strip does with each look at the mouse and keys, as plain
//! data: when to ask which button the mouse is on, when to open the strip,
//! when to close it, and when Windows' own popup may be hidden. Nothing
//! here calls Windows, so all of it is tested on made-up looks.

use super::keys::{decide, Decision, KeyState, KeyWatch};

/// Left, top, right, bottom in physical screen pixels.
pub type Rect = [i32; 4];
pub type Point = (i32, i32);

/// The mouse must be still this long before UI Automation is asked which
/// button it is on, so moving along the taskbar asks nothing.
pub const LOOKUP_MS: u64 = 60;
/// Moving less than this still counts as resting.
pub const MOVE_PX: i32 = 4;
/// The strip closes this long after the mouse left both the button and
/// the strip.
pub const LEAVE_GRACE_MS: u64 = 500;
/// A strip that has not shown by then is given up on.
pub const OPEN_TIMEOUT_MS: u64 = 3000;
pub const DEFAULT_DELAY_MS: u64 = 400;
pub const MIN_DELAY_MS: u64 = 200;
pub const MAX_DELAY_MS: u64 = 1500;
/// Apps with fewer windows keep Windows' own preview.
pub const MIN_WINDOWS: usize = 2;

pub fn contains(rect: Rect, (x, y): Point) -> bool {
    x >= rect[0] && x < rect[2] && y >= rect[1] && y < rect[3]
}

/// The delay setting in seconds, as milliseconds within the allowed range.
pub fn delay_ms(seconds: Option<f64>) -> u64 {
    match seconds {
        Some(seconds) if seconds.is_finite() => {
            ((seconds * 1000.0).round().max(0.0) as u64).clamp(MIN_DELAY_MS, MAX_DELAY_MS)
        }
        _ => DEFAULT_DELAY_MS,
    }
}

/// A taskbar app button, as UI Automation described it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Button {
    pub app: String,
    pub rect: Rect,
    /// The app's windows on every desktop.
    pub windows: usize,
}

impl Button {
    pub fn eligible(&self) -> bool {
        self.windows >= MIN_WINDOWS
    }
}

/// The answer to a lookup asked for at `at`: the button there, or `None`
/// when the point is not on an app button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub at: Point,
    pub button: Option<Button>,
}

/// One look at the world.
#[derive(Clone, Debug, Default)]
pub struct Look {
    pub now: u64,
    pub cursor: Point,
    pub on_taskbar: bool,
    pub keys: KeyState,
    /// The strip's window, while it is on screen.
    pub strip: Option<Rect>,
    pub answer: Option<Answer>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Ask which button is at this point.
    Lookup(Point),
    Open {
        app: String,
        button: Rect,
    },
    Close,
}

/// When Windows' popup may be hidden: only while the mouse is on this
/// button or in the strip, and only when the key rules allow it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gate {
    pub button: Rect,
    pub strip: Option<Rect>,
    pub keys: KeyWatch,
}

impl Gate {
    pub fn allows(&self, cursor: Point, keys: KeyState, now: u64) -> bool {
        if self.keys.win_was_down() {
            return false;
        }
        let on = contains(self.button, cursor) || self.strip.is_some_and(|s| contains(s, cursor));
        decide(&self.keys.inputs(keys, now, on)) == Decision::Hide
    }
}

#[derive(Clone, Debug, PartialEq)]
enum State {
    /// The mouse is not on a taskbar.
    Idle,
    /// On a taskbar, not yet known to be on an app button.
    Resting { at: Point, since: u64, asked: bool },
    /// Resting on a known button; the strip opens once the delay is over.
    OnButton { button: Button, since: u64 },
    Open {
        button: Button,
        opened_at: u64,
        /// The strip's window has been seen, so its going away later
        /// means the strip closed itself.
        seen: bool,
        left_since: Option<u64>,
        /// A mouse button went down inside the strip and is still down,
        /// as in a drag that may leave it for a moment.
        held_inside: bool,
        /// The mouse resting on another taskbar button, for a move from
        /// one app to the next.
        other: Option<(Point, u64, bool)>,
    },
    /// Done with this hover: nothing happens again until the mouse leaves
    /// this rectangle.
    Done { rect: Option<Rect> },
}

#[derive(Clone, Debug)]
pub struct Machine {
    pub on: bool,
    pub delay_ms: u64,
    state: State,
    keys: KeyWatch,
    mouse_was_down: bool,
    escape_was_down: bool,
}

impl Default for Machine {
    fn default() -> Self {
        Self {
            on: false,
            delay_ms: DEFAULT_DELAY_MS,
            state: State::Idle,
            keys: KeyWatch::default(),
            mouse_was_down: false,
            escape_was_down: false,
        }
    }
}

impl Machine {
    /// Whether the mouse needs a close watch: it is on a taskbar or the
    /// strip is open.
    pub fn engaged(&self) -> bool {
        self.on && self.state != State::Idle
    }

    pub fn is_open(&self) -> bool {
        matches!(self.state, State::Open { .. })
    }

    /// The app the hover is about, for the log.
    pub fn app(&self) -> Option<&str> {
        match &self.state {
            State::OnButton { button, .. } | State::Open { button, .. } => Some(&button.app),
            _ => None,
        }
    }

    /// When Windows' popup may be hidden now, or `None` when it must be
    /// left alone.
    pub fn gate(&self, strip: Option<Rect>) -> Option<Gate> {
        if !self.on {
            return None;
        }
        match &self.state {
            State::OnButton { button, .. } if button.eligible() => Some(Gate {
                button: button.rect,
                strip: None,
                keys: self.keys,
            }),
            State::Open { button, .. } => Some(Gate {
                button: button.rect,
                strip,
                keys: self.keys,
            }),
            _ => None,
        }
    }

    /// Switching off closes an open strip at once.
    pub fn switch(&mut self, on: bool) -> Vec<Command> {
        self.on = on;
        if on {
            return Vec::new();
        }
        let was_open = self.is_open();
        self.state = State::Idle;
        if was_open {
            vec![Command::Close]
        } else {
            Vec::new()
        }
    }

    pub fn step(&mut self, look: &Look) -> Vec<Command> {
        self.keys.see(look.keys, look.now);
        let pressed = look.keys.mouse_button_down && !self.mouse_was_down;
        let escape = look.keys.escape && !self.escape_was_down;
        self.mouse_was_down = look.keys.mouse_button_down;
        self.escape_was_down = look.keys.escape;
        if !self.on {
            self.state = State::Idle;
            return Vec::new();
        }
        let mut out = Vec::new();
        let state = std::mem::replace(&mut self.state, State::Idle);
        self.state = match state {
            State::Idle => Self::rest_or_idle(look),
            State::Done { rect } => match rect {
                Some(rect) if look.on_taskbar && contains(rect, look.cursor) => {
                    State::Done { rect: Some(rect) }
                }
                _ => Self::rest_or_idle(look),
            },
            State::Resting { at, since, asked } => self.resting(look, at, since, asked, &mut out),
            State::OnButton { button, since } => self.on_button(look, button, since, &mut out),
            State::Open {
                button,
                opened_at,
                seen,
                left_since,
                held_inside,
                other,
            } => self.open(
                look,
                OpenState {
                    button,
                    opened_at,
                    seen,
                    left_since,
                    held_inside,
                    other,
                },
                pressed,
                escape,
                &mut out,
            ),
        };
        out
    }

    fn rest_or_idle(look: &Look) -> State {
        if look.on_taskbar {
            State::Resting {
                at: look.cursor,
                since: look.now,
                asked: false,
            }
        } else {
            State::Idle
        }
    }

    fn resting(
        &self,
        look: &Look,
        at: Point,
        since: u64,
        asked: bool,
        out: &mut Vec<Command>,
    ) -> State {
        if !look.on_taskbar {
            return State::Idle;
        }
        if moved(at, look.cursor) {
            return Self::rest_or_idle(look);
        }
        if asked {
            if let Some(answer) = &look.answer {
                if answer.at == at {
                    return match &answer.button {
                        Some(button) if contains(button.rect, look.cursor) => State::OnButton {
                            button: button.clone(),
                            since,
                        },
                        _ => State::Resting { at, since, asked },
                    };
                }
            }
            return State::Resting { at, since, asked };
        }
        if look.now.saturating_sub(since) >= LOOKUP_MS {
            out.push(Command::Lookup(at));
            return State::Resting {
                at,
                since,
                asked: true,
            };
        }
        State::Resting { at, since, asked }
    }

    fn on_button(&self, look: &Look, button: Button, since: u64, out: &mut Vec<Command>) -> State {
        if !look.on_taskbar || !contains(button.rect, look.cursor) {
            return Self::rest_or_idle(look);
        }
        if !button.eligible() {
            return State::OnButton { button, since };
        }
        // The keyboard or a click had a part in this: Windows' popup is
        // theirs, and nothing happens until the mouse leaves the button.
        let inputs = self.keys.inputs(look.keys, look.now, true);
        if decide(&inputs) != Decision::Hide {
            return State::Done {
                rect: Some(button.rect),
            };
        }
        if look.now.saturating_sub(since) < self.delay_ms {
            return State::OnButton { button, since };
        }
        if look.strip.is_some() {
            // Win+Alt+A's strip is already open; this hover leaves it be.
            return State::Done {
                rect: Some(button.rect),
            };
        }
        out.push(Command::Open {
            app: button.app.clone(),
            button: button.rect,
        });
        State::Open {
            button,
            opened_at: look.now,
            seen: false,
            left_since: None,
            held_inside: false,
            other: None,
        }
    }

    fn open(
        &self,
        look: &Look,
        mut open: OpenState,
        pressed: bool,
        escape: bool,
        out: &mut Vec<Command>,
    ) -> State {
        let done = State::Done {
            rect: Some(open.button.rect),
        };
        match look.strip {
            Some(_) => open.seen = true,
            // Closed by itself: a card was clicked, or Esc or a click
            // elsewhere once it had the keyboard.
            None if open.seen => return done,
            None if look.now.saturating_sub(open.opened_at) >= OPEN_TIMEOUT_MS => {
                out.push(Command::Close);
                return done;
            }
            None => {}
        }
        let in_strip = look.strip.is_some_and(|strip| contains(strip, look.cursor));
        let on_button = look.on_taskbar && contains(open.button.rect, look.cursor);
        let keyboard = look.keys.win_down || look.keys.alt_down;
        if escape || keyboard || (pressed && !in_strip) {
            out.push(Command::Close);
            return done;
        }
        if pressed && in_strip {
            open.held_inside = true;
        }
        if !look.keys.mouse_button_down {
            open.held_inside = false;
        }
        if on_button || in_strip || open.held_inside {
            open.left_since = None;
            open.other = None;
            return open.into_state();
        }
        let left = *open.left_since.get_or_insert(look.now);
        if let Some(next) = self.next_button(look, &mut open, out) {
            return next;
        }
        if look.now.saturating_sub(left) >= LEAVE_GRACE_MS {
            out.push(Command::Close);
            return State::Done { rect: None };
        }
        open.into_state()
    }

    /// The mouse went from the open strip's button to another one: the
    /// strip follows it, the way Windows' popup does.
    fn next_button(
        &self,
        look: &Look,
        open: &mut OpenState,
        out: &mut Vec<Command>,
    ) -> Option<State> {
        if !look.on_taskbar {
            open.other = None;
            return None;
        }
        let (at, since, asked) = match open.other {
            Some((at, since, asked)) if !moved(at, look.cursor) => (at, since, asked),
            _ => (look.cursor, look.now, false),
        };
        if !asked {
            let ask = look.now.saturating_sub(since) >= LOOKUP_MS;
            if ask {
                out.push(Command::Lookup(at));
            }
            open.other = Some((at, since, ask));
            return None;
        }
        open.other = Some((at, since, asked));
        let answer = look.answer.as_ref().filter(|answer| answer.at == at)?;
        let button = answer
            .button
            .as_ref()
            .filter(|button| contains(button.rect, look.cursor))?;
        if button.rect == open.button.rect {
            return None;
        }
        let inputs = self.keys.inputs(look.keys, look.now, true);
        if !button.eligible() || decide(&inputs) != Decision::Hide {
            // Windows' own preview takes over for this one.
            out.push(Command::Close);
            return Some(State::OnButton {
                button: button.clone(),
                since,
            });
        }
        out.push(Command::Open {
            app: button.app.clone(),
            button: button.rect,
        });
        Some(State::Open {
            button: button.clone(),
            opened_at: look.now,
            // The strip is on screen already; it only moves.
            seen: open.seen,
            left_since: None,
            held_inside: false,
            other: None,
        })
    }
}

struct OpenState {
    button: Button,
    opened_at: u64,
    seen: bool,
    left_since: Option<u64>,
    held_inside: bool,
    other: Option<(Point, u64, bool)>,
}

impl OpenState {
    fn into_state(self) -> State {
        State::Open {
            button: self.button,
            opened_at: self.opened_at,
            seen: self.seen,
            left_since: self.left_since,
            held_inside: self.held_inside,
            other: self.other,
        }
    }
}

fn moved(from: Point, to: Point) -> bool {
    (from.0 - to.0).abs() > MOVE_PX || (from.1 - to.1).abs() > MOVE_PX
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUTTON: Rect = [1000, 2040, 1048, 2088];
    const NEXT: Rect = [1048, 2040, 1096, 2088];
    const STRIP: Rect = [700, 1700, 1350, 2016];
    const ON_BUTTON: Point = (1020, 2060);
    const IN_STRIP: Point = (1020, 1800);
    const DESKTOP: Point = (500, 500);

    fn button(windows: usize) -> Button {
        Button {
            app: "Chrome.UserData.Profile3".to_string(),
            rect: BUTTON,
            windows,
        }
    }

    fn on() -> Machine {
        Machine {
            on: true,
            ..Machine::default()
        }
    }

    fn look(now: u64, cursor: Point) -> Look {
        Look {
            now,
            cursor,
            on_taskbar: cursor.1 >= 2040,
            ..Look::default()
        }
    }

    fn answered(now: u64, at: Point, button: Option<Button>) -> Look {
        Look {
            answer: Some(Answer { at, button }),
            ..look(now, at)
        }
    }

    /// Rests on the button until the lookup is answered; returns the time.
    fn rest_on(machine: &mut Machine, windows: usize) -> u64 {
        assert!(machine.step(&look(0, ON_BUTTON)).is_empty());
        assert_eq!(
            machine.step(&look(LOOKUP_MS, ON_BUTTON)),
            [Command::Lookup(ON_BUTTON)]
        );
        assert!(machine
            .step(&answered(100, ON_BUTTON, Some(button(windows))))
            .is_empty());
        100
    }

    fn open_strip(machine: &mut Machine) -> u64 {
        rest_on(machine, 3);
        assert_eq!(
            machine.step(&look(DEFAULT_DELAY_MS, ON_BUTTON)),
            [Command::Open {
                app: "Chrome.UserData.Profile3".to_string(),
                button: BUTTON
            }]
        );
        let shown = Look {
            strip: Some(STRIP),
            ..look(DEFAULT_DELAY_MS + 100, ON_BUTTON)
        };
        assert!(machine.step(&shown).is_empty());
        assert!(machine.is_open());
        DEFAULT_DELAY_MS + 100
    }

    fn with_strip(now: u64, cursor: Point) -> Look {
        Look {
            strip: Some(STRIP),
            ..look(now, cursor)
        }
    }

    #[test]
    fn the_delay_comes_from_the_setting_within_its_range() {
        assert_eq!(delay_ms(None), 400);
        assert_eq!(delay_ms(Some(0.4)), 400);
        assert_eq!(delay_ms(Some(1.0)), 1000);
        assert_eq!(delay_ms(Some(0.05)), 200);
        assert_eq!(delay_ms(Some(9.0)), 1500);
        assert_eq!(delay_ms(Some(f64::NAN)), 400);
        assert_eq!(delay_ms(Some(-1.0)), 200);
    }

    #[test]
    fn nothing_happens_away_from_the_taskbar() {
        let mut machine = on();
        for now in (0..2000).step_by(16) {
            assert!(machine.step(&look(now, DESKTOP)).is_empty());
        }
        assert!(!machine.engaged());
    }

    #[test]
    fn moving_along_the_taskbar_asks_nothing() {
        let mut machine = on();
        for (step, now) in (0..1000).step_by(16).enumerate() {
            let x = 100 + step as i32 * 10;
            assert!(machine.step(&look(now, (x, 2060))).is_empty());
        }
        assert!(machine.engaged());
    }

    #[test]
    fn the_strip_opens_after_the_rest_delay() {
        let mut machine = on();
        rest_on(&mut machine, 2);
        assert!(machine.step(&look(300, ON_BUTTON)).is_empty());
        assert_eq!(machine.app(), Some("Chrome.UserData.Profile3"));
        assert_eq!(
            machine.step(&look(400, ON_BUTTON)),
            [Command::Open {
                app: "Chrome.UserData.Profile3".to_string(),
                button: BUTTON
            }]
        );

        let mut slow = on();
        slow.delay_ms = 1000;
        rest_on(&mut slow, 2);
        assert!(slow.step(&look(900, ON_BUTTON)).is_empty());
        assert_eq!(slow.step(&look(1000, ON_BUTTON)).len(), 1);
    }

    #[test]
    fn small_moves_on_the_button_do_not_restart_the_delay() {
        let mut machine = on();
        rest_on(&mut machine, 2);
        assert!(machine.step(&look(200, (1030, 2050))).is_empty());
        assert_eq!(machine.step(&look(400, (1040, 2070))).len(), 1);
    }

    #[test]
    fn an_app_with_one_window_keeps_windows_preview() {
        let mut machine = on();
        rest_on(&mut machine, 1);
        for now in (100..3000).step_by(16) {
            assert!(machine.step(&look(now, ON_BUTTON)).is_empty());
        }
        assert_eq!(machine.gate(None), None, "Windows' popup is never hidden");
    }

    #[test]
    fn a_point_that_is_not_an_app_button_opens_nothing() {
        let mut machine = on();
        machine.step(&look(0, ON_BUTTON));
        machine.step(&look(LOOKUP_MS, ON_BUTTON));
        assert!(machine.step(&answered(100, ON_BUTTON, None)).is_empty());
        for now in (100..2000).step_by(16) {
            assert!(machine.step(&look(now, ON_BUTTON)).is_empty());
        }
        assert_eq!(machine.gate(None), None);
    }

    #[test]
    fn leaving_before_the_delay_opens_nothing() {
        let mut machine = on();
        rest_on(&mut machine, 2);
        assert!(machine.step(&look(200, DESKTOP)).is_empty());
        assert!(machine.step(&look(800, DESKTOP)).is_empty());
        assert!(!machine.engaged());
    }

    #[test]
    fn the_popup_may_be_hidden_only_on_an_eligible_button() {
        let mut machine = on();
        rest_on(&mut machine, 2);
        let gate = machine.gate(None).expect("eligible button");
        assert!(gate.allows(ON_BUTTON, KeyState::default(), 150));
        assert!(!gate.allows(DESKTOP, KeyState::default(), 150));
        let win = KeyState {
            win_down: true,
            ..KeyState::default()
        };
        assert!(
            !gate.allows(ON_BUTTON, win, 150),
            "never with a Windows key"
        );
        let alt = KeyState {
            alt_down: true,
            ..KeyState::default()
        };
        assert!(!gate.allows(ON_BUTTON, alt, 150), "never during Alt+Tab");
        let click = KeyState {
            mouse_button_down: true,
            ..KeyState::default()
        };
        assert!(!gate.allows(ON_BUTTON, click, 150));
        let mut held = gate;
        held.keys.see(win, 140);
        assert!(
            !held.allows(ON_BUTTON, KeyState::default(), 150),
            "a Windows key let go since the last look still counts"
        );
    }

    #[test]
    fn the_gate_covers_the_strip_while_it_is_open() {
        let mut machine = on();
        let now = open_strip(&mut machine);
        let gate = machine.gate(Some(STRIP)).expect("open");
        assert!(gate.allows(IN_STRIP, KeyState::default(), now));
        assert!(gate.allows(ON_BUTTON, KeyState::default(), now));
        assert!(!gate.allows(DESKTOP, KeyState::default(), now));
    }

    #[test]
    fn win_number_while_resting_leaves_everything_to_windows() {
        let mut machine = on();
        rest_on(&mut machine, 3);
        let combo = Look {
            keys: KeyState {
                win_down: true,
                digit_down: true,
                ..KeyState::default()
            },
            ..look(200, ON_BUTTON)
        };
        assert!(machine.step(&combo).is_empty());
        assert_eq!(machine.gate(None), None);
        // Still on the button long after: no strip until the mouse leaves.
        for now in (300..4000).step_by(16) {
            assert!(machine.step(&look(now, ON_BUTTON)).is_empty());
        }
        machine.step(&look(4000, DESKTOP));
        assert!(!machine.engaged());
    }

    #[test]
    fn a_recent_key_or_alt_also_leaves_it_to_windows() {
        for keys in [
            KeyState {
                alt_down: true,
                ..KeyState::default()
            },
            KeyState {
                nav_down: true,
                ..KeyState::default()
            },
            KeyState {
                mouse_button_down: true,
                ..KeyState::default()
            },
        ] {
            let mut machine = on();
            rest_on(&mut machine, 3);
            let pressed = Look {
                keys,
                ..look(200, ON_BUTTON)
            };
            machine.step(&pressed);
            for now in (300..2000).step_by(16) {
                assert!(machine.step(&look(now, ON_BUTTON)).is_empty(), "{keys:?}");
            }
        }
    }

    #[test]
    fn the_strip_stays_while_the_mouse_moves_up_into_it() {
        let mut machine = on();
        let now = open_strip(&mut machine);
        // Across the gap above the taskbar, then into the strip.
        assert!(machine.step(&with_strip(now + 50, (1020, 2030))).is_empty());
        assert!(machine
            .step(&with_strip(now + 200, (1020, 2020)))
            .is_empty());
        for t in (now + 300..now + 3000).step_by(16) {
            assert!(machine.step(&with_strip(t, IN_STRIP)).is_empty());
        }
        assert!(machine.is_open());
    }

    #[test]
    fn the_strip_closes_half_a_second_after_the_mouse_leaves_both() {
        let mut machine = on();
        let now = open_strip(&mut machine);
        assert!(machine.step(&with_strip(now + 16, DESKTOP)).is_empty());
        assert!(machine
            .step(&with_strip(now + 16 + LEAVE_GRACE_MS - 1, DESKTOP))
            .is_empty());
        assert_eq!(
            machine.step(&with_strip(now + 16 + LEAVE_GRACE_MS, DESKTOP)),
            [Command::Close]
        );
        assert!(!machine.is_open());
        assert_eq!(machine.gate(Some(STRIP)), None);
    }

    #[test]
    fn coming_back_within_the_grace_keeps_it_open() {
        let mut machine = on();
        let now = open_strip(&mut machine);
        machine.step(&with_strip(now + 16, DESKTOP));
        assert!(machine.step(&with_strip(now + 300, IN_STRIP)).is_empty());
        assert!(machine.step(&with_strip(now + 700, DESKTOP)).is_empty());
        assert!(machine.step(&with_strip(now + 1100, DESKTOP)).is_empty());
        assert_eq!(
            machine.step(&with_strip(now + 1200, DESKTOP)),
            [Command::Close]
        );
    }

    #[test]
    fn escape_closes_and_the_strip_waits_for_the_mouse_to_leave() {
        let mut machine = on();
        let now = open_strip(&mut machine);
        let escape = Look {
            keys: KeyState {
                escape: true,
                ..KeyState::default()
            },
            ..with_strip(now + 16, ON_BUTTON)
        };
        assert_eq!(machine.step(&escape), [Command::Close]);
        for t in (now + 32..now + 3000).step_by(16) {
            assert!(machine.step(&look(t, ON_BUTTON)).is_empty());
        }
        // Off the button and back: a new hover.
        machine.step(&look(now + 3000, (1200, 2060)));
        machine.step(&look(now + 3016, ON_BUTTON));
        assert_eq!(
            machine.step(&look(now + 3016 + LOOKUP_MS, ON_BUTTON)),
            [Command::Lookup(ON_BUTTON)]
        );
    }

    #[test]
    fn a_click_elsewhere_closes_but_a_click_in_the_strip_does_not() {
        let mut machine = on();
        let now = open_strip(&mut machine);
        let press = |t, at| Look {
            keys: KeyState {
                mouse_button_down: true,
                ..KeyState::default()
            },
            ..with_strip(t, at)
        };
        // A drag that starts in the strip and goes outside for a while.
        assert!(machine.step(&press(now + 16, IN_STRIP)).is_empty());
        assert!(machine.step(&press(now + 400, DESKTOP)).is_empty());
        assert!(machine.step(&press(now + 1200, DESKTOP)).is_empty());
        assert!(machine.step(&with_strip(now + 1300, IN_STRIP)).is_empty());
        assert!(machine.is_open());

        assert!(machine.step(&with_strip(now + 1400, DESKTOP)).is_empty());
        assert_eq!(machine.step(&press(now + 1416, DESKTOP)), [Command::Close]);
    }

    #[test]
    fn a_click_on_the_button_closes_and_leaves_it_to_windows() {
        let mut machine = on();
        let now = open_strip(&mut machine);
        let press = Look {
            keys: KeyState {
                mouse_button_down: true,
                ..KeyState::default()
            },
            ..with_strip(now + 16, ON_BUTTON)
        };
        assert_eq!(machine.step(&press), [Command::Close]);
        assert_eq!(machine.gate(None), None);
        for t in (now + 32..now + 2000).step_by(16) {
            assert!(machine.step(&look(t, ON_BUTTON)).is_empty());
        }
    }

    #[test]
    fn a_windows_key_closes_the_strip_at_once() {
        let mut machine = on();
        let now = open_strip(&mut machine);
        let win = Look {
            keys: KeyState {
                win_down: true,
                ..KeyState::default()
            },
            ..with_strip(now + 16, IN_STRIP)
        };
        assert_eq!(machine.step(&win), [Command::Close]);
        assert_eq!(machine.gate(Some(STRIP)), None);
    }

    #[test]
    fn a_strip_that_closed_itself_is_not_opened_again_on_the_same_button() {
        let mut machine = on();
        let now = open_strip(&mut machine);
        // A card was clicked: the strip went away by itself.
        assert!(machine.step(&look(now + 16, ON_BUTTON)).is_empty());
        assert!(!machine.is_open());
        for t in (now + 32..now + 2000).step_by(16) {
            assert!(machine.step(&look(t, ON_BUTTON)).is_empty());
        }
    }

    #[test]
    fn a_strip_that_never_shows_is_given_up() {
        let mut machine = on();
        rest_on(&mut machine, 2);
        machine.step(&look(400, ON_BUTTON));
        assert!(machine
            .step(&look(400 + OPEN_TIMEOUT_MS - 1, ON_BUTTON))
            .is_empty());
        assert_eq!(
            machine.step(&look(400 + OPEN_TIMEOUT_MS, ON_BUTTON)),
            [Command::Close]
        );
    }

    #[test]
    fn win_alt_a_strip_already_open_is_left_alone() {
        let mut machine = on();
        rest_on(&mut machine, 2);
        assert!(machine.step(&with_strip(400, ON_BUTTON)).is_empty());
        assert!(!machine.is_open());
    }

    #[test]
    fn the_strip_follows_the_mouse_to_the_next_app() {
        let mut machine = on();
        let now = open_strip(&mut machine);
        let next_point = (1070, 2060);
        assert!(machine.step(&with_strip(now + 16, next_point)).is_empty());
        assert_eq!(
            machine.step(&with_strip(now + 16 + LOOKUP_MS, next_point)),
            [Command::Lookup(next_point)]
        );
        let other = Button {
            app: "Telegram.TelegramDesktop".to_string(),
            rect: NEXT,
            windows: 2,
        };
        let answer = Look {
            answer: Some(Answer {
                at: next_point,
                button: Some(other),
            }),
            ..with_strip(now + 120, next_point)
        };
        assert_eq!(
            machine.step(&answer),
            [Command::Open {
                app: "Telegram.TelegramDesktop".to_string(),
                button: NEXT
            }]
        );
        assert_eq!(machine.app(), Some("Telegram.TelegramDesktop"));
        assert!(machine.step(&with_strip(now + 2000, next_point)).is_empty());
    }

    #[test]
    fn moving_to_a_one_window_app_closes_the_strip() {
        let mut machine = on();
        let now = open_strip(&mut machine);
        let next_point = (1070, 2060);
        machine.step(&with_strip(now + 16, next_point));
        machine.step(&with_strip(now + 16 + LOOKUP_MS, next_point));
        let single = Look {
            answer: Some(Answer {
                at: next_point,
                button: Some(Button {
                    app: "Notepad".to_string(),
                    rect: NEXT,
                    windows: 1,
                }),
            }),
            ..with_strip(now + 120, next_point)
        };
        assert_eq!(machine.step(&single), [Command::Close]);
        assert_eq!(machine.gate(None), None);
    }

    #[test]
    fn switching_off_closes_at_once_and_stops_everything() {
        let mut machine = on();
        open_strip(&mut machine);
        assert_eq!(machine.switch(false), [Command::Close]);
        assert!(!machine.engaged());
        assert_eq!(machine.gate(Some(STRIP)), None);
        assert!(machine.step(&look(5000, ON_BUTTON)).is_empty());
        assert!(machine.step(&look(6000, ON_BUTTON)).is_empty());
        assert!(!machine.engaged());

        let mut idle = on();
        assert!(idle.switch(false).is_empty());
        assert!(idle.switch(true).is_empty());
        assert!(idle.on);
    }
}
