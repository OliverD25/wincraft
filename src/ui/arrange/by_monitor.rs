//! The By monitor view: one band per virtual desktop, the current one first,
//! and inside each band one column per monitor, laid out the way the
//! monitors stand on the desk.

use egui::text::TextWrapping;
use egui::{pos2, vec2, Color32, Rect, Response, Sense, Shape, StrokeKind, Ui, UiBuilder, Vec2};

use super::{
    card_above_or_below, card_look, card_menu, cards_here, ctrl_drop, ctrl_hint, ctrl_mark,
    ctrl_place, ctrl_target, float_picture, ghost_thumbnail, grown, hit_rect, line_width,
    middle_close, name_card, nudge, number_chip, order_with_place, others, others_link,
    paint_badges, paint_look, picture_width, place_in_row, taskbar_numbers, thumbnail,
    visible_flags, wrap, Badge, Drag, Frame, Pending, Shared, NUMBER_GAP, SLACK,
};
use crate::core::taskbar_scope::MultiMonitor;
use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{
    ArrangeAction, ArrangeDesktop, ArrangeGroup, ArrangeMonitor, ArrangeSnapshot, ArrangeWindow,
};
use crate::ui::widgets::icons::{self, Icon};
use crate::ui::widgets::text;
mod bands;
mod chrome;
mod columns;
mod draw;
mod draw_band;
mod draw_card;
mod keys;
mod moves;
pub use bands::*;
pub use chrome::*;
pub use columns::*;
pub use draw::*;
use draw_card::*;
pub use keys::*;
pub use moves::*;

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

/// The last footer hint. The toolbar shows the same key while the footer is
/// hidden, as "Hints".
pub const HIDE_HINT: (&str, &str) = ("Ctrl + H", "Hide hints");

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

#[cfg(test)]
mod tests;
