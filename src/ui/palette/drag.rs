//! Moving the palette by pressing and holding on its blank space.
//!
//! The palette does not put a widget under the panel for this. A widget that
//! senses drags would also take the drag from the buttons above it (egui
//! gives a drag to the widget below when the one above only senses clicks).
//! Instead the press is watched: if egui gave it to no widget, it is on blank
//! space.

use super::*;

/// How far the pointer moves, in points, before a held press becomes a drag.
const START_DISTANCE: f32 = 4.0;

/// What the window should do after this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Step {
    #[default]
    None,
    /// Let Windows move the window.
    StartDrag,
    /// Double-click: back to the default spot.
    Centre,
}

/// The pointer, as one frame of the palette saw it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pointer {
    pub pressed: bool,
    pub down: bool,
    pub released: bool,
    pub double_click: bool,
    pub pos: Option<egui::Pos2>,
    /// The press is inside the panel (not in the shadow margin).
    pub in_panel: bool,
    /// egui gave the press to a widget: a button, a row or the search box.
    pub taken: bool,
    /// The press is in the search box's empty part, right of the text (or
    /// anywhere in it while it is empty). The box takes the press, but a
    /// drag from there still moves the window.
    pub in_search_blank: bool,
}

#[derive(Default)]
pub struct BlankPress {
    origin: Option<egui::Pos2>,
    /// The press began in the search box's empty part.
    from_search: bool,
}

impl BlankPress {
    pub fn step(&mut self, pointer: Pointer) -> Step {
        if pointer.pressed {
            let blank = !pointer.taken || pointer.in_search_blank;
            self.origin = pointer.pos.filter(|_| pointer.in_panel && blank);
            self.from_search = pointer.in_search_blank;
        }
        let Some(origin) = self.origin else {
            return Step::None;
        };
        if pointer.released {
            self.origin = None;
            // A double-click in the search box selects a word there; it does
            // not centre the palette.
            return if pointer.double_click && !self.from_search {
                Step::Centre
            } else {
                Step::None
            };
        }
        if !pointer.down {
            self.origin = None;
            return Step::None;
        }
        match pointer.pos {
            Some(pos) if pos.distance(origin) > START_DISTANCE => {
                self.origin = None;
                Step::StartDrag
            }
            _ => Step::None,
        }
    }

    pub fn cancel(&mut self) {
        self.origin = None;
    }
}

impl Palette {
    /// Watches the pointer for a press and hold on blank space; called after
    /// the frame's widgets exist, so egui has said who took the press.
    pub(super) fn watch_blank_space(&mut self, ctx: &egui::Context, interactive: bool) {
        if !interactive {
            self.blank.cancel();
            return;
        }
        let mut pointer = ctx.input(|input| {
            let pos = input.pointer.interact_pos();
            Pointer {
                pressed: input.pointer.button_pressed(egui::PointerButton::Primary),
                down: input.pointer.button_down(egui::PointerButton::Primary),
                released: input.pointer.button_released(egui::PointerButton::Primary),
                double_click: input
                    .pointer
                    .button_double_clicked(egui::PointerButton::Primary),
                pos,
                in_panel: pos.is_some_and(|pos| self.panel.contains(pos)),
                taken: false,
                in_search_blank: pos.is_some_and(|pos| self.search_blank.contains(pos)),
            }
        });
        pointer.taken = ctx.egui_is_using_pointer();
        let step = self.blank.step(pointer);
        if step == Step::StartDrag {
            // The search box took the press and would go on selecting text
            // as the mouse moves; the window moves instead.
            ctx.stop_dragging();
        }
        if step != Step::None {
            self.window_step = step;
        }
    }

    /// What the window should do now; read once per frame.
    pub fn take_window_step(&mut self) -> Step {
        std::mem::take(&mut self.window_step)
    }
}
