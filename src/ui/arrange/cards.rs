use super::*;

/// The right-click menu. Closing a window is only offered here, never on a
/// key, because it cannot be undone. `renamed` is None when WindowNamer is
/// off, else whether the window has a name. "Taskbar place" lists the places
/// of the window's row and changes nothing but its number.
pub(super) fn card_menu(
    ui: &mut Ui,
    group: &ArrangeGroup,
    member: usize,
    desktops: &[ArrangeDesktop],
    renamed: Option<bool>,
) -> Option<ArrangeAction> {
    let hwnd = group.windows[member].hwnd;
    let current = group.windows[member].desktop.as_str();
    let mut chosen = None;
    if let Some(renamed) = renamed {
        if ui.button("Rename\u{2026}").clicked() {
            chosen = Some(ArrangeAction::AskName(hwnd));
            ui.close();
        }
        if renamed && ui.button("Clear name").clicked() {
            chosen = Some(ArrangeAction::Rename { hwnd, name: None });
            ui.close();
        }
        ui.separator();
    }
    if ui.button("Close window").clicked() {
        chosen = Some(ArrangeAction::Close(hwnd));
        ui.close();
    }
    if let Some((place, count)) = place_in_row(group, desktops, member).filter(|(_, n)| *n > 1) {
        ui.menu_button("Taskbar place", |ui| {
            for (index, label) in place_labels(count, place).iter().enumerate() {
                if ui.selectable_label(index + 1 == place, label).clicked() {
                    chosen = take_place(group, desktops, member, index + 1);
                    ui.close();
                }
            }
        });
    }
    ui.menu_button("Move to desktop", |ui| {
        for desktop in desktops.iter().filter(|desktop| desktop.id != current) {
            if ui.button(&desktop.name).clicked() {
                chosen = Some(ArrangeAction::MoveToDesktop {
                    hwnd,
                    desktop: desktop.id.clone(),
                });
                ui.close();
            }
        }
    });
    chosen
}

/// The text of the "Same title" chip's tooltip.
const SAME_TITLE_HINT: &str = "Restores cannot tell these windows apart. Give one a name.";

/// One window, `width` wide: the monitor chip when it is not on the primary
/// monitor, its picture area (the live picture is put on top of it later)
/// with an amber "Same title" badge in its top right corner when another
/// window of the app has the same title (`can_rename` says whether
/// WindowNamer is on, so the badge can offer to rename), and its name
/// underneath, after its taskbar number. Without a picture the area shows
/// the name and desktop instead. Returns the card's response, the picture
/// area, whether the badge was clicked to rename, and the badges' rectangle.
pub(super) fn card(
    ui: &mut Ui,
    width: f32,
    window: &ArrangeWindow,
    desktop: &str,
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
    } = info;
    let label = window.label.as_str();
    let monitor = window.monitor.as_deref();
    let same_title = window.same_title.then_some(can_rename);
    let tokens = Tokens::get(ui.ctx());
    let sense = if leaving {
        Sense::hover()
    } else {
        Sense::click_and_drag()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width, CARD_HEIGHT), sense);
    let lit = !leaving && ((awake && response.hovered()) || is_cursor);
    let mut faded = ui.painter().clone();
    faded.multiply_opacity(fade);
    let painter = &faded;
    let look = card_look(&tokens, lit.then_some(tokens.hover_bg), window.attention);
    paint_look(painter, grown(rect, scale), 6, look);
    if let Some(monitor) = monitor {
        monitor_chip(painter, rect, monitor);
    }
    let area = grown(picture_area(rect), scale);
    let border = if lit { tokens.accent } else { tokens.border };
    painter.rect(
        area,
        tokens.radius_row,
        tokens.panel_bg,
        theme::line(ui.ctx(), border),
        StrokeKind::Inside,
    );
    let picture = area.shrink(1.0);
    if !has_picture {
        name_card(painter, picture, label, desktop);
    }
    let mut badge_list = Vec::new();
    if same_title.is_some() && !leaving {
        badge_list.push(Badge::SameTitle);
    }
    let shown = paint_badges(
        ui,
        painter,
        response.id,
        area,
        &badge_list,
        same_title.unwrap_or(false),
    );

    let name_top = area.bottom() + CARD_INSET;
    let chip = number_chip(painter, pos2(area.left(), name_top), NAME_HEIGHT, number);
    let name_left = chip.right() + NUMBER_GAP;
    if let Some(edit) = edit {
        let field = Rect::from_min_max(
            pos2(name_left, name_top),
            pos2(area.right(), name_top + NAME_HEIGHT),
        );
        edit.show(ui, field, 12.0);
    } else {
        let mut job = text::job(label, theme::regular(12.0), tokens.text_primary, None);
        job.wrap = TextWrapping {
            max_width: (area.right() - name_left).max(0.0),
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('\u{2026}'),
        };
        let galley = painter.layout_job(job);
        painter.galley(pos2(name_left, name_top), galley, tokens.text_primary);
    }
    (
        response.on_hover_cursor(egui::CursorIcon::Grab),
        picture,
        shown.renamed,
        shown.clear,
    )
}

/// What a card shows besides its window.
pub(super) struct CardInfo<'a> {
    /// The title is being edited: the box replaces the name.
    pub(super) edit: Option<&'a mut rename::Rename>,
    /// The window's place in Windows' taskbar row, from 1.
    pub(super) number: usize,
    pub(super) can_rename: bool,
    pub(super) has_picture: bool,
    /// The window is gone and its card fades out.
    pub(super) leaving: bool,
    /// The card's size and opacity; see `card_motion`.
    pub(super) motion: (f32, f32),
    pub(super) is_cursor: bool,
    /// The mouse has moved since the strip opened, so hover counts.
    pub(super) awake: bool,
}

/// The gap between the taskbar number and the name after it.
pub(super) const NUMBER_GAP: f32 = 4.0;

/// The window's place in Windows' taskbar row, as a small keycap at the
/// start of its name line. It sits below the picture, because DWM draws the
/// picture over anything painted on it, and the name starts after it, so it
/// stays readable on the narrowest card. Returns its rectangle.
pub(super) fn number_chip(painter: &egui::Painter, at: Pos2, height: f32, number: usize) -> Rect {
    let tokens = Tokens::get(painter.ctx());
    let galley = painter.layout_no_wrap(
        number.to_string(),
        theme::regular(12.0),
        tokens.text_secondary,
    );
    let chip = Rect::from_min_size(at, vec2((galley.size().x + 10.0).max(height), height));
    painter.rect(
        chip,
        tokens.radius_row,
        theme::with_alpha(tokens.panel_bg, 0.75),
        theme::line(painter.ctx(), tokens.border),
        StrokeKind::Inside,
    );
    painter.galley(
        chip.center() - galley.size() / 2.0,
        galley,
        tokens.text_secondary,
    );
    chip
}

/// What is drawn behind a card's picture: a fill and an outline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub fill: Option<Color32>,
    pub outline: Option<Color32>,
}

/// How a card is marked. `lit` is the view's own fill for the card under
/// the mouse or the keys. A window that wants attention gets an orange tint
/// and outline instead, like its orange taskbar button, a little stronger
/// while lit; a view draws its keyboard ring apart.
pub fn card_look(tokens: &Tokens, lit: Option<Color32>, attention: bool) -> Look {
    if attention {
        let tint = if lit.is_some() { 0.28 } else { 0.18 };
        Look {
            fill: Some(theme::with_alpha(tokens.warning, tint)),
            outline: Some(tokens.warning),
        }
    } else {
        Look {
            fill: lit,
            outline: None,
        }
    }
}

/// Paints a card's look in `rect`. The outline stays on the card's edge,
/// clear of the picture, which DWM draws over anything painted under it.
pub(super) fn paint_look(painter: &egui::Painter, rect: Rect, radius: u8, look: Look) {
    if let Some(fill) = look.fill {
        painter.rect_filled(rect, radius, fill);
    }
    if let Some(outline) = look.outline {
        painter.rect_stroke(
            rect,
            radius,
            theme::stroke(painter.ctx(), 1.5, outline),
            StrokeKind::Inside,
        );
    }
}

/// A badge on a card's picture: "Same title" (amber) or "Moved" (accent).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Badge {
    SameTitle,
    Moved,
}

impl Badge {
    pub(super) fn text(self) -> &'static str {
        match self {
            Self::SameTitle => "Same title",
            Self::Moved => "Moved",
        }
    }

    pub(super) fn icon(self) -> Icon {
        match self {
            Self::SameTitle => Icon::Copy,
            Self::Moved => Icon::ArrowRightLeft,
        }
    }

    pub(super) fn colour(self, tokens: &Tokens) -> Color32 {
        match self {
            Self::SameTitle => tokens.warning,
            Self::Moved => tokens.accent,
        }
    }

    /// The tooltip. An icon badge has no text, so its tooltip starts with
    /// the badge's name.
    pub(super) fn hint(self, can_rename: bool, icon_only: bool) -> String {
        let hint = match self {
            Self::SameTitle if can_rename => format!("{SAME_TITLE_HINT} Click to rename."),
            Self::SameTitle => SAME_TITLE_HINT.to_string(),
            Self::Moved => "This window is on another monitor than in your saved layout.".into(),
        };
        if icon_only {
            format!("{}. {hint}", self.text())
        } else {
            hint
        }
    }
}

/// The gap between a badge and the picture's top and right edges.
pub(super) const BADGE_INSET: f32 = 6.0;

pub(super) const BADGE_HEIGHT: f32 = 18.0;

pub(super) const BADGE_GAP: f32 = 4.0;

/// The space a text badge adds round its text, in all.
const BADGE_PAD: f32 = 12.0;

const BADGE_ICON: f32 = 12.0;

/// Where a picture's badges go, left to right and ending BADGE_INSET from
/// its right edge, for badges whose text is `text_widths` wide. They are
/// text badges when all of them fit in the picture's width less
/// 2 * BADGE_INSET, otherwise every one of them is an 18 x 18 icon badge.
/// Returns whether they are icons, and their rectangles.
pub(super) fn badge_rects(thumb: Rect, text_widths: &[f32]) -> (bool, Vec<Rect>) {
    let gaps = BADGE_GAP * text_widths.len().saturating_sub(1) as f32;
    let text_total: f32 = text_widths.iter().map(|width| width + BADGE_PAD).sum();
    let icons = text_total + gaps > thumb.width() - 2.0 * BADGE_INSET;
    let widths: Vec<f32> = text_widths
        .iter()
        .map(|width| {
            if icons {
                BADGE_HEIGHT
            } else {
                width + BADGE_PAD
            }
        })
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + gaps;
    let mut x = (thumb.right() - BADGE_INSET - total).max(thumb.left());
    let top = thumb.top() + BADGE_INSET;
    let rects = widths
        .iter()
        .map(|width| {
            let rect = Rect::from_min_size(pos2(x, top), vec2(*width, BADGE_HEIGHT));
            x += width + BADGE_GAP;
            rect
        })
        .collect();
    (icons, rects)
}

/// What `paint_badges` did.
pub(super) struct BadgesShown {
    /// The rectangle round all the badges, or NOTHING when there are none.
    pub(super) clear: Rect,
    /// The "Same title" badge was clicked and renaming is on.
    pub(super) renamed: bool,
}

/// Draws `badges` in the top right corner of a card's picture (`thumb`),
/// never in the title line: as text when they all fit, else as icons with
/// the text in a tooltip. "Same title" asks to rename when clicked, if
/// `can_rename`.
pub(super) fn paint_badges(
    ui: &Ui,
    painter: &egui::Painter,
    id: egui::Id,
    thumb: Rect,
    badges: &[Badge],
    can_rename: bool,
) -> BadgesShown {
    let tokens = Tokens::get(ui.ctx());
    let galleys: Vec<_> = badges
        .iter()
        .map(|badge| {
            painter.layout_no_wrap(
                badge.text().to_string(),
                theme::regular(12.0),
                badge.colour(&tokens),
            )
        })
        .collect();
    let widths: Vec<f32> = galleys.iter().map(|galley| galley.size().x).collect();
    let (icons_only, rects) = badge_rects(thumb, &widths);
    let mut shown = BadgesShown {
        clear: Rect::NOTHING,
        renamed: false,
    };
    for ((badge, galley), rect) in badges.iter().zip(galleys).zip(rects) {
        let colour = badge.colour(&tokens);
        painter.rect(
            rect,
            tokens.radius_row,
            tokens.window_bg,
            theme::line(ui.ctx(), colour),
            StrokeKind::Inside,
        );
        if icons_only {
            let glyph = Rect::from_center_size(rect.center(), Vec2::splat(BADGE_ICON));
            icons::paint(painter, glyph, badge.icon(), colour);
        } else {
            painter.galley(rect.center() - galley.size() / 2.0, galley, colour);
        }
        let renames = *badge == Badge::SameTitle && can_rename;
        let sense = if renames {
            Sense::click()
        } else {
            Sense::hover()
        };
        let mut response = ui
            .interact(rect, id.with(badge.text()), sense)
            .on_hover_text(badge.hint(can_rename, icons_only));
        if renames {
            response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
            shown.renamed |= response.clicked();
        }
        shown.clear = if shown.clear.is_positive() {
            shown.clear.union(rect)
        } else {
            rect
        };
    }
    shown
}

/// Where a card's picture goes, below its label line.
pub(super) fn picture_area(card: Rect) -> Rect {
    Rect::from_min_size(
        card.min + vec2(CARD_INSET, CARD_INSET + LABEL_LINE + CARD_INSET),
        vec2(card.width() - 2.0 * CARD_INSET, THUMB_HEIGHT),
    )
}

/// The monitor a window is on, left-aligned in the card's label line: the
/// hotkey chip's look at 12 px, with an opaque fill.
pub(super) fn monitor_chip(painter: &egui::Painter, card: Rect, text: &str) {
    let tokens = Tokens::get(painter.ctx());
    let galley = painter.layout_no_wrap(
        text.to_string(),
        theme::regular(12.0),
        tokens.text_secondary,
    );
    let chip = Rect::from_min_size(
        card.min + Vec2::splat(CARD_INSET),
        vec2(galley.size().x + 12.0, LABEL_LINE),
    );
    painter.rect(
        chip,
        tokens.radius_row,
        tokens.panel_bg,
        theme::line(painter.ctx(), tokens.border),
        StrokeKind::Inside,
    );
    painter.galley(
        chip.center() - galley.size() / 2.0,
        galley,
        tokens.text_secondary,
    );
}

/// The gap a dragged card would land in, as wide as that card.
pub(super) fn gap(ui: &mut Ui, width: f32) {
    let tokens = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(width, CARD_HEIGHT), Sense::hover());
    let area = picture_area(rect);
    ui.painter().rect(
        area,
        tokens.radius_row,
        theme::with_alpha(tokens.accent, 0.12),
        theme::line(ui.ctx(), tokens.accent),
        StrokeKind::Inside,
    );
}

/// An empty row, still a place to drop a window on.
pub(super) fn placeholder(ui: &mut Ui, text_line: &str) {
    let tokens = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(card_width(None), CARD_HEIGHT), Sense::hover());
    let area = picture_area(rect);
    ui.painter().rect(
        area,
        tokens.radius_row,
        egui::Color32::TRANSPARENT,
        theme::line(ui.ctx(), tokens.border),
        StrokeKind::Inside,
    );
    let galley = ui.painter().layout_job(text::job(
        text_line,
        theme::regular(12.0),
        tokens.text_secondary,
        None,
    ));
    ui.painter().galley(
        area.center() - galley.size() / 2.0,
        galley,
        tokens.text_secondary,
    );
}

/// Stands in for a picture DWM cannot give, and for all pictures while a menu
/// is open, since DWM would draw them over it.
pub(super) fn name_card(painter: &egui::Painter, area: Rect, label: &str, desktop: &str) {
    let tokens = Tokens::get(painter.ctx());
    let mut job = text::job(label, theme::semibold(15.0), tokens.text_primary, None);
    job.wrap = TextWrapping {
        max_width: area.width() - 16.0,
        max_rows: 2,
        break_anywhere: false,
        overflow_character: Some('\u{2026}'),
    };
    job.halign = egui::Align::Center;
    let title = painter.layout_job(job);
    let detail = painter.layout_job(text::job(
        desktop,
        theme::regular(12.0),
        tokens.text_secondary,
        None,
    ));
    let height = title.size().y
        + if desktop.is_empty() {
            0.0
        } else {
            4.0 + detail.size().y
        };
    let top = area.center().y - height / 2.0;
    painter.galley(
        pos2(area.center().x, top),
        title.clone(),
        tokens.text_primary,
    );
    if !desktop.is_empty() {
        painter.galley(
            pos2(
                area.center().x - detail.size().x / 2.0,
                top + title.size().y + 4.0,
            ),
            detail,
            tokens.text_secondary,
        );
    }
}
