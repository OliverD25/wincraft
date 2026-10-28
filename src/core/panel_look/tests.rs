use super::*;
use crate::core::theme::Tokens;

#[test]
fn the_border_grows_with_the_scale_but_is_never_thinner_than_one_pixel() {
    let tokens = Tokens::dark();
    assert_eq!(border(&tokens, 1.0), 1.0);
    assert_eq!(border(&tokens, 2.0), 2.0);
    assert_eq!(border(&tokens, 0.2), 1.0);
}

#[test]
fn the_panel_leaves_room_for_its_shadow_on_every_side() {
    let tokens = Tokens::light();
    let mut canvas = Canvas::new(200, 100);
    let area = panel(&mut canvas, &tokens, 1.0);
    assert_eq!(
        area,
        Area::new(SHADOW, SHADOW, 200.0 - 2.0 * SHADOW, 100.0 - 2.0 * SHADOW)
    );
}

#[test]
fn the_shadow_margin_follows_the_scale() {
    let tokens = Tokens::dark();
    let mut canvas = Canvas::new(400, 300);
    let area = panel(&mut canvas, &tokens, 2.0);
    assert_eq!((area.x, area.y), (32.0, 32.0));
    assert_eq!((area.w, area.h), (336.0, 236.0));
}

#[test]
fn a_keycap_takes_its_colours_from_the_theme() {
    let tokens = Tokens::dark();
    let cap = Cap::new(&tokens, 1, 22.0, 1.5);
    assert_eq!(cap.text, tokens.text_secondary);
    assert_eq!(cap.edge, tokens.border);
    assert_eq!(cap.face, tokens.panel_bg);
    assert_eq!((cap.font, cap.height, cap.scale), (1, 22.0, 1.5));
}
