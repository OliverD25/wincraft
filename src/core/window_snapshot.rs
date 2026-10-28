//! The app windows, read on a thread of their own so that the host thread
//! never waits for them.
//!
//! When a Windows key goes down, the host asks for a snapshot (`read_later`).
//! When it is back, the host builds WindowDeck's strip from it inside
//! `while_used`; meanwhile `live_windows::on_desktops` and
//! `strip_groups::desktop_of` answer from the snapshot instead of asking
//! Windows again. Outside `while_used` nothing changes. Read only: nothing
//! here changes a window.
//!
//! The reading costs 10–20 ms on a desktop with 50 windows, and more while
//! Explorer is busy with the Windows key itself, because the desktop and app
//! id of each window are asked from Explorer.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Instant;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

use crate::core::live_windows::{self, known_desktop, LiveWindow, Read};
use crate::core::virtual_desktops::{self as desktops, DesktopId, Reader};
use crate::core::windows_list;

/// Every app window at one moment, top of the z-order first, with the
/// desktop each is on.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub windows: Vec<Read>,
    /// When the reading started.
    pub taken: Instant,
}

thread_local! {
    static IN_USE: RefCell<Option<Rc<Snapshot>>> = const { RefCell::new(None) };
}

/// Runs `work` with `snapshot` standing in for the windows open now.
pub fn while_used<R>(snapshot: Snapshot, work: impl FnOnce() -> R) -> R {
    struct Done(Option<Rc<Snapshot>>);
    impl Drop for Done {
        fn drop(&mut self) {
            let outer = self.0.take();
            IN_USE.with(|cell| *cell.borrow_mut() = outer);
        }
    }
    let outer = IN_USE.with(|cell| cell.borrow_mut().replace(Rc::new(snapshot)));
    let _done = Done(outer);
    work()
}

fn in_use() -> Option<Rc<Snapshot>> {
    IN_USE.with(|cell| cell.borrow().clone())
}

/// The windows of the programs `watched` accepts, from the snapshot in
/// use; None when none is.
pub fn live(watched: &dyn Fn(&str) -> bool) -> Option<Vec<LiveWindow>> {
    let snapshot = in_use()?;
    Some(
        snapshot
            .windows
            .iter()
            .filter(|window| watched(&live_windows::file_name(&window.exe_path)))
            .map(Read::live)
            .collect(),
    )
}

/// The desktop of a window in the snapshot in use: None when no snapshot
/// is in use or the window is not in it, Some(None) when it was not known.
pub fn desktop_of(hwnd: isize) -> Option<Option<DesktopId>> {
    let snapshot = in_use()?;
    snapshot
        .windows
        .iter()
        .find(|window| window.hwnd == hwnd)
        .map(|window| window.desktop)
}

/// Reads a snapshot on a thread of its own. It comes on the receiver, and
/// `done` runs on that thread once it is there. Nothing comes when Windows
/// cannot say which desktop the windows are on: the caller reads them itself
/// as before.
pub fn read_later(done: impl FnOnce() + Send + 'static) -> mpsc::Receiver<Snapshot> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
        if hr >= 0 {
            if let Some(snapshot) = read_now() {
                let _ = sender.send(snapshot);
            }
            unsafe { CoUninitialize() };
        }
        done();
    });
    receiver
}

/// Reads every app window now, the way `live_windows::on_desktops` does
/// with a reader. Every COM object is gone before it returns.
pub fn read_now() -> Option<Snapshot> {
    let taken = Instant::now();
    let reader = Reader::new().ok()?;
    let registry = desktops::list();
    let on_known_desktop = |hwnd: HWND| {
        reader
            .read(hwnd)
            .is_some_and(|id| known_desktop(id, &registry))
    };
    let mut path_of_pid: HashMap<u32, String> = HashMap::new();
    let windows = windows_list::app_windows(&on_known_desktop)
        .into_iter()
        .map(|hwnd| {
            let exe_path = live_windows::path_of(&mut path_of_pid, hwnd);
            let mut read = live_windows::read_one(hwnd, exe_path);
            read.desktop = reader.read(hwnd);
            read
        })
        .collect();
    Some(Snapshot { windows, taken })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::appid::AppId;

    fn window(hwnd: isize, exe_path: &str, title: &str, desktop: Option<u128>) -> Read {
        Read {
            hwnd,
            exe_path: exe_path.to_string(),
            title: title.to_string(),
            rect: [0, 0, 800, 600],
            maximized: false,
            app: AppId::default(),
            desktop: desktop.map(DesktopId),
        }
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            windows: vec![
                window(11, r"C:\Apps\Chrome.exe", "Mail", Some(7)),
                window(12, r"C:\Windows\notepad.exe", "notes.txt", None),
            ],
            taken: Instant::now(),
        }
    }

    #[test]
    fn nothing_is_answered_from_a_snapshot_outside_its_use() {
        assert!(live(&|_| true).is_none());
        assert_eq!(desktop_of(11), None);
        while_used(snapshot(), || assert!(live(&|_| true).is_some()));
        assert!(live(&|_| true).is_none(), "gone after the use");
        assert_eq!(desktop_of(11), None);
    }

    #[test]
    fn a_snapshot_in_use_gives_the_watched_windows_in_its_order() {
        while_used(snapshot(), || {
            let all = live(&|_| true).unwrap();
            let handles: Vec<isize> = all.iter().map(|w| w.hwnd as isize).collect();
            assert_eq!(handles, [11, 12]);
            assert_eq!(all[0].identity.exe, "chrome.exe");
            assert_eq!(all[0].group, r"c:\apps\chrome.exe");
            let chrome = live(&|exe| exe == "chrome.exe").unwrap();
            assert_eq!(chrome.len(), 1);
            assert_eq!(chrome[0].identity.title, "Mail");
        });
    }

    #[test]
    fn a_snapshot_in_use_knows_the_desktop_of_its_windows_only() {
        while_used(snapshot(), || {
            assert_eq!(desktop_of(11), Some(Some(DesktopId(7))));
            assert_eq!(desktop_of(12), Some(None), "read, but not known");
            assert_eq!(desktop_of(99), None, "not in the snapshot");
        });
    }

    #[test]
    fn a_snapshot_is_put_away_even_when_the_work_panics() {
        let outcome = std::panic::catch_unwind(|| while_used(snapshot(), || panic!("boom")));
        assert!(outcome.is_err());
        assert!(live(&|_| true).is_none());
    }

    #[test]
    fn a_read_window_becomes_the_same_live_window_enumerate_gives() {
        let read = window(5, r"C:\Apps\Code.exe", "main.rs", None);
        let live = read.live();
        assert_eq!(live.hwnd as isize, 5);
        assert_eq!(live.exe_path, r"C:\Apps\Code.exe");
        assert_eq!(live.identity.exe, "code.exe");
        assert_eq!(live.identity.rect, [0, 0, 800, 600]);
        assert_eq!(live.group, r"c:\apps\code.exe");
    }
}
