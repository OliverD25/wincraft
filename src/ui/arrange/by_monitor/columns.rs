use super::*;

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
    pub(super) need: f32,
    /// How many windows it holds, in all the bands on screen.
    pub(super) count: usize,
    pub(super) primary: bool,
    /// A connected monitor with no windows, folded into a narrow rail.
    pub(super) rail: bool,
    /// The monitor's width in pixels, which sets its share of the row.
    pub(super) screen: f32,
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
pub(super) fn winner(cols: &[Col]) -> Option<usize> {
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
pub(super) fn rail_width(open: f32) -> f32 {
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
    // An opening rail takes only width the other columns can spare: taking
    // more would wrap their cards onto a line the panel has no room for.
    let folded: f32 = cols
        .iter()
        .map(|col| if col.rail { RAIL } else { col.need })
        .sum();
    let wanted: f32 = cols
        .iter()
        .enumerate()
        .filter(|(_, col)| col.rail)
        .map(|(index, _)| rail_width(open.get(index).copied().unwrap_or(0.0)) - RAIL)
        .sum();
    let share = if wanted > 0.0 {
        ((available - folded) / wanted).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let mut widths: Vec<f32> = cols
        .iter()
        .enumerate()
        .map(|(index, col)| {
            if col.rail {
                RAIL + (rail_width(open.get(index).copied().unwrap_or(0.0)) - RAIL) * share
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
pub(super) fn row_tracks(cols: &[Col], open: &[f32], width: f32) -> Vec<f32> {
    let gaps = COLUMN_GAP * cols.len().saturating_sub(1) as f32;
    tracks(cols, open, width - 2.0 * SIDE - gaps)
}

/// A card's width for a window whose picture shows `source`.
pub fn card_width(source: Option<Vec2>) -> f32 {
    picture_width(source, THUMB_HEIGHT) + 2.0 * CARD_PAD
}

/// The widths of a cell's cards and ghosts, from every window's card width.
pub(super) fn flow_widths(flow: &[Item], widths: &[f32]) -> Vec<f32> {
    flow.iter()
        .map(|item| match item {
            Item::Card(member) => widths[*member],
            Item::Ghost(_) => GHOST_WIDTH,
        })
        .collect()
}

/// A cell's cards and ghosts, `widths` wide, in lines that fit in a column
/// `track` wide.
pub(super) fn cell_lines(widths: &[f32], track: f32) -> Vec<std::ops::Range<usize>> {
    wrap(widths, track - INSET)
}

pub(super) fn cell_height(widths: &[f32], track: f32) -> f32 {
    if widths.is_empty() {
        return EMPTY;
    }
    let lines = cell_lines(widths, track).len() as f32;
    lines * CARD_HEIGHT + (lines - 1.0) * CARD_GAP
}
