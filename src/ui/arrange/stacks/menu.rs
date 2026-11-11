//! The right-click menus: a stack's own menu, and the group items of a
//! window card's menu. What they ask for is done after the frame.

use super::*;

/// The group part of a card's menu. With `stack_only`, a stack's whole menu
/// and true when the card is a stack (the card menu is then left out);
/// otherwise the group items after a window card's own items.
pub fn group_menu(
    ui: &mut Ui,
    shared: &mut Shared,
    group: &ArrangeGroup,
    hwnd: isize,
    stack_only: bool,
) -> bool {
    if stack_only {
        let stack = shared.stacks.closed(hwnd).is_some();
        if stack {
            stack_menu(ui, &mut shared.stacks, hwnd);
        }
        return stack;
    }
    let defs = shared.snapshot.as_ref().and_then(|s| s.deck_groups.clone());
    card_menu_items(ui, &mut shared.stacks, group, hwnd, defs.as_deref());
    false
}

/// The menu of a stack, in place of a window card's menu.
pub fn stack_menu(ui: &mut Ui, stacks: &mut Stacks, hwnd: isize) {
    let Some(count) = stacks.closed(hwnd).map(|stack| stack.members.len()) else {
        return;
    };
    let mut chosen = None;
    if ui.button("Open the group").clicked() {
        chosen = Some(Ask::Open(hwnd));
    }
    if ui.button("Rename the group").clicked() {
        chosen = Some(Ask::Rename(hwnd));
    }
    if ui.button("Ungroup").clicked() {
        chosen = Some(Ask::Ungroup(hwnd));
    }
    ui.separator();
    if ui
        .button(format!("Close {count} windows\u{2026}"))
        .clicked()
    {
        chosen = Some(Ask::Close(hwnd));
    }
    if let Some(ask) = chosen {
        stacks.asked.push(ask);
        ui.close();
    }
}

/// "Add to group" and "New group…" at the end of a window card's menu,
/// while "Group related windows" is on.
pub fn card_menu_items(
    ui: &mut Ui,
    stacks: &mut Stacks,
    group: &ArrangeGroup,
    hwnd: isize,
    defs: Option<&[DeckGroup]>,
) {
    let Some(defs) = defs else {
        return;
    };
    let own = stacks.stack_of(hwnd).map(|stack| stack.name.clone());
    let mut chosen = None;
    ui.separator();
    let others: Vec<&DeckGroup> = deck_groups::of_app(defs, &group.key)
        .filter(|def| Some(&def.name) != own.as_ref())
        .collect();
    if !others.is_empty() {
        ui.menu_button("Add to group", |ui| {
            for def in &others {
                let open = stacks
                    .found
                    .iter()
                    .find(|s| s.app == group.key && s.name == def.name)
                    .map_or(0, |s| s.members.len());
                if ui.button(format!("{} ({open})", def.name)).clicked() {
                    chosen = Some(Ask::AddTo(hwnd, def.name.clone()));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("New group\u{2026}").clicked() {
                chosen = Some(Ask::NewGroup(hwnd));
                ui.close();
            }
        });
    }
    if ui.button("New group\u{2026}  (Ctrl + G)").clicked() {
        chosen = Some(Ask::NewGroup(hwnd));
        ui.close();
    }
    if let Some(ask) = chosen {
        stacks.asked.push(ask);
    }
}
