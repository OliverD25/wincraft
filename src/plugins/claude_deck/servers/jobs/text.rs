//! The words about server jobs (pure): a PC chat's line and hover panel,
//! the server line on the Chats tab, and the rows of "Work from the PC".

use crate::plugins::claude_deck::servers::group_name;
use crate::plugins::claude_deck::servers::tab::view::{cpu_text, ram_text};

use super::Job;

/// The longest command shown in a hover panel, in characters.
pub const COMMAND_SHOWN: usize = 120;
/// A job over this CPU (in tenths of a percent) makes its mark pulse.
pub const BUSY_CPU10: u32 = 50;

/// "45 s", "12 min", "1 h 48 m", "2 d 3 h".
pub fn run_time(ms: u64) -> String {
    let secs = ms / 1000;
    match secs {
        s if s < 60 => format!("{s} s"),
        s if s < 3600 => format!("{} min", s / 60),
        s if s < 86_400 => match (s / 3600, s % 3600 / 60) {
            (h, 0) => format!("{h} h"),
            (h, m) => format!("{h} h {m} m"),
        },
        s => match (s / 86_400, s % 86_400 / 3600) {
            (d, 0) => format!("{d} d"),
            (d, h) => format!("{d} d {h} h"),
        },
    }
}

/// How long a job has run, as words; empty when its start is unknown.
pub fn ran(job: &Job, now: u64) -> String {
    job.started
        .map(|at| run_time(now.saturating_sub(at)))
        .unwrap_or_default()
}

/// The command cut to `COMMAND_SHOWN` characters.
pub fn short_command(command: &str) -> String {
    let mut short: String = command.chars().take(COMMAND_SHOWN).collect();
    if command.chars().count() > COMMAND_SHOWN {
        short.push('\u{2026}');
    }
    short
}

/// The servers of some jobs, each once, by their short names.
pub fn servers_of(jobs: &[&Job]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for job in jobs {
        let name = group_name(&job.server);
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// A PC chat's line: "Runs on home2: archive write · 1 h 48 m", or "Runs
/// 2 jobs on home2, office1".
pub fn chat_line(jobs: &[&Job], now: u64) -> String {
    match jobs {
        [job] => {
            let time = ran(job, now);
            let head = format!("Runs on {}: {}", group_name(&job.server), job.name);
            if time.is_empty() {
                head
            } else {
                format!("{head} \u{b7} {time}")
            }
        }
        _ => format!(
            "Runs {} jobs on {}",
            jobs.len(),
            servers_of(jobs).join(", ")
        ),
    }
}

/// CPU and RAM in words: "CPU 2.4% · RAM 2.6 GB"; empty for an ssh call.
pub fn load_words(job: &Job) -> String {
    let mut parts = Vec::new();
    parts.extend(job.cpu10.map(|cpu| format!("CPU {}", cpu_text(cpu))));
    parts.extend(job.rss_kb.map(|kb| format!("RAM {}", ram_text(kb))));
    parts.join(" \u{b7} ")
}

/// One job in a hover panel: "home2 · archive write · 1 h 48 m · CPU
/// 2.4% · RAM 2.6 GB", then its command.
pub fn job_lines(job: &Job, now: u64) -> String {
    let mut head = vec![group_name(&job.server), job.name.clone()];
    head.extend(Some(ran(job, now)).filter(|time| !time.is_empty()));
    head.extend(Some(load_words(job)).filter(|load| !load.is_empty()));
    format!("{}\n{}", head.join(" \u{b7} "), short_command(&job.command))
}

/// A PC chat's hover panel body: each of its jobs.
pub fn chat_full(jobs: &[&Job], now: u64) -> String {
    jobs.iter()
        .map(|job| job_lines(job, now))
        .collect::<Vec<_>>()
        .join("\n\n")
}
