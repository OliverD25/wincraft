//! What a token of each kind costs on each Claude model, so ClaudeDeck can
//! weigh a chat's tokens by price. The weights are only compared with each
//! other (shares and bars); WinCraft never shows them as money.
//!
//! Source: the `claude-api` skill of Claude Code 2.1.325, "Current Models"
//! (cached 2026-11-03), `shared/models.md` and `shared/prompt-caching.md`
//! ("Economics"), read on 2026-11-18. Rates are US dollars per million
//! tokens on the Anthropic API. A cache write costs 1.25x the input rate
//! for the 5-minute cache and 2x for the 1-hour cache. Where the skill
//! gives no cache read rate, it is 0.1x the input rate, as the skill says
//! for "most" models.

/// One model's rates, in dollars per million tokens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rates {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
}

impl Rates {
    pub fn write_5m(self) -> f64 {
        self.input * 1.25
    }

    pub fn write_1h(self) -> f64 {
        self.input * 2.0
    }
}

const fn rates(input: f64, output: f64, cache_read: f64) -> Rates {
    Rates {
        input,
        output,
        cache_read,
    }
}

/// Model ids and their rates. A transcript's model id is matched by its
/// start (it may end in a date or "[1m]"); the longest match wins.
const TABLE: &[(&str, Rates)] = &[
    ("claude-fable-5-2", rates(8.0, 40.0, 0.20)),
    ("claude-mythos-5-2", rates(8.0, 40.0, 0.20)),
    ("claude-fable-5-1", rates(10.0, 50.0, 0.25)),
    ("claude-mythos-5-1", rates(10.0, 50.0, 0.25)),
    ("claude-fable-5", rates(10.0, 50.0, 1.00)),
    ("claude-mythos-5", rates(10.0, 50.0, 1.00)),
    ("claude-opus-5-5", rates(4.0, 20.0, 0.20)),
    ("claude-opus-5", rates(5.0, 25.0, 0.50)),
    ("claude-opus-4-8", rates(5.0, 25.0, 0.50)),
    ("claude-opus-4-7", rates(5.0, 25.0, 0.50)),
    ("claude-opus-4-6", rates(5.0, 25.0, 0.50)),
    ("claude-sonnet-5-5", rates(2.0, 10.0, 0.10)),
    ("claude-sonnet-5", rates(2.0, 10.0, 0.20)),
    ("claude-sonnet-4-6", rates(3.0, 15.0, 0.30)),
    // The rate for prompts up to 100K tokens; longer ones cost more.
    ("claude-haiku-5-5", rates(0.10, 0.50, 0.01)),
    ("claude-haiku-4-5", rates(1.0, 5.0, 0.10)),
];

/// The rate a model not in the table is counted at: the current Sonnet.
pub const FALLBACK: Rates = rates(2.0, 10.0, 0.10);

/// The rates of a model id; None when the table does not know it.
pub fn known(model: &str) -> Option<Rates> {
    TABLE
        .iter()
        .filter(|(id, _)| model.starts_with(id))
        .max_by_key(|(id, _)| id.len())
        .map(|(_, rates)| *rates)
}

/// The rates of a model id, the Sonnet rate when it is not known.
pub fn of(model: &str) -> Rates {
    known(model).unwrap_or(FALLBACK)
}
