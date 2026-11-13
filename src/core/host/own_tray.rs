//! Plugins' own tray icons, beside WinCraft's. A plugin asks for one with
//! `host::set_own_tray` and gets its clicks through `on_own_tray_click`,
//! `own_tray_menu` and `on_tray_action`. Each icon has the `uID` of its
//! plugin's slot, and the same callback window and message as WinCraft's
//! own icon, which keeps `uID` 1. An icon goes when its plugin is switched
//! off, and every icon goes when WinCraft quits (Principle 3: a plugin
//! leaves nothing behind).

use super::*;
use crate::core::plugin_tray::{self, OwnTray};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::HICON;

/// The first `uID` of a plugin's icon; WinCraft's own icon is 1.
const FIRST_ID: u32 = 100;

struct Shown {
    plugin: String,
    id: u32,
    tray: OwnTray,
    icon: HICON,
    /// Windows took the icon.
    added: bool,
}

thread_local! {
    static WANTED: RefCell<Vec<(String, Option<OwnTray>)>> = const { RefCell::new(Vec::new()) };
    static SHOWN: RefCell<Vec<Shown>> = const { RefCell::new(Vec::new()) };
}

/// Gives the plugin `plugin` a tray icon of its own, or changes it; None
/// removes it. The plugin hears of a left click through
/// `on_own_tray_click`, and a right click shows `own_tray_menu`, whose
/// picks come back through `on_tray_action`. Safe to call from any plugin
/// callback.
pub fn set_own_tray(plugin: &str, tray: Option<OwnTray>) {
    WANTED.with(|queue| queue.borrow_mut().push((plugin.to_string(), tray)));
    plugin_changed();
}

/// What to do with one plugin's icon.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Step {
    Add,
    Change,
    Remove,
    Nothing,
}

/// An icon is shown only while its plugin is on; the same picture and
/// tooltip again changes nothing.
pub(super) fn step(shown: Option<&OwnTray>, wanted: Option<&OwnTray>, plugin_on: bool) -> Step {
    match (shown, wanted.filter(|_| plugin_on)) {
        (None, None) => Step::Nothing,
        (None, Some(_)) => Step::Add,
        (Some(_), None) => Step::Remove,
        (Some(old), Some(new)) if old == new => Step::Nothing,
        (Some(_), Some(_)) => Step::Change,
    }
}

pub(super) fn id_for(slot: usize) -> u32 {
    FIRST_ID + slot as u32
}

fn data(id: u32, icon: HICON, tip: &str) -> NOTIFYICONDATAW {
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = HOST_WINDOW.with(|cell| cell.get());
    data.uID = id;
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = WM_TRAY_CALLBACK;
    data.hIcon = icon;
    crate::core::tray::copy_into(&mut data.szTip, tip);
    data
}

fn add(shown: &mut Shown) {
    let ok = unsafe { Shell_NotifyIconW(NIM_ADD, &data(shown.id, shown.icon, &shown.tray.tip)) };
    shown.added = ok != 0;
    if !shown.added {
        log::warn!("could not add the tray icon of {}", shown.plugin);
    }
}

fn delete(shown: &Shown) {
    if shown.added {
        unsafe { Shell_NotifyIconW(NIM_DELETE, &data(shown.id, shown.icon, "")) };
    }
    plugin_tray::free(shown.icon);
}

/// Does what the plugins asked for. `slot_if_on` gives a plugin's slot
/// while it is on.
pub(super) fn apply(slot_if_on: impl Fn(&str) -> Option<usize>) {
    let wanted = WANTED.with(|queue| std::mem::take(&mut *queue.borrow_mut()));
    SHOWN.with(|shown| {
        let mut shown = shown.borrow_mut();
        for (plugin, tray) in wanted {
            let slot = slot_if_on(&plugin);
            let at = shown.iter().position(|icon| icon.plugin == plugin);
            match step(at.map(|at| &shown[at].tray), tray.as_ref(), slot.is_some()) {
                Step::Nothing => {}
                Step::Remove => {
                    if let Some(at) = at {
                        delete(&shown.remove(at));
                        log::info!("tray icon of {plugin} removed");
                    }
                }
                Step::Add => {
                    let (Some(tray), Some(slot)) = (tray, slot) else {
                        continue;
                    };
                    let mut icon = Shown {
                        plugin: plugin.clone(),
                        id: id_for(slot),
                        icon: plugin_tray::make_icon(&tray),
                        tray,
                        added: false,
                    };
                    add(&mut icon);
                    log::info!("tray icon of {plugin} added");
                    shown.push(icon);
                }
                Step::Change => {
                    let (Some(tray), Some(at)) = (tray, at) else {
                        continue;
                    };
                    let icon = &mut shown[at];
                    let old = icon.icon;
                    icon.icon = plugin_tray::make_icon(&tray);
                    icon.tray = tray;
                    if icon.added {
                        unsafe {
                            Shell_NotifyIconW(NIM_MODIFY, &data(icon.id, icon.icon, &icon.tray.tip))
                        };
                    } else {
                        add(icon);
                    }
                    plugin_tray::free(old);
                }
            }
        }
    });
}

/// Explorer started again and forgot every icon: adds them back.
pub(super) fn add_all() {
    SHOWN.with(|shown| shown.borrow_mut().iter_mut().for_each(add));
}

/// WinCraft quits: no icon may stay behind.
pub(super) fn remove_all() {
    SHOWN.with(|shown| shown.borrow_mut().drain(..).for_each(|icon| delete(&icon)));
}

/// The plugin whose icon has this `uID`.
pub(super) fn owner(id: u32) -> Option<String> {
    SHOWN.with(|shown| {
        shown
            .borrow()
            .iter()
            .find(|icon| icon.id == id)
            .map(|icon| icon.plugin.clone())
    })
}

/// A click on a plugin's icon: the left button tells the plugin, the right
/// one shows its menu and hands the pick back.
pub(super) fn clicked(plugin: &str, event: u32) {
    if event == WM_LBUTTONUP {
        with_host(|host| {
            if let Some(slot) = host.plugins.index_of(plugin) {
                let slot = &mut host.plugins.slots[slot];
                if slot.enabled {
                    detailed_log::plugin_call(
                        slot.plugin.as_mut(),
                        || "own tray icon click".to_string(),
                        0.0,
                        |plugin| plugin.on_own_tray_click(),
                    );
                }
            }
        });
        return;
    }
    if event != WM_RBUTTONUP && event != WM_CONTEXTMENU {
        return;
    }
    // The borrow ends before the menu runs its own message loop.
    let Some((items, hwnd, slot)) = with_host(|host| {
        let slot = host
            .plugins
            .index_of(plugin)
            .filter(|slot| host.plugins.slots[*slot].enabled)?;
        Some((
            host.plugins.slots[slot].plugin.own_tray_menu(),
            host.hwnd,
            slot,
        ))
    })
    .flatten() else {
        return;
    };
    if items.is_empty() {
        return;
    }
    if let Some(choice) = show_menu(hwnd, &items) {
        with_host(|host| {
            if let Some(slot) = host.plugins.slots.get_mut(slot).filter(|s| s.enabled) {
                detailed_log::plugin_call(
                    slot.plugin.as_mut(),
                    || format!("own tray action {choice}"),
                    0.0,
                    |plugin| plugin.on_tray_action(choice),
                );
            }
        });
    }
}

#[cfg(test)]
mod tests;
