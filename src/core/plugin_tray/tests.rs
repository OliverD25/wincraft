use super::*;

fn solid(side: u32, rgba: [u8; 4]) -> egui::IconData {
    egui::IconData {
        rgba: rgba.repeat((side * side) as usize),
        width: side,
        height: side,
    }
}

fn pixel(pixels: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * size + x) * 4) as usize;
    [pixels[at], pixels[at + 1], pixels[at + 2], pixels[at + 3]]
}

#[test]
fn the_badge_says_the_number_and_9_plus_above_9() {
    assert_eq!(badge_text(0), None, "no badge for nothing");
    assert_eq!(badge_text(1).as_deref(), Some("1"));
    assert_eq!(badge_text(9).as_deref(), Some("9"));
    assert_eq!(badge_text(10).as_deref(), Some("9+"));
    assert_eq!(badge_text(250).as_deref(), Some("9+"));
}

#[test]
fn a_solid_picture_stays_solid_when_scaled() {
    let icon = solid(64, [200, 120, 20, 255]);
    for size in [16, 20, 24, 32] {
        let out = scaled(&icon, size);
        assert_eq!(out.len(), (size * size * 4) as usize);
        assert_eq!(pixel(&out, size, 0, 0), [200, 120, 20, 255]);
        assert_eq!(pixel(&out, size, size - 1, size - 1), [200, 120, 20, 255]);
    }
}

#[test]
fn see_through_pixels_do_not_darken_the_colour() {
    // Half the source is clear black, half solid white: the scaled pixel is
    // white at half alpha, not grey.
    let mut rgba = Vec::new();
    for x in 0..4 {
        rgba.extend(if x < 2 {
            [0, 0, 0, 0]
        } else {
            [255, 255, 255, 255]
        });
    }
    let icon = egui::IconData {
        rgba: rgba.repeat(4),
        width: 4,
        height: 4,
    };
    let out = scaled(&icon, 1);
    assert_eq!(pixel(&out, 1, 0, 0), [255, 255, 255, 128]);
}

#[test]
fn the_badge_sits_bottom_right_and_leaves_the_top_left_alone() {
    let icon = solid(64, [200, 120, 20, 255]);
    let plain = picture(&icon, 16, 0);
    let badged = picture(&icon, 16, 3);
    assert_eq!(pixel(&badged, 16, 0, 0), pixel(&plain, 16, 0, 0));
    assert_ne!(pixel(&badged, 16, 12, 12), pixel(&plain, 16, 12, 12));
    let red_or_white = |p: [u8; 4]| p[..3] == BADGE || p[..3] == BADGE_TEXT;
    assert!(red_or_white(pixel(&badged, 16, 11, 11)));
    let whites = (0..16 * 16)
        .filter(|at| badged[at * 4..at * 4 + 3] == BADGE_TEXT)
        .count();
    assert!(whites >= 5, "the digit is drawn, {whites} white pixels");
}

#[test]
fn nine_plus_fits_inside_its_circle_at_every_tray_size() {
    for size in [16, 20, 24, 32] {
        let mut pixels = vec![0u8; (size * size * 4) as usize];
        draw_badge(&mut pixels, size, "9+");
        let white: Vec<u32> = (0..size * size)
            .filter(|at| pixels[(at * 4) as usize..(at * 4 + 3) as usize] == BADGE_TEXT)
            .collect();
        assert!(!white.is_empty(), "{size} px");
        assert!(white.iter().all(|at| at % size >= size / 3), "{size} px");
    }
}

#[test]
fn the_icon_bytes_have_the_header_the_pixels_and_the_mask() {
    let rgba = [10, 20, 30, 40].repeat(16 * 16);
    let bytes = icon_resource(&rgba, 16);
    assert_eq!(bytes.len(), 40 + 16 * 16 * 4 + 4 * 16);
    assert_eq!(&bytes[0..4], &40u32.to_le_bytes());
    assert_eq!(&bytes[8..12], &32u32.to_le_bytes(), "twice the height");
    assert_eq!(&bytes[14..16], &32u16.to_le_bytes(), "32 bits a pixel");
    assert_eq!(&bytes[40..44], &[30, 20, 10, 40], "BGRA");
}

#[test]
fn two_trays_are_the_same_when_their_badges_read_the_same() {
    let icon = Arc::new(solid(2, [1, 2, 3, 4]));
    let tray = |badge| OwnTray {
        icon: Arc::clone(&icon),
        tip: "ClaudeDeck".to_string(),
        badge,
    };
    assert_eq!(tray(12), tray(40), "both say 9+");
    assert_ne!(tray(1), tray(2));
}
