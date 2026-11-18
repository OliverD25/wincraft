//! The Stats tab's numbers and words, kept apart from the drawing so they
//! can be tested: the bars of the chats and groups, the names of models
//! and folders, and the tooltips with the raw token counts.

use std::collections::HashMap;

use crate::core::claude_usage::{self, ChatUse, Span, Tokens};
use crate::plugins::claude_deck::model::Chat;
use crate::plugins::claude_deck::store::Saved;

use super::split::{self, Hue, Part};

pub const TAB: &str = "Stats";
pub const TODAY: &str = "Today";
pub const WEEK: &str = "7 days";
pub const EMPTY: &str = "No Claude Code chats in this time yet.";
pub const NOTE: &str = "Bars weigh each token by its model's API price.";
pub const CHATS: &str = "Top chats";
pub const GROUPS: &str = "Groups";
pub const DAYS: &str = "Days";
pub const MODELS: &str = "Models";
pub const HOURS: &str = "Busiest hours";
pub const NO_GROUP: &str = "Not in a group";
/// How many chats the tab lists.
pub const TOP: usize = 10;
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// "Counting… 120 of 1,538 files".
pub fn counting(done: usize, total: usize) -> String {
    format!(
        "Counting\u{2026} {} of {} files",
        thousands(done),
        thousands(total)
    )
}

fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (at, c) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A token count in short: "950", "34k", "1.2M", "3.4B".
pub fn count(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{}k", (n + 500) / 1_000),
        1_000_000..1_000_000_000 => format!("{:.1}M", n as f64 / 1e6),
        _ => format!("{:.1}B", n as f64 / 1e9),
    }
}

/// A share as "34%", or "<1%" for a small one that is not nothing.
pub fn percent(part: f64, whole: f64) -> String {
    if whole <= 0.0 || part <= 0.0 {
        return "0%".to_string();
    }
    let p = part * 100.0 / whole;
    if p < 1.0 {
        "<1%".to_string()
    } else {
        format!("{}%", p.round() as u64)
    }
}

/// "claude-opus-5-5[1m]" as "Opus 5.5"; a date at the end is left out.
pub fn model_name(id: &str) -> String {
    let id = id.split('[').next().unwrap_or(id);
    let id = id.strip_prefix("claude-").unwrap_or(id);
    let mut parts = id
        .split('-')
        .filter(|p| !(p.len() == 8 && p.bytes().all(|b| b.is_ascii_digit())));
    let Some(family) = parts.next() else {
        return id.to_string();
    };
    let mut name: String = family
        .chars()
        .enumerate()
        .map(|(at, c)| if at == 0 { c.to_ascii_uppercase() } else { c })
        .collect();
    let version: Vec<&str> = parts.collect();
    if !version.is_empty() {
        name.push(' ');
        name.push_str(&version.join("."));
    }
    name
}

/// A project folder's name under `projects` in short: the part after its
/// last "--", "E--code--wincraft" as "wincraft".
pub fn folder_name(folder: &str) -> String {
    folder
        .rsplit("--")
        .next()
        .filter(|tail| !tail.is_empty())
        .unwrap_or(folder)
        .to_string()
}

/// Each listed chat's mark: its length against the biggest chat of the
/// span, and its share of the span's weight.
pub fn shares(span: &Span, chat_of: &HashMap<String, String>) -> HashMap<String, Share> {
    let most = span.chats.first().map_or(0.0, |c| c.weight);
    if most <= 0.0 || span.weight <= 0.0 {
        return HashMap::new();
    }
    span.chats
        .iter()
        .filter(|c| c.weight > 0.0)
        .filter_map(|c| {
            let chat = chat_of.get(&c.session)?;
            let share = Share {
                length: (c.weight / most) as f32,
                percent: c.weight * 100.0 / span.weight,
            };
            Some((chat.clone(), share))
        })
        .collect()
}

/// A chat's token mark.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Share {
    /// 0 to 1: the chat that used most is 1.
    pub length: f32,
    /// Its share of the span, 0 to 100.
    pub percent: f64,
}

/// One bar on the tab.
#[derive(Clone, Debug, PartialEq)]
pub struct Bar {
    pub name: String,
    /// The length, 0 to 1, against the biggest bar.
    pub length: f32,
    /// "34%" of the span's weight; a chat's says its tokens first,
    /// "41.2M tokens · 34%".
    pub share: String,
    pub tip: String,
    /// The ClaudeDeck chat a click opens, if it is listed.
    pub chat: Option<String>,
    /// A chat's bar cut by model; empty for the other bars.
    pub parts: Vec<Part>,
}

/// A chat's right text: "41.2M tokens · 31%".
pub fn chat_share(chat: &ChatUse, span: &Span) -> String {
    format!(
        "{} tokens \u{b7} {}",
        count(chat.tokens.total()),
        percent(chat.weight, span.weight)
    )
}

/// The tooltip of a chat: its raw tokens, subagent and cache shares, and
/// its context now.
pub fn chat_tip(chat: &ChatUse) -> String {
    let t = &chat.tokens;
    let mut tip = format!(
        "Input {} \u{b7} Output {} \u{b7} Cache read {} \u{b7} Cache write {}",
        count(t.input),
        count(t.output),
        count(t.read),
        count(t.write)
    );
    tip.push_str(&format!(
        "\nSubagents: {}",
        percent(chat.subagent_weight, chat.weight)
    ));
    tip.push_str(&format!(
        "\nCache read: {} of the tokens",
        percent(t.read as f64, t.total() as f64)
    ));
    if let Some(context) = chat.context {
        tip.push_str(&format!("\nContext now: {}", count(context)));
    }
    if chat.other_model {
        tip.push_str("\nOther model: counted at the Sonnet price");
    }
    tip
}

fn tokens_tip(t: &Tokens) -> String {
    format!(
        "{} tokens: input {}, output {}, cache read {}, cache write {}",
        count(t.total()),
        count(t.input),
        count(t.output),
        count(t.read),
        count(t.write)
    )
}

/// The top chats of a span, with their names from ClaudeDeck's list or,
/// for a chat not listed, from its project folder, each bar cut by model.
pub fn chat_bars(
    span: &Span,
    chats: &[Chat],
    chat_of: &HashMap<String, String>,
    hues: &[(String, Hue)],
) -> Vec<Bar> {
    let most = span.chats.first().map_or(0.0, |c| c.weight);
    span.chats
        .iter()
        .take(TOP)
        .map(|c| {
            let chat = chat_of
                .get(&c.session)
                .and_then(|id| chats.iter().find(|chat| &chat.id == id));
            let name = chat.map_or_else(
                || format!("{} (not listed)", folder_name(&c.folder)),
                |chat| chat.title.clone(),
            );
            let length = if most > 0.0 {
                (c.weight / most) as f32
            } else {
                0.0
            };
            Bar {
                name,
                length,
                share: chat_share(c, span),
                tip: chat_tip(c),
                chat: chat.map(|chat| chat.id.clone()),
                parts: split::parts(c, length, hues),
            }
        })
        .collect()
}

/// The group a chat is in: its own, or the one of the chat it was forked
/// from.
fn group_of<'a>(chat_id: &str, chats: &[Chat], saved: &'a Saved) -> Option<&'a str> {
    let mut id = chat_id.to_string();
    for _ in 0..16 {
        if let Some(group) = saved.groups.iter().find(|g| g.chats.contains(&id)) {
            return Some(group.name.as_str());
        }
        id = chats.iter().find(|c| c.id == id)?.parent.clone()?;
    }
    None
}

/// The span's weight by ClaudeDeck group, biggest first.
pub fn group_bars(
    span: &Span,
    chats: &[Chat],
    chat_of: &HashMap<String, String>,
    saved: &Saved,
) -> Vec<Bar> {
    let mut sums: Vec<(String, f64, Tokens)> = Vec::new();
    for c in &span.chats {
        let name = chat_of
            .get(&c.session)
            .and_then(|id| group_of(id, chats, saved))
            .unwrap_or(NO_GROUP)
            .to_string();
        match sums.iter_mut().find(|(n, _, _)| *n == name) {
            Some(sum) => {
                sum.1 += c.weight;
                sum.2.add(&c.tokens);
            }
            None => sums.push((name, c.weight, c.tokens)),
        }
    }
    sums.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let most = sums.first().map_or(0.0, |s| s.1);
    sums.into_iter()
        .map(|(name, weight, tokens)| Bar {
            length: if most > 0.0 {
                (weight / most) as f32
            } else {
                0.0
            },
            share: percent(weight, span.weight),
            tip: tokens_tip(&tokens),
            name,
            chat: None,
            parts: Vec::new(),
        })
        .collect()
}

/// The models of a span, for the stacked bar and its legend.
pub fn model_bars(span: &Span) -> Vec<Bar> {
    span.models
        .iter()
        .filter(|m| m.weight > 0.0)
        .map(|m| {
            let mut tip = tokens_tip(&m.tokens);
            if !m.known {
                tip.push_str("\nOther model: counted at the Sonnet price");
            }
            Bar {
                name: model_name(&m.model),
                length: if span.weight > 0.0 {
                    (m.weight / span.weight) as f32
                } else {
                    0.0
                },
                share: percent(m.weight, span.weight),
                tip,
                chat: None,
                parts: Vec::new(),
            }
        })
        .collect()
}

/// A day's short name, "Mon", or "Today".
pub fn day_label(day: i32, today: i32) -> String {
    if day == today {
        TODAY.to_string()
    } else {
        WEEKDAYS[claude_usage::weekday(day) as usize].to_string()
    }
}

/// Column heights, 0 to 1, against the biggest.
pub fn heights(values: &[f64]) -> Vec<f32> {
    let most = values.iter().copied().fold(0.0, f64::max);
    values
        .iter()
        .map(|v| if most > 0.0 { (v / most) as f32 } else { 0.0 })
        .collect()
}
