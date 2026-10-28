use super::*;

/// A monitor with no windows. Folded, it is a narrow dashed strip with the
/// monitor's icon and its name, written upward. While `open` it is a
/// column wide, with the same dashed box to drop a window into.
pub(super) fn rail(
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
pub(super) fn drop_box(ui: &Ui, painter: &egui::Painter, rect: Rect, target: bool, label: &str) {
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
pub(super) fn insertion_bar(ui: &Ui, others: &[Rect], index: usize, start: egui::Pos2) {
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

/// The Shift and digit hint, for a desktop with more than ten cards.
pub const SHIFT_DIGIT_HINT: (&str, &str) = ("Shift + 1\u{2013}0", "Cards 11\u{2013}20");

/// The key hints under the bands, most needed first: when the strip is too
/// narrow they go from the end. The D hint needs other desktops to show; the
/// Shift hint needs more than ten cards on this desktop.
pub(super) fn footer_hints(others_key: bool, over_ten: bool) -> Vec<(&'static str, &'static str)> {
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
    if over_ten {
        hints.insert(7, SHIFT_DIGIT_HINT);
    }
    if others_key {
        hints.push(("D", "Other desktops"));
    }
    hints.push(HIDE_HINT);
    hints
}

/// How many hints, from the first, fit in `room` when they are `widths`
/// wide and `gap` apart. "Select" and "Move" (the first and third) stay even
/// when they do not fit, so at least `keep` are shown.
pub(super) fn hints_that_fit(widths: &[f32], gap: f32, room: f32, keep: usize) -> usize {
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
pub(super) fn footer(ui: &Ui, panel: Rect, note: bool, others_key: bool, over_ten: bool) {
    let tokens = Tokens::get(ui.ctx());
    let painter = ui.painter();
    let area = Rect::from_min_max(pos2(panel.min.x, panel.max.y - FOOTER), panel.max);
    painter.hline(
        area.x_range(),
        area.top(),
        theme::line(ui.ctx(), tokens.border),
    );
    let centre = area.top() + 1.0 + (FOOTER - 1.0) / 2.0;
    let mut hints: Vec<Hint> = footer_hints(others_key, over_ten)
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
