use super::*;

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
pub(super) fn primary_cell(monitors: &[ArrangeMonitor], rows: &[Vec<usize>]) -> (usize, usize) {
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
pub(super) fn add_ghosts(
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
pub(super) fn band_height(
    band: &Band,
    cols: &[Vec<Col>],
    tracks: &[Vec<f32>],
    widths: &[f32],
) -> f32 {
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
pub(super) fn row_height(
    band: &Band,
    r: usize,
    cols: &[Col],
    tracks: &[f32],
    widths: &[f32],
) -> f32 {
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
    // A drag opens every rail, which takes width from the other columns and
    // can wrap their cards onto one more line; the panel is tall enough for
    // both, or that line was cut off while dragging.
    let height_with = |open: f32| -> f32 {
        let tracks: Vec<Vec<f32>> = cols
            .iter()
            .map(|row| row_tracks(row, &vec![open; row.len()], width))
            .collect();
        bands
            .iter()
            .map(|band| band_height(band, cols, &tracks, widths))
            .sum()
    };
    let bands_height = height_with(0.0).max(height_with(1.0));
    let link = if link { others::LINK_HEIGHT } else { 0.0 };
    let height = TOOLBAR + bands_height + link + footer_height(hints) + SLACK;
    vec2(width, height.min(limit.y))
}

/// Everything the view is laid out from.
pub(super) struct Model {
    pub(super) rows: Vec<Vec<usize>>,
    /// What each column's width comes from, shaped like `rows`.
    pub(super) cols: Vec<Vec<Col>>,
    /// The bands on screen.
    pub(super) bands: Vec<Band>,
    /// Every band, for a key that moves a window to a hidden desktop.
    pub(super) all: Vec<Band>,
    /// Each window's card width.
    pub(super) widths: Vec<f32>,
}

/// Which windows the strip shows and lists where: all of them or only the
/// `only` monitor's, and which windows each taskbar lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    pub only: Option<usize>,
    pub taskbars: MultiMonitor,
}

impl Model {
    pub(super) fn new(
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
