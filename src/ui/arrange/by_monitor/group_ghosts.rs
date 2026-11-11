//! An open group in the By monitor view: its windows on other monitors show
//! as ghosts in the column where the group stands, so the whole group sits
//! together in one frame. Their own cards stay in their columns, dimmed.

use super::*;
use crate::ui::arrange::stacks::OpenMembers;

/// The cell of `band` that holds most of `members`; on a tie the one with
/// `rep` (the window used last), else the first in reading order.
fn home_cell(band: &Band, members: &[usize], rep: Option<usize>) -> Option<(usize, usize)> {
    let mut best: Option<((usize, usize), usize, bool)> = None;
    for (r, row) in band.cells.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let count = cell.iter().filter(|m| members.contains(m)).count();
            if count == 0 {
                continue;
            }
            let has_rep = rep.is_some_and(|rep| cell.contains(&rep));
            let better = match best {
                None => true,
                Some((_, most, rep_there)) => {
                    count > most || (count == most && has_rep && !rep_there)
                }
            };
            if better {
                best = Some(((r, c), count, has_rep));
            }
        }
    }
    best.map(|(cell, _, _)| cell)
}

/// Puts the open group's windows that are on another connected monitor as
/// ghosts into the group's home cell, in each band. Ghosts of other windows
/// that the home cell does not show give way while the group is open.
/// Returns the members that got a ghost.
pub fn add_group_ghosts(
    bands: &mut [Band],
    rows: &[Vec<usize>],
    monitors: &[ArrangeMonitor],
    members: &[usize],
    rep: Option<usize>,
) -> Vec<usize> {
    let mut ghosted = Vec::new();
    for band in bands {
        let Some(home) = home_cell(band, members, rep) else {
            continue;
        };
        let away: Vec<usize> = band
            .cells
            .iter()
            .enumerate()
            .flat_map(|(r, row)| row.iter().enumerate().map(move |(c, cell)| ((r, c), cell)))
            .filter(|(at, _)| *at != home && monitors[rows[at.0][at.1]].connected)
            .flat_map(|(_, cell)| cell.iter().copied())
            .filter(|member| members.contains(member))
            .collect();
        if away.is_empty() {
            continue;
        }
        let mut ghosts = if band.ghost_cell == Some(home) {
            std::mem::take(&mut band.ghosts)
        } else {
            Vec::new()
        };
        for member in &away {
            if !ghosts.contains(member) {
                ghosts.push(*member);
            }
        }
        ghosts.sort_unstable();
        band.ghost_cell = Some(home);
        band.ghosts = ghosts;
        ghosted.extend(away);
    }
    ghosted
}

/// The room above each line of cards while a group is open: the group's
/// header line and a little air.
pub const GROUP_LEAD: f32 = 20.0;

/// A cell's height with `lead` above each of its lines. An empty cell (its
/// drop box) has no lines.
pub(super) fn led_height(height: f32, lead: f32) -> f32 {
    if height == EMPTY {
        return height;
    }
    let lines = ((height + CARD_GAP) / (CARD_HEIGHT + CARD_GAP)).round();
    height + lines * lead
}

/// A cell's places, from `top`, each line moved down by `lead` more than
/// the line above.
pub(super) fn led_places(
    placed: Vec<(Option<Item>, Rect)>,
    top: f32,
    lead: f32,
) -> Vec<(Option<Item>, Rect)> {
    if lead == 0.0 {
        return placed;
    }
    placed
        .into_iter()
        .map(|(item, rect)| {
            let line = ((rect.top() - top) / (CARD_HEIGHT + CARD_GAP)).round();
            (item, rect.translate(vec2(0.0, (line + 1.0) * lead)))
        })
        .collect()
}

impl Model {
    /// The model with the open group's ghosts, and the windows that got
    /// one (as handles).
    pub(super) fn with_open_group(
        mut self,
        group: &ArrangeGroup,
        monitors: &[ArrangeMonitor],
        open: &OpenMembers,
    ) -> (Self, Vec<isize>) {
        if open.hwnds.is_empty() {
            return (self, Vec::new());
        }
        for band in self.bands.iter_mut().chain(self.all.iter_mut()) {
            band.lead = GROUP_LEAD;
        }
        let place = |hwnd: &isize| group.windows.iter().position(|w| w.hwnd == *hwnd);
        let members: Vec<usize> = open.hwnds.iter().filter_map(place).collect();
        let rep = open.rep.as_ref().and_then(place);
        add_group_ghosts(&mut self.all, &self.rows, monitors, &members, rep);
        let ghosted = add_group_ghosts(&mut self.bands, &self.rows, monitors, &members, rep);
        self.cols = columns(&self.bands, &self.rows, monitors, &self.widths);
        let hwnds = ghosted.iter().map(|m| group.windows[*m].hwnd).collect();
        (self, hwnds)
    }
}

#[cfg(test)]
mod tests;
