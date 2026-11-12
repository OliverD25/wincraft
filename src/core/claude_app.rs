//! Opening a chat inside the Claude desktop app's main window. A chat that
//! has no window of its own is only a button in the app's sidebar list, and
//! the app has no link that opens one (`claude://code/continue` starts an
//! empty chat). So WinCraft finds the main window through UI Automation,
//! brings it forward and presses the sidebar button that has the chat's
//! title, as a screen reader would.
//!
//! The tree walks are generic over a small [`Tree`], so tests run them on a
//! made-up tree. The real tree is the one of `core/uia.rs`; every call into
//! it runs on a thread of its own that started COM.

use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowThreadProcessId, IsWindowVisible, SW_SHOWNORMAL,
};

use crate::core::uia::{self, Automation};
use crate::core::{com, instance, wide, window_actions, windows_list};

/// The class of the app's windows (it is an Electron app).
const WINDOW_CLASS: &str = "Chrome_WidgetWin_1";
const PROGRAM: &str = "\\claude.exe";
const SIDEBAR: &str = "Sidebar";
/// How deep and how far the walk for the sidebar group goes.
const SIDEBAR_DEPTH: usize = 14;
const SIDEBAR_ITEMS: usize = 5000;
/// How deep and how far the walk for the chat's button goes.
const BUTTON_DEPTH: usize = 8;
const BUTTON_ITEMS: usize = 2000;
/// The most children read from one element.
const MOST_CHILDREN: usize = 500;
/// How long a freshly started app gets to show its main window.
const START_WAIT: Duration = Duration::from_secs(5);
const START_STEP: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Button,
    Group,
    Other,
}

/// What the walks need to know about an element tree.
pub trait Tree {
    type Node;
    fn children(&self, node: &Self::Node) -> Vec<Self::Node>;
    fn name(&self, node: &Self::Node) -> Option<String>;
    fn kind(&self, node: &Self::Node) -> Kind;
}

/// The group named `Sidebar` nearest to the root, looking at most
/// `SIDEBAR_DEPTH` levels down and at `SIDEBAR_ITEMS` elements.
pub fn find_sidebar<T: Tree>(tree: &T, root: &T::Node) -> Option<T::Node> {
    let mut level = tree.children(root);
    let mut seen = 0;
    for _ in 0..SIDEBAR_DEPTH {
        let mut next = Vec::new();
        for node in level {
            seen += 1;
            if seen > SIDEBAR_ITEMS {
                return None;
            }
            if tree.kind(&node) == Kind::Group && tree.name(&node).as_deref() == Some(SIDEBAR) {
                return Some(node);
            }
            next.extend(tree.children(&node));
        }
        if next.is_empty() {
            return None;
        }
        level = next;
    }
    None
}

/// The button under `sidebar` whose name is `title` (trimmed, exact), in
/// reading order, and how many buttons have that name.
pub struct Found<N> {
    pub node: N,
    pub matches: usize,
}

pub fn find_button<T: Tree>(tree: &T, sidebar: &T::Node, title: &str) -> Option<Found<T::Node>> {
    let title = title.trim();
    let mut stack: Vec<(T::Node, usize)> = tree
        .children(sidebar)
        .into_iter()
        .rev()
        .map(|node| (node, 1))
        .collect();
    let mut seen = 0;
    let mut found: Option<Found<T::Node>> = None;
    while let Some((node, depth)) = stack.pop() {
        seen += 1;
        if seen > BUTTON_ITEMS {
            break;
        }
        if tree.kind(&node) == Kind::Button
            && tree.name(&node).is_some_and(|name| name.trim() == title)
        {
            match &mut found {
                Some(found) => found.matches += 1,
                None => found = Some(Found { node, matches: 1 }),
            }
            continue;
        }
        if depth < BUTTON_DEPTH {
            stack.extend(
                tree.children(&node)
                    .into_iter()
                    .rev()
                    .map(|child| (child, depth + 1)),
            );
        }
    }
    found
}

/// The words of the notice when the chat cannot be opened. It names
/// ClaudeDeck, so the user knows who is talking.
pub fn not_found_notice(title: &str) -> String {
    format!("ClaudeDeck could not find \"{title}\" in the Claude app's list. Open it there.")
}

struct Real<'a>(&'a Automation);

impl Tree for Real<'_> {
    type Node = com::ComPtr;

    fn children(&self, node: &Self::Node) -> Vec<Self::Node> {
        let mut children = Vec::new();
        let mut next = self.0.first_child(node);
        while let Some(child) = next {
            next = self.0.next_sibling(&child);
            children.push(child);
            if children.len() >= MOST_CHILDREN {
                break;
            }
        }
        children
    }

    fn name(&self, node: &Self::Node) -> Option<String> {
        uia::name(node)
    }

    fn kind(&self, node: &Self::Node) -> Kind {
        match uia::control_type(node) {
            Some(uia::CONTROL_BUTTON) => Kind::Button,
            Some(uia::CONTROL_GROUP) => Kind::Group,
            _ => Kind::Other,
        }
    }
}

/// The visible windows of the app, whose program is `claude.exe`.
fn app_windows() -> Vec<HWND> {
    windows_list::top_windows()
        .into_iter()
        .filter(|hwnd| unsafe { IsWindowVisible(*hwnd) } != 0)
        .filter(|hwnd| windows_list::class_name(*hwnd) == WINDOW_CLASS)
        .filter(|hwnd| {
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(*hwnd, &mut pid) };
            windows_list::exe_path(pid)
                .to_lowercase()
                .ends_with(PROGRAM)
        })
        .collect()
}

/// The main window: the app window that has a sidebar, with the sidebar.
/// The windows of popped-out chats have none.
fn locate(automation: &Automation) -> Option<(HWND, com::ComPtr)> {
    app_windows().into_iter().find_map(|hwnd| {
        let root = automation.element_from_handle(hwnd).ok()?;
        find_sidebar(&Real(automation), &root).map(|sidebar| (hwnd, sidebar))
    })
}

fn start_app() {
    let verb = wide("open");
    let link = wide("claude://");
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            link.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
}

/// Does the work on a COM thread. `dry`: a test copy only reads.
fn run(title: &str, dry: bool) -> Result<(), String> {
    let missing = || not_found_notice(title);
    let automation = Automation::new().map_err(|reason| {
        log::warn!("ClaudeDeck: opening \"{title}\" in the Claude app: {reason}");
        missing()
    })?;
    let mut deadline: Option<Instant> = None;
    let mut brought = false;
    loop {
        if let Some((window, sidebar)) = locate(&automation) {
            if !brought && !dry {
                window_actions::activate(window);
            }
            brought = true;
            if let Some(found) = find_button(&Real(&automation), &sidebar, title) {
                if found.matches > 1 {
                    log::info!(
                        "ClaudeDeck: {} chats in the Claude app are named \"{title}\"; the first is used",
                        found.matches
                    );
                }
                if dry {
                    log::info!(
                        "read-only test instance: pressing \"{title}\" in the Claude app skipped"
                    );
                    return Ok(());
                }
                return uia::invoke(&found.node).map_err(|reason| {
                    log::warn!("ClaudeDeck: could not press \"{title}\": {reason}");
                    missing()
                });
            }
            if deadline.is_none() {
                return Err(missing());
            }
        } else if deadline.is_none() {
            if dry {
                log::info!("read-only test instance: starting the Claude app skipped");
                return Ok(());
            }
            start_app();
            deadline = Some(Instant::now() + START_WAIT);
        }
        if deadline.is_none_or(|end| Instant::now() >= end) {
            return Err(missing());
        }
        std::thread::sleep(START_STEP);
    }
}

/// Opens the chat with this title in the app's main window, on a thread of
/// its own. `failed` gets the notice text when the chat cannot be found.
/// A test copy of WinCraft only reads; unit tests start nothing.
pub fn open_chat(title: &str, failed: impl FnOnce(String) + Send + 'static) {
    if cfg!(test) {
        return;
    }
    let title = title.trim().to_string();
    let dry = instance::is_test();
    std::thread::spawn(move || {
        let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
        if hr < 0 {
            log::warn!("ClaudeDeck: COM did not start ({})", com::hex(hr));
            failed(not_found_notice(&title));
            return;
        }
        let outcome = run(&title, dry);
        unsafe { CoUninitialize() };
        if let Err(text) = outcome {
            failed(text);
        }
    });
}

#[cfg(test)]
mod tests;
