use super::*;

impl Draw<'_> {
    pub(super) fn card(
        &mut self,
        ui: &mut Ui,
        shared: &mut Shared,
        frame: &mut Frame,
        member: usize,
        rect: Rect,
        alpha: f32,
    ) {
        let tokens = Tokens::get(ui.ctx());
        let window = &self.group.windows[member];
        let leaving = shared.is_leaving(window.hwnd);
        let (scale, fade) = shared.motion(window.hwnd);
        let alpha = alpha * fade;
        let response = ui.interact(
            rect,
            egui::Id::new(("arrange-card", window.hwnd)),
            item_sense(Item::Card(member), leaving),
        );
        let mut painter = ui.painter().clone();
        painter.multiply_opacity(alpha);
        let is_cursor = shared.cursor == Some(window.hwnd) && !shared.cursor_ghost && !leaving;
        let lit = if is_cursor {
            Some(tokens.selection_bg)
        } else if response.hovered() && shared.pointer.moved() && self.dragged.is_none() {
            Some(tokens.hover_bg)
        } else {
            None
        };
        paint_look(
            &painter,
            grown(rect, scale),
            4,
            card_look(&tokens, lit, window.attention),
        );
        if is_cursor {
            // Outside the picture, which DWM would draw over.
            painter.rect_stroke(
                rect.expand(3.0),
                tokens.radius_row + 2,
                theme::stroke(ui.ctx(), 2.0, tokens.accent),
                StrokeKind::Middle,
            );
        }
        let thumb = grown(
            Rect::from_min_size(
                rect.min + Vec2::splat(CARD_PAD),
                vec2(rect.width() - 2.0 * CARD_PAD, THUMB_HEIGHT),
            ),
            scale,
        );
        painter.rect(
            thumb,
            3,
            tokens.panel_bg,
            theme::line(ui.ctx(), tokens.border),
            StrokeKind::Inside,
        );
        let picture = thumb.shrink(1.0);
        let mut badges = Vec::new();
        if window.same_title && !leaving {
            badges.push(Badge::SameTitle);
        }
        if moved(window, self.monitors) {
            badges.push(Badge::Moved);
        }
        let shown = paint_badges(ui, &painter, response.id, thumb, &badges, self.can_rename);
        if shown.renamed {
            frame.actions.push(ArrangeAction::AskName(window.hwnd));
        }
        // The one live picture of a dragged window is the ghost's, so its
        // old place shows the name instead.
        let has_picture = !self.menu_open
            && self.dragged != Some(member)
            && thumbnail(shared, window.hwnd).is_some_and(|thumb| thumb.source_size().is_some());
        if has_picture {
            frame.pending.push(Pending {
                hwnd: window.hwnd,
                dest: picture,
                clip: ui.clip_rect(),
                opacity: (255.0 * alpha).round() as u8,
                clear: shown.clear,
                ghost: false,
            });
        } else {
            name_card(&painter, picture, &window.label, "");
        }

        let title_top = thumb.bottom() + TITLE_GAP;
        let right = rect.right() - CARD_PAD - 2.0;
        let number = number_chip(
            &painter,
            pos2(rect.left() + CARD_PAD + 2.0, title_top),
            TITLE,
            self.numbers[member],
        );
        let left = number.right() + NUMBER_GAP;
        // F2: the title is a text box, which owns the mouse on this card.
        if let Some(edit) = shared
            .rename
            .as_mut()
            .filter(|edit| edit.hwnd == window.hwnd && !edit.ghost)
        {
            let field = Rect::from_min_max(pos2(left, title_top), pos2(right, title_top + TITLE));
            edit.show(ui, field, 13.0);
            frame.cards.push((window.hwnd, window.monitor.is_some()));
            return;
        }
        let mut job = text::job(
            &window.label,
            theme::regular(13.0),
            tokens.text_primary,
            None,
        );
        job.wrap = TextWrapping {
            max_width: (right - left).max(0.0),
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('\u{2026}'),
        };
        let galley = painter.layout_job(job);
        let elided = galley.elided;
        painter.galley(
            pos2(left, title_top + (TITLE - galley.size().y) / 2.0),
            galley,
            tokens.text_primary,
        );

        let mut response = if elided {
            response.on_hover_text(&window.label)
        } else {
            response
        };
        if leaving {
            return;
        }
        if self.dragged.is_none() {
            response = response.on_hover_cursor(egui::CursorIcon::Grab);
        }
        self.interact(shared, frame, member, &response);
    }

    /// A stand-in in the primary column for a window on another monitor, at
    /// its taskbar place: a narrow name strip (a pill with an arrow toward
    /// that monitor and its first letter, the title written upward, the
    /// number), then the window's faded picture filling the rest. It cannot
    /// be dragged; a click picks the window's own card.
    pub(super) fn ghost_card(
        &mut self,
        ui: &mut Ui,
        shared: &mut Shared,
        frame: &mut Frame,
        member: usize,
        rect: Rect,
        alpha: f32,
    ) {
        let tokens = Tokens::get(ui.ctx());
        let window = &self.group.windows[member];
        let Some(monitor) = column_of(window, self.monitors).and_then(|m| self.monitors.get(m))
        else {
            return;
        };
        let left = self
            .monitors
            .iter()
            .find(|primary| primary.primary && primary.connected)
            .is_some_and(|primary| points_left(monitor, primary));
        let response = ui.interact(
            rect,
            egui::Id::new(("arrange-ghost", window.hwnd)),
            item_sense(Item::Ghost(member), false),
        );
        let mut painter = ui.painter().clone();
        // Its old place stays behind, faded, while it is dragged.
        let alpha = if self.dragged == Some(member) && self.dragged_ghost {
            alpha * DRAGGED
        } else {
            alpha
        };
        painter.multiply_opacity(alpha);
        ghost_box(ui, &painter, rect, window.attention);
        if shared.cursor == Some(window.hwnd) && shared.cursor_ghost {
            // Outside the picture, which DWM would draw over.
            painter.rect_stroke(
                rect.expand(3.0),
                6,
                theme::stroke(ui.ctx(), 2.0, tokens.accent),
                StrokeKind::Middle,
            );
        }

        let strip = Rect::from_min_size(rect.min, vec2(GHOST_STRIP, rect.height()));
        let x = strip.right() - 0.5;
        painter.extend(Shape::dashed_line(
            &[pos2(x, rect.top()), pos2(x, rect.bottom())],
            theme::line(ui.ctx(), tokens.border),
            4.0,
            3.0,
        ));

        let letter = painter.layout_no_wrap(
            monitor_letter(&monitor.label),
            theme::semibold(12.0),
            tokens.text_disabled,
        );
        let pill = Rect::from_min_size(
            pos2(strip.center().x - GHOST_PILL / 2.0, rect.top() + GHOST_TOP),
            vec2(
                GHOST_PILL,
                GHOST_PILL_PAD + GHOST_ARROW + letter.size().y + GHOST_PILL_PAD,
            ),
        );
        painter.rect_stroke(
            pill,
            7,
            theme::line(ui.ctx(), tokens.text_disabled),
            StrokeKind::Inside,
        );
        let arrow = Rect::from_min_size(
            pos2(
                pill.center().x - GHOST_ARROW / 2.0,
                pill.top() + GHOST_PILL_PAD,
            ),
            Vec2::splat(GHOST_ARROW),
        );
        let icon = if left {
            Icon::ArrowLeft
        } else {
            Icon::ArrowRight
        };
        icons::paint(&painter, arrow, icon, tokens.text_disabled);
        let letter_at = pos2(pill.center().x - letter.size().x / 2.0, arrow.bottom());
        painter.galley(letter_at, letter, tokens.text_disabled);

        let chip = ghost_chip(
            ui,
            &painter,
            strip.center().x,
            rect.bottom() - GHOST_BOTTOM,
            self.numbers[member],
        );
        let free = (chip.top() - GHOST_GAP) - (pill.bottom() + GHOST_GAP);
        if free > 0.0 {
            let mut job = text::job(
                &window.label,
                theme::regular(12.0),
                tokens.text_secondary,
                None,
            );
            job.wrap = TextWrapping {
                max_width: free,
                max_rows: 1,
                break_anywhere: true,
                overflow_character: Some('\u{2026}'),
            };
            let title = painter.layout_job(job);
            // Turned a quarter anticlockwise, it reads upward from its pivot,
            // its bottom left corner.
            let at = pos2(
                strip.center().x - title.size().y / 2.0,
                chip.top() - GHOST_GAP,
            );
            painter.add(
                egui::epaint::TextShape::new(at, title, tokens.text_secondary)
                    .with_angle(-std::f32::consts::FRAC_PI_2),
            );
        }

        let picture_box = Rect::from_min_max(pos2(strip.right(), rect.top()), rect.max).shrink(1.0);
        let editing = shared
            .rename
            .as_ref()
            .is_some_and(|edit| edit.hwnd == window.hwnd && edit.ghost);
        // The picture would be drawn over the text box.
        let has_picture = !self.menu_open
            && !editing
            && ghost_thumbnail(shared, window.hwnd)
                .is_some_and(|thumb| thumb.source_size().is_some());
        if has_picture {
            let source = shared.sources.get(&window.hwnd).map(|source| source.size());
            frame.pending.push(Pending {
                hwnd: window.hwnd,
                dest: source.map_or(picture_box, |source| cover(source, picture_box)),
                clip: picture_box.intersect(ui.clip_rect()),
                opacity: (255.0 * GHOST_OPACITY * alpha).round() as u8,
                clear: Rect::NOTHING,
                ghost: true,
            });
        } else {
            let mut faded = painter.clone();
            faded.multiply_opacity(GHOST_OPACITY);
            name_card(&faded, picture_box, &window.label, "");
        }

        // F2: the title is a text box, over the picture's place.
        if let Some(edit) = shared.rename.as_mut().filter(|_| editing) {
            let field = Rect::from_center_size(
                rect.center(),
                vec2((rect.width() - 8.0).max(0.0), TITLE + 4.0),
            );
            edit.show(ui, field, 13.0);
            return;
        }

        let response =
            response.on_hover_text(format!("{} \u{b7} on {}", window.label, monitor.label));
        let response = if self.dragged.is_none() {
            response.on_hover_cursor(egui::CursorIcon::Grab)
        } else {
            response
        };
        if response.hovered() {
            frame.hovered_ghost = Some(window.hwnd);
        }
        // A click opens the window, as on its real card.
        if response.clicked() {
            pick(shared, self.group, Some(Item::Ghost(member)));
            shared.rail_focus = None;
            frame.activate = Some(window.hwnd);
        }
        if response.middle_clicked() {
            frame.close.extend(middle_close(window.hwnd, true));
        }
        if response.drag_started() {
            if let Some(at) = response.interact_pointer_pos() {
                shared.drag = Some(Drag {
                    hwnd: window.hwnd,
                    grab: at - response.rect.min,
                    ghost: true,
                });
                pick(shared, self.group, Some(Item::Ghost(member)));
            }
        }
    }

    /// Clicks, drags, the menu and the hover peek of one card.
    pub(super) fn interact(
        &self,
        shared: &mut Shared,
        frame: &mut Frame,
        member: usize,
        response: &Response,
    ) {
        let window = &self.group.windows[member];
        frame.cards.push((window.hwnd, window.monitor.is_some()));
        if response.hovered() {
            frame.hovered = Some(window.hwnd);
        }
        if response.clicked() {
            frame.activate = Some(window.hwnd);
        }
        if response.middle_clicked() {
            frame.close.extend(middle_close(window.hwnd, false));
        }
        if response.drag_started() {
            if let Some(at) = response.interact_pointer_pos() {
                shared.drag = Some(Drag {
                    hwnd: window.hwnd,
                    grab: at - response.rect.min,
                    ghost: false,
                });
                pick(shared, self.group, Some(Item::Card(member)));
            }
        }
        let names = self.can_rename.then_some(window.renamed);
        response.context_menu(|ui| {
            if let Some(action) = card_menu(ui, self.group, member, self.desktops, names) {
                frame.actions.push(action);
            }
        });
    }
}

/// Whether a ghost's arrow points left: its monitor stands left of the
/// primary one.
pub(super) fn points_left(monitor: &ArrangeMonitor, primary: &ArrangeMonitor) -> bool {
    monitor.rect[0] + monitor.rect[2] < primary.rect[0] + primary.rect[2]
}

/// A picture of `source`'s shape that covers `area`, centred on it: the
/// parts that stick out are cut off.
pub(super) fn cover(source: Vec2, area: Rect) -> Rect {
    if source.x <= 0.0 || source.y <= 0.0 {
        return area;
    }
    let scale = (area.width() / source.x).max(area.height() / source.y);
    Rect::from_center_size(area.center(), source * scale)
}

/// The letter a ghost's pill shows for its monitor: the first of its name.
pub(super) fn monitor_letter(label: &str) -> String {
    label
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default()
}

/// A ghost's taskbar number in a dashed chip, centred on `centre_x` and
/// ending at `bottom`; returns its rectangle.
fn ghost_chip(ui: &Ui, painter: &egui::Painter, centre_x: f32, bottom: f32, number: usize) -> Rect {
    let tokens = Tokens::get(ui.ctx());
    let galley = painter.layout_no_wrap(
        number.to_string(),
        theme::regular(12.0),
        tokens.text_disabled,
    );
    let width = (galley.size().x + 4.0).max(GHOST_CHIP);
    let chip = Rect::from_min_max(
        pos2(centre_x - width / 2.0, bottom - GHOST_CHIP),
        pos2(centre_x + width / 2.0, bottom),
    );
    dashed_box(ui, painter, chip, false);
    painter.galley(
        chip.center() - galley.size() / 2.0,
        galley,
        tokens.text_disabled,
    );
    chip
}

/// A ghost's dashed outline. A window that wants attention gets the same
/// orange tint as its real card, and the dashes turn orange, so the mark
/// shows in the primary column too.
fn ghost_box(ui: &Ui, painter: &egui::Painter, rect: Rect, attention: bool) {
    let (fill, outline) = ghost_colours(&Tokens::get(ui.ctx()), attention);
    if let Some(fill) = fill {
        painter.rect_filled(rect, 4, fill);
    }
    dashed_outline(ui, painter, rect, outline);
}

/// A ghost's fill and dash colour.
pub(super) fn ghost_colours(tokens: &Tokens, attention: bool) -> (Option<Color32>, Color32) {
    let outline = if attention {
        tokens.warning
    } else {
        tokens.border
    };
    (card_look(tokens, None, attention).fill, outline)
}

/// A 1 px dashed outline, in the accent while a card is about to land in it.
pub(super) fn dashed_box(ui: &Ui, painter: &egui::Painter, rect: Rect, lit: bool) {
    let tokens = Tokens::get(ui.ctx());
    let colour = if lit { tokens.accent } else { tokens.border };
    dashed_outline(ui, painter, rect, colour);
}

fn dashed_outline(ui: &Ui, painter: &egui::Painter, rect: Rect, colour: Color32) {
    let stroke = theme::line(ui.ctx(), colour);
    let r = rect.shrink(0.5);
    let corners = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    painter.extend(Shape::dashed_line(&corners, stroke, 4.0, 3.0));
}
