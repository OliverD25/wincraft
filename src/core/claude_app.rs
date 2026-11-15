//! Working with the Claude desktop app's main window. A chat that has no
//! window of its own is shown there with the app's link
//! (`claude://code/continue?session=<id>`), which needs the session file's
//! `local_` id (see `link.rs`); any other id starts an empty chat, so it
//! gets no link. Opening a chat never presses buttons in the app. UI
//! Automation stays only for renaming, which has no link or command.
//!
//! The tree walks are generic over a small [`Tree`], so tests run them on a
//! made-up tree. The real tree is the one of `core/uia.rs`; every call into
//! it runs on a thread of its own that started COM.

use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindowVisible};

use crate::core::uia::{self, Automation};
use crate::core::{com, windows_list};

/// The class of the app's windows (it is an Electron app).
const WINDOW_CLASS: &str = "Chrome_WidgetWin_1";
const PROGRAM: &str = "\\claude.exe";
const HOME_TITLE: &str = "Claude Home";
const SIDEBAR: &str = "Sidebar";
const SHOW_SIDEBAR: &str = "Show sidebar";
/// The radio button that switches the home window to the chats of Claude Code.
const CODE_VIEW: &str = "Code";
/// How deep and how far a walk through a whole window goes. The chat
/// buttons are not inside the `Sidebar` group, so the whole window is
/// searched.
const DEPTH: usize = 40;
const ITEMS: usize = 8000;
/// The most children read from one element.
const MOST_CHILDREN: usize = 500;
/// How long the app gets to show its main window after a link (the link
/// also starts the app when it is not running).
const HOME_WAIT: Duration = Duration::from_secs(5);
const HOME_STEP: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Button,
    Group,
    MenuItem,
    Edit,
    Radio,
    Other,
}

/// What the walks need to know about an element tree.
pub trait Tree {
    type Node;
    fn children(&self, node: &Self::Node) -> Vec<Self::Node>;
    fn name(&self, node: &Self::Node) -> Option<String>;
    fn kind(&self, node: &Self::Node) -> Kind;
    /// Whether the element is selected; None when it cannot say.
    fn selected(&self, node: &Self::Node) -> Option<bool>;
    /// Whether the element has the keyboard focus; None when it cannot say.
    fn focused(&self, node: &Self::Node) -> Option<bool>;
    /// The text of an edit box.
    fn value(&self, node: &Self::Node) -> Option<String>;
    fn expand(&self, node: &Self::Node) -> Result<(), String>;
    fn invoke(&self, node: &Self::Node) -> Result<(), String>;
    /// The first descendant of this kind and exact name. The default walks
    /// the tree element by element; the real tree asks the app that draws it.
    fn find_first(&self, root: &Self::Node, kind: Kind, name: &str) -> Option<Self::Node>
    where
        Self: Sized,
    {
        search(self, root, DEPTH, ITEMS, false, &|tree, node| {
            named(tree, node, kind, name)
        })
        .map(|found| found.node)
    }
    /// Every descendant of this kind and exact name, in reading order.
    fn find_all(&self, root: &Self::Node, kind: Kind, name: &str) -> Vec<Self::Node>
    where
        Self: Sized,
    {
        let mut all = Vec::new();
        let mut stack: Vec<(Self::Node, usize)> = self
            .children(root)
            .into_iter()
            .rev()
            .map(|node| (node, 1))
            .collect();
        let mut seen = 0;
        while let Some((node, level)) = stack.pop() {
            seen += 1;
            if seen > ITEMS {
                break;
            }
            if level < DEPTH {
                stack.extend(
                    self.children(&node)
                        .into_iter()
                        .rev()
                        .map(|child| (child, level + 1)),
                );
            }
            if named(self, &node, kind, name) {
                all.push(node);
            }
        }
        all
    }
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

/// The first element of this kind and exact name.
pub fn find_named<T: Tree>(
    tree: &T,
    root: &T::Node,
    kind: Kind,
    name: &str,
) -> Option<Found<T::Node>> {
    tree.find_first(root, kind, name)
        .map(|node| Found { node, matches: 1 })
}

/// Whether this window is the app's home window: it has the chat list or
/// the button that opens it.
fn is_home<T: Tree>(tree: &T, root: &T::Node) -> bool {
    find_named(tree, root, Kind::Group, SIDEBAR).is_some()
        || find_named(tree, root, Kind::Button, SHOW_SIDEBAR).is_some()
        || find_named(tree, root, Kind::Radio, CODE_VIEW).is_some()
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
            Some(uia::CONTROL_RADIO) => Kind::Radio,
            _ => Kind::Other,
        }
    }

    fn selected(&self, node: &Self::Node) -> Option<bool> {
        uia::is_selected(node)
    }

    fn focused(&self, node: &Self::Node) -> Option<bool> {
        uia::has_focus(node)
    }

    fn value(&self, node: &Self::Node) -> Option<String> {
        uia::value(node)
    }

    fn expand(&self, node: &Self::Node) -> Result<(), String> {
        uia::expand(node)
    }

    fn invoke(&self, node: &Self::Node) -> Result<(), String> {
        uia::invoke(node)
    }

    fn find_first(&self, root: &Self::Node, kind: Kind, name: &str) -> Option<Self::Node> {
        self.0.find_first(root, name, control_type_of(kind))
    }

    fn find_all(&self, root: &Self::Node, kind: Kind, name: &str) -> Vec<Self::Node> {
        self.0.find_all(root, name, control_type_of(kind))
    }
}

/// The UI Automation control type of a kind; None for any type.
fn control_type_of(kind: Kind) -> Option<i32> {
    match kind {
        Kind::Button => Some(uia::CONTROL_BUTTON),
        Kind::Group => Some(uia::CONTROL_GROUP),
        Kind::MenuItem => Some(uia::CONTROL_MENU_ITEM),
        Kind::Edit => Some(uia::CONTROL_EDIT),
        Kind::Radio => Some(uia::CONTROL_RADIO),
        Kind::Other => None,
    }
}

/// Whether the window belongs to the app (`claude.exe`).
fn is_app_window(hwnd: HWND) -> bool {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    windows_list::exe_path(pid)
        .to_lowercase()
        .ends_with(PROGRAM)
}

/// The visible top-level windows of the app (`claude.exe`), those with the
/// app's window class first.
pub(crate) fn app_windows() -> Vec<HWND> {
    let mut windows: Vec<HWND> = windows_list::top_windows()
        .into_iter()
        .filter(|hwnd| unsafe { IsWindowVisible(*hwnd) } != 0)
        .filter(|hwnd| is_app_window(*hwnd))
        .collect();
    windows.sort_by_key(|hwnd| windows_list::class_name(*hwnd) != WINDOW_CLASS);
    windows
}

/// The home window: the one titled "Claude Home", else the app window
/// that has a chat list. Popped-out chat windows have neither.
fn locate(automation: &Automation) -> Option<(HWND, com::ComPtr)> {
    // The window title says it at once: no UI Automation look at the
    // other windows of the app is needed.
    let titled = windows_list::top_windows().into_iter().find(|hwnd| {
        let visible = unsafe { IsWindowVisible(*hwnd) } != 0;
        visible
            && windows_list::window_text(*hwnd) == HOME_TITLE
            && windows_list::class_name(*hwnd) == WINDOW_CLASS
            && is_app_window(*hwnd)
    });
    if let Some(root) = titled.and_then(|hwnd| {
        automation
            .element_from_handle(hwnd)
            .ok()
            .map(|root| (hwnd, root))
    }) {
        return Some(root);
    }
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

/// The home window, waited for: a link may have just started the app.
pub(crate) fn home_window(automation: &Automation) -> Option<(HWND, com::ComPtr)> {
    poll(HOME_WAIT, HOME_STEP, || locate(automation))
}

/// Runs `work` on a thread of its own that started COM; `say` gets the
/// notice text that `work` gives, as a result or as a failure, and
/// `failed` when COM does not start.
pub(crate) fn on_com_thread(
    title: String,
    failed: String,
    say: impl FnOnce(String) + Send + 'static,
    work: impl FnOnce(&str) -> Result<Option<String>, String> + Send + 'static,
) {
    std::thread::spawn(move || {
        let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
        if hr < 0 {
            log::warn!("ClaudeDeck: COM did not start ({})", com::hex(hr));
            say(failed);
            return;
        }
        let outcome = work(&title);
        unsafe { CoUninitialize() };
        if let Some(text) = outcome.unwrap_or_else(Some) {
            say(text);
        }
    });
}

/// `--claude-app-find=<title>`: the check a rename makes for the chat's
/// button in the open home window, read-only (no link is opened, nothing
/// is focused or pressed), for checking it against the live Claude app.
/// Returns one line to print.
pub fn find_report(title: &str) -> String {
    let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
    if hr < 0 {
        return format!("COM did not start ({})", com::hex(hr));
    }
    let began = Instant::now();
    let line = match Automation::new() {
        Err(reason) => format!("UI Automation did not start: {reason}"),
        Ok(automation) => match locate(&automation) {
            None => "not found: the Claude app's main window is not open".to_string(),
            Some((_, root)) => match guards::chat_button(&Real(&automation), &root, title) {
                Ok(_) => "found".to_string(),
                Err(stop) => format!("not found: {}", stop.reason()),
            },
        },
    };
    unsafe { CoUninitialize() };
    format!(
        "\"{}\": {line} in {} ms",
        title.trim(),
        began.elapsed().as_millis()
    )
}

pub mod guards;
mod keys;
mod link;
mod menu;
pub use link::{chat_link, show_chat, show_chat_then};
pub use menu::rename_chat;

#[cfg(test)]
mod tests;
