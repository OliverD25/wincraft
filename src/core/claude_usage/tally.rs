//! The read buckets summed for one time span: per chat, per model, per
//! day and per hour of the day. Every sum is weighed by model price
//! (`Tokens::weight`); the raw tokens are kept for tooltips.

use std::collections::HashMap;

use super::files::FileState;
use super::parse::Tokens;
use super::prices;

/// One chat's use in a span. Its subagents are in it, and also apart.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChatUse {
    /// The Claude Code session id.
    pub session: String,
    /// The project folder's name under `projects`.
    pub folder: String,
    pub tokens: Tokens,
    pub weight: f64,
    pub subagent_tokens: Tokens,
    pub subagent_weight: f64,
    /// The context of its last reply, any time in the last days.
    pub context: Option<u64>,
    /// Some tokens are of a model the price table does not know.
    pub other_model: bool,
    /// The chat's weight per model id, subagents in it, biggest first.
    pub models: Vec<(String, f64)>,
}

/// One model's use in a span.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelUse {
    pub model: String,
    pub tokens: Tokens,
    pub weight: f64,
    pub known: bool,
}

/// The sums of one span, biggest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Span {
    pub chats: Vec<ChatUse>,
    pub models: Vec<ModelUse>,
    /// Each day of the span, oldest first, with its weight.
    pub days: Vec<(i32, f64)>,
    pub hours: [f64; 24],
    pub tokens: Tokens,
    pub weight: f64,
}

/// Sums the buckets of days `first..=last`.
pub fn span<'a>(files: impl Iterator<Item = &'a FileState>, first: i32, last: i32) -> Span {
    let mut chats: HashMap<&str, ChatUse> = HashMap::new();
    let mut models: HashMap<&str, ModelUse> = HashMap::new();
    let mut days: Vec<(i32, f64)> = (first..=last).map(|day| (day, 0.0)).collect();
    let mut out = Span::default();
    for file in files {
        let chat = chats
            .entry(file.session.as_str())
            .or_insert_with(|| ChatUse {
                session: file.session.clone(),
                folder: file.folder.clone(),
                ..ChatUse::default()
            });
        if !file.subagent {
            if let Some((_, context)) = file.context {
                chat.context = Some(context);
            }
        }
        for bucket in file
            .buckets
            .iter()
            .filter(|b| b.day >= first && b.day <= last)
        {
            let weight = bucket.tokens.weight(&bucket.model);
            let known = prices::known(&bucket.model).is_some();
            chat.tokens.add(&bucket.tokens);
            chat.weight += weight;
            chat.other_model |= !known;
            match chat.models.iter_mut().find(|(m, _)| *m == bucket.model) {
                Some(part) => part.1 += weight,
                None => chat.models.push((bucket.model.clone(), weight)),
            }
            if file.subagent {
                chat.subagent_tokens.add(&bucket.tokens);
                chat.subagent_weight += weight;
            }
            let model = models
                .entry(bucket.model.as_str())
                .or_insert_with(|| ModelUse {
                    model: bucket.model.clone(),
                    known,
                    ..ModelUse::default()
                });
            model.tokens.add(&bucket.tokens);
            model.weight += weight;
            if let Some(day) = days.iter_mut().find(|(day, _)| *day == bucket.day) {
                day.1 += weight;
            }
            out.hours[usize::from(bucket.hour.min(23))] += weight;
            out.tokens.add(&bucket.tokens);
            out.weight += weight;
        }
    }
    out.chats = chats
        .into_values()
        .filter(|c| c.tokens.total() > 0)
        .collect();
    for chat in &mut out.chats {
        chat.models
            .sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    }
    out.chats.sort_by(|a, b| {
        b.weight
            .total_cmp(&a.weight)
            .then_with(|| a.session.cmp(&b.session))
    });
    out.models = models.into_values().collect();
    out.models.sort_by(|a, b| {
        b.weight
            .total_cmp(&a.weight)
            .then_with(|| a.model.cmp(&b.model))
    });
    out.days = days;
    out
}
