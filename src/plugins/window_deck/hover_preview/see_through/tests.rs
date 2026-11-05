use super::super::machine::{Answer, Button, Look, Machine, Point, Rect, LOOKUP_MS};
use super::*;
use crate::core::taskbar_popup::keys::KeyState;

const TASKBAR: Rect = [0, 2040, 3840, 2088];
const BUTTON: Rect = [1000, 2040, 1048, 2088];
const ON_BUTTON: Point = (1020, 2060);
const DESKTOP: Point = (500, 500);

fn look(now: u64, cursor: Point) -> Look {
    Look {
        now,
        cursor,
        on_taskbar: cursor.1 >= 2040,
        taskbar: (cursor.1 >= 2040).then_some(TASKBAR),
        ..Look::default()
    }
}

fn on() -> Machine {
    let mut machine = Machine::default();
    machine.switch(true);
    machine
}

fn see_through(machine: &Machine, look: &Look) -> bool {
    machine.see_through(look.strip, look.cursor, look.keys, look.now)
}

/// Rests on the button until the lookup says what is there.
fn answered(machine: &mut Machine, button: Option<Button>) -> Look {
    machine.step(&look(0, ON_BUTTON));
    machine.step(&look(LOOKUP_MS, ON_BUTTON));
    let answer = Look {
        answer: Some(Answer {
            at: ON_BUTTON,
            button,
        }),
        ..look(100, ON_BUTTON)
    };
    machine.step(&answer);
    look(110, ON_BUTTON)
}

fn app(windows: usize) -> Option<Button> {
    Some(Button {
        app: "Notes".to_string(),
        rect: BUTTON,
        windows,
    })
}

#[test]
fn a_windowdeck_button_keeps_the_popup_see_through() {
    let mut machine = on();
    let now = answered(&mut machine, app(3));
    machine.step(&now);
    assert!(see_through(&machine, &now));
}

#[test]
fn a_button_not_known_yet_is_see_through_until_the_answer() {
    let mut machine = on();
    let first = look(0, ON_BUTTON);
    machine.step(&first);
    assert!(see_through(&machine, &first));
}

#[test]
fn a_one_window_app_or_no_app_button_shows_the_popup() {
    let mut machine = on();
    let now = answered(&mut machine, app(1));
    machine.step(&now);
    assert!(!see_through(&machine, &now), "one window");

    let mut machine = on();
    let now = answered(&mut machine, None);
    machine.step(&now);
    assert!(!see_through(&machine, &now), "no app button");
}

#[test]
fn a_windows_key_or_a_click_shows_the_popup() {
    let mut machine = on();
    let win = Look {
        keys: KeyState {
            win_down: true,
            ..KeyState::default()
        },
        ..look(0, ON_BUTTON)
    };
    machine.step(&win);
    assert!(!see_through(&machine, &win), "Win+number is Windows' own");

    let mut machine = on();
    let click = Look {
        keys: KeyState {
            mouse_button_down: true,
            ..KeyState::default()
        },
        ..look(0, ON_BUTTON)
    };
    machine.step(&click);
    assert!(!see_through(&machine, &click));
}

#[test]
fn leaving_the_taskbar_or_switching_off_ends_it() {
    let mut machine = on();
    let now = answered(&mut machine, app(3));
    machine.step(&now);
    let away = look(200, DESKTOP);
    machine.step(&away);
    assert!(!see_through(&machine, &away), "left the taskbar");

    let mut machine = on();
    let now = answered(&mut machine, app(3));
    machine.step(&now);
    machine.switch(false);
    assert!(!see_through(&machine, &now), "switched off");
}

fn hwnd(n: usize) -> HWND {
    n as HWND
}

#[test]
fn only_new_popups_are_made_see_through_and_all_are_put_back() {
    let known = [hwnd(1), hwnd(2)];
    assert_eq!(changes(true, &known, &[]), (known.to_vec(), Vec::new()));
    assert_eq!(
        changes(true, &known, &[hwnd(1)]),
        (vec![hwnd(2)], Vec::new()),
        "the one already see-through is left as it is"
    );
    assert_eq!(
        changes(false, &known, &known),
        (Vec::new(), known.to_vec()),
        "leaving, switching off and teardown put every one back"
    );
    assert_eq!(changes(false, &known, &[]), (Vec::new(), Vec::new()));
}
