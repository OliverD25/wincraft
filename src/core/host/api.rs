use super::*;

/// Shows a tray balloon for a plugin. Safe to call from any plugin callback.
pub fn notify(title: &str, text: &str) {
    NOTICES.with(|queue| {
        queue
            .borrow_mut()
            .push((title.to_string(), text.to_string()))
    });
    plugin_changed();
}

/// Sets the tray icon's tooltip, or with None puts the plain name back.
/// Safe to call from any plugin callback.
pub fn set_tray_tip(tip: Option<String>) {
    TRAY_TIP.with(|cell| *cell.borrow_mut() = Some(tip));
    plugin_changed();
}

/// Opens the palette as a one-line box that asks the user `prompt`; the
/// answer comes back to the plugin it names, through `on_prompt_answer`.
/// Safe to call from any plugin callback.
pub fn ask(prompt: Prompt) {
    PROMPT.with(|cell| *cell.borrow_mut() = Some(prompt));
    plugin_changed();
}

/// The Arrange strip's hotkey was pressed: wait for a taskbar number, or on
/// a second press open the strip for the window in front. Call it from
/// `on_hotkey`. Safe to call from any plugin callback.
pub fn arrange_hotkey() {
    ARRANGE_HOTKEY.with(|cell| cell.set(true));
    plugin_changed();
}

/// Opens the Arrange windows list. Safe to call from any plugin callback.
pub fn open_arrange() {
    ARRANGE_WANTED.with(|cell| cell.set(true));
    plugin_changed();
}

/// Opens the Arrange strip on `app`, centred above its taskbar button at
/// `button` (left, top, right, bottom in physical pixels), without taking
/// the keyboard from the window in front. Safe to call from any callback.
pub fn open_arrange_at(app: String, button: [i32; 4]) {
    open_timing::begin("hover", false);
    ARRANGE_ANCHOR.with(|cell| *cell.borrow_mut() = Some((app, button)));
    open_arrange();
}

/// Win and `digit` were pressed and a plugin's keyboard hook kept them:
/// the strip for that taskbar button, even for an app with one window.
/// `again` is the same digit once more while Win stayed down, which picks
/// the next card of an open strip. A button whose app has no window goes
/// back to Windows. Safe to call from any callback, a hook's too.
pub fn open_taskbar_button(digit: u8, again: bool) {
    WIN_DIGITS.with(|queue| queue.borrow_mut().push((digit, again)));
    plugin_changed();
}

/// The host window, for the calls that other threads may make.
pub(super) static HOST_HWND: AtomicIsize = AtomicIsize::new(0);

/// `open_taskbar_button` from any thread: for a keyboard hook that runs on
/// a thread of its own, so it answers Windows at once however busy the
/// host is.
pub fn post_taskbar_button(digit: u8, again: bool) {
    let hwnd = HOST_HWND.load(Ordering::Relaxed);
    if hwnd != 0 {
        unsafe {
            PostMessageW(
                hwnd as HWND,
                WM_APP_WIN_DIGIT,
                usize::from(digit),
                isize::from(again),
            )
        };
    }
}

/// A Windows key went down, from a keyboard hook on a thread of its own:
/// the host gathers WindowDeck's windows, so a digit that follows opens it
/// without waiting for them.
pub fn post_win_down() {
    let hwnd = HOST_HWND.load(Ordering::Relaxed);
    if hwnd != 0 {
        unsafe { PostMessageW(hwnd as HWND, WM_APP_WIN_DOWN, 0, 0) };
    }
}

/// A Windows key let go, from a keyboard hook on a thread of its own:
/// `alone` when no other key went down while it was held. Win pressed and
/// let go alone closes an open WindowDeck, and Start opens as usual.
pub fn post_win_released(alone: bool) {
    let hwnd = HOST_HWND.load(Ordering::Relaxed);
    if hwnd != 0 {
        unsafe { PostMessageW(hwnd as HWND, WM_APP_WIN_UP, usize::from(alone), 0) };
    }
}

/// Closes the Arrange strip if it is open. Safe to call from any callback.
pub fn close_arrange() {
    ARRANGE_CLOSE.with(|cell| cell.set(true));
    plugin_changed();
}

/// Opens one of the plugin's own windows (see `WinCraftPlugin::windows`),
/// or brings it back if it is open. Safe to call from any callback.
pub fn show_window(plugin: &str, window: &str) {
    WINDOWS_WANTED.with(|queue| {
        queue
            .borrow_mut()
            .push((plugin.to_string(), Some(window.to_string()), true))
    });
    plugin_changed();
}

/// Changes one of a plugin's own settings and saves it, as if the user had
/// changed it on the plugin's page; the plugin hears of it through
/// `on_settings_changed`. Safe to call from any callback.
pub fn save_setting(plugin: &str, key: &str, value: Value) {
    SETTINGS_WANTED.with(|queue| {
        queue
            .borrow_mut()
            .push((plugin.to_string(), key.to_string(), value))
    });
    plugin_changed();
}

/// True in a test instance started with `--allow-hide-popup`: WindowDeck's
/// hover strip may hide Windows' taskbar preview, which a test instance
/// otherwise never touches.
pub fn allow_hide_popup() -> bool {
    ALLOW_HIDE_POPUP.load(Ordering::Relaxed)
}

/// Sends the Arrange strip fresh data if it is open, for changes that show
/// a moment after the plugin made them. Safe to call from any callback.
pub fn refresh_arrange() {
    ARRANGE_STALE.with(|cell| cell.set(true));
    plugin_changed();
}

/// Asks the host to send the settings window a fresh snapshot, for example
/// because a plugin's `status` line changed.
pub fn plugin_changed() {
    let hwnd = HOST_WINDOW.with(|cell| cell.get());
    if !hwnd.is_null() {
        unsafe { PostMessageW(hwnd, WM_APP_PLUGIN, 0, 0) };
    }
}

/// The list the ShortcutDetector marks as WinCraft's own.
///
/// It takes an immutable borrow, so calling it while the host is mutably
/// borrowed panics instead of quietly misbehaving. That is the re-entrancy rule
/// with teeth: a plugin may call this from its own WndProc, never from inside
/// on_hotkey or on_tray_action.
pub fn registered_hotkeys() -> Vec<(String, String, Hotkey)> {
    HOST.with(|cell| {
        let borrowed = cell.borrow();
        borrowed
            .as_ref()
            .map(|host| {
                let mut found = host.plugins.registered();
                found.extend(host.shortcut_keys.iter().filter(|(_, _, ok)| *ok).map(
                    |(_, press, _)| {
                        let what = if press.seconds.is_empty() {
                            "Shortcut"
                        } else {
                            "Start of a chord"
                        };
                        ("WinCraft".to_string(), what.to_string(), press.hotkey)
                    },
                ));
                found
            })
            .unwrap_or_default()
    })
}

pub(super) fn cursor_monitor() -> MonitorRect {
    let mut point = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut point) };
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTOPRIMARY) };
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return MonitorRect::default();
    }
    MonitorRect {
        left: info.rcWork.left,
        top: info.rcWork.top,
        right: info.rcWork.right,
        bottom: info.rcWork.bottom,
    }
}

/// Runs on the host thread on purpose. Delivering the hotkey is what gives this
/// process the right to take the foreground, and that right belongs to the
/// thread the hotkey was delivered to.
pub fn bring_to_front(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }
    let began = std::time::Instant::now();
    let ms = |began: std::time::Instant| began.elapsed().as_millis();
    if unsafe { SetForegroundWindow(hwnd) } != 0 {
        log::debug!("took the keyboard for {hwnd:?} at once ({} ms)", ms(began));
        return;
    }
    // Without a hotkey, as when the palette reopens after an update, Windows
    // refuses and only flashes the taskbar button. It allows the process that
    // sent the last input, so send an empty mouse event: no move, no click.
    send_empty_input();
    if unsafe { SetForegroundWindow(hwnd) } != 0 {
        log::debug!(
            "took the keyboard for {hwnd:?} after an empty input ({} ms)",
            ms(began)
        );
        return;
    }
    log::warn!("Windows did not let WinCraft take the keyboard");
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_null() {
        return;
    }
    let other = unsafe { GetWindowThreadProcessId(foreground, std::ptr::null_mut()) };
    let mine = unsafe { GetCurrentThreadId() };
    let attached = unsafe { AttachThreadInput(mine, other, 1) };
    let set = unsafe { SetForegroundWindow(hwnd) };
    unsafe { SetFocus(hwnd) };
    unsafe { AttachThreadInput(mine, other, 0) };
    log::info!(
        "took the keyboard for {hwnd:?} through the other thread's input (attached {attached}, set {set}, {} ms)",
        ms(began)
    );
}

fn send_empty_input() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_MOUSE};
    let mut input: INPUT = unsafe { std::mem::zeroed() };
    input.r#type = INPUT_MOUSE;
    unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
}

/// The palette window is larger than the panel so the shadow fits inside it,
/// which means Windows' own rounded corners and 1 px border would outline the
/// transparent margin rather than the panel. Both are turned off; the panel
/// draws its own.
pub(super) fn clear_system_frame(hwnd: HWND) {
    if hwnd.is_null() {
        return;
    }
    let corners = DWMWCP_DONOTROUND;
    let border = DWMWA_COLOR_NONE;
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &corners as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&corners) as u32,
        );
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR as u32,
            &border as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&border) as u32,
        );
    }
}

/// The longest the palette that reopens after an update waits for the app
/// index (about a second at start). After that it opens anyway and fills in
/// when the index arrives.
const REOPEN_WAIT_CAP: std::time::Duration = std::time::Duration::from_secs(2);
const REOPEN_POLL_MS: u32 = 50;

pub(super) fn palette_ready_to_reopen(index_ready: bool, waited: std::time::Duration) -> bool {
    index_ready || waited >= REOPEN_WAIT_CAP
}

/// Opens the palette as soon as the app index is ready, or after the cap.
pub(super) fn reopen_palette_when_ready(hwnd: HWND) {
    REOPEN_SINCE.with(|since| since.set(Some(std::time::Instant::now())));
    unsafe { SetTimer(hwnd, REOPEN_TIMER, REOPEN_POLL_MS, None) };
}

pub(super) fn on_reopen_timer(hwnd: HWND) {
    let Some(began) = REOPEN_SINCE.with(|since| since.get()) else {
        unsafe { KillTimer(hwnd, REOPEN_TIMER) };
        return;
    };
    let index_ready = search::providers::apps::published().is_some();
    if !palette_ready_to_reopen(index_ready, began.elapsed()) {
        return;
    }
    unsafe { KillTimer(hwnd, REOPEN_TIMER) };
    REOPEN_SINCE.with(|since| since.set(None));
    log::info!(
        "palette reopens after the update, {} ms after start (app index ready: {index_ready})",
        began.elapsed().as_millis()
    );
    show_palette();
}

pub(super) fn show_palette() {
    search::actions::remember_front();
    let palette = with_host(|host| {
        host.to_ui.send(UiCommand::ShowPalette(cursor_monitor()));
        host.palette_hwnd
    });
    if let Some(hwnd) = palette {
        bring_to_front(hwnd);
    }
    with_host(|host| host.send_toggle_states());
}

pub(super) fn open_shortcut_detector() {
    with_host(|host| {
        let slot = host
            .plugins
            .slots
            .iter_mut()
            .find(|slot| slot.enabled && slot.plugin.metadata().id == DETECTOR_ID);
        match slot {
            Some(slot) => slot.plugin.on_tray_action(DETECTOR_OPEN_ACTION),
            None => log::info!("no enabled {DETECTOR_ID} plugin to open"),
        }
    });
}

pub(super) fn open_in_notepad(path: &std::path::Path) {
    let args = wide(&format!("\"{}\"", path.display()));
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide("open").as_ptr(),
            wide("notepad.exe").as_ptr(),
            args.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
}
