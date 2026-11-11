//! The Board: the Claude chats of this desktop in four columns by status,
//! drawn over the deck. A view only; no card moves.

use super::*;

/// One row of the Board.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::ui::arrange) struct BoardRow {
    pub hwnd: isize,
    /// The card's taskbar number.
    pub number: usize,
    pub title: String,
    /// The wait, the run, or the age.
    pub time: String,
}

/// The Board's four columns, in the order of `BOARD_COLUMNS`: Waiting for
/// you (longest wait first), Working (longest run first), Idle under 1 day
/// (newest first), Cold, 1 day or more (oldest first). `windows` are the
/// cards on this desktop: (window, taskbar number, title, card).
pub(in crate::ui::arrange) fn columns(
    windows: &[(isize, usize, &str, &Card)],
    now: u64,
) -> [Vec<BoardRow>; 4] {
    let row = |(hwnd, number, title, _): &(isize, usize, &str, &Card), time: String| BoardRow {
        hwnd: *hwnd,
        number: *number,
        title: title.to_string(),
        time,
    };
    let lasted = |card: &Card| {
        card.since
            .map(|since| claude_sessions::duration_text(now.saturating_sub(since)))
            .unwrap_or_default()
    };
    let aged = |card: &Card| {
        card.last_activity
            .map(|at| claude_sessions::age_text(now.saturating_sub(at)))
            .unwrap_or_default()
    };
    let sorted = |filter: Filter, key: &dyn Fn(&Card) -> i128| {
        let mut picked: Vec<&(isize, usize, &str, &Card)> = windows
            .iter()
            .filter(|(_, _, _, card)| match filter {
                Filter::Idle => Filter::Idle.matches(card, now) && !Filter::Cold.matches(card, now),
                other => other.matches(card, now),
            })
            .collect();
        // Stable, so equal keys keep the cards' own order.
        picked.sort_by_key(|(_, _, _, card)| key(card));
        picked
    };
    let since = |card: &Card| i128::from(card.since.unwrap_or(u64::MAX));
    let activity = |card: &Card| i128::from(card.last_activity.unwrap_or(0));
    let waiting = sorted(Filter::Waiting, &since);
    let working = sorted(Filter::Working, &since);
    let idle = sorted(Filter::Idle, &|card| -activity(card));
    let cold = sorted(Filter::Cold, &activity);
    [
        waiting.iter().map(|w| row(w, lasted(w.3))).collect(),
        working.iter().map(|w| row(w, lasted(w.3))).collect(),
        idle.iter().map(|w| row(w, aged(w.3))).collect(),
        cold.iter().map(|w| row(w, aged(w.3))).collect(),
    ]
}

/// The Board's column names and the order each one keeps.
pub(in crate::ui::arrange) const BOARD_COLUMNS: [(&str, &str); 4] = [
    ("Waiting for you", "Longest wait first"),
    ("Working", "Longest run first"),
    ("Idle", "Under 1 day, newest first"),
    ("Cold", "1 day or more, oldest first"),
];

/// The Board's selected row after an arrow key: up and down in a column,
/// left and right to the nearest column with rows, keeping the row as far
/// as that column reaches. `lens` are the columns' lengths.
pub(in crate::ui::arrange) fn board_move(
    lens: [usize; 4],
    at: (usize, usize),
    key: egui::Key,
) -> (usize, usize) {
    let (column, row) = at;
    match key {
        egui::Key::ArrowUp => (column, row.saturating_sub(1)),
        egui::Key::ArrowDown => (column, (row + 1).min(lens[column].saturating_sub(1))),
        egui::Key::ArrowLeft => (0..column)
            .rev()
            .find(|c| lens[*c] > 0)
            .map_or(at, |c| (c, row.min(lens[c] - 1))),
        egui::Key::ArrowRight => (column + 1..4)
            .find(|c| lens[*c] > 0)
            .map_or(at, |c| (c, row.min(lens[c] - 1))),
        _ => at,
    }
}

/// The first row of the first column with rows.
pub(in crate::ui::arrange) fn board_start(lens: [usize; 4]) -> (usize, usize) {
    ((0..4).find(|c| lens[*c] > 0).unwrap_or(0), 0)
}

pub(in crate::ui::arrange) const BOARD_PAD: f32 = 16.0;
pub(in crate::ui::arrange) const BOARD_ROW: f32 = 32.0;
pub(in crate::ui::arrange) const BOARD_GAP: f32 = 12.0;

/// The Board, drawn over the deck in `area`: four columns of rows. Arrows
/// move, Enter or a click opens the chat (returned), Esc goes back to the
/// deck, which `window` handles through `Deck::escape`.
pub(in crate::ui::arrange) fn board(
    ui: &mut Ui,
    shared: &mut Shared,
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    area: Rect,
) -> Option<isize> {
    let tokens = Tokens::get(ui.ctx());
    let now = claude_sessions::now_ms();
    let cards = this_desktop(shared, group, desktops);
    let columns = columns(&cards, now);
    let lens = [
        columns[0].len(),
        columns[1].len(),
        columns[2].len(),
        columns[3].len(),
    ];
    let mut at = shared.claude.board.unwrap_or((0, 0));
    if lens[at.0] == 0 || at.1 >= lens[at.0] {
        at = if lens[at.0] == 0 {
            board_start(lens)
        } else {
            (at.0, lens[at.0] - 1)
        };
    }
    let ctx = ui.ctx().clone();
    let (moves, enter) = ctx.input_mut(|i| {
        let moves: Vec<egui::Key> = [
            egui::Key::ArrowUp,
            egui::Key::ArrowDown,
            egui::Key::ArrowLeft,
            egui::Key::ArrowRight,
        ]
        .into_iter()
        .filter(|key| i.consume_key(egui::Modifiers::NONE, *key))
        .collect();
        (
            moves,
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
        )
    });
    for key in moves {
        at = board_move(lens, at, key);
    }
    let mut open = enter
        .then(|| columns[at.0].get(at.1).map(|row| row.hwnd))
        .flatten();

    let painter = ui.painter().clone();
    painter.rect(
        area,
        tokens.radius_tile,
        tokens.elevated_bg,
        theme::line(ui.ctx(), tokens.border),
        StrokeKind::Inside,
    );
    let inner = area.shrink(BOARD_PAD);
    let title = painter.layout_no_wrap(
        "Claude board".into(),
        theme::semibold(15.0),
        tokens.text_primary,
    );
    let title_height = title.size().y;
    painter.galley(inner.min, title, tokens.text_primary);
    let chats = match cards.len() {
        1 => "1 chat".to_string(),
        n => format!("{n} chats"),
    };
    let sub = painter.layout_no_wrap(
        format!("{chats} on this desktop \u{b7} a view only, your cards stay where you put them"),
        theme::regular(13.0),
        tokens.text_secondary,
    );
    let hint = painter.layout_no_wrap(
        "Enter opens \u{b7} Esc back to WindowDeck".into(),
        theme::regular(13.0),
        tokens.text_secondary,
    );
    painter.galley(
        pos2(inner.left(), inner.top() + title_height + 2.0),
        sub,
        tokens.text_secondary,
    );
    painter.galley(
        pos2(inner.right() - hint.size().x, inner.top()),
        hint,
        tokens.text_secondary,
    );
    let columns_top = inner.top() + title_height + 2.0 + 18.0 + BOARD_GAP;
    let width = (inner.width() - 3.0 * BOARD_GAP) / 4.0;
    for (index, rows) in columns.iter().enumerate() {
        let left = inner.left() + index as f32 * (width + BOARD_GAP);
        let column =
            Rect::from_min_max(pos2(left, columns_top), pos2(left + width, inner.bottom()));
        let (name, order) = BOARD_COLUMNS[index];
        let filter = Filter::ALL[index];
        let colour = match filter {
            Filter::Waiting => tokens.warning,
            Filter::Working | Filter::Cold => tokens.info,
            Filter::Idle => tokens.text_secondary,
        };
        let mark = Rect::from_center_size(
            pos2(column.left() + MARK_SIZE / 2.0, column.top() + 10.0),
            Vec2::splat(MARK_SIZE),
        );
        match filter {
            Filter::Waiting => {
                painter.circle_filled(mark.center(), 4.0, tokens.warning);
            }
            Filter::Working => ring_mark(&painter, mark, tokens.info, 0.0),
            Filter::Idle => {
                painter.circle_filled(mark.center(), 4.0, tokens.text_disabled);
            }
            Filter::Cold => snowflake(&painter, mark.center(), 11.0, tokens.info),
        }
        let head =
            painter.layout_no_wrap(name.to_string(), theme::semibold(14.0), tokens.text_primary);
        let head_left = mark.right() + 6.0;
        let head_right = head_left + head.size().x;
        painter.galley(
            pos2(head_left, mark.center().y - head.size().y / 2.0),
            head,
            tokens.text_primary,
        );
        let count = painter.layout_no_wrap(rows.len().to_string(), theme::semibold(14.0), colour);
        painter.galley(
            pos2(head_right + 6.0, mark.center().y - count.size().y / 2.0),
            count,
            colour,
        );
        let order = painter.layout_no_wrap(
            order.to_string(),
            theme::regular(12.0),
            tokens.text_secondary,
        );
        painter.galley(
            pos2(column.left(), column.top() + 22.0),
            order,
            tokens.text_secondary,
        );
        let mut y = column.top() + 22.0 + 18.0 + 6.0;
        if rows.is_empty() {
            let none =
                painter.layout_no_wrap("None".into(), theme::regular(13.0), tokens.text_disabled);
            painter.galley(pos2(column.left(), y + 6.0), none, tokens.text_disabled);
        }
        for (row_index, row) in rows.iter().enumerate() {
            if y + BOARD_ROW > column.bottom() {
                break;
            }
            let rect = Rect::from_min_size(pos2(column.left(), y), vec2(width, BOARD_ROW));
            y += BOARD_ROW + 4.0;
            let response = ui
                .interact(
                    rect,
                    egui::Id::new(("claude-board-row", row.hwnd)),
                    Sense::click(),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            let picked = at == (index, row_index);
            if response.hovered() && ui.ctx().input(|i| i.pointer.delta() != Vec2::ZERO) {
                at = (index, row_index);
            }
            if response.clicked() {
                open = Some(row.hwnd);
            }
            let fill = if picked {
                Some(selected_fill(&tokens))
            } else if response.hovered() {
                Some(tokens.hover_bg)
            } else {
                None
            };
            if let Some(fill) = fill {
                painter.rect_filled(rect, tokens.radius_row, fill);
            }
            if picked {
                painter.rect_stroke(
                    rect,
                    tokens.radius_row,
                    theme::stroke(ui.ctx(), 2.0, selected_ring(&tokens)),
                    StrokeKind::Inside,
                );
            }
            let chip = number_chip(
                &painter,
                pos2(rect.left() + 6.0, rect.center().y - 11.0),
                22.0,
                row.number,
            );
            let time = painter.layout_no_wrap(
                row.time.clone(),
                theme::regular(12.0),
                tokens.text_secondary,
            );
            let time_left = rect.right() - 8.0 - time.size().x;
            painter.galley(
                pos2(time_left, rect.center().y - time.size().y / 2.0),
                time,
                tokens.text_secondary,
            );
            let mut job = text::job(&row.title, theme::regular(13.0), tokens.text_primary, None);
            job.wrap = TextWrapping {
                max_width: (time_left - 8.0 - chip.right() - 6.0).max(0.0),
                max_rows: 1,
                break_anywhere: true,
                overflow_character: Some('\u{2026}'),
            };
            let galley = painter.layout_job(job);
            painter.galley(
                pos2(chip.right() + 6.0, rect.center().y - galley.size().y / 2.0),
                galley,
                tokens.text_primary,
            );
        }
    }
    shared.claude.board = Some(at);
    open
}
