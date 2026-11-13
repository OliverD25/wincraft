//! The words about server jobs (pure): a PC chat's line and hover panel,
//! the server line on the Chats tab, and the rows of "Work from the PC".

use crate::plugins::claude_deck::servers::group_name;
use crate::plugins::claude_deck::servers::tab::view::{cpu_text, ram_text};

use super::history::{Ended, How};
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

/// The section's title on a server's card.
pub const SECTION: &str = "Work from the PC";
pub const SHOW_CHAT: &str = "Show the chat";

/// The running jobs of one server.
pub fn on_server<'a>(server: &str, jobs: &'a [Job]) -> Vec<&'a Job> {
    jobs.iter().filter(|job| job.server == server).collect()
}

/// "2 jobs from 1 PC chat · CPU 5%".
pub fn server_words(jobs: &[&Job]) -> String {
    let mut owners: Vec<String> = Vec::new();
    for job in jobs {
        let owner = job.owner();
        if !owners.contains(&owner) {
            owners.push(owner);
        }
    }
    let count = match jobs.len() {
        1 => "1 job".to_string(),
        n => format!("{n} jobs"),
    };
    let from = match owners.len() {
        1 => "1 PC chat".to_string(),
        n => format!("{n} PC chats"),
    };
    let mut words = format!("{count} from {from}");
    let cpu: Vec<u32> = jobs.iter().filter_map(|job| job.cpu10).collect();
    if !cpu.is_empty() {
        let total: u32 = cpu.iter().sum();
        words.push_str(&format!(" \u{b7} CPU {}%", (total + 5) / 10));
    }
    words
}

/// A server line's words with its jobs: the jobs' words in place of
/// "No chat working" or "No Claude chats", else after its counts.
pub fn with_jobs(line: String, online: bool, jobs: &[&Job]) -> String {
    use crate::plugins::claude_deck::servers::view::{NONE_WORKING, NO_CHATS};
    if !online || jobs.is_empty() {
        return line;
    }
    let words = server_words(jobs);
    if line == NONE_WORKING || line == NO_CHATS {
        words
    } else {
        format!("{line} \u{b7} {words}")
    }
}

/// The server hover panel's lines about jobs: "Work from the PC:" and one
/// line per job, "WinCraft design · archive write · 1 h 48 m".
pub fn server_hover(jobs: &[&Job], now: u64) -> String {
    if jobs.is_empty() {
        return String::new();
    }
    let mut lines = vec![format!("{SECTION}:")];
    for job in jobs {
        let mut parts = vec![job.owner(), job.name.clone()];
        parts.extend(Some(ran(job, now)).filter(|time| !time.is_empty()));
        lines.push(parts.join(" \u{b7} "));
    }
    lines.join("\n")
}

/// The right words of a running job's row: "1 h 48 m · 2.4% · 2.6 GB".
pub fn job_right(job: &Job, now: u64) -> String {
    let mut parts = Vec::new();
    parts.extend(Some(ran(job, now)).filter(|time| !time.is_empty()));
    parts.extend(job.cpu10.map(cpu_text));
    parts.extend(job.rss_kb.map(ram_text));
    parts.join(" \u{b7} ")
}

/// The right words of an ended job's row: "ran 2 h 10 m · ended well",
/// "ran 5 min · failed (exit 1)".
pub fn ended_right(end: &Ended) -> String {
    let how = match (end.how, end.code) {
        (How::Well, _) => "ended well".to_string(),
        (How::Failed, Some(code)) => format!("failed (exit {code})"),
        (How::Failed, None) => "failed".to_string(),
        (How::Stopped, _) => "stopped".to_string(),
        (How::Ended, _) => "ended".to_string(),
    };
    match end.started {
        Some(start) if start <= end.ended => {
            format!("ran {} \u{b7} {how}", run_time(end.ended - start))
        }
        _ => how,
    }
}

/// The hover of a job's row: its command, its folder and its PID.
pub fn row_hover(command: &str, folder: Option<&str>, pid: Option<u32>) -> String {
    let mut lines = vec![short_command(command)];
    lines.extend(folder.map(str::to_string));
    lines.extend(pid.map(|pid| format!("PID {pid}")));
    lines.join("\n")
}
