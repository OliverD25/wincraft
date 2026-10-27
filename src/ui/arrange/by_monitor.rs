//! The By monitor view: one band per virtual desktop, the current one first,
//! and inside each band one column per monitor, laid out the way the
//! monitors stand on the desk.

use egui::text::TextWrapping;
use egui::{pos2, vec2, Color32, Rect, Response, Sense, Shape, StrokeKind, Ui, UiBuilder, Vec2};

use super::{
    card_above_or_below, card_look, card_menu, ctrl_drop, ctrl_hint, ctrl_mark, ctrl_place,
    ctrl_target, float_picture, ghost_thumbnail, grown, hit_rect, line_width, middle_close,
    name_card, nudge, number_chip, order_with_place, others, others_link, paint_badges, paint_look,
    picture_width, place_in_row, taskbar_numbers, thumbnail, visible_flags, wrap, Badge, Drag,
    Frame, Pending, Shared, NUMBER_GAP, SLACK,
};
use crate::core::taskbar_scope::MultiMonitor;
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

/// The height of the footer: none while the user has hidden the hints.
fn footer_height(hints: bool) -> f32 {
    if hints {
        FOOTER
    } else {
        0.0
    }
}
const BAND_TOP: f32 = 12.0;
const BAND_BOTTOM: f32 = 16.0;
const BAND_HEADER: f32 = 18.0;
const HEADER_GAP: f32 = 10.0;
const COLUMN_GAP: f32 = 16.0;
/// Between two rows of monitors stacked one above the other.
const ROW_GAP: f32 = 16.0;
pub const MIN_TRACK: f32 = 200.0;
/// A monitor with no windows, folded into a narrow rail.
const RAIL: f32 = 40.0;
/// A rail while a card is dragged, or while the keyboard is on it.
const RAIL_OPEN: f32 = 200.0;
const RAIL_ANIMATION: f32 = 0.15;
const RAIL_MIN_HEIGHT: f32 = 150.0;
const RAIL_ICON: f32 = 14.0;
const RAIL_PAD: f32 = 8.0;
const RAIL_TEXT_GAP: f32 = 6.0;
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
/// A stand-in in the primary column for a window on another monitor.
const GHOST_WIDTH: f32 = 84.0;
/// Its name strip: the monitor pill, the title written upward and the
/// number.
const GHOST_STRIP: f32 = 20.0;
const GHOST_PILL: f32 = 14.0;
const GHOST_ARROW: f32 = 8.0;
/// Above the pill's arrow and below its letter.
const GHOST_PILL_PAD: f32 = 3.0;
const GHOST_TOP: f32 = 4.0;
const GHOST_BOTTOM: f32 = 5.0;
const GHOST_GAP: f32 = 5.0;
const GHOST_CHIP: f32 = 16.0;
/// DWM cannot draw a picture in grey, so a ghost's picture is only faded.
const GHOST_OPACITY: f32 = 0.4;

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

/// One monitor column of a row, as far as its width goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Col {
    /// The width its cards need on one line, rule and padding included. The
    /// widest of all the bands on screen.
    need: f32,
    /// How many windows it holds, in all the bands on screen.
    count: usize,
    primary: bool,
    /// A connected monitor with no windows, folded into a narrow rail.
    rail: bool,
    /// The monitor's width in pixels, which sets its share of the row.
    screen: f32,
}

/// The columns of each row of monitors. A monitor with no windows becomes a
/// rail, but only when there is another column in its row that has some:
/// a strip limited to one monitor, or a row with nothing in it, keeps
/// ordinary columns. A disconnected monitor is never a rail.
pub fn columns(
    bands: &[Band],
    rows: &[Vec<usize>],
    monitors: &[ArrangeMonitor],
    widths: &[f32],
) -> Vec<Vec<Col>> {
    rows.iter()
        .enumerate()
        .map(|(r, row)| {
            let mut cols: Vec<Col> = row
                .iter()
                .enumerate()
                .map(|(c, monitor)| {
                    let monitor = &monitors[*monitor];
                    let count = bands.iter().map(|band| band.cells[r][c].len()).sum();
                    let widest = bands
                        .iter()
                        .map(|band| line_width(&flow_widths(&band.flow((r, c)), widths)))
                        .fold(card_width(None), f32::max);
                    Col {
                        need: INSET + widest,
                        count,
                        primary: monitor.primary && monitor.connected,
                        rail: monitor.connected && count == 0,
                        screen: (monitor.rect[2] - monitor.rect[0]).max(0) as f32,
                    }
                })
                .collect();
            if cols.len() < 2 || cols.iter().all(|col| col.count == 0) {
                for col in &mut cols {
                    col.rail = false;
                }
            }
            cols
        })
        .collect()
}

/// The column that takes the space the others leave when the columns do not
/// all fit on one line: the one with the widest content, the primary
/// monitor's when several tie, else the leftmost. Ghosts are part of the
/// content, so a column of one card and two ghosts can beat one of two cards.
fn winner(cols: &[Col]) -> Option<usize> {
    let widest = cols
        .iter()
        .filter(|col| !col.rail)
        .map(|col| col.need)
        .fold(None, |best: Option<f32>, need| {
            Some(best.map_or(need, |best| best.max(need)))
        })?;
    let tied = || {
        cols.iter()
            .enumerate()
            .filter(|(_, col)| !col.rail && col.need >= widest)
    };
    tied()
        .find(|(_, col)| col.primary)
        .or_else(|| tied().next())
        .map(|(index, _)| index)
}

/// The column that takes the spare width when every column fits on one line:
/// the last one with windows, else the last one that is not a rail.
fn last_filled(cols: &[Col]) -> Option<usize> {
    let last = |filled: bool| {
        cols.iter()
            .rposition(|col| !col.rail && (!filled || col.count > 0))
    };
    last(true).or_else(|| last(false))
}

/// A rail `open` of the way (0 to 1) from folded to unfolded.
fn rail_width(open: f32) -> f32 {
    RAIL + (RAIL_OPEN - RAIL) * open
}

/// The width of a column that is not the winner, when not all fit: what one line of its cards
/// needs, at most its monitor's share of `available` (its part of the
/// width of all the row's monitors), and at least MIN_TRACK.
fn capped(col: &Col, cols: &[Col], available: f32) -> f32 {
    let total: f32 = cols.iter().map(|col| col.screen).sum();
    let share = if total > 0.0 && col.screen > 0.0 {
        available * (col.screen / total)
    } else {
        available
    };
    col.need.min(share).max(MIN_TRACK)
}

/// Each column's width out of `available`. A rail is as wide as its `open`
/// says. When every other column fits on one line (its `need`) beside the
/// rails, each gets exactly that and the last column with windows takes the
/// spare width. Otherwise the widest one (see `winner`) takes what the
/// others leave, also at least MIN_TRACK, and the others are `capped`: when
/// even that does not fit, the columns add up to more than `available` and
/// the row scrolls sideways.
pub fn tracks(cols: &[Col], open: &[f32], available: f32) -> Vec<f32> {
    let mut widths: Vec<f32> = cols
        .iter()
        .enumerate()
        .map(|(index, col)| {
            if col.rail {
                rail_width(open.get(index).copied().unwrap_or(0.0))
            } else {
                col.need
            }
        })
        .collect();
    let all: f32 = widths.iter().sum();
    if all <= available {
        if let Some(last) = last_filled(cols) {
            widths[last] += available - all;
        }
        return widths;
    }
    for (index, col) in cols.iter().enumerate() {
        if !col.rail {
            widths[index] = capped(col, cols, available);
        }
    }
    if let Some(winner) = winner(cols) {
        let others: f32 = widths
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != winner)
            .map(|(_, width)| width)
            .sum();
        widths[winner] = (available - others).max(MIN_TRACK);
    }
    widths
}

/// The columns of a row that is `width` wide, side padding and gaps taken
/// out first.
fn row_tracks(cols: &[Col], open: &[f32], width: f32) -> Vec<f32> {
    let gaps = COLUMN_GAP * cols.len().saturating_sub(1) as f32;
    tracks(cols, open, width - 2.0 * SIDE - gaps)
}

/// A card's width for a window whose picture shows `source`.
pub fn card_width(source: Option<Vec2>) -> f32 {
    picture_width(source, THUMB_HEIGHT) + 2.0 * CARD_PAD
}

/// One thing in a cell's flow: a window's own card, or the ghost of a
/// window on another monitor, as places in the group's windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Card(usize),
    Ghost(usize),
}

impl Item {
    pub fn member(self) -> usize {
        match self {
            Item::Card(member) | Item::Ghost(member) => member,
        }
    }
}

/// The widths of a cell's cards and ghosts, from every window's card width.
fn flow_widths(flow: &[Item], widths: &[f32]) -> Vec<f32> {
    flow.iter()
        .map(|item| match item {
            Item::Card(member) => widths[*member],
            Item::Ghost(_) => GHOST_WIDTH,
        })
        .collect()
}

/// A cell's cards and ghosts, `widths` wide, in lines that fit in a column
/// `track` wide.
fn cell_lines(widths: &[f32], track: f32) -> Vec<std::ops::Range<usize>> {
    wrap(widths, track - INSET)
}

fn cell_height(widths: &[f32], track: f32) -> f32 {
    if widths.is_empty() {
        return EMPTY;
    }
    let lines = cell_lines(widths, track).len() as f32;
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
    /// The cell that shows `ghosts`: the primary monitor's, while the main
    /// taskbar lists every window.
    pub ghost_cell: Option<(usize, usize)>,
    /// The windows of the other connected monitors' columns, in taskbar
    /// order, as places in the group's windows.
    pub ghosts: Vec<usize>,
}

impl Band {
    pub fn count(&self) -> usize {
        self.cells.iter().flatten().map(Vec::len).sum()
    }

    /// A cell's cards and, in the ghost cell, the ghosts among them, in
    /// taskbar order: the group's order is the taskbar's.
    pub fn flow(&self, cell: (usize, usize)) -> Vec<Item> {
        let mut items: Vec<Item> = self.cells[cell.0][cell.1]
            .iter()
            .map(|member| Item::Card(*member))
            .collect();
        if self.ghost_cell == Some(cell) {
            items.extend(self.ghosts.iter().map(|member| Item::Ghost(*member)));
            items.sort_by_key(|item| item.member());
        }
        items
    }

    /// Where a place among the cell's cards other than `moved` is in its
    /// flow without `moved`: the keys move among cards and step over
    /// ghosts. Before the card at `index`, after the last card, or in a cell
    /// with no other card where `moved` falls by taskbar order.
    pub fn flow_index(&self, cell: (usize, usize), moved: usize, index: usize) -> usize {
        let flow: Vec<usize> = self
            .flow(cell)
            .iter()
            .map(|item| item.member())
            .filter(|member| *member != moved)
            .collect();
        let cards: Vec<usize> = self.cells[cell.0][cell.1]
            .iter()
            .copied()
            .filter(|member| *member != moved)
            .collect();
        match (cards.get(index), cards.last()) {
            (Some(next), _) => flow.iter().position(|m| m == next).unwrap_or(flow.len()),
            (None, Some(last)) => flow
                .iter()
                .position(|m| m == last)
                .map_or(flow.len(), |p| p + 1),
            (None, None) => flow.iter().filter(|m| **m < moved).count(),
        }
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
        ghost_cell: None,
        ghosts: Vec::new(),
    });
    for (index, desktop) in desktops.iter().enumerate() {
        if !desktop.current {
            bands.push(Band {
                desktop: desktop.id.clone(),
                title: desktop.name.clone(),
                number: Some(index + 1),
                current: false,
                cells: empty.clone(),
                ghost_cell: None,
                ghosts: Vec::new(),
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

/// The primary monitor's cell, when its column shows ghosts of the windows
/// on the other monitors: only while the main taskbar lists every window,
/// the strip is not limited to one monitor, and another monitor is
/// connected.
pub fn ghost_cell(
    monitors: &[ArrangeMonitor],
    rows: &[Vec<usize>],
    taskbars: MultiMonitor,
    only: Option<usize>,
) -> Option<(usize, usize)> {
    if only.is_some() || taskbars.shows_only_own(true) {
        return None;
    }
    let connected = rows
        .iter()
        .flatten()
        .filter(|monitor| monitors[**monitor].connected)
        .count();
    if connected < 2 {
        return None;
    }
    let primary = monitors
        .iter()
        .position(|monitor| monitor.primary && monitor.connected)?;
    cell_of(rows, primary)
}

/// Gives each band the ghosts of `cell`: every window in another connected
/// monitor's column. A disconnected monitor's windows are where they were
/// saved, not on a monitor, so they get none.
fn add_ghosts(
    bands: &mut [Band],
    rows: &[Vec<usize>],
    monitors: &[ArrangeMonitor],
    cell: Option<(usize, usize)>,
) {
    for band in bands {
        let mut ghosts: Vec<usize> = match cell {
            None => Vec::new(),
            Some(home) => band
                .cells
                .iter()
                .enumerate()
                .flat_map(|(r, row)| row.iter().enumerate().map(move |(c, cell)| ((r, c), cell)))
                .filter(|(at, _)| *at != home && monitors[rows[at.0][at.1]].connected)
                .flat_map(|(_, cell)| cell.iter().copied())
                .collect(),
        };
        ghosts.sort_unstable();
        band.ghost_cell = cell;
        band.ghosts = ghosts;
    }
}

/// The height of a band whose rows have these column widths, with cards
/// `widths` wide. A desktop with none of the group's windows is its header
/// alone.
fn band_height(band: &Band, cols: &[Vec<Col>], tracks: &[Vec<f32>], widths: &[f32]) -> f32 {
    let header = 1.0 + BAND_TOP + BAND_HEADER;
    if band.count() == 0 {
        return header + BAND_TOP;
    }
    let mut height = header + HEADER_GAP + BAND_BOTTOM;
    for (r, (cols, tracks)) in cols.iter().zip(tracks).enumerate() {
        if r > 0 {
            height += ROW_GAP;
        }
        height += row_height(band, r, cols, tracks, widths);
    }
    height
}

/// A band's row `r` of monitor columns: the label, then the tallest column,
/// but tall enough for a rail to be a rail.
fn row_height(band: &Band, r: usize, cols: &[Col], tracks: &[f32], widths: &[f32]) -> f32 {
    let tallest = tracks
        .iter()
        .enumerate()
        .map(|(c, track)| cell_height(&flow_widths(&band.flow((r, c)), widths), *track))
        .fold(0.0, f32::max);
    let height = LABEL + LABEL_GAP + tallest;
    if cols.iter().any(|col| col.rail) {
        height.max(RAIL_MIN_HEIGHT)
    } else {
        height
    }
}

/// The bands on screen: only this desktop's until the other desktops are
/// shown.
pub fn visible(mut bands: Vec<Band>, shown: bool) -> Vec<Band> {
    if !shown {
        bands.truncate(1);
    }
    bands
}

/// The width at which every column holds its windows and ghosts on one line,
/// rails folded, or at least MIN_PANEL.
pub fn natural_width(cols: &[Vec<Col>]) -> f32 {
    cols.iter()
        .map(|row| {
            let columns: f32 = row
                .iter()
                .map(|col| if col.rail { RAIL } else { col.need })
                .sum();
            columns + COLUMN_GAP * row.len().saturating_sub(1) as f32 + 2.0 * SIDE
        })
        .fold(MIN_PANEL, f32::max)
}

/// The window size for the view: wide enough for one line of cards per
/// column, as tall as the bands then are, and at most `limit`, beyond which
/// it scrolls. `link` makes room for the line that opens the other desktops,
/// `hints` for the footer. `widths` is each window's card width.
pub fn panel_size(
    bands: &[Band],
    cols: &[Vec<Col>],
    widths: &[f32],
    link: bool,
    hints: bool,
    limit: Vec2,
) -> Vec2 {
    let width = (natural_width(cols) + SLACK).min(limit.x);
    let tracks: Vec<Vec<f32>> = cols.iter().map(|row| row_tracks(row, &[], width)).collect();
    let bands_height: f32 = bands
        .iter()
        .map(|band| band_height(band, cols, &tracks, widths))
        .sum();
    let link = if link { others::LINK_HEIGHT } else { 0.0 };
    let height = TOOLBAR + bands_height + link + footer_height(hints) + SLACK;
    vec2(width, height.min(limit.y))
}

/// Everything the view is laid out from.
struct Model {
    rows: Vec<Vec<usize>>,
    /// What each column's width comes from, shaped like `rows`.
    cols: Vec<Vec<Col>>,
    /// The bands on screen.
    bands: Vec<Band>,
    /// Every band, for a key that moves a window to a hidden desktop.
    all: Vec<Band>,
    /// Each window's card width.
    widths: Vec<f32>,
}

/// Which windows the strip shows and lists where: all of them or only the
/// `only` monitor's, and which windows each taskbar lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    pub only: Option<usize>,
    pub taskbars: MultiMonitor,
}

impl Model {
    fn new(
        group: &ArrangeGroup,
        snapshot: &ArrangeSnapshot,
        widths: &[f32],
        shown: bool,
        scope: Scope,
    ) -> Self {
        let Scope { only, taskbars } = scope;
        let monitors = &snapshot.monitors;
        let used = |index: usize| match only {
            Some(only) => index == only,
            None => {
                monitors[index].connected
                    || group
                        .windows
                        .iter()
                        .any(|window| column_of(window, monitors) == Some(index))
            }
        };
        let rows = monitor_rows(monitors, &used);
        let mut all = bands(group, &snapshot.desktops, monitors, &rows);
        let flags = visible_flags(group, monitors, only);
        for cell in all
            .iter_mut()
            .flat_map(|band| band.cells.iter_mut().flatten())
        {
            cell.retain(|member| flags[*member]);
        }
        add_ghosts(
            &mut all,
            &rows,
            monitors,
            ghost_cell(monitors, &rows, taskbars, only),
        );
        let mut bands = visible(all.clone(), shown);
        let cols = columns(&bands, &rows, monitors, widths);
        // A primary monitor with none of the group's windows folds into a
        // rail like any other, and its ghosts go with it.
        let folded = bands
            .first()
            .and_then(|band| band.ghost_cell)
            .is_some_and(|(r, c)| cols[r][c].rail);
        if folded {
            add_ghosts(&mut all, &rows, monitors, None);
            add_ghosts(&mut bands, &rows, monitors, None);
        }
        Self {
            cols,
            rows,
            bands,
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
    scope: Scope,
) -> Vec2 {
    let model = Model::new(group, snapshot, widths, shown, scope);
    panel_size(
        &model.bands,
        &model.cols,
        widths,
        link,
        snapshot.hints,
        limit,
    )
}

pub fn toolbar_rect(panel: Rect) -> Rect {
    Rect::from_min_size(
        panel.min + vec2(SIDE, 16.0),
        vec2(panel.width() - 2.0 * SIDE, 32.0),
    )
}

/// Every card, without the ghosts, band by band, row by row, column by
/// column.
pub fn walk(bands: &[Band]) -> Vec<usize> {
    bands
        .iter()
        .flat_map(|band| band.cells.iter().flatten().flatten().copied())
        .collect()
}

/// Every card and ghost in the order ← and → walk them: as `walk`, with each
/// cell's ghosts at their places in it.
pub fn walk_items(bands: &[Band]) -> Vec<Item> {
    bands
        .iter()
        .flat_map(|band| {
            band.cells
                .iter()
                .enumerate()
                .flat_map(move |(r, row)| (0..row.len()).flat_map(move |c| band.flow((r, c))))
        })
        .collect()
}

/// The card or ghost after `from` (or before it) in the order of `walk_items`.
pub fn next_item(bands: &[Band], from: Item, forward: bool) -> Option<Item> {
    let items = walk_items(bands);
    let at = items.iter().position(|item| *item == from)?;
    let next = if forward { at + 1 } else { at.checked_sub(1)? };
    items.get(next).copied()
}

/// Puts the keys on `item`: a card, or a ghost.
fn pick(shared: &mut Shared, group: &ArrangeGroup, item: Option<Item>) {
    shared.cursor = item.map(|item| group.windows[item.member()].hwnd);
    shared.cursor_ghost = matches!(item, Some(Item::Ghost(_)));
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
/// `spot`: right before the window at `spot.index` among the other cards
/// and ghosts of the cell, or right after the last of them. In a cell with
/// no other window it keeps its place. Every other window keeps its order.
pub fn order_after(count: usize, bands: &[Band], moved: usize, spot: Spot) -> Vec<usize> {
    let mut order: Vec<usize> = (0..count).filter(|member| *member != moved).collect();
    let others: Vec<usize> = bands[spot.band]
        .flow(spot.cell)
        .iter()
        .map(|item| item.member())
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

/// The place in its taskbar row (from 1) that `moved` takes at `spot` in
/// its own desktop's band: just before the card or ghost at `spot.index`
/// among the cell's others, or just after the last of them. None in a cell
/// with no other window.
fn row_place(
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    band: &Band,
    moved: usize,
    spot: Spot,
) -> Option<usize> {
    let others: Vec<usize> = band
        .flow(spot.cell)
        .iter()
        .map(|item| item.member())
        .filter(|member| *member != moved)
        .collect();
    let (neighbour, after) = match others.get(spot.index) {
        Some(next) => (*next, false),
        None => (*others.last()?, true),
    };
    let (own, _) = place_in_row(group, desktops, moved)?;
    let (place, _) = place_in_row(group, desktops, neighbour)?;
    // Without `moved` in the row, the neighbour moves up one place.
    let place = if own < place { place - 1 } else { place };
    Some(if after { place + 1 } else { place })
}

/// What putting `moved` at `spot` means: a move to the band's desktop when
/// it is another one, a move to the column's monitor when that is another
/// one, then the group's new order.
pub fn drop_actions(
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
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
    let now: Vec<isize> = group.windows.iter().map(|window| window.hwnd).collect();
    let order: Vec<isize> =
        if from.map(|from| from.band) == Some(spot.band) && to.ghost_cell == Some(spot.cell) {
            // Beside a ghost on its own desktop: the same number change the
            // menu makes, so the windows of other desktops keep their slots.
            row_place(group, desktops, to, moved, spot)
                .and_then(|place| order_with_place(group, desktops, moved, place))
                .unwrap_or_else(|| now.clone())
        } else {
            order_after(group.windows.len(), bands, moved, spot)
                .into_iter()
                .map(|member| group.windows[member].hwnd)
                .collect()
        };
    if order != now || !actions.is_empty() {
        actions.push(ArrangeAction::Reorder {
            group: group.key.clone(),
            order,
        });
    }
    actions
}

/// Whether a rail is unfolded: while any card is dragged, and while the
/// keyboard is on that rail.
fn rail_wanted(dragging: bool, focus: Option<usize>, monitor: usize) -> bool {
    dragging || focus == Some(monitor)
}

/// The monitor behind `cell` when its column is a rail.
fn rail_at(cols: &[Vec<Col>], rows: &[Vec<usize>], cell: (usize, usize)) -> Option<usize> {
    let col = cols.get(cell.0)?.get(cell.1)?;
    col.rail.then(|| rows[cell.0][cell.1])
}

/// A key the view answers through `step`. ↑ and ↓ pick by where the cards
/// are on screen, so the view answers them itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// ← and →: the card before or after, in the order of the cards.
    Prev,
    Next,
    /// Ctrl+← and Ctrl+→: one place within the column.
    MoveLeft,
    MoveRight,
    /// Ctrl+Shift+← and Ctrl+Shift+→: the next monitor's column.
    MonitorLeft,
    MonitorRight,
    /// Ctrl+Shift+↑ and Ctrl+Shift+↓: the band of the next desktop.
    DesktopUp,
    DesktopDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Select(Item),
    Move(Spot),
}

/// What a key does to the window `cursor`: ← and → pick the card before or
/// after it, across columns and bands; Ctrl+← and Ctrl+→ move it one place
/// within its column; Ctrl+Shift+← and Ctrl+Shift+→ move it to the end of
/// the next column of a connected monitor; Ctrl+Shift+↑ and Ctrl+Shift+↓
/// move it to the end of the same monitor's column in the band above or
/// below, or of the primary monitor's when its own is disconnected.
pub fn step(
    bands: &[Band],
    rows: &[Vec<usize>],
    monitors: &[ArrangeMonitor],
    cursor: usize,
    key: Key,
) -> Option<Step> {
    let here = locate(bands, cursor)?;
    let connected = |(r, c): (usize, usize)| monitors[rows[r][c]].connected;
    let to = |band: usize, cell: (usize, usize), card: usize| {
        Step::Move(Spot {
            band,
            cell,
            index: bands[band].flow_index(cell, cursor, card),
        })
    };
    let end =
        |band: usize, cell: (usize, usize)| to(band, cell, bands[band].cells[cell.0][cell.1].len());
    match key {
        Key::Prev | Key::Next => {
            next_item(bands, Item::Card(cursor), key == Key::Next).map(Step::Select)
        }
        Key::MoveLeft => here
            .index
            .checked_sub(1)
            .map(|index| to(here.band, here.cell, index)),
        Key::MoveRight => {
            let len = bands[here.band].cells[here.cell.0][here.cell.1].len();
            (here.index + 1 < len).then(|| to(here.band, here.cell, here.index + 1))
        }
        Key::MonitorLeft | Key::MonitorRight => {
            let cells: Vec<(usize, usize)> = rows
                .iter()
                .enumerate()
                .flat_map(|(r, row)| (0..row.len()).map(move |c| (r, c)))
                .collect();
            let at = cells.iter().position(|cell| *cell == here.cell)?;
            let next = if key == Key::MonitorRight {
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
        Key::DesktopUp | Key::DesktopDown => {
            let band = if key == Key::DesktopUp {
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
    /// Each window's place in Windows' taskbar row, indexed like the group.
    numbers: Vec<usize>,
    dragged: Option<usize>,
    /// The dragged window was picked up by its ghost.
    dragged_ghost: bool,
    target: Option<Spot>,
    /// How far each rail is unfolded, shaped like `rows`.
    open: Vec<Vec<f32>>,
    /// The monitor whose rail the keyboard is on.
    rail_focus: Option<usize>,
    zones: Vec<Zone>,
    /// Every card drawn, with its window handle, for a Ctrl drop.
    hits: Vec<(isize, Rect)>,
    /// Every card and ghost drawn, for ↑ and ↓.
    items: Vec<(Item, Rect)>,
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
    let visible = visible_flags(group, &snapshot.monitors, shared.only_monitor);
    let others_count = others::count(group, &snapshot.desktops, &visible);
    let shown = shared.others.shown(others_count);
    let widths = shared.widths(group, card_width);
    let scope = Scope {
        only: shared.only_monitor,
        taskbars: shared.taskbars,
    };
    let model = Model::new(group, snapshot, &widths, shown, scope);
    let mut frame = Frame::default();
    let items = walk_items(&model.bands);
    let member_of = |hwnd: isize| group.windows.iter().position(|w| w.hwnd == hwnd);
    let current = shared.cursor.and_then(member_of).map(|member| {
        if shared.cursor_ghost {
            Item::Ghost(member)
        } else {
            Item::Card(member)
        }
    });
    if current.is_none_or(|item| !items.contains(&item)) {
        let first = walk(&model.bands)
            .first()
            .map(|member| Item::Card(*member))
            .or(items.first().copied());
        pick(shared, group, first);
    }

    let dragged = shared.drag.as_ref().and_then(|drag| member_of(drag.hwnd));
    if dragged.is_none() {
        shared.drag = None;
    }
    let dragged_ghost = shared.drag.as_ref().is_some_and(|drag| drag.ghost);
    let home = |member: usize| {
        locate(&model.bands, member)
            .map(|spot| spot.cell)
            .filter(|(r, c)| snapshot.monitors[model.rows[*r][*c]].connected)
            .unwrap_or_else(|| primary_cell(&snapshot.monitors, &model.rows))
    };
    let ctrl = ctx.input(|i| i.modifiers.ctrl);
    let target = match (dragged, ctx.pointer_latest_pos()) {
        (Some(member), Some(pointer)) => drop_spot(pointer, &shared.zones, member, home(member)),
        _ => None,
    };
    // With Ctrl the drop only changes the number: the card under the
    // pointer says which, and the window stays on its monitor and desktop.
    let ctrl_card = match (dragged, ctx.pointer_latest_pos(), ctrl) {
        (Some(member), Some(pointer), true) => {
            ctrl_target(pointer, group.windows[member].hwnd, &shared.card_hits)
        }
        _ => None,
    };
    if let Some(member) = dragged {
        if !ctx.input(|i| i.pointer.primary_down()) {
            if ctrl {
                frame.actions.extend(
                    ctrl_card.and_then(|card| ctrl_drop(group, &snapshot.desktops, member, card)),
                );
            } else if let Some(spot) = target {
                let moved = if dragged_ghost {
                    Item::Ghost(member)
                } else {
                    Item::Card(member)
                };
                frame.actions.extend(drop_item_actions(
                    group,
                    &snapshot.desktops,
                    &model.bands,
                    &model.rows,
                    &snapshot.monitors,
                    moved,
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
        pos2(panel.max.x, panel.max.y - footer_height(snapshot.hints)),
    );
    frame.area = area;
    let rails: Vec<usize> = model
        .rows
        .iter()
        .zip(&model.cols)
        .flat_map(|(row, cols)| row.iter().zip(cols).filter(|(_, col)| col.rail))
        .map(|(monitor, _)| *monitor)
        .collect();
    if shared
        .rail_focus
        .is_some_and(|monitor| !rails.contains(&monitor))
    {
        shared.rail_focus = None;
    }
    let open: Vec<Vec<f32>> = model
        .rows
        .iter()
        .zip(&model.cols)
        .map(|(row, cols)| {
            row.iter()
                .zip(cols)
                .map(|(monitor, col)| {
                    if !col.rail {
                        return 0.0;
                    }
                    let device = snapshot.monitors[*monitor].device.as_str();
                    ctx.animate_bool_with_time_and_easing(
                        egui::Id::new(("arrange-rail", device)),
                        rail_wanted(dragging, shared.rail_focus, *monitor),
                        RAIL_ANIMATION,
                        egui::emath::easing::cubic_out,
                    )
                })
                .collect()
        })
        .collect();
    let mut draw = Draw {
        group,
        monitors: &snapshot.monitors,
        desktops: &snapshot.desktops,
        model: &model,
        menu_open,
        can_rename: snapshot.can_rename,
        numbers: taskbar_numbers(group, &snapshot.desktops),
        dragged: dragged.filter(|_| dragging),
        dragged_ghost,
        target: target.filter(|_| dragging && !ctrl),
        open,
        rail_focus: shared.rail_focus,
        zones: Vec::new(),
        hits: Vec::new(),
        items: Vec::new(),
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
    shared.card_hits = std::mem::take(&mut draw.hits);
    shared.items = std::mem::take(&mut draw.items);
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
        if ctrl {
            let ghost = Rect::from_min_size(
                ctx.pointer_latest_pos().unwrap_or_default() - drag.grab,
                vec2(model.widths[member], CARD_HEIGHT),
            );
            let place =
                ctrl_card.and_then(|card| ctrl_place(group, &snapshot.desktops, member, card));
            let outlined = ctrl_card
                .filter(|_| place.is_some())
                .and_then(|card| hit_rect(&shared.card_hits, card));
            let hint = ctrl_mark(&ctx, ghost, outlined, &ctrl_hint(place));
            frame.floating = Some(ghost.union(hint));
        }
    }

    if snapshot.hints {
        footer(ui, panel, shown && snapshot.keeps_order, others_count > 0);
    }

    if !menu_open && !dragging && shared.rename.is_none() {
        keyboard(
            &ctx,
            shared,
            group,
            &snapshot.desktops,
            &model,
            &snapshot.monitors,
            &mut frame,
        );
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
    layer.rect_filled(rect, tokens.radius_row, tokens.hover_bg);
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
            .cols
            .iter()
            .zip(&self.open)
            .map(|(cols, open)| row_tracks(cols, open, width))
            .collect();
        let (header, _) =
            ui.allocate_exact_size(vec2(width, 1.0 + BAND_TOP + BAND_HEADER), Sense::hover());
        let mut painter = ui.painter().clone();
        painter.multiply_opacity(alpha);
        painter.hline(
            header.x_range(),
            header.top(),
            theme::line(ui.ctx(), tokens.border),
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
        for (r, row_tracks) in tracks.iter().enumerate() {
            if r > 0 {
                ui.add_space(ROW_GAP);
            }
            let height = row_height(band, r, &self.model.cols[r], row_tracks, &self.model.widths);
            let needed = 2.0 * SIDE
                + row_tracks.iter().sum::<f32>()
                + COLUMN_GAP * row_tracks.len().saturating_sub(1) as f32;
            let at = |rect: Rect| RowAt {
                band_index: b,
                row: r,
                band,
                rect,
                tracks: row_tracks,
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
            let flow = band.flow((r, c));
            let mut painter = ui.painter().clone();
            painter.multiply_opacity(alpha);
            if self.model.cols[r][c].rail {
                let lit = self
                    .target
                    .is_some_and(|spot| spot.band == b && spot.cell == (r, c))
                    || self.rail_focus == Some(self.model.rows[r][c]);
                rail(ui, &painter, column, monitor, self.open[r][c], lit);
                self.zones.push(Zone {
                    band: b,
                    cell: Some((r, c)),
                    rect: column,
                    open: true,
                    cards: Vec::new(),
                });
                continue;
            }
            painter.vline(
                column.left(),
                column.y_range(),
                theme::line(ui.ctx(), tokens.border),
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
            let widths = flow_widths(&flow, &self.model.widths);
            let mut placed: Vec<(Item, Rect)> = Vec::new();
            for (line, items) in cell_lines(&widths, *track).into_iter().enumerate() {
                let mut x = inner.left();
                for at in items {
                    let min = pos2(x, top + line as f32 * (CARD_HEIGHT + CARD_GAP));
                    x += widths[at] + CARD_GAP;
                    placed.push((
                        flow[at],
                        Rect::from_min_size(min, vec2(widths[at], CARD_HEIGHT)),
                    ));
                }
            }
            let cards: Vec<(usize, Rect)> = placed
                .iter()
                .filter_map(|(item, rect)| match item {
                    Item::Card(member) => Some((*member, *rect)),
                    Item::Ghost(_) => None,
                })
                .collect();
            for (member, card_rect) in &cards {
                self.hits
                    .push((self.group.windows[*member].hwnd, *card_rect));
            }
            self.items.extend(placed.iter().copied());
            for (member, card_rect) in cards.clone() {
                let mut card_alpha = alpha;
                if !monitor.connected {
                    card_alpha *= DISCONNECTED;
                }
                if self.dragged == Some(member) && !self.dragged_ghost {
                    card_alpha *= DRAGGED;
                }
                self.card(ui, shared, frame, member, card_rect, card_alpha);
            }
            for (item, ghost_rect) in &placed {
                if let Item::Ghost(member) = item {
                    self.ghost_card(ui, shared, frame, *member, *ghost_rect, alpha);
                }
            }
            let target_here = self
                .target
                .filter(|spot| spot.band == b && spot.cell == (r, c));
            let others: Vec<Rect> = placed
                .iter()
                .filter(|(item, _)| Some(item.member()) != self.dragged)
                .map(|(_, rect)| *rect)
                .collect();
            if flow.is_empty() && monitor.connected {
                let empty =
                    Rect::from_min_size(pos2(inner.left(), top), vec2(inner.width(), EMPTY));
                drop_box(
                    ui,
                    &painter,
                    empty,
                    target_here.is_some(),
                    "Drop a window here to move it to this monitor",
                );
            }
            if let Some(spot) = target_here {
                insertion_bar(ui, &others, spot.index, pos2(inner.left(), top));
            }
            self.zones.push(Zone {
                band: b,
                cell: Some((r, c)),
                rect: column,
                open: monitor.connected,
                cards: placed
                    .iter()
                    .map(|(item, rect)| (item.member(), *rect))
                    .collect(),
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
        let leaving = shared.is_leaving(window.hwnd);
        let (scale, fade) = shared.motion(window.hwnd);
        let alpha = alpha * fade;
        let response = ui.interact(
            rect,
            egui::Id::new(("arrange-card", window.hwnd)),
            item_sense(Item::Card(member), leaving),
        );
        let mut painter = ui.painter().clone();
        painter.multiply_opacity(alpha);
        let is_cursor = shared.cursor == Some(window.hwnd) && !shared.cursor_ghost && !leaving;
        let lit = if is_cursor {
            Some(tokens.selection_bg)
        } else if response.hovered() && shared.pointer.moved() && self.dragged.is_none() {
            Some(tokens.hover_bg)
        } else {
            None
        };
        paint_look(
            &painter,
            grown(rect, scale),
            4,
            card_look(&tokens, lit, window.attention),
        );
        if is_cursor {
            // Outside the picture, which DWM would draw over.
            painter.rect_stroke(
                rect.expand(3.0),
                tokens.radius_row + 2,
                theme::stroke(ui.ctx(), 2.0, tokens.accent),
                StrokeKind::Middle,
            );
        }
        let thumb = grown(
            Rect::from_min_size(
                rect.min + Vec2::splat(CARD_PAD),
                vec2(rect.width() - 2.0 * CARD_PAD, THUMB_HEIGHT),
            ),
            scale,
        );
        painter.rect(
            thumb,
            3,
            tokens.panel_bg,
            theme::line(ui.ctx(), tokens.border),
            StrokeKind::Inside,
        );
        let picture = thumb.shrink(1.0);
        let mut badges = Vec::new();
        if window.same_title && !leaving {
            badges.push(Badge::SameTitle);
        }
        if moved(window, self.monitors) {
            badges.push(Badge::Moved);
        }
        let shown = paint_badges(ui, &painter, response.id, thumb, &badges, self.can_rename);
        if shown.renamed {
            frame.actions.push(ArrangeAction::AskName(window.hwnd));
        }
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
                clear: shown.clear,
                ghost: false,
            });
        } else {
            name_card(&painter, picture, &window.label, "");
        }

        let title_top = thumb.bottom() + TITLE_GAP;
        let right = rect.right() - CARD_PAD - 2.0;
        let number = number_chip(
            &painter,
            pos2(rect.left() + CARD_PAD + 2.0, title_top),
            TITLE,
            self.numbers[member],
        );
        let left = number.right() + NUMBER_GAP;
        // F2: the title is a text box, which owns the mouse on this card.
        if let Some(edit) = shared
            .rename
            .as_mut()
            .filter(|edit| edit.hwnd == window.hwnd && !edit.ghost)
        {
            let field = Rect::from_min_max(pos2(left, title_top), pos2(right, title_top + TITLE));
            edit.show(ui, field, 13.0);
            frame.cards.push((window.hwnd, window.monitor.is_some()));
            return;
        }
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
        if leaving {
            return;
        }
        if self.dragged.is_none() {
            response = response.on_hover_cursor(egui::CursorIcon::Grab);
        }
        self.interact(shared, frame, member, &response);
    }

    /// A stand-in in the primary column for a window on another monitor, at
    /// its taskbar place: a narrow name strip (a pill with an arrow toward
    /// that monitor and its first letter, the title written upward, the
    /// number), then the window's faded picture filling the rest. It cannot
    /// be dragged; a click picks the window's own card.
    fn ghost_card(
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
        let Some(monitor) = column_of(window, self.monitors).and_then(|m| self.monitors.get(m))
        else {
            return;
        };
        let left = self
            .monitors
            .iter()
            .find(|primary| primary.primary && primary.connected)
            .is_some_and(|primary| points_left(monitor, primary));
        let response = ui.interact(
            rect,
            egui::Id::new(("arrange-ghost", window.hwnd)),
            item_sense(Item::Ghost(member), false),
        );
        let mut painter = ui.painter().clone();
        // Its old place stays behind, faded, while it is dragged.
        let alpha = if self.dragged == Some(member) && self.dragged_ghost {
            alpha * DRAGGED
        } else {
            alpha
        };
        painter.multiply_opacity(alpha);
        ghost_box(ui, &painter, rect, window.attention);
        if shared.cursor == Some(window.hwnd) && shared.cursor_ghost {
            // Outside the picture, which DWM would draw over.
            painter.rect_stroke(
                rect.expand(3.0),
                6,
                theme::stroke(ui.ctx(), 2.0, tokens.accent),
                StrokeKind::Middle,
            );
        }

        let strip = Rect::from_min_size(rect.min, vec2(GHOST_STRIP, rect.height()));
        let x = strip.right() - 0.5;
        painter.extend(Shape::dashed_line(
            &[pos2(x, rect.top()), pos2(x, rect.bottom())],
            theme::line(ui.ctx(), tokens.border),
            4.0,
            3.0,
        ));

        let letter = painter.layout_no_wrap(
            monitor_letter(&monitor.label),
            theme::semibold(12.0),
            tokens.text_disabled,
        );
        let pill = Rect::from_min_size(
            pos2(strip.center().x - GHOST_PILL / 2.0, rect.top() + GHOST_TOP),
            vec2(
                GHOST_PILL,
                GHOST_PILL_PAD + GHOST_ARROW + letter.size().y + GHOST_PILL_PAD,
            ),
        );
        painter.rect_stroke(
            pill,
            7,
            theme::line(ui.ctx(), tokens.text_disabled),
            StrokeKind::Inside,
        );
        let arrow = Rect::from_min_size(
            pos2(
                pill.center().x - GHOST_ARROW / 2.0,
                pill.top() + GHOST_PILL_PAD,
            ),
            Vec2::splat(GHOST_ARROW),
        );
        let icon = if left {
            Icon::ArrowLeft
        } else {
            Icon::ArrowRight
        };
        icons::paint(&painter, arrow, icon, tokens.text_disabled);
        let letter_at = pos2(pill.center().x - letter.size().x / 2.0, arrow.bottom());
        painter.galley(letter_at, letter, tokens.text_disabled);

        let chip = ghost_chip(
            ui,
            &painter,
            strip.center().x,
            rect.bottom() - GHOST_BOTTOM,
            self.numbers[member],
        );
        let free = (chip.top() - GHOST_GAP) - (pill.bottom() + GHOST_GAP);
        if free > 0.0 {
            let mut job = text::job(
                &window.label,
                theme::regular(12.0),
                tokens.text_secondary,
                None,
            );
            job.wrap = TextWrapping {
                max_width: free,
                max_rows: 1,
                break_anywhere: true,
                overflow_character: Some('\u{2026}'),
            };
            let title = painter.layout_job(job);
            // Turned a quarter anticlockwise, it reads upward from its pivot,
            // its bottom left corner.
            let at = pos2(
                strip.center().x - title.size().y / 2.0,
                chip.top() - GHOST_GAP,
            );
            painter.add(
                egui::epaint::TextShape::new(at, title, tokens.text_secondary)
                    .with_angle(-std::f32::consts::FRAC_PI_2),
            );
        }

        let picture_box = Rect::from_min_max(pos2(strip.right(), rect.top()), rect.max).shrink(1.0);
        let editing = shared
            .rename
            .as_ref()
            .is_some_and(|edit| edit.hwnd == window.hwnd && edit.ghost);
        // The picture would be drawn over the text box.
        let has_picture = !self.menu_open
            && !editing
            && ghost_thumbnail(shared, window.hwnd)
                .is_some_and(|thumb| thumb.source_size().is_some());
        if has_picture {
            let source = shared.sources.get(&window.hwnd).map(|source| source.size());
            frame.pending.push(Pending {
                hwnd: window.hwnd,
                dest: source.map_or(picture_box, |source| cover(source, picture_box)),
                clip: picture_box.intersect(ui.clip_rect()),
                opacity: (255.0 * GHOST_OPACITY * alpha).round() as u8,
                clear: Rect::NOTHING,
                ghost: true,
            });
        } else {
            let mut faded = painter.clone();
            faded.multiply_opacity(GHOST_OPACITY);
            name_card(&faded, picture_box, &window.label, "");
        }

        // F2: the title is a text box, over the picture's place.
        if let Some(edit) = shared.rename.as_mut().filter(|_| editing) {
            let field = Rect::from_center_size(
                rect.center(),
                vec2((rect.width() - 8.0).max(0.0), TITLE + 4.0),
            );
            edit.show(ui, field, 13.0);
            return;
        }

        let response =
            response.on_hover_text(format!("{} \u{b7} on {}", window.label, monitor.label));
        let response = if self.dragged.is_none() {
            response.on_hover_cursor(egui::CursorIcon::Grab)
        } else {
            response
        };
        if response.hovered() {
            frame.hovered_ghost = Some(window.hwnd);
        }
        // A click opens the window, as on its real card.
        if response.clicked() {
            pick(shared, self.group, Some(Item::Ghost(member)));
            shared.rail_focus = None;
            frame.activate = Some(window.hwnd);
        }
        if response.middle_clicked() {
            frame.close.extend(middle_close(window.hwnd, true));
        }
        if response.drag_started() {
            if let Some(at) = response.interact_pointer_pos() {
                shared.drag = Some(Drag {
                    hwnd: window.hwnd,
                    grab: at - response.rect.min,
                    ghost: true,
                });
                pick(shared, self.group, Some(Item::Ghost(member)));
            }
        }
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
        if response.middle_clicked() {
            frame.close.extend(middle_close(window.hwnd, false));
        }
        if response.drag_started() {
            if let Some(at) = response.interact_pointer_pos() {
                shared.drag = Some(Drag {
                    hwnd: window.hwnd,
                    grab: at - response.rect.min,
                    ghost: false,
                });
                pick(shared, self.group, Some(Item::Card(member)));
            }
        }
        let names = self.can_rename.then_some(window.renamed);
        response.context_menu(|ui| {
            if let Some(action) = card_menu(ui, self.group, member, self.desktops, names) {
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
            theme::regular(12.0),
            tokens.text_disabled,
        );
        let chip_rect = Rect::from_min_size(
            pos2(x, centre - LABEL / 2.0),
            vec2(chip.size().x + 12.0, LABEL),
        );
        painter.rect_stroke(
            chip_rect,
            tokens.radius_row,
            theme::line(ui.ctx(), tokens.border),
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

/// Whether a ghost's arrow points left: its monitor stands left of the
/// primary one.
fn points_left(monitor: &ArrangeMonitor, primary: &ArrangeMonitor) -> bool {
    monitor.rect[0] + monitor.rect[2] < primary.rect[0] + primary.rect[2]
}

/// A picture of `source`'s shape that covers `area`, centred on it: the
/// parts that stick out are cut off.
fn cover(source: Vec2, area: Rect) -> Rect {
    if source.x <= 0.0 || source.y <= 0.0 {
        return area;
    }
    let scale = (area.width() / source.x).max(area.height() / source.y);
    Rect::from_center_size(area.center(), source * scale)
}

/// The letter a ghost's pill shows for its monitor: the first of its name.
fn monitor_letter(label: &str) -> String {
    label
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default()
}

/// A ghost's taskbar number in a dashed chip, centred on `centre_x` and
/// ending at `bottom`; returns its rectangle.
fn ghost_chip(ui: &Ui, painter: &egui::Painter, centre_x: f32, bottom: f32, number: usize) -> Rect {
    let tokens = Tokens::get(ui.ctx());
    let galley = painter.layout_no_wrap(
        number.to_string(),
        theme::regular(12.0),
        tokens.text_disabled,
    );
    let width = (galley.size().x + 4.0).max(GHOST_CHIP);
    let chip = Rect::from_min_max(
        pos2(centre_x - width / 2.0, bottom - GHOST_CHIP),
        pos2(centre_x + width / 2.0, bottom),
    );
    dashed_box(ui, painter, chip, false);
    painter.galley(
        chip.center() - galley.size() / 2.0,
        galley,
        tokens.text_disabled,
    );
    chip
}

/// A ghost's dashed outline. A window that wants attention gets the same
/// orange tint as its real card, and the dashes turn orange, so the mark
/// shows in the primary column too.
fn ghost_box(ui: &Ui, painter: &egui::Painter, rect: Rect, attention: bool) {
    let (fill, outline) = ghost_colours(&Tokens::get(ui.ctx()), attention);
    if let Some(fill) = fill {
        painter.rect_filled(rect, 4, fill);
    }
    dashed_outline(ui, painter, rect, outline);
}

/// A ghost's fill and dash colour.
fn ghost_colours(tokens: &Tokens, attention: bool) -> (Option<Color32>, Color32) {
    let outline = if attention {
        tokens.warning
    } else {
        tokens.border
    };
    (card_look(tokens, None, attention).fill, outline)
}

/// A 1 px dashed outline, in the accent while a card is about to land in it.
fn dashed_box(ui: &Ui, painter: &egui::Painter, rect: Rect, lit: bool) {
    let tokens = Tokens::get(ui.ctx());
    let colour = if lit { tokens.accent } else { tokens.border };
    dashed_outline(ui, painter, rect, colour);
}

fn dashed_outline(ui: &Ui, painter: &egui::Painter, rect: Rect, colour: Color32) {
    let stroke = theme::line(ui.ctx(), colour);
    let r = rect.shrink(0.5);
    let corners = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    painter.extend(Shape::dashed_line(&corners, stroke, 4.0, 3.0));
}

/// A monitor with no windows. Folded, it is a narrow dashed strip with the
/// monitor's icon and its name, written upward. While `open` it is a
/// column wide, with the same dashed box to drop a window into.
fn rail(
    ui: &Ui,
    painter: &egui::Painter,
    rect: Rect,
    monitor: &ArrangeMonitor,
    open: f32,
    lit: bool,
) {
    let tokens = Tokens::get(ui.ctx());
    dashed_box(ui, painter, rect, lit);
    if open < 1.0 {
        let mut folded = painter.with_clip_rect(rect);
        folded.multiply_opacity(1.0 - open);
        let icon_top = rect.top() + RAIL_PAD;
        icons::paint(
            &folded,
            Rect::from_min_size(
                pos2(rect.center().x - RAIL_ICON / 2.0, icon_top),
                Vec2::splat(RAIL_ICON),
            ),
            Icon::Monitor,
            tokens.text_disabled,
        );
        let name = folded.layout_no_wrap(
            monitor.label.clone(),
            theme::semibold(12.0),
            tokens.text_secondary,
        );
        let empty = folded.layout_no_wrap(
            "empty".to_string(),
            theme::regular(12.0),
            tokens.text_disabled,
        );
        let top = icon_top + RAIL_ICON + RAIL_PAD;
        let total = name.size().x + RAIL_TEXT_GAP + empty.size().x;
        // Turned a quarter anticlockwise, the text starts at its pivot and
        // reads upward: the pivot is its bottom left corner.
        for (galley, bottom, colour) in [
            (name, top + total, tokens.text_secondary),
            (empty.clone(), top + empty.size().x, tokens.text_disabled),
        ] {
            let at = pos2(rect.center().x - galley.size().y / 2.0, bottom);
            folded.add(
                egui::epaint::TextShape::new(at, galley, colour)
                    .with_angle(-std::f32::consts::FRAC_PI_2),
            );
        }
    }
    if open > 0.0 {
        let mut unfolded = painter.with_clip_rect(rect);
        unfolded.multiply_opacity(open);
        let icon = Rect::from_min_size(
            pos2(rect.left() + RAIL_PAD, rect.top() + RAIL_PAD),
            Vec2::splat(RAIL_ICON),
        );
        icons::paint(&unfolded, icon, Icon::Monitor, tokens.text_disabled);
        let name = unfolded.layout_no_wrap(
            monitor.label.clone(),
            theme::semibold(12.0),
            tokens.text_secondary,
        );
        unfolded.galley(
            pos2(icon.right() + 6.0, icon.center().y - name.size().y / 2.0),
            name,
            tokens.text_secondary,
        );
        let below = Rect::from_min_max(pos2(rect.left(), icon.bottom() + RAIL_PAD), rect.max);
        let mut job = text::job(
            "Drop a window here",
            theme::regular(12.0),
            tokens.text_disabled,
            None,
        );
        job.wrap = TextWrapping {
            max_width: (rect.width() - 24.0).max(0.0),
            max_rows: 2,
            break_anywhere: false,
            overflow_character: Some('\u{2026}'),
        };
        job.halign = egui::Align::Center;
        let galley = unfolded.layout_job(job);
        unfolded.galley(
            pos2(below.center().x, below.center().y - galley.size().y / 2.0),
            galley,
            tokens.text_disabled,
        );
    }
}

/// The dashed box of a column with no windows, outlined in the accent while
/// a card is about to land in it.
fn drop_box(ui: &Ui, painter: &egui::Painter, rect: Rect, target: bool, label: &str) {
    let tokens = Tokens::get(ui.ctx());
    dashed_box(ui, painter, rect, target);
    let mut job = text::job(label, theme::regular(12.0), tokens.text_disabled, None);
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

/// The last footer hint. The toolbar shows the same key while the footer is
/// hidden, as "Hints".
pub const HIDE_HINT: (&str, &str) = ("Ctrl + H", "Hide hints");

/// The key hints under the bands, most needed first: when the strip is too
/// narrow they go from the end. The D hint needs other desktops to show.
fn footer_hints(others_key: bool) -> Vec<(&'static str, &'static str)> {
    let mut hints = vec![
        ("\u{2190}\u{2192}\u{2191}\u{2193}", "Select"),
        ("Enter", "Open"),
        ("Ctrl + \u{2190}\u{2192}", "Move"),
        ("Ctrl + Shift + \u{2190}\u{2192}", "Next monitor"),
        ("Ctrl + Shift + \u{2191}\u{2193}", "Next desktop"),
        ("Alt + \u{2190}\u{2192}", "Change number"),
        ("1\u{2013}9", "Open by number"),
        ("Ctrl + 1\u{2013}9", "Taskbar button"),
        ("Ctrl + W", "Close"),
        ("Ctrl + N", "New window"),
        ("F2", "Rename"),
    ];
    if others_key {
        hints.push(("D", "Other desktops"));
    }
    hints.push(HIDE_HINT);
    hints
}

/// How many hints, from the first, fit in `room` when they are `widths`
/// wide and `gap` apart. "Select" and "Move" (the first and third) stay even
/// when they do not fit, so at least `keep` are shown.
fn hints_that_fit(widths: &[f32], gap: f32, room: f32, keep: usize) -> usize {
    let mut used = 0.0;
    let mut count = 0;
    for (index, width) in widths.iter().enumerate() {
        let wanted = used + width + if index > 0 { gap } else { 0.0 };
        if wanted > room {
            break;
        }
        used = wanted;
        count += 1;
    }
    count.max(keep).min(widths.len())
}

/// A key and what it does, laid out.
type Hint = (std::sync::Arc<egui::Galley>, std::sync::Arc<egui::Galley>);

fn hint_galleys(painter: &egui::Painter, key: &str, label: &str, color: Color32) -> Hint {
    let label = painter.layout_no_wrap(label.to_string(), theme::regular(12.0), color);
    let font = if key.is_ascii() {
        theme::regular(12.0)
    } else {
        theme::mono(12.0)
    };
    (painter.layout_no_wrap(key.to_string(), font, color), label)
}

/// The key chip (12 px of padding) and the label after it, 6 px apart.
fn hint_width((key, label): &Hint) -> f32 {
    key.size().x + 12.0 + 6.0 + label.size().x
}

/// Draws the hint with its right edge at `right`, centred on `centre`.
/// Returns its left edge.
fn paint_hint(
    painter: &egui::Painter,
    tokens: &Tokens,
    (key, label): Hint,
    right: f32,
    centre: f32,
    color: Color32,
) -> f32 {
    let mut x = right - label.size().x;
    painter.galley(pos2(x, centre - label.size().y / 2.0), label, color);
    x -= 6.0;
    let chip = Rect::from_min_max(
        pos2(x - key.size().x - 12.0, centre - key.size().y / 2.0 - 1.0),
        pos2(x, centre + key.size().y / 2.0 + 1.0),
    );
    painter.rect(
        chip,
        tokens.radius_row,
        tokens.panel_bg,
        theme::line(painter.ctx(), tokens.border),
        StrokeKind::Inside,
    );
    painter.galley(chip.center() - key.size() / 2.0, key, color);
    chip.left()
}

/// The small hint that ends the toolbar while the footer is hidden: the key
/// that brings it back. Returns its width.
pub fn paint_toolbar_hint(
    painter: &egui::Painter,
    tokens: &Tokens,
    right: f32,
    centre: f32,
) -> f32 {
    let (key, label) = (HIDE_HINT.0, "Hints");
    let hint = hint_galleys(painter, key, label, tokens.text_disabled);
    let width = hint_width(&hint);
    paint_hint(painter, tokens, hint, right, centre, tokens.text_disabled);
    width
}

/// The note on order and the key hints under the bands. The note is only
/// true while other desktops are on screen and a plugin saves the order;
/// the D hint needs some to show.
fn footer(ui: &Ui, panel: Rect, note: bool, others_key: bool) {
    let tokens = Tokens::get(ui.ctx());
    let painter = ui.painter();
    let area = Rect::from_min_max(pos2(panel.min.x, panel.max.y - FOOTER), panel.max);
    painter.hline(
        area.x_range(),
        area.top(),
        theme::line(ui.ctx(), tokens.border),
    );
    let centre = area.top() + 1.0 + (FOOTER - 1.0) / 2.0;
    let mut hints: Vec<Hint> = footer_hints(others_key)
        .into_iter()
        .map(|(key, label)| hint_galleys(painter, key, label, tokens.text_secondary))
        .collect();
    // The hint that hides the hints always stays: it is the way back.
    let hide = hints.pop();
    let reserved = hide.as_ref().map_or(0.0, |hint| hint_width(hint) + 16.0);
    let widths: Vec<f32> = hints.iter().map(hint_width).collect();
    let shown = hints_that_fit(&widths, 16.0, area.width() - 2.0 * SIDE - reserved, 3);
    hints.truncate(shown);
    hints.extend(hide);
    let mut x = area.right() - SIDE;
    for (index, hint) in hints.into_iter().enumerate().rev() {
        x = paint_hint(painter, &tokens, hint, x, centre, tokens.text_secondary);
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

/// How a card or a ghost takes the mouse: both can be clicked and dragged,
/// except a card whose window is gone and fading out.
pub fn item_sense(item: Item, leaving: bool) -> Sense {
    match item {
        Item::Card(_) if leaving => Sense::hover(),
        Item::Card(_) | Item::Ghost(_) => Sense::click_and_drag(),
    }
}

/// Where Ctrl+← (`forward` false) or Ctrl+→ puts window `member`'s ghost:
/// one place along the primary column's cards and ghosts. None at either
/// end, or when the window has no ghost.
pub fn ghost_step(bands: &[Band], member: usize, forward: bool) -> Option<Spot> {
    let band = bands
        .iter()
        .position(|band| band.ghosts.contains(&member))?;
    let cell = bands[band].ghost_cell?;
    let flow = bands[band].flow(cell);
    let at = flow.iter().position(|item| *item == Item::Ghost(member))?;
    let others = flow.len() - 1;
    let index = if forward {
        Some(at + 1).filter(|index| *index <= others)?
    } else {
        at.checked_sub(1)?
    };
    Some(Spot { band, cell, index })
}

/// What putting `moved`, a card or a ghost, at `spot` means. A ghost's
/// window is on another monitor. Put among the primary column's cards and
/// ghosts on its own desktop, it only takes that taskbar place and stays on
/// its monitor; put anywhere else, it goes where its own card would.
pub fn drop_item_actions(
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    bands: &[Band],
    rows: &[Vec<usize>],
    monitors: &[ArrangeMonitor],
    moved: Item,
    spot: Spot,
) -> Vec<ArrangeAction> {
    let member = moved.member();
    let mut actions = drop_actions(group, desktops, bands, rows, monitors, member, spot);
    let own_band = locate(bands, member).map(|from| from.band) == Some(spot.band);
    if let Item::Ghost(_) = moved {
        if own_band && bands[spot.band].ghost_cell == Some(spot.cell) {
            let now: Vec<isize> = group.windows.iter().map(|window| window.hwnd).collect();
            actions.retain(|action| match action {
                ArrangeAction::MoveToMonitor { .. } => false,
                ArrangeAction::Reorder { order, .. } => *order != now,
                _ => true,
            });
        }
    }
    actions
}

/// The keys that move a window or change its number, for the compact popup,
/// which picks and opens windows itself and leaves these to the rules here.
pub(super) fn move_keys(
    ctx: &egui::Context,
    shared: &mut Shared,
    group: &ArrangeGroup,
    snapshot: &ArrangeSnapshot,
    frame: &mut Frame,
) {
    let visible = visible_flags(group, &snapshot.monitors, shared.only_monitor);
    let shown = shared
        .others
        .shown(others::count(group, &snapshot.desktops, &visible));
    let widths = shared.widths(group, card_width);
    let scope = Scope {
        only: shared.only_monitor,
        taskbars: shared.taskbars,
    };
    let model = Model::new(group, snapshot, &widths, shown, scope);
    shared.cursor_ghost = false;
    keyboard(
        ctx,
        shared,
        group,
        &snapshot.desktops,
        &model,
        &snapshot.monitors,
        frame,
    );
}

/// The arrow keys `step` answers, the most modifiers first: a pattern with
/// fewer modifiers also matches when more are held. Plain arrows only pick;
/// moving a window needs Ctrl.
fn arrow_keys() -> [(egui::Modifiers, egui::Key, Key); 8] {
    let ctrl_shift = egui::Modifiers::CTRL | egui::Modifiers::SHIFT;
    [
        (ctrl_shift, egui::Key::ArrowUp, Key::DesktopUp),
        (ctrl_shift, egui::Key::ArrowDown, Key::DesktopDown),
        (ctrl_shift, egui::Key::ArrowLeft, Key::MonitorLeft),
        (ctrl_shift, egui::Key::ArrowRight, Key::MonitorRight),
        (egui::Modifiers::CTRL, egui::Key::ArrowLeft, Key::MoveLeft),
        (egui::Modifiers::CTRL, egui::Key::ArrowRight, Key::MoveRight),
        (egui::Modifiers::NONE, egui::Key::ArrowLeft, Key::Prev),
        (egui::Modifiers::NONE, egui::Key::ArrowRight, Key::Next),
    ]
}

/// The arrow keys pick a window, ghosts too: ← and → in the order of the
/// cards, ↑ and ↓ on the line above or below. Enter on a ghost switches to
/// its window; nothing else acts on a ghost. Ctrl with ← or → moves a card
/// within its column,
/// Ctrl+Shift with the arrows to the next monitor or desktop, Alt with ← or
/// → gives it the next lower or higher number without moving it anywhere
/// else, Enter switches to it. Esc is handled with the other ways to close.
fn keyboard(
    ctx: &egui::Context,
    shared: &mut Shared,
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
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
    let keys = arrow_keys();
    let (alt_left, alt_right, pressed, up, down, enter) = ctx.input_mut(|i| {
        // Alt first: a key pattern without modifiers also matches with Alt
        // down.
        let alt_left = i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowLeft);
        let alt_right = i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowRight);
        let pressed = keys
            .iter()
            .find(|(modifiers, key, _)| i.consume_key(*modifiers, *key))
            .map(|(_, _, key)| *key);
        (
            alt_left,
            alt_right,
            pressed,
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
        )
    });
    let item = if shared.cursor_ghost {
        Item::Ghost(cursor)
    } else {
        Item::Card(cursor)
    };
    if up || down {
        shared.rail_focus = None;
        if let Some(next) = card_above_or_below(&shared.items, item, down) {
            pick(shared, group, Some(next));
        }
        return;
    }
    if enter {
        frame.activate = Some(group.windows[cursor].hwnd);
        return;
    }
    if alt_left || alt_right {
        let step = if alt_left { -1 } else { 1 };
        frame.actions.extend(nudge(group, desktops, cursor, step));
        return;
    }
    // A ghost's window is on another monitor: Ctrl+← and Ctrl+→ give it
    // another taskbar place and leave it there; the keys to another monitor
    // or desktop move it as its own card does.
    if let Item::Ghost(_) = item {
        match pressed {
            Some(key @ (Key::Prev | Key::Next)) => {
                if let Some(next) = next_item(&model.bands, item, key == Key::Next) {
                    pick(shared, group, Some(next));
                }
                return;
            }
            Some(key @ (Key::MoveLeft | Key::MoveRight)) => {
                if let Some(spot) = ghost_step(&model.bands, cursor, key == Key::MoveRight) {
                    frame.actions.extend(drop_item_actions(
                        group,
                        desktops,
                        &model.bands,
                        &model.rows,
                        monitors,
                        item,
                        spot,
                    ));
                }
                return;
            }
            Some(_) => pick(shared, group, Some(Item::Card(cursor))),
            None => return,
        }
    }
    // A move to another desktop looks through all of them, so it also
    // works while they are hidden.
    let bands = if matches!(pressed, Some(Key::DesktopUp | Key::DesktopDown)) {
        &model.all
    } else {
        &model.bands
    };
    if pressed.is_some() {
        shared.rail_focus = None;
    }
    match pressed.and_then(|key| step(bands, &model.rows, monitors, cursor, key)) {
        Some(Step::Select(next)) => pick(shared, group, Some(next)),
        Some(Step::Move(spot)) => {
            if spot.band >= model.bands.len() {
                shared.others.expand();
            }
            shared.rail_focus = rail_at(&model.cols, &model.rows, spot.cell);
            frame.actions.extend(drop_actions(
                group,
                desktops,
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
mod tests;
