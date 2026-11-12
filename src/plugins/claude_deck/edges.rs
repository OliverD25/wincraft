//! Resizing ClaudeDeck's frameless window by its edges and corners.

use egui::viewport::ResizeDirection;
use egui::{CursorIcon, Rect, Ui};

/// How close to the edge the mouse resizes the window.
const EDGE: f32 = 5.0;

/// The edge or corner the mouse is on, for resizing.
fn edge(rect: Rect, at: egui::Pos2) -> Option<ResizeDirection> {
    let left = at.x - rect.left() < EDGE;
    let right = rect.right() - at.x < EDGE;
    let top = at.y - rect.top() < EDGE;
    let bottom = rect.bottom() - at.y < EDGE;
    match (left, right, top, bottom) {
        (true, _, true, _) => Some(ResizeDirection::NorthWest),
        (_, true, true, _) => Some(ResizeDirection::NorthEast),
        (true, _, _, true) => Some(ResizeDirection::SouthWest),
        (_, true, _, true) => Some(ResizeDirection::SouthEast),
        (true, ..) => Some(ResizeDirection::West),
        (_, true, ..) => Some(ResizeDirection::East),
        (_, _, true, _) => Some(ResizeDirection::North),
        (_, _, _, true) => Some(ResizeDirection::South),
        _ => None,
    }
}

fn cursor(direction: ResizeDirection) -> CursorIcon {
    match direction {
        ResizeDirection::North | ResizeDirection::South => CursorIcon::ResizeVertical,
        ResizeDirection::East | ResizeDirection::West => CursorIcon::ResizeHorizontal,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => CursorIcon::ResizeNwSe,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorIcon::ResizeNeSw,
    }
}

/// True when the mouse press began on an edge or corner, so a drag that
/// follows it is a resize and must not move the window.
pub fn pressed_on_edge(ctx: &egui::Context) -> bool {
    began_on_edge(ctx.content_rect(), ctx.input(|i| i.pointer.press_origin()))
}

fn began_on_edge(rect: Rect, origin: Option<egui::Pos2>) -> bool {
    origin.is_some_and(|at| edge(rect, at).is_some())
}

pub fn resize(ui: &Ui) {
    let ctx = ui.ctx();
    let rect = ctx.content_rect();
    let Some(at) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    let Some(direction) = edge(rect, at) else {
        return;
    };
    ctx.set_cursor_icon(cursor(direction));
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    #[test]
    fn the_edges_and_corners_resize_and_the_middle_does_not() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(300.0, 400.0));
        assert_eq!(edge(rect, pos2(1.0, 1.0)), Some(ResizeDirection::NorthWest));
        assert_eq!(
            edge(rect, pos2(299.0, 399.0)),
            Some(ResizeDirection::SouthEast)
        );
        assert_eq!(
            edge(rect, pos2(299.0, 1.0)),
            Some(ResizeDirection::NorthEast)
        );
        assert_eq!(
            edge(rect, pos2(1.0, 399.0)),
            Some(ResizeDirection::SouthWest)
        );
        assert_eq!(edge(rect, pos2(150.0, 2.0)), Some(ResizeDirection::North));
        assert_eq!(edge(rect, pos2(150.0, 398.0)), Some(ResizeDirection::South));
        assert_eq!(edge(rect, pos2(2.0, 200.0)), Some(ResizeDirection::West));
        assert_eq!(edge(rect, pos2(298.0, 200.0)), Some(ResizeDirection::East));
        assert_eq!(edge(rect, pos2(150.0, 200.0)), None);
        assert_eq!(cursor(ResizeDirection::North), CursorIcon::ResizeVertical);
        assert_eq!(cursor(ResizeDirection::West), CursorIcon::ResizeHorizontal);
        assert_eq!(cursor(ResizeDirection::NorthWest), CursorIcon::ResizeNwSe);
        assert_eq!(cursor(ResizeDirection::SouthWest), CursorIcon::ResizeNeSw);
    }

    #[test]
    fn a_press_on_the_top_edge_resizes_and_does_not_drag_the_window() {
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(300.0, 400.0));
        assert!(
            began_on_edge(rect, Some(pos2(150.0, 2.0))),
            "top edge, in the header"
        );
        assert!(
            began_on_edge(rect, Some(pos2(299.0, 1.0))),
            "top right corner"
        );
        assert!(
            !began_on_edge(rect, Some(pos2(150.0, 20.0))),
            "the header still drags"
        );
        assert!(!began_on_edge(rect, None));
    }
}
