//! What the thumbnail probe saw, as plain data, and the text report made
//! from it. Nothing here calls Windows, so all of it is tested on made-up
//! samples.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use super::geometry::{self, Rect, SlotGrid};

/// Thumbnail popups and the XAML hosts Explorer has used for them.
pub const KNOWN_POPUP_CLASSES: &[&str] = &[
    "TaskListThumbnailWnd",
    "Windows.UI.Composition.DesktopWindowContentBridge",
    "TopLevelWindowForOverflowXamlIsland",
    "XamlExplorerHostIslandWindow",
    "Xaml_WindowedPopupClass",
];

const MAX_TEXT: usize = 120;
/// A move is written when its slot changes, or after this long.
const MOVE_EVERY_MS: u64 = 100;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub class: String,
    pub title: String,
    pub process: String,
    pub rect: Rect,
    /// The effective DPI of the window's monitor.
    pub dpi: u32,
    pub style: u32,
    pub ex_style: u32,
    pub owner: isize,
    pub owner_class: String,
    /// DWMWA_CLOAKED.
    pub cloak: u32,
    /// DWMWA_EXTENDED_FRAME_BOUNDS, when DWM gives it.
    pub frame_bounds: Option<Rect>,
    /// Not shown (visible and not cloaked) when the probe started.
    pub new_since_start: bool,
    pub children: Vec<ChildWindow>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChildWindow {
    pub class: String,
    pub rect: Rect,
    pub visible: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiaNode {
    pub depth: usize,
    pub control_type: String,
    pub name: String,
    pub automation_id: String,
    pub class: String,
    pub rect: Option<Rect>,
    pub offscreen: Option<bool>,
    pub native_window: isize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Popup {
    pub window: WindowInfo,
    /// Asked only for a likely popup; see [`is_likely`].
    pub uia: Option<Result<Vec<UiaNode>, String>>,
}

/// The windows the probe looks at in one tick, and the slot guess for the
/// popup it picked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub popups: Vec<Popup>,
    pub grid: Option<SlotGrid>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CursorSample {
    pub point: (i32, i32),
    /// WindowFromPoint's class, its root window's class and the root's
    /// program.
    pub under: String,
    /// ElementFromPoint and its parents, asked only over a popup or a
    /// taskbar.
    pub chain: Option<Result<Vec<UiaNode>, String>>,
    pub slot: Option<usize>,
}

/// The taskbar group whose button was hovered last.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Group {
    pub app_id: String,
    /// The group's windows on the current desktop, top of the z-order first.
    pub windows: Vec<(isize, String)>,
    /// Titles in LayoutKeeper's saved taskbar order, from its state file.
    pub saved_order: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseKind {
    Move,
    LeftDown,
    LeftUp,
    RightDown,
    RightUp,
    MiddleDown,
    MiddleUp,
    Wheel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MouseEvent {
    pub t_ms: u64,
    pub kind: MouseKind,
    pub x: i32,
    pub y: i32,
    /// Inside a likely popup's rectangle at the time, as the hook knew it.
    pub inside: bool,
}

pub enum Sample {
    Frame(Frame),
    Cursor(CursorSample),
    Group(Group),
    Note(String),
}

/// A window worth a UI Automation walk: a class Explorer uses for the
/// popup, or one that appeared after the probe started.
pub fn is_likely(window: &WindowInfo) -> bool {
    KNOWN_POPUP_CLASSES.contains(&window.class.as_str()) || window.new_since_start
}

fn shown(window: &WindowInfo) -> bool {
    window.cloak == 0 && geometry::area(window.rect) > 0
}

/// The popup to guess slots in: the classic thumbnail window when it is
/// shown, else the largest shown window of Explorer's that appeared after
/// the start.
pub fn pick_popup(popups: &[Popup]) -> Option<&Popup> {
    let shown_popups = || popups.iter().filter(|popup| shown(&popup.window));
    shown_popups()
        .find(|popup| popup.window.class == "TaskListThumbnailWnd")
        .or_else(|| {
            shown_popups()
                .filter(|popup| {
                    popup.window.new_since_start && popup.window.process == "explorer.exe"
                })
                .max_by_key(|popup| geometry::area(popup.window.rect))
        })
}

/// The rectangles a mouse event counts as inside: every shown likely popup.
pub fn popup_rects(popups: &[Popup]) -> Vec<Rect> {
    popups
        .iter()
        .filter(|popup| is_likely(&popup.window) && shown(&popup.window))
        .map(|popup| popup.window.rect)
        .collect()
}

pub fn rect_text(rect: Rect) -> String {
    format!(
        "[{},{},{},{}] {}x{}",
        rect[0],
        rect[1],
        rect[2],
        rect[3],
        rect[2] - rect[0],
        rect[3] - rect[1]
    )
}

fn short(text: &str) -> String {
    let one_line: String = text
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    if one_line.chars().count() <= MAX_TEXT {
        return one_line;
    }
    let cut: String = one_line.chars().take(MAX_TEXT).collect();
    format!("{cut}...")
}

pub fn node_text(node: &UiaNode) -> String {
    let mut line = format!(
        "{}{} name=\"{}\" id=\"{}\" class=\"{}\"",
        "  ".repeat(node.depth),
        node.control_type,
        short(&node.name),
        short(&node.automation_id),
        short(&node.class)
    );
    match node.rect {
        Some(rect) => {
            let _ = write!(line, " rect={}", rect_text(rect));
        }
        None => line.push_str(" rect=?"),
    }
    if let Some(offscreen) = node.offscreen {
        let _ = write!(line, " offscreen={offscreen}");
    }
    if node.native_window != 0 {
        let _ = write!(line, " hwnd=0x{:X}", node.native_window);
    }
    line
}

fn window_text(window: &WindowInfo) -> String {
    let mut text = format!(
        "window 0x{:X} class=\"{}\" process={} title=\"{}\"\n  rect={} dpi={} style=0x{:08X} exstyle=0x{:08X} cloak={}",
        window.hwnd,
        window.class,
        window.process,
        short(&window.title),
        rect_text(window.rect),
        window.dpi,
        window.style,
        window.ex_style,
        window.cloak,
    );
    match window.frame_bounds {
        Some(bounds) => {
            let _ = write!(text, " dwm-frame={}", rect_text(bounds));
        }
        None => text.push_str(" dwm-frame=?"),
    }
    if window.owner != 0 {
        let _ = write!(text, " owner=0x{:X} ({})", window.owner, window.owner_class);
    }
    if window.new_since_start {
        text.push_str(" NEW");
    }
    for child in &window.children {
        let _ = write!(
            text,
            "\n  child class=\"{}\" rect={}{}",
            child.class,
            rect_text(child.rect),
            if child.visible { "" } else { " hidden" }
        );
    }
    text
}

fn uia_text(out: &mut String, uia: &Result<Vec<UiaNode>, String>) {
    match uia {
        Ok(nodes) if nodes.is_empty() => out.push_str("    (no elements)\n"),
        Ok(nodes) => {
            for node in nodes {
                let _ = writeln!(out, "    {}", node_text(node));
            }
        }
        Err(reason) => {
            let _ = writeln!(out, "    UI Automation: {reason}");
        }
    }
}

/// Everything about a frame except when it was seen, so two frames that
/// look the same give the same text.
pub fn frame_text(frame: &Frame) -> String {
    let mut out = String::new();
    if frame.popups.is_empty() {
        out.push_str("  no explorer windows besides the taskbars and the desktop\n");
    }
    for popup in &frame.popups {
        let mark = if is_likely(&popup.window) {
            "LIKELY "
        } else {
            ""
        };
        let _ = writeln!(out, "  {mark}{}", window_text(&popup.window));
        if let Some(uia) = &popup.uia {
            out.push_str("   UI Automation tree:\n");
            uia_text(&mut out, uia);
        }
    }
    match &frame.grid {
        Some(grid) => {
            let _ = writeln!(
                out,
                "  slot guess: {} slots, {} row(s) x {} column(s), padding {} px, inner {}",
                grid.count,
                grid.rows,
                grid.cols,
                grid.padding,
                rect_text(grid.inner)
            );
            for index in 0..grid.count {
                if let Some(rect) = grid.slot_rect(index) {
                    let _ = writeln!(out, "    slot {index}: {}", rect_text(rect));
                }
            }
        }
        None => out.push_str("  slot guess: none (no popup picked, or no group window count)\n"),
    }
    out
}

pub fn cursor_text(sample: &CursorSample) -> String {
    let mut out = format!(
        "cursor {},{} over {}",
        sample.point.0, sample.point.1, sample.under
    );
    match sample.slot {
        Some(slot) => {
            let _ = write!(out, " slot={slot}");
        }
        None => out.push_str(" slot=-"),
    }
    out.push('\n');
    match &sample.chain {
        Some(Ok(chain)) => {
            out.push_str("  element at the cursor, then its parents:\n");
            for node in chain {
                let _ = writeln!(
                    out,
                    "    {}",
                    node_text(&UiaNode {
                        depth: 0,
                        ..node.clone()
                    })
                );
            }
        }
        Some(Err(reason)) => {
            let _ = writeln!(out, "  element at the cursor: {reason}");
        }
        None => {}
    }
    out
}

fn group_text(group: &Group) -> String {
    let mut out = format!(
        "hovered group \"{}\": {} window(s) on this desktop, top of the z-order first\n",
        group.app_id,
        group.windows.len()
    );
    for (hwnd, title) in &group.windows {
        let _ = writeln!(out, "  0x{hwnd:X} \"{}\"", short(title));
    }
    if group.saved_order.is_empty() {
        out.push_str("  LayoutKeeper has no saved order for this group\n");
    } else {
        out.push_str("  LayoutKeeper's saved taskbar order (all desktops):\n");
        for (index, title) in group.saved_order.iter().enumerate() {
            let _ = writeln!(out, "    {index}: \"{}\"", short(title));
        }
    }
    out
}

fn mouse_name(kind: MouseKind) -> &'static str {
    match kind {
        MouseKind::Move => "move",
        MouseKind::LeftDown => "left down",
        MouseKind::LeftUp => "left up",
        MouseKind::RightDown => "right down",
        MouseKind::RightUp => "right up",
        MouseKind::MiddleDown => "middle down",
        MouseKind::MiddleUp => "middle up",
        MouseKind::Wheel => "wheel",
    }
}

/// Collects samples in time order, writing a frame or a cursor line only
/// when it differs from the one before.
#[derive(Default)]
pub struct Recorder {
    body: String,
    last_frame: Option<String>,
    last_cursor: Option<String>,
    frames: Vec<(u64, Frame)>,
    groups: Vec<Group>,
    cursors_inside: usize,
    repeated: usize,
}

impl Recorder {
    pub fn add(&mut self, t_ms: u64, sample: Sample) {
        match sample {
            Sample::Frame(frame) => {
                let text = frame_text(&frame);
                if self.last_frame.as_deref() == Some(text.as_str()) {
                    self.repeated += 1;
                    return;
                }
                let _ = write!(self.body, "[{t_ms} ms] windows:\n{text}");
                self.last_frame = Some(text);
                self.frames.push((t_ms, frame));
            }
            Sample::Cursor(sample) => {
                let text = cursor_text(&sample);
                if self.last_cursor.as_deref() == Some(text.as_str()) {
                    self.repeated += 1;
                    return;
                }
                if sample.slot.is_some() {
                    self.cursors_inside += 1;
                }
                let _ = write!(self.body, "[{t_ms} ms] {text}");
                self.last_cursor = Some(text);
            }
            Sample::Group(group) => {
                let _ = write!(self.body, "[{t_ms} ms] {}", group_text(&group));
                self.groups.push(group);
            }
            Sample::Note(note) => {
                let _ = writeln!(self.body, "[{t_ms} ms] {note}");
            }
        }
    }

    /// The slot grid in effect at a moment: the one of the last frame seen
    /// at or before it.
    fn grid_at(&self, t_ms: u64) -> Option<SlotGrid> {
        self.frames
            .iter()
            .rev()
            .find(|(t, _)| *t <= t_ms)
            .and_then(|(_, frame)| frame.grid)
    }

    fn events_text(&self, events: &[MouseEvent]) -> String {
        let mut out = String::new();
        let mut last_move: Option<(u64, Option<usize>)> = None;
        for event in events {
            let slot = self
                .grid_at(event.t_ms)
                .and_then(|grid| grid.slot_at(event.x, event.y));
            if event.kind == MouseKind::Move {
                let due = match last_move {
                    None => true,
                    Some((t, last_slot)) => last_slot != slot || event.t_ms >= t + MOVE_EVERY_MS,
                };
                if !due {
                    continue;
                }
                last_move = Some((event.t_ms, slot));
            }
            let _ = writeln!(
                out,
                "[{} ms] {} at {},{} {} slot={}",
                event.t_ms,
                mouse_name(event.kind),
                event.x,
                event.y,
                if event.inside {
                    "inside popup"
                } else {
                    "outside popup"
                },
                slot.map(|s| s.to_string())
                    .unwrap_or_else(|| "-".to_string())
            );
        }
        if out.is_empty() {
            out.push_str("(none)\n");
        }
        out
    }

    /// The whole report: the header, the summary, the mouse events, then
    /// every distinct frame.
    pub fn finish(&self, header: &str, events: &[MouseEvent]) -> String {
        let mut out = String::new();
        out.push_str(header);
        out.push_str("\n== Summary ==\n");
        out.push_str(&self.summary(events));
        out.push_str(
            "\n== Mouse buttons, and moves inside a popup (seen by the hook, never changed) ==\n",
        );
        out.push_str(&self.events_text(events));
        let _ = write!(
            out,
            "\n== Timeline ({} repeated samples left out) ==\n",
            self.repeated
        );
        out.push_str(&self.body);
        out
    }

    pub fn summary(&self, events: &[MouseEvent]) -> String {
        let mut out = String::new();
        let mut classes: BTreeMap<String, usize> = BTreeMap::new();
        for (_, frame) in &self.frames {
            for popup in &frame.popups {
                if is_likely(&popup.window) && shown(&popup.window) {
                    *classes.entry(popup.window.class.clone()).or_default() += 1;
                }
            }
        }
        if classes.is_empty() {
            out.push_str("Popup: none found. No likely popup window was shown during the run.\n");
        } else {
            out.push_str("Popup: found. Shown likely popup windows (class: distinct frames):\n");
            for (class, count) in &classes {
                let _ = writeln!(out, "  {class}: {count}");
            }
        }

        let titles: BTreeSet<String> = self
            .groups
            .iter()
            .flat_map(|group| {
                group
                    .windows
                    .iter()
                    .map(|(_, title)| title.trim().to_string())
            })
            .filter(|title| !title.is_empty())
            .collect();
        let handles: BTreeSet<isize> = self
            .groups
            .iter()
            .flat_map(|group| group.windows.iter().map(|(hwnd, _)| *hwnd))
            .collect();
        match self.groups.last() {
            Some(group) => {
                let _ = writeln!(
                    out,
                    "Group: last hovered \"{}\" with {} window(s) on this desktop.",
                    group.app_id,
                    group.windows.len()
                );
            }
            None => out.push_str("Group: no app button on the taskbar was hovered.\n"),
        }

        let mut most_nodes = 0;
        let mut items = 0;
        let mut named_titles: BTreeSet<String> = BTreeSet::new();
        let mut handle_hits: BTreeSet<isize> = BTreeSet::new();
        let mut best_overlaps: Vec<f64> = Vec::new();
        for (_, frame) in &self.frames {
            let picked = pick_popup(&frame.popups);
            for popup in &frame.popups {
                let Some(Ok(nodes)) = &popup.uia else {
                    continue;
                };
                most_nodes = most_nodes.max(nodes.len());
                let inner: Vec<Rect> = item_rects(nodes, popup.window.rect);
                items = items.max(inner.len());
                for node in nodes {
                    let name = node.name.trim();
                    if !name.is_empty() && titles.iter().any(|title| name.contains(title.as_str()))
                    {
                        named_titles.insert(name.to_string());
                    }
                    if node.native_window != 0 && handles.contains(&node.native_window) {
                        handle_hits.insert(node.native_window);
                    }
                }
                if let (Some(grid), Some(picked)) = (frame.grid, picked) {
                    if picked.window.hwnd == popup.window.hwnd && !inner.is_empty() {
                        for index in 0..grid.count {
                            if let Some(slot) = grid.slot_rect(index) {
                                let best = inner
                                    .iter()
                                    .map(|rect| geometry::overlap(slot, *rect))
                                    .fold(0.0, f64::max);
                                best_overlaps.push(best);
                            }
                        }
                    }
                }
            }
        }
        let _ = writeln!(
            out,
            "UI Automation: at most {most_nodes} element(s) under one popup; at most {items} of them with a rectangle inside the popup and at least 40x30 px."
        );
        let _ = writeln!(
            out,
            "  Element names that contain a window title of the group: {} distinct.",
            named_titles.len()
        );
        let _ = writeln!(
            out,
            "  Elements whose native window is one of the group's windows: {}.",
            handle_hits.len()
        );
        let verdict = if classes.is_empty() {
            "nothing to identify: no popup was seen"
        } else if items >= 2 && (!named_titles.is_empty() || !handle_hits.is_empty()) {
            "UI Automation gives thumbnail items with rectangles that can be matched to windows"
        } else if items >= 2 {
            "UI Automation gives items with rectangles, but nothing ties them to a window"
        } else {
            "only the popup's own rectangle is known; thumbnails must be guessed"
        };
        let _ = writeln!(out, "Identification: {verdict}.");
        if best_overlaps.is_empty() {
            out.push_str("Slot guess against UI Automation: nothing to compare.\n");
        } else {
            let mean = best_overlaps.iter().sum::<f64>() / best_overlaps.len() as f64;
            let good = best_overlaps.iter().filter(|value| **value >= 0.5).count();
            let _ = writeln!(
                out,
                "Slot guess against UI Automation: mean best overlap {mean:.2} over {} slot(s); {good} slot(s) overlap an element by half or more.",
                best_overlaps.len()
            );
        }

        let count = |kind: MouseKind, inside: bool| {
            events
                .iter()
                .filter(|event| event.kind == kind && event.inside == inside)
                .count()
        };
        let _ = writeln!(
            out,
            "Mouse: left down inside a popup {}, left up inside {}; outside: down {}, up {}. Moves inside: {}. Cursor samples with a slot: {}.",
            count(MouseKind::LeftDown, true),
            count(MouseKind::LeftUp, true),
            count(MouseKind::LeftDown, false),
            count(MouseKind::LeftUp, false),
            count(MouseKind::Move, true),
            self.cursors_inside
        );
        out.push_str(&self.drag_text(events));
        out
    }

    /// Each left press inside a popup and the release after it, with the
    /// slot guessed for both: what a drag of a thumbnail would look like.
    fn drag_text(&self, events: &[MouseEvent]) -> String {
        let mut out = String::new();
        let mut down: Option<&MouseEvent> = None;
        for event in events {
            match event.kind {
                MouseKind::LeftDown if event.inside => down = Some(event),
                MouseKind::LeftUp => {
                    if let Some(start) = down.take() {
                        let slot = |e: &MouseEvent| {
                            self.grid_at(e.t_ms)
                                .and_then(|grid| grid.slot_at(e.x, e.y))
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| "-".to_string())
                        };
                        let _ = writeln!(
                            out,
                            "  press at {},{} (slot {}) and release at {},{} (slot {}{}) after {} ms",
                            start.x,
                            start.y,
                            slot(start),
                            event.x,
                            event.y,
                            slot(event),
                            if event.inside { "" } else { ", outside the popup" },
                            event.t_ms.saturating_sub(start.t_ms)
                        );
                    }
                }
                _ => {}
            }
        }
        if out.is_empty() {
            return "Press and release inside a popup: not seen.\n".to_string();
        }
        format!("Press and release inside a popup:\n{out}")
    }
}

/// The rectangles of elements below the popup's own element that lie
/// inside the popup and are big enough to be a thumbnail.
fn item_rects(nodes: &[UiaNode], popup: Rect) -> Vec<Rect> {
    nodes
        .iter()
        .filter(|node| node.depth > 0)
        .filter_map(|node| node.rect)
        .filter(|rect| {
            *rect != popup
                && geometry::within(*rect, popup)
                && rect[2] - rect[0] >= 40
                && rect[3] - rect[1] >= 30
        })
        .collect::<BTreeSet<Rect>>()
        .into_iter()
        .collect()
}

/// "20261024-130511" from "2026-10-24T13:05:11+03:00", for a file name.
pub fn file_stamp(local_iso: &str) -> String {
    local_iso
        .chars()
        .take(19)
        .filter(char::is_ascii_digit)
        .enumerate()
        .flat_map(|(index, ch)| (index == 8).then_some('-').into_iter().chain([ch]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(class: &str, rect: Rect, new: bool) -> WindowInfo {
        WindowInfo {
            hwnd: 0x100,
            class: class.to_string(),
            process: "explorer.exe".to_string(),
            rect,
            dpi: 96,
            new_since_start: new,
            ..WindowInfo::default()
        }
    }

    fn node(depth: usize, name: &str, rect: Rect) -> UiaNode {
        UiaNode {
            depth,
            control_type: "ListItem".to_string(),
            name: name.to_string(),
            rect: Some(rect),
            ..UiaNode::default()
        }
    }

    const POPUP: Rect = [0, 0, 432, 156];

    fn popup_frame(nodes: Option<Vec<UiaNode>>) -> Frame {
        Frame {
            popups: vec![Popup {
                window: window("TaskListThumbnailWnd", POPUP, true),
                uia: nodes.map(Ok),
            }],
            grid: geometry::slot_grid(POPUP, 2, 96),
        }
    }

    fn group() -> Group {
        Group {
            app_id: "Chrome".to_string(),
            windows: vec![
                (0x10, "Mail - Chrome".to_string()),
                (0x20, "News - Chrome".to_string()),
            ],
            saved_order: vec!["News - Chrome".to_string(), "Mail - Chrome".to_string()],
        }
    }

    fn event(t_ms: u64, kind: MouseKind, x: i32, y: i32, inside: bool) -> MouseEvent {
        MouseEvent {
            t_ms,
            kind,
            x,
            y,
            inside,
        }
    }

    #[test]
    fn known_classes_and_new_windows_are_likely() {
        assert!(is_likely(&window("TaskListThumbnailWnd", POPUP, false)));
        assert!(is_likely(&window("SomethingNew", POPUP, true)));
        assert!(!is_likely(&window("SomethingOld", POPUP, false)));
    }

    #[test]
    fn the_classic_popup_is_picked_first_then_the_largest_new_one() {
        let small = Popup {
            window: window("XamlExplorerHostIslandWindow", [0, 0, 10, 10], true),
            uia: None,
        };
        let large = Popup {
            window: WindowInfo {
                hwnd: 0x200,
                ..window("XamlExplorerHostIslandWindow", [0, 0, 500, 300], true)
            },
            uia: None,
        };
        let classic = Popup {
            window: WindowInfo {
                hwnd: 0x300,
                ..window("TaskListThumbnailWnd", [0, 0, 5, 5], false)
            },
            uia: None,
        };
        let popups = vec![small.clone(), large.clone()];
        assert_eq!(pick_popup(&popups).unwrap().window.hwnd, 0x200);
        let popups = vec![small.clone(), large.clone(), classic];
        assert_eq!(pick_popup(&popups).unwrap().window.hwnd, 0x300);
        let cloaked = Popup {
            window: WindowInfo {
                cloak: 2,
                ..large.window.clone()
            },
            uia: None,
        };
        let other = Popup {
            window: WindowInfo {
                process: "chrome.exe".to_string(),
                ..window("Chrome_WidgetWin_1", [0, 0, 900, 900], true)
            },
            uia: None,
        };
        assert_eq!(pick_popup(&[cloaked.clone(), other.clone()]), None);
        assert_eq!(
            popup_rects(&[cloaked, other, small]),
            vec![[0, 0, 900, 900], [0, 0, 10, 10]]
        );
    }

    #[test]
    fn nodes_read_as_one_line_each() {
        let mut item = node(2, "Mail\n- Chrome", [1, 2, 41, 32]);
        item.offscreen = Some(false);
        item.native_window = 0xAB;
        assert_eq!(
            node_text(&item),
            "    ListItem name=\"Mail - Chrome\" id=\"\" class=\"\" rect=[1,2,41,32] 40x30 offscreen=false hwnd=0xAB"
        );
        let bare = UiaNode {
            control_type: "Pane".to_string(),
            name: "x".repeat(130),
            ..UiaNode::default()
        };
        let text = node_text(&bare);
        assert!(text.contains(&format!("{}...", "x".repeat(120))), "{text}");
        assert!(text.ends_with("rect=?"));
    }

    #[test]
    fn a_frame_shows_windows_children_the_tree_and_the_slots() {
        let mut frame = popup_frame(Some(vec![
            node(0, "", POPUP),
            node(1, "Mail - Chrome", [8, 8, 216, 148]),
        ]));
        frame.popups[0].window.children.push(ChildWindow {
            class: "Windows.UI.Composition.DesktopWindowContentBridge".to_string(),
            rect: POPUP,
            visible: false,
        });
        frame.popups[0].window.owner = 0x55;
        frame.popups[0].window.owner_class = "Shell_TrayWnd".to_string();
        frame.popups[0].window.frame_bounds = Some(POPUP);
        let text = frame_text(&frame);
        assert!(
            text.contains("LIKELY window 0x100 class=\"TaskListThumbnailWnd\""),
            "{text}"
        );
        assert!(text.contains("owner=0x55 (Shell_TrayWnd) NEW"), "{text}");
        assert!(text.contains("dwm-frame=[0,0,432,156]"), "{text}");
        assert!(text.contains("child class=\"Windows.UI.Composition.DesktopWindowContentBridge\" rect=[0,0,432,156] 432x156 hidden"), "{text}");
        assert!(
            text.contains("      ListItem name=\"Mail - Chrome\""),
            "{text}"
        );
        assert!(
            text.contains("slot guess: 2 slots, 1 row(s) x 2 column(s), padding 8 px"),
            "{text}"
        );
        assert!(text.contains("slot 1: [216,8,424,148] 208x140"), "{text}");
        let empty = frame_text(&Frame::default());
        assert!(empty.contains("no explorer windows"));
        assert!(empty.contains("slot guess: none"));
        let failed = Frame {
            popups: vec![Popup {
                window: window("Other", POPUP, false),
                uia: Some(Err("ElementFromHandle failed (0x80004005)".to_string())),
            }],
            grid: None,
        };
        let text = frame_text(&failed);
        assert!(text.starts_with("  window 0x100"), "{text}");
        assert!(text.contains("UI Automation: ElementFromHandle failed"));
        let none = popup_frame(Some(Vec::new()));
        assert!(frame_text(&none).contains("(no elements)"));
    }

    #[test]
    fn unchanged_frames_and_cursor_lines_are_written_once() {
        let mut recorder = Recorder::default();
        recorder.add(0, Sample::Frame(popup_frame(None)));
        recorder.add(100, Sample::Frame(popup_frame(None)));
        let cursor = CursorSample {
            point: (20, 20),
            under: "TaskListThumbnailWnd / TaskListThumbnailWnd / explorer.exe".to_string(),
            chain: Some(Ok(vec![node(3, "Mail - Chrome", [8, 8, 216, 148])])),
            slot: Some(0),
        };
        recorder.add(100, Sample::Cursor(cursor.clone()));
        recorder.add(200, Sample::Cursor(cursor.clone()));
        recorder.add(
            300,
            Sample::Cursor(CursorSample {
                point: (21, 20),
                ..cursor
            }),
        );
        recorder.add(300, Sample::Frame(Frame::default()));
        recorder.add(400, Sample::Note("started".to_string()));
        assert_eq!(recorder.frames.len(), 2);
        let report = recorder.finish("HEADER\n", &[]);
        assert!(report.starts_with("HEADER\n"));
        assert!(report.contains("(2 repeated samples left out)"), "{report}");
        assert_eq!(
            report.matches("[100 ms] cursor 20,20").count(),
            1,
            "{report}"
        );
        assert!(report.contains("[300 ms] cursor 21,20"));
        assert!(
            report.contains(
                "parents:
    ListItem name=\"Mail - Chrome\""
            ),
            "the chain is not indented by depth"
        );
        assert!(report.contains("[400 ms] started"));
        assert!(report.contains("(none)"), "no mouse events");
    }

    #[test]
    fn cursor_lines_say_when_the_element_could_not_be_read() {
        let text = cursor_text(&CursorSample {
            point: (-5, 7),
            under: "x".to_string(),
            chain: Some(Err("ElementFromPoint failed".to_string())),
            slot: None,
        });
        assert_eq!(
            text,
            "cursor -5,7 over x slot=-\n  element at the cursor: ElementFromPoint failed\n"
        );
    }

    #[test]
    fn the_group_lists_its_windows_and_the_saved_order() {
        let mut recorder = Recorder::default();
        recorder.add(50, Sample::Group(group()));
        recorder.add(
            60,
            Sample::Group(Group {
                saved_order: Vec::new(),
                ..group()
            }),
        );
        let report = recorder.finish("", &[]);
        assert!(
            report.contains("hovered group \"Chrome\": 2 window(s)"),
            "{report}"
        );
        assert!(report.contains("  0x10 \"Mail - Chrome\""));
        assert!(report.contains("    0: \"News - Chrome\""));
        assert!(report.contains("LayoutKeeper has no saved order"));
        assert!(report.contains("Group: last hovered \"Chrome\" with 2 window(s)"));
    }

    #[test]
    fn a_run_with_nothing_seen_says_so() {
        let summary = Recorder::default().summary(&[]);
        assert!(summary.contains("Popup: none found"), "{summary}");
        assert!(summary.contains("no app button on the taskbar was hovered"));
        assert!(summary.contains("Identification: nothing to identify"));
        assert!(summary.contains("Slot guess against UI Automation: nothing to compare"));
        assert!(summary.contains("Press and release inside a popup: not seen"));
    }

    #[test]
    fn matching_items_are_found_and_compared_with_the_slots() {
        let mut recorder = Recorder::default();
        recorder.add(0, Sample::Group(group()));
        let mut native = node(2, "", [0, 0, 1, 1]);
        native.native_window = 0x20;
        recorder.add(
            10,
            Sample::Frame(popup_frame(Some(vec![
                node(0, "", POPUP),
                node(1, "Mail - Chrome", [8, 8, 216, 148]),
                node(1, "News - Chrome", [216, 8, 424, 148]),
                node(2, "close", [190, 10, 210, 30]),
                native,
            ]))),
        );
        let summary = recorder.summary(&[]);
        assert!(summary.contains("Popup: found"), "{summary}");
        assert!(summary.contains("  TaskListThumbnailWnd: 1"));
        assert!(
            summary.contains("at most 5 element(s) under one popup; at most 2 of them"),
            "{summary}"
        );
        assert!(summary.contains("contain a window title of the group: 2 distinct"));
        assert!(summary.contains("native window is one of the group's windows: 1."));
        assert!(summary.contains("can be matched to windows"), "{summary}");
        assert!(
            summary.contains("mean best overlap 1.00 over 2 slot(s); 2 slot(s)"),
            "{summary}"
        );
    }

    #[test]
    fn items_without_names_or_no_items_give_weaker_verdicts() {
        let mut recorder = Recorder::default();
        recorder.add(
            0,
            Sample::Frame(popup_frame(Some(vec![
                node(0, "", POPUP),
                node(1, "", [8, 8, 216, 148]),
                node(1, "", [216, 8, 424, 148]),
            ]))),
        );
        assert!(recorder
            .summary(&[])
            .contains("nothing ties them to a window"));
        let mut recorder = Recorder::default();
        recorder.add(
            0,
            Sample::Frame(popup_frame(Some(vec![node(0, "", POPUP)]))),
        );
        assert!(recorder
            .summary(&[])
            .contains("only the popup's own rectangle is known"));
    }

    #[test]
    fn mouse_events_get_their_slot_and_moves_are_thinned() {
        let mut recorder = Recorder::default();
        recorder.add(0, Sample::Frame(popup_frame(None)));
        let events = [
            event(5, MouseKind::Move, 20, 20, true),
            event(20, MouseKind::Move, 21, 20, true),
            event(30, MouseKind::Move, 300, 20, true),
            event(140, MouseKind::Move, 301, 20, true),
            event(150, MouseKind::LeftDown, 20, 20, true),
            event(400, MouseKind::LeftUp, 300, 20, true),
            event(500, MouseKind::LeftDown, 900, 20, false),
            event(510, MouseKind::LeftUp, 900, 20, false),
            event(600, MouseKind::LeftDown, 20, 20, true),
            event(700, MouseKind::LeftUp, 900, 900, false),
            event(800, MouseKind::Wheel, 1, 1, true),
        ];
        let report = recorder.finish("", &events);
        assert!(
            report.contains("[5 ms] move at 20,20 inside popup slot=0"),
            "{report}"
        );
        assert!(!report.contains("[20 ms] move"), "same slot within 100 ms");
        assert!(report.contains("[30 ms] move at 300,20 inside popup slot=1"));
        assert!(report.contains("[140 ms] move at 301,20"), "100 ms later");
        assert!(report.contains("[400 ms] left up at 300,20 inside popup slot=1"));
        assert!(report.contains("[510 ms] left up at 900,20 outside popup slot=-"));
        assert!(report.contains("[800 ms] wheel"));
        assert!(report.contains("left down inside a popup 2, left up inside 1; outside: down 1, up 2. Moves inside: 4."), "{report}");
        assert!(
            report.contains("press at 20,20 (slot 0) and release at 300,20 (slot 1) after 250 ms"),
            "{report}"
        );
        assert!(
            report.contains("release at 900,900 (slot -, outside the popup) after 100 ms"),
            "{report}"
        );
        assert_eq!(
            report.matches("press at").count(),
            2,
            "the outside press is no drag"
        );
    }

    #[test]
    fn events_before_any_frame_have_no_slot() {
        let recorder = Recorder::default();
        let report = recorder.finish(
            "",
            &[
                event(0, MouseKind::RightDown, 1, 2, false),
                event(1, MouseKind::RightUp, 1, 2, false),
                event(2, MouseKind::MiddleDown, 1, 2, false),
                event(3, MouseKind::MiddleUp, 1, 2, false),
            ],
        );
        assert!(report.contains("[0 ms] right down at 1,2 outside popup slot=-"));
        assert!(report.contains("right up"));
        assert!(report.contains("middle down"));
        assert!(report.contains("middle up"));
    }

    #[test]
    fn a_file_stamp_keeps_the_date_and_the_time() {
        assert_eq!(file_stamp("2026-10-24T13:05:11+03:00"), "20261024-130511");
        assert_eq!(file_stamp("2026-09-24T21:40:11Z"), "20260924-214011");
        assert_eq!(file_stamp(""), "");
    }
}
