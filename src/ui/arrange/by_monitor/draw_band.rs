use super::*;

impl Draw<'_> {
    pub(super) fn band(
        &mut self,
        ui: &mut Ui,
        shared: &mut Shared,
        frame: &mut Frame,
        b: usize,
        band: &Band,
        width: f32,
    ) {
        let tokens = Tokens::get(ui.ctx());
        let alpha = if band.current { 1.0 } else { NOT_CURRENT };
        let tracks: Vec<Vec<f32>> = self
            .model
            .cols
            .iter()
            .zip(&self.open)
            .map(|(cols, open)| row_tracks(cols, open, width))
            .collect();
        let (header, _) =
            ui.allocate_exact_size(vec2(width, 1.0 + BAND_TOP + BAND_HEADER), Sense::hover());
        let mut painter = ui.painter().clone();
        painter.multiply_opacity(alpha);
        painter.hline(
            header.x_range(),
            header.top(),
            theme::line(ui.ctx(), tokens.border),
        );
        let title_colour = if band.current {
            tokens.accent
        } else {
            tokens.text_primary
        };
        let title = painter.layout_no_wrap(band.title.clone(), theme::semibold(13.0), title_colour);
        let meta = painter.layout_no_wrap(band.meta(), theme::regular(12.0), tokens.text_disabled);
        let centre = header.top() + 1.0 + BAND_TOP + BAND_HEADER / 2.0;
        let title_width = title.size().x;
        painter.galley(
            pos2(header.left() + SIDE, centre - title.size().y / 2.0),
            title,
            title_colour,
        );
        painter.galley(
            pos2(
                header.left() + SIDE + title_width + 10.0,
                centre - meta.size().y / 2.0,
            ),
            meta,
            tokens.text_disabled,
        );

        if band.count() == 0 {
            let (rest, _) = ui.allocate_exact_size(vec2(width, BAND_TOP), Sense::hover());
            let whole = header.union(rest);
            if self.target.is_some_and(|spot| spot.band == b) {
                painter.hline(
                    whole.x_range(),
                    whole.top() + 1.0,
                    theme::stroke(ui.ctx(), 2.0, tokens.accent),
                );
            }
            self.zones.push(Zone {
                band: b,
                cell: None,
                rect: whole,
                open: true,
                cards: Vec::new(),
            });
            return;
        }

        ui.add_space(HEADER_GAP);
        for (r, row_tracks) in tracks.iter().enumerate() {
            if r > 0 {
                ui.add_space(ROW_GAP);
            }
            let height = row_height(band, r, &self.model.cols[r], row_tracks, &self.model.widths);
            let needed = 2.0 * SIDE
                + row_tracks.iter().sum::<f32>()
                + COLUMN_GAP * row_tracks.len().saturating_sub(1) as f32;
            let at = |rect: Rect| RowAt {
                band_index: b,
                row: r,
                band,
                rect,
                tracks: row_tracks,
                alpha,
            };
            if needed <= width + 0.5 {
                let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
                self.row(ui, shared, frame, at(rect));
            } else {
                // The one sideways scroll the view has: columns at their
                // narrowest that still do not fit.
                egui::ScrollArea::horizontal()
                    .id_salt(("arrange-band-row", b, r))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(vec2(needed, height), Sense::hover());
                        self.row(ui, shared, frame, at(rect));
                    });
            }
        }
        ui.add_space(BAND_BOTTOM);
    }

    pub(super) fn row(&mut self, ui: &mut Ui, shared: &mut Shared, frame: &mut Frame, at: RowAt) {
        let RowAt {
            band_index: b,
            row: r,
            band,
            rect,
            tracks,
            alpha,
        } = at;
        let tokens = Tokens::get(ui.ctx());
        let mut x = rect.left() + SIDE;
        for (c, track) in tracks.iter().enumerate() {
            let column = Rect::from_min_size(pos2(x, rect.top()), vec2(*track, rect.height()));
            x += track + COLUMN_GAP;
            let monitor = &self.monitors[self.model.rows[r][c]];
            let cell = &band.cells[r][c];
            let flow = band.flow((r, c));
            let mut painter = ui.painter().clone();
            painter.multiply_opacity(alpha);
            if self.model.cols[r][c].rail {
                let lit = self
                    .target
                    .is_some_and(|spot| spot.band == b && spot.cell == (r, c))
                    || self.rail_focus == Some(self.model.rows[r][c]);
                rail(ui, &painter, column, monitor, self.open[r][c], lit);
                self.zones.push(Zone {
                    band: b,
                    cell: Some((r, c)),
                    rect: column,
                    open: true,
                    cards: Vec::new(),
                });
                continue;
            }
            painter.vline(
                column.left(),
                column.y_range(),
                theme::line(ui.ctx(), tokens.border),
            );
            let inner = Rect::from_min_max(pos2(column.left() + INSET, column.top()), column.max);
            label_row(
                ui,
                &painter,
                Rect::from_min_size(inner.min, vec2(inner.width(), LABEL)),
                monitor,
                cell.len(),
            );

            let top = inner.top() + LABEL + LABEL_GAP;
            let widths = flow_widths(&flow, &self.model.widths);
            let mut placed: Vec<(Item, Rect)> = Vec::new();
            for (line, items) in cell_lines(&widths, *track).into_iter().enumerate() {
                let mut x = inner.left();
                for at in items {
                    let min = pos2(x, top + line as f32 * (CARD_HEIGHT + CARD_GAP));
                    x += widths[at] + CARD_GAP;
                    placed.push((
                        flow[at],
                        Rect::from_min_size(min, vec2(widths[at], CARD_HEIGHT)),
                    ));
                }
            }
            let cards: Vec<(usize, Rect)> = placed
                .iter()
                .filter_map(|(item, rect)| match item {
                    Item::Card(member) => Some((*member, *rect)),
                    Item::Ghost(_) => None,
                })
                .collect();
            for (member, card_rect) in &cards {
                self.hits
                    .push((self.group.windows[*member].hwnd, *card_rect));
            }
            self.items.extend(placed.iter().copied());
            for (member, card_rect) in cards.clone() {
                let mut card_alpha = alpha;
                if !monitor.connected {
                    card_alpha *= DISCONNECTED;
                }
                if self.dragged == Some(member) && !self.dragged_ghost {
                    card_alpha *= DRAGGED;
                }
                self.card(ui, shared, frame, member, card_rect, card_alpha);
            }
            for (item, ghost_rect) in &placed {
                if let Item::Ghost(member) = item {
                    self.ghost_card(ui, shared, frame, *member, *ghost_rect, alpha);
                }
            }
            let target_here = self
                .target
                .filter(|spot| spot.band == b && spot.cell == (r, c));
            let others: Vec<Rect> = placed
                .iter()
                .filter(|(item, _)| Some(item.member()) != self.dragged)
                .map(|(_, rect)| *rect)
                .collect();
            if flow.is_empty() && monitor.connected {
                let empty =
                    Rect::from_min_size(pos2(inner.left(), top), vec2(inner.width(), EMPTY));
                drop_box(
                    ui,
                    &painter,
                    empty,
                    target_here.is_some(),
                    "Drop a window here to move it to this monitor",
                );
            }
            if let Some(spot) = target_here {
                insertion_bar(ui, &others, spot.index, pos2(inner.left(), top));
            }
            self.zones.push(Zone {
                band: b,
                cell: Some((r, c)),
                rect: column,
                open: monitor.connected,
                cards: placed
                    .iter()
                    .map(|(item, rect)| (item.member(), *rect))
                    .collect(),
            });
        }
    }
}

/// A column's label: the monitor icon, its name, its resolution, a
/// "Primary" chip and, at the right, how many windows it has. The friendly
/// name, when the monitor gives one, is the tooltip.
fn label_row(
    ui: &mut Ui,
    painter: &egui::Painter,
    rect: Rect,
    monitor: &ArrangeMonitor,
    count: usize,
) {
    let tokens = Tokens::get(ui.ctx());
    let painter = painter.with_clip_rect(rect);
    let centre = rect.center().y;
    icons::paint(
        &painter,
        Rect::from_center_size(pos2(rect.left() + 6.0, centre), Vec2::splat(12.0)),
        Icon::Monitor,
        tokens.text_disabled,
    );
    let mut x = rect.left() + 12.0 + 8.0;
    let name = painter.layout_no_wrap(
        monitor.label.clone(),
        theme::semibold(12.0),
        tokens.text_secondary,
    );
    let name_width = name.size().x;
    painter.galley(
        pos2(x, centre - name.size().y / 2.0),
        name,
        tokens.text_secondary,
    );
    x += name_width + 8.0;
    let detail = if monitor.connected {
        format!(
            "{}\u{d7}{}",
            monitor.rect[2] - monitor.rect[0],
            monitor.rect[3] - monitor.rect[1]
        )
    } else {
        "(disconnected)".to_string()
    };
    let detail = painter.layout_no_wrap(detail, theme::regular(12.0), tokens.text_disabled);
    let detail_width = detail.size().x;
    painter.galley(
        pos2(x, centre - detail.size().y / 2.0),
        detail,
        tokens.text_disabled,
    );
    x += detail_width + 8.0;
    if monitor.primary {
        let chip = painter.layout_no_wrap(
            "Primary".to_string(),
            theme::regular(12.0),
            tokens.text_disabled,
        );
        let chip_rect = Rect::from_min_size(
            pos2(x, centre - LABEL / 2.0),
            vec2(chip.size().x + 12.0, LABEL),
        );
        painter.rect_stroke(
            chip_rect,
            tokens.radius_row,
            theme::line(ui.ctx(), tokens.border),
            StrokeKind::Inside,
        );
        painter.galley(
            chip_rect.center() - chip.size() / 2.0,
            chip,
            tokens.text_disabled,
        );
    }
    if count > 0 {
        let count = painter.layout_no_wrap(
            count.to_string(),
            theme::regular(12.0),
            tokens.text_disabled,
        );
        painter.galley(
            pos2(rect.right() - count.size().x, centre - count.size().y / 2.0),
            count,
            tokens.text_disabled,
        );
    }
    if !monitor.name.is_empty() && monitor.name != monitor.label {
        ui.interact(
            rect,
            egui::Id::new(("arrange-monitor", monitor.device.as_str())),
            Sense::hover(),
        )
        .on_hover_text(&monitor.name);
    }
}
