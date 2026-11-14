//! The snapshot ClaudeDeck publishes for WindowDeck's Claude board
//! (`core::server_jobs`): per PC chat, the servers its own jobs run on,
//! how many, the oldest start and whether one is busy. A job of a whole
//! repo belongs to no single chat and is left out, as `mark` leaves it.

use crate::core::server_jobs::Entry;

use super::super::group_name;
use super::{text, Job};

pub fn entries(jobs: &[Job]) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for job in jobs {
        let Some((chat, _)) = job.chat() else {
            continue;
        };
        let at = match out.iter().position(|entry| &entry.chat_id == chat) {
            Some(at) => at,
            None => {
                out.push(Entry {
                    chat_id: chat.clone(),
                    ..Entry::default()
                });
                out.len() - 1
            }
        };
        let entry = &mut out[at];
        let server = group_name(&job.server);
        if !entry.servers.contains(&server) {
            entry.servers.push(server);
        }
        entry.jobs += 1;
        entry.oldest = match (entry.oldest, job.started) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        entry.busy |= job.cpu10.is_some_and(|cpu| cpu > text::BUSY_CPU10);
    }
    out
}
