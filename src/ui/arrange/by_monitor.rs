//! The By monitor view: one band per virtual desktop, the current one first,
//! and inside each band one column per monitor, laid out the way the
//! monitors stand on the desk.

use egui::text::TextWrapping;
use egui::{pos2, vec2, Rect, Response, Sense, Shape, StrokeKind, Ui, UiBuilder, Vec2};

use super::{
    card_look, card_menu, float_picture, line_width, name_card, others, others_link, paint_look,
    picture_width, thumbnail, wrap, Drag, Frame, Pending, Shared, SAME_TITLE_HINT, SLACK,
};
use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{
    ArrangeAction, ArrangeDesktop, ArrangeGroup, ArrangeMonitor, ArrangeSnapshot, ArrangeWindow,
};
use crate::ui::widgets::icons::{self, Icon};
use crate::ui::widgets::text;

/// Left and right of the toolbar, the bands and the footer.
const SIDE: f32 = 20.0;
const TOOLBAR: f32 = 16.0 + 32.0 + 12.0;
const FOOTER: f32 = 1.0 + 10.0 + 20.0 + 10.0;
const BAND_TOP: f32 = 12.0;
const BAND_BOTTOM: f32 = 16.0;
const BAND_HEADER: f32 = 18.0;
const HEADER_GAP: f32 = 10.0;
const COLUMN_GAP: f32 = 16.0;
/// Between two rows of monitors stacked one above the other.
const ROW_GAP: f32 = 16.0;
pub const MIN_TRACK: f32 = 240.0;
/// A column's 1 px rule and the 12 px after it.
const INSET: f32 = 13.0;
const LABEL: f32 = 18.0;
const LABEL_GAP: f32 = 8.0;
/// The dashed drop target of a column with no windows.
const EMPTY: f32 = 112.0;
const CARD_PAD: f32 = 4.0;
const THUMB_HEIGHT: f32 = 96.0;
const TITLE_GAP: f32 = 6.0;
const TITLE: f32 = 18.0;
const CARD_HEIGHT: f32 = CARD_PAD + THUMB_HEIGHT + TITLE_GAP + TITLE + CARD_PAD;
const CARD_GAP: f32 = 8.0;
/// Room for the toolbar's controls and its counts.
const MIN_PANEL: f32 = 680.0;
const NOT_CURRENT: f32 = 0.85;
const DISCONNECTED: f32 = 0.45;
const DRAGGED: f32 = 0.6;

/// Rows of monitor columns, each row left to right, as places in
/// `monitors`. Connected monitors are sorted by the left edge of their work
/// area, ties by its top; one that shares part of its width with a monitor
/// already in a row goes to the next row, so monitors stacked one above the
/// other get a row each. The disconnected ones follow on the first row.
pub fn monitor_rows(monitors: &[ArrangeMonitor], shown: &dyn Fn(usize) -> bool) -> Vec<Vec<usize>> {
    let mut connected: Vec<usize> = (0..monitors.len())
        .filter(|index| monitors[*index].connected && shown(*index))
        .collect();
    connected.sort_by_key(|index| (monitors[*index].work[0], monitors[*index].work[1]));
    let mut rows: Vec<Vec<usize>> = Vec::new();
    for index in connected {
        let [left, _, right, _] = monitors[index].work;
        let free = rows.iter().position(|row| {
            row.iter().all(|other| {
                let other = monitors[*other].work;
                right <= other[0] || other[2] <= left
            })
        });
        match free {
            Some(row) => rows[row].push(index),
            None => rows.push(vec![index]),
        }
    }
    let gone: Vec<usize> = (0..monitors.len())
        .filter(|index| !monitors[*index].connected && shown(*index))
        .collect();
    if !gone.is_empty() {
        if rows.is_empty() {
            rows.push(Vec::new());
        }
        rows[0].extend(gone);
    }
    rows
}

/// How wide each column of a row wants to be, relatively: its monitor's
/// width in pixels. A disconnected monitor's size is not known any more; it
/// gets the narrowest connected one's.
pub fn weights(monitors: &[ArrangeMonitor], row: &[usize]) -> Vec<f32> {
    let width = |monitor: &ArrangeMonitor| (monitor.rect[2] - monitor.rect[0]).max(1) as f32;
    let narrowest = monitors
        .iter()
        .filter(|monitor| monitor.connected)
        .map(width)
        .reduce(f32::min)
        .unwrap_or(1920.0);
    row.iter()
        .map(|index| {
            let monitor = &monitors[*index];
            if monitor.connected {
                width(monitor)
            } else {
                narrowest
            }
        })
        .collect()
}

/// Each column's width out of `available`, in proportion to its weight but
/// never under MIN_TRACK: a column that would be narrower gets MIN_TRACK and
/// the others share what is left. When even that does not fit, the columns
/// add up to more than `available` and the row scrolls sideways.
pub fn tracks(weights: &[f32], available: f32) -> Vec<f32> {
    let mut fixed = vec![false; weights.len()];
    loop {
        let fixed_width = fixed.iter().filter(|f| **f).count() as f32 * MIN_TRACK;
        let free_weight: f32 = weights
            .iter()
            .zip(&fixed)
            .filter(|(_, fixed)| !**fixed)
            .map(|(weight, _)| *weight)
            .sum();
        let left = (available - fixed_width).max(0.0);
        let widths: Vec<f32> = weights
            .iter()
            .zip(&fixed)
            .map(|(weight, fixed)| {
                if *fixed || free_weight <= 0.0 {
                    MIN_TRACK
                } else {
                    left * weight / free_weight
                }
            })
            .collect();
        let mut changed = false;
        for (index, width) in widths.iter().enumerate() {
            if !fixed[index] && *width < MIN_TRACK {
                fixed[index] = true;
                changed = true;
            }
        }
        if !changed {
            return widths;
        }
    }
}

/// The columns of a row that is `width` wide, side padding and gaps taken
/// out first.
fn row_tracks(weights: &[f32], width: f32) -> Vec<f32> {
    let gaps = COLUMN_GAP * weights.len().saturating_sub(1) as f32;
    tracks(weights, width - 2.0 * SIDE - gaps)
}

/// A card's width for a window whose picture shows `source`.
pub fn card_width(source: Option<Vec2>) -> f32 {
    picture_width(source, THUMB_HEIGHT) + 2.0 * CARD_PAD
}

/// The widths of a cell's cards, from every window's card width.
fn cell_widths(cell: &[usize], widths: &[f32]) -> Vec<f32> {
    cell.iter().map(|member| widths[*member]).collect()
}

/// The cell's cards in lines that fit in a column `track` wide.
fn cell_lines(cell: &[usize], widths: &[f32], track: f32) -> Vec<std::ops::Range<usize>> {
    wrap(&cell_widths(cell, widths), track - INSET)
}

fn cell_height(cell: &[usize], widths: &[f32], track: f32) -> f32 {
    if cell.is_empty() {
        return EMPTY;
    }
    let lines = cell_lines(cell, widths, track).len() as f32;
    lines * CARD_HEIGHT + (lines - 1.0) * CARD_GAP
}

/// One virtual desktop's windows, split by monitor.
#[derive(Clone, Debug, PartialEq)]
pub struct Band {
    /// The desktop's id; empty when desktops are not known.
    pub desktop: String,
    pub title: String,
    /// The desktop's place in Task View, from 1.
    pub number: Option<usize>,
    pub current: bool,
    /// The windows of each monitor column, shaped like the rows of
    /// `monitor_rows`: `cells[row][column]`, each in taskbar order, as
    /// places in the group's windows.
    pub cells: Vec<Vec<Vec<usize>>>,
}

impl Band {
    pub fn count(&self) -> usize {
        self.cells.iter().flatten().map(Vec::len).sum()
    }

    /// "Desktop 1 · 12 windows".
    pub fn meta(&self) -> String {
        let windows = match self.count() {
            1 => "1 window".to_string(),
            n => format!("{n} windows"),
        };
        match self.number {
            Some(number) => format!("Desktop {number} \u{b7} {windows}"),
            None => windows,
        }
    }
}

/// The monitor whose column a window's card goes in: the one it is on,
/// unless the one it was saved on is disconnected; then that one, so a
/// disconnected monitor's column keeps the windows that belong on it.
pub fn column_of(window: &ArrangeWindow, monitors: &[ArrangeMonitor]) -> Option<usize> {
    match window.saved_monitor {
        Some(saved) if monitors.get(saved).is_some_and(|m| !m.connected) => Some(saved),
        _ => window.on_monitor,
    }
}

/// Where `monitor`'s column is in `rows`: (row, column).
fn cell_of(rows: &[Vec<usize>], monitor: usize) -> Option<(usize, usize)> {
    rows.iter().enumerate().find_map(|(r, row)| {
        row.iter()
            .position(|index| *index == monitor)
            .map(|column| (r, column))
    })
}

/// The primary monitor's column, else the first one.
fn primary_cell(monitors: &[ArrangeMonitor], rows: &[Vec<usize>]) -> (usize, usize) {
    monitors
        .iter()
        .position(|monitor| monitor.primary && monitor.connected)
        .and_then(|primary| cell_of(rows, primary))
        .unwrap_or((0, 0))
}

/// The group's windows by desktop and monitor: the current desktop first,
/// titled "This desktop", then the others in Task View order, each with all
/// its monitor columns. A window with no known desktop is on this one, and a
/// window with no known monitor is on the primary one.
pub fn bands(
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    monitors: &[ArrangeMonitor],
    rows: &[Vec<usize>],
) -> Vec<Band> {
    let empty: Vec<Vec<Vec<usize>>> = rows.iter().map(|row| vec![Vec::new(); row.len()]).collect();
    let mut bands: Vec<Band> = Vec::new();
    let current = desktops.iter().position(|desktop| desktop.current);
    bands.push(Band {
        desktop: current
            .map(|index| desktops[index].id.clone())
            .unwrap_or_default(),
        title: "This desktop".to_string(),
        number: current.map(|index| index + 1),
        current: true,
        cells: empty.clone(),
    });
    for (index, desktop) in desktops.iter().enumerate() {
        if !desktop.current {
            bands.push(Band {
                desktop: desktop.id.clone(),
                title: desktop.name.clone(),
                number: Some(index + 1),
                current: false,
                cells: empty.clone(),
            });
        }
    }
    let fallback = primary_cell(monitors, rows);
    for (member, window) in group.windows.iter().enumerate() {
        let band = bands
            .iter()
            .position(|band| !band.current && band.desktop == window.desktop)
            .unwrap_or(0);
        let (r, c) = column_of(window, monitors)
            .and_then(|monitor| cell_of(rows, monitor))
            .unwrap_or(fallback);
        if let Some(cell) = bands[band].cells.get_mut(r).and_then(|row| row.get_mut(c)) {
            cell.push(member);
        }
    }
    bands
}

/// The height of a band whose rows have these column widths, with cards
/// `widths` wide. A desktop with none of the group's windows is its header
/// alone.
fn band_height(band: &Band, tracks: &[Vec<f32>], widths: &[f32]) -> f32 {
    let header = 1.0 + BAND_TOP + BAND_HEADER;
    if band.count() == 0 {
        return header + BAND_TOP;
    }
    let mut height = header + HEADER_GAP + BAND_BOTTOM;
    for (r, row) in band.cells.iter().enumerate() {
        if r > 0 {
            height += ROW_GAP;
        }
        let tallest = row
            .iter()
            .zip(&tracks[r])
            .map(|(cell, track)| cell_height(cell, widths, *track))
            .fold(0.0, f32::max);
        height += LABEL + LABEL_GAP + tallest;
    }
    height
}

/// The bands on screen: only this desktop's until the other desktops are
/// shown.
pub fn visible(mut bands: Vec<Band>, shown: bool) -> Vec<Band> {
    if !shown {
        bands.truncate(1);
    }
    bands
}

/// The width at which every column holds its windows, `widths` wide, on
/// one line, or at least MIN_PANEL.
pub fn natural_width(bands: &[Band], weights: &[Vec<f32>], widths: &[f32]) -> f32 {
    weights
        .iter()
        .enumerate()
        .map(|(r, row)| {
            let total: f32 = row.iter().sum();
            let available = row
                .iter()
                .enumerate()
                .map(|(c, weight)| {
                    let widest = bands
                        .iter()
                        .map(|band| line_width(&cell_widths(&band.cells[r][c], widths)))
                        .fold(card_width(None), f32::max);
                    let needed = (INSET + widest).max(MIN_TRACK);
                    needed * total / weight
                })
                .fold(0.0, f32::max);
            available + COLUMN_GAP * row.len().saturating_sub(1) as f32 + 2.0 * SIDE
        })
        .fold(MIN_PANEL, f32::max)
}

/// The window size for the view: wide enough for one line of cards per
/// column, as tall as the bands then are, and at most `limit`, beyond which
/// it scrolls. `link` makes room for the line that opens the other desktops.
/// `widths` is each window's card width.
pub fn panel_size(
    bands: &[Band],
    weights: &[Vec<f32>],
    widths: &[f32],
    link: bool,
    limit: Vec2,
) -> Vec2 {
    let width = (natural_width(bands, weights, widths) + SLACK).min(limit.x);
    let tracks: Vec<Vec<f32>> = weights.iter().map(|row| row_tracks(row, width)).collect();
    let bands_height: f32 = bands
        .iter()
        .map(|band| band_height(band, &tracks, widths))
        .sum();
    let link = if link { others::LINK_HEIGHT } else { 0.0 };
    let height = TOOLBAR + bands_height + link + FOOTER + SLACK;
    vec2(width, height.min(limit.y))
}

/// Everything the view is laid out from.
struct Model {
    rows: Vec<Vec<usize>>,
    weights: Vec<Vec<f32>>,
    /// The bands on screen.
    bands: Vec<Band>,
    /// Every band, for a key that moves a window to a hidden desktop.
    all: Vec<Band>,
    /// Each window's card width.
    widths: Vec<f32>,
}

impl Model {
    fn new(group: &ArrangeGroup, snapshot: &ArrangeSnapshot, widths: &[f32], shown: bool) -> Self {
        let monitors = &snapshot.monitors;
        let used = |index: usize| {
            monitors[index].connected
                || group
                    .windows
                    .iter()
                    .any(|window| column_of(window, monitors) == Some(index))
        };
        let rows = monitor_rows(monitors, &used);
        let weights = rows.iter().map(|row| weights(monitors, row)).collect();
        let all = bands(group, &snapshot.desktops, monitors, &rows);
        Self {
            rows,
            weights,
            bands: visible(all.clone(), shown),
            all,
            widths: widths.to_vec(),
        }
    }
}

pub fn panel_for(
    group: &ArrangeGroup,
    snapshot: &ArrangeSnapshot,
    widths: &[f32],
    shown: bool,
    link: bool,
    limit: Vec2,
) -> Vec2 {
    let model = Model::new(group, snapshot, widths, shown);
    panel_size(&model.bands, &model.weights, widths, link, limit)
}

pub fn toolbar_rect(panel: Rect) -> Rect {
    Rect::from_min_size(
        panel.min + vec2(SIDE, 16.0),
        vec2(panel.width() - 2.0 * SIDE, 32.0),
    )
}

/// Every card in the order ↑ and ↓ walk them: band by band, row by row,
/// column by column.
pub fn walk(bands: &[Band]) -> Vec<usize> {
    bands
        .iter()
        .flat_map(|band| band.cells.iter().flatten().flatten().copied())
        .collect()
}

/// A card's place: its band, its monitor cell (row, column) and its place
/// in that cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spot {
    pub band: usize,
    pub cell: (usize, usize),
    /// For a drop, the place among the cell's other windows.
    pub index: usize,
}

pub fn locate(bands: &[Band], member: usize) -> Option<Spot> {
    bands.iter().enumerate().find_map(|(b, band)| {
        band.cells.iter().enumerate().find_map(|(r, row)| {
            row.iter().enumerate().find_map(|(c, cell)| {
                cell.iter().position(|m| *m == member).map(|index| Spot {
                    band: b,
                    cell: (r, c),
                    index,
                })
            })
        })
    })
}

/// The group's windows in their new taskbar order once `moved` is put at
/// `spot`: right before the window at `spot.index` among the cell's other
/// windows, or right after the last of them. In a cell with no other window
/// it keeps its place. Every other window keeps its order.
pub fn order_after(count: usize, bands: &[Band], moved: usize, spot: Spot) -> Vec<usize> {
    let mut order: Vec<usize> = (0..count).filter(|member| *member != moved).collect();
    let others: Vec<usize> = bands[spot.band].cells[spot.cell.0][spot.cell.1]
        .iter()
        .copied()
        .filter(|member| *member != moved)
        .collect();
    let at = match others.get(spot.index) {
        Some(next) => order.iter().position(|m| m == next).unwrap_or(order.len()),
        None => match others.last() {
            Some(last) => order
                .iter()
                .position(|m| m == last)
                .map_or(order.len(), |p| p + 1),
            None => moved.min(order.len()),
        },
    };
    order.insert(at, moved);
    order
}

/// What putting `moved` at `spot` means: a move to the band's desktop when
/// it is another one, a move to the column's monitor when that is another
/// one, then the group's new order.
pub fn drop_actions(
    group: &ArrangeGroup,
    bands: &[Band],
    rows: &[Vec<usize>],
    monitors: &[ArrangeMonitor],
    moved: usize,
    spot: Spot,
) -> Vec<ArrangeAction> {
    let mut actions = Vec::new();
    let window = &group.windows[moved];
    let hwnd = window.hwnd;
    let from = locate(bands, moved);
    let to = &bands[spot.band];
    if from.map(|from| from.band) != Some(spot.band) && !to.desktop.is_empty() {
        actions.push(ArrangeAction::MoveToDesktop {
            hwnd,
            desktop: to.desktop.clone(),
        });
    }
    let target = rows[spot.cell.0][spot.cell.1];
    if monitors[target].connected && column_of(window, monitors) != Some(target) {
        actions.push(ArrangeAction::MoveToMonitor {
            hwnd,
            monitor: monitors[target].device.clone(),
        });
    }
    let order: Vec<isize> = order_after(group.windows.len(), bands, moved, spot)
        .into_iter()
        .map(|member| group.windows[member].hwnd)
        .collect();
    let now: Vec<isize> = group.windows.iter().map(|window| window.hwnd).collect();
    if order != now || !actions.is_empty() {
        actions.push(ArrangeAction::Reorder {
            group: group.key.clone(),
            order,
        });
    }
    actions
}

/// A key the view answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    ShiftUp,
    ShiftDown,
    ShiftLeft,
    ShiftRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Select(usize),
    Move(Spot),
}

/// What a key does to the window `cursor`: ↑ and ↓ pick the card before or
/// after it, across columns and bands; ← and → move it one place within its
/// column; Shift+← and Shift+→ move it to the end of the next column of a
/// connected monitor; Shift+↑ and Shift+↓ move it to the end of the same
/// monitor's column in the band above or below, or of the primary monitor's
/// when its own is disconnected.
pub fn step(
    bands: &[Band],
    rows: &[Vec<usize>],
    monitors: &[ArrangeMonitor],
    cursor: usize,
    key: Key,
) -> Option<Step> {
    let here = locate(bands, cursor)?;
    let connected = |(r, c): (usize, usize)| monitors[rows[r][c]].connected;
    let end = |band: usize, cell: (usize, usize)| {
        Step::Move(Spot {
            band,
            cell,
            index: bands[band].cells[cell.0][cell.1].len(),
        })
    };
    match key {
        Key::Up | Key::Down => {
            let walk = walk(bands);
            let at = walk.iter().position(|m| *m == cursor)?;
            let next = if key == Key::Up {
                at.checked_sub(1)?
            } else {
                at + 1
            };
            walk.get(next).map(|member| Step::Select(*member))
        }
        Key::Left => here
            .index
            .checked_sub(1)
            .map(|index| Step::Move(Spot { index, ..here })),
        Key::Right => {
            let len = bands[here.band].cells[here.cell.0][here.cell.1].len();
            (here.index + 1 < len).then_some(Step::Move(Spot {
                index: here.index + 1,
                ..here
            }))
        }
        Key::ShiftLeft | Key::ShiftRight => {
            let cells: Vec<(usize, usize)> = rows
                .iter()
                .enumerate()
                .flat_map(|(r, row)| (0..row.len()).map(move |c| (r, c)))
                .collect();
            let at = cells.iter().position(|cell| *cell == here.cell)?;
            let next = if key == Key::ShiftRight {
                cells[at + 1..]
                    .iter()
                    .copied()
                    .find(|cell| connected(*cell))
            } else {
                cells[..at]
                    .iter()
                    .rev()
                    .copied()
                    .find(|cell| connected(*cell))
            }?;
            Some(end(here.band, next))
        }
        Key::ShiftUp | Key::ShiftDown => {
            let band = if key == Key::ShiftUp {
                here.band.checked_sub(1)?
            } else {
                here.band + 1
            };
            if band >= bands.len() {
                return None;
            }
            let cell = if connected(here.cell) {
                here.cell
            } else {
                primary_cell(monitors, rows)
            };
            Some(end(band, cell))
        }
    }
}

/// The window is on another monitor than the last saved layout says: moved
/// by hand since the last save, or not put back yet by a running restore.
pub fn moved(window: &ArrangeWindow, monitors: &[ArrangeMonitor]) -> bool {
    match (window.saved_monitor, window.on_monitor) {
        (Some(saved), Some(now)) => {
            saved != now && monitors.get(saved).is_some_and(|monitor| monitor.connected)
        }
        _ => false,
    }
}

/// A place a dragged card can land, from last frame's layout.
#[derive(Clone, Debug)]
pub struct Zone {
    band: usize,
    /// The monitor cell, or None for the header of a band with no windows.
    cell: Option<(usize, usize)>,
    rect: Rect,
    /// A disconnected monitor takes no windows; its own can still be
    /// reordered.
    open: bool,
    /// The cards in it: their windows and rectangles.
    cards: Vec<(usize, Rect)>,
}

/// Where the pointer would drop `dragged`: the zone under it, or the
/// nearest, and how many of that zone's other cards come before it, on the
/// lines above or to its left on its own line. `home` is the cell a drop on
/// a bare band header puts the window in.
fn drop_spot(
    pointer: egui::Pos2,
    zones: &[Zone],
    dragged: usize,
    home: (usize, usize),
) -> Option<Spot> {
    let usable = |zone: &&Zone| zone.open || zone.cards.iter().any(|(m, _)| *m == dragged);
    let zone = zones
        .iter()
        .filter(usable)
        .find(|zone| zone.rect.contains(pointer))
        .or_else(|| {
            zones.iter().filter(usable).min_by(|a, b| {
                a.rect
                    .distance_to_pos(pointer)
                    .total_cmp(&b.rect.distance_to_pos(pointer))
            })
        })?;
    let index = zone
        .cards
        .iter()
        .filter(|(member, _)| *member != dragged)
        .filter(|(_, card)| {
            card.max.y <= pointer.y
                || (card.y_range().contains(pointer.y) && card.center().x < pointer.x)
        })
        .count();
    Some(Spot {
        band: zone.band,
        cell: zone.cell.unwrap_or(home),
        index: if zone.cell.is_some() { index } else { 0 },
    })
}

/// One row of monitor columns in a band, laid out in `rect`.
struct RowAt<'a> {
    band_index: usize,
    row: usize,
    band: &'a Band,
    rect: Rect,
    tracks: &'a [f32],
    alpha: f32,
}

/// What drawing a frame of the view needs besides the shared state.
struct Draw<'a> {
    group: &'a ArrangeGroup,
    monitors: &'a [ArrangeMonitor],
    desktops: &'a [ArrangeDesktop],
    model: &'a Model,
    menu_open: bool,
    can_rename: bool,
    dragged: Option<usize>,
    target: Option<Spot>,
    zones: Vec<Zone>,
}

pub fn body(
    ui: &mut Ui,
    shared: &mut Shared,
    group: &ArrangeGroup,
    snapshot: &ArrangeSnapshot,
    panel: Rect,
    menu_open: bool,
) -> Frame {
    let ctx = ui.ctx().clone();
    let others_count = others::count(group, &snapshot.desktops);
    let shown = shared.others.shown(others_count);
    let widths = shared.widths(group, card_width);
    let model = Model::new(group, snapshot, &widths, shown);
    let mut frame = Frame::default();
    let walk = walk(&model.bands);
    let member_of = |hwnd: isize| group.windows.iter().position(|w| w.hwnd == hwnd);
    if shared
        .cursor
        .and_then(member_of)
        .is_none_or(|member| !walk.contains(&member))
    {
        shared.cursor = walk.first().map(|m| group.windows[*m].hwnd);
    }

    let dragged = shared.drag.as_ref().and_then(|drag| member_of(drag.hwnd));
    if dragged.is_none() {
        shared.drag = None;
    }
    let home = |member: usize| {
        locate(&model.bands, member)
            .map(|spot| spot.cell)
            .filter(|(r, c)| snapshot.monitors[model.rows[*r][*c]].connected)
            .unwrap_or_else(|| primary_cell(&snapshot.monitors, &model.rows))
    };
    let target = match (dragged, ctx.pointer_latest_pos()) {
        (Some(member), Some(pointer)) => drop_spot(pointer, &shared.zones, member, home(member)),
        _ => None,
    };
    if let Some(member) = dragged {
        if !ctx.input(|i| i.pointer.primary_down()) {
            if let Some(spot) = target {
                frame.actions.extend(drop_actions(
                    group,
                    &model.bands,
                    &model.rows,
                    &snapshot.monitors,
                    member,
                    spot,
                ));
            }
            shared.drag = None;
        }
    }
    let dragging = shared.drag.is_some();
    if dragging {
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    }

    let area = Rect::from_min_max(
        pos2(panel.min.x, panel.min.y + TOOLBAR),
        pos2(panel.max.x, panel.max.y - FOOTER),
    );
    frame.area = area;
    let mut draw = Draw {
        group,
        monitors: &snapshot.monitors,
        desktops: &snapshot.desktops,
        model: &model,
        menu_open,
        can_rename: snapshot.can_rename,
        dragged: dragged.filter(|_| dragging),
        target: target.filter(|_| dragging),
        zones: Vec::new(),
    };
    ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("arrange-bands")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let width = ui.available_width();
                for (b, band) in model.bands.iter().enumerate() {
                    draw.band(ui, shared, &mut frame, b, band, width);
                    if b == 0 && others_count > 0 {
                        let (line, _) = ui.allocate_exact_size(
                            vec2(width - SIDE, others::LINK_HEIGHT),
                            Sense::hover(),
                        );
                        others_link(ui, shared, line.translate(vec2(SIDE, 0.0)), others_count);
                    }
                }
            });
    });
    shared.zones = std::mem::take(&mut draw.zones);
    frame.dragged = dragged.map(|m| group.windows[m].hwnd);
    if let (Some(drag), Some(member), Some(pointer)) =
        (shared.drag.as_ref(), dragged, ctx.pointer_latest_pos())
    {
        ghost(
            &ctx,
            &mut frame,
            &group.windows[member],
            Rect::from_min_size(pointer - drag.grab, vec2(model.widths[member], CARD_HEIGHT)),
            menu_open,
        );
    }

    footer(ui, panel, shown, others_count > 0);

    if !menu_open && !dragging {
        keyboard(&ctx, shared, group, &model, &snapshot.monitors, &mut frame);
    }
    frame
}

/// The dragged card riding above everything at the pointer, grabbed where
/// it was picked up, as big as its card. Its old place stays behind, faded,
/// as the "from" mark.
fn ghost(
    ctx: &egui::Context,
    frame: &mut Frame,
    window: &ArrangeWindow,
    rect: Rect,
    menu_open: bool,
) {
    let tokens = Tokens::get(ctx);
    let layer = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("arrange-drag"),
    ));
    layer.rect_filled(rect, 4, tokens.hover_bg);
    let thumb = Rect::from_min_size(
        rect.min + Vec2::splat(CARD_PAD),
        vec2(rect.width() - 2.0 * CARD_PAD, THUMB_HEIGHT),
    );
    float_picture(&layer, frame, window.hwnd, thumb, menu_open);
    let mut job = text::job(
        &window.label,
        theme::regular(13.0),
        tokens.text_primary,
        None,
    );
    job.wrap = TextWrapping {
        max_width: rect.width() - 2.0 * CARD_PAD - 4.0,
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    let galley = layer.layout_job(job);
    layer.galley(
        pos2(
            rect.left() + CARD_PAD + 2.0,
            thumb.bottom() + TITLE_GAP + (TITLE - galley.size().y) / 2.0,
        ),
        galley,
        tokens.text_primary,
    );
    frame.floating = Some(rect);
}

impl Draw<'_> {
    fn band(
        &mut self,
        ui: &mut Ui,
        shared: &mut Shared,
        frame: &mut Frame,
        b: usize,
        band: &Band,
        width: f32,
    ) {
        let tokens = Tokens::get(ui.ctx());
        let alpha = if band.current { 1.0 } else { NOT_CURRENT };
        let tracks: Vec<Vec<f32>> = self
            .model
            .weights
            .iter()
            .map(|row| row_tracks(row, width))
            .collect();
        let (header, _) =
            ui.allocate_exact_size(vec2(width, 1.0 + BAND_TOP + BAND_HEADER), Sense::hover());
        let mut painter = ui.painter().clone();
        painter.multiply_opacity(alpha);
        painter.hline(
            header.x_range(),
            header.top(),
            theme::stroke(ui.ctx(), 1.0, tokens.border),
        );
        let title_colour = if band.current {
            tokens.accent
        } else {
            tokens.text_primary
        };
        let title = painter.layout_no_wrap(band.title.clone(), theme::semibold(13.0), title_colour);
        let meta = painter.layout_no_wrap(band.meta(), theme::regular(12.0), tokens.text_disabled);
        let centre = header.top() + 1.0 + BAND_TOP + BAND_HEADER / 2.0;
        let title_width = title.size().x;
        painter.galley(
            pos2(header.left() + SIDE, centre - title.size().y / 2.0),
            title,
            title_colour,
        );
        painter.galley(
            pos2(
                header.left() + SIDE + title_width + 10.0,
                centre - meta.size().y / 2.0,
            ),
            meta,
            tokens.text_disabled,
        );

        if band.count() == 0 {
            let (rest, _) = ui.allocate_exact_size(vec2(width, BAND_TOP), Sense::hover());
            let whole = header.union(rest);
            if self.target.is_some_and(|spot| spot.band == b) {
                painter.hline(
                    whole.x_range(),
                    whole.top() + 1.0,
                    theme::stroke(ui.ctx(), 2.0, tokens.accent),
                );
            }
            self.zones.push(Zone {
                band: b,
                cell: None,
                rect: whole,
                open: true,
                cards: Vec::new(),
            });
            return;
        }

        ui.add_space(HEADER_GAP);
        for (r, row) in band.cells.iter().enumerate() {
            if r > 0 {
                ui.add_space(ROW_GAP);
            }
            let tallest = row
                .iter()
                .zip(&tracks[r])
                .map(|(cell, track)| cell_height(cell, &self.model.widths, *track))
                .fold(0.0, f32::max);
            let height = LABEL + LABEL_GAP + tallest;
            let needed = 2.0 * SIDE
                + tracks[r].iter().sum::<f32>()
                + COLUMN_GAP * tracks[r].len().saturating_sub(1) as f32;
            let at = |rect: Rect| RowAt {
                band_index: b,
                row: r,
                band,
                rect,
                tracks: &tracks[r],
                alpha,
            };
            if needed <= width + 0.5 {
                let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
                self.row(ui, shared, frame, at(rect));
            } else {
                // The one sideways scroll the view has: columns at their
                // narrowest that still do not fit.
                egui::ScrollArea::horizontal()
                    .id_salt(("arrange-band-row", b, r))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(vec2(needed, height), Sense::hover());
                        self.row(ui, shared, frame, at(rect));
                    });
            }
        }
        ui.add_space(BAND_BOTTOM);
    }

    fn row(&mut self, ui: &mut Ui, shared: &mut Shared, frame: &mut Frame, at: RowAt) {
        let RowAt {
            band_index: b,
            row: r,
            band,
            rect,
            tracks,
            alpha,
        } = at;
        let tokens = Tokens::get(ui.ctx());
        let mut x = rect.left() + SIDE;
        for (c, track) in tracks.iter().enumerate() {
            let column = Rect::from_min_size(pos2(x, rect.top()), vec2(*track, rect.height()));
            x += track + COLUMN_GAP;
            let monitor = &self.monitors[self.model.rows[r][c]];
            let cell = &band.cells[r][c];
            let mut painter = ui.painter().clone();
            painter.multiply_opacity(alpha);
            painter.vline(
                column.left(),
                column.y_range(),
                theme::stroke(ui.ctx(), 1.0, tokens.border),
            );
            let inner = Rect::from_min_max(pos2(column.left() + INSET, column.top()), column.max);
            label_row(
                ui,
                &painter,
                Rect::from_min_size(inner.min, vec2(inner.width(), LABEL)),
                monitor,
                cell.len(),
            );

            let top = inner.top() + LABEL + LABEL_GAP;
            let widths = &self.model.widths;
            let mut cards: Vec<(usize, Rect)> = Vec::new();
            for (line, members) in cell_lines(cell, widths, *track).into_iter().enumerate() {
                let mut x = inner.left();
                for member in &cell[members] {
                    let at = pos2(x, top + line as f32 * (CARD_HEIGHT + CARD_GAP));
                    x += widths[*member] + CARD_GAP;
                    cards.push((
                        *member,
                        Rect::from_min_size(at, vec2(widths[*member], CARD_HEIGHT)),
                    ));
                }
            }
            for (member, card_rect) in cards.clone() {
                let mut card_alpha = alpha;
                if !monitor.connected {
                    card_alpha *= DISCONNECTED;
                }
                if self.dragged == Some(member) {
                    card_alpha *= DRAGGED;
                }
                self.card(ui, shared, frame, member, card_rect, card_alpha);
            }
            let target_here = self
                .target
                .filter(|spot| spot.band == b && spot.cell == (r, c));
            let others: Vec<Rect> = cards
                .iter()
                .filter(|(member, _)| Some(*member) != self.dragged)
                .map(|(_, rect)| *rect)
                .collect();
            if cell.is_empty() && monitor.connected {
                let empty =
                    Rect::from_min_size(pos2(inner.left(), top), vec2(inner.width(), EMPTY));
                drop_box(ui, &painter, empty, target_here.is_some());
            }
            if let Some(spot) = target_here {
                insertion_bar(ui, &others, spot.index, pos2(inner.left(), top));
            }
            self.zones.push(Zone {
                band: b,
                cell: Some((r, c)),
                rect: column,
                open: monitor.connected,
                cards,
            });
        }
    }

    fn card(
        &mut self,
        ui: &mut Ui,
        shared: &mut Shared,
        frame: &mut Frame,
        member: usize,
        rect: Rect,
        alpha: f32,
    ) {
        let tokens = Tokens::get(ui.ctx());
        let window = &self.group.windows[member];
        let response = ui.interact(
            rect,
            egui::Id::new(("arrange-card", window.hwnd)),
            Sense::click_and_drag(),
        );
        let mut painter = ui.painter().clone();
        painter.multiply_opacity(alpha);
        let is_cursor = shared.cursor == Some(window.hwnd);
        let lit = if is_cursor {
            Some(tokens.selection_bg)
        } else if response.hovered() && self.dragged.is_none() {
            Some(tokens.hover_bg)
        } else {
            None
        };
        paint_look(&painter, rect, 4, card_look(&tokens, lit, window.attention));
        if is_cursor {
            // Outside the picture, which DWM would draw over.
            painter.rect_stroke(
                rect.expand(3.0),
                6,
                theme::stroke(ui.ctx(), 2.0, tokens.accent),
                StrokeKind::Middle,
            );
        }
        let thumb = Rect::from_min_size(
            rect.min + Vec2::splat(CARD_PAD),
            vec2(rect.width() - 2.0 * CARD_PAD, THUMB_HEIGHT),
        );
        painter.rect(
            thumb,
            3,
            tokens.panel_bg,
            theme::stroke(ui.ctx(), 1.0, tokens.border),
            StrokeKind::Inside,
        );
        let picture = thumb.shrink(1.0);
        // The one live picture of a dragged window is the ghost's, so its
        // old place shows the name instead.
        let has_picture = !self.menu_open
            && self.dragged != Some(member)
            && thumbnail(shared, window.hwnd).is_some_and(|thumb| thumb.source_size().is_some());
        if has_picture {
            frame.pending.push(Pending {
                hwnd: window.hwnd,
                dest: picture,
                clip: ui.clip_rect(),
                opacity: (255.0 * alpha).round() as u8,
            });
        } else {
            name_card(&painter, picture, &window.label, "");
        }

        let title_top = thumb.bottom() + TITLE_GAP;
        let mut right = rect.right() - CARD_PAD - 2.0;
        // In the title line, not on the picture as in the design: DWM draws
        // the picture over anything painted there.
        if moved(window, self.monitors) {
            let chip = title_chip(&painter, right, title_top, "Moved", tokens.accent);
            right = chip.left() - 4.0;
        }
        if window.same_title {
            let chip = title_chip(&painter, right, title_top, "Same title", tokens.warning);
            right = chip.left() - 4.0;
            let mut chip_response = ui
                .interact(chip, response.id.with("same-title"), Sense::click())
                .on_hover_text(if self.can_rename {
                    format!("{SAME_TITLE_HINT} Click to rename.")
                } else {
                    SAME_TITLE_HINT.to_string()
                });
            if self.can_rename {
                chip_response = chip_response.on_hover_cursor(egui::CursorIcon::PointingHand);
                if chip_response.clicked() {
                    frame.actions.push(ArrangeAction::AskName(window.hwnd));
                }
            }
        }
        let left = rect.left() + CARD_PAD + 2.0;
        let mut job = text::job(
            &window.label,
            theme::regular(13.0),
            tokens.text_primary,
            None,
        );
        job.wrap = TextWrapping {
            max_width: (right - left).max(0.0),
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('\u{2026}'),
        };
        let galley = painter.layout_job(job);
        let elided = galley.elided;
        painter.galley(
            pos2(left, title_top + (TITLE - galley.size().y) / 2.0),
            galley,
            tokens.text_primary,
        );

        let mut response = if elided {
            response.on_hover_text(&window.label)
        } else {
            response
        };
        if self.dragged.is_none() {
            response = response.on_hover_cursor(egui::CursorIcon::Grab);
        }
        self.interact(shared, frame, member, &response);
    }

    /// Clicks, drags, the menu and the hover peek of one card.
    fn interact(&self, shared: &mut Shared, frame: &mut Frame, member: usize, response: &Response) {
        let window = &self.group.windows[member];
        frame.cards.push((window.hwnd, window.monitor.is_some()));
        if response.hovered() {
            frame.hovered = Some(window.hwnd);
        }
        if response.clicked() {
            frame.activate = Some(window.hwnd);
        }
        if response.drag_started() {
            if let Some(at) = response.interact_pointer_pos() {
                shared.drag = Some(Drag {
                    hwnd: window.hwnd,
                    grab: at - response.rect.min,
                });
                shared.cursor = Some(window.hwnd);
            }
        }
        let names = self.can_rename.then_some(window.renamed);
        response.context_menu(|ui| {
            if let Some(action) = card_menu(ui, window.hwnd, &window.desktop, self.desktops, names)
            {
                frame.actions.push(action);
            }
        });
    }
}

/// A column's label: the monitor icon, its name, its resolution, a
/// "Primary" chip and, at the right, how many windows it has. The friendly
/// name, when the monitor gives one, is the tooltip.
fn label_row(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    monitor: &ArrangeMonitor,
    count: usize,
) {
    let tokens = Tokens::get(ui.ctx());
    let painter = painter.with_clip_rect(rect);
    let centre = rect.center().y;
    icons::paint(
        &painter,
        Rect::from_center_size(pos2(rect.left() + 6.0, centre), Vec2::splat(12.0)),
        Icon::Monitor,
        tokens.text_disabled,
    );
    let mut x = rect.left() + 12.0 + 8.0;
    let name = painter.layout_no_wrap(
        monitor.label.clone(),
        theme::semibold(12.0),
        tokens.text_secondary,
    );
    let name_width = name.size().x;
    painter.galley(
        pos2(x, centre - name.size().y / 2.0),
        name,
        tokens.text_secondary,
    );
    x += name_width + 8.0;
    let detail = if monitor.connected {
        format!(
            "{}\u{d7}{}",
            monitor.rect[2] - monitor.rect[0],
            monitor.rect[3] - monitor.rect[1]
        )
    } else {
        "(disconnected)".to_string()
    };
    let detail = painter.layout_no_wrap(detail, theme::regular(12.0), tokens.text_disabled);
    let detail_width = detail.size().x;
    painter.galley(
        pos2(x, centre - detail.size().y / 2.0),
        detail,
        tokens.text_disabled,
    );
    x += detail_width + 8.0;
    if monitor.primary {
        let chip = painter.layout_no_wrap(
            "Primary".to_string(),
            theme::regular(11.0),
            tokens.text_disabled,
        );
        let chip_rect = Rect::from_min_size(
            pos2(x, centre - LABEL / 2.0),
            vec2(chip.size().x + 12.0, LABEL),
        );
        painter.rect_stroke(
            chip_rect,
            4,
            theme::stroke(ui.ctx(), 1.0, tokens.border),
            StrokeKind::Inside,
        );
        painter.galley(
            chip_rect.center() - chip.size() / 2.0,
            chip,
            tokens.text_disabled,
        );
    }
    if count > 0 {
        let count = painter.layout_no_wrap(
            count.to_string(),
            theme::regular(12.0),
            tokens.text_disabled,
        );
        painter.galley(
            pos2(rect.right() - count.size().x, centre - count.size().y / 2.0),
            count,
            tokens.text_disabled,
        );
    }
    if !monitor.name.is_empty() && monitor.name != monitor.label {
        ui.interact(
            rect,
            egui::Id::new(("arrange-monitor", monitor.device.as_str())),
            Sense::hover(),
        )
        .on_hover_text(&monitor.name);
    }
}

/// The dashed box of a column with no windows, outlined in the accent while
/// a card is about to land in it.
fn drop_box(ui: &Ui, painter: &egui::Painter, rect: Rect, target: bool) {
    let tokens = Tokens::get(ui.ctx());
    let colour = if target { tokens.accent } else { tokens.border };
    let stroke = theme::stroke(ui.ctx(), 1.0, colour);
    let r = rect.shrink(0.5);
    let corners = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    painter.extend(Shape::dashed_line(&corners, stroke, 4.0, 3.0));
    let mut job = text::job(
        "Drop a window here to move it to this monitor",
        theme::regular(12.0),
        tokens.text_disabled,
        None,
    );
    job.wrap = TextWrapping {
        max_width: rect.width() - 24.0,
        max_rows: 3,
        break_anywhere: false,
        overflow_character: Some('\u{2026}'),
    };
    job.halign = egui::Align::Center;
    let galley = painter.layout_job(job);
    painter.galley(
        pos2(rect.center().x, rect.center().y - galley.size().y / 2.0),
        galley,
        tokens.text_disabled,
    );
}

/// The 2 px accent bar where a dragged card would land: before the card at
/// `index` among `others`, after the last of them, or at `start` in a cell
/// with none.
fn insertion_bar(ui: &Ui, others: &[Rect], index: usize, start: egui::Pos2) {
    let tokens = Tokens::get(ui.ctx());
    let stroke = theme::stroke(ui.ctx(), 2.0, tokens.accent);
    let (x, top) = match (others.get(index), others.last()) {
        (Some(next), _) => (next.left() - CARD_GAP / 2.0, next.top()),
        (None, Some(last)) => (last.right() + CARD_GAP / 2.0, last.top()),
        (None, None) => (start.x - CARD_GAP / 2.0, start.y),
    };
    ui.painter()
        .vline(x, egui::Rangef::new(top, top + CARD_HEIGHT), stroke);
}

/// A chip at the right of a card's title line, ending at `right`; returns
/// its rectangle.
fn title_chip(
    painter: &egui::Painter,
    right: f32,
    top: f32,
    text_line: &str,
    colour: egui::Color32,
) -> Rect {
    let tokens = Tokens::get(painter.ctx());
    let galley = painter.layout_no_wrap(text_line.to_string(), theme::regular(11.0), colour);
    let chip = Rect::from_min_max(
        pos2(right - galley.size().x - 12.0, top),
        pos2(right, top + TITLE),
    );
    painter.rect(
        chip,
        4,
        tokens.window_bg,
        theme::stroke(painter.ctx(), 1.0, colour),
        StrokeKind::Inside,
    );
    painter.galley(chip.center() - galley.size() / 2.0, galley, colour);
    chip
}

/// The note on order and the key hints under the bands. The note is only
/// true while other desktops are on screen; the D hint needs some to show.
fn footer(ui: &Ui, panel: Rect, note: bool, others_key: bool) {
    let tokens = Tokens::get(ui.ctx());
    let painter = ui.painter();
    let area = Rect::from_min_max(pos2(panel.min.x, panel.max.y - FOOTER), panel.max);
    painter.hline(
        area.x_range(),
        area.top(),
        theme::stroke(ui.ctx(), 1.0, tokens.border),
    );
    let centre = area.top() + 1.0 + (FOOTER - 1.0) / 2.0;
    let mut hints = vec![
        ("\u{2190}\u{2192}", "Move within monitor"),
        ("Shift + \u{2190}\u{2192}", "Move to next monitor"),
        ("Shift + \u{2191}\u{2193}", "Move to next desktop"),
    ];
    if others_key {
        hints.push(("D", "Other desktops"));
    }
    let mut x = area.right() - SIDE;
    for (index, (key, label)) in hints.iter().enumerate().rev() {
        let label = painter.layout_no_wrap(
            label.to_string(),
            theme::regular(12.0),
            tokens.text_secondary,
        );
        x -= label.size().x;
        painter.galley(
            pos2(x, centre - label.size().y / 2.0),
            label,
            tokens.text_secondary,
        );
        x -= 6.0;
        let font = if key.is_ascii() {
            theme::regular(12.0)
        } else {
            theme::mono(12.0)
        };
        let key = painter.layout_no_wrap(key.to_string(), font, tokens.text_secondary);
        let chip = Rect::from_min_max(
            pos2(x - key.size().x - 12.0, centre - key.size().y / 2.0 - 1.0),
            pos2(x, centre + key.size().y / 2.0 + 1.0),
        );
        painter.rect(
            chip,
            4,
            tokens.panel_bg,
            theme::stroke(ui.ctx(), 1.0, tokens.border),
            StrokeKind::Inside,
        );
        painter.galley(chip.center() - key.size() / 2.0, key, tokens.text_secondary);
        x = chip.left();
        if index > 0 {
            x -= 16.0;
        }
    }
    if !note {
        return;
    }
    let mut job = text::job(
        "Order on other desktops applies at the next restore.",
        theme::regular(12.0),
        tokens.text_secondary,
        None,
    );
    job.wrap = TextWrapping {
        max_width: (x - 16.0 - area.left() - SIDE).max(0.0),
        max_rows: 1,
        break_anywhere: false,
        overflow_character: Some('\u{2026}'),
    };
    let note = painter.layout_job(job);
    painter.galley(
        pos2(area.left() + SIDE, centre - note.size().y / 2.0),
        note,
        tokens.text_secondary,
    );
}

/// ↑ and ↓ pick a window, ← and → move it within its column, Shift with
/// the arrows moves it to the next monitor or desktop, Enter switches to it.
/// Esc is handled with the other ways to close.
fn keyboard(
    ctx: &egui::Context,
    shared: &mut Shared,
    group: &ArrangeGroup,
    model: &Model,
    monitors: &[ArrangeMonitor],
    frame: &mut Frame,
) {
    let Some(cursor) = shared
        .cursor
        .and_then(|hwnd| group.windows.iter().position(|w| w.hwnd == hwnd))
    else {
        return;
    };
    // The Shift keys first: a key without modifiers also matches with Shift.
    let keys = [
        (egui::Modifiers::SHIFT, egui::Key::ArrowUp, Key::ShiftUp),
        (egui::Modifiers::SHIFT, egui::Key::ArrowDown, Key::ShiftDown),
        (egui::Modifiers::SHIFT, egui::Key::ArrowLeft, Key::ShiftLeft),
        (
            egui::Modifiers::SHIFT,
            egui::Key::ArrowRight,
            Key::ShiftRight,
        ),
        (egui::Modifiers::NONE, egui::Key::ArrowUp, Key::Up),
        (egui::Modifiers::NONE, egui::Key::ArrowDown, Key::Down),
        (egui::Modifiers::NONE, egui::Key::ArrowLeft, Key::Left),
        (egui::Modifiers::NONE, egui::Key::ArrowRight, Key::Right),
    ];
    let (pressed, enter) = ctx.input_mut(|i| {
        let pressed = keys
            .iter()
            .find(|(modifiers, key, _)| i.consume_key(*modifiers, *key))
            .map(|(_, _, key)| *key);
        (
            pressed,
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
        )
    });
    if enter {
        frame.activate = Some(group.windows[cursor].hwnd);
        return;
    }
    // A move to another desktop looks through all of them, so it also
    // works while they are hidden.
    let bands = if matches!(pressed, Some(Key::ShiftUp | Key::ShiftDown)) {
        &model.all
    } else {
        &model.bands
    };
    match pressed.and_then(|key| step(bands, &model.rows, monitors, cursor, key)) {
        Some(Step::Select(member)) => shared.cursor = Some(group.windows[member].hwnd),
        Some(Step::Move(spot)) => {
            if spot.band >= model.bands.len() {
                shared.others.expand();
            }
            frame.actions.extend(drop_actions(
                group,
                bands,
                &model.rows,
                monitors,
                cursor,
                spot,
            ));
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A card of a 16:9 window.
    fn card_size() -> Vec2 {
        vec2(card_width(None), CARD_HEIGHT)
    }

    const SAME: [f32; 10] = [179.0; 10];

    fn monitor(rect: [i32; 4], taskbar: i32, primary: bool) -> ArrangeMonitor {
        ArrangeMonitor {
            device: format!(r"\\.\DISPLAY{}", rect[0]),
            label: String::new(),
            name: String::new(),
            rect,
            work: [rect[0], rect[1], rect[2], rect[3] - taskbar],
            primary,
            connected: true,
        }
    }

    fn disconnected() -> ArrangeMonitor {
        ArrangeMonitor {
            connected: false,
            ..monitor([0, 0, 0, 0], 0, false)
        }
    }

    /// The monitors of the PC this was built on, in the order Windows lists
    /// them: the primary 3840 x 2160 one (150 %), the 1920 x 1080 one to its
    /// left (100 %) and the portrait-like 2560 x 2880 one to its right
    /// (175 %).
    fn desk() -> Vec<ArrangeMonitor> {
        vec![
            monitor([0, 0, 3840, 2160], 72, true),
            monitor([-1920, 0, 0, 1080], 48, false),
            monitor([3840, 0, 6400, 2880], 84, false),
        ]
    }

    fn every(_: usize) -> bool {
        true
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn columns_follow_the_monitors_from_left_to_right() {
        assert_eq!(monitor_rows(&desk(), &every), [vec![1, 0, 2]]);
    }

    #[test]
    fn stacked_monitors_get_a_row_each_and_ties_go_by_top() {
        let monitors = vec![
            monitor([0, 0, 1920, 1080], 48, true),
            monitor([1920, 0, 4480, 1440], 48, false),
            monitor([0, -1080, 1920, 0], 0, false),
        ];
        // The one above the primary comes first at the same left edge; the
        // primary under it shares its width, so it starts a second row.
        assert_eq!(monitor_rows(&monitors, &every), [vec![2, 1], vec![0]]);
    }

    #[test]
    fn a_disconnected_monitor_follows_on_the_first_row_when_shown() {
        let mut monitors = desk();
        monitors.push(disconnected());
        assert_eq!(monitor_rows(&monitors, &every), [vec![1, 0, 2, 3]]);
        assert_eq!(monitor_rows(&monitors, &|i| i != 3), [vec![1, 0, 2]]);
        // It is as wide as the narrowest connected monitor.
        assert_eq!(
            weights(&monitors, &[1, 0, 2, 3]),
            [1920.0, 3840.0, 2560.0, 1920.0]
        );
    }

    #[test]
    fn column_widths_follow_monitor_widths() {
        // The design's example: 1920 / 3440 / 1920.
        let widths = tracks(&[1920.0, 3440.0, 1920.0], 1400.0);
        assert!(close(widths.iter().sum(), 1400.0));
        assert!(close(widths[1] / widths[0], 3440.0 / 1920.0));
        assert!(close(widths[0], widths[2]));

        // This PC: 1920 / 3840 / 2560.
        let widths = tracks(&[1920.0, 3840.0, 2560.0], 2000.0);
        assert!(close(widths[0], 2000.0 * 1920.0 / 8320.0));
        assert!(close(widths[1], 2000.0 * 3840.0 / 8320.0));
        assert!(close(widths[2], 2000.0 * 2560.0 / 8320.0));
    }

    #[test]
    fn a_column_is_never_narrower_than_240() {
        // 1920 of 7280 at 900 would be 237: the side columns are held at
        // 240 and the middle one gets the rest.
        let widths = tracks(&[1920.0, 3440.0, 1920.0], 900.0);
        assert_eq!(widths, [MIN_TRACK, 420.0, MIN_TRACK]);
        // Too narrow for all three: each keeps 240 and the row scrolls.
        let widths = tracks(&[1920.0, 3440.0, 1920.0], 600.0);
        assert_eq!(widths, [MIN_TRACK; 3]);
        assert!(widths.iter().sum::<f32>() > 600.0);
        assert!(tracks(&[], 600.0).is_empty());
    }

    fn desktops() -> Vec<ArrangeDesktop> {
        ["Home", "Work", "Reading"]
            .iter()
            .enumerate()
            .map(|(i, name)| ArrangeDesktop {
                id: format!("d{i}"),
                name: name.to_string(),
                current: i == 1,
            })
            .collect()
    }

    /// Windows as (desktop, monitor now, monitor at the last save).
    fn group(windows: &[(&str, Option<usize>, Option<usize>)]) -> ArrangeGroup {
        ArrangeGroup {
            key: "chrome.exe".to_string(),
            label: "Chrome".to_string(),
            windows: windows
                .iter()
                .enumerate()
                .map(|(i, (desktop, on, saved))| ArrangeWindow {
                    hwnd: 100 + i as isize,
                    label: format!("w{i}"),
                    desktop: desktop.to_string(),
                    monitor: None,
                    on_monitor: *on,
                    saved_monitor: *saved,
                    renamed: false,
                    same_title: false,
                    attention: false,
                })
                .collect(),
        }
    }

    #[test]
    fn bands_put_this_desktop_first_then_task_view_order() {
        let monitors = desk();
        let rows = monitor_rows(&monitors, &every);
        let group = group(&[
            ("d2", Some(0), Some(0)),
            ("d1", Some(1), Some(1)),
            ("d1", Some(0), Some(0)),
            ("", None, None),
            ("d1", Some(2), Some(2)),
        ]);
        let bands = bands(&group, &desktops(), &monitors, &rows);
        let titles: Vec<&str> = bands.iter().map(|b| b.title.as_str()).collect();
        assert_eq!(titles, ["This desktop", "Home", "Reading"]);
        assert_eq!(bands[0].number, Some(2));
        // Columns are Left, Centre (primary), Right; the window with no
        // known desktop or monitor is on this desktop's primary monitor.
        assert_eq!(bands[0].cells, [vec![vec![1], vec![2, 3], vec![4]]]);
        assert_eq!(bands[2].cells, [vec![vec![], vec![0], vec![]]]);
        assert_eq!(bands[0].meta(), "Desktop 2 \u{b7} 4 windows");
        assert_eq!(bands[2].meta(), "Desktop 3 \u{b7} 1 window");
    }

    #[test]
    fn a_desktop_without_windows_is_its_header_alone() {
        let monitors = desk();
        let rows = monitor_rows(&monitors, &every);
        let bands = bands(
            &group(&[("d1", Some(0), None)]),
            &desktops(),
            &monitors,
            &rows,
        );
        assert_eq!(bands[1].count(), 0);
        let tracks = vec![row_tracks(&weights(&monitors, &rows[0]), 1200.0)];
        assert_eq!(
            band_height(&bands[1], &tracks, &SAME),
            1.0 + BAND_TOP + BAND_HEADER + BAND_TOP
        );
        // One card in the primary column; the empty ones show the 112 high
        // drop target, which is the taller of the two.
        assert_eq!(
            band_height(&bands[0], &tracks, &SAME),
            1.0 + BAND_TOP
                + BAND_HEADER
                + HEADER_GAP
                + LABEL
                + LABEL_GAP
                + CARD_HEIGHT.max(EMPTY)
                + BAND_BOTTOM
        );
    }

    #[test]
    fn without_desktops_every_window_is_on_this_desktop() {
        let monitors = desk();
        let rows = monitor_rows(&monitors, &every);
        let bands = bands(&group(&[("d7", Some(1), None)]), &[], &monitors, &rows);
        assert_eq!(bands.len(), 1);
        assert_eq!(bands[0].number, None);
        assert_eq!(bands[0].meta(), "1 window");
        assert_eq!(bands[0].cells, [vec![vec![0], vec![], vec![]]]);
    }

    #[test]
    fn a_window_saved_on_a_disconnected_monitor_stays_in_its_column() {
        let mut monitors = desk();
        monitors.push(disconnected());
        let windows = group(&[
            ("d1", Some(0), Some(3)),
            ("d1", Some(0), Some(1)),
            ("d1", Some(2), None),
        ]);
        assert_eq!(column_of(&windows.windows[0], &monitors), Some(3));
        // Moved by hand since the save: its column is where it is now.
        assert_eq!(column_of(&windows.windows[1], &monitors), Some(0));
        assert_eq!(column_of(&windows.windows[2], &monitors), Some(2));
    }

    #[test]
    fn the_panel_is_as_wide_as_one_line_of_cards_per_column_needs() {
        let monitors = desk();
        let rows = monitor_rows(&monitors, &every);
        let weights: Vec<Vec<f32>> = rows.iter().map(|row| weights(&monitors, row)).collect();
        let one_each = bands(
            &group(&[
                ("d1", Some(1), None),
                ("d1", Some(0), None),
                ("d1", Some(2), None),
            ]),
            &desktops(),
            &monitors,
            &rows,
        );
        // The 1920 column is the narrowest share; it needs 240, so the
        // three tracks need 240 * 8320 / 1920.
        let available = MIN_TRACK * 8320.0 / 1920.0;
        assert_eq!(card_size().x, SAME[0]);
        let width = natural_width(&one_each, &weights, &SAME);
        assert!(close(width, available + 2.0 * COLUMN_GAP + 2.0 * SIDE));
        let size = panel_size(&one_each, &weights, &SAME, false, vec2(5000.0, 5000.0));
        assert!(close(size.x, width + SLACK));
        let tracks = row_tracks(&weights[0], size.x);
        assert!(tracks[0] >= MIN_TRACK);
        // Two cards fit side by side in the centre column, not three.
        assert_eq!(cell_lines(&[0, 1, 2], &SAME, tracks[1]), [0..2, 2..3]);

        // Never wider than the limit, nor taller.
        let small = panel_size(&one_each, &weights, &SAME, false, vec2(800.0, 300.0));
        assert_eq!(small, vec2(800.0, 300.0));
    }

    #[test]
    fn mixed_cards_wrap_by_their_widths_inside_a_column() {
        // A 240 column holds 227 of cards: a 60 and a 140 side by side
        // (208), then a 230 that only fits alone.
        let widths = [60.0, 140.0, 230.0, 60.0];
        assert_eq!(
            cell_lines(&[0, 1, 2, 3], &widths, MIN_TRACK),
            [0..2, 2..3, 3..4]
        );
        assert_eq!(
            cell_height(&[0, 1, 2, 3], &widths, MIN_TRACK),
            3.0 * CARD_HEIGHT + 2.0 * CARD_GAP
        );
        assert_eq!(cell_height(&[], &widths, MIN_TRACK), EMPTY);
        // The widest cell on one line sets the panel's width.
        let band = Band {
            desktop: String::new(),
            title: String::new(),
            number: None,
            current: true,
            cells: vec![vec![vec![0, 1, 2]]],
        };
        let width = natural_width(&[band], &[vec![1.0]], &widths);
        assert!(close(width, MIN_PANEL.max(INSET + 446.0 + 2.0 * SIDE)));
    }

    #[test]
    fn the_landing_spot_follows_cards_of_mixed_widths() {
        let at = |x: f32, w: f32, y: f32| Rect::from_min_size(pos2(x, y), vec2(w, CARD_HEIGHT));
        let zones = vec![Zone {
            band: 0,
            cell: Some((0, 0)),
            rect: Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 400.0)),
            open: true,
            // A narrow card, a wide one, and a third card on the next line.
            cards: vec![
                (0, at(13.0, 60.0, 26.0)),
                (1, at(81.0, 230.0, 26.0)),
                (2, at(13.0, 179.0, 26.0 + CARD_HEIGHT + CARD_GAP)),
            ],
        }];
        // Over the wide card's left half: before it.
        assert_eq!(
            drop_spot(pos2(150.0, 60.0), &zones, 9, (0, 0)),
            Some(spot(0, (0, 0), 1))
        );
        // Over its right half: after it.
        assert_eq!(
            drop_spot(pos2(250.0, 60.0), &zones, 9, (0, 0)),
            Some(spot(0, (0, 0), 2))
        );
        // On the second line, past the third card's middle.
        assert_eq!(
            drop_spot(pos2(150.0, 200.0), &zones, 9, (0, 0)),
            Some(spot(0, (0, 0), 3))
        );
    }

    #[test]
    fn hidden_bands_are_not_laid_out_or_found_while_collapsed() {
        let all = three_columns();
        let collapsed = visible(all.clone(), false);
        assert_eq!(collapsed.len(), 1);
        assert!(collapsed[0].current);
        // Arrow keys and drops only know the bands on screen.
        assert_eq!(walk(&collapsed), [0, 1, 2, 3, 4]);
        assert_eq!(locate(&collapsed, 5), None);
        assert_eq!(visible(all, true).len(), 2);
    }

    #[test]
    fn the_collapsed_panel_is_shorter_and_the_link_adds_a_line() {
        let weights = vec![vec![1.0, 1.0, 1.0]];
        let limit = vec2(5000.0, 5000.0);
        let all = three_columns();
        let open = panel_size(&all, &weights, &SAME, true, limit);
        let shut = panel_size(&visible(all.clone(), false), &weights, &SAME, true, limit);
        let bare = panel_size(&visible(all, false), &weights, &SAME, false, limit);
        assert!(shut.y < open.y);
        assert!(close(shut.y - bare.y, others::LINK_HEIGHT));
    }

    #[test]
    fn shift_down_reaches_a_hidden_desktop_through_all_the_bands() {
        let all = three_columns();
        let monitors = vec![monitor([0, 0, 1920, 1080], 48, true); 3];
        let rows = vec![vec![0, 1, 2]];
        let collapsed = visible(all.clone(), false);
        assert_eq!(step(&collapsed, &rows, &monitors, 0, Key::ShiftDown), None);
        let Some(Step::Move(spot)) = step(&all, &rows, &monitors, 0, Key::ShiftDown) else {
            panic!("expected a move");
        };
        assert!(spot.band >= collapsed.len());
    }

    fn three_columns() -> Vec<Band> {
        vec![
            Band {
                desktop: "d1".to_string(),
                title: "This desktop".to_string(),
                number: Some(2),
                current: true,
                cells: vec![vec![vec![0, 1], vec![2, 3, 4], vec![]]],
            },
            Band {
                desktop: "d0".to_string(),
                title: "Home".to_string(),
                number: Some(1),
                current: false,
                cells: vec![vec![vec![], vec![5], vec![]]],
            },
        ]
    }

    fn spot(band: usize, cell: (usize, usize), index: usize) -> Spot {
        Spot { band, cell, index }
    }

    #[test]
    fn a_move_inside_a_column_keeps_every_other_window_in_its_order() {
        let bands = three_columns();
        // 3 goes before 2, its neighbour in the centre column.
        assert_eq!(
            order_after(6, &bands, 3, spot(0, (0, 1), 0)),
            [0, 1, 3, 2, 4, 5]
        );
        // 0 goes after 1, the last other window of its column.
        assert_eq!(
            order_after(6, &bands, 0, spot(0, (0, 0), 1)),
            [1, 0, 2, 3, 4, 5]
        );
        // Into an empty column it keeps its place.
        assert_eq!(
            order_after(6, &bands, 2, spot(0, (0, 2), 0)),
            [0, 1, 2, 3, 4, 5]
        );
        // Into the other band, before 5.
        assert_eq!(
            order_after(6, &bands, 1, spot(1, (0, 1), 0)),
            [0, 2, 3, 4, 1, 5]
        );
    }

    #[test]
    fn a_drop_in_another_band_moves_the_window_to_that_desktop_first() {
        let bands = three_columns();
        let monitors = desk();
        let rows = monitor_rows(&monitors, &every);
        let group = group(&[
            ("d1", Some(1), None),
            ("d1", Some(1), None),
            ("d1", Some(0), None),
            ("d1", Some(0), None),
            ("d1", Some(0), None),
            ("d0", Some(0), None),
        ]);
        let actions = drop_actions(&group, &bands, &rows, &monitors, 1, spot(1, (0, 1), 1));
        assert_eq!(
            actions[0],
            ArrangeAction::MoveToDesktop {
                hwnd: 101,
                desktop: "d0".to_string()
            }
        );
        // From the left monitor's column to the primary's: moved there too.
        assert_eq!(
            actions[1],
            ArrangeAction::MoveToMonitor {
                hwnd: 101,
                monitor: monitors[0].device.clone()
            }
        );
        assert_eq!(
            actions[2],
            ArrangeAction::Reorder {
                group: "chrome.exe".to_string(),
                order: vec![100, 102, 103, 104, 105, 101],
            }
        );
        // Dropped where it already is: nothing to do.
        assert!(drop_actions(&group, &bands, &rows, &monitors, 3, spot(0, (0, 1), 1)).is_empty());
    }

    #[test]
    fn a_drop_on_another_column_moves_the_window_to_that_monitor_now() {
        let bands = three_columns();
        let monitors = desk();
        let rows = monitor_rows(&monitors, &every);
        let group = group(&[
            ("d1", Some(1), None),
            ("d1", Some(1), None),
            ("d1", Some(0), None),
            ("d1", Some(0), None),
            ("d1", Some(0), None),
            ("d0", Some(0), None),
        ]);
        // Window 3, on the primary monitor, to the empty right column.
        assert_eq!(
            drop_actions(&group, &bands, &rows, &monitors, 3, spot(0, (0, 2), 0)),
            [
                ArrangeAction::MoveToMonitor {
                    hwnd: 103,
                    monitor: monitors[2].device.clone()
                },
                // Its place in the taskbar is the same; the order is sent
                // anyway, as after a desktop move.
                ArrangeAction::Reorder {
                    group: "chrome.exe".to_string(),
                    order: (100..106).collect(),
                }
            ]
        );
    }

    #[test]
    fn a_window_from_a_disconnected_column_is_kept_where_it_is_dropped() {
        let mut monitors = desk();
        monitors.push(disconnected());
        let rows = monitor_rows(&monitors, &every);
        // Saved on the unplugged monitor, on the primary one now.
        let group = group(&[("d1", Some(0), Some(3))]);
        let bands = bands(&group, &desktops(), &monitors, &rows);
        assert_eq!(bands[0].cells[0][3], [0]);
        // Dropped on the primary column: that is where it is, but the
        // plugin hears of it, so the saved monitor follows.
        assert_eq!(
            drop_actions(&group, &bands, &rows, &monitors, 0, spot(0, (0, 1), 0)),
            [
                ArrangeAction::MoveToMonitor {
                    hwnd: 100,
                    monitor: monitors[0].device.clone()
                },
                ArrangeAction::Reorder {
                    group: "chrome.exe".to_string(),
                    order: vec![100],
                }
            ]
        );
    }

    #[test]
    fn a_window_on_another_monitor_than_saved_is_marked_moved() {
        let mut monitors = desk();
        monitors.push(disconnected());
        let windows = group(&[
            ("d1", Some(0), Some(1)),
            ("d1", Some(0), Some(0)),
            ("d1", Some(0), None),
            ("d1", Some(0), Some(3)),
            ("d1", None, Some(1)),
        ]);
        let marked: Vec<bool> = windows
            .windows
            .iter()
            .map(|w| moved(w, &monitors))
            .collect();
        // Only the first: the second is where it was saved, the third was
        // never saved, the fourth sits in its disconnected column and the
        // fifth is on no known monitor.
        assert_eq!(marked, [true, false, false, false, false]);
    }

    #[test]
    fn the_keys_walk_across_columns_and_move_within_one() {
        let bands = three_columns();
        let monitors = desk();
        let rows = monitor_rows(&monitors, &every);
        let step = |cursor: usize, key: Key| step(&bands, &rows, &monitors, cursor, key);
        assert_eq!(walk(&bands), [0, 1, 2, 3, 4, 5]);
        assert_eq!(step(1, Key::Down), Some(Step::Select(2)));
        assert_eq!(step(4, Key::Down), Some(Step::Select(5)));
        assert_eq!(step(5, Key::Down), None);
        assert_eq!(step(0, Key::Up), None);
        assert_eq!(step(2, Key::Up), Some(Step::Select(1)));
        assert_eq!(step(3, Key::Left), Some(Step::Move(spot(0, (0, 1), 0))));
        assert_eq!(step(3, Key::Right), Some(Step::Move(spot(0, (0, 1), 2))));
        assert_eq!(step(4, Key::Right), None);
        assert_eq!(step(2, Key::Left), None);
        assert_eq!(
            order_after(6, &bands, 3, spot(0, (0, 1), 2)),
            [0, 1, 2, 4, 3, 5]
        );
    }

    #[test]
    fn shift_moves_to_the_end_of_the_next_monitor_or_desktop() {
        let bands = three_columns();
        let mut monitors = desk();
        let rows = monitor_rows(&monitors, &every);
        let step = |monitors: &[ArrangeMonitor], cursor: usize, key: Key| {
            step(&bands, &rows, monitors, cursor, key)
        };
        // Window 0 is on the left monitor: right goes to the primary's end.
        assert_eq!(
            step(&monitors, 0, Key::ShiftRight),
            Some(Step::Move(spot(0, (0, 1), 3)))
        );
        assert_eq!(step(&monitors, 0, Key::ShiftLeft), None);
        // From the primary to the empty right column.
        assert_eq!(
            step(&monitors, 3, Key::ShiftRight),
            Some(Step::Move(spot(0, (0, 2), 0)))
        );
        // Down to the next desktop, same monitor; up from the first is none.
        assert_eq!(
            step(&monitors, 3, Key::ShiftDown),
            Some(Step::Move(spot(1, (0, 1), 1)))
        );
        assert_eq!(
            step(&monitors, 0, Key::ShiftDown),
            Some(Step::Move(spot(1, (0, 0), 0)))
        );
        assert_eq!(step(&monitors, 0, Key::ShiftUp), None);
        assert_eq!(
            step(&monitors, 5, Key::ShiftUp),
            Some(Step::Move(spot(0, (0, 1), 3)))
        );
        // A disconnected right monitor is skipped; its windows go down to
        // the primary column of the next desktop.
        monitors[2].connected = false;
        assert_eq!(step(&monitors, 3, Key::ShiftRight), None);
        let mut in_gone = three_columns();
        in_gone[0].cells[0][2].push(6);
        assert_eq!(
            super::step(&in_gone, &rows, &monitors, 6, Key::ShiftDown),
            Some(Step::Move(spot(1, (0, 1), 1)))
        );
        assert_eq!(
            super::step(&in_gone, &rows, &monitors, 6, Key::ShiftLeft),
            Some(Step::Move(spot(0, (0, 1), 3)))
        );
    }

    #[test]
    fn the_drop_spot_comes_from_last_frames_zones() {
        let card = |x: f32, y: f32| Rect::from_min_size(pos2(x, y), card_size());
        let column = |x: f32| Rect::from_min_size(pos2(x, 0.0), vec2(300.0, 400.0));
        let zones = vec![
            Zone {
                band: 0,
                cell: Some((0, 0)),
                rect: column(0.0),
                open: true,
                cards: vec![(0, card(13.0, 26.0)), (1, card(13.0, 162.0))],
            },
            Zone {
                band: 0,
                cell: Some((0, 1)),
                rect: column(316.0),
                open: false,
                cards: vec![(2, card(329.0, 26.0))],
            },
            Zone {
                band: 1,
                cell: None,
                rect: Rect::from_min_size(pos2(0.0, 400.0), vec2(700.0, 43.0)),
                open: true,
                cards: Vec::new(),
            },
        ];
        // Below the first card of the first column.
        assert_eq!(
            drop_spot(pos2(50.0, 170.0), &zones, 2, (0, 0)),
            Some(spot(0, (0, 0), 1))
        );
        // Its own card does not count when the dragged one is in the zone.
        assert_eq!(
            drop_spot(pos2(50.0, 300.0), &zones, 0, (0, 0)),
            Some(spot(0, (0, 0), 1))
        );
        // A disconnected column takes no other window: the nearest open
        // zone does, and its own window can still be put back in it.
        assert_eq!(
            drop_spot(pos2(500.0, 100.0), &zones, 0, (0, 0)),
            Some(spot(0, (0, 0), 0))
        );
        assert_eq!(
            drop_spot(pos2(500.0, 100.0), &zones, 2, (0, 0)),
            Some(spot(0, (0, 1), 0))
        );
        // A bare band header puts the window in the home cell.
        assert_eq!(
            drop_spot(pos2(500.0, 420.0), &zones, 0, (0, 1)),
            Some(spot(1, (0, 1), 0))
        );
    }
}
