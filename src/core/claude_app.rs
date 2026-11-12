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
/// The radio button that switches the home window to the chats of Claude Code.
const CODE_VIEW: &str = "Code";
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
    Radio,
    Other,
}

/// What the walks need to know about an element tree.
pub trait Tree {
    type Node;
    fn children(&self, node: &Self::Node) -> Vec<Self::Node>;
    fn name(&self, node: &Self::Node) -> Option<String>;
    fn kind(&self, node: &Self::Node) -> Kind;
    /// Whether a radio button is selected; None when it cannot say.
    fn selected(&self, node: &Self::Node) -> Option<bool>;
    /// Whether an element with the ExpandCollapse pattern is collapsed.
    fn collapsed(&self, node: &Self::Node) -> Option<bool>;
    fn select(&self, node: &Self::Node) -> Result<(), String>;
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
/// order.
pub fn find_button<T: Tree>(tree: &T, root: &T::Node, title: &str) -> Option<Found<T::Node>> {
    find_named(tree, root, Kind::Button, title.trim())
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
        || find_named(tree, root, Kind::Radio, CODE_VIEW).is_some()
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
            Some(uia::CONTROL_RADIO) => Kind::Radio,
            _ => Kind::Other,
        }
    }

    fn selected(&self, node: &Self::Node) -> Option<bool> {
        uia::is_selected(node)
    }

    fn collapsed(&self, node: &Self::Node) -> Option<bool> {
        uia::is_collapsed(node)
    }

    fn select(&self, node: &Self::Node) -> Result<(), String> {
        uia::select(node).or_else(|_| uia::invoke(node))
    }

    fn expand(&self, node: &Self::Node) -> Result<(), String> {
        uia::expand(node)
    }

    fn invoke(&self, node: &Self::Node) -> Result<(), String> {
        uia::invoke(node)
    }

    fn find_first(&self, root: &Self::Node, kind: Kind, name: &str) -> Option<Self::Node> {
        let control_type = match kind {
            Kind::Button => Some(uia::CONTROL_BUTTON),
            Kind::Group => Some(uia::CONTROL_GROUP),
            Kind::MenuItem => Some(uia::CONTROL_MENU_ITEM),
            Kind::Edit => Some(uia::CONTROL_EDIT),
            Kind::Radio => Some(uia::CONTROL_RADIO),
            Kind::Other => None,
        };
        self.0.find_first(root, name, control_type)
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
    pub root: com::ComPtr,
    pub button: com::ComPtr,
    pub matches: usize,
    /// The "Show sidebar" button, when the list had to be opened for this.
    pub opened: Option<com::ComPtr>,
}

/// How long each step gets before the next one is tried.
pub struct Waits {
    /// After the Code view was selected.
    pub switch: Duration,
    /// After the list or a section was opened.
    pub expand: Duration,
    pub step: Duration,
}

const WAITS: Waits = Waits {
    switch: EXPAND_WAIT,
    expand: EXPAND_WAIT,
    step: EXPAND_STEP,
};

/// Names of buttons that look collapsible but are not a list section.
const NOT_SECTIONS: [&str; 19] = [
    "Minimize",
    "Restore",
    "Close",
    "Menu",
    "Hide sidebar",
    "Show sidebar",
    "Back",
    "Forward",
    "Session rail",
    "Terminal",
    "Changes",
    "Browser",
    "Files",
    "New",
    "Search",
    "Projects Beta",
    "Artifacts",
    "Scheduled",
    "Plugins",
];
const NOT_SECTION_STARTS: [&str; 5] = [
    "More options for",
    "View options",
    "Create project",
    "New from a template",
    "More navigation items",
];

/// Whether a collapsed button is a folded section of the chat list (such as
/// "Pinned"), or a folded group of nested chats.
fn is_section(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && !NOT_SECTIONS.contains(&name)
        && !NOT_SECTION_STARTS
            .iter()
            .any(|start| name.starts_with(start))
}

/// The folded sections of the chat list below `root`, in reading order.
pub fn collapsed_sections<T: Tree>(tree: &T, root: &T::Node) -> Vec<T::Node> {
    let mut found = Vec::new();
    let mut stack: Vec<(T::Node, usize)> = tree
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
        if tree.kind(&node) == Kind::Button {
            let folded = tree.collapsed(&node) == Some(true);
            if folded && tree.name(&node).is_some_and(|name| is_section(&name)) {
                found.push(node);
            }
            continue;
        }
        if level < DEPTH {
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

/// The chat's button found in a home window, and the "Show sidebar" button
/// when the list had to be opened for it.
pub struct Located<N> {
    pub button: N,
    pub matches: usize,
    pub opened: Option<N>,
}

fn located<N>(found: Found<N>, opened: Option<N>) -> Located<N> {
    Located {
        button: found.node,
        matches: found.matches,
        opened,
    }
}

/// Finds the chat's button in the home window, step by step. First the
/// Code view is selected when it is not (the Claude view does not list
/// code chats), then the hidden list is opened, then each folded section
/// once. Nothing is closed again. `act` is false for a read-only look.
pub fn locate_chat<T: Tree>(
    tree: &T,
    root: &T::Node,
    title: &str,
    waits: &Waits,
    act: bool,
) -> Result<Option<Located<T::Node>>, String> {
    let find = || find_button(tree, root, title);
    let radio = find_named(tree, root, Kind::Radio, CODE_VIEW)
        .filter(|radio| tree.selected(&radio.node) == Some(false));
    if let Some(radio) = radio {
        if !act {
            log::info!("read-only test instance: switching to the Code view skipped");
            return Ok(find().map(|found| located(found, None)));
        }
        log::info!("ClaudeDeck: switching the Claude app to the Code view");
        tree.select(&radio.node)?;
        if let Some(found) = poll(waits.switch, waits.step, find) {
            return Ok(Some(located(found, None)));
        }
    } else if let Some(found) = find() {
        return Ok(Some(located(found, None)));
    }
    let mut opened = None;
    let hidden = find_named(tree, root, Kind::Button, SHOW_SIDEBAR)
        .filter(|show| tree.collapsed(&show.node) == Some(true));
    if let Some(show) = hidden {
        if !act {
            log::info!("read-only test instance: opening the chat list skipped");
            return Ok(None);
        }
        log::info!("ClaudeDeck: opening the Claude app's hidden chat list");
        tree.expand(&show.node)?;
        if let Some(found) = poll(waits.expand, waits.step, find) {
            return Ok(Some(located(found, Some(show.node))));
        }
        opened = Some(show.node);
    }
    let sections = collapsed_sections(tree, root);
    if !sections.is_empty() && act {
        log::info!(
            "ClaudeDeck: opening {} folded sections of the chat list",
            sections.len()
        );
        for section in &sections {
            if let Err(reason) = tree.expand(section) {
                log::info!("ClaudeDeck: a folded section stayed folded: {reason}");
            }
        }
        if let Some(found) = poll(waits.expand, waits.step, find) {
            return Ok(Some(located(found, opened)));
        }
    }
    Ok(None)
}

/// Finds the chat's button. The home window is brought forward and the
/// chat list is made to show the chat (see `locate_chat`). `start` starts
/// the app when it is not running. `dry`: nothing is moved, opened or
/// started.
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
            log::info!("ClaudeDeck: the Claude app's home window is open; looking for \"{title}\"");
            let found = locate_chat(&tree, &root, title, &WAITS, !dry)?;
            if found.is_none() {
                log::info!("ClaudeDeck: \"{title}\" is not in the home window's chat list");
            }
            if found.is_some() || deadline.is_none() {
                return Ok(found.map(|found| Chat {
                    root,
                    button: found.button,
                    matches: found.matches,
                    opened: found.opened,
                }));
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
fn run(title: &str, dry: bool) -> Result<Option<String>, String> {
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
        .ok_or_else(|| {
            log::warn!("ClaudeDeck: open \"{title}\": the chat is not in the Claude app's list");
            missing()
        })?;
    if chat.matches > 1 {
        log::info!(
            "ClaudeDeck: {} chats in the Claude app are named \"{title}\"; the first is used",
            chat.matches
        );
    }
    if dry {
        log::info!("read-only test instance: pressing \"{title}\" in the Claude app skipped");
        return Ok(None);
    }
    // The list stays as the app leaves it after the click.
    uia::invoke(&chat.button).map(|()| None).map_err(|reason| {
        log::warn!("ClaudeDeck: could not press \"{title}\": {reason}");
        missing()
    })
}

/// Runs `work` on a thread of its own that started COM; `say` gets the
/// notice text that `work` gives, as a result or as a failure.
pub(crate) fn on_com_thread(
    title: String,
    say: impl FnOnce(String) + Send + 'static,
    work: impl FnOnce(&str) -> Result<Option<String>, String> + Send + 'static,
) {
    std::thread::spawn(move || {
        let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
        if hr < 0 {
            log::warn!("ClaudeDeck: COM did not start ({})", com::hex(hr));
            say(not_found_notice(&title));
            return;
        }
        let outcome = work(&title);
        unsafe { CoUninitialize() };
        if let Some(text) = outcome.unwrap_or_else(Some) {
            say(text);
        }
    });
}

/// `--claude-app-find=<title>`: the same search an open makes, read-only
/// (nothing is switched, unfolded or pressed), for checking it against the
/// live Claude app. Returns one line to print.
pub fn find_report(title: &str) -> String {
    let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
    if hr < 0 {
        return format!("COM did not start ({})", com::hex(hr));
    }
    let began = Instant::now();
    let line = match Automation::new() {
        Err(reason) => format!("UI Automation did not start: {reason}"),
        Ok(automation) => match find_chat(&automation, title.trim(), true, true) {
            Ok(Some(_)) => "found".to_string(),
            Ok(None) => "not found".to_string(),
            Err(reason) => format!("not found: {reason}"),
        },
    };
    unsafe { CoUninitialize() };
    format!(
        "\"{}\": {line} in {} ms",
        title.trim(),
        began.elapsed().as_millis()
    )
}

/// Opens the chat with this title in the app's main window, on a thread of
/// its own. `failed` gets the notice text when the chat cannot be found.
/// A test copy of WinCraft only reads; unit tests start nothing.
pub fn open_chat(
    title: &str,
    failed: impl FnOnce(String) + Send + 'static,
    finished: impl FnOnce() + Send + 'static,
) {
    if cfg!(test) {
        return;
    }
    let dry = instance::is_test();
    log::info!(
        "ClaudeDeck: opening \"{}\" in the Claude app (read-only: {dry})",
        title.trim()
    );
    on_com_thread(title.trim().to_string(), failed, move |title| {
        let began = Instant::now();
        let outcome = run(title, dry);
        log::info!(
            "ClaudeDeck: opening \"{title}\" took {} ms",
            began.elapsed().as_millis()
        );
        finished();
        outcome
    });
}

mod keys;
mod menu;
pub use menu::rename_chat;

#[cfg(test)]
mod tests;
