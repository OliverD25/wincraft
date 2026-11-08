//! One Quick access card: the picture box, the app icon, the title and app
//! line, and the keycap under it.

use egui::{pos2, vec2, Align2, Painter, Rect, Stroke, StrokeKind, Ui};

use crate::core::theme::{self, Tokens};
use crate::ui::widgets::icons::{self, Icon};

use super::super::matching::{SlotView, State};
use super::grid::{CAP, CAP_GAP, ICON, PICTURE_H, TEXT_SHIFT};
use super::{app_icon, name_card, ListUi, NOT_OPEN};

/// The light outline of a card's picture box, when no live picture fills it.
fn frame_box(painter: &Painter, picture: Rect, tokens: &Tokens) {
    painter.rect_stroke(
        picture,
        6,
        Stroke::new(1.0, theme::with_alpha(tokens.text_primary, 0.08)),
        StrokeKind::Inside,
    );
}

/// A card that is not open names its window in the quieter colour.
pub(super) fn title_colour(state: State, tokens: &Tokens) -> egui::Color32 {
    if state == State::NotOpen {
        tokens.text_secondary
    } else {
        tokens.text_primary
    }
}

/// How one card is drawn.
#[derive(Clone, Copy)]
pub(super) struct Look {
    pub(super) selected: bool,
    pub(super) hovered: bool,
    /// Whether its live picture may show.
    pub(super) live: bool,
    /// How much of the card shows, for one that fades out.
    pub(super) alpha: f32,
}

pub(super) fn draw_card(
    ui: &Ui,
    painter: &Painter,
    rect: Rect,
    card: &SlotView,
    look: Look,
    state: &mut ListUi,
) {
    let tokens = Tokens::get(ui.ctx());
    let Look {
        selected, hovered, ..
    } = look;
    let (fill, edge) = if selected {
        (theme::with_alpha(tokens.accent, 0.10), tokens.accent)
    } else if hovered {
        (tokens.hover_bg, theme::with_alpha(tokens.text_primary, 0.0))
    } else {
        (
            theme::with_alpha(tokens.text_primary, 0.04),
            theme::with_alpha(tokens.text_primary, 0.0),
        )
    };
    painter.rect(rect, 9, fill, Stroke::new(2.0, edge), StrokeKind::Inside);
    let inner = rect.shrink(10.0);
    let picture = Rect::from_min_size(inner.min, vec2(inner.width(), PICTURE_H));
    let pixels_per_point = ui.ctx().pixels_per_point();
    match (card.state, card.hwnd) {
        (State::Open | State::InFront, Some(hwnd)) => {
            let scale = state.picture_scale;
            let shown = look.live.then(|| {
                state
                    .pictures
                    .show_faded(hwnd, picture, pixels_per_point, scale, look.alpha)
            });
            match shown.flatten() {
                // A frame that hugs the picture, so its width shows.
                Some(shown) => {
                    painter.rect_filled(shown.expand(1.0), 4, tokens.panel_bg);
                    painter.rect_stroke(
                        shown.expand(1.0),
                        4,
                        Stroke::new(1.0, theme::with_alpha(tokens.text_primary, 0.12)),
                        StrokeKind::Outside,
                    );
                }
                None => {
                    painter.rect_filled(picture, 6, tokens.panel_bg);
                    name_card(painter, picture, card.title_line(), &tokens, 1.0);
                    frame_box(painter, picture, &tokens);
                }
            }
        }
        _ => {
            painter.rect_filled(picture, 6, theme::with_alpha(tokens.panel_bg, NOT_OPEN));
            let _ = app_icon(
                ui,
                painter,
                state,
                card,
                Rect::from_center_size(picture.center(), vec2(48.0, 48.0)),
                NOT_OPEN,
            );
            frame_box(painter, picture, &tokens);
        }
    }
    let width = inner.width() - 4.0;
    if selected || hovered {
        // A card can be dragged to another place.
        icons::paint(
            painter,
            Rect::from_min_size(
                pos2(inner.right() - 16.0, picture.bottom() + 14.0),
                vec2(16.0, 16.0),
            ),
            Icon::Grip,
            tokens.text_secondary,
        );
    }
    let alpha = icon_alpha(card.state);
    let icon = Rect::from_min_size(
        pos2(inner.left() + 2.0, picture.bottom() + 14.0),
        vec2(ICON, ICON),
    );
    if !app_icon(ui, painter, state, card, icon, alpha) {
        letter_icon(painter, icon, &card.app_name, &tokens, alpha);
    }
    let text_left = inner.left() + 2.0 + TEXT_SHIFT;
    let width = width - TEXT_SHIFT;
    let title = super::clip_text(
        painter,
        card.title_line(),
        theme::regular(18.0),
        width - 22.0,
    );
    painter.text(
        pos2(text_left, picture.bottom() + 10.0),
        Align2::LEFT_TOP,
        title,
        theme::regular(18.0),
        title_colour(card.state, &tokens),
    );
    let second = match card.state {
        State::NotOpen => format!("{} \u{b7} not open", card.app_name),
        _ if card.place.is_empty() => card.app_name.clone(),
        _ => format!("{} \u{b7} {}", card.app_name, card.place),
    };
    let second = super::clip_text(painter, &second, theme::regular(13.0), width);
    painter.text(
        pos2(text_left, picture.bottom() + 36.0),
        Align2::LEFT_TOP,
        second,
        theme::regular(13.0),
        tokens.text_secondary,
    );
    let cap = Rect::from_center_size(
        pos2(rect.center().x, rect.bottom() + CAP_GAP + CAP / 2.0),
        vec2(CAP, CAP),
    );
    let (cap_edge, cap_text) = if selected {
        (tokens.accent, tokens.accent)
    } else {
        (
            theme::with_alpha(tokens.text_primary, 0.2),
            tokens.text_primary,
        )
    };
    painter.rect(
        cap,
        6,
        theme::with_alpha(tokens.text_primary, 0.05),
        Stroke::new(1.0, cap_edge),
        StrokeKind::Inside,
    );
    painter.hline(
        cap.x_range().shrink(3.0),
        cap.bottom() - 1.5,
        Stroke::new(3.0, cap_edge),
    );
    painter.text(
        cap.center() - vec2(0.0, 1.0),
        Align2::CENTER_CENTER,
        card.digit.to_string(),
        theme::semibold(15.0),
        cap_text,
    );
}

/// A closed window's icon is faded like the rest of its card.
pub(super) fn icon_alpha(state: State) -> f32 {
    match state {
        State::Open | State::InFront => 1.0,
        _ => NOT_OPEN,
    }
}

/// The first letter of the app, for an app whose exe gave no icon.
pub(super) fn first_letter(app: &str) -> String {
    app.chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".to_owned())
}

fn letter_icon(painter: &Painter, rect: Rect, app: &str, tokens: &Tokens, alpha: f32) {
    painter.rect_filled(
        rect,
        7,
        theme::with_alpha(tokens.text_primary, 0.10 * alpha),
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        first_letter(app),
        theme::semibold(16.0),
        tokens.text_secondary.gamma_multiply(alpha),
    );
}
