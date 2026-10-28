use super::*;

/// Skips the work instead of panicking when the host is already borrowed.
/// Creating a window inside a callback lets Windows send other messages to the
/// host window before the first call has returned. For a message that cannot
/// wait or carries a pointer, dropping it is right and crashing the tray
/// program is not; the others go through `with_host_or_later`.
pub(super) fn with_host<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
    HOST.with(|cell| match cell.try_borrow_mut() {
        Ok(mut borrowed) => borrowed.as_mut().map(f),
        Err(_) => {
            let name = CURRENT_MESSAGE.with(|current| current.get()).map_or_else(
                || "call".to_string(),
                |(msg, wparam)| message_name(msg, wparam),
            );
            log::warn!("host message {name} arrived while the host was busy; ignored");
            None
        }
    })
}

pub(super) fn host_is_busy() -> bool {
    HOST.with(|cell| cell.try_borrow_mut().is_err())
}

/// Runs `f` on what the cell holds, unless the cell is borrowed already.
/// Returns true when it was borrowed and `f` did not run.
pub(super) fn run_unless_busy<T>(cell: &RefCell<Option<T>>, f: impl FnOnce(&mut T)) -> bool {
    match cell.try_borrow_mut() {
        Ok(mut borrowed) => {
            if let Some(value) = borrowed.as_mut() {
                f(value);
            }
            false
        }
        Err(_) => true,
    }
}

/// Like `with_host`, but a message that finds the host busy is delivered
/// again a moment later instead of being dropped. Windows sends
/// WM_DISPLAYCHANGE while the host thread waits on another program's
/// window, as when LayoutKeeper moves windows while a monitor powers up,
/// and a dropped one leaves ScreenDimmer on the old layout. Only for messages
/// whose parameters are plain values: a pointer in one would be stale by
/// then. Anything `f` takes out of a queue must be taken inside `f`, which
/// runs only when the host is free, or it is lost with the first attempt.
pub(super) fn with_host_or_later(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    f: impl FnOnce(&mut Host),
) {
    if HOST.with(|cell| run_unless_busy(cell, f)) {
        deliver_later(hwnd, msg, wparam, lparam);
    }
}

pub(super) fn deliver_later(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) {
    log::info!(
        "host message {} arrived while the host was busy; delivering it again in {RETRY_MS} ms",
        message_name(msg, wparam)
    );
    RETRY.with(|queue| queue_again(&mut queue.borrow_mut(), msg, wparam, lparam));
    unsafe { SetTimer(hwnd, RETRY_TIMER, RETRY_MS, None) };
}

/// Only the newest display change is kept: each one describes the whole
/// layout, and running an old one after a newer one would be wrong. A plugin
/// request is kept once, because the notices and the other things it asks for
/// wait in their own queues, not in the message; and it is kept past the
/// limit, because losing it would lose what those queues hold.
pub(super) fn queue_again(
    queue: &mut Vec<(u32, WPARAM, LPARAM)>,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) {
    if msg == WM_DISPLAYCHANGE {
        queue.retain(|(queued, _, _)| *queued != WM_DISPLAYCHANGE);
    }
    if msg == WM_APP_PLUGIN {
        if !queue.iter().any(|(queued, _, _)| *queued == WM_APP_PLUGIN) {
            queue.push((msg, wparam, lparam));
        }
        return;
    }
    if queue.len() < RETRY_LIMIT {
        queue.push((msg, wparam, lparam));
    }
}

/// A test instance only reads the user's real windows, so a rename typed in
/// the strip changes none of them. `--rename-window` and the palette's box
/// are the ways to rename a window the test owns.
pub(super) fn skipped_in_test_instance(action: &ArrangeAction, test_instance: bool) -> bool {
    test_instance && matches!(action, ArrangeAction::Rename { .. })
}

pub(super) fn message_name(msg: u32, wparam: WPARAM) -> String {
    if msg != 0 && msg == TASKBAR_CREATED.with(|cell| cell.get()) {
        return "TaskbarCreated".to_string();
    }
    match msg {
        WM_DISPLAYCHANGE => "WM_DISPLAYCHANGE".to_string(),
        WM_HOTKEY => "WM_HOTKEY".to_string(),
        WM_TRAY_CALLBACK => "WM_TRAY_CALLBACK (tray icon)".to_string(),
        WM_APP_UI => "WM_APP_UI (request from the UI thread)".to_string(),
        WM_APP_PLUGIN => "WM_APP_PLUGIN (plugin request)".to_string(),
        WM_APP_UPDATE => "WM_APP_UPDATE (update result)".to_string(),
        WM_APP_NOTICE_CLICK => "WM_APP_NOTICE_CLICK (click on the update notice)".to_string(),
        WM_SETTINGCHANGE => "WM_SETTINGCHANGE".to_string(),
        WM_POWERBROADCAST => "WM_POWERBROADCAST".to_string(),
        WM_QUERYENDSESSION => "WM_QUERYENDSESSION".to_string(),
        WM_ENDSESSION => "WM_ENDSESSION".to_string(),
        WM_DESTROY => "WM_DESTROY".to_string(),
        WM_TIMER => match wparam {
            UPDATE_TIMER => "WM_TIMER (update check)".to_string(),
            WAKE_TIMER => "WM_TIMER (update check after a wake)".to_string(),
            HEALTHY_TIMER => "WM_TIMER (healthy report)".to_string(),
            NOTICE_TIMER => "WM_TIMER (update notice)".to_string(),
            BADGE_TIMER => "WM_TIMER (taskbar number badges)".to_string(),
            RETRY_TIMER => "WM_TIMER (redelivery)".to_string(),
            REOPEN_TIMER => "WM_TIMER (palette reopen)".to_string(),
            other => format!("WM_TIMER 0x{other:X}"),
        },
        other => format!("0x{other:04X}"),
    }
}

/// What plugins asked the host for through `notify`, `set_tray_tip`, `ask`,
/// `open_arrange` and `refresh_arrange`. Taking it empties the queues, so it
/// is taken only once the host is free to act on it.
pub(super) struct PluginRequests {
    pub(super) notices: Vec<(String, String)>,
    pub(super) tip: Option<Option<String>>,
    pub(super) arrange: bool,
    pub(super) arrange_hotkey: bool,
    pub(super) anchor: Option<(String, [i32; 4])>,
    pub(super) close: bool,
    pub(super) free: bool,
    pub(super) stale: bool,
    pub(super) win_digits: Vec<(u8, bool)>,
    pub(super) prompt: Option<Prompt>,
    pub(super) settings: Vec<(String, String, Value)>,
    pub(super) windows: Vec<(String, Option<String>, bool)>,
}

impl PluginRequests {
    pub(super) fn take() -> Self {
        PluginRequests {
            notices: NOTICES.with(|queue| std::mem::take(&mut *queue.borrow_mut())),
            tip: TRAY_TIP.with(|cell| cell.borrow_mut().take()),
            arrange: ARRANGE_WANTED.with(|cell| cell.replace(false)),
            arrange_hotkey: ARRANGE_HOTKEY.with(|cell| cell.replace(false)),
            anchor: ARRANGE_ANCHOR.with(|cell| cell.borrow_mut().take()),
            close: ARRANGE_CLOSE.with(|cell| cell.replace(false)),
            free: ARRANGE_FREE.with(|cell| cell.replace(false)),
            stale: ARRANGE_STALE.with(|cell| cell.replace(false)),
            win_digits: WIN_DIGITS.with(|queue| std::mem::take(&mut *queue.borrow_mut())),
            prompt: PROMPT.with(|cell| cell.borrow_mut().take()),
            settings: SETTINGS_WANTED.with(|queue| std::mem::take(&mut *queue.borrow_mut())),
            windows: WINDOWS_WANTED.with(|queue| std::mem::take(&mut *queue.borrow_mut())),
        }
    }
}

pub(super) fn handle_menu_choice(choice: u32) {
    match choice {
        MENU_PALETTE => show_palette(),
        MENU_SETTINGS => {
            with_host(|host| host.to_ui.send(UiCommand::ShowSettings(Page::General)));
        }
        MENU_UPDATE => {
            with_host(|host| {
                host.to_ui.send(UiCommand::ShowSettings(Page::About));
                host.updates.reopen = None;
                host.update_now();
            });
        }
        MENU_QUIT => {
            with_host(|host| host.shutdown());
        }
        id if id >= MENU_PLUGIN_ACTION_BASE => {
            with_host(|host| {
                let offset = id - MENU_PLUGIN_ACTION_BASE;
                let index = (offset / 100) as usize;
                let action_id = offset % 100;
                if let Some(slot) = host.plugins.slots.get_mut(index) {
                    if slot.enabled {
                        slot.plugin.on_tray_action(action_id);
                    }
                }
            });
        }
        _ => {}
    }
}

/// What the flags of `WM_QUERYENDSESSION` and `WM_ENDSESSION` say, for the
/// log. An installer or Windows Update closes apps with `ENDSESSION_CLOSEAPP`
/// while Windows keeps running; a real shutdown or restart sends no flags.
pub(super) fn end_session_words(lparam: LPARAM) -> String {
    let flags = lparam as u32;
    let mut words = Vec::new();
    if flags & ENDSESSION_CLOSEAPP != 0 {
        words.push("ENDSESSION_CLOSEAPP (an installer or update asks apps to close, Windows keeps running)");
    }
    if flags & ENDSESSION_CRITICAL != 0 {
        words.push("ENDSESSION_CRITICAL (Windows ends the session now and does not wait for apps)");
    }
    if flags & ENDSESSION_LOGOFF != 0 {
        words.push("ENDSESSION_LOGOFF (the user is signing out)");
    }
    if words.is_empty() {
        words.push("no flags (shutdown or restart)");
    }
    words.join("; ")
}

pub(super) fn is_colour_change(lparam: LPARAM) -> bool {
    if lparam == 0 {
        return false;
    }
    let mut text = Vec::new();
    let mut at = lparam as *const u16;
    for _ in 0..64 {
        let ch = unsafe { *at };
        if ch == 0 {
            break;
        }
        text.push(ch);
        at = unsafe { at.add(1) };
    }
    String::from_utf16_lossy(&text) == "ImmersiveColorSet"
}

pub(super) unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let outer = CURRENT_MESSAGE.with(|current| current.replace(Some((msg, wparam))));
    let result = dispatch(hwnd, msg, wparam, lparam);
    CURRENT_MESSAGE.with(|current| current.set(outer));
    result
}

pub(super) fn dispatch(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let taskbar_created = TASKBAR_CREATED.with(|cell| cell.get());
    if taskbar_created != 0 && msg == taskbar_created {
        with_host_or_later(hwnd, msg, wparam, lparam, |host| {
            host.tray.add();
            host.buttons_changed();
            for slot in host.plugins.slots.iter_mut().filter(|slot| slot.enabled) {
                slot.plugin.on_taskbar_created();
            }
        });
        return 0;
    }
    let shell_hook = SHELL_HOOK.with(|cell| cell.get());
    if shell_hook != 0 && msg == shell_hook {
        let foreground = unsafe { GetForegroundWindow() } as isize;
        // Which window an app flashes is not always the one with the news
        // (one app may flash its main window for every window), so the log
        // names it.
        if wparam == 0x8006 && lparam != 0 {
            log::info!(
                "attention: window {lparam:#x} \"{}\" flashed{}",
                crate::core::windows_list::window_text(lparam as HWND),
                if lparam == foreground {
                    " (in front, not marked)"
                } else {
                    ""
                }
            );
        }
        let changed = ATTENTION.with(|attention| {
            attention
                .borrow_mut()
                .shell_message(wparam, lparam, foreground)
        });
        if (changed || window_list_changed(wparam)) && ui_bridge::arrange_window().is_some() {
            refresh_arrange();
        }
        if taskbar_cache::buttons_may_change(wparam) {
            // A busy host misses nothing: a press checks the list it counts.
            let _ = HOST.with(|cell| run_unless_busy(cell, |host| host.buttons_changed()));
        }
        return 0;
    }

    match msg {
        WM_HOTKEY => {
            let global_id = wparam as i32;
            if global_id == PALETTE_HOTKEY_ID {
                if host_is_busy() {
                    deliver_later(hwnd, msg, wparam, lparam);
                } else {
                    show_palette();
                }
                return 0;
            }
            with_host_or_later(hwnd, msg, wparam, lparam, |host| {
                if global_id >= SHORTCUT_ID_BASE {
                    host.shortcut_pressed(global_id);
                    return;
                }
                if let Some((index, action_id)) = host.plugins.hotkey_target(global_id) {
                    let pressed = host.plugins.slots[index]
                        .registered
                        .iter()
                        .find(|entry| entry.active && entry.global_id == global_id)
                        .map(|entry| (entry.hotkey, entry.keys.clone()));
                    LAST_HOTKEY.with(|cell| *cell.borrow_mut() = pressed);
                    host.plugins.slots[index].plugin.on_hotkey(action_id);
                }
            });
            0
        }
        WM_TRAY_CALLBACK => {
            if host_is_busy() {
                deliver_later(hwnd, msg, wparam, lparam);
                return 0;
            }
            let event = (lparam as u32) & 0xFFFF;
            if event == NIN_BALLOONUSERCLICK {
                match with_host(|host| host.balloon_click) {
                    Some(BalloonClick::Detector) => open_shortcut_detector(),
                    Some(BalloonClick::About) => {
                        with_host(|host| host.to_ui.send(UiCommand::ShowSettings(Page::About)));
                    }
                    _ => {}
                }
                return 0;
            }
            if event == WM_LBUTTONUP || event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                // The borrow must end before TrackPopupMenu: it runs its own
                // message loop, which re-enters this WndProc and would panic on
                // the second borrow_mut.
                let Some((items, menu_hwnd)) = with_host(|host| (host.menu_items(), host.hwnd))
                else {
                    return 0;
                };
                if let Some(choice) = show_menu(menu_hwnd, &items) {
                    handle_menu_choice(choice);
                }
            }
            0
        }
        WM_APP_PLUGIN => {
            let mut palette = None;
            with_host_or_later(hwnd, msg, wparam, lparam, |host| {
                let requests = PluginRequests::take();
                if let Some(tip) = &requests.tip {
                    host.tray.set_tip(tip.as_deref());
                }
                for (title, text) in &requests.notices {
                    host.balloon_click = BalloonClick::Nothing;
                    host.tray.balloon(title, text);
                }
                for (plugin, key, value) in requests.settings {
                    host.plugins.set_setting(&plugin, &key, value);
                }
                if requests.close {
                    host.to_ui.send(UiCommand::HideArrange);
                }
                if requests.free && !host.strip_plugin_on() {
                    host.to_ui.send(UiCommand::FreeArrange);
                }
                for (plugin, window, open) in requests.windows {
                    match window {
                        Some(window) if open && !host.plugins.is_on(&plugin) => {
                            log::info!("{plugin} is off, so its window {window} stays closed");
                        }
                        Some(window) => host.to_ui.send(UiCommand::PluginWindow {
                            plugin,
                            window,
                            open,
                        }),
                        None => host.to_ui.send(UiCommand::ClosePluginWindows(plugin)),
                    }
                }
                if requests.arrange_hotkey {
                    match LAST_HOTKEY.with(|cell| cell.borrow_mut().take()) {
                        Some((hotkey, keys)) => host.arrange_hotkey(hotkey, &keys),
                        None => {
                            host.arrange_anchor = None;
                            host.send_arrange(true);
                        }
                    }
                }
                if requests.arrange {
                    if requests.anchor.is_some() {
                        open_timing::mark(OpenStep::Host);
                    }
                    host.arrange_anchor = requests.anchor;
                    host.send_arrange(true);
                } else if requests.stale {
                    host.send_arrange(false);
                }
                for (digit, again) in requests.win_digits {
                    if host.win_digit(digit, again) == WinDigit::GiveBack {
                        if crate::core::instance::is_test() {
                            log::info!(
                                "read-only test instance: handing Win+{digit} back to Windows skipped"
                            );
                        } else {
                            mask_key::replay_win_digit(0x30 + u32::from(digit));
                        }
                    }
                }
                host.publish();
                if let Some(prompt) = requests.prompt {
                    host.to_ui
                        .send(UiCommand::ShowPrompt(prompt, cursor_monitor()));
                    palette = Some(host.palette_hwnd);
                }
            });
            if let Some(hwnd) = palette {
                bring_to_front(hwnd);
            }
            0
        }
        WM_APP_UI => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| {
                while let Ok(request) = host.host_rx.try_recv() {
                    host.handle_request(request);
                }
            });
            0
        }
        WM_APP_UPDATE => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| host.update_events());
            0
        }
        WM_TIMER if wparam == UPDATE_TIMER => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| host.timed_check());
            0
        }
        WM_TIMER if wparam == WAKE_TIMER => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| host.wake_check());
            0
        }
        WM_APP_WIN_DIGIT => {
            open_taskbar_button(wparam as u8, lparam != 0);
            0
        }
        WM_APP_WIN_DOWN => {
            // Only to save time later: a busy host skips it.
            let _ = HOST.with(|cell| run_unless_busy(cell, |host| host.prepare_deck()));
            0
        }
        WM_APP_BUTTONS => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| host.buttons_arrived());
            0
        }
        WM_TIMER if wparam == BUTTONS_TIMER => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| host.buttons_timer());
            0
        }
        WM_APP_WIN_UP => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| {
                host.win_released(wparam != 0)
            });
            0
        }
        WM_APP_WAIT_KEY => {
            if host_is_busy() {
                deliver_later(hwnd, msg, wparam, lparam);
                return 0;
            }
            let news = chord::take_for(hwnd as isize);
            with_host(|host| host.wait_news(news));
            0
        }
        WM_TIMER if wparam == chord::TIMER_ID => {
            chord::on_timer(hwnd as isize);
            with_host_or_later(hwnd, msg, wparam, lparam, |host| host.end_wait());
            0
        }
        WM_TIMER if wparam == BADGE_TIMER => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| host.reveal_tick());
            0
        }
        WM_TIMER if wparam == NOTICE_TIMER => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| host.notice_tick());
            0
        }
        WM_APP_NOTICE_CLICK => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| {
                host.close_update_notice();
                host.to_ui.send(UiCommand::ShowSettings(Page::About));
            });
            0
        }
        WM_TIMER if wparam == HEALTHY_TIMER => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| host.report_healthy(false));
            0
        }
        WM_TIMER if wparam == REOPEN_TIMER => {
            if !host_is_busy() {
                on_reopen_timer(hwnd);
            }
            0
        }
        WM_TIMER if wparam == RETRY_TIMER => {
            unsafe { KillTimer(hwnd, RETRY_TIMER) };
            let queued = RETRY.with(|queue| std::mem::take(&mut *queue.borrow_mut()));
            for (msg, wparam, lparam) in queued {
                unsafe { wnd_proc(hwnd, msg, wparam, lparam) };
            }
            0
        }
        WM_DISPLAYCHANGE => {
            with_host_or_later(hwnd, msg, wparam, lparam, |host| {
                // The palette was placed against a monitor layout that no
                // longer exists, so it is hidden rather than left stranded.
                host.to_ui.send(UiCommand::HideAll);
                for slot in host.plugins.slots.iter_mut().filter(|slot| slot.enabled) {
                    slot.plugin.on_windows_message(msg, wparam, lparam);
                }
            });
            0
        }
        WM_SETTINGCHANGE | WM_POWERBROADCAST | WM_TIMER => {
            // Windows sends this one on every resume, with or without a
            // key pressed; the check waits for the network.
            if msg == WM_POWERBROADCAST && wparam == PBT_APMRESUMEAUTOMATIC as WPARAM {
                with_host(|host| host.woke());
            }
            if msg == WM_SETTINGCHANGE
                && (is_colour_change(lparam) || wparam == SPI_SETHIGHCONTRAST as WPARAM)
            {
                with_host(|host| {
                    host.tray.refresh_icon();
                    host.to_ui.send(UiCommand::ThemeChanged);
                });
            }
            with_host(|host| {
                for slot in host.plugins.slots.iter_mut().filter(|slot| slot.enabled) {
                    slot.plugin.on_windows_message(msg, wparam, lparam);
                }
            });
            0
        }
        // Plugins get these synchronously, while every program is still
        // open, so a plugin can record the session before it goes away.
        // Answering TRUE never holds up the shutdown.
        WM_QUERYENDSESSION | WM_ENDSESSION => {
            let why = end_session_words(lparam);
            if msg == WM_QUERYENDSESSION {
                log::info!("Windows asks if the session can end: {why}");
            } else if wparam != 0 {
                log::info!("the session is ending: {why}");
            } else {
                log::info!("the session did not end after all: {why}");
            }
            with_host(|host| {
                for slot in host.plugins.slots.iter_mut().filter(|slot| slot.enabled) {
                    slot.plugin.on_windows_message(msg, wparam, lparam);
                }
            });
            if msg == WM_QUERYENDSESSION {
                1
            } else {
                0
            }
        }
        WM_DESTROY => {
            with_host(|host| host.shutdown());
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
