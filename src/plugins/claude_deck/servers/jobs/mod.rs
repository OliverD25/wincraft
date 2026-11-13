//! Work that PC chats started on the servers ("Work from the PC"): long
//! jobs in user units, `nohup`, `setsid` or `tmux`, and a chat's running
//! background ssh command. Only work a PC chat started shows; a timer's
//! job or a service started at boot does not.
//!
//! - `load`: the probe's `JOB` and `UNIT` lines.
//! - `link`: which PC chat a job belongs to (pure).

pub mod link;
pub mod load;

use crate::core::claude_launches::Launch;

/// A PC chat with its ssh calls, for linking.
#[derive(Clone, Copy, Debug)]
pub struct PcChat<'a> {
    pub id: &'a str,
    pub title: &'a str,
    /// The chat's project folder (a worktree is its repo).
    pub folder: Option<&'a str>,
    pub launches: &'a [Launch],
}

/// A job on a server that belongs to PC chats.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Job {
    pub server: String,
    /// The chats it belongs to, id and title: the one that started it, or
    /// every chat of its repo.
    pub chats: Vec<(String, String)>,
    /// Set when it was linked by its folder: the repo's name.
    pub repo: Option<String>,
    /// The unit without `.service`, or the program words.
    pub name: String,
    /// The command line.
    pub command: String,
    pub folder: Option<String>,
    /// None for a background ssh call.
    pub pid: Option<u32>,
    pub unit: Option<String>,
    /// When it started, in the PC's Unix ms.
    pub started: Option<u64>,
    /// CPU in tenths of a percent and RAM in kB; None for an ssh call.
    pub cpu10: Option<u32>,
    pub rss_kb: Option<u64>,
}

impl Job {
    /// The one chat it belongs to; None when it is a repo's.
    pub fn chat(&self) -> Option<&(String, String)> {
        match self.chats.as_slice() {
            [one] => Some(one),
            _ => None,
        }
    }

    /// Who it belongs to, in words: the chat's name, or "verdict-ai chats".
    pub fn owner(&self) -> String {
        match (self.chat(), &self.repo) {
            (Some((_, title)), _) => title.clone(),
            (None, Some(repo)) => format!("{repo} chats"),
            (None, None) => "PC chats".to_string(),
        }
    }

    /// What tells it apart from other jobs, for the 24 h history.
    pub fn key(&self) -> String {
        match (&self.unit, self.pid) {
            (Some(unit), _) => format!("{}/{unit}", self.server),
            (None, Some(pid)) => format!("{}/pid {pid}", self.server),
            (None, None) => format!(
                "{}/call {}/{}",
                self.server,
                self.chats.first().map_or("", |(id, _)| id.as_str()),
                self.started.unwrap_or(0)
            ),
        }
    }
}

#[cfg(test)]
mod link_tests;
#[cfg(test)]
mod tests;
