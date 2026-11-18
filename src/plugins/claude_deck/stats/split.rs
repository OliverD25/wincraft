//! The Top chats bars cut by model (design section 16, A1): each model
//! keeps one colour on the whole tab, so a chat's parts, the legend under
//! the list and the Models section agree. Opus is the accent, Sonnet
//! green, Haiku amber; any other model takes the next free colour of the
//! Models section's row (info, danger, then grey).

use crate::core::claude_usage::{ChatUse, Span};

use super::view::model_name;

/// A model's colour, as a theme token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hue {
    Accent,
    Success,
    Warning,
    Info,
    Danger,
    Grey,
}

/// One piece of a chat's bar.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub hue: Hue,
    /// The piece's length, 0 to 1, against the biggest bar.
    pub length: f32,
}

fn family_hue(id: &str) -> Option<Hue> {
    let name = model_name(id).to_ascii_lowercase();
    match name.split(' ').next() {
        Some("opus") => Some(Hue::Accent),
        Some("sonnet") => Some(Hue::Success),
        Some("haiku") => Some(Hue::Warning),
        _ => None,
    }
}

/// Each model of the span with its colour, in the span's order (biggest
/// first). A model with no weight is left out, as on the Models bar.
pub fn hues(span: &Span) -> Vec<(String, Hue)> {
    let mut out: Vec<(String, Hue)> = Vec::new();
    let mut spare = [Hue::Info, Hue::Danger].into_iter();
    for model in span.models.iter().filter(|m| m.weight > 0.0) {
        let used = |hue: Hue| out.iter().any(|(_, h)| *h == hue);
        let name = model_name(&model.model);
        let twin = out
            .iter()
            .find(|(m, _)| model_name(m) == name)
            .map(|(_, h)| *h);
        let hue = match (twin, family_hue(&model.model)) {
            (Some(hue), _) => hue,
            (None, Some(hue)) if !used(hue) => hue,
            _ => spare.next().unwrap_or(Hue::Grey),
        };
        out.push((model.model.clone(), hue));
    }
    out
}

fn hue_of(hues: &[(String, Hue)], model: &str) -> Hue {
    hues.iter()
        .find(|(m, _)| m == model)
        .map_or(Hue::Grey, |(_, h)| *h)
}

/// A chat's bar of `length` cut into its models, in the span's model
/// order, each as long as its part of the chat's weight.
pub fn parts(chat: &ChatUse, length: f32, hues: &[(String, Hue)]) -> Vec<Part> {
    if chat.weight <= 0.0 {
        return Vec::new();
    }
    let mut out: Vec<(usize, Part)> = chat
        .models
        .iter()
        .filter(|(_, w)| *w > 0.0)
        .map(|(model, weight)| {
            let at = hues
                .iter()
                .position(|(m, _)| m == model)
                .unwrap_or(usize::MAX);
            let part = Part {
                hue: hue_of(hues, model),
                length: length * (*weight / chat.weight) as f32,
            };
            (at, part)
        })
        .collect();
    out.sort_by_key(|(at, _)| *at);
    out.into_iter().map(|(_, part)| part).collect()
}

/// The legend under the Top chats: the models of the listed chats, by
/// name, in the span's order.
pub fn legend(chats: &[&ChatUse], hues: &[(String, Hue)]) -> Vec<(String, Hue)> {
    let mut out: Vec<(String, Hue)> = Vec::new();
    for (model, hue) in hues {
        let listed = chats
            .iter()
            .any(|c| c.models.iter().any(|(m, w)| m == model && *w > 0.0));
        let name = model_name(model);
        if listed && !out.iter().any(|(n, _)| *n == name) {
            out.push((name, *hue));
        }
    }
    out
}
