//! Which PC chat a server job belongs to (pure). In this order:
//! 1. A launch: an ssh call of the chat (`claude_launches`) to the same
//!    server that started its unit, or whose program words are the job's,
//!    made before the job started. A unit a timer starts needs its unit
//!    named; program words alone could be a test run by hand.
//! 2. Else the job's project folder: the folder right under `~/projects/`
//!    (a worktree counts as its repo) names the PC chats of that repo. A
//!    unit that a timer or the boot starts is left out here: nobody
//!    started it from a chat. So is a job that runs longer than the
//!    launches are kept.
//! 3. Else it is not shown.
//!
//! A chat's open background ssh call is a job of its own, unless it
//! already gave a job its owner.

use std::collections::{BTreeMap, HashSet};

use crate::core::claude_launches::{program_words, Launch, KEEP_MS};

use super::super::busy::repo_of;
use super::super::{Aliases, HostView};
use super::load::RawJob;
use super::{Job, PcChat};

/// A job may start this much before the call that started it: the two
/// clocks and the rounding of `ps`'s times.
const SLACK_MS: u64 = 2 * 60 * 1000;
/// An open background ssh call older than this is not shown: the app may
/// have closed and never written its end.
pub const OPEN_CALL_MS: u64 = 12 * 60 * 60 * 1000;

fn words_of(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_string).collect()
}

/// The words that name a job: its main process's program words.
pub fn job_words(job: &RawJob) -> Option<String> {
    program_words(&words_of(&job.args))
}

/// The name a job shows: its unit without `.service`, else its program
/// words, else its command's first word.
pub fn job_name(job: &RawJob) -> String {
    if let Some(unit) = &job.unit {
        return unit.strip_suffix(".service").unwrap_or(unit).to_string();
    }
    job_words(job).unwrap_or_else(|| {
        let first = job.args.split_whitespace().next().unwrap_or("job");
        first.rsplit('/').next().unwrap_or(first).to_string()
    })
}

/// Two sets of program words name the same thing: the same first word,
/// and the same second word when both have one.
pub fn same_words(a: &str, b: &str) -> bool {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let mut a = a.split_whitespace();
    let mut b = b.split_whitespace();
    match (a.next(), b.next()) {
        (Some(x), Some(y)) if x == y => match (a.next(), b.next()) {
            (Some(x), Some(y)) => x == y,
            _ => true,
        },
        _ => false,
    }
}

/// The launch started the job: it came first, and it names the job's
/// unit or (for a job no timer starts) its program words.
pub fn started_by(job: &RawJob, started: u64, launch: &Launch) -> bool {
    if launch.at.is_some_and(|at| started + SLACK_MS < at) {
        return false;
    }
    if let Some(unit) = &job.unit {
        let name = unit.strip_suffix(".service").unwrap_or(unit);
        if launch.units.iter().any(|u| u == name) {
            return true;
        }
        if job.timer {
            return false;
        }
    }
    let Some(words) = job_words(job) else {
        return false;
    };
    launch.words.iter().any(|key| same_words(key, &words))
}

/// The repo of a server folder: the part right under `~/projects/`.
pub fn server_repo(cwd: &str, home: Option<&str>) -> Option<String> {
    let rest = match home {
        Some(home) => cwd.strip_prefix(home)?.strip_prefix("/projects/")?,
        None => cwd.split_once("/projects/")?.1,
    };
    let repo = rest.split('/').next().filter(|repo| !repo.is_empty())?;
    Some(repo.to_lowercase())
}

/// A job WinCraft never shows: Claude Code's own remote server.
fn is_claude_tool(job: &RawJob) -> bool {
    job.args
        .split_whitespace()
        .next()
        .is_some_and(|program| program.contains("/.claude/"))
}

/// The launches of every chat that ran ssh to this server.
fn launches_to<'a>(
    server: &str,
    chats: &'a [PcChat<'a>],
    aliases: &Aliases,
) -> Vec<(&'a PcChat<'a>, usize, &'a Launch)> {
    chats
        .iter()
        .flat_map(|chat| {
            chat.launches
                .iter()
                .enumerate()
                .map(move |(at, launch)| (chat, at, launch))
        })
        .filter(|(_, _, launch)| {
            launch
                .hosts
                .iter()
                .any(|host| aliases.server_of(host) == Some(server))
        })
        .collect()
}

/// The jobs of every server that answers, each with the PC chats it
/// belongs to; jobs of no PC chat are left out.
pub fn link(
    chats: &[PcChat],
    views: &[(String, HostView)],
    aliases: &Aliases,
    now: u64,
) -> Vec<Job> {
    let mut jobs = Vec::new();
    for (server, view) in views {
        if !view.online {
            continue;
        }
        let read_at = view.last_seen.unwrap_or(now);
        let calls = launches_to(server, chats, aliases);
        let mut used: HashSet<(&str, usize)> = HashSet::new();
        for raw in view.jobs.iter().filter(|raw| !is_claude_tool(raw)) {
            let started = read_at.saturating_sub(raw.secs * 1000);
            let by = calls
                .iter()
                .filter(|(_, _, launch)| started_by(raw, started, launch))
                .max_by_key(|(_, _, launch)| launch.at);
            let (owners, repo) = match by {
                Some((chat, at, launch)) => {
                    if launch.background {
                        used.insert((chat.id, *at));
                    }
                    (vec![(chat.id.to_string(), chat.title.to_string())], None)
                }
                None if raw.timer || raw.enabled => continue,
                None if raw.secs * 1000 > KEEP_MS => continue,
                None => {
                    let Some(repo) = server_repo(&raw.cwd, view.home.as_deref()) else {
                        continue;
                    };
                    let owners: Vec<(String, String)> = chats
                        .iter()
                        .filter(|chat| chat.folder.and_then(repo_of).as_deref() == Some(&*repo))
                        .map(|chat| (chat.id.to_string(), chat.title.to_string()))
                        .collect();
                    if owners.is_empty() {
                        continue;
                    }
                    (owners, Some(repo))
                }
            };
            jobs.push(Job {
                server: server.clone(),
                chats: owners,
                repo,
                name: job_name(raw),
                command: raw.args.clone(),
                folder: Some(raw.cwd.clone()),
                pid: Some(raw.pid),
                unit: raw.unit.clone(),
                started: Some(started),
                cpu10: Some(raw.cpu10),
                rss_kb: Some(raw.rss_kb),
            });
        }
        for (chat, at, launch) in &calls {
            let fresh = launch
                .at
                .is_some_and(|at| now.saturating_sub(at) < OPEN_CALL_MS);
            if !launch.is_open() || !fresh || used.contains(&(chat.id, *at)) {
                continue;
            }
            jobs.push(Job {
                server: server.clone(),
                chats: vec![(chat.id.to_string(), chat.title.to_string())],
                repo: None,
                name: call_name(launch),
                command: launch.command.clone(),
                folder: None,
                pid: None,
                unit: None,
                started: launch.at,
                cpu10: None,
                rss_kb: None,
            });
        }
    }
    jobs
}

/// The name of a background ssh call: what it runs on the server.
pub fn call_name(launch: &Launch) -> String {
    launch
        .remote
        .clone()
        .or_else(|| launch.words.first().cloned())
        .unwrap_or_else(|| "ssh command".to_string())
}

/// The user units to ask systemd about, by server: those the chats'
/// launches started and those of the jobs shown, with `.service`.
pub fn units_to_ask(
    chats: &[PcChat],
    jobs: &[Job],
    aliases: &Aliases,
) -> BTreeMap<String, Vec<String>> {
    let mut asks: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut add = |server: &str, unit: String| {
        let list = asks.entry(server.to_string()).or_default();
        if !list.contains(&unit) {
            list.push(unit);
        }
    };
    for launch in chats.iter().flat_map(|chat| chat.launches.iter()) {
        for server in launch
            .hosts
            .iter()
            .filter_map(|host| aliases.server_of(host))
        {
            for unit in &launch.units {
                add(server, format!("{unit}.service"));
            }
        }
    }
    for job in jobs {
        if let Some(unit) = &job.unit {
            add(&job.server, unit.clone());
        }
    }
    asks
}
