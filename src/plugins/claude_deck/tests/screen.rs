//! A headless egui screen: runs frames of the drawing code with chosen
//! input and reads back what was painted, with no window.

use egui::{pos2, vec2, Event, Rect, Shape, Ui};

use crate::core::config::{ThemeChoice, ThemeName};
use crate::core::theme::{self, Tokens};

pub const WIDTH: f32 = 360.0;
pub const HEIGHT: f32 = 480.0;

pub struct Screen {
    pub ctx: egui::Context,
    time: f64,
}

/// What one frame painted.
pub struct Painted {
    pub shapes: usize,
    pub texts: Vec<String>,
    /// Where each text was painted, on the screen.
    pub rects: Vec<(String, Rect)>,
}

impl Painted {
    pub fn has(&self, words: &str) -> bool {
        self.texts.iter().any(|text| text.contains(words))
    }

    /// The middle of the first text that contains `words`.
    pub fn at(&self, words: &str) -> egui::Pos2 {
        let (_, rect) = self
            .rects
            .iter()
            .find(|(text, _)| text.contains(words))
            .unwrap_or_else(|| panic!("{words:?} not painted: {:?}", self.texts));
        rect.center()
    }
}

fn collect(shape: &Shape, out: &mut Painted) {
    match shape {
        Shape::Vec(inner) => inner.iter().for_each(|shape| collect(shape, out)),
        Shape::Text(text) => {
            out.shapes += 1;
            out.texts.push(text.galley.text().to_string());
            let rect = text.galley.rect.translate(text.pos.to_vec2());
            out.rects.push((text.galley.text().to_string(), rect));
        }
        _ => out.shapes += 1,
    }
}

impl Screen {
    pub fn new(mode: ThemeChoice) -> Self {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(&ctx, ThemeName::Classic, mode);
        Self { ctx, time: 1.0 }
    }

    pub fn tokens(&self) -> Tokens {
        Tokens::get(&self.ctx)
    }

    pub fn frame(&mut self, events: Vec<Event>, mut draw: impl FnMut(&mut Ui)) -> Painted {
        self.time += 0.1;
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(WIDTH, HEIGHT))),
            time: Some(self.time),
            events,
            focused: true,
            ..Default::default()
        };
        let mut output = self.ctx.run_ui(input, |ui| draw(ui));
        output.textures_delta.clear();
        let mut painted = Painted {
            shapes: 0,
            texts: Vec::new(),
            rects: Vec::new(),
        };
        for clipped in &output.shapes {
            collect(&clipped.shape, &mut painted);
        }
        painted
    }

    /// Frames with nothing pressed, to let the layout settle.
    pub fn settle(&mut self, draw: &mut impl FnMut(&mut Ui)) -> Painted {
        let mut last = self.frame(Vec::new(), &mut *draw);
        for _ in 0..2 {
            last = self.frame(Vec::new(), &mut *draw);
        }
        last
    }

    /// A left click at `at`: the pointer moves there in one frame, then
    /// the button goes down and up in the next.
    pub fn click(&mut self, at: egui::Pos2, draw: &mut impl FnMut(&mut Ui)) -> Painted {
        self.right_or_left(at, egui::PointerButton::Primary, draw)
    }

    pub fn right_click(&mut self, at: egui::Pos2, draw: &mut impl FnMut(&mut Ui)) -> Painted {
        self.right_or_left(at, egui::PointerButton::Secondary, draw)
    }

    fn right_or_left(
        &mut self,
        at: egui::Pos2,
        button: egui::PointerButton,
        draw: &mut impl FnMut(&mut Ui),
    ) -> Painted {
        self.frame(vec![Event::PointerMoved(at)], &mut *draw);
        self.frame(vec![Event::PointerMoved(at)], &mut *draw);
        let press = |pressed| Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame(vec![press(true)], &mut *draw);
        self.frame(vec![press(false)], &mut *draw)
    }

    pub fn key(
        &mut self,
        key: egui::Key,
        modifiers: egui::Modifiers,
        draw: &mut impl FnMut(&mut Ui),
    ) -> Painted {
        let event = |pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        self.frame(vec![event(true), event(false)], &mut *draw)
    }
}
