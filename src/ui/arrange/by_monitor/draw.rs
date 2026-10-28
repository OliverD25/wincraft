use super::*;

/// One row of monitor columns in a band, laid out in `rect`.
pub(super) struct RowAt<'a> {
    pub(super) band_index: usize,
    pub(super) row: usize,
    pub(super) band: &'a Band,
    pub(super) rect: Rect,
    pub(super) tracks: &'a [f32],
    pub(super) alpha: f32,
}

/// What drawing a frame of the view needs besides the shared state.
pub(super) struct Draw<'a> {
    pub(super) group: &'a ArrangeGroup,
    pub(super) monitors: &'a [ArrangeMonitor],
    pub(super) desktops: &'a [ArrangeDesktop],
    pub(super) model: &'a Model,
    pub(super) menu_open: bool,
    pub(super) can_rename: bool,
    /// Each window's place in Windows' taskbar row, indexed like the group.
    pub(super) numbers: Vec<usize>,
    pub(super) dragged: Option<usize>,
    /// The dragged window was picked up by its ghost.
    pub(super) dragged_ghost: bool,
    pub(super) target: Option<Spot>,
    /// How far each rail is unfolded, shaped like `rows`.
    pub(super) open: Vec<Vec<f32>>,
    /// The monitor whose rail the keyboard is on.
    pub(super) rail_focus: Option<usize>,
    pub(super) zones: Vec<Zone>,
    /// Every card drawn, with its window handle, for a Ctrl drop.
    pub(super) hits: Vec<(isize, Rect)>,
    /// Every card and ghost drawn, for ↑ and ↓.
    pub(super) items: Vec<(Item, Rect)>,
}

pub fn body(
    ui: &mut Ui,
    shared: &mut Shared,
    group: &ArrangeGroup,
    snapshot: &ArrangeSnapshot,
    panel: Rect,
    menu_open: bool,
) -> Frame {
    let ctx = ui.ctx().clone();
    let visible = visible_flags(group, &snapshot.monitors, shared.only_monitor);
    let others_count = others::count(group, &snapshot.desktops, &visible);
    let shown = shared.others.shown(others_count);
    let widths = shared.widths(group, card_width);
    let scope = Scope {
        only: shared.only_monitor,
        taskbars: shared.taskbars,
    };
    let model = Model::new(group, snapshot, &widths, shown, scope);
    let mut frame = Frame::default();
    let items = walk_items(&model.bands);
    let member_of = |hwnd: isize| group.windows.iter().position(|w| w.hwnd == hwnd);
    let current = shared.cursor.and_then(member_of).map(|member| {
        if shared.cursor_ghost {
            Item::Ghost(member)
        } else {
            Item::Card(member)
        }
    });
    if current.is_none_or(|item| !items.contains(&item)) {
        let first = walk(&model.bands)
            .first()
            .map(|member| Item::Card(*member))
            .or(items.first().copied());
        pick(shared, group, first);
    }

    let dragged = shared.drag.as_ref().and_then(|drag| member_of(drag.hwnd));
    if dragged.is_none() {
        shared.drag = None;
    }
    let dragged_ghost = shared.drag.as_ref().is_some_and(|drag| drag.ghost);
    let home = |member: usize| {
        locate(&model.bands, member)
            .map(|spot| spot.cell)
            .filter(|(r, c)| snapshot.monitors[model.rows[*r][*c]].connected)
            .unwrap_or_else(|| primary_cell(&snapshot.monitors, &model.rows))
    };
    let ctrl = ctx.input(|i| i.modifiers.ctrl);
    let target = match (dragged, ctx.pointer_latest_pos()) {
        (Some(member), Some(pointer)) => drop_spot(pointer, &shared.zones, member, home(member)),
        _ => None,
    };
    // With Ctrl the drop only changes the number: the card under the
    // pointer says which, and the window stays on its monitor and desktop.
    let ctrl_card = match (dragged, ctx.pointer_latest_pos(), ctrl) {
        (Some(member), Some(pointer), true) => {
            ctrl_target(pointer, group.windows[member].hwnd, &shared.card_hits)
        }
        _ => None,
    };
    if let Some(member) = dragged {
        if !ctx.input(|i| i.pointer.primary_down()) {
            if ctrl {
                frame.actions.extend(
                    ctrl_card.and_then(|card| ctrl_drop(group, &snapshot.desktops, member, card)),
                );
            } else if let Some(spot) = target {
                let moved = if dragged_ghost {
                    Item::Ghost(member)
                } else {
                    Item::Card(member)
                };
                frame.actions.extend(drop_item_actions(
                    group,
                    &snapshot.desktops,
                    &model.bands,
                    &model.rows,
                    &snapshot.monitors,
                    moved,
                    spot,
                ));
            }
            shared.drag = None;
        }
    }
    let dragging = shared.drag.is_some();
    if dragging {
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    }

    let area = Rect::from_min_max(
        pos2(panel.min.x, panel.min.y + TOOLBAR),
        pos2(panel.max.x, panel.max.y - footer_height(snapshot.hints)),
    );
    frame.area = area;
    let rails: Vec<usize> = model
        .rows
        .iter()
        .zip(&model.cols)
        .flat_map(|(row, cols)| row.iter().zip(cols).filter(|(_, col)| col.rail))
        .map(|(monitor, _)| *monitor)
        .collect();
    if shared
        .rail_focus
        .is_some_and(|monitor| !rails.contains(&monitor))
    {
        shared.rail_focus = None;
    }
    let open: Vec<Vec<f32>> = model
        .rows
        .iter()
        .zip(&model.cols)
        .map(|(row, cols)| {
            row.iter()
                .zip(cols)
                .map(|(monitor, col)| {
                    if !col.rail {
                        return 0.0;
                    }
                    let device = snapshot.monitors[*monitor].device.as_str();
                    ctx.animate_bool_with_time_and_easing(
                        egui::Id::new(("arrange-rail", device)),
                        rail_wanted(dragging, shared.rail_focus, *monitor),
                        RAIL_ANIMATION,
                        egui::emath::easing::cubic_out,
                    )
                })
                .collect()
        })
        .collect();
    let mut draw = Draw {
        group,
        monitors: &snapshot.monitors,
        desktops: &snapshot.desktops,
        model: &model,
        menu_open,
        can_rename: snapshot.can_rename,
        numbers: taskbar_numbers(group, &snapshot.desktops),
        dragged: dragged.filter(|_| dragging),
        dragged_ghost,
        target: target.filter(|_| dragging && !ctrl),
        open,
        rail_focus: shared.rail_focus,
        zones: Vec::new(),
        hits: Vec::new(),
        items: Vec::new(),
    };
    ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("arrange-bands")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let width = ui.available_width();
                for (b, band) in model.bands.iter().enumerate() {
                    draw.band(ui, shared, &mut frame, b, band, width);
                    if b == 0 && others_count > 0 {
                        let (line, _) = ui.allocate_exact_size(
                            vec2(width - SIDE, others::LINK_HEIGHT),
                            Sense::hover(),
                        );
                        others_link(ui, shared, line.translate(vec2(SIDE, 0.0)), others_count);
                    }
                }
            });
    });
    shared.zones = std::mem::take(&mut draw.zones);
    shared.card_hits = std::mem::take(&mut draw.hits);
    shared.items = std::mem::take(&mut draw.items);
    frame.dragged = dragged.map(|m| group.windows[m].hwnd);
    if let (Some(drag), Some(member), Some(pointer)) =
        (shared.drag.as_ref(), dragged, ctx.pointer_latest_pos())
    {
        ghost(
            &ctx,
            &mut frame,
            &group.windows[member],
            Rect::from_min_size(pointer - drag.grab, vec2(model.widths[member], CARD_HEIGHT)),
            menu_open,
        );
        if ctrl {
            let ghost = Rect::from_min_size(
                ctx.pointer_latest_pos().unwrap_or_default() - drag.grab,
                vec2(model.widths[member], CARD_HEIGHT),
            );
            let place =
                ctrl_card.and_then(|card| ctrl_place(group, &snapshot.desktops, member, card));
            let outlined = ctrl_card
                .filter(|_| place.is_some())
                .and_then(|card| hit_rect(&shared.card_hits, card));
            let hint = ctrl_mark(&ctx, ghost, outlined, &ctrl_hint(place));
            frame.floating = Some(ghost.union(hint));
        }
    }

    if snapshot.hints {
        footer(
            ui,
            panel,
            shown && snapshot.keeps_order,
            others_count > 0,
            cards_here(group, &snapshot.desktops) > 10,
        );
    }

    if !menu_open && !dragging && shared.rename.is_none() {
        keyboard(
            &ctx,
            shared,
            group,
            &snapshot.desktops,
            &model,
            &snapshot.monitors,
            &mut frame,
        );
    }
    frame
}

/// The dragged card riding above everything at the pointer, grabbed where
/// it was picked up, as big as its card. Its old place stays behind, faded,
/// as the "from" mark.
pub(super) fn ghost(
    ctx: &egui::Context,
    frame: &mut Frame,
    window: &ArrangeWindow,
    rect: Rect,
    menu_open: bool,
) {
    let tokens = Tokens::get(ctx);
    let layer = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("arrange-drag"),
    ));
    layer.rect_filled(rect, tokens.radius_row, tokens.hover_bg);
    let thumb = Rect::from_min_size(
        rect.min + Vec2::splat(CARD_PAD),
        vec2(rect.width() - 2.0 * CARD_PAD, THUMB_HEIGHT),
    );
    float_picture(&layer, frame, window.hwnd, thumb, menu_open);
    let mut job = text::job(
        &window.label,
        theme::regular(13.0),
        tokens.text_primary,
        None,
    );
    job.wrap = TextWrapping {
        max_width: rect.width() - 2.0 * CARD_PAD - 4.0,
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    let galley = layer.layout_job(job);
    layer.galley(
        pos2(
            rect.left() + CARD_PAD + 2.0,
            thumb.bottom() + TITLE_GAP + (TITLE - galley.size().y) / 2.0,
        ),
        galley,
        tokens.text_primary,
    );
    frame.floating = Some(rect);
}

/// How a card or a ghost takes the mouse: both can be clicked and dragged,
/// except a card whose window is gone and fading out.
pub fn item_sense(item: Item, leaving: bool) -> Sense {
    match item {
        Item::Card(_) if leaving => Sense::hover(),
        Item::Card(_) | Item::Ghost(_) => Sense::click_and_drag(),
    }
}
