//! Working with the Claude desktop app's main window. A chat that has no
//! window of its own is only a button in the app's chat list, and the app
//! has no link that opens one (`claude://code/continue` starts an empty
//! chat). So WinCraft finds the home window through UI Automation, brings
//! it forward and presses the chat's button, as a screen reader would.
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
const HOME_TITLE: &str = "Claude Home";
const SIDEBAR: &str = "Sidebar";
const SHOW_SIDEBAR: &str = "Show sidebar";
/// How deep and how far a walk through a whole window goes. The chat
/// buttons are not inside the `Sidebar` group, so the whole window is
/// searched.
const DEPTH: usize = 40;
const ITEMS: usize = 8000;
/// The most children read from one element.
const MOST_CHILDREN: usize = 500;
/// How long a freshly started app gets to show its main window.
const START_WAIT: Duration = Duration::from_secs(5);
const START_STEP: Duration = Duration::from_millis(250);
/// How long the chat list gets to open after `Expand`.
const EXPAND_WAIT: Duration = Duration::from_millis(1500);
const EXPAND_STEP: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Button,
    Group,
    MenuItem,
    Edit,
    Other,
}

/// What the walks need to know about an element tree.
pub trait Tree {
    type Node;
    fn children(&self, node: &Self::Node) -> Vec<Self::Node>;
    fn name(&self, node: &Self::Node) -> Option<String>;
    fn kind(&self, node: &Self::Node) -> Kind;
}

/// A node a walk found, and how many nodes matched.
pub struct Found<N> {
    pub node: N,
    pub matches: usize,
}

/// Walks below `root` in reading order, down `depth` levels and over
/// `items` elements, and returns the first node that `matches`. With
/// `count_all` the walk goes on to count the other matches.
fn search<T: Tree>(
    tree: &T,
    root: &T::Node,
    depth: usize,
    items: usize,
    count_all: bool,
    matches: &dyn Fn(&T, &T::Node) -> bool,
) -> Option<Found<T::Node>> {
    let mut stack: Vec<(T::Node, usize)> = tree
        .children(root)
        .into_iter()
        .rev()
        .map(|node| (node, 1))
        .collect();
    let mut seen = 0;
    let mut found: Option<Found<T::Node>> = None;
    while let Some((node, level)) = stack.pop() {
        seen += 1;
        if seen > items {
            break;
        }
        if matches(tree, &node) {
            match &mut found {
                Some(found) => found.matches += 1,
                None => found = Some(Found { node, matches: 1 }),
            }
            if !count_all {
                break;
            }
            continue;
        }
        if level < depth {
            stack.extend(
                tree.children(&node)
                    .into_iter()
                    .rev()
                    .map(|child| (child, level + 1)),
            );
        }
    }
    found
}

fn named<T: Tree>(tree: &T, node: &T::Node, kind: Kind, name: &str) -> bool {
    tree.kind(node) == kind && tree.name(node).is_some_and(|text| text.trim() == name)
}

/// The button whose name is `title` (trimmed, exact), first in reading
/// order, and how many buttons have that name.
pub fn find_button<T: Tree>(tree: &T, root: &T::Node, title: &str) -> Option<Found<T::Node>> {
    let title = title.trim();
    search(tree, root, DEPTH, ITEMS, true, &|tree, node| {
        named(tree, node, Kind::Button, title)
    })
}

/// The first element of this kind and exact name.
pub fn find_named<T: Tree>(
    tree: &T,
    root: &T::Node,
    kind: Kind,
    name: &str,
) -> Option<Found<T::Node>> {
    search(tree, root, DEPTH, ITEMS, false, &|tree, node| {
        named(tree, node, kind, name)
    })
}

/// The first edit field below `root`.
pub fn find_edit<T: Tree>(tree: &T, root: &T::Node) -> Option<Found<T::Node>> {
    search(tree, root, DEPTH, ITEMS, false, &|tree, node| {
        tree.kind(node) == Kind::Edit
    })
}

/// Whether this window is the app's home window: it has the chat list or
/// the button that opens it.
fn is_home<T: Tree>(tree: &T, root: &T::Node) -> bool {
    find_named(tree, root, Kind::Group, SIDEBAR).is_some()
        || find_named(tree, root, Kind::Button, SHOW_SIDEBAR).is_some()
}

/// The words of the notice when the chat cannot be opened. It names
/// ClaudeDeck, so the user knows who is talking.
pub fn not_found_notice(title: &str) -> String {
    format!("ClaudeDeck could not find \"{title}\" in the Claude app's list. Open it there.")
}

pub(crate) struct Real<'a>(pub(crate) &'a Automation);

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
            Some(uia::CONTROL_MENU_ITEM) => Kind::MenuItem,
            Some(uia::CONTROL_EDIT) => Kind::Edit,
            _ => Kind::Other,
        }
    }
}

/// The visible top-level windows of the app (`claude.exe`), those with the
/// app's window class first.
pub(crate) fn app_windows() -> Vec<HWND> {
    let mut windows: Vec<HWND> = windows_list::top_windows()
        .into_iter()
        .filter(|hwnd| unsafe { IsWindowVisible(*hwnd) } != 0)
        .filter(|hwnd| {
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(*hwnd, &mut pid) };
            windows_list::exe_path(pid)
                .to_lowercase()
                .ends_with(PROGRAM)
        })
        .collect();
    windows.sort_by_key(|hwnd| windows_list::class_name(*hwnd) != WINDOW_CLASS);
    windows
}

/// The home window: the one titled "Claude Home", else the app window
/// that has a chat list. Popped-out chat windows have neither.
fn locate(automation: &Automation) -> Option<(HWND, com::ComPtr)> {
    let mut windows: Vec<HWND> = app_windows()
        .into_iter()
        .filter(|hwnd| windows_list::class_name(*hwnd) == WINDOW_CLASS)
        .collect();
    windows.sort_by_key(|hwnd| windows_list::window_text(*hwnd) != HOME_TITLE);
    windows.into_iter().find_map(|hwnd| {
        let root = automation.element_from_handle(hwnd).ok()?;
        let titled = windows_list::window_text(hwnd) == HOME_TITLE;
        (titled || is_home(&Real(automation), &root)).then_some((hwnd, root))
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

/// Calls `step` until it gives something or `wait` is over.
pub(crate) fn poll<R>(
    wait: Duration,
    every: Duration,
    mut step: impl FnMut() -> Option<R>,
) -> Option<R> {
    let end = Instant::now() + wait;
    loop {
        if let Some(found) = step() {
            return Some(found);
        }
        if Instant::now() >= end {
            return None;
        }
        std::thread::sleep(every);
    }
}

/// A chat's button in the home window.
pub(crate) struct Chat {
    pub window: HWND,
    pub root: com::ComPtr,
    pub button: com::ComPtr,
    pub matches: usize,
    /// The "Show sidebar" button, when the list had to be opened for this.
    pub opened: Option<com::ComPtr>,
}

/// Finds the chat's button. The home window is brought forward; when the
/// list is hidden (a narrow window), it is opened through the "Show
/// sidebar" button. `start` starts the app when it is not running. `dry`:
/// nothing is moved, opened or started.
pub(crate) fn find_chat(
    automation: &Automation,
    title: &str,
    start: bool,
    dry: bool,
) -> Result<Option<Chat>, String> {
    let tree = Real(automation);
    let mut deadline: Option<Instant> = None;
    let mut brought = false;
    loop {
        if let Some((window, root)) = locate(automation) {
            if !brought && !dry {
                window_actions::activate(window);
            }
            brought = true;
            if let Some(found) = find_button(&tree, &root, title) {
                return Ok(Some(Chat {
                    window,
                    root,
                    button: found.node,
                    matches: found.matches,
                    opened: None,
                }));
            }
            let hidden = find_named(&tree, &root, Kind::Button, SHOW_SIDEBAR)
                .filter(|show| uia::is_collapsed(&show.node) == Some(true));
            if let Some(show) = hidden {
                if dry {
                    log::info!("read-only test instance: opening the chat list skipped");
                    return Ok(None);
                }
                uia::expand(&show.node)?;
                let found = poll(EXPAND_WAIT, EXPAND_STEP, || {
                    find_button(&tree, &root, title)
                });
                return Ok(found.map(|found| Chat {
                    window,
                    root,
                    button: found.node,
                    matches: found.matches,
                    opened: Some(show.node),
                }));
            }
            if deadline.is_none() {
                return Ok(None);
            }
        } else if deadline.is_none() {
            if dry || !start {
                log::info!("the Claude app's home window is not open; not starting it");
                return Ok(None);
            }
            start_app();
            deadline = Some(Instant::now() + START_WAIT);
        }
        if deadline.is_none_or(|end| Instant::now() >= end) {
            return Ok(None);
        }
        std::thread::sleep(START_STEP);
    }
}

/// Does the work on a COM thread. `dry`: a test copy only reads.
fn run(title: &str, dry: bool) -> Result<(), String> {
    let missing = || not_found_notice(title);
    let automation = Automation::new().map_err(|reason| {
        log::warn!("ClaudeDeck: opening \"{title}\" in the Claude app: {reason}");
        missing()
    })?;
    let chat = find_chat(&automation, title, true, dry)
        .map_err(|reason| {
            log::warn!("ClaudeDeck: the chat list of the Claude app: {reason}");
            missing()
        })?
        .ok_or_else(missing)?;
    if chat.matches > 1 {
        log::info!(
            "ClaudeDeck: {} chats in the Claude app are named \"{title}\"; the first is used",
            chat.matches
        );
    }
    if dry {
        log::info!("read-only test instance: pressing \"{title}\" in the Claude app skipped");
        return Ok(());
    }
    // The list stays as the app leaves it after the click.
    uia::invoke(&chat.button).map_err(|reason| {
        log::warn!("ClaudeDeck: could not press \"{title}\": {reason}");
        missing()
    })
}

/// Runs `work` on a thread of its own that started COM; `failed` gets a
/// notice text when `work` gives one.
pub(crate) fn on_com_thread(
    title: String,
    failed: impl FnOnce(String) + Send + 'static,
    work: impl FnOnce(&str) -> Result<(), String> + Send + 'static,
) {
    std::thread::spawn(move || {
        let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
        if hr < 0 {
            log::warn!("ClaudeDeck: COM did not start ({})", com::hex(hr));
            failed(not_found_notice(&title));
            return;
        }
        let outcome = work(&title);
        unsafe { CoUninitialize() };
        if let Err(text) = outcome {
            failed(text);
        }
    });
}

/// Opens the chat with this title in the app's main window, on a thread of
/// its own. `failed` gets the notice text when the chat cannot be found.
/// A test copy of WinCraft only reads; unit tests start nothing.
pub fn open_chat(title: &str, failed: impl FnOnce(String) + Send + 'static) {
    if cfg!(test) {
        return;
    }
    let dry = instance::is_test();
    on_com_thread(title.trim().to_string(), failed, move |title| {
        run(title, dry)
    });
}

#[cfg(test)]
mod tests;
