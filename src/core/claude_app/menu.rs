//! Rename in the Claude app. The app has no link or command for it, so
//! WinCraft presses the app's own controls, but only after each check in
//! `guards.rs` passed. A chat with its own window is renamed through the
//! rename button of that window's header. Any other chat is first shown in
//! the main window with its link; then its button in the list must be the
//! only one with its exact name (or the selected one) and take the focus,
//! the menu must have the exact Rename item, and the edit box with the
//! focus must hold the chat's name. Any failed check stops the rename with
//! a notice. The app's own files are never written: the app does not
//! notice that.

use std::time::Duration;

use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

use crate::core::uia::{self, Automation};
use crate::core::{com, instance, window_actions};

use super::guards::{self, Stop};
use super::keys;
use super::{app_windows, find_named, home_window, on_com_thread, poll, Kind, Real, Tree};

const RENAME: &str = "Rename";
const LIST_WAIT: Duration = Duration::from_secs(2);
const MENU_WAIT: Duration = Duration::from_secs(1);
const HEADER_WAIT: Duration = Duration::from_millis(1500);
const FIELD_WAIT: Duration = Duration::from_secs(1);
const STEP: Duration = Duration::from_millis(100);
/// The time the app needs to take the focus and the keys.
const SETTLE: Duration = Duration::from_millis(200);

pub fn rename_failed_notice(title: &str) -> String {
    format!("ClaudeDeck could not rename \"{title}\" in the Claude app. Rename it there.")
}

/// The name of the button that opens a chat's menu.
pub fn more_options_name(title: &str) -> String {
    format!("More options for {}", title.trim())
}

/// The name of the button in the open chat's header that renames it.
pub fn rename_header_name(title: &str) -> String {
    format!("{}, rename session", title.trim())
}

/// Opens the chat's menu through its "More options" button. False when the
/// chat has no such button.
pub fn open_with_more_options<T: Tree>(
    tree: &T,
    root: &T::Node,
    title: &str,
) -> Result<bool, String> {
    match find_named(tree, root, Kind::Button, &more_options_name(title)) {
        Some(more) => tree.expand(&more.node).map(|()| true),
        None => Ok(false),
    }
}

/// Presses the rename button of the open chat's header, waiting for it.
/// False when it does not show up.
pub fn press_rename_header<T: Tree>(
    tree: &T,
    root: &T::Node,
    title: &str,
    wait: Duration,
    every: Duration,
) -> Result<bool, String> {
    let name = rename_header_name(title);
    let header = poll(wait, every, || find_named(tree, root, Kind::Button, &name));
    match header {
        Some(found) => tree.invoke(&found.node).map(|()| true),
        None => Ok(false),
    }
}

/// The Rename item, waited for in every window of the app.
fn wait_for_item(automation: &Automation) -> Option<com::ComPtr> {
    let tree = Real(automation);
    poll(MENU_WAIT, STEP, || {
        let roots: Vec<com::ComPtr> = app_windows()
            .into_iter()
            .filter_map(|window| automation.element_from_handle(window).ok())
            .collect();
        guards::menu_item(&tree, &roots, RENAME)
    })
}

/// Presses a menu item. When Invoke is not supported, the letter the app
/// shows beside it is pressed (never Delete's).
fn press(item: &com::ComPtr, name: &str) -> Result<(), String> {
    match uia::invoke(item) {
        Ok(()) => Ok(()),
        Err(reason) => match keys::menu_letter(name) {
            Some(letter) if keys::chord(&[letter]) => {
                log::info!("ClaudeDeck: \"{name}\" pressed by its letter ({reason})");
                Ok(())
            }
            _ => Err(reason),
        },
    }
}

/// The rename box, waited for: the focused edit box that holds the name.
fn wait_for_name_box(automation: &Automation, title: &str) -> Result<com::ComPtr, Stop> {
    let tree = Real(automation);
    poll(FIELD_WAIT, STEP, || {
        guards::name_box(&tree, automation.focused_element(), title).ok()
    })
    .ok_or(Stop::NoNameBox)
}

/// Puts the new name into the rename box, which has the focus, and saves
/// it with Enter.
fn type_name(field: &com::ComPtr, new_name: &str) -> Result<(), Stop> {
    if let Err(reason) = uia::set_value(field, new_name) {
        log::info!("ClaudeDeck: typing the name, as SetValue failed: {reason}");
        if !keys::chord(&[keys::CONTROL, keys::KEY_A]) || !keys::type_text(new_name) {
            return Err(Stop::KeysRefused);
        }
    }
    if keys::chord(&[keys::RETURN]) {
        Ok(())
    } else {
        Err(Stop::KeysRefused)
    }
}

/// The rename box is up: a failure from here on closes what opened with
/// one Esc.
fn finish(automation: &Automation, title: &str, new_name: &str) -> Result<(), Stop> {
    let field = wait_for_name_box(automation, title);
    let typed = field.and_then(|field| type_name(&field, new_name));
    if typed.is_err() {
        keys::chord(&[keys::ESCAPE]);
    }
    typed
}

/// A chat with a window of its own: the rename button of its header.
fn rename_in_own_window(
    automation: &Automation,
    hwnd: isize,
    title: &str,
    new_name: &str,
    dry: bool,
) -> Result<(), Stop> {
    let hwnd = hwnd as windows_sys::Win32::Foundation::HWND;
    let tree = Real(automation);
    let root = automation
        .element_from_handle(hwnd)
        .map_err(|_| Stop::NoHeaderButton)?;
    if dry {
        let found = find_named(&tree, &root, Kind::Button, &rename_header_name(title));
        log::info!(
            "read-only test instance: renaming \"{title}\" in its own window skipped (rename button found: {})",
            found.is_some()
        );
        return Ok(());
    }
    window_actions::activate(hwnd);
    match press_rename_header(&tree, &root, title, HEADER_WAIT, STEP) {
        Ok(true) => {}
        Ok(false) => return Err(Stop::NoHeaderButton),
        Err(reason) => {
            log::info!("ClaudeDeck: rename \"{title}\": the header button: {reason}");
            return Err(Stop::NoHeaderButton);
        }
    }
    finish(automation, title, new_name)
}

/// A chat with no window of its own: shown with its link, then renamed
/// through its menu in the list.
fn rename_in_list(
    automation: &Automation,
    id: &str,
    title: &str,
    new_name: &str,
    dry: bool,
) -> Result<(), Stop> {
    if super::chat_link(id).is_none() {
        return Err(Stop::NoLink);
    }
    let tree = Real(automation);
    if dry {
        let found = super::locate(automation)
            .map(|(_, root)| guards::chat_button(&tree, &root, title).map(|_| ()));
        log::info!(
            "read-only test instance: renaming \"{title}\" skipped (its button in the list: {found:?})"
        );
        return Ok(());
    }
    if !super::show_chat(id) {
        return Err(Stop::LinkFailed);
    }
    let (home, root) = home_window(automation).ok_or(Stop::NoHomeWindow)?;
    window_actions::activate(home);
    let button = poll(LIST_WAIT, STEP, || {
        guards::chat_button(&tree, &root, title).ok()
    })
    .map_or_else(|| guards::chat_button(&tree, &root, title), Ok)?;
    if let Err(reason) = uia::set_focus(&button) {
        log::info!("ClaudeDeck: rename \"{title}\": SetFocus on its button: {reason}");
    }
    std::thread::sleep(SETTLE);
    let in_front = unsafe { GetForegroundWindow() } == home;
    guards::ready_for_keys(tree.focused(&button), in_front)?;
    let by_more_options = open_with_more_options(&tree, &root, title).unwrap_or_else(|reason| {
        log::info!("ClaudeDeck: rename \"{title}\": More options: {reason}");
        false
    });
    if !by_more_options && !keys::chord(&[keys::SHIFT, keys::F10]) {
        return Err(Stop::KeysRefused);
    }
    let Some(item) = wait_for_item(automation) else {
        keys::chord(&[keys::ESCAPE]);
        return Err(Stop::NoRenameItem);
    };
    if let Err(reason) = press(&item, RENAME) {
        log::info!("ClaudeDeck: rename \"{title}\": pressing Rename: {reason}");
        keys::chord(&[keys::ESCAPE]);
        return Err(Stop::NoRenameItem);
    }
    finish(automation, title, new_name)
}

fn rename(
    id: &str,
    title: &str,
    new_name: &str,
    window: Option<isize>,
    dry: bool,
) -> Result<Option<String>, String> {
    log::info!(
        "ClaudeDeck: renaming \"{title}\" to \"{new_name}\" in the Claude app (own window: {}, read-only: {dry})",
        window.is_some()
    );
    let automation = Automation::new().map_err(|reason| {
        log::warn!("ClaudeDeck: rename \"{title}\": UI Automation failed: {reason}");
        rename_failed_notice(title)
    })?;
    let done = match window {
        Some(hwnd) => rename_in_own_window(&automation, hwnd, title, new_name, dry),
        None => rename_in_list(&automation, id, title, new_name, dry),
    };
    match done {
        Ok(()) => {
            log::info!("ClaudeDeck: rename \"{title}\": done");
            Ok(None)
        }
        Err(stop) => {
            log::warn!(
                "ClaudeDeck: rename \"{title}\" stopped: {} ({stop:?})",
                stop.reason()
            );
            Err(rename_failed_notice(title))
        }
    }
}

/// Renames the chat in the Claude app. `say` gets a notice only when a
/// check stopped the rename.
pub fn rename_chat(
    id: &str,
    title: &str,
    new_name: &str,
    window: Option<isize>,
    say: impl FnOnce(String) + Send + 'static,
) {
    let new_name = new_name.trim().to_string();
    if new_name.is_empty() {
        log::info!("ClaudeDeck: rename \"{title}\": the new name is empty; nothing done");
        return;
    }
    if cfg!(test) {
        return;
    }
    let dry = instance::is_test();
    let id = id.to_string();
    let title = title.trim().to_string();
    let failed = rename_failed_notice(&title);
    on_com_thread(title, failed, say, move |title| {
        rename(&id, title, &new_name, window, dry)
    });
}
