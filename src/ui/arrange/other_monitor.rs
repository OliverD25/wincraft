//! The strip that opens from the taskbar of a monitor that is not the main
//! one, when that taskbar lists only its own monitor's windows. It needs no
//! app list and no view switch: unless the user turned the toolbar on, a
//! short header takes its place. When all of this desktop's cards fit on one
//! line at the compact popup's size, they are drawn as its cards, sharing
//! the width equally.

use super::*;

/// The widest a card gets there.
pub const TILE_MAX: f32 = 280.0;
/// The short header, from the panel's top: the app name's line, then the
/// key hints under it.
const HEADER_TOP: f32 = 8.0;
const TITLE_ROW: f32 = 28.0;
const HINTS_TOP: f32 = 30.0;
/// The button that shows or hides the other desktops.
const BUTTON: f32 = 28.0;
/// Where the compact cards start, from the panel's top: under the header,
/// with room for the selected card's ring.
pub(super) const CARDS_TOP: f32 = 58.0;
/// Room above the first line of compact cards for the ring around the
/// selected one, which the scroll area would cut off.
pub(super) const RING_ROOM: f32 = 4.0;

/// Whether a strip limited to `only` sits on a monitor that is not the
/// main one.
pub fn on_other_monitor(only: Option<usize>, monitors: &[ArrangeMonitor]) -> bool {
    only.and_then(|at| monitors.get(at))
        .is_some_and(|monitor| !monitor.primary)
}

/// Whether the strip shows its toolbar: always, but on a monitor that is not
/// the main one only when the setting says so.
pub fn shows_toolbar(other_monitor: bool, setting: bool) -> bool {
    !other_monitor || setting
}

/// The compact cards' width when `count` of them fit on one line `room`
/// wide at the compact popup's size: an equal share of the room, at most
/// `TILE_MAX`. None when they do not fit.
pub fn tile_width(count: usize, room: f32) -> Option<f32> {
    if count == 0 {
        return None;
    }
    let gaps = (count - 1) as f32 * CARD_GAP;
    // Half a point of room: a panel sized to fit exactly can round down.
    if count as f32 * compact::CARD_WIDTH + gaps > room + 0.5 {
        return None;
    }
    Some(((room - gaps) / count as f32).floor().min(TILE_MAX))
}

/// The size of the compact cards.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tiles {
    pub width: f32,
    /// The pictures' height, one for every card.
    pub thumb: f32,
}

/// How the strip's cards are drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Cards {
    /// Today's cards, each as wide as its window's shape.
    Strip,
    /// The compact popup's cards.
    Tiles(Tiles),
}

impl Cards {
    pub(super) fn height(self) -> f32 {
        match self {
            Cards::Strip => CARD_HEIGHT,
            Cards::Tiles(tiles) => tile::card_height(tiles.thumb),
        }
    }

    /// Where the picture goes in a card `rect`.
    pub(super) fn picture(self, rect: Rect) -> Rect {
        match self {
            Cards::Strip => picture_area(rect),
            Cards::Tiles(tiles) => tile::picture_box(rect, tiles.thumb),
        }
    }
}

/// The compact cards for `members`, this desktop's shown windows, in a strip
/// whose cards may use `room` across. None when they do not fit on one line.
pub(super) fn tiles_for(
    group: &ArrangeGroup,
    monitors: &[ArrangeMonitor],
    members: &[usize],
    room: f32,
) -> Option<Tiles> {
    let width = tile_width(members.len(), room)?;
    let thumb = members
        .iter()
        .map(|member| tile::thumb_height(width, compact::monitor_size(group, *member, monitors)))
        .fold(tile::THUMB_MIN, f32::max);
    Some(Tiles { width, thumb })
}

/// The panel for the compact cards: this desktop's line under the short
/// header, then each other desktop shown with its name, its cards wrapping
/// at `limit.x`. `link` makes room for the line that opens the other
/// desktops, `note` for the note on their order.
pub fn panel_size(rows: &[Row], tiles: Tiles, link: bool, note: bool, limit: Vec2) -> Vec2 {
    let room = limit.x - 2.0 * PADDING - SLACK;
    let card = tile::card_height(tiles.thumb);
    let mut widest: f32 = 0.0;
    let mut height = CARDS_TOP + RING_ROOM;
    for (index, row) in rows.iter().enumerate() {
        let widths = vec![tiles.width; row.members.len().max(1)];
        let lines = wrap(&widths, room);
        widest = lines
            .iter()
            .map(|line| line_width(&widths[line.clone()]))
            .fold(widest, f32::max);
        if index > 0 {
            height += ROW_GAP + HEADER + HEADER_GAP;
        }
        let count = lines.len() as f32;
        height += count * card + (count - 1.0) * CARD_GAP;
        if index == 0 && link {
            height += others::LINK_HEIGHT;
        }
    }
    if note && rows.len() > 1 {
        height += FOOTER;
    }
    height += PADDING + SLACK;
    vec2(2.0 * PADDING + widest + SLACK, height).min(limit)
}

impl Shared {
    /// The strip sits on a monitor that is not the main one.
    pub(super) fn on_other_monitor(&self) -> bool {
        let monitors = self.snapshot.as_ref().map_or(&[][..], |s| &s.monitors);
        on_other_monitor(self.only_monitor, monitors)
    }

    /// The short header replaces the toolbar.
    pub(super) fn short_header(&self) -> bool {
        let setting = self
            .snapshot
            .as_ref()
            .is_some_and(|s| s.toolbar_on_other_monitors);
        !shows_toolbar(self.on_other_monitor(), setting)
    }

    /// The compact cards, when the strip is on a monitor that is not the
    /// main one, is not the compact popup, and this desktop's cards fit on
    /// one line.
    pub(super) fn tiles(&self) -> Option<Tiles> {
        if !self.on_other_monitor() || self.compact() {
            return None;
        }
        let (group, snapshot) = (self.group()?, self.snapshot.as_ref()?);
        let rows = shown_rows(group, &snapshot.desktops, false, &self.visible(group));
        let here = rows.first().map(|row| row.members.as_slice())?;
        let room = self.limit().x - 2.0 * PADDING - SLACK;
        tiles_for(group, &snapshot.monitors, here, room)
    }
}

/// The short header: the app's name, the button that shows or hides the
/// other desktops (as D does) when they have windows, and the key hints.
pub(super) fn header(
    ui: &mut Ui,
    shared: &mut Shared,
    panel: Rect,
    group: &ArrangeGroup,
    snapshot: &ArrangeSnapshot,
    others_count: usize,
) {
    let tokens = Tokens::get(ui.ctx());
    let row = Rect::from_min_size(
        pos2(panel.left() + PADDING, panel.top() + HEADER_TOP),
        vec2(panel.width() - 2.0 * PADDING, TITLE_ROW),
    );
    let mut title_right = row.right();
    if others_count > 0 {
        let rect = Rect::from_min_size(
            pos2(row.right() - BUTTON, row.center().y - BUTTON / 2.0),
            Vec2::splat(BUTTON),
        );
        title_right = rect.left() - 6.0;
        let open = shared.others.shown(others_count);
        let tip = if open {
            "Hide the other desktops (D)"
        } else {
            "Show the other desktops (D)"
        };
        let response = ui
            .interact(
                rect,
                egui::Id::new("arrange-other-desktops"),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tip);
        if open || response.hovered() {
            ui.painter()
                .rect_filled(rect, tokens.radius_row, tokens.hover_bg);
        }
        icons::paint(
            ui.painter(),
            Rect::from_center_size(rect.center(), Vec2::splat(14.0)),
            Icon::ChevronDown,
            tokens.text_primary,
        );
        if response.clicked() {
            shared.others.toggle();
            ui.ctx().request_repaint();
        }
    }
    let mut job = text::job(
        &group.label,
        theme::semibold(13.0),
        tokens.text_primary,
        None,
    );
    job.wrap = TextWrapping {
        max_width: (title_right - row.left()).max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        pos2(row.left(), row.center().y - galley.size().y / 2.0),
        galley,
        tokens.text_primary,
    );
    let hints = compact::hints(false, false, None, seen::any_marked(snapshot));
    compact::hint_line(
        ui,
        pos2(row.left(), panel.top() + HEADER_TOP + HINTS_TOP),
        &hints,
        row.width(),
    );
}

/// One compact card in the strip, laid out by `ui` like today's cards and
/// answering the mouse the same way. Returns what `cards::card` returns.
pub(super) fn tile_card(
    ui: &mut Ui,
    tiles: Tiles,
    window: &ArrangeWindow,
    info: CardInfo<'_>,
) -> (Response, Rect, bool, Rect) {
    let CardInfo {
        number,
        can_rename,
        has_picture,
        is_cursor,
        leaving,
        motion: (scale, fade),
        awake,
        edit,
        slides,
        animate,
        waiting,
        claude: _,
    } = info;
    let sense = if leaving {
        Sense::hover()
    } else {
        Sense::click_and_drag()
    };
    let size = vec2(tiles.width, tile::card_height(tiles.thumb));
    let (placed, response) = ui.allocate_exact_size(size, sense);
    let rect = grown(
        slides.place((window.hwnd, false), placed, Instant::now(), animate),
        scale,
    );
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(fade);
    let painted = tile::paint(
        ui,
        &painter,
        &response,
        rect,
        window,
        tile::Look {
            thumb: tiles.thumb,
            number,
            picked: is_cursor && !leaving,
            faded: false,
            hovered: awake && !leaving && response.hovered(),
            attention: window.attention,
            fill: tile::Fill::Strip,
            has_picture,
            // The painter is faded already.
            opacity: 1.0,
            same_title: (window.same_title && !leaving).then_some(can_rename),
            waiting: waiting.as_deref(),
        },
        edit,
    );
    (
        response.on_hover_cursor(egui::CursorIcon::Grab),
        painted.picture,
        painted.renamed,
        painted.clear,
    )
}

#[cfg(test)]
mod tests;
