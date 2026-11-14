//! The context ring (design section 15): how full a chat's context is,
//! where 100% is the point where Claude Code compacts the chat.

use crate::core::claude_turns::context::{self, Context, ONE_MILLION};

/// A chat's context, when its size is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fill {
    /// The context of the last request, in tokens.
    pub tokens: u64,
    /// Where the chat is compacted, in tokens.
    pub limit: u64,
    /// How many times the chat was compacted.
    pub compactions: u32,
}

/// The fill of a chat from what its transcript tells. `setting` is the
/// user's `autoCompactWindow`; `model` the session's model id.
pub fn fill(context: &Context, setting: Option<u64>, model: Option<&str>) -> Option<Fill> {
    let tokens = context.tokens?;
    let one_million = context.one_million || model.is_some_and(|model| model.contains(ONE_MILLION));
    Some(Fill {
        tokens,
        limit: context::compaction_point(setting, one_million, context.most),
        compactions: context.compactions,
    })
}

#[cfg(test)]
mod tests;
