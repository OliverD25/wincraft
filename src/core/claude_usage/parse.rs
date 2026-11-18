//! One transcript line to one reply's tokens. Only assistant lines carry a
//! `message.usage`; a cheap text search finds them before serde reads the
//! small usage object. A line of another shape, or a broken one, is no
//! reply: it is skipped, never an error.

use serde::{Deserialize, Serialize};

use crate::core::claude_jobs::timestamp_ms;

use super::prices;

/// The start of a line that holds the message's role, model and id.
const HEAD: usize = 800;
const USAGE_KEY: &str = "\"usage\":{";

/// Tokens of the four kinds. `write_1h` is the part of `write` that went
/// to the 1-hour cache, which costs more.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    #[serde(rename = "i", default)]
    pub input: u64,
    #[serde(rename = "o", default)]
    pub output: u64,
    #[serde(rename = "r", default)]
    pub read: u64,
    #[serde(rename = "w", default)]
    pub write: u64,
    #[serde(rename = "w1", default)]
    pub write_1h: u64,
}

impl Tokens {
    pub fn add(&mut self, other: &Tokens) {
        self.input += other.input;
        self.output += other.output;
        self.read += other.read;
        self.write += other.write;
        self.write_1h += other.write_1h;
    }

    pub fn take(&mut self, other: &Tokens) {
        self.input = self.input.saturating_sub(other.input);
        self.output = self.output.saturating_sub(other.output);
        self.read = self.read.saturating_sub(other.read);
        self.write = self.write.saturating_sub(other.write);
        self.write_1h = self.write_1h.saturating_sub(other.write_1h);
    }

    /// Every token, each kind once.
    pub fn total(&self) -> u64 {
        self.input + self.output + self.read + self.write
    }

    /// The context the request was sent with: input, cache read and cache
    /// write.
    pub fn context(&self) -> u64 {
        self.input + self.read + self.write
    }

    /// The tokens weighed by the model's price, in dollars. Only compared
    /// with other weights, never shown.
    pub fn weight(&self, model: &str) -> f64 {
        let rates = prices::of(model);
        let write_1h = self.write_1h.min(self.write);
        let write_5m = self.write - write_1h;
        (self.input as f64 * rates.input
            + self.output as f64 * rates.output
            + self.read as f64 * rates.cache_read
            + write_5m as f64 * rates.write_5m()
            + write_1h as f64 * rates.write_1h())
            / 1_000_000.0
    }
}

/// One reply of Claude, as its line tells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    /// The API message id. A reply with several blocks is written as
    /// several lines with the same id and the same usage.
    pub id: Option<String>,
    pub model: String,
    pub at_ms: u64,
    pub tokens: Tokens,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
    #[serde(default)]
    cache_creation: Option<Creation>,
}

#[derive(Deserialize)]
struct Creation {
    #[serde(default)]
    ephemeral_1h_input_tokens: Option<u64>,
}

/// The cheap test: could this line be a reply with tokens?
pub fn may_have_usage(line: &str) -> bool {
    line.contains(USAGE_KEY)
}

fn head(line: &str) -> &str {
    let mut end = line.len().min(HEAD);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    &line[..end]
}

fn value_after<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let from = text.find(key)? + key.len();
    let rest = &text[from..];
    Some(&rest[..rest.find('"')?])
}

/// The reply a line holds; None for any other line, a broken one, or a
/// reply with no tokens (a failed or made-up one).
pub fn reply(line: &str) -> Option<Reply> {
    if !may_have_usage(line) {
        return None;
    }
    let head = head(line);
    if !head.contains("\"role\":\"assistant\"") {
        return None;
    }
    let model = value_after(head, "\"model\":\"")?;
    if model.starts_with('<') {
        return None;
    }
    let id = value_after(head, "\"id\":\"msg_").map(|id| format!("msg_{id}"));
    let at = line.rfind(USAGE_KEY)? + USAGE_KEY.len() - 1;
    let usage = serde_json::Deserializer::from_str(&line[at..])
        .into_iter::<Usage>()
        .next()?
        .ok()?;
    let write = usage.cache_creation_input_tokens.unwrap_or(0);
    let tokens = Tokens {
        input: usage.input_tokens.unwrap_or(0),
        output: usage.output_tokens.unwrap_or(0),
        read: usage.cache_read_input_tokens.unwrap_or(0),
        write,
        write_1h: usage
            .cache_creation
            .and_then(|c| c.ephemeral_1h_input_tokens)
            .unwrap_or(0)
            .min(write),
    };
    if tokens.total() == 0 {
        return None;
    }
    // The line's own time comes after the message, so it is the last one.
    let stamp = line.rfind("\"timestamp\":\"")?;
    let at_ms = timestamp_ms(&line[stamp..])?;
    Some(Reply {
        id,
        model: model.to_string(),
        at_ms,
        tokens,
    })
}
