//! Keys that the hover strip's keyboard hook kept from the app in front
//! while the mouse was in the strip. They go into the strip's own input at
//! the start of its next frame, as plain egui key events, so the strip's
//! usual key handling runs on them: there is one copy of the key rules.

use super::*;
use crate::core::ui_bridge::ArrangeKey;

/// What the strip says while keys follow the mouse into it.
pub(super) const ROUTED_LINE: &str = "Keys go to WindowDeck while the mouse is here";

/// The egui event for a routed key: pressed, by its place on the keyboard
/// too, so Shift and a digit is read by the key and not by its sign.
pub(super) fn key_event(key: ArrangeKey) -> egui::Event {
    egui::Event::Key {
        key: key.key,
        physical_key: Some(key.key),
        pressed: true,
        repeat: key.repeat,
        modifiers: key.modifiers,
    }
}

/// F2 starts a title edit, which needs the keyboard for real: the strip
/// takes it, as after a click.
pub(super) fn takes_keyboard(key: ArrangeKey) -> bool {
    key.key == egui::Key::F2 && key.modifiers.is_none()
}

/// Whether the strip shows `ROUTED_LINE`: only while keys go to it and it
/// does not have the keyboard itself.
pub(super) fn shows_line(routed: bool, focused: Option<bool>) -> bool {
    routed && focused != Some(true)
}

impl Arrange {
    /// A key from the hover strip's hook, for the strip's next frame.
    pub fn route_key(&self, key: ArrangeKey) {
        crate::detailed!("deck: routed key {:?} {:?}", key.key, key.modifiers);
        if let Ok(mut shared) = self.shared.lock() {
            if shared.parked {
                return;
            }
            if takes_keyboard(key) && shared.window != 0 {
                shared.focus_wanted = true;
            }
            shared.routed.push(key_event(key));
        }
    }

    /// Whether keys go to the strip now.
    pub fn keys_routed(&self, on: bool) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.keys_routed = on;
        }
    }
}

/// Puts the routed keys into this frame's input.
pub(super) fn feed(ctx: &egui::Context, shared: &mut Shared) {
    if shared.routed.is_empty() {
        return;
    }
    let events = std::mem::take(&mut shared.routed);
    ctx.input_mut(|input| input.events.extend(events));
}

/// The line at the foot of the strip that says keys go to it.
pub(super) fn routed_line(ctx: &egui::Context, panel: Rect) {
    let tokens = Tokens::get(ctx);
    let layer = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("arrange-routed-keys"),
    ));
    let galley = layer.layout_no_wrap(
        ROUTED_LINE.to_string(),
        theme::regular(12.0),
        tokens.text_secondary,
    );
    let size = galley.size() + vec2(20.0, 8.0);
    let rect = Rect::from_center_size(
        pos2(panel.center().x, panel.bottom() - 6.0 - size.y / 2.0),
        size,
    );
    layer.rect(
        rect,
        tokens.radius_row,
        tokens.elevated_bg,
        theme::line(ctx, tokens.border),
        StrokeKind::Inside,
    );
    layer.galley(
        rect.center() - galley.size() / 2.0,
        galley,
        tokens.text_secondary,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ui_bridge;

    fn key(key: egui::Key, modifiers: egui::Modifiers) -> ArrangeKey {
        ArrangeKey {
            key,
            modifiers,
            repeat: false,
        }
    }

    #[test]
    fn a_routed_key_becomes_a_pressed_key_event_with_its_place() {
        let event = key_event(key(egui::Key::Num2, egui::Modifiers::SHIFT));
        assert_eq!(
            event,
            egui::Event::Key {
                key: egui::Key::Num2,
                physical_key: Some(egui::Key::Num2),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::SHIFT,
            }
        );
        let held = ArrangeKey {
            repeat: true,
            ..key(egui::Key::ArrowLeft, egui::Modifiers::NONE)
        };
        assert!(matches!(
            key_event(held),
            egui::Event::Key { repeat: true, .. }
        ));
    }

    #[test]
    fn only_a_plain_f2_takes_the_keyboard() {
        assert!(takes_keyboard(key(egui::Key::F2, egui::Modifiers::NONE)));
        assert!(!takes_keyboard(key(egui::Key::F2, egui::Modifiers::CTRL)));
        assert!(!takes_keyboard(key(egui::Key::D, egui::Modifiers::NONE)));
        assert!(!takes_keyboard(key(
            egui::Key::Enter,
            egui::Modifiers::NONE
        )));
    }

    #[test]
    fn the_line_shows_only_while_keys_are_routed_and_the_strip_lacks_the_keyboard() {
        assert!(shows_line(true, Some(false)));
        assert!(shows_line(true, None));
        assert!(!shows_line(true, Some(true)), "clicked into: its own keys");
        assert!(!shows_line(false, Some(false)));
        assert!(!shows_line(false, None));
        assert!(ROUTED_LINE.contains("WindowDeck"), "names WinCraft's tool");
    }

    #[test]
    fn routed_keys_reach_the_strip_input_once_as_if_typed() {
        let bridge = ui_bridge::create();
        let arrange = Arrange::new(bridge.to_host, 0);
        arrange.route_key(key(egui::Key::W, egui::Modifiers::CTRL));
        arrange.route_key(key(egui::Key::Num3, egui::Modifiers::NONE));
        let ctx = egui::Context::default();
        let mut seen = (false, false, false);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let ctx = ui.ctx();
            let mut shared = arrange.shared.lock().unwrap();
            feed(ctx, &mut shared);
            assert!(shared.routed.is_empty(), "taken for this frame");
            seen = ctx.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::CTRL, egui::Key::W),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Num3),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Num3),
                )
            });
        });
        output.textures_delta.clear();
        assert_eq!(seen, (true, true, false));
    }

    #[test]
    fn a_parked_strip_drops_routed_keys_and_f2_asks_for_the_keyboard() {
        let bridge = ui_bridge::create();
        let arrange = Arrange::new(bridge.to_host, 0);
        arrange.shared.lock().unwrap().parked = true;
        arrange.route_key(key(egui::Key::Enter, egui::Modifiers::NONE));
        assert!(arrange.shared.lock().unwrap().routed.is_empty());

        let mut shared = arrange.shared.lock().unwrap();
        shared.parked = false;
        shared.window = 0x1234;
        drop(shared);
        arrange.route_key(key(egui::Key::F2, egui::Modifiers::NONE));
        let shared = arrange.shared.lock().unwrap();
        assert!(shared.focus_wanted);
        assert_eq!(shared.routed.len(), 1);
    }

    #[test]
    fn keys_routed_follows_the_host() {
        let bridge = ui_bridge::create();
        let arrange = Arrange::new(bridge.to_host, 0);
        arrange.keys_routed(true);
        assert!(arrange.shared.lock().unwrap().keys_routed);
        arrange.keys_routed(false);
        assert!(!arrange.shared.lock().unwrap().keys_routed);
    }
}
