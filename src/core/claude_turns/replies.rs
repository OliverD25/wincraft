//! The finished replies of a chat, for ClaudeDeck's News tab. Read in the
//! same pass as the turn (`Tail`), one line at a time, so nothing is read
//! twice.
//!
//! A finished reply is the text Claude shows at the end of a turn: an
//! assistant record with text that is not followed by a tool call before
//! the next user record. The same rule as `End::Replied`: a turn whose
//! last record is such a text has ended. Text records before a tool call
//! are Claude's words on the way, not a reply. Several text records in a
//! row with no tool between them are one reply. A failed request is not a
//! reply.
//!
//! Only Claude's visible text is kept, as written (Markdown as it is),
//! cut to `REPLY_CAP` characters.

use std::collections::VecDeque;

use serde_json::Value;

use super::{kind_of, text_of, words, Kind, MOST_TEXT_LINE};

/// A reply is cut to this many characters.
pub const REPLY_CAP: usize = 4000;
/// How many finished replies are kept per chat, newest last: the News tab
/// shows at most 20 of the last 24 hours, plus the newest older one.
pub const KEEP_REPLIES: usize = 21;

/// One reply Claude finished.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    /// When it was written, in Unix ms (the record's `timestamp`); 0 when
    /// the record has none.
    pub at: u64,
    pub text: String,
}

/// The replies of one transcript, kept while it is read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Replies {
    done: VecDeque<Reply>,
    /// The text the transcript ends with so far: a reply unless a tool
    /// call follows.
    pending: Option<Reply>,
}

impl Replies {
    /// Looks at one line of the transcript.
    pub fn line(&mut self, line: &str) {
        let Some(kind) = kind_of(line) else {
            return;
        };
        match kind {
            // A user record starts a new turn (a message, a stop, a task
            // notice): the text before it ended the last one. A tool's
            // result only follows a tool call, which already dropped the
            // text before it.
            Kind::User => self.commit(),
            Kind::Assistant => self.assistant(line),
        }
    }

    pub fn reset(&mut self) {
        *self = Replies::default();
    }

    /// The finished replies, oldest first, with the text the transcript
    /// ends with.
    pub fn all(&self) -> Vec<Reply> {
        self.done
            .iter()
            .chain(self.pending.iter())
            .cloned()
            .collect()
    }

    fn assistant(&mut self, line: &str) {
        if line.contains("\"isApiErrorMessage\":true") {
            return;
        }
        if line.contains("\"type\":\"tool_use\"") {
            self.pending = None;
            return;
        }
        if line.len() > MOST_TEXT_LINE || !line.contains("\"type\":\"text\"") {
            return;
        }
        let Some(text) = serde_json::from_str::<Value>(line)
            .ok()
            .as_ref()
            .and_then(text_of)
        else {
            return;
        };
        let text = text.replace('\r', "");
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let at = crate::core::claude_jobs::timestamp_ms(line).unwrap_or(0);
        let text = match self.pending.take() {
            Some(before) => format!("{}\n\n{text}", before.text),
            None => text.to_string(),
        };
        self.pending = Some(Reply {
            at,
            text: words::cap(&text, REPLY_CAP),
        });
    }

    fn commit(&mut self) {
        let Some(reply) = self.pending.take() else {
            return;
        };
        if self.done.len() == KEEP_REPLIES {
            self.done.pop_front();
        }
        self.done.push_back(reply);
    }
}

#[cfg(test)]
mod tests;
