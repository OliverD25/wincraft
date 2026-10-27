use super::*;

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
pub(super) fn pick(shared: &mut Shared, group: &ArrangeGroup, item: Option<Item>) {
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
pub(super) fn rail_wanted(dragging: bool, focus: Option<usize>, monitor: usize) -> bool {
    dragging || focus == Some(monitor)
}

/// The monitor behind `cell` when its column is a rail.
pub(super) fn rail_at(
    cols: &[Vec<Col>],
    rows: &[Vec<usize>],
    cell: (usize, usize),
) -> Option<usize> {
    let col = cols.get(cell.0)?.get(cell.1)?;
    col.rail.then(|| rows[cell.0][cell.1])
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
    pub(super) band: usize,
    /// The monitor cell, or None for the header of a band with no windows.
    pub(super) cell: Option<(usize, usize)>,
    pub(super) rect: Rect,
    /// A disconnected monitor takes no windows; its own can still be
    /// reordered.
    pub(super) open: bool,
    /// The cards in it: their windows and rectangles.
    pub(super) cards: Vec<(usize, Rect)>,
}

/// Where the pointer would drop `dragged`: the zone under it, or the
/// nearest, and how many of that zone's other cards come before it, on the
/// lines above or to its left on its own line. `home` is the cell a drop on
/// a bare band header puts the window in.
pub(super) fn drop_spot(
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
