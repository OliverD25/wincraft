//! The keyboard in ClaudeDeck's list. Up and Down pick a chat or a group,
//! Enter opens the chat (or folds the group), Alt+Up and Alt+Down move the
//! picked chat or group, F2 renames the picked group, Ctrl+C copies the
//! picked chat's name, and the Menu key or Shift+F10 opens its menu. Stage
//! 4 adds the rest (1-9, Esc).

use egui::{Key, Ui};
use windows_sys::Win32::Foundation::HWND;

use crate::core::clipboard;
use crate::core::ui_bridge::HostChannel;

use super::drop::{self, kinds, Placed};
use super::edit;
use super::list::start_rename;
use super::model::Line;
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
        }
    })
}

pub fn keys(ui: &Ui, lines: &[Line], state: &mut DeckUi, to_host: &HostChannel) {
    // A menu, a rename or a drag has the keys while it is on.
    let renamed = std::mem::take(&mut state.renamed);
    let busy = egui::Popup::is_any_open(ui.ctx()) || state.renaming.is_some() || renamed;
    if busy || state.drag.is_some() {
        return;
    }
    let pressed = read(ui, state);
    let picks = picks(lines);
    if pressed.alt && (pressed.up || pressed.down) {
        move_picked(lines, state, pressed.up);
        return;
    }
    if pressed.up || pressed.down {
        if let Some(next) = step(&picks, state.selected.as_ref(), pressed.up) {
            state.selected = Some(next);
            state.scroll_to_selected = true;
        }
    }
    let Some(picked) = state.selected.clone().filter(|pick| picks.contains(pick)) else {
        return;
    };
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
                return;
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
