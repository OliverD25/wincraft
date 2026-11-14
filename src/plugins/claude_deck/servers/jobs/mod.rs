//! Work that PC chats started on the servers ("Work from the PC"): long
//! jobs in user units, `nohup`, `setsid` or `tmux`, and a chat's running
//! background ssh command. Only work a PC chat started shows; a timer's
//! job or a service started at boot does not.
//!
//! - `load`: the probe's `JOB` and `UNIT` lines.
//! - `link`: which PC chat a job belongs to (pure).
//! - `text`: the words about jobs (pure).
//! - `history`: the jobs of the last 24 h that ended.

pub mod history;
pub mod link;
pub mod load;
pub mod publish;
pub mod text;

use std::collections::HashMap;
use std::sync::Arc;

use crate::core::claude_jobs;
use crate::core::claude_launches::Launch;
use crate::core::claude_sessions::Session;
use crate::plugins::claude_deck::model::{Chat, Mark};

use super::Deck;

/// Links the servers' jobs to the PC chats, asks the readers about the
/// units the chats started, and marks the chats. Called each time the
/// list is built.
pub fn refresh(deck: &mut Deck, chats: &mut [Chat], sessions: &[Session], now: u64) {
    if !deck.is_on() {
        deck.jobs.clear();
        crate::core::server_jobs::clear();
        return;
    }
    let cli: HashMap<&str, &str> = sessions
        .iter()
        .map(|session| (session.id.as_str(), session.cli_session_id.as_str()))
        .collect();
    let launches: Vec<Arc<Vec<Launch>>> = chats
        .iter()
        .map(|chat| {
            cli.get(chat.id.as_str())
                .and_then(|id| claude_jobs::launches(id))
                .unwrap_or_default()
        })
        .collect();
    let pc: Vec<PcChat> = chats
        .iter()
        .zip(&launches)
        .map(|(chat, launches)| PcChat {
            id: &chat.id,
            title: &chat.title,
            folder: chat.folder.as_deref(),
            launches,
        })
        .collect();
    let views = deck.views();
    let jobs = link::link(&pc, &views, deck.aliases(), now);
    deck.ask_units(
        link::units_to_ask(&pc, &jobs, deck.aliases())
            .into_iter()
            .collect(),
    );
    deck.history.load_once(&history::path());
    if deck.history.note(&jobs, &views, &pc, &deck.aliases, now) {
        history::save(&history::path(), &deck.history.ended);
    }
    mark(chats, &jobs, now);
    crate::core::server_jobs::publish(publish::entries(&jobs), now);
    deck.jobs = jobs;
}

/// Gives an idle PC chat with a job of its own on a server the Server
/// job mark, its line and its hover panel. A job of a whole repo marks
/// no chat: it shows on the server only.
pub fn mark(chats: &mut [Chat], jobs: &[Job], now: u64) {
    for chat in chats.iter_mut() {
        if !matches!(chat.mark, Mark::Idle(_)) {
            continue;
        }
        let mine: Vec<&Job> = jobs
            .iter()
            .filter(|job| job.chat().is_some_and(|(id, _)| *id == chat.id))
            .collect();
        if mine.is_empty() {
            continue;
        }
        let busy = mine
            .iter()
            .any(|job| job.cpu10.is_some_and(|cpu| cpu > text::BUSY_CPU10));
        chat.mark = Mark::ServerJob(busy);
        chat.since = mine
            .iter()
            .filter_map(|job| job.started)
            .min()
            .or(chat.since);
        chat.text = Some(text::chat_line(&mine, now));
        chat.full = Some(text::chat_full(&mine, now));
        chat.job_on = Some(text::servers_of(&mine).join(", "));
    }
}

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
mod publish_tests;
#[cfg(test)]
mod status_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod view_tests;
