use std::cell::RefCell;
use std::collections::HashMap;

use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateFontIndirectW, DeleteObject, GetDC, GetMonitorInfoW, GetTextExtentPoint32W,
    MonitorFromPoint, ReleaseDC, SelectObject, SetBkMode,
    SetTextColor, COLOR_BTNFACE, HFONT, HMONITOR, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
    TRANSPARENT,
};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;
use windows_sys::Win32::UI::Controls::{
    InitCommonControlsEx, INITCOMMONCONTROLSEX, ICC_LISTVIEW_CLASSES, ICC_STANDARD_CLASSES,
    LVCF_SUBITEM, LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVIF_TEXT, LVITEMW, LVM_DELETEALLITEMS,
    LVM_INSERTCOLUMNW, LVM_INSERTITEMW, LVM_SETCOLUMNWIDTH, LVM_SETEXTENDEDLISTVIEWSTYLE,
    LVM_SETITEMW,
    LVN_COLUMNCLICK, LVS_EX_DOUBLEBUFFER, LVS_EX_FULLROWSELECT, LVS_REPORT, LVS_SHOWSELALWAYS,
    LVS_SINGLESEL, NMHDR, NMLISTVIEW, BST_CHECKED, BST_UNCHECKED,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, VK_LMENU, VK_LSHIFT,
    VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect,
    GetWindowTextW, LoadCursorW, LoadIconW, MoveWindow, PostMessageW, RegisterClassW, SendMessageW,
    SetForegroundWindow, SetWindowPos, SetWindowTextW, SetWindowsHookExW,
    ShowWindow, SystemParametersInfoW, UnhookWindowsHookEx, BM_GETCHECK, BM_SETCHECK,
    BS_AUTOCHECKBOX, CBN_SELCHANGE, CBS_DROPDOWNLIST, CB_ADDSTRING,
    CB_GETCURSEL, CB_SETCURSEL, EN_CHANGE, EN_KILLFOCUS, EN_SETFOCUS, ES_AUTOHSCROLL,
    HWND_TOP, IDC_ARROW, KBDLLHOOKSTRUCT, MINMAXINFO, NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS,
    SW_RESTORE, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER, WH_KEYBOARD_LL, WM_APP, WM_CLOSE, WM_COMMAND, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DPICHANGED,
    WM_GETMINMAXINFO, WM_KEYDOWN, WM_NOTIFY, WM_SETFONT, WM_SIZE, WM_SYSKEYDOWN, WNDCLASSW,
    WS_CHILD, WS_EX_CLIENTEDGE, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

use crate::core::traits::Hotkey;
use crate::core::{host, hotkeys, wide};
use crate::modules::shortcut_detector::probe::{self, Entry, ScanResult, Status};

const CLASS_NAME: &str = "WinCraftShortcutDetector";
pub const WM_APP_SCAN: u32 = WM_APP + 11;

const ID_SEARCH: i32 = 101;
const ID_MODIFIERS: i32 = 102;
const ID_SHOW_FREE: i32 = 103;
const ID_REFRESH: i32 = 104;
const ID_COPY: i32 = 105;
const ID_CAPTURE: i32 = 106;
const ID_VERDICT: i32 = 107;
const ID_LIST: i32 = 108;
const ID_STATUS: i32 = 109;
const ID_SEARCH_LABEL: i32 = 110;
const ID_MODIFIERS_LABEL: i32 = 111;
const ID_CAPTURE_LABEL: i32 = 112;

const SUBCLASS_FORWARD_F5: usize = 1;

const WINDOW_WIDTH: i32 = 760;
const WINDOW_HEIGHT: i32 = 560;
const MIN_WIDTH: i32 = 560;
const MIN_HEIGHT: i32 = 400;

const COLOUR_FREE: COLORREF = 0x0000_7A18;
const COLOUR_TAKEN: COLORREF = 0x0000_20C0;
const COLOUR_SYSTEM: COLORREF = 0x00A0_5000;
const COLOUR_PLAIN: COLORREF = 0x0000_0000;

struct Detector {
    controls: HashMap<i32, HWND>,
    font: HFONT,
    scan: Option<ScanResult>,
    visible: Vec<usize>,
    sort_column: i32,
    sort_ascending: bool,
    show_free: bool,
    verdict_colour: COLORREF,
    hook: isize,
    capture_modifiers: u32,
}

thread_local! {
    static DETECTORS: RefCell<HashMap<isize, Detector>> = RefCell::new(HashMap::new());
    /// The keyboard hook callback gets no user data, so it needs a way back to
    /// the window that installed it. Only one capture box can have focus.
    static CAPTURING: RefCell<isize> = const { RefCell::new(0) };
}

pub fn open(existing: Option<HWND>) -> Option<HWND> {
    if let Some(hwnd) = existing {
        unsafe {
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
        }
        return Some(hwnd);
    }
    create()
}

fn create() -> Option<HWND> {
    ensure_class()?;
    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let (x, y, width, height) = starting_rect();

    let hwnd = unsafe {
        CreateWindowExW(
            0,
            wide(CLASS_NAME).as_ptr(),
            wide("WinCraft \u{2014} Shortcut detector").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            x,
            y,
            width,
            height,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        )
    };
    if hwnd.is_null() {
        log::error!("could not create the shortcut detector window");
        return None;
    }

    build_controls(hwnd);
    unsafe {
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        // The scan must not run here: open() is reached from on_tray_action,
        // which holds the host's mutable borrow. By the time this message comes
        // back through the loop that borrow is gone.
        PostMessageW(hwnd, WM_APP_SCAN, 0, 0);
    }
    Some(hwnd)
}

fn ensure_class() -> Option<()> {
    thread_local! {
        static REGISTERED: RefCell<bool> = const { RefCell::new(false) };
    }
    let already = REGISTERED.with(|cell| *cell.borrow());
    if already {
        return Some(());
    }

    let mut controls: INITCOMMONCONTROLSEX = unsafe { std::mem::zeroed() };
    controls.dwSize = std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32;
    controls.dwICC = ICC_LISTVIEW_CLASSES | ICC_STANDARD_CLASSES;
    unsafe { InitCommonControlsEx(&controls) };

    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let name = wide(CLASS_NAME);
    let mut class: WNDCLASSW = unsafe { std::mem::zeroed() };
    class.lpfnWndProc = Some(window_proc);
    class.hInstance = hinstance;
    class.lpszClassName = name.as_ptr();
    class.hCursor = unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) };
    class.hIcon = unsafe { LoadIconW(hinstance, 1 as *const u16) };
    class.hbrBackground = (COLOR_BTNFACE + 1) as isize as _;
    if unsafe { RegisterClassW(&class) } == 0 {
        log::error!("could not register the shortcut detector window class");
        return None;
    }
    REGISTERED.with(|cell| *cell.borrow_mut() = true);
    Some(())
}

fn starting_rect() -> (i32, i32, i32, i32) {
    let mut point = POINT { x: 0, y: 0 };
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut point) };
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTOPRIMARY) };

    // The window does not exist yet, so its scaling has to come from the monitor
    // it is about to appear on. Skip this and the window comes out a third too
    // small on a 150 % display, with every control spilling over its neighbour.
    let dpi = monitor_dpi(monitor);
    let width = WINDOW_WIDTH * dpi / 96;
    let height = WINDOW_HEIGHT * dpi / 96;

    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return (100, 100, width, height);
    }
    let work = info.rcWork;
    let x = work.left + ((work.right - work.left) - width) / 2;
    let y = work.top + ((work.bottom - work.top) - height) / 2;
    (x.max(work.left), y.max(work.top), width, height)
}

fn monitor_dpi(monitor: HMONITOR) -> i32 {
    let mut x: u32 = 96;
    let mut y: u32 = 96;
    if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y) } != 0 {
        return 96;
    }
    (x as i32).max(96)
}

fn build_controls(hwnd: HWND) {
    let hinstance = unsafe { GetModuleHandleW(std::ptr::null()) };
    let mut controls: HashMap<i32, HWND> = HashMap::new();

    let mut add = |id: i32, class: &str, text: &str, style: u32, ex_style: u32| {
        let child = unsafe {
            CreateWindowExW(
                ex_style,
                wide(class).as_ptr(),
                wide(text).as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
                0,
                0,
                0,
                0,
                hwnd,
                id as isize as *mut core::ffi::c_void,
                hinstance,
                std::ptr::null(),
            )
        };
        controls.insert(id, child);
    };

    add(ID_SEARCH_LABEL, "STATIC", "Search:", 0, 0);
    add(
        ID_SEARCH,
        "EDIT",
        "",
        WS_TABSTOP | ES_AUTOHSCROLL as u32,
        WS_EX_CLIENTEDGE,
    );
    add(ID_MODIFIERS_LABEL, "STATIC", "Modifiers:", 0, 0);
    add(
        ID_MODIFIERS,
        "COMBOBOX",
        "",
        WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32,
        0,
    );
    add(
        ID_SHOW_FREE,
        "BUTTON",
        "Show free",
        WS_TABSTOP | BS_AUTOCHECKBOX as u32,
        0,
    );
    add(ID_REFRESH, "BUTTON", "Refresh (F5)", WS_TABSTOP, 0);
    add(ID_COPY, "BUTTON", "Copy", WS_TABSTOP, 0);
    add(
        ID_CAPTURE_LABEL,
        "STATIC",
        "Check a shortcut:",
        0,
        0,
    );
    add(
        ID_CAPTURE,
        "EDIT",
        "",
        WS_TABSTOP | ES_AUTOHSCROLL as u32,
        WS_EX_CLIENTEDGE,
    );
    add(ID_VERDICT, "STATIC", "", 0, 0);
    add(
        ID_LIST,
        "SysListView32",
        "",
        WS_TABSTOP | LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS,
        0,
    );
    add(ID_STATUS, "STATIC", "Scanning\u{2026}", 0, 0);

    let font = message_font(hwnd);
    for child in controls.values() {
        unsafe { SendMessageW(*child, WM_SETFONT, font as WPARAM, 1) };
    }

    let list = controls[&ID_LIST];
    unsafe {
        SendMessageW(
            list,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            0,
            (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as LPARAM,
        )
    };
    for (index, title) in ["Shortcut", "Status", "Owner / meaning", "Source"]
        .into_iter()
        .enumerate()
    {
        let mut text = wide(title);
        let mut column: LVCOLUMNW = unsafe { std::mem::zeroed() };
        column.mask = LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM;
        column.cx = scaled(hwnd, 120);
        column.pszText = text.as_mut_ptr();
        column.iSubItem = index as i32;
        unsafe { SendMessageW(list, LVM_INSERTCOLUMNW, index, &column as *const _ as LPARAM) };
    }

    let combo = controls[&ID_MODIFIERS];
    unsafe { SendMessageW(combo, CB_ADDSTRING, 0, wide("All").as_ptr() as LPARAM) };
    for modifiers in probe::modifier_sets() {
        let label = hotkeys::format_modifiers(modifiers);
        unsafe { SendMessageW(combo, CB_ADDSTRING, 0, wide(&label).as_ptr() as LPARAM) };
    }
    unsafe { SendMessageW(combo, CB_SETCURSEL, 0, 0) };

    for id in [ID_SEARCH, ID_CAPTURE, ID_LIST] {
        unsafe { SetWindowSubclass(controls[&id], Some(forward_f5), SUBCLASS_FORWARD_F5, 0) };
    }

    DETECTORS.with(|cell| {
        cell.borrow_mut().insert(
            hwnd as isize,
            Detector {
                controls,
                font,
                scan: None,
                visible: Vec::new(),
                sort_column: 0,
                sort_ascending: true,
                show_free: false,
                verdict_colour: COLOUR_PLAIN,
                hook: 0,
                capture_modifiers: 0,
            },
        )
    });
    layout(hwnd);
}

fn message_font(hwnd: HWND) -> HFONT {
    let mut metrics: NONCLIENTMETRICSW = unsafe { std::mem::zeroed() };
    metrics.cbSize = std::mem::size_of::<NONCLIENTMETRICSW>() as u32;
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            metrics.cbSize,
            &mut metrics as *mut _ as *mut core::ffi::c_void,
            0,
        )
    };
    if ok == 0 {
        return std::ptr::null_mut();
    }
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
    metrics.lfMessageFont.lfHeight = metrics.lfMessageFont.lfHeight * dpi / 96;
    unsafe { CreateFontIndirectW(&metrics.lfMessageFont) }
}

/// Label widths are measured, not guessed. The system message font changes with
/// the user's display scaling and their font settings, and a width that fits
/// "Modifiers:" on one machine clips it to "Modifie" on the next.
fn text_width(hwnd: HWND, text: &str) -> i32 {
    text_size(hwnd, text).cx
}

fn text_height(hwnd: HWND) -> i32 {
    text_size(hwnd, "Wg").cy
}

fn text_size(hwnd: HWND, text: &str) -> SIZE {
    let font = DETECTORS.with(|cell| {
        cell.borrow()
            .get(&(hwnd as isize))
            .map(|detector| detector.font)
            .unwrap_or(std::ptr::null_mut())
    });
    let mut size = SIZE { cx: 0, cy: 0 };
    let dc = unsafe { GetDC(hwnd) };
    if dc.is_null() {
        return size;
    }
    let previous = if font.is_null() {
        std::ptr::null_mut()
    } else {
        unsafe { SelectObject(dc, font as _) }
    };
    let encoded = wide(text);
    unsafe {
        GetTextExtentPoint32W(dc, encoded.as_ptr(), (encoded.len() - 1) as i32, &mut size);
        if !previous.is_null() {
            SelectObject(dc, previous);
        }
        ReleaseDC(hwnd, dc);
    }
    size
}

fn scaled(hwnd: HWND, value: i32) -> i32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
    value * dpi / 96
}

fn layout(hwnd: HWND) {
    let controls = DETECTORS.with(|cell| {
        cell.borrow()
            .get(&(hwnd as isize))
            .map(|detector| detector.controls.clone())
    });
    let Some(controls) = controls else {
        return;
    };

    let mut client: RECT = unsafe { std::mem::zeroed() };
    unsafe { GetClientRect(hwnd, &mut client) };
    let width = client.right - client.left;
    let height = client.bottom - client.top;
    let pad = scaled(hwnd, 10);
    let row = scaled(hwnd, 26);
    let gap = scaled(hwnd, 8);

    let place = |id: i32, x: i32, y: i32, w: i32, h: i32| {
        if let Some(child) = controls.get(&id) {
            unsafe { MoveWindow(*child, x, y, w, h, 1) };
        }
    };

    let label_top = scaled(hwnd, 4);
    let small = scaled(hwnd, 4);
    let tick = scaled(hwnd, 22);
    let y = pad;
    let mut x = pad;

    let search_label = text_width(hwnd, "Search:");
    place(ID_SEARCH_LABEL, x, y + label_top, search_label, row);
    x += search_label + small;
    let search_width = scaled(hwnd, 130);
    place(ID_SEARCH, x, y, search_width, row);
    x += search_width + gap;

    let modifiers_label = text_width(hwnd, "Modifiers:");
    place(ID_MODIFIERS_LABEL, x, y + label_top, modifiers_label, row);
    x += modifiers_label + small;
    let combo_width = scaled(hwnd, 108);
    place(ID_MODIFIERS, x, y, combo_width, scaled(hwnd, 260));
    x += combo_width + gap;

    let copy_width = text_width(hwnd, "Copy") + scaled(hwnd, 20);
    let refresh_width = text_width(hwnd, "Refresh (F5)") + scaled(hwnd, 20);
    let copy_x = width - pad - copy_width;
    let refresh_x = copy_x - small - refresh_width;
    place(ID_COPY, copy_x, y, copy_width, row);
    place(ID_REFRESH, refresh_x, y, refresh_width, row);

    let free_width = (tick + text_width(hwnd, "Show free")).min((refresh_x - gap - x).max(0));
    place(ID_SHOW_FREE, x, y + label_top, free_width, row);

    let second = y + row + gap;
    let capture_label = text_width(hwnd, "Check a shortcut:");
    place(ID_CAPTURE_LABEL, pad, second + label_top, capture_label, row);
    let capture_x = pad + capture_label + small;
    let capture_width = scaled(hwnd, 150);
    place(ID_CAPTURE, capture_x, second, capture_width, row);
    let verdict_x = capture_x + capture_width + gap;
    place(
        ID_VERDICT,
        verdict_x,
        second + label_top,
        (width - pad - verdict_x).max(0),
        row,
    );

    // Two lines: the status sentence names the limitation as well as the counts,
    // and it must not be cut off on a narrow window.
    let status_height = text_height(hwnd) * 2 + small;
    let list_top = second + row + gap;
    let list_width = width - pad * 2;
    let list_height = (height - list_top - status_height - small).max(0);
    place(ID_LIST, pad, list_top, list_width, list_height);
    place(ID_STATUS, pad, list_top + list_height + small, list_width, status_height);

    if let Some(list) = controls.get(&ID_LIST) {
        let cell_pad = scaled(hwnd, 16);
        let shortcut = text_width(hwnd, "Win+Ctrl+Alt+Shift+F12") + cell_pad;
        let status = text_width(hwnd, "Taken by an app") + cell_pad;
        let source = text_width(hwnd, "windows list") + cell_pad;
        let scrollbar = scaled(hwnd, 24);
        let owner = (list_width - shortcut - status - source - scrollbar).max(scaled(hwnd, 120));
        for (column, cx) in [shortcut, status, owner, source].into_iter().enumerate() {
            unsafe { SendMessageW(*list, LVM_SETCOLUMNWIDTH, column, cx as LPARAM) };
        }
    }
}


fn run_scan(hwnd: HWND) {
    // Safe here: this runs straight from the message loop, so no host borrow is
    // on the stack and the read-only accessor cannot panic.
    let mine = host::registered_hotkeys();
    let result = probe::scan(hwnd, &mine);
    let line = result.status_line();
    log::info!("shortcut scan: {line}");

    DETECTORS.with(|cell| {
        if let Some(detector) = cell.borrow_mut().get_mut(&(hwnd as isize)) {
            detector.scan = Some(result);
        }
    });
    set_text(hwnd, ID_STATUS, &line);
    refill(hwnd);
}

fn refill(hwnd: HWND) {
    let search = get_text(hwnd, ID_SEARCH).to_lowercase();
    let modifiers = selected_modifiers(hwnd);
    let rows = DETECTORS.with(|cell| {
        let mut borrowed = cell.borrow_mut();
        let Some(detector) = borrowed.get_mut(&(hwnd as isize)) else {
            return Vec::new();
        };
        let Some(scan) = detector.scan.as_ref() else {
            return Vec::new();
        };

        let mut chosen: Vec<usize> = scan
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| detector.show_free || entry.status != Status::Free)
            .filter(|(_, entry)| match modifiers {
                Some(wanted) => entry.hotkey.modifiers & !MOD_NOREPEAT == wanted,
                None => true,
            })
            .filter(|(_, entry)| {
                search.is_empty()
                    || hotkeys::format(entry.hotkey).to_lowercase().contains(&search)
                    || entry.owner.to_lowercase().contains(&search)
                    || entry.status.label().to_lowercase().contains(&search)
            })
            .map(|(index, _)| index)
            .collect();

        let column = detector.sort_column;
        let ascending = detector.sort_ascending;
        chosen.sort_by(|a, b| {
            let order = sort_key(&scan.entries[*a], column).cmp(&sort_key(&scan.entries[*b], column));
            if ascending {
                order
            } else {
                order.reverse()
            }
        });
        detector.visible = chosen.clone();

        chosen
            .iter()
            .map(|index| row_text(&scan.entries[*index]))
            .collect::<Vec<[String; 4]>>()
    });

    let list = control(hwnd, ID_LIST);
    if list.is_null() {
        return;
    }
    unsafe { SendMessageW(list, LVM_DELETEALLITEMS, 0, 0) };
    for (row, cells) in rows.iter().enumerate() {
        for (column, cell) in cells.iter().enumerate() {
            let mut text = wide(cell);
            let mut item: LVITEMW = unsafe { std::mem::zeroed() };
            item.mask = LVIF_TEXT;
            item.iItem = row as i32;
            item.iSubItem = column as i32;
            item.pszText = text.as_mut_ptr();
            let message = if column == 0 {
                LVM_INSERTITEMW
            } else {
                LVM_SETITEMW
            };
            unsafe { SendMessageW(list, message, 0, &item as *const _ as LPARAM) };
        }
    }
}

fn sort_key(entry: &Entry, column: i32) -> String {
    match column {
        1 => entry.status.label().to_string(),
        2 => entry.owner.clone(),
        3 => entry.source.to_string(),
        _ => format!(
            "{:010} {}",
            entry.hotkey.modifiers & !MOD_NOREPEAT,
            hotkeys::format(entry.hotkey)
        ),
    }
}

fn row_text(entry: &Entry) -> [String; 4] {
    [
        hotkeys::format(entry.hotkey),
        entry.status.label().to_string(),
        entry.owner.clone(),
        entry.source.to_string(),
    ]
}

fn selected_modifiers(hwnd: HWND) -> Option<u32> {
    let combo = control(hwnd, ID_MODIFIERS);
    if combo.is_null() {
        return None;
    }
    let index = unsafe { SendMessageW(combo, CB_GETCURSEL, 0, 0) };
    if index <= 0 {
        return None;
    }
    probe::modifier_sets().get(index as usize - 1).copied()
}

fn copy_visible(hwnd: HWND) {
    let text = DETECTORS.with(|cell| {
        let borrowed = cell.borrow();
        let Some(detector) = borrowed.get(&(hwnd as isize)) else {
            return String::new();
        };
        let Some(scan) = detector.scan.as_ref() else {
            return String::new();
        };
        let mut out = String::from("Shortcut\tStatus\tOwner\tSource\r\n");
        for index in &detector.visible {
            let cells = row_text(&scan.entries[*index]);
            out.push_str(&cells.join("\t"));
            out.push_str("\r\n");
        }
        out
    });

    let rows = text.lines().count().saturating_sub(1);
    if !put_on_clipboard(hwnd, &text) {
        set_text(hwnd, ID_STATUS, "Could not open the clipboard.");
        return;
    }
    set_text(hwnd, ID_STATUS, &format!("Copied {rows} rows."));
}

fn put_on_clipboard(hwnd: HWND, text: &str) -> bool {
    let encoded = wide(text);
    let bytes = encoded.len() * 2;
    if unsafe { OpenClipboard(hwnd) } == 0 {
        return false;
    }
    let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) };
    if handle.is_null() {
        unsafe { CloseClipboard() };
        return false;
    }
    let target = unsafe { GlobalLock(handle) } as *mut u16;
    if target.is_null() {
        unsafe { CloseClipboard() };
        return false;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(encoded.as_ptr(), target, encoded.len());
        GlobalUnlock(handle);
        EmptyClipboard();
        // The clipboard owns the block from here on; freeing it would be a
        // double free once Windows is done with it.
        SetClipboardData(CF_UNICODETEXT as u32, handle);
        CloseClipboard();
    }
    true
}

fn control(hwnd: HWND, id: i32) -> HWND {
    DETECTORS.with(|cell| {
        cell.borrow()
            .get(&(hwnd as isize))
            .and_then(|detector| detector.controls.get(&id).copied())
            .unwrap_or(std::ptr::null_mut())
    })
}

fn set_text(hwnd: HWND, id: i32, text: &str) {
    let child = control(hwnd, id);
    if !child.is_null() {
        unsafe { SetWindowTextW(child, wide(text).as_ptr()) };
    }
}

fn get_text(hwnd: HWND, id: i32) -> String {
    let child = control(hwnd, id);
    if child.is_null() {
        return String::new();
    }
    let mut buffer = [0u16; 256];
    let taken = unsafe { GetWindowTextW(child, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..taken.max(0) as usize])
}

fn set_verdict(hwnd: HWND, text: &str, colour: COLORREF) {
    DETECTORS.with(|cell| {
        if let Some(detector) = cell.borrow_mut().get_mut(&(hwnd as isize)) {
            detector.verdict_colour = colour;
        }
    });
    set_text(hwnd, ID_VERDICT, text);
    let child = control(hwnd, ID_VERDICT);
    if !child.is_null() {
        unsafe {
            windows_sys::Win32::Graphics::Gdi::InvalidateRect(child, std::ptr::null(), 1);
        }
    }
}

fn start_capture(hwnd: HWND) {
    let already = DETECTORS.with(|cell| {
        cell.borrow()
            .get(&(hwnd as isize))
            .map(|detector| detector.hook != 0)
            .unwrap_or(true)
    });
    if already {
        return;
    }
    let hook = unsafe {
        SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_hook),
            GetModuleHandleW(std::ptr::null()),
            0,
        )
    };
    if hook.is_null() {
        log::warn!("could not install the keyboard hook for the capture box");
        return;
    }
    CAPTURING.with(|cell| *cell.borrow_mut() = hwnd as isize);
    DETECTORS.with(|cell| {
        if let Some(detector) = cell.borrow_mut().get_mut(&(hwnd as isize)) {
            detector.hook = hook as isize;
            detector.capture_modifiers = 0;
        }
    });
    set_verdict(hwnd, "Press a combination\u{2026}", COLOUR_PLAIN);
}

fn stop_capture(hwnd: HWND) {
    let hook = DETECTORS.with(|cell| {
        cell.borrow_mut()
            .get_mut(&(hwnd as isize))
            .map(|detector| {
                let hook = detector.hook;
                detector.hook = 0;
                detector.capture_modifiers = 0;
                hook
            })
            .unwrap_or(0)
    });
    if hook != 0 {
        unsafe { UnhookWindowsHookEx(hook as *mut core::ffi::c_void) };
    }
    CAPTURING.with(|cell| {
        if *cell.borrow() == hwnd as isize {
            *cell.borrow_mut() = 0;
        }
    });
}

fn modifier_bit(vk: u32) -> Option<u32> {
    match vk as u16 {
        v if v == VK_SHIFT || v == VK_LSHIFT || v == VK_RSHIFT => Some(MOD_SHIFT),
        v if v == VK_MENU || v == VK_LMENU || v == VK_RMENU => Some(MOD_ALT),
        v if v == VK_LWIN || v == VK_RWIN => Some(MOD_WIN),
        0x11 | 0xA2 => Some(MOD_CONTROL),
        v if v == VK_RCONTROL => Some(MOD_CONTROL),
        _ => None,
    }
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }
    let owner = CAPTURING.with(|cell| *cell.borrow());
    if owner == 0 {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }
    let hwnd = owner as HWND;
    let event = wparam as u32;
    let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
    let down = event == WM_KEYDOWN || event == WM_SYSKEYDOWN;

    let handled = if let Some(bit) = modifier_bit(info.vkCode) {
        DETECTORS.with(|cell| {
            if let Some(detector) = cell.borrow_mut().get_mut(&owner) {
                if down {
                    detector.capture_modifiers |= bit;
                } else {
                    detector.capture_modifiers &= !bit;
                }
            }
        });
        if down {
            let modifiers = DETECTORS
                .with(|cell| cell.borrow().get(&owner).map(|d| d.capture_modifiers))
                .unwrap_or(0);
            set_text(hwnd, ID_CAPTURE, &format!("{}\u{2026}", hotkeys::format_modifiers(modifiers)));
        }
        false
    } else if down {
        let modifiers = DETECTORS
            .with(|cell| cell.borrow().get(&owner).map(|d| d.capture_modifiers))
            .unwrap_or(0);
        if info.vkCode as u16 == 0x1B && modifiers == 0 {
            set_text(hwnd, ID_CAPTURE, "");
            set_verdict(hwnd, "", COLOUR_PLAIN);
        } else {
            show_verdict(hwnd, modifiers, info.vkCode);
        }
        true
    } else {
        false
    };

    if handled {
        // Swallow it, or testing Win+E in the box would open File Explorer.
        return 1;
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

fn show_verdict(hwnd: HWND, modifiers: u32, vk: u32) {
    let hotkey = Hotkey {
        modifiers: modifiers | MOD_NOREPEAT,
        vk,
    };
    set_text(hwnd, ID_CAPTURE, &hotkeys::format(hotkey));

    let mine = host::registered_hotkeys();
    let entry = probe::verdict(hwnd, hotkey, &mine);
    let colour = match entry.status {
        Status::Free => COLOUR_FREE,
        Status::TakenByApp => COLOUR_TAKEN,
        Status::Windows | Status::WinCraft => COLOUR_SYSTEM,
    };

    let mut text = probe::verdict_text(&entry);
    let at_last_scan = DETECTORS.with(|cell| {
        cell.borrow()
            .get(&(hwnd as isize))
            .and_then(|detector| detector.scan.as_ref())
            .and_then(|scan| scan.find(hotkey))
            .map(|found| found.status)
    });
    if at_last_scan.is_some_and(|before| before != entry.status) {
        text.push_str("  (changed since the last scan \u{2014} press F5)");
    }
    set_verdict(hwnd, &text, colour);
}

unsafe extern "system" fn forward_f5(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if msg == WM_KEYDOWN && wparam as u16 == 0x74 {
        let parent = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetParent(hwnd) };
        if !parent.is_null() {
            unsafe { PostMessageW(parent, WM_APP_SCAN, 0, 0) };
            return 0;
        }
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_SCAN => {
            run_scan(hwnd);
            0
        }
        WM_SIZE => {
            layout(hwnd);
            0
        }
        WM_GETMINMAXINFO => {
            let info = unsafe { &mut *(lparam as *mut MINMAXINFO) };
            info.ptMinTrackSize.x = scaled(hwnd, MIN_WIDTH);
            info.ptMinTrackSize.y = scaled(hwnd, MIN_HEIGHT);
            0
        }
        WM_DPICHANGED => {
            let suggested = unsafe { &*(lparam as *const RECT) };
            unsafe {
                SetWindowPos(
                    hwnd,
                    HWND_TOP,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
            let font = message_font(hwnd);
            DETECTORS.with(|cell| {
                if let Some(detector) = cell.borrow_mut().get_mut(&(hwnd as isize)) {
                    let old = detector.font;
                    detector.font = font;
                    for child in detector.controls.values() {
                        unsafe { SendMessageW(*child, WM_SETFONT, font as WPARAM, 1) };
                    }
                    if !old.is_null() {
                        unsafe { DeleteObject(old as _) };
                    }
                }
            });
            layout(hwnd);
            0
        }
        WM_KEYDOWN if wparam as u16 == 0x74 => {
            unsafe { PostMessageW(hwnd, WM_APP_SCAN, 0, 0) };
            0
        }
        WM_CTLCOLORSTATIC => {
            let child = lparam as HWND;
            let verdict = control(hwnd, ID_VERDICT);
            if child == verdict && !verdict.is_null() {
                let colour = DETECTORS.with(|cell| {
                    cell.borrow()
                        .get(&(hwnd as isize))
                        .map(|detector| detector.verdict_colour)
                        .unwrap_or(COLOUR_PLAIN)
                });
                unsafe {
                    SetTextColor(wparam as _, colour);
                    SetBkMode(wparam as _, TRANSPARENT as i32);
                }
                return unsafe {
                    windows_sys::Win32::Graphics::Gdi::GetStockObject(
                        windows_sys::Win32::Graphics::Gdi::HOLLOW_BRUSH,
                    )
                } as LRESULT;
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as i32;
            let notification = ((wparam >> 16) & 0xFFFF) as u32;
            match (id, notification) {
                (ID_REFRESH, _) => unsafe {
                    PostMessageW(hwnd, WM_APP_SCAN, 0, 0);
                },
                (ID_COPY, _) => copy_visible(hwnd),
                (ID_SEARCH, EN_CHANGE) => refill(hwnd),
                (ID_MODIFIERS, CBN_SELCHANGE) => refill(hwnd),
                (ID_SHOW_FREE, _) => {
                    let checkbox = control(hwnd, ID_SHOW_FREE);
                    let checked = unsafe { SendMessageW(checkbox, BM_GETCHECK, 0, 0) }
                        == BST_CHECKED as LRESULT;
                    DETECTORS.with(|cell| {
                        if let Some(detector) = cell.borrow_mut().get_mut(&(hwnd as isize)) {
                            detector.show_free = checked;
                        }
                    });
                    refill(hwnd);
                }
                (ID_CAPTURE, EN_SETFOCUS) => start_capture(hwnd),
                (ID_CAPTURE, EN_KILLFOCUS) => stop_capture(hwnd),
                _ => {}
            }
            0
        }
        WM_NOTIFY => {
            let header = unsafe { &*(lparam as *const NMHDR) };
            if header.idFrom as i32 == ID_LIST && header.code == LVN_COLUMNCLICK {
                let notice = unsafe { &*(lparam as *const NMLISTVIEW) };
                DETECTORS.with(|cell| {
                    if let Some(detector) = cell.borrow_mut().get_mut(&(hwnd as isize)) {
                        if detector.sort_column == notice.iSubItem {
                            detector.sort_ascending = !detector.sort_ascending;
                        } else {
                            detector.sort_column = notice.iSubItem;
                            detector.sort_ascending = true;
                        }
                    }
                });
                refill(hwnd);
            }
            0
        }
        WM_CLOSE => {
            unsafe { DestroyWindow(hwnd) };
            0
        }
        WM_DESTROY => {
            stop_capture(hwnd);
            DETECTORS.with(|cell| {
                if let Some(detector) = cell.borrow_mut().remove(&(hwnd as isize)) {
                    for id in [ID_SEARCH, ID_CAPTURE, ID_LIST] {
                        if let Some(child) = detector.controls.get(&id) {
                            unsafe {
                                RemoveWindowSubclass(*child, Some(forward_f5), SUBCLASS_FORWARD_F5)
                            };
                        }
                    }
                    if !detector.font.is_null() {
                        unsafe { DeleteObject(detector.font as _) };
                    }
                }
            });
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

pub fn set_show_free_default(hwnd: HWND, show_free: bool) {
    DETECTORS.with(|cell| {
        if let Some(detector) = cell.borrow_mut().get_mut(&(hwnd as isize)) {
            detector.show_free = show_free;
        }
    });
    let checkbox = control(hwnd, ID_SHOW_FREE);
    if !checkbox.is_null() {
        let state = if show_free { BST_CHECKED } else { BST_UNCHECKED };
        unsafe { SendMessageW(checkbox, BM_SETCHECK, state as WPARAM, 0) };
    }
}
