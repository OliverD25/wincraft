//! What the keys, clicks, drops and menus do to groups: called by the strip
//! before its views draw (the dialog and the group keys) and after (the
//! card that was clicked, closed or moved may be a stack).

use super::dialogs::save_groups;
use super::*;

/// Before the views draw: the dialog, which then has the keyboard, and the
/// keys that belong to groups. `quiet` is true while a menu is open, a
/// title is edited or a card is dragged.
pub fn before_frame(
    ctx: &egui::Context,
    shared: &mut Shared,
    group: &ArrangeGroup,
    panel: Rect,
    quiet: bool,
) {
    shared.stacks.covers.clear();
    let on = shared
        .snapshot
        .as_ref()
        .is_some_and(|s| s.deck_groups.is_some());
    if !on {
        shared.stacks.dialog = None;
        return;
    }
    if shared.stacks.dialog.is_some() {
        dialogs::show(ctx, shared, panel);
        // No other key acts while the dialog is up, nor in its last frame.
        ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Key { .. })));
        return;
    }
    if quiet {
        return;
    }
    if shared.stacks.open.is_some()
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        close_group(shared);
        ctx.request_repaint();
        return;
    }
    if outside_click(ctx, shared, panel) {
        close_group(shared);
        ctx.request_repaint();
        return;
    }
    if shared.stacks.open.is_some() {
        open_group_keys(ctx, shared, group);
        return;
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::G)) {
        ctrl_g(shared, group);
        ctx.request_repaint();
    }
    let on_stack = shared
        .cursor
        .filter(|hwnd| !shared.cursor_ghost && shared.stacks.closed(*hwnd).is_some());
    if let Some(hwnd) = on_stack {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F2)) {
            start_rename(shared, group, hwnd);
        }
    }
    let shift_arrow = ctx.input(|i| {
        i.modifiers.shift
            && !i.modifiers.ctrl
            && [
                egui::Key::ArrowLeft,
                egui::Key::ArrowRight,
                egui::Key::ArrowUp,
                egui::Key::ArrowDown,
            ]
            .iter()
            .any(|key| i.key_pressed(*key))
    });
    if shift_arrow {
        if let Some(hwnd) = shared.cursor {
            pick(&mut shared.stacks, hwnd, true);
        }
        shared.stacks.pick_follow = true;
    }
}

/// The keys while a group is open: Left and Right pick among its windows,
/// Alt + Left and Right move the picked one, Enter opens the picked one, Ctrl+G takes it out of the group and Ctrl+W
/// closes it. The row under the layer gets no keys; numbers are left for
/// the strip, which asks `digit`.
fn open_group_keys(ctx: &egui::Context, shared: &mut Shared, group: &ArrangeGroup) {
    let count = shared
        .stacks
        .open_stack()
        .map_or(0, |stack| stack.members.len());
    let (alt_left, alt_right) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowLeft),
            i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowRight),
        )
    });
    if alt_left || alt_right {
        shared.stacks.nudge = Some(alt_left);
        ctx.request_repaint();
    }
    let (left, right, enter, ctrl_g, ctrl_w) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            i.consume_key(egui::Modifiers::CTRL, egui::Key::G),
            i.consume_key(egui::Modifiers::CTRL, egui::Key::W),
        )
    });
    let front = shared.stacks.front.min(count.saturating_sub(1));
    shared.stacks.front = overlay::step(front, count, left, right);
    if left || right {
        shared.stacks.follow = None;
        ctx.request_repaint();
    }
    let picked = shared.stacks.front();
    if enter {
        shared.stacks.list_chosen = picked;
    }
    if let Some(hwnd) = picked.filter(|_| ctrl_g) {
        take_out(shared, group, hwnd);
        ctx.request_repaint();
    }
    if let Some(hwnd) = picked.filter(|hwnd| ctrl_w && !shared.is_leaving(*hwnd)) {
        shared.stacks.close_member = Some(hwnd);
        ctx.request_repaint();
    }
    // The row's own moves would act on cards nobody sees.
    ctx.input_mut(|i| {
        i.events.retain(|event| {
            !matches!(
                event,
                egui::Event::Key { key, pressed: true, .. } if overlay::ROW_KEYS.contains(key)
            )
        })
    });
}

/// After the views drew: a click, Enter or number on a stack opens it, a
/// close asks first, a move or reorder takes all its windows, and what the
/// menus and drops asked for is done.
pub fn after_frame(
    ctx: &egui::Context,
    shared: &mut Shared,
    group: &ArrangeGroup,
    frame: &mut Frame,
) {
    let on = shared
        .snapshot
        .as_ref()
        .is_some_and(|s| s.deck_groups.is_some());
    if !on {
        return;
    }
    if std::mem::take(&mut shared.stacks.pick_follow) {
        if let Some(hwnd) = shared.cursor {
            pick(&mut shared.stacks, hwnd, true);
        }
    }
    // Ctrl + click picks a card for Ctrl+G instead of opening it.
    if ctx.input(|i| i.modifiers.ctrl && i.pointer.any_click()) {
        if let Some(hwnd) = frame.activate.take() {
            let picked = !shared.stacks.is_picked(hwnd);
            pick(&mut shared.stacks, hwnd, picked);
            shared.cursor = Some(hwnd);
            shared.cursor_ghost = false;
        }
    }
    if let Some(hwnd) = frame
        .activate
        .filter(|hwnd| shared.stacks.closed(*hwnd).is_some())
    {
        frame.activate = None;
        open_group(shared, hwnd);
        ctx.request_repaint();
    }
    // A row of a stack's list switches to its window, even the one the
    // stack shows, which a click on the stack itself would open.
    if let Some(hwnd) = shared.stacks.list_chosen.take() {
        frame.activate = Some(hwnd);
    }
    let closing: Vec<isize> = frame
        .close
        .iter()
        .copied()
        .filter(|hwnd| shared.stacks.closed(*hwnd).is_some())
        .collect();
    if let Some(hwnd) = closing.first().copied() {
        frame.close.retain(|h| !closing.contains(h));
        shared.cursor = Some(hwnd);
        shared.cursor_ghost = false;
        shared.close_when_empty = false;
        ask_close(shared, group, hwnd);
    }
    for action in std::mem::take(&mut frame.actions) {
        match action {
            ArrangeAction::Close(hwnd) if shared.stacks.closed(hwnd).is_some() => {
                ask_close(shared, group, hwnd)
            }
            ArrangeAction::AskName(hwnd) if shared.stacks.closed(hwnd).is_some() => {
                start_rename(shared, group, hwnd)
            }
            ArrangeAction::Rename {
                hwnd,
                name: Some(new),
            } => {
                keep_renamed(shared, group, hwnd, &new);
                frame.actions.push(ArrangeAction::Rename {
                    hwnd,
                    name: Some(new),
                });
            }
            other => frame
                .actions
                .extend(fold::expand_action(other, &shared.stacks.found)),
        }
    }
    for ask in std::mem::take(&mut shared.stacks.asked) {
        answer(shared, group, ask);
        ctx.request_repaint();
    }
    if let Some(hwnd) = shared.stacks.close_member.take() {
        shared.close_when_empty = false;
        if !frame.close.contains(&hwnd) {
            frame.close.push(hwnd);
        }
    }
    for cover in &shared.stacks.covers {
        frame.covers.push(*cover);
    }
}

/// While a group is open and the mouse is outside its layer, a cover over
/// the whole strip takes the mouse: a click there closes the group and
/// does nothing else (the card under it does not open). True on that click.
fn outside_click(ctx: &egui::Context, shared: &Shared, panel: Rect) -> bool {
    if shared.stacks.open.is_none() {
        return false;
    }
    let Some(at) = ctx.input(|i| i.pointer.hover_pos()) else {
        return false;
    };
    if !panel.contains(at) || !outside_open_group(at, shared.stacks.layer) {
        return false;
    }
    let id = egui::Id::new("deck-group-outside");
    egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(panel.min)
        .movable(false)
        .constrain(false)
        .show(ctx, |ui| {
            let response = ui.interact(panel, id.with("cover"), egui::Sense::click_and_drag());
            response.clicked() || response.secondary_clicked() || response.middle_clicked()
        })
        .inner
}

fn pick(stacks: &mut Stacks, hwnd: isize, on: bool) {
    stacks.picked.retain(|h| *h != hwnd);
    if on {
        stacks.picked.push(hwnd);
    }
}

/// The shown names of the windows behind these cards: a stack's card
/// stands for all its windows.
fn labels_of(stacks: &Stacks, group: &ArrangeGroup, hwnds: &[isize]) -> Vec<String> {
    let mut labels = Vec::new();
    for hwnd in hwnds {
        match stacks.closed(*hwnd) {
            Some(stack) => labels.extend(stack.members.iter().map(|w| w.label.clone())),
            None => labels.extend(label_of(stacks, group, *hwnd)),
        }
    }
    labels
}

/// One window's own name. A window of the open group has no card of its
/// own: the group's card carries the group's name, not the window's.
fn label_of(stacks: &Stacks, group: &ArrangeGroup, hwnd: isize) -> Option<String> {
    stacks
        .stack_of(hwnd)
        .and_then(|stack| stack.members.iter().find(|w| w.hwnd == hwnd))
        .or_else(|| group.windows.iter().find(|w| w.hwnd == hwnd))
        .map(|w| w.label.clone())
}

fn note(shared: &mut Shared, text: &str) {
    shared.note = Some((text.to_string(), Instant::now()));
}

fn defs(shared: &Shared) -> Option<Vec<DeckGroup>> {
    shared.snapshot.as_ref()?.deck_groups.clone()
}

/// Ctrl+G: groups the picked cards, ungroups a stack, or takes a window
/// out of its group.
fn ctrl_g(shared: &mut Shared, group: &ArrangeGroup) {
    let cursor = shared.cursor;
    if !shared.stacks.picked.is_empty() {
        let mut cards = shared.stacks.picked.clone();
        if let Some(hwnd) = cursor.filter(|h| !cards.contains(h)) {
            cards.push(hwnd);
        }
        name_box(shared, group, &cards);
        return;
    }
    let Some(hwnd) = cursor else {
        return;
    };
    if shared.stacks.closed(hwnd).is_some() {
        ungroup(shared, group, hwnd);
    } else if shared.stacks.stack_of(hwnd).is_some() {
        take_out(shared, group, hwnd);
    } else {
        note(
            shared,
            "Pick two or more cards with Ctrl + click, then press Ctrl + G",
        );
    }
}

/// The name box for a new group of these cards' windows.
fn name_box(shared: &mut Shared, group: &ArrangeGroup, cards: &[isize]) {
    let labels = labels_of(&shared.stacks, group, cards);
    let mut unique: Vec<String> = Vec::new();
    for label in labels {
        if !unique.contains(&label) {
            unique.push(label);
        }
    }
    if unique.len() < 2 {
        note(
            shared,
            "Pick two or more cards with Ctrl + click, then press Ctrl + G",
        );
        return;
    }
    let names: Vec<&str> = unique.iter().map(String::as_str).collect();
    let text = deck_groups::suggested_name(&names, &group.label);
    shared.stacks.dialog = Some(Dialog::Name {
        app: group.key.clone(),
        labels: unique,
        text,
        focused: false,
    });
}

fn ungroup(shared: &mut Shared, group: &ArrangeGroup, hwnd: isize) {
    let (Some(stack), Some(mut defs)) = (shared.stacks.stack_of(hwnd).cloned(), defs(shared))
    else {
        return;
    };
    deck_groups::ungroup(&mut defs, &group.key, &stack.name);
    log::info!("WindowDeck: group \"{}\" ungrouped", stack.name);
    save_groups(shared, defs);
    shared.cursor = Some(stack.rep);
    shared.cursor_ghost = false;
}

fn take_out(shared: &mut Shared, group: &ArrangeGroup, hwnd: isize) {
    let Some(mut defs) = defs(shared) else {
        return;
    };
    let Some(label) = label_of(&shared.stacks, group, hwnd) else {
        return;
    };
    deck_groups::remove(&mut defs, &group.key, &label);
    save_groups(shared, defs);
    if shared.stacks.open.is_none() {
        shared.cursor = Some(hwnd);
        shared.cursor_ghost = false;
    }
}

/// F2 on a stack: the box for the group's new name.
fn start_rename(shared: &mut Shared, group: &ArrangeGroup, hwnd: isize) {
    let Some(stack) = shared.stacks.stack_of(hwnd) else {
        return;
    };
    shared.stacks.dialog = Some(Dialog::Rename {
        app: group.key.clone(),
        old: stack.name.clone(),
        text: stack.name.clone(),
        focused: false,
    });
}

/// A window of a group got a new name: the group keeps it.
fn keep_renamed(shared: &mut Shared, group: &ArrangeGroup, hwnd: isize, new: &str) {
    if shared.stacks.closed(hwnd).is_some() {
        return;
    }
    let Some(stack) = shared.stacks.stack_of(hwnd) else {
        return;
    };
    let Some(old) = stack
        .members
        .iter()
        .find(|w| w.hwnd == hwnd)
        .map(|w| w.label.clone())
    else {
        return;
    };
    if let Some(mut defs) = defs(shared) {
        deck_groups::rename_member(&mut defs, &group.key, &old, new);
        save_groups(shared, defs);
    }
}

/// The question before a stack's windows close (Principle 1).
fn ask_close(shared: &mut Shared, group: &ArrangeGroup, hwnd: isize) {
    let Some(stack) = shared.stacks.closed(hwnd) else {
        return;
    };
    shared.stacks.dialog = Some(Dialog::Close {
        app_label: group.label.clone(),
        name: stack.name.clone(),
        windows: stack
            .members
            .iter()
            .map(|w| (w.hwnd, w.label.clone()))
            .collect(),
    });
}

/// Opens the stack `hwnd` in place: its windows show on a layer above the
/// row, and the row keeps every card where it is.
pub fn open_group(shared: &mut Shared, hwnd: isize) {
    let Some(stack) = shared.stacks.closed(hwnd).cloned() else {
        return;
    };
    log::info!(
        "WindowDeck opens group \"{}\" ({} windows)",
        stack.name,
        stack.members.len()
    );
    shared.stacks.open = Some((stack.app.clone(), stack.name.clone()));
    if let Some(snapshot) = shared.snapshot.as_mut() {
        shared.stacks.refold(snapshot);
    }
    shared.stacks.front = 0;
    shared.stacks.follow = None;
    shared.stacks.layer = None;
    shared.cursor = Some(hwnd);
    shared.cursor_ghost = false;
    shared.hide_peek();
}

/// Esc in an open group: it folds back into its stack, which keeps the
/// keys.
pub fn close_group(shared: &mut Shared) {
    let Some((app, name)) = shared.stacks.open.take() else {
        return;
    };
    if let Some(snapshot) = shared.snapshot.as_mut() {
        shared.stacks.refold(snapshot);
    }
    shared.stacks.layer = None;
    shared.stacks.close_member = None;
    shared.stacks.follow = None;
    shared.stacks.nudge = None;
    let rep = shared
        .stacks
        .found
        .iter()
        .find(|s| s.app == app && s.name == name && s.kind == Kind::Closed)
        .map(|s| s.rep);
    if rep.is_some() {
        shared.cursor = rep;
        shared.cursor_ghost = false;
    }
}

/// Does what a menu item or a drop asked for.
fn answer(shared: &mut Shared, group: &ArrangeGroup, ask: Ask) {
    match ask {
        Ask::Open(hwnd) => open_group(shared, hwnd),
        Ask::Rename(hwnd) => start_rename(shared, group, hwnd),
        Ask::Ungroup(hwnd) => ungroup(shared, group, hwnd),
        Ask::Close(hwnd) => ask_close(shared, group, hwnd),
        Ask::AddTo(hwnd, name) => {
            let Some(mut defs) = defs(shared) else {
                return;
            };
            for label in labels_of(&shared.stacks, group, &[hwnd]) {
                deck_groups::add(&mut defs, &group.key, &name, &label);
            }
            save_groups(shared, defs);
        }
        Ask::NewGroup(hwnd) => {
            let mut cards = shared.stacks.picked.clone();
            if !cards.contains(&hwnd) {
                cards.push(hwnd);
            }
            if cards.len() < 2 {
                pick(&mut shared.stacks, hwnd, true);
                note(
                    shared,
                    "Picked. Pick another card with Ctrl + click, then press Ctrl + G",
                );
            } else {
                name_box(shared, group, &cards);
            }
        }
        Ask::DropOn { dragged, card } => drop_on(shared, group, dragged, card),
        Ask::TakeOut(hwnd) => take_out(shared, group, hwnd),
    }
}

/// A card dropped on the middle of another: onto a group it joins it,
/// otherwise the name box offers a new group of both.
fn drop_on(shared: &mut Shared, group: &ArrangeGroup, dragged: isize, card: isize) {
    let target = shared.stacks.stack_of(card).map(|s| s.name.clone());
    let source = shared.stacks.closed(dragged).map(|s| s.name.clone());
    let (into, joining) = match (target, source) {
        (Some(name), _) => (name, dragged),
        (None, Some(name)) => (name, card),
        (None, None) => {
            name_box(shared, group, &[card, dragged]);
            return;
        }
    };
    let Some(mut defs) = defs(shared) else {
        return;
    };
    for label in labels_of(&shared.stacks, group, &[joining]) {
        deck_groups::add(&mut defs, &group.key, &into, &label);
    }
    log::info!("WindowDeck: a window joined group \"{into}\" by a drop");
    save_groups(shared, defs);
}

/// While a card is dragged: the card whose middle is under the pointer, so
/// a drop there groups the two. Not with Ctrl, which changes numbers, nor
/// for a ghost.
pub fn group_drop_target(
    shared: &mut Shared,
    pointer: Option<Pos2>,
    dragged: Option<isize>,
) -> Option<isize> {
    let on = shared
        .snapshot
        .as_ref()
        .is_some_and(|s| s.deck_groups.is_some());
    let ghost = shared.drag.as_ref().is_some_and(|drag| drag.ghost);
    let found = match (on && !ghost, pointer, dragged) {
        (true, Some(pointer), Some(dragged)) => middle_hit(pointer, dragged, &shared.card_hits),
        _ => None,
    };
    shared.stacks.drop_on = found;
    found
}

/// The card whose middle (half its width and height) holds `pointer`.
pub fn middle_hit(pointer: Pos2, dragged: isize, cards: &[(isize, Rect)]) -> Option<isize> {
    cards
        .iter()
        .filter(|(hwnd, _)| *hwnd != dragged)
        .find(|(_, rect)| rect.shrink2(rect.size() / 4.0).contains(pointer))
        .map(|(hwnd, _)| *hwnd)
}

/// A drop on the middle of a card: the groups are changed after the frame.
/// True when it took the drop.
pub fn group_dropped(shared: &mut Shared, card: Option<isize>, dragged: isize) -> bool {
    match card {
        Some(card) => {
            shared.stacks.asked.push(Ask::DropOn { dragged, card });
            shared.stacks.drop_on = None;
            true
        }
        None => false,
    }
}

/// A digit typed while a group is open picks among its windows only.
pub fn digit_card(
    shared: &Shared,
    group: &ArrangeGroup,
    number: usize,
    otherwise: impl FnOnce() -> Option<isize>,
) -> Option<isize> {
    match shared.stacks.digit(group, number) {
        Some(found) => found,
        None => otherwise(),
    }
}
