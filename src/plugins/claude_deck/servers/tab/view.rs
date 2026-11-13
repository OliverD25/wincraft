//! What the Servers tab shows, worked out without drawing: the lines of
//! each server's card, the words on them, the owner of each `claude`
//! process, and the problems the tab's red count counts.

use crate::core::claude_sessions::duration_text;
use crate::plugins::claude_deck::model::{Chat, Mark};

use super::super::load::{Kind, Proc, Service, State};
use super::super::view::{self as group_view, ServerLine, Shape};
use super::super::{group_name, HostView};
use super::Pick;

/// The tab's name.
pub const TAB: &str = "Servers";
pub const CHATS: &str = "Chats";
pub const CLAUDE: &str = "Claude processes";
pub const BUSIEST: &str = "Busiest";
pub const SERVICES: &str = "Services";
pub const RESTART: &str = "Restart needed";
/// The left words of a `claude` process with no chat.
pub const ORPHAN: &str = "No chat (orphan)";
pub const COPY_COMMAND: &str = "Copy the command line";
pub const COPY_PID: &str = "Copy the PID";

/// One line of the tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    /// A server's card header, by its place in the views.
    Card {
        at: usize,
        folded: bool,
    },
    /// A section's title inside a card.
    Section(&'static str),
    /// A chat row, or the quiet or hidden chats' line, as on the Chats tab
    /// before.
    Chat(ServerLine),
    Proc {
        server: String,
        proc: Proc,
        /// For a `claude` process: its chat's title, or None for an orphan.
        owner: Option<Option<String>>,
    },
    Service {
        server: String,
        service: Service,
    },
}

impl Line {
    /// What the arrow keys stop on; None for a section title.
    pub fn pick(&self, views: &[(String, HostView)]) -> Option<Pick> {
        match self {
            Line::Card { at, .. } => Some(Pick::Card(views.get(*at)?.0.clone())),
            Line::Section(_) => None,
            Line::Chat(ServerLine::Chat { chat, .. }) => Some(Pick::Chat(chat.id.clone())),
            Line::Chat(ServerLine::Quiet { server, .. }) => Some(Pick::Quiet(server.clone())),
            Line::Chat(ServerLine::Hidden { server, .. }) => Some(Pick::Hidden(server.clone())),
            Line::Chat(ServerLine::Server(_)) => None,
            Line::Proc {
                server,
                proc,
                owner,
            } => Some(Pick::Proc {
                server: server.clone(),
                pid: proc.pid,
                claude: owner.is_some(),
            }),
            Line::Service { server, service } => Some(Pick::Service {
                server: server.clone(),
                name: format!("{:?}/{}", service.kind, service.name),
            }),
        }
    }
}

/// The chat a `claude` process belongs to: a chat on the same server in
/// the same folder, a live one first. None when no chat is there.
pub fn owner<'a>(proc: &Proc, chats: &'a [Chat]) -> Option<&'a Chat> {
    let folder = proc.folder.as_deref()?;
    let here = |chat: &&Chat| chat.folder.as_deref() == Some(folder);
    chats
        .iter()
        .filter(here)
        .find(|chat| chat.mark != Mark::Stopped)
        .or_else(|| chats.iter().find(here))
}

/// Every line of the tab, top to bottom. A folded card shows only its
/// header; a section with nothing to show is left out.
pub fn lines(
    views: &[(String, HostView)],
    folded: &dyn Fn(&str) -> bool,
    shape: &Shape,
) -> Vec<Line> {
    let mut lines = Vec::new();
    for (at, (server, view)) in views.iter().enumerate() {
        let is_folded = folded(server);
        lines.push(Line::Card {
            at,
            folded: is_folded,
        });
        if is_folded {
            continue;
        }
        let one = [(server.clone(), view.clone())];
        let chats: Vec<ServerLine> = group_view::lines(&one, shape)
            .into_iter()
            .filter(|line| !matches!(line, ServerLine::Server(_)))
            .collect();
        if !chats.is_empty() {
            lines.push(Line::Section(CHATS));
            lines.extend(chats.into_iter().map(Line::Chat));
        }
        if !view.claude.is_empty() {
            lines.push(Line::Section(CLAUDE));
            lines.extend(view.claude.iter().map(|proc| Line::Proc {
                server: server.clone(),
                proc: proc.clone(),
                owner: Some(owner(proc, &view.chats).map(|chat| chat.title.clone())),
            }));
        }
        if !view.busiest.is_empty() {
            lines.push(Line::Section(BUSIEST));
            lines.extend(view.busiest.iter().map(|proc| Line::Proc {
                server: server.clone(),
                proc: proc.clone(),
                owner: None,
            }));
        }
        // A stopped systemd unit is left out: most are timer jobs that ran
        // and finished (26 of them on home1, checked 2026-11-13).
        let mut services: Vec<Service> = view
            .services
            .iter()
            .filter(|s| s.state != State::Stopped || s.kind == Kind::Docker)
            .cloned()
            .collect();
        if !services.is_empty() {
            lines.push(Line::Section(SERVICES));
            // Problems first, then system, user and Docker, by name.
            services.sort_by(|a, b| {
                (!a.state.is_problem(), a.kind, &a.name).cmp(&(
                    !b.state.is_problem(),
                    b.kind,
                    &b.name,
                ))
            });
            lines.extend(services.into_iter().map(|service| Line::Service {
                server: server.clone(),
                service,
            }));
        }
    }
    lines
}

fn ago(at: u64, now: u64) -> String {
    format!("{} ago", duration_text(now.saturating_sub(at)))
}

fn gb(kb: u64) -> u64 {
    (kb as f64 / 1024.0 / 1024.0).round() as u64
}

/// The words at the right of a card's header, without "Restart needed":
/// "CPU 34% · RAM 61% · Disk 120 GB free · Up 12 days", or
/// "Offline · last seen 3 h ago".
pub fn card_text(view: &HostView, now: u64) -> String {
    match (view.checked, view.online) {
        (None, _) => "Checking\u{2026}".to_string(),
        (Some(_), false) => match view.last_seen {
            Some(seen) => format!("Offline \u{b7} last seen {}", ago(seen, now)),
            None => "Offline".to_string(),
        },
        (Some(_), true) => {
            let mut parts = Vec::new();
            parts.extend(view.cpu.map(|cpu| format!("CPU {cpu}%")));
            parts.extend(view.ram.map(|ram| format!("RAM {ram}%")));
            parts.extend(
                view.health
                    .disk
                    .map(|(free, _)| format!("Disk {} GB free", gb(free))),
            );
            parts.extend(
                view.health
                    .up
                    .map(|up| format!("Up {}", duration_text(up * 1000))),
            );
            parts.join(" \u{b7} ")
        }
    }
}

/// Whether the card shows "Restart needed".
pub fn needs_restart(view: &HostView) -> bool {
    view.online && view.health.reboot
}

/// "12.5%".
pub fn cpu_text(cpu10: u32) -> String {
    if cpu10.is_multiple_of(10) {
        format!("{}%", cpu10 / 10)
    } else {
        format!("{}.{}%", cpu10 / 10, cpu10 % 10)
    }
}

/// "200 MB", "1.6 GB", "900 kB".
pub fn ram_text(kb: u64) -> String {
    match kb {
        kb if kb < 1024 => format!("{kb} kB"),
        kb if kb < 1024 * 1024 => format!("{} MB", (kb as f64 / 1024.0).round() as u64),
        kb => format!("{:.1} GB", kb as f64 / 1024.0 / 1024.0),
    }
}

/// The words at the right of a process row: "12.5% · 200 MB · 1 h".
pub fn proc_right(proc: &Proc) -> String {
    let mut parts = vec![cpu_text(proc.cpu10), ram_text(proc.rss_kb)];
    parts.extend(proc.secs.map(|secs| duration_text(secs * 1000)));
    parts.join(" \u{b7} ")
}

/// The small words after a process's name: the folder's last part for a
/// `claude` process, else the PID.
pub fn proc_note(proc: &Proc) -> String {
    match proc.folder.as_deref() {
        Some(folder) => {
            let last = folder.trim_end_matches('/').rsplit('/').next();
            last.filter(|last| !last.is_empty())
                .unwrap_or(folder)
                .to_string()
        }
        None => format!("PID {}", proc.pid),
    }
}

/// The small words after a service's name.
pub fn kind_words(kind: Kind) -> &'static str {
    match kind {
        Kind::System => "system",
        Kind::User => "user",
        Kind::Docker => "Docker",
    }
}

/// The words at the right of a service row: "failed", "running · Up 3 days".
pub fn service_right(service: &Service) -> String {
    let words = service.state.words();
    let detail = service.detail.trim();
    if service.kind == Kind::Docker && !detail.is_empty() {
        format!("{words} \u{b7} {detail}")
    } else {
        words.to_string()
    }
}

/// The hover text of a process row: its whole command line and PID.
pub fn proc_hover(proc: &Proc) -> String {
    let mut lines = vec![format!("PID {}", proc.pid)];
    if let Some(folder) = &proc.folder {
        lines.push(folder.clone());
    }
    lines.push(proc.command.clone());
    lines.join("\n")
}

/// What is wrong on the servers, one line each: offline servers, failed or
/// restarting services and containers, and orphan `claude` processes. An
/// offline server counts once; its old data is not counted again.
pub fn problems(views: &[(String, HostView)]) -> Vec<String> {
    let mut problems = Vec::new();
    for (server, view) in views {
        let name = group_name(server);
        if view.checked.is_some() && !view.online {
            problems.push(format!("{name} is offline"));
            continue;
        }
        for service in view.services.iter().filter(|s| s.state.is_problem()) {
            problems.push(format!(
                "{name}: {} {}",
                service.name,
                service.state.words()
            ));
        }
        for proc in &view.claude {
            if owner(proc, &view.chats).is_none() {
                problems.push(format!(
                    "{name}: a claude process with no chat (PID {})",
                    proc.pid
                ));
            }
        }
    }
    problems
}

/// The tab's tooltip: the problems, or that all is well.
pub fn problems_text(problems: &[String]) -> String {
    if problems.is_empty() {
        "No problems on the servers.".to_string()
    } else {
        problems.join("\n")
    }
}
