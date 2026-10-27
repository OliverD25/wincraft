use super::*;

/// One desktop's windows, in thumbnail order.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub title: String,
    pub desktop: String,
    /// Indexes into the group's windows.
    pub members: Vec<usize>,
}

/// The windows of one program, one row per desktop: the current desktop
/// first, titled "This desktop" and shown even when empty, then the others
/// in Task View order. A window with no known desktop is on this one.
pub fn rows(group: &ArrangeGroup, desktops: &[ArrangeDesktop]) -> Vec<Row> {
    let current = desktops
        .iter()
        .find(|desktop| desktop.current)
        .map(|desktop| desktop.id.clone())
        .unwrap_or_default();
    let known = |id: &str| desktops.iter().any(|desktop| desktop.id == id);
    let mut rows = vec![Row {
        title: "This desktop".to_string(),
        desktop: current.clone(),
        members: group
            .windows
            .iter()
            .enumerate()
            .filter(|(_, window)| window.desktop == current || !known(&window.desktop))
            .map(|(index, _)| index)
            .collect(),
    }];
    for desktop in desktops.iter().filter(|desktop| !desktop.current) {
        let members: Vec<usize> = group
            .windows
            .iter()
            .enumerate()
            .filter(|(_, window)| window.desktop == desktop.id)
            .map(|(index, _)| index)
            .collect();
        if !members.is_empty() {
            rows.push(Row {
                title: desktop.name.clone(),
                desktop: desktop.id.clone(),
                members,
            });
        }
    }
    rows
}

/// Each window's place in Windows' taskbar row for its app on its own
/// desktop, counted from 1 and indexed like `group.windows`. The group keeps
/// WinCraft's order, which is the order it puts on the taskbar, and Win+number
/// goes through the same row. A desktop counts on its own, whether it is on
/// screen or not.
pub fn taskbar_numbers(group: &ArrangeGroup, desktops: &[ArrangeDesktop]) -> Vec<usize> {
    let mut numbers = vec![0; group.windows.len()];
    for row in rows(group, desktops) {
        for (place, member) in row.members.iter().enumerate() {
            numbers[*member] = place + 1;
        }
    }
    numbers
}

/// The window whose card shows number `digit` on this desktop (0 is 10),
/// among the cards on screen (`visible`): what a plain digit in the strip
/// opens. None when no card has that number.
pub fn card_for_digit(
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    visible: &[bool],
    digit: u8,
) -> Option<isize> {
    let number = if digit == 0 { 10 } else { usize::from(digit) };
    let numbers = taskbar_numbers(group, desktops);
    let this_desktop = rows(group, desktops).into_iter().next()?;
    this_desktop
        .members
        .into_iter()
        .find(|member| numbers[*member] == number && visible.get(*member).copied().unwrap_or(true))
        .map(|member| group.windows[member].hwnd)
}

/// The card after `cursor` among `cards` (this desktop's, in taskbar
/// order), round to the first after the last; the first when none is
/// picked or the picked one is not among them.
pub fn next_card(cards: &[isize], cursor: Option<isize>) -> Option<isize> {
    let at = cursor.and_then(|hwnd| cards.iter().position(|card| *card == hwnd));
    match at {
        Some(at) => cards.get((at + 1) % cards.len()).copied(),
        None => cards.first().copied(),
    }
}

/// A window's place in its desktop's taskbar row (from 1) and how many
/// windows the row has.
pub fn place_in_row(
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    member: usize,
) -> Option<(usize, usize)> {
    rows(group, desktops).iter().find_map(|row| {
        row.members
            .iter()
            .position(|m| *m == member)
            .map(|at| (at + 1, row.members.len()))
    })
}

/// The whole group's order once `member` takes `place` (from 1, kept within
/// the row) in its desktop's taskbar row. The row's windows keep their
/// slots in the group's order, so every other desktop and every window's
/// monitor are untouched. None when nothing would change.
pub fn order_with_place(
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    member: usize,
    place: usize,
) -> Option<Vec<isize>> {
    let rows = rows(group, desktops);
    let row = rows.iter().find(|row| row.members.contains(&member))?;
    let mut sequence = row.members.clone();
    sequence.retain(|m| *m != member);
    sequence.insert((place.max(1) - 1).min(sequence.len()), member);
    let now: Vec<isize> = group.windows.iter().map(|w| w.hwnd).collect();
    let mut order = now.clone();
    for (slot, window) in row.members.iter().zip(sequence) {
        order[*slot] = group.windows[window].hwnd;
    }
    (order != now).then_some(order)
}

/// Gives `member` another place in its taskbar row and nothing else: the
/// same Reorder a drag sends, never a move to a monitor or desktop.
pub fn take_place(
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    member: usize,
    place: usize,
) -> Option<ArrangeAction> {
    order_with_place(group, desktops, member, place).map(|order| ArrangeAction::Reorder {
        group: group.key.clone(),
        order,
    })
}

/// Alt+Left (`step` -1) and Alt+Right (+1): one number lower or higher. None
/// at either end of the row.
pub fn nudge(
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    member: usize,
    step: isize,
) -> Option<ArrangeAction> {
    let (place, count) = place_in_row(group, desktops, member)?;
    let wanted = place.checked_add_signed(step)?;
    if (1..=count).contains(&wanted) {
        take_place(group, desktops, member, wanted)
    } else {
        None
    }
}

/// The entries of the "Taskbar place" menu: one per place in a row of
/// `count` windows, the window's own place (`current`) marked.
pub fn place_labels(count: usize, current: usize) -> Vec<String> {
    (1..=count)
        .map(|place| {
            if place == current {
                format!("{place} (now)")
            } else {
                place.to_string()
            }
        })
        .collect()
}

/// The card a Ctrl drop lands on: the one under the pointer, a little
/// beyond its edge so the gap between cards belongs to a card too. The
/// dragged window's own card never counts.
pub fn ctrl_target(pointer: Pos2, dragged: isize, cards: &[(isize, Rect)]) -> Option<isize> {
    cards
        .iter()
        .filter(|(hwnd, _)| *hwnd != dragged)
        .find(|(_, rect)| rect.expand(CARD_GAP / 2.0).contains(pointer))
        .map(|(hwnd, _)| *hwnd)
}

/// The place a Ctrl drop gives `member`: the target card's number. None when
/// the target is on another desktop, whose numbers are not this row's.
pub fn ctrl_place(
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    member: usize,
    target: isize,
) -> Option<usize> {
    let target = group.windows.iter().position(|w| w.hwnd == target)?;
    let (place, _) = place_in_row(group, desktops, target)?;
    let same_row = rows(group, desktops)
        .iter()
        .any(|row| row.members.contains(&member) && row.members.contains(&target));
    (same_row && target != member).then_some(place)
}

/// What a Ctrl drop onto `target` does: `member` takes the target's number.
pub fn ctrl_drop(
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    member: usize,
    target: isize,
) -> Option<ArrangeAction> {
    let place = ctrl_place(group, desktops, member, target)?;
    take_place(group, desktops, member, place)
}

/// The hint shown beside a card dragged with Ctrl held.
pub fn ctrl_hint(place: Option<usize>) -> String {
    match place {
        Some(place) => format!("Change number only: takes number {place}"),
        None => "Change number only: drop on a card of this desktop".to_string(),
    }
}

/// The monitor a hover strip is limited to: the one whose taskbar it sits
/// on, when Windows lists only that monitor's windows on that taskbar. None
/// for a strip opened by the hotkey (no `anchor`) and for a taskbar that
/// lists every window.
pub fn only_monitor(
    anchor: Option<[i32; 4]>,
    monitors: &[ArrangeMonitor],
    setting: MultiMonitor,
) -> Option<usize> {
    let button = anchor?;
    let (x, y) = ((button[0] + button[2]) / 2, (button[1] + button[3]) / 2);
    let at = monitors.iter().position(|m| {
        m.connected && x >= m.rect[0] && x < m.rect[2] && y >= m.rect[1] && y < m.rect[3]
    })?;
    setting.shows_only_own(monitors[at].primary).then_some(at)
}

/// Which of the group's windows the strip shows: all of them, or only those
/// on the `only` monitor (a window of unknown place counts as on the main
/// one).
pub fn visible_flags(
    group: &ArrangeGroup,
    monitors: &[ArrangeMonitor],
    only: Option<usize>,
) -> Vec<bool> {
    let Some(only) = only else {
        return vec![true; group.windows.len()];
    };
    let main = monitors.iter().position(|m| m.primary && m.connected);
    group
        .windows
        .iter()
        .map(|window| by_monitor::column_of(window, monitors).or(main) == Some(only))
        .collect()
}

/// `rows` with only the windows that are shown. This desktop's row stays
/// even when empty; another desktop's row goes when nothing in it is shown.
pub(super) fn only_visible(rows: Vec<Row>, visible: &[bool]) -> Vec<Row> {
    rows.into_iter()
        .enumerate()
        .filter_map(|(at, mut row)| {
            row.members.retain(|member| visible[*member]);
            (at == 0 || !row.members.is_empty()).then_some(row)
        })
        .collect()
}

/// The group's order once the shown windows are in `shown`'s order: they
/// keep the places the shown windows had, so a window that is not shown stays
/// where it was.
pub fn merge_order(full: &[isize], shown: &[isize]) -> Vec<isize> {
    let wanted: Vec<isize> = shown
        .iter()
        .copied()
        .filter(|hwnd| full.contains(hwnd))
        .collect();
    let mut merged = full.to_vec();
    let slots: Vec<usize> = full
        .iter()
        .enumerate()
        .filter(|(_, hwnd)| wanted.contains(hwnd))
        .map(|(at, _)| at)
        .collect();
    for (slot, hwnd) in slots.into_iter().zip(wanted) {
        merged[slot] = hwnd;
    }
    merged
}

/// The rows on screen: only this desktop's until the other desktops are
/// shown, and only the windows that are shown.
pub(super) fn shown_rows(
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    shown: bool,
    visible: &[bool],
) -> Vec<Row> {
    let mut rows = only_visible(rows(group, desktops), visible);
    if !shown {
        rows.truncate(1);
    }
    rows
}
