//! Archive and rename through the chat's menu in the Claude app. The menu
//! opens with Shift+F10 on the focused chat button; its items are
//! `MenuItem` elements, pressed by their exact name. The app's own files
//! are never written: the app does not notice that.

use std::time::Duration;

use crate::core::uia::{self, Automation};
use crate::core::{com, instance};

use super::keys;
use super::{app_windows, find_chat, find_edit, find_named, on_com_thread, poll, Chat, Kind, Real};

const ARCHIVE: &str = "Archive";
const RENAME: &str = "Rename";
const MENU_WAIT: Duration = Duration::from_secs(1);
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

/// Focuses the chat's button, opens its menu and waits for the item.
fn menu_item(automation: &Automation, chat: &Chat, name: &str) -> Result<com::ComPtr, String> {
    uia::set_focus(&chat.button)?;
    std::thread::sleep(SETTLE);
    if !keys::chord(&[keys::SHIFT, keys::F10]) {
        return Err("Windows did not take the keys".to_string());
    }
    let tree = Real(automation);
    poll(MENU_WAIT, STEP, || {
        // The menu may be a window of its own, or part of the home window.
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
    .ok_or_else(|| format!("the menu has no \"{name}\" item"))
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
fn give_up(chat: &Chat) {
    keys::chord(&[keys::ESCAPE]);
    close_list(chat);
}

fn close_list(chat: &Chat) {
    if let Some(show) = &chat.opened {
        std::thread::sleep(SETTLE);
        if let Err(reason) = uia::collapse(show) {
            log::info!("ClaudeDeck: the chat list was left open: {reason}");
        }
    }
}

fn archive(title: &str, dry: bool) -> Result<Option<String>, String> {
    let failed = || archive_failed_notice(title);
    let automation = Automation::new().map_err(|_| failed())?;
    let chat = find_chat(&automation, title, false, dry)
        .map_err(|_| failed())?
        .ok_or_else(failed)?;
    if dry {
        log::info!("read-only test instance: archiving \"{title}\" in the Claude app skipped");
        return Ok(None);
    }
    let item = match menu_item(&automation, &chat, ARCHIVE) {
        Ok(item) => item,
        Err(reason) => {
            log::warn!("ClaudeDeck: could not archive \"{title}\": {reason}");
            give_up(&chat);
            return Err(failed());
        }
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
fn name_field(automation: &Automation, chat: &Chat) -> Option<com::ComPtr> {
    poll(FIELD_WAIT, STEP, || {
        if let Some(focused) = automation
            .focused_element()
            .filter(|element| uia::control_type(element) == Some(uia::CONTROL_EDIT))
        {
            return Some(focused);
        }
        let row = automation
            .parent(&chat.button)
            .and_then(|parent| automation.parent(&parent))?;
        find_edit(&Real(automation), &row).map(|found| found.node)
    })
}

fn rename(title: &str, new_name: &str, dry: bool) -> Result<Option<String>, String> {
    let failed = || rename_failed_notice(title);
    let automation = Automation::new().map_err(|_| failed())?;
    let chat = find_chat(&automation, title, false, dry)
        .map_err(|_| failed())?
        .ok_or_else(failed)?;
    if dry {
        log::info!("read-only test instance: renaming \"{title}\" in the Claude app skipped");
        return Ok(None);
    }
    let steps = (|| -> Result<(), String> {
        let item = menu_item(&automation, &chat, RENAME)?;
        press(&item, RENAME)?;
        let field = name_field(&automation, &chat).ok_or("the app showed no name field")?;
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
    })();
    match steps {
        Ok(()) => {
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
/// own. `say` gets the notice: what was done, or that it failed. A test
/// copy only reads; unit tests start nothing.
pub fn archive_chat(title: &str, say: impl FnOnce(String) + Send + 'static) {
    if cfg!(test) {
        return;
    }
    let dry = instance::is_test();
    on_com_thread(title.trim().to_string(), say, move |title| {
        archive(title, dry)
    });
}

/// Renames the chat in the Claude app. `say` gets a notice only on failure.
pub fn rename_chat(title: &str, new_name: &str, say: impl FnOnce(String) + Send + 'static) {
    let new_name = new_name.trim().to_string();
    if cfg!(test) || new_name.is_empty() {
        return;
    }
    let dry = instance::is_test();
    on_com_thread(title.trim().to_string(), say, move |title| {
        rename(title, &new_name, dry)
    });
}
