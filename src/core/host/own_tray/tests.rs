use super::*;
use std::sync::Arc;

fn tray(tip: &str, badge: usize) -> OwnTray {
    OwnTray {
        icon: Arc::new(egui::IconData::default()),
        tip: tip.to_string(),
        badge,
    }
}

#[test]
fn an_icon_shows_only_while_its_plugin_is_on() {
    let a = tray("ClaudeDeck", 0);
    assert_eq!(step(None, Some(&a), true), Step::Add);
    assert_eq!(step(None, Some(&a), false), Step::Nothing);
    assert_eq!(step(Some(&a), Some(&a), false), Step::Remove);
    assert_eq!(step(Some(&a), None, true), Step::Remove);
    assert_eq!(step(None, None, true), Step::Nothing);
}

#[test]
fn the_same_icon_again_changes_nothing() {
    let a = tray("ClaudeDeck", 2);
    assert_eq!(
        step(Some(&a), Some(&tray("ClaudeDeck", 2)), true),
        Step::Nothing
    );
    assert_eq!(
        step(Some(&a), Some(&tray("ClaudeDeck", 3)), true),
        Step::Change
    );
    assert_eq!(step(Some(&a), Some(&tray("Other", 2)), true), Step::Change);
}

#[test]
fn plugin_icons_never_take_wincraft_s_own_id() {
    assert_eq!(id_for(0), 100);
    assert_eq!(id_for(7), 107);
    assert!(id_for(0) > 1);
}
