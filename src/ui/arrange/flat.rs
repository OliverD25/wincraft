use super::*;

/// The group list, the view switch and, where there is room, how many
/// monitors, desktops and windows there are. Returns the group and the view
/// picked on this frame.
pub(super) fn toolbar(
    ui: &mut Ui,
    area: Rect,
    snapshot: &ArrangeSnapshot,
    group: &ArrangeGroup,
    view: ArrangeView,
    visible: &[bool],
) -> (Option<usize>, Option<ArrangeView>) {
    let tokens = Tokens::get(ui.ctx());
    let labels: Vec<&str> = snapshot.groups.iter().map(|g| g.label.as_str()).collect();
    let current = snapshot.groups.iter().position(|g| g.key == group.key);
    let shown = match view {
        ArrangeView::ByMonitor => 0,
        _ => 1,
    };
    let (picked, switched, right) = ui
        .scope_builder(UiBuilder::new().max_rect(area), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(12.0, 0.0);
                // App names such as "Chrome · Google Gemini WebAPP" need more
                // than the normal width; the toolbar has room to the right.
                let max = (area.width() * 0.4).min(480.0);
                let picked = choice::dropdown_fit(ui, "arrange-group", &labels, current, max)
                    .filter(|index| Some(*index) != current);
                let switched = choice::segmented(ui, &VIEWS, Some(shown)).map(|index| {
                    if index == 0 {
                        ArrangeView::ByMonitor
                    } else {
                        ArrangeView::Flat
                    }
                });
                (picked, switched, ui.min_rect().right())
            })
            .inner
        })
        .inner;
    let connected = snapshot.monitors.iter().filter(|m| m.connected).count();
    let shown = visible.iter().filter(|v| **v).count();
    // A strip limited to one monitor counts that monitor only.
    let monitors = if shown < group.windows.len() {
        1
    } else {
        connected
    };
    let line = summary(monitors, snapshot.desktops.len(), shown);
    let galley = ui
        .painter()
        .layout_no_wrap(line, theme::regular(13.0), tokens.text_secondary);
    // While the footer is hidden, the toolbar ends with the way to bring it
    // back.
    let mut end = area.right();
    if view == ArrangeView::ByMonitor && !snapshot.hints {
        let used = by_monitor::paint_toolbar_hint(ui.painter(), &tokens, end, area.top() + 16.0);
        end -= used + 16.0;
    }
    if right + 24.0 + galley.size().x <= end {
        ui.painter().galley(
            pos2(
                end - galley.size().x,
                area.top() + (32.0 - galley.size().y) / 2.0,
            ),
            galley,
            tokens.text_secondary,
        );
    }
    (picked, switched)
}

/// "3 monitors · 6 desktops · 27 windows". Without the desktop list there
/// is still the one desktop.
pub(super) fn summary(monitors: usize, desktops: usize, windows: usize) -> String {
    let count = |n: usize, one: &str| {
        if n == 1 {
            format!("1 {one}")
        } else {
            format!("{n} {one}s")
        }
    };
    [
        count(monitors.max(1), "monitor"),
        count(desktops.max(1), "desktop"),
        count(windows, "window"),
    ]
    .join(" \u{b7} ")
}

/// The view as before 0.13: one wrapping row of cards per desktop.
pub(super) fn flat(
    ui: &mut Ui,
    shared: &mut Shared,
    group: &ArrangeGroup,
    desktops: &[ArrangeDesktop],
    content: Rect,
    menu_open: bool,
) -> Frame {
    let tokens = Tokens::get(ui.ctx());
    let visible = shared.visible(group);
    let others_count = others::count(group, desktops, &visible);
    let rows = shown_rows(group, desktops, shared.others.shown(others_count), &visible);
    let ctx = ui.ctx().clone();
    let can_rename = shared.snapshot.as_ref().is_some_and(|s| s.can_rename);
    let note = shared.snapshot.as_ref().is_some_and(|s| s.keeps_order);
    let pointer = ctx.pointer_latest_pos();
    let mut frame = Frame::default();

    let numbers = taskbar_numbers(group, desktops);
    let flat: Vec<usize> = rows.iter().flat_map(|row| row.members.clone()).collect();
    let widths = shared.widths(group, card_width);
    if shared
        .cursor
        .is_none_or(|hwnd| !flat.iter().any(|m| group.windows[*m].hwnd == hwnd))
    {
        shared.cursor = flat.first().map(|m| group.windows[*m].hwnd);
    }

    // The drag: follow the pointer until the button comes up, then drop.
    let dragged = shared.drag.as_ref().and_then(|drag| {
        group
            .windows
            .iter()
            .position(|window| window.hwnd == drag.hwnd)
    });
    if dragged.is_none() {
        shared.drag = None;
    }
    let ctrl = ctx.input(|i| i.modifiers.ctrl);
    let target = match (dragged, pointer) {
        (Some(_), Some(pointer)) => drop_target(pointer, &shared.row_rects, &shared.card_rects),
        _ => None,
    };
    // With Ctrl the drop only changes the number: the card under the
    // pointer says which.
    let ctrl_card = match (dragged, pointer, ctrl) {
        (Some(member), Some(pointer), true) => {
            ctrl_target(pointer, group.windows[member].hwnd, &shared.card_hits)
        }
        _ => None,
    };
    if let Some(member) = dragged {
        if !ctx.input(|i| i.pointer.primary_down()) {
            if ctrl {
                frame
                    .actions
                    .extend(ctrl_card.and_then(|card| ctrl_drop(group, desktops, member, card)));
            } else if let Some(target) = target {
                frame
                    .actions
                    .extend(drop_actions(group, &rows, member, target));
            }
            shared.drag = None;
        }
    }
    let dragging = shared.drag.is_some();
    let gap_width = dragged.map_or(card_width(None), |member| widths[member]);
    // The gap stays where the card was while Ctrl is held: nothing moves.
    let home = dragged.and_then(|member| {
        rows.iter().enumerate().find_map(|(r, row)| {
            row.members
                .iter()
                .position(|m| *m == member)
                .map(|at| (r, at))
        })
    });
    let landing = if ctrl { home } else { target };
    let layout = preview(
        &rows,
        dragged.filter(|_| dragging),
        landing.filter(|_| dragging),
    );

    let mut row_rects: Vec<Rect> = Vec::new();
    let mut card_rects: Vec<Vec<Rect>> = Vec::new();
    let mut hits: Vec<(isize, Rect)> = Vec::new();
    let footer = if note && rows.len() > 1 { FOOTER } else { 0.0 };
    let rows_area = Rect::from_min_max(content.min, pos2(content.max.x, content.max.y - footer));
    frame.area = rows_area;

    ui.scope_builder(UiBuilder::new().max_rect(rows_area), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("arrange-rows")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(CARD_GAP, 0.0);
                for (row_index, row) in rows.iter().enumerate() {
                    if row_index > 0 {
                        ui.add_space(ROW_GAP);
                    }
                    let row_top = ui.cursor().min.y;
                    text::single(ui, text::section_job(&row.title, tokens.text_secondary));
                    ui.add_space(HEADER_GAP);
                    let mut cards_here: Vec<Rect> = Vec::new();
                    // Rows wrap rather than scroll sideways, so the one scroll
                    // area left is the vertical one and takes the wheel.
                    ui.scope(|ui| {
                        if layout[row_index].is_empty() {
                            placeholder(ui, "No windows on this desktop");
                            return;
                        }
                        let slot_widths: Vec<f32> = layout[row_index]
                            .iter()
                            .map(|slot| match slot {
                                Slot::Window(member) => widths[*member],
                                Slot::Gap => gap_width,
                            })
                            .collect();
                        let lines = wrap(&slot_widths, ui.available_width());
                        for (line_index, line) in lines.into_iter().enumerate() {
                            if line_index > 0 {
                                ui.add_space(CARD_GAP);
                            }
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = vec2(CARD_GAP, 0.0);
                                for slot in &layout[row_index][line] {
                                    let Slot::Window(member) = *slot else {
                                        gap(ui, gap_width);
                                        continue;
                                    };
                                    let window = &group.windows[member];
                                    let desktop_name = desktop_name(desktops, &window.desktop);
                                    let has_picture = !menu_open
                                        && thumbnail(shared, window.hwnd)
                                            .is_some_and(|thumb| thumb.source_size().is_some());
                                    let is_cursor = shared.cursor == Some(window.hwnd);
                                    let leaving = shared.is_leaving(window.hwnd);
                                    let editing = shared
                                        .rename
                                        .as_ref()
                                        .is_some_and(|edit| edit.hwnd == window.hwnd);
                                    let motion = shared.motion(window.hwnd);
                                    let waiting = shared.waiting_note(window.hwnd);
                                    let animate = shared.animate;
                                    let (response, picture, rename, clear) = card(
                                        ui,
                                        widths[member],
                                        window,
                                        desktop_name,
                                        CardInfo {
                                            number: numbers[member],
                                            can_rename,
                                            has_picture,
                                            is_cursor,
                                            leaving,
                                            motion,
                                            awake: shared.pointer.moved(),
                                            slides: &mut shared.slides,
                                            animate,
                                            waiting,
                                            edit: shared
                                                .rename
                                                .as_mut()
                                                .filter(|edit| edit.hwnd == window.hwnd),
                                        },
                                    );
                                    if rename {
                                        frame.actions.push(ArrangeAction::AskName(window.hwnd));
                                    }
                                    cards_here.push(response.rect);
                                    hits.push((window.hwnd, response.rect));
                                    frame.cards.push((window.hwnd, window.monitor.is_some()));
                                    if response.hovered() {
                                        frame.hovered = Some(window.hwnd);
                                    }
                                    if has_picture {
                                        frame.pending.push(Pending {
                                            hwnd: window.hwnd,
                                            dest: picture,
                                            clip: ui.clip_rect(),
                                            opacity: (255.0 * motion.1).round() as u8,
                                            clear,
                                            ghost: false,
                                        });
                                    }
                                    if leaving || editing {
                                        continue;
                                    }
                                    if response.middle_clicked() {
                                        frame.close.extend(middle_close(window.hwnd, false));
                                    }
                                    if response.clicked() {
                                        frame.activate = Some(window.hwnd);
                                    }
                                    if response.drag_started() {
                                        if let Some(at) = response.interact_pointer_pos() {
                                            shared.drag = Some(Drag {
                                                hwnd: window.hwnd,
                                                grab: at - response.rect.min,
                                                ghost: false,
                                            });
                                            shared.cursor = Some(window.hwnd);
                                        }
                                    }
                                    response.context_menu(|ui| {
                                        let names = can_rename.then_some(window.renamed);
                                        if let Some(action) =
                                            card_menu(ui, group, member, desktops, names)
                                        {
                                            frame.actions.push(action);
                                        }
                                    });
                                }
                            });
                        }
                    });
                    card_rects.push(cards_here);
                    row_rects.push(Rect::from_min_max(
                        pos2(rows_area.min.x, row_top),
                        pos2(rows_area.max.x, ui.cursor().min.y),
                    ));
                    if row_index == 0 && others_count > 0 {
                        let (line, _) = ui.allocate_exact_size(
                            vec2(rows_area.width(), others::LINK_HEIGHT),
                            Sense::hover(),
                        );
                        others_link(ui, shared, line, others_count);
                    }
                }
            });
    });
    shared.row_rects = row_rects;
    shared.card_rects = card_rects;
    shared.card_hits = hits;

    // The dragged card rides above everything, following the pointer.
    if let (Some(drag), Some(member), Some(pointer)) = (shared.drag.as_ref(), dragged, pointer) {
        let rect = Rect::from_min_size(pointer - drag.grab, vec2(widths[member], CARD_HEIGHT));
        let window = &group.windows[member];
        let layer = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("arrange-drag"),
        ));
        layer.rect_filled(rect, tokens.radius_tile, tokens.hover_bg);
        if let Some(monitor) = &window.monitor {
            monitor_chip(&layer, rect, monitor);
        }
        float_picture(
            &layer,
            &mut frame,
            window.hwnd,
            picture_area(rect),
            menu_open,
        );
        frame.floating = Some(rect);
        if ctrl {
            let place = ctrl_card.and_then(|card| ctrl_place(group, desktops, member, card));
            let outlined = ctrl_card
                .filter(|_| place.is_some())
                .and_then(|card| hit_rect(&shared.card_hits, card));
            let hint = ctrl_mark(&ctx, rect, outlined, &ctrl_hint(place));
            frame.floating = Some(rect.union(hint));
        }
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    frame.dragged = dragged.map(|m| group.windows[m].hwnd);

    if footer > 0.0 {
        let job = text::job(
            "Order on other desktops applies at the next restore.",
            theme::regular(12.0),
            tokens.text_secondary,
            None,
        );
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(
            pos2(content.min.x, content.max.y - galley.size().y),
            galley,
            tokens.text_secondary,
        );
    }

    if !menu_open && !dragging && shared.rename.is_none() {
        let Frame {
            actions, activate, ..
        } = &mut frame;
        keyboard(&ctx, shared, group, desktops, &rows, actions, activate);
    }
    frame
}

/// The line under this desktop's windows that opens and closes the other
/// desktops. A card dragged onto it opens them after a short rest, so it
/// can be dropped there. `line` has `others::LINK_HEIGHT`, the gap on top.
pub(super) fn others_link(ui: &mut Ui, shared: &mut Shared, line: Rect, count: usize) {
    let shown = shared.others.shown(count);
    let Some(text) = others::label(count, shown) else {
        return;
    };
    let ctx = ui.ctx().clone();
    let tokens = Tokens::get(&ctx);
    let galley = ui
        .painter()
        .layout_no_wrap(text, theme::regular(12.0), tokens.text_secondary);
    let arrow = 10.0;
    let hit = Rect::from_min_size(line.min, vec2(galley.size().x + 6.0 + arrow, line.height()));
    let response = ui
        .interact(hit, egui::Id::new("arrange-others"), Sense::click())
        .on_hover_text("Press D to show or hide them.");
    let dragging = shared.drag.is_some();
    let over = ctx.pointer_latest_pos().is_some_and(|at| hit.contains(at));
    if response.clicked() {
        shared.others.toggle();
        ctx.request_repaint();
    }
    if let Some(wait) = shared.others.drag_over(Instant::now(), dragging && over) {
        ctx.request_repaint_after(wait);
    }
    if !dragging && response.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let colour = if over {
        tokens.accent
    } else {
        tokens.text_secondary
    };
    let centre = line.bottom() - (line.height() - 6.0) / 2.0;
    let galley_pos = pos2(line.left(), centre - galley.size().y / 2.0);
    ui.painter().galley(galley_pos, galley.clone(), colour);
    // A triangle drawn here, as the fonts may not have one.
    let tip = pos2(line.left() + galley.size().x + 6.0 + arrow / 2.0, centre);
    let (half, rise) = (3.5, 2.0);
    let points = if shown {
        vec![
            pos2(tip.x - half, tip.y + rise),
            pos2(tip.x + half, tip.y + rise),
            pos2(tip.x, tip.y - rise),
        ]
    } else {
        vec![
            pos2(tip.x - half, tip.y - rise),
            pos2(tip.x + half, tip.y - rise),
            pos2(tip.x, tip.y + rise),
        ]
    };
    ui.painter().add(egui::Shape::convex_polygon(
        points,
        colour,
        egui::Stroke::NONE,
    ));
}

pub(super) fn hit_rect(cards: &[(isize, Rect)], hwnd: isize) -> Option<Rect> {
    cards
        .iter()
        .find(|(card, _)| *card == hwnd)
        .map(|(_, rect)| *rect)
}

/// What a drag with Ctrl held shows: an accent outline round the card that
/// gives its number (`outlined`) and a small hint above the dragged card, or
/// below it when there is no room. Returns the hint's rectangle, so the
/// pictures under it can be hidden: DWM draws them over anything.
pub(super) fn ctrl_mark(
    ctx: &egui::Context,
    ghost: Rect,
    outlined: Option<Rect>,
    hint: &str,
) -> Rect {
    let tokens = Tokens::get(ctx);
    let layer = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("arrange-drag-hint"),
    ));
    if let Some(card) = outlined {
        layer.rect_stroke(
            card,
            tokens.radius_tile,
            theme::stroke(ctx, 2.0, tokens.accent),
            StrokeKind::Inside,
        );
    }
    let galley = layer.layout_no_wrap(hint.to_string(), theme::regular(12.0), tokens.text_primary);
    let size = galley.size() + vec2(16.0, 8.0);
    let room = ctx.content_rect();
    let above = ghost.top() - size.y - 4.0;
    let top = if above >= room.top() {
        above
    } else {
        ghost.bottom() + 4.0
    };
    let left = ghost.left().min(room.right() - size.x).max(room.left());
    let chip = Rect::from_min_size(pos2(left, top), size);
    layer.rect(
        chip,
        tokens.radius_row,
        tokens.panel_bg,
        theme::line(ctx, tokens.accent),
        StrokeKind::Inside,
    );
    layer.galley(
        chip.center() - galley.size() / 2.0,
        galley,
        tokens.text_primary,
    );
    chip
}

/// The live picture of the dragged window, slightly see-through inside an
/// accent outline, drawn on the layer above everything. Both views use it
/// for the card that follows the pointer.
pub(super) fn float_picture(
    layer: &egui::Painter,
    frame: &mut Frame,
    hwnd: isize,
    area: Rect,
    menu_open: bool,
) {
    let tokens = Tokens::get(layer.ctx());
    layer.rect(
        area,
        tokens.radius_row,
        tokens.panel_bg,
        theme::line(layer.ctx(), tokens.accent),
        StrokeKind::Inside,
    );
    if !menu_open {
        frame.pending.push(Pending {
            hwnd,
            dest: area.shrink(1.0),
            clip: Rect::EVERYTHING,
            opacity: 204,
            clear: Rect::NOTHING,
            ghost: false,
        });
    }
}

/// The card a test scene asked to peek at, from the cards in their order on
/// screen, each with whether it has a monitor chip.
pub(super) fn forced_card(cards: &[(isize, bool)], card: PeekCard) -> Option<isize> {
    match card {
        PeekCard::Index(index) => cards.get(index).map(|(hwnd, _)| *hwnd),
        PeekCard::Chip => cards
            .iter()
            .find(|(_, chip)| *chip)
            .or(cards.first())
            .map(|(hwnd, _)| *hwnd),
    }
}

fn desktop_name<'a>(desktops: &'a [ArrangeDesktop], id: &str) -> &'a str {
    desktops
        .iter()
        .find(|desktop| desktop.id == id)
        .map(|desktop| desktop.name.as_str())
        .unwrap_or("")
}
