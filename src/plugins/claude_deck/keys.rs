//! The keyboard in ClaudeDeck's list (design section 9). Up and Down pick a
//! chat or a group, Home and End the first and the last, 1 to 9 the chat
//! with that digit, Left and Right fold and unfold. Enter opens the chat
//! (or folds the group), Alt+Up and Alt+Down move the picked chat or group,
//! F2 renames the picked group, Ctrl+C copies the picked chat's name, and
//! the Menu key or Shift+F10 opens its menu. Ctrl+M switches mini mode, and
//! Esc gives the keyboard back to the window that had it before the hotkey.

use egui::{Key, Ui};
use windows_sys::Win32::Foundation::HWND;

use crate::core::clipboard;
use crate::core::ui_bridge::HostChannel;

use super::drop::{self, kinds, Placed};
use super::edit;
use super::list::start_rename;
use super::model::{Fold, Line};
use super::select::DIGITS;
use super::window::{bring_forward, DeckUi, Pick};

/// The keys pressed this frame that the list acts on.
#[derive(Default)]
struct Pressed {
    down: bool,
    up: bool,
    enter: bool,
    alt: bool,
    rename: bool,
    copy: bool,
    menu: bool,
    home: bool,
    end: bool,
    left: bool,
    right: bool,
    digit: Option<usize>,
    esc: bool,
    mini: bool,
}

impl Pressed {
    /// A key that picks a line: the list then shows the digits.
    fn picks(&self) -> bool {
        self.up || self.down || self.home || self.end || self.digit.is_some()
    }
}

/// What the window does after the keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Asked {
    /// Esc: give the keyboard back.
    pub give_back: bool,
    /// Ctrl+M: switch mini mode.
    pub mini: bool,
}

/// The lines that can be picked, top to bottom.
pub fn picks(lines: &[Line]) -> Vec<Pick> {
    lines
        .iter()
        .filter_map(|line| match line {
            Line::Group(group) => Some(Pick::Group(group.key.clone())),
            Line::Chat(chat) => Some(Pick::Chat(chat.chat.id.clone())),
            _ => None,
        })
        .collect()
}

/// The pick after (or before, `up`) the current one; the first or the last
/// when nothing is picked.
pub fn step(picks: &[Pick], current: Option<&Pick>, up: bool) -> Option<Pick> {
    if picks.is_empty() {
        return None;
    }
    let at = current.and_then(|pick| picks.iter().position(|p| p == pick));
    let next = match (up, at) {
        (false, Some(at)) => (at + 1).min(picks.len() - 1),
        (false, None) => 0,
        (true, Some(at)) => at.saturating_sub(1),
        (true, None) => picks.len() - 1,
    };
    picks.get(next).cloned()
}

fn read(ui: &Ui, state: &mut DeckUi) -> Pressed {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_APPS};
    // egui has no Menu key, so its state is read from Windows. It is the
    // state of this thread's keys, so only a focused window acts on it.
    let menu_down = unsafe { GetKeyState(i32::from(VK_APPS)) } < 0;
    let menu_pressed = menu_down && !state.menu_key_down;
    state.menu_key_down = menu_down;
    ui.ctx().input(|i| {
        let shift_f10 = i.events.iter().any(|event| {
            matches!(event, egui::Event::Key { key: Key::F10, pressed: true, modifiers, .. } if modifiers.shift)
        });
        Pressed {
            down: i.key_pressed(Key::ArrowDown),
            up: i.key_pressed(Key::ArrowUp),
            enter: i.key_pressed(Key::Enter),
            alt: i.modifiers.alt,
            rename: i.key_pressed(Key::F2),
            copy: i.events.iter().any(|e| matches!(e, egui::Event::Copy)),
            menu: (menu_pressed && i.focused) || shift_f10,
            home: i.key_pressed(Key::Home),
            end: i.key_pressed(Key::End),
            left: i.key_pressed(Key::ArrowLeft),
            right: i.key_pressed(Key::ArrowRight),
            digit: (!i.modifiers.any())
                .then(|| DIGITS.iter().position(|key| i.key_pressed(*key)))
                .flatten(),
            esc: i.key_pressed(Key::Escape),
            mini: i.modifiers.command && i.key_pressed(Key::M),
        }
    })
}

/// The keys of one frame. `menu_was_open`: a menu was open when the frame
/// began, so an Esc that closed it is not the list's.
pub fn keys(
    ui: &Ui,
    lines: &[Line],
    state: &mut DeckUi,
    to_host: &HostChannel,
    menu_was_open: bool,
) -> Asked {
    // A menu, a rename or a drag has the keys while it is on.
    let renamed = std::mem::take(&mut state.renamed);
    let busy =
        menu_was_open || egui::Popup::is_any_open(ui.ctx()) || state.renaming.is_some() || renamed;
    if busy || state.drag.is_some() {
        return Asked::default();
    }
    let pressed = read(ui, state);
    let asked = Asked {
        give_back: pressed.esc,
        mini: pressed.mini,
    };
    let picks = picks(lines);
    if pressed.picks() {
        state.keyboard = true;
    }
    if pressed.alt && (pressed.up || pressed.down) {
        move_picked(lines, state, pressed.up);
        return asked;
    }
    let next = if pressed.up || pressed.down {
        step(&picks, state.selected.as_ref(), pressed.up)
    } else if pressed.home {
        picks.first().cloned()
    } else if pressed.end {
        picks.last().cloned()
    } else {
        pressed
            .digit
            .and_then(|n| state.digit_rows.get(n))
            .map(|id| Pick::Chat(id.clone()))
    };
    if let Some(next) = next {
        state.selected = Some(next);
        state.scroll_to_selected = true;
    }
    let Some(picked) = state.selected.clone().filter(|pick| picks.contains(pick)) else {
        return asked;
    };
    if pressed.left || pressed.right {
        fold_picked(lines, state, &picked, pressed.right);
    }
    if pressed.menu {
        state.menu_for = Some(picked.clone());
        state.scroll_to_selected = true;
    }
    match picked {
        Pick::Group(id) => {
            if pressed.enter {
                edit::fold_group(&mut state.saved, &id);
            }
            if pressed.rename {
                let name = state.saved.groups.iter().find(|g| g.id == id);
                if let Some(name) = name.map(|g| g.name.clone()) {
                    start_rename(state, &id, &name);
                }
            }
        }
        Pick::Chat(id) => {
            let line = lines.iter().find_map(|line| match line {
                Line::Chat(chat) if chat.chat.id == id => Some(chat),
                _ => None,
            });
            let Some(line) = line else {
                return asked;
            };
            if pressed.enter {
                if line.stand_in {
                    edit::fold_chat(&mut state.saved, &id);
                } else {
                    bring_forward(line.chat.window, to_host);
                }
            }
            if pressed.copy && !line.stand_in {
                let _ = clipboard::put_text(state.window as HWND, &line.chat.title);
            }
        }
    }
    asked
}

/// Left folds the picked group or chat's forks, Right unfolds them; a line
/// already that way stays.
fn fold_picked(lines: &[Line], state: &mut DeckUi, picked: &Pick, open: bool) {
    for line in lines {
        match (line, picked) {
            (Line::Group(g), Pick::Group(id)) if &g.key == id && g.folded == open => {
                edit::fold_group(&mut state.saved, id);
            }
            (Line::Chat(c), Pick::Chat(id)) if &c.chat.id == id => {
                let folded = matches!(c.fold, Fold::Folded(_));
                if c.fold != Fold::None && folded == open {
                    edit::fold_chat(&mut state.saved, id);
                }
            }
            _ => {}
        }
    }
}

/// Alt+Up or Alt+Down: the picked chat moves past its neighbour (into the
/// next group at a group's edge); a picked group moves one place.
fn move_picked(lines: &[Line], state: &mut DeckUi, up: bool) {
    match state.selected.clone() {
        Some(Pick::Chat(key)) => {
            let placed: Vec<Placed> = kinds(lines)
                .into_iter()
                .map(|kind| Placed {
                    kind,
                    top: 0.0,
                    bottom: 0.0,
                })
                .collect();
            if let Some(spot) = drop::step_chat(&placed, &key, up) {
                edit::place_chat(&mut state.saved, &key, &spot);
            }
        }
        Some(Pick::Group(id)) => edit::step_group(&mut state.saved, &id, up),
        None => {}
    }
    state.scroll_to_selected = true;
}
