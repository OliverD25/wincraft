use super::*;

/// A picture to put on screen once the whole frame is laid out, so that one
/// under the dragged card or under a menu can be hidden instead.
pub(super) struct Pending {
    pub(super) hwnd: isize,
    pub(super) dest: Rect,
    pub(super) clip: Rect,
    pub(super) opacity: u8,
    /// Badges drawn over the picture. DWM draws its picture above egui, so
    /// the picture is kept off this rectangle (see `dwm_thumbs::clear_of`).
    pub(super) clear: Rect,
    /// For a ghost in the By monitor view, whose window also has its own
    /// card: a picture has one place, so a ghost needs a picture of its own.
    pub(super) ghost: bool,
}

/// What a view's body did this frame, for the parts both views share.
pub(super) struct Frame {
    pub(super) pending: Vec<Pending>,
    /// Every card in its order on screen, with whether it has a monitor chip.
    pub(super) cards: Vec<(isize, bool)>,
    pub(super) hovered: Option<isize>,
    /// The ghost under the mouse, in the By monitor view.
    pub(super) hovered_ghost: Option<isize>,
    pub(super) actions: Vec<ArrangeAction>,
    /// Windows a middle-click asked to close. Their cards stay until the
    /// windows are really gone: an app may ask to save first.
    pub(super) close: Vec<isize>,
    pub(super) activate: Option<isize>,
    /// Where the cards are; the peek ends when the pointer leaves it.
    pub(super) area: Rect,
    /// The dragged card when it follows the pointer above the others.
    pub(super) floating: Option<Rect>,
    pub(super) dragged: Option<isize>,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            cards: Vec::new(),
            hovered: None,
            hovered_ghost: None,
            actions: Vec::new(),
            close: Vec::new(),
            activate: None,
            area: Rect::NOTHING,
            floating: None,
            dragged: None,
        }
    }
}

pub(super) fn strip(ui: &mut Ui, shared: &mut Shared) {
    if shared.settle_motion() {
        ui.ctx().request_repaint();
    }
    let tokens = Tokens::get(ui.ctx());
    let panel = ui.max_rect();
    let view = shared.view();
    let background = match view {
        ArrangeView::ByMonitor => tokens.window_bg,
        _ => tokens.elevated_bg,
    };
    ui.painter().rect_filled(panel, 0, background);

    let content = panel.shrink(PADDING);
    let (Some(group), Some(snapshot)) = (shared.group().cloned(), shared.snapshot.clone()) else {
        let watched = shared
            .snapshot
            .as_ref()
            .map(|s| s.watched.join(", "))
            .unwrap_or_default();
        ui.scope_builder(UiBuilder::new().max_rect(content), |ui| {
            empty_state(
                ui,
                "No windows to arrange",
                &format!("No windows of {watched} are open."),
            );
        });
        shared.thumbs.clear();
        shared.ghost_thumbs.clear();
        shared.hide_peek();
        if shared.close_when_empty {
            shared.closed = true;
        }
        return;
    };
    if shared.close_when_empty && group.windows.iter().all(|w| shared.is_leaving(w.hwnd)) {
        shared.closed = true;
    }

    let before = (shared.cursor, shared.cursor_ghost);
    let threshold = pointer::threshold(ui.ctx().pixels_per_point());
    let awake = shared.pointer.see(cursor_position(), threshold);
    if let Some(hwnd) = shared.select_window.take() {
        if group.windows.iter().any(|window| window.hwnd == hwnd) {
            shared.cursor = Some(hwnd);
            shared.cursor_ghost = false;
        }
    }
    if std::mem::take(&mut shared.select_next) {
        let visible = shared.visible(&group);
        let this_desktop = rows(&group, &snapshot.desktops).swap_remove(0);
        let shown: Vec<isize> = this_desktop
            .members
            .iter()
            .filter(|member| visible.get(**member).copied().unwrap_or(true))
            .map(|member| group.windows[*member].hwnd)
            .collect();
        if let Some(next) = next_card(&shown, shared.cursor) {
            shared.cursor = Some(next);
            shared.cursor_ghost = false;
        }
    }
    if let Some(place) = shared.select.take() {
        let row = rows(&group, &snapshot.desktops);
        let picked = place
            .checked_sub(1)
            .and_then(|at| row.first()?.members.get(at).copied());
        if let Some(member) = picked {
            shared.cursor = Some(group.windows[member].hwnd);
            shared.cursor_ghost = false;
        }
    }

    let bar = match view {
        ArrangeView::ByMonitor => by_monitor::toolbar_rect(panel),
        _ => Rect::from_min_size(content.min, vec2(content.width(), SWITCHER)),
    };
    let visible = shared.visible(&group);
    let compact = compact::applies(&group, &snapshot.desktops, &visible);
    let (chosen_group, chosen_view) = if compact {
        (None, None)
    } else {
        toolbar(ui, bar, &snapshot, &group, view, &visible)
    };
    if let Some(index) = chosen_group {
        switch_group(ui.ctx(), shared, &snapshot, index);
    }
    if let Some(chosen) = chosen_view {
        if let Some(snapshot) = shared.snapshot.as_mut() {
            snapshot.view = chosen;
        }
        shared.to_host.send(HostRequest::SetSetting {
            plugin: snapshot.look_plugin.clone(),
            key: "arrange_view".to_string(),
            value: serde_json::Value::String(chosen.setting().to_string()),
        });
        shared.drag = None;
        shared.hide_peek();
        ui.ctx().request_repaint();
    }

    shared
        .thumbs
        .retain(|hwnd, _| group.windows.iter().any(|window| window.hwnd == *hwnd));
    shared
        .ghost_thumbs
        .retain(|hwnd, _| group.windows.iter().any(|window| window.hwnd == *hwnd));
    let ctx = ui.ctx().clone();
    if refresh_sources(shared, &group) {
        // The cards change shape, and the window with them.
        ctx.request_repaint();
    }
    let ppp = ctx.pixels_per_point();
    let menu_open = egui::Popup::is_any_open(&ctx);
    // Esc ends a title edit and nothing more; `show` does not close the
    // strip for it.
    if shared.rename.is_some()
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        shared.rename = None;
    }
    let others_count = others::count(&group, &snapshot.desktops, &visible);
    // While the title box has the keyboard, no strip key acts.
    let typing = shared.rename.is_some();
    if others_count > 0
        && !menu_open
        && !typing
        && shared.drag.is_none()
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::D))
    {
        shared.others.toggle();
        ctx.request_repaint();
    }
    // A plain digit opens the card with that number and Shift and a digit
    // the card ten higher; Ctrl and a digit jump to that taskbar button's
    // app, the way Win+Alt+A and a digit do.
    let mut digit_card = None;
    if !menu_open && !typing && shared.drag.is_none() {
        let typed = |modifiers: egui::Modifiers| {
            ctx.input_mut(|i| (0u8..=9).find(|digit| i.consume_key(modifiers, digit_key(*digit))))
        };
        if let Some(digit) = typed(egui::Modifiers::CTRL) {
            shared.to_host.send(HostRequest::ArrangeDigit(digit));
        }
        // Shift goes first: egui also reports Shift and a digit as the plain
        // digit on layouts where that pair types no character.
        let number = ctx
            .input_mut(consume_shift_digit)
            .map(|digit| typed_number(digit, true))
            .or_else(|| typed(egui::Modifiers::NONE).map(|digit| typed_number(digit, false)));
        if let Some(number) = number {
            digit_card = card_for_digit(&group, &snapshot.desktops, &visible, number);
            if digit_card.is_none() {
                log::info!("number {number} typed in the strip: no card has it");
            }
        }
    }
    if view != ArrangeView::ByMonitor {
        // The Flat view has no ghosts.
        shared.cursor_ghost = false;
    }
    let mut frame = match view {
        _ if compact => {
            let (frame, chosen) = compact::body(ui, shared, &group, &snapshot, panel, menu_open);
            if let Some(index) = chosen {
                switch_group(&ctx, shared, &snapshot, index);
            }
            frame
        }
        ArrangeView::ByMonitor => by_monitor::body(ui, shared, &group, &snapshot, panel, menu_open),
        _ => {
            let mut rows_area = content;
            rows_area.min.y += SWITCHER;
            flat(ui, shared, &group, &snapshot.desktops, rows_area, menu_open)
        }
    };
    let by_digit = frame.activate.is_none() && digit_card.is_some();
    if frame.activate.is_none() {
        frame.activate = digit_card;
    }
    finish_rename(shared, &group, &mut frame);
    if !menu_open && !typing && shared.drag.is_none() {
        let (close_key, new_key, rename_key, hints_key) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::CTRL, egui::Key::W),
                i.consume_key(egui::Modifiers::CTRL, egui::Key::N),
                i.consume_key(egui::Modifiers::NONE, egui::Key::F2),
                i.consume_key(egui::Modifiers::CTRL, egui::Key::H),
            )
        });
        // Only the By monitor view has a footer of hints to hide.
        if hints_key && view == ArrangeView::ByMonitor && !compact {
            toggle_hints(shared);
            ctx.request_repaint();
        }
        if rename_key {
            start_rename(shared, &group, snapshot.can_rename);
        }
        if close_key {
            ctrl_w(shared, &mut frame);
        }
        if new_key {
            let hwnd = shared
                .cursor
                .or_else(|| group.windows.first().map(|window| window.hwnd));
            if let Some(hwnd) = hwnd {
                shared.to_host.send(HostRequest::Arrange {
                    plugin: snapshot.look_plugin.clone(),
                    action: ArrangeAction::NewWindow {
                        group: group.key.clone(),
                        hwnd,
                    },
                });
                shared.hide_peek();
                shared.closed = true;
            }
        }
    }
    // A card under a mouse that has not moved since the strip opened is not
    // hovered at all; one under a moved mouse is the selected card.
    if !awake || menu_open || shared.drag.is_some() {
        frame.hovered = None;
        frame.hovered_ghost = None;
    }
    let hovered = frame
        .hovered
        .map(|hwnd| (Some(hwnd), false))
        .or(frame.hovered_ghost.map(|hwnd| (Some(hwnd), true)));
    let (selection, rest) = pointer::settle(before, (shared.cursor, shared.cursor_ghost), hovered);
    (shared.cursor, shared.cursor_ghost) = selection;
    if rest && awake {
        shared.pointer.rest(cursor_position());
        frame.hovered = None;
    }

    let in_rows = ctx
        .input(|i| i.pointer.hover_pos())
        .is_some_and(|at| frame.area.contains(at));
    // Asked again rather than reusing the flags from the top of the frame,
    // so a menu or drag that started on this frame hides the peek at once.
    let blocked = !snapshot.preview
        || egui::Popup::is_any_open(&ctx)
        || shared.drag.is_some()
        || shared.window == 0;
    let forced = shared
        .forced_peek
        .and_then(|card| forced_card(&frame.cards, card));
    match forced {
        Some(hwnd) if !blocked && frame.hovered.is_none() => {
            shared.peek.show(hwnd, shared.window as HWND)
        }
        _ => match shared
            .hover
            .step(Instant::now(), frame.hovered, in_rows, blocked)
        {
            Step::Show(hwnd) => shared.peek.show(hwnd, shared.window as HWND),
            Step::Hide => shared.peek.hide(),
            Step::Wait(after) => ctx.request_repaint_after(after),
        },
    }
    if shared.peek.is_shown() {
        // Looks again now and then, so a window that closes takes its
        // peek with it even when nothing else repaints the strip.
        ctx.request_repaint_after(peek::REST);
    }

    if routed::shows_line(shared.keys_routed, ctx.input(|i| i.viewport().focused)) {
        routed::routed_line(&ctx, panel);
    }
    if let Some((text, since)) = shared.note.clone() {
        match NOTE_TIME.checked_sub(since.elapsed()) {
            Some(left) => {
                note_line(&ctx, panel, &text);
                ctx.request_repaint_after(left);
            }
            None => shared.note = None,
        }
    }

    place_pictures(shared, &frame.pending, frame.floating, frame.dragged, ppp);

    let actions = std::mem::take(&mut frame.actions);
    for action in &actions {
        apply_locally(shared, action);
    }
    hold_changes(shared, &actions);
    let moves_monitor = actions
        .iter()
        .any(|action| matches!(action, ArrangeAction::MoveToMonitor { .. }));
    for action in actions {
        shared.to_host.send(HostRequest::Arrange {
            plugin: snapshot.plugin.clone(),
            action,
        });
    }
    for hwnd in std::mem::take(&mut frame.close) {
        shared.to_host.send(HostRequest::Arrange {
            plugin: snapshot.plugin.clone(),
            action: ArrangeAction::Close(hwnd),
        });
    }
    if moves_monitor && shared.window != 0 {
        shared.keep_open_until = Some(Instant::now() + MOVE_GRACE);
        shared.to_host.send(HostRequest::FocusWindow(shared.window));
    }
    if let Some(hwnd) = frame.activate {
        log::info!(
            "WindowDeck switches to window {hwnd:#x} ({}) and closes",
            if by_digit {
                "a digit typed in it"
            } else {
                "a click or Enter"
            }
        );
        shared.hide_peek();
        shared.to_host.send(HostRequest::Arrange {
            plugin: snapshot.plugin.clone(),
            action: ArrangeAction::Activate(hwnd),
        });
        shared.closed = true;
    }
}

/// F2: the picked card's title becomes a text box, or a line says why not.
fn start_rename(shared: &mut Shared, group: &ArrangeGroup, can_rename: bool) {
    let leaving = |hwnd| shared.is_leaving(hwnd);
    match rename::start(
        can_rename,
        shared.cursor,
        shared.cursor_ghost,
        group,
        &leaving,
    ) {
        rename::Start::Edit(edit) => {
            shared.hide_peek();
            shared.rename = Some(edit);
        }
        rename::Start::NeedsNamer => {
            shared.note = Some((rename::NEEDS_NAMER.to_string(), Instant::now()));
        }
        rename::Start::Nothing => {}
    }
}

/// Ends a title edit that was saved or cancelled this frame, or whose
/// window is gone. A save becomes a rename for WindowNamer, sent with the
/// frame's other actions; the card shows the new name at once.
fn finish_rename(shared: &mut Shared, group: &ArrangeGroup, frame: &mut Frame) {
    let Some(edit) = shared.rename.as_mut() else {
        return;
    };
    let hwnd = edit.hwnd;
    let gone = !group.windows.iter().any(|window| window.hwnd == hwnd);
    let Some(done) = edit.take_done().or(gone.then_some(rename::Done::Cancel)) else {
        return;
    };
    shared.rename = None;
    if let rename::Done::Save(text) = done {
        frame.actions.push(rename::action(hwnd, &text));
    }
}

/// What Ctrl+W does: the window to close, the picked card's, and the card
/// picked after it, from the cards on screen in their order. Nothing when
/// no card is picked or its window is already going.
pub fn ctrl_w_plan(
    cards: &[isize],
    cursor: Option<isize>,
    leaving: &dyn Fn(isize) -> bool,
) -> Option<(isize, Option<isize>)> {
    let hwnd = cursor.filter(|hwnd| !leaving(*hwnd))?;
    let mut order: Vec<isize> = Vec::new();
    for card in cards {
        if !order.contains(card) && !leaving(*card) {
            order.push(*card);
        }
    }
    if !order.contains(&hwnd) {
        order.push(hwnd);
    }
    Some((hwnd, after_close(&order, hwnd)))
}

/// Ctrl+W: the picked card's window is asked to close, as a middle-click
/// does, and the card next to it is picked. The strip stays open.
fn ctrl_w(shared: &mut Shared, frame: &mut Frame) {
    let cards: Vec<isize> = frame.cards.iter().map(|(hwnd, _)| *hwnd).collect();
    let Some((hwnd, next)) = ctrl_w_plan(&cards, shared.cursor, &|h| shared.is_leaving(h)) else {
        return;
    };
    shared.cursor = next;
    shared.cursor_ghost = false;
    shared.close_when_empty = next.is_none();
    if !frame.close.contains(&hwnd) {
        frame.close.push(hwnd);
    }
}

/// Another group picked in the strip's app picker.
fn switch_group(
    ctx: &egui::Context,
    shared: &mut Shared,
    snapshot: &ArrangeSnapshot,
    index: usize,
) {
    shared.key = Some(snapshot.groups[index].key.clone());
    shared.cursor = None;
    shared.drag = None;
    shared.others.reset();
    shared.hide_peek();
    ctx.request_repaint();
}

/// Ctrl+H: hides or shows the key hints now, and asks the plugin that keeps
/// the look to remember it, so the strip does not wait for the answer.
pub(super) fn toggle_hints(shared: &mut Shared) {
    let Some(snapshot) = shared.snapshot.as_mut() else {
        return;
    };
    snapshot.hints = !snapshot.hints;
    let request = hints_request(&snapshot.look_plugin, snapshot.hints);
    shared.to_host.send(request);
}

/// The request that saves the hints switch in the look plugin's settings.
fn hints_request(look_plugin: &str, hints: bool) -> HostRequest {
    HostRequest::SetSetting {
        plugin: look_plugin.to_string(),
        key: "show_hints".to_string(),
        value: serde_json::Value::Bool(hints),
    }
}
