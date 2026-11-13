//! The jobs of the last 24 h that ended, for "Work from the PC": a job
//! WinCraft saw go away while its window was open, a unit a PC chat
//! started that systemd says is over, and a chat's background ssh call
//! whose task notice came. Kept in a small file next to ClaudeDeck's
//! other files, at most `MOST` entries; older than a day is dropped.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::core::claude_launches::Outcome;
use crate::core::config;

use super::super::{Aliases, HostView};
use super::link::call_name;
use super::{Job, PcChat};

/// Entries are kept this long.
pub const KEEP_MS: u64 = 24 * 60 * 60 * 1000;
/// At most this many entries, newest kept.
pub const MOST: usize = 200;
/// Two ends of the same job this close are one end.
const SAME_END_MS: u64 = 5 * 60 * 1000;

/// How a job ended, as far as is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum How {
    Well,
    Failed,
    Stopped,
    /// It went away; how is not known.
    Ended,
}

/// A job that ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ended {
    pub server: String,
    /// The running job's key (`Job::key`).
    pub key: String,
    /// The chat's name, or "verdict-ai chats".
    pub owner: String,
    /// The chat's id, when it belongs to one chat.
    #[serde(default)]
    pub chat: Option<String>,
    pub name: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub started: Option<u64>,
    /// When it ended, in Unix ms.
    pub ended: u64,
    pub how: How,
    /// The exit code of a failed job, when known.
    #[serde(default)]
    pub code: Option<i32>,
}

/// The file's form.
#[derive(Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    ended: Vec<Ended>,
}

/// The history, and the jobs that ran at the last look.
#[derive(Default)]
pub struct History {
    pub ended: Vec<Ended>,
    running: Vec<Job>,
    loaded: bool,
}

pub fn path() -> PathBuf {
    config::plugins_dir().join("claude_deck.server_jobs.json")
}

pub fn load(path: &Path) -> Vec<Ended> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<File>(config::strip_bom(&text)).ok())
        .map(|file| file.ended)
        .unwrap_or_default()
}

pub fn save(path: &Path, ended: &[Ended]) {
    let file = File {
        ended: ended.to_vec(),
    };
    let Ok(text) = serde_json::to_string(&file) else {
        return;
    };
    if let Err(err) = config::write_atomic(path, &text) {
        log::warn!("ClaudeDeck: could not save the server jobs that ended: {err}");
    }
}

fn from_job(job: &Job, ended: u64, how: How, code: Option<i32>) -> Ended {
    Ended {
        server: job.server.clone(),
        key: job.key(),
        owner: job.owner(),
        chat: job.chat().map(|(id, _)| id.clone()),
        name: job.name.clone(),
        command: job.command.clone(),
        folder: job.folder.clone(),
        pid: job.pid,
        started: job.started,
        ended,
        how,
        code,
    }
}

/// systemd's word on a unit that is over, if it has one.
fn unit_end(view: &HostView, unit: &str) -> Option<(u64, How, Option<i32>)> {
    let info = view
        .units
        .iter()
        .find(|info| info.name == unit && info.is_over())?;
    let how = if info.ended_well() {
        How::Well
    } else {
        How::Failed
    };
    let code = (how == How::Failed).then_some(info.status).flatten();
    Some((info.ended?, how, code))
}

impl History {
    /// Reads the file once, before the first look.
    pub fn load_once(&mut self, path: &Path) {
        if !std::mem::replace(&mut self.loaded, true) {
            self.ended = load(path);
        }
    }

    /// Adds an end unless the same job's end is there; a known outcome
    /// replaces "ended". Returns whether anything changed.
    fn add(&mut self, end: Ended, now: u64) -> bool {
        if now.saturating_sub(end.ended) > KEEP_MS {
            return false;
        }
        let same = self
            .ended
            .iter_mut()
            .find(|old| old.key == end.key && old.ended.abs_diff(end.ended) < SAME_END_MS);
        match same {
            Some(old) if old.how == How::Ended && end.how != How::Ended => {
                *old = end;
                true
            }
            Some(_) => false,
            None => {
                self.ended.push(end);
                true
            }
        }
    }

    /// Looks at the jobs that run now and what the servers and the chats
    /// say: the jobs that ended since the last look are added. Returns
    /// whether the history changed.
    pub fn note(
        &mut self,
        jobs: &[Job],
        views: &[(String, HostView)],
        chats: &[PcChat],
        aliases: &Aliases,
        now: u64,
    ) -> bool {
        let online = |server: &str| {
            views
                .iter()
                .find(|(name, _)| name == server)
                .filter(|(_, view)| view.online)
                .map(|(_, view)| view)
        };
        let mut changed = false;
        let before = std::mem::replace(&mut self.running, jobs.to_vec());
        for gone in before
            .iter()
            .filter(|old| old.pid.is_some() && !jobs.iter().any(|job| job.key() == old.key()))
        {
            // A server that stopped answering says nothing about its jobs.
            let Some(view) = online(&gone.server) else {
                self.running.push(gone.clone());
                continue;
            };
            let info = gone.unit.as_deref().and_then(|unit| unit_end(view, unit));
            let (at, how, code) = info.unwrap_or((now, How::Ended, None));
            let mut end = from_job(gone, at, how, code);
            if let Some(started) = gone
                .unit
                .as_deref()
                .and_then(|unit| view.units.iter().find(|u| u.name == unit))
                .and_then(|u| u.started)
            {
                end.started = Some(started);
            }
            changed |= self.add(end, now);
        }
        for (server, view) in views.iter().filter(|(_, view)| view.online) {
            for info in view.units.iter().filter(|info| info.is_over()) {
                let unit = info.name.strip_suffix(".service").unwrap_or(&info.name);
                let Some((at, how, code)) = unit_end(view, &info.name) else {
                    continue;
                };
                let starter = chats
                    .iter()
                    .flat_map(|chat| chat.launches.iter().map(move |l| (chat, l)))
                    .filter(|(_, l)| l.units.iter().any(|u| u == unit))
                    .filter(|(_, l)| l.hosts.iter().any(|h| aliases.server_of(h) == Some(server)))
                    .filter(|(_, l)| l.at.is_some_and(|start| start <= at))
                    .max_by_key(|(_, l)| l.at);
                let Some((chat, _)) = starter else {
                    continue;
                };
                let job = Job {
                    server: server.clone(),
                    chats: vec![(chat.id.to_string(), chat.title.to_string())],
                    name: unit.to_string(),
                    unit: Some(info.name.clone()),
                    started: info.started,
                    ..Job::default()
                };
                changed |= self.add(from_job(&job, at, how, code), now);
            }
        }
        for chat in chats {
            for launch in chat.launches.iter().filter(|l| l.background) {
                let Some((Some(at), outcome)) = launch.ended else {
                    continue;
                };
                let Some(server) = launch.hosts.iter().find_map(|h| aliases.server_of(h)) else {
                    continue;
                };
                let (how, code) = match outcome {
                    Outcome::Done => (How::Well, None),
                    Outcome::Failed(code) => (How::Failed, code),
                    Outcome::Stopped => (How::Stopped, None),
                };
                let job = Job {
                    server: server.to_string(),
                    chats: vec![(chat.id.to_string(), chat.title.to_string())],
                    name: call_name(launch),
                    command: launch.command.clone(),
                    started: launch.at,
                    ..Job::default()
                };
                changed |= self.add(from_job(&job, at, how, code), now);
            }
        }
        changed |= self.prune(now);
        changed
    }

    /// Drops ends older than a day, and the oldest over `MOST`.
    pub fn prune(&mut self, now: u64) -> bool {
        let count = self.ended.len();
        self.ended
            .retain(|end| now.saturating_sub(end.ended) <= KEEP_MS && end.ended <= now + KEEP_MS);
        self.ended.sort_by_key(|end| std::cmp::Reverse(end.ended));
        self.ended.truncate(MOST);
        self.ended.len() != count
    }

    /// The ends of one server, newest first, without jobs that run again.
    pub fn of_server<'a>(&'a self, server: &str, running: &[Job]) -> Vec<&'a Ended> {
        self.ended
            .iter()
            .filter(|end| end.server == server)
            .filter(|end| !running.iter().any(|job| job.key() == end.key))
            .collect()
    }
}
