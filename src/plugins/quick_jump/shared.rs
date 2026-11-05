//! What the plugin and its windows on the UI thread share: a view of the
//! pins and the open windows, and the requests the windows make. A request
//! is handed over with a message to the plugin's window, since only the
//! host thread may touch the plugin.

use std::sync::{Arc, Mutex};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

use super::matching::{Live, SlotView};
use super::pins::Pin;

/// Posted to the plugin's window when a request is waiting.
pub const WM_APP_REQUEST: u32 = WM_APP + 43;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Jump(usize),
    /// Add this window to the end of the list.
    Add(isize),
    /// Move a pin to another place; the others close up around it.
    Move {
        from: usize,
        to: usize,
    },
    Remove(usize),
    /// The details panel's Done: the pin as edited.
    Update {
        slot: usize,
        pin: Pin,
    },
    /// A window is open and wants fresh data.
    Refresh,
}

/// An app the details panel can pick.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct App {
    pub exe: String,
    pub app_id: Option<String>,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub slots: Vec<SlotView>,
    /// The pins themselves, in order, for the details panel.
    pub pins: Vec<Pin>,
    /// The open app windows, top of the z-order first.
    pub windows: Vec<Live>,
    /// Running apps, then the pinned ones that are not running.
    pub apps: Vec<App>,
    /// The shortcuts as the user set them; empty for none.
    pub jump_keys: String,
    pub pin_keys: String,
}

#[derive(Default)]
pub struct Shared {
    pub view: View,
    requests: Vec<Request>,
    /// The plugin's own window while the plugin is on; 0 when off.
    pub plugin_window: isize,
    /// Quick access is on screen, or was asked for.
    pub list_open: bool,
    /// The picker is on screen (the value is not used).
    pub picker: Option<usize>,
    /// Quick access should select this pin when it next draws.
    pub select: Option<usize>,
    /// Quick access opens where the user left it, not in the middle.
    pub keep_spot: bool,
    /// Where the user last left Quick access.
    pub spot: Option<super::spot::Spot>,
    /// Where Quick access was when a jump left from it, so the monitor
    /// hint knows which monitor the user was looking at.
    pub jumped_from: Option<crate::core::monitors::Rect>,
}

pub type Handle = Arc<Mutex<Shared>>;

pub fn new() -> Handle {
    Arc::new(Mutex::new(Shared::default()))
}

impl Shared {
    /// Queues a request and wakes the plugin; nothing happens while it is
    /// off.
    pub fn send(&mut self, request: Request) {
        if self.plugin_window == 0 {
            return;
        }
        if !self.requests.contains(&request) {
            self.requests.push(request);
        }
        unsafe { PostMessageW(self.plugin_window as HWND, WM_APP_REQUEST, 0, 0) };
    }

    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_wait_once_each_and_only_while_the_plugin_is_on() {
        let mut shared = Shared::default();
        shared.send(Request::Refresh);
        assert!(shared.take_requests().is_empty(), "off: dropped");
        // A window handle no window has: the post fails quietly.
        shared.plugin_window = 1;
        shared.send(Request::Refresh);
        shared.send(Request::Refresh);
        shared.send(Request::Jump(2));
        assert_eq!(
            shared.take_requests(),
            vec![Request::Refresh, Request::Jump(2)]
        );
        assert!(shared.take_requests().is_empty());
    }
}
