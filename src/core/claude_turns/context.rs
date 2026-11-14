//! How full a chat's context is, for ClaudeDeck's context ring. Read in
//! the same pass as the turn (`Tail`), so nothing is read twice.
//!
//! What was learned from real transcripts (2026-11-14):
//! - An assistant record holds `"usage":{...}` near its end, inside
//!   `message`. The context of that request is `input_tokens +
//!   cache_read_input_tokens + cache_creation_input_tokens`; output tokens
//!   are not part of it.
//! - A failed request (model `<synthetic>`) has a usage of all zeros.
//! - A compaction writes a user record with `"isCompactSummary":true` near
//!   its end, after the summary text.
//! - The model id names a 1M window as `claude-opus-5-5[1m]`.
//! - `settings.json` can hold `autoCompactWindow` at the top level and
//!   again under `modelSettings.<model>`.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;

use super::{prefix, suffix};

const USAGE_KEY: &str = "\"usage\":";
const COMPACT_KEY: &str = "\"isCompactSummary\":true";
/// Where a compact summary record names itself, from its end.
const COMPACT_WITHIN: usize = 3000;
const MODEL_KEY: &str = "\"model\":\"";
/// Where an assistant record names its model, from its start.
const MODEL_WITHIN: usize = 600;
pub const ONE_MILLION: &str = "[1m]";
pub const SMALL_WINDOW: u64 = 200_000;
pub const LARGE_WINDOW: u64 = 1_000_000;
const SETTING_KEY: &str = "autoCompactWindow";
/// The settings file is read at most this often.
const SETTING_EVERY: Duration = Duration::from_secs(60);

/// What the transcript tells of the chat's context.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Context {
    /// The context of the last request, in tokens. None before a request
    /// with a usage was read.
    pub tokens: Option<u64>,
    /// The largest context seen in the part read.
    pub most: u64,
    /// The compactions seen in the part read.
    pub compactions: u32,
    /// The last request's model names a 1M window.
    pub one_million: bool,
}

impl Context {
    /// Looks at one assistant record of the chat.
    pub(super) fn assistant(&mut self, line: &str) {
        let Some(tokens) = usage_tokens(line) else {
            return;
        };
        self.tokens = Some(tokens);
        self.most = self.most.max(tokens);
        self.one_million = model_of(line).is_some_and(|model| model.contains(ONE_MILLION));
    }

    /// Looks at one user record of the chat.
    pub(super) fn user(&mut self, line: &str) {
        if suffix(line, COMPACT_WITHIN).contains(COMPACT_KEY) {
            self.compactions += 1;
        }
    }
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u64>,
}

/// The context of the request an assistant record answers, in tokens.
/// None without a usage, or with a usage of all zeros (a failed request).
pub fn usage_tokens(line: &str) -> Option<u64> {
    // The message's usage comes after its content, so the last one is it.
    let at = line.rfind(USAGE_KEY)? + USAGE_KEY.len();
    let usage = serde_json::Deserializer::from_str(&line[at..])
        .into_iter::<Usage>()
        .next()?
        .ok()?;
    let sum = [
        usage.input_tokens,
        usage.cache_read_input_tokens,
        usage.cache_creation_input_tokens,
    ]
    .into_iter()
    .flatten()
    .sum::<u64>();
    (sum > 0).then_some(sum)
}

fn model_of(line: &str) -> Option<&str> {
    let head = prefix(line, MODEL_WITHIN);
    let from = head.find(MODEL_KEY)? + MODEL_KEY.len();
    let rest = &head[from..];
    Some(&rest[..rest.find('"')?])
}

/// Where Claude Code compacts the chat, in tokens: the user's
/// `autoCompactWindow` when set, else 80% of the model's window. The window
/// is 1M when the model says so or a context over 200k was seen.
pub fn compaction_point(setting: Option<u64>, one_million: bool, most: u64) -> u64 {
    if let Some(setting) = setting.filter(|setting| *setting > 0) {
        return setting;
    }
    let window = if one_million || most > SMALL_WINDOW {
        LARGE_WINDOW
    } else {
        SMALL_WINDOW
    };
    window / 5 * 4
}

/// `autoCompactWindow` from the text of `settings.json`: the top-level key
/// when present, else the first one found deeper in.
pub fn setting_of(text: &str) -> Option<u64> {
    let value: Value = serde_json::from_str(text).ok()?;
    value
        .get(SETTING_KEY)
        .and_then(Value::as_u64)
        .or_else(|| nested(&value))
}

fn nested(value: &Value) -> Option<u64> {
    match value {
        Value::Object(map) => map.iter().find_map(|(key, inner)| {
            if key == SETTING_KEY {
                inner.as_u64()
            } else {
                nested(inner)
            }
        }),
        Value::Array(list) => list.iter().find_map(nested),
        _ => None,
    }
}

fn settings_file() -> Option<PathBuf> {
    let folder = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".claude"))
        })?;
    Some(folder.join("settings.json"))
}

static SETTING: Mutex<Option<(Instant, Option<u64>)>> = Mutex::new(None);

/// The user's `autoCompactWindow`, read from Claude Code's settings at most
/// once a minute. Only read, never written.
pub fn compact_setting() -> Option<u64> {
    let Ok(mut cached) = SETTING.lock() else {
        return None;
    };
    if let Some((at, setting)) = *cached {
        if at.elapsed() < SETTING_EVERY {
            return setting;
        }
    }
    let setting = settings_file()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| setting_of(&text));
    *cached = Some((Instant::now(), setting));
    setting
}

#[cfg(test)]
mod tests;
