//! Archive and rename through the chat's menu in the Claude app. The menu
//! opens with Shift+F10 on the focused chat button; its items are
//! `MenuItem` elements, pressed by their exact name. The app's own files
//! are never written: the app does not notice that.

use std::time::Duration;

use crate::core::uia::{self, Automation};
use crate::core::{com, instance};

use super::keys;
use super::{
    app_windows, find_chat, find_edit, find_named, on_com_thread, poll, Chat, Kind, Real, Tree,
};

const ARCHIVE: &str = "Archive";
const RENAME: &str = "Rename";
const MENU_WAIT: Duration = Duration::from_secs(1);
const HEADER_WAIT: Duration = Duration::from_millis(1500);
const FIELD_WAIT: Duration = Duration::from_secs(1);
const STEP: Duration = Duration::from_millis(100);
/// The time the app needs to take the focus and the keys.
const SETTLE: Duration = Duration::from_millis(200);

pub fn archived_notice(title: &str) -> String {
    format!("ClaudeDeck archived \"{title}\". Find it under Archived in the Claude app.")
}

pub fn archive_failed_notice(title: &str) -> String {
    format!("ClaudeDeck could not archive \"{title}\". Archive it in the Claude app.")
}

pub fn rename_failed_notice(title: &str) -> String {
    format!("ClaudeDeck could not rename \"{title}\". Rename it in the Claude app.")
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

/// The menu item in any window of the app: the menu may be a window of
/// its own, or part of the home window.
fn wait_for_item(automation: &Automation, name: &str) -> Option<com::ComPtr> {
    let tree = Real(automation);
    poll(MENU_WAIT, STEP, || {
        let mut roots = vec![];
        for window in app_windows() {
            if let Ok(root) = automation.element_from_handle(window) {
                roots.push(root);
            }
        }
        roots
            .iter()
            .find_map(|root| find_named(&tree, root, Kind::MenuItem, name))
            .map(|found| found.node)
    })
}

/// How a rename was started.
#[derive(Debug, PartialEq, Eq)]
pub enum RenameStart {
    /// The rename button of the chat's own window header was pressed: the
    /// name field is up.
    Header,
    /// The chat's "More options" menu was opened in the list.
    Menu,
    /// Neither button was found.
    Missing,
}

/// Starts a rename without ever opening the chat. A chat with a window of
/// its own (`own`) is renamed in that window, through its header, so the
/// app does not move the chat out of it. Otherwise (or when that window
/// has no such button) the chat's "More options" menu in the home window's
/// list is opened. `list` finds the list when it is needed.
pub fn start_rename<T: Tree>(
    tree: &T,
    own: Option<&T::Node>,
    list: &mut dyn FnMut() -> Result<Option<T::Node>, String>,
    title: &str,
    wait: Duration,
    every: Duration,
) -> Result<RenameStart, String> {
    if let Some(own) = own {
        if press_rename_header(tree, own, title, wait, every)? {
            log::info!("ClaudeDeck: rename \"{title}\": the header of its own window pressed");
            return Ok(RenameStart::Header);
        }
        log::info!("ClaudeDeck: rename \"{title}\": its own window has no rename button");
    }
    let Some(root) = list()? else {
        return Ok(RenameStart::Missing);
    };
    if open_with_more_options(tree, &root, title)? {
        log::info!("ClaudeDeck: rename \"{title}\": its More options menu opened in the list");
        Ok(RenameStart::Menu)
    } else {
        Ok(RenameStart::Missing)
    }
}

/// Opens the chat's menu for Archive, again without opening the chat:
/// first in the chat's own window, else in the home window's list. False
/// when no "More options" button was found.
pub fn start_archive<T: Tree>(
    tree: &T,
    own: Option<&T::Node>,
    list: &mut dyn FnMut() -> Result<Option<T::Node>, String>,
    title: &str,
) -> Result<bool, String> {
    if let Some(own) = own {
        if open_with_more_options(tree, own, title)? {
            log::info!("ClaudeDeck: archive \"{title}\": its own window's menu opened");
            return Ok(true);
        }
        log::info!("ClaudeDeck: archive \"{title}\": its own window has no More options button");
    }
    match list()? {
        Some(root) => open_with_more_options(tree, &root, title),
        None => Ok(false),
    }
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

/// Closes the menu with Esc and puts the chat list back as it was.
fn give_up(chat: &Option<Chat>) {
    keys::chord(&[keys::ESCAPE]);
    close_list(chat);
}

fn close_list(chat: &Option<Chat>) {
    if let Some(show) = chat.as_ref().and_then(|chat| chat.opened.as_ref()) {
        std::thread::sleep(SETTLE);
        if let Err(reason) = uia::collapse(show) {
            log::info!("ClaudeDeck: the chat list was left open: {reason}");
        }
    }
}

/// The chat's own window, when it has one: brought forward, with its root.
fn own_root(automation: &Automation, window: Option<isize>, dry: bool) -> Option<com::ComPtr> {
    let hwnd = window? as windows_sys::Win32::Foundation::HWND;
    let root = automation.element_from_handle(hwnd).ok()?;
    if !dry {
        crate::core::window_actions::activate(hwnd);
    }
    Some(root)
}

/// The Shift+F10 fallback: the chat's button gets the focus (it is not
/// pressed, so the chat does not open) and the app's menu key is sent.
fn menu_key(chat: &Chat) -> Result<(), String> {
    uia::set_focus(&chat.button)?;
    std::thread::sleep(SETTLE);
    if keys::chord(&[keys::SHIFT, keys::F10]) {
        Ok(())
    } else {
        Err("Windows did not take the keys".to_string())
    }
}

fn archive(title: &str, window: Option<isize>, dry: bool) -> Result<Option<String>, String> {
    let failed = || archive_failed_notice(title);
    log::info!(
        "ClaudeDeck: archiving \"{title}\" in the Claude app (own window: {}, read-only: {dry})",
        window.is_some()
    );
    let automation = Automation::new().map_err(|reason| {
        log::warn!("ClaudeDeck: archive \"{title}\": UI Automation failed: {reason}");
        failed()
    })?;
    if dry {
        let own = own_root(&automation, window, dry);
        let found = find_chat(&automation, title, false, dry).ok().flatten();
        log::info!(
            "read-only test instance: archiving \"{title}\" skipped (own window found: {}, in the list: {})",
            own.is_some(),
            found.is_some()
        );
        return Ok(None);
    }
    let tree = Real(&automation);
    let own = own_root(&automation, window, dry);
    let mut chat: Option<Chat> = None;
    let mut list = || -> Result<Option<com::ComPtr>, String> {
        chat = find_chat(&automation, title, false, dry)?;
        Ok(chat.as_ref().map(|chat| chat.root.clone()))
    };
    let opened = start_archive(&tree, own.as_ref(), &mut list, title).map_err(|reason| {
        log::warn!("ClaudeDeck: archive \"{title}\": the menu failed: {reason}");
        failed()
    })?;
    let mut item = if opened {
        wait_for_item(&automation, ARCHIVE)
    } else {
        None
    };
    if item.is_none() {
        if let Some(chat) = &chat {
            keys::chord(&[keys::ESCAPE]);
            log::info!("ClaudeDeck: archive \"{title}\": trying the menu key on its list entry");
            if menu_key(chat).is_ok() {
                item = wait_for_item(&automation, ARCHIVE);
            }
        }
    }
    let Some(item) = item else {
        log::warn!("ClaudeDeck: archive \"{title}\": the menu showed no Archive item");
        give_up(&chat);
        return Err(failed());
    };
    if let Err(reason) = press(&item, ARCHIVE) {
        log::warn!("ClaudeDeck: could not archive \"{title}\": {reason}");
        give_up(&chat);
        return Err(failed());
    }
    close_list(&chat);
    Ok(Some(archived_notice(title)))
}

/// The edit field the app shows for the new name: the one with the
/// keyboard focus, else one in the chat's row.
fn name_field(automation: &Automation, chat: Option<&Chat>) -> Option<com::ComPtr> {
    poll(FIELD_WAIT, STEP, || {
        if let Some(focused) = automation
            .focused_element()
            .filter(|element| uia::control_type(element) == Some(uia::CONTROL_EDIT))
        {
            return Some(focused);
        }
        let row = automation
            .parent(&chat?.button)
            .and_then(|parent| automation.parent(&parent))?;
        find_edit(&Real(automation), &row).map(|found| found.node)
    })
}

fn type_name(automation: &Automation, chat: Option<&Chat>, new_name: &str) -> Result<(), String> {
    let field = name_field(automation, chat).ok_or("the app showed no name field")?;
    if let Err(reason) = uia::set_value(&field, new_name) {
        log::info!("ClaudeDeck: typing the name, as SetValue failed: {reason}");
        uia::set_focus(&field)?;
        keys::chord(&[keys::CONTROL, keys::KEY_A]);
        if !keys::type_text(new_name) {
            return Err("Windows did not take the text".to_string());
        }
    }
    if keys::chord(&[keys::RETURN]) {
        Ok(())
    } else {
        Err("Windows did not take Enter".to_string())
    }
}

fn rename(
    title: &str,
    new_name: &str,
    window: Option<isize>,
    dry: bool,
) -> Result<Option<String>, String> {
    let failed = || rename_failed_notice(title);
    log::info!(
        "ClaudeDeck: renaming \"{title}\" to \"{new_name}\" in the Claude app (own window: {}, read-only: {dry})",
        window.is_some()
    );
    let automation = Automation::new().map_err(|reason| {
        log::warn!("ClaudeDeck: rename \"{title}\": UI Automation failed: {reason}");
        failed()
    })?;
    if dry {
        let own = own_root(&automation, window, dry);
        let found = find_chat(&automation, title, false, dry).ok().flatten();
        log::info!(
            "read-only test instance: renaming \"{title}\" skipped (own window found: {}, in the list: {})",
            own.is_some(),
            found.is_some()
        );
        return Ok(None);
    }
    let tree = Real(&automation);
    let own = own_root(&automation, window, dry);
    let mut chat: Option<Chat> = None;
    let mut list = || -> Result<Option<com::ComPtr>, String> {
        chat = find_chat(&automation, title, false, dry)?;
        Ok(chat.as_ref().map(|chat| chat.root.clone()))
    };
    let started = start_rename(&tree, own.as_ref(), &mut list, title, HEADER_WAIT, STEP).map_err(
        |reason| {
            log::warn!("ClaudeDeck: rename \"{title}\": could not start: {reason}");
            failed()
        },
    )?;
    let steps = (|| -> Result<(), String> {
        match started {
            RenameStart::Header => {}
            RenameStart::Menu | RenameStart::Missing => {
                let mut item = if started == RenameStart::Menu {
                    wait_for_item(&automation, RENAME)
                } else {
                    None
                };
                if item.is_none() {
                    let chat = chat
                        .as_ref()
                        .ok_or("the chat is not in the Claude app's list")?;
                    keys::chord(&[keys::ESCAPE]);
                    log::info!(
                        "ClaudeDeck: rename \"{title}\": trying the menu key on its list entry"
                    );
                    menu_key(chat)?;
                    item = wait_for_item(&automation, RENAME);
                }
                let item = item.ok_or("the menu has no Rename item")?;
                press(&item, RENAME)?;
            }
        }
        type_name(&automation, chat.as_ref(), new_name)
    })();
    match steps {
        Ok(()) => {
            log::info!("ClaudeDeck: rename \"{title}\": done");
            close_list(&chat);
            Ok(None)
        }
        Err(reason) => {
            log::warn!("ClaudeDeck: could not rename \"{title}\": {reason}");
            give_up(&chat);
            Err(failed())
        }
    }
}

/// Archives the chat with this title in the Claude app, on a thread of its
/// own. `window` is the chat's own window, when it has one. `say` gets the
/// notice: what was done, or that it failed. A test copy only reads; unit
/// tests start nothing.
pub fn archive_chat(title: &str, window: Option<isize>, say: impl FnOnce(String) + Send + 'static) {
    if cfg!(test) {
        return;
    }
    let dry = instance::is_test();
    on_com_thread(title.trim().to_string(), say, move |title| {
        archive(title, window, dry)
    });
}

/// Renames the chat in the Claude app. `say` gets a notice only on failure.
pub fn rename_chat(
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
    on_com_thread(title.trim().to_string(), say, move |title| {
        rename(title, &new_name, window, dry)
    });
}
