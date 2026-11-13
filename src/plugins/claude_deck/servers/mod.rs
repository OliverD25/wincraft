//! ClaudeDeck's homelab servers: one group per server under the PC chats,
//! with its Claude chats, and "Waits on <server>" for PC chats blocked on
//! a foreground ssh command. Off while the setting "Servers (ssh names)"
//! is empty. Servers are read only while the window is open, by one
//! read-only ssh call every 30 s (`poll`); nothing is installed or
//! changed on them.

pub mod aliases;
pub mod busy;
pub mod draw;
pub mod load;
pub mod poll;
pub mod probe;
pub mod reply;
pub mod status;
pub mod tab;
mod terminal;
pub mod view;

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::core::claude_turns::Turn;
use crate::plugins::claude_deck::window::Pick;

pub use aliases::{group_name, parse_servers, Aliases};
pub use poll::{HostView, Poller, Runner};

/// A PC chat that waits on a server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wait {
    /// The chat's id in the list.
    pub chat: String,
    /// The listed server it runs ssh to.
    pub server: String,
    pub since: Option<u64>,
    /// The chat's title, for the server line's hover.
    pub title: String,
}

/// The listed server a chat's open foreground ssh command runs to.
pub fn waits_on<'a>(turn: Option<&Turn>, aliases: &'a Aliases) -> Option<&'a str> {
    let ssh = turn?.ssh.as_ref()?;
    ssh.hosts.iter().find_map(|host| aliases.server_of(host))
}

/// The row's line for a waiting chat: "Waits on home2 · docker compose up".
pub fn wait_line(server: &str, command: &str) -> String {
    let remote = remote_part(command);
    if remote.is_empty() {
        format!("Waits on {}", group_name(server))
    } else {
        format!("Waits on {} \u{b7} {remote}", group_name(server))
    }
}

/// What a command runs on the server, roughly: the words after its host,
/// without quotes and without a leading `cd <folder> &&`.
fn remote_part(command: &str) -> String {
    let words: Vec<&str> = command.split_whitespace().collect();
    let Some(ssh) = words.iter().position(|w| {
        let name = w
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(w)
            .to_ascii_lowercase();
        name == "ssh" || name == "ssh.exe"
    }) else {
        return String::new();
    };
    let mut at = ssh + 1;
    while let Some(word) = words.get(at) {
        let Some(option) = word.strip_prefix('-') else {
            break;
        };
        at += if option.len() == 1 && "BbcDEeFIiJLlmOoPpQRSWw".contains(option) {
            2
        } else {
            1
        };
    }
    let rest = words.get(at + 1..).unwrap_or_default().join(" ");
    let rest = rest.replace(['\'', '"'], "");
    let rest = rest.trim();
    let rest = match rest.strip_prefix("cd ").and_then(|r| r.split_once("&&")) {
        Some((_, after)) => after.trim(),
        None => rest,
    };
    rest.to_string()
}

/// The hover panel's body for a waiting chat.
pub fn wait_full(command: &str) -> String {
    format!(
        "The chat is blocked until this command ends. It can do nothing else meanwhile.\n{command}"
    )
}

/// What Enter and Ctrl+C act on for a server line or a server chat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub server: String,
    /// The chat's folder; None for a server line (the home folder).
    pub folder: Option<String>,
    /// What Ctrl+C copies: the chat's name, or the server's.
    pub title: String,
}

/// The pick of a server line, apart from the user's group ids.
pub fn server_pick(server: &str) -> Pick {
    Pick::Group(format!("server:{server}"))
}

/// Opens a terminal on a server, in a folder or in the home folder.
pub fn open_terminal(server: &str, folder: Option<&str>) {
    terminal::open(server, folder);
}

/// The servers part of the window's state: the poller while it runs, the
/// names ssh knows the servers by, and the parts the user unfolded.
#[derive(Default)]
pub struct Deck {
    servers: Vec<String>,
    aliases: Aliases,
    poller: Option<Poller>,
    /// A fake for tests; None is the real ssh.
    runner: Option<Arc<dyn Runner>>,
    /// The servers whose quiet chats are unfolded.
    pub quiet_open: BTreeSet<String>,
    /// The servers whose hidden chats are listed.
    pub hidden_open: BTreeSet<String>,
    /// The PC chats that wait on a server now.
    pub waits: Vec<Wait>,
    /// The server lines and rows the arrow keys reach, top to bottom, as
    /// drawn last.
    pub keyed: Vec<(Pick, Target)>,
    /// The Servers tab's own state.
    pub tab: tab::TabUi,
}

impl Deck {
    #[cfg(test)]
    pub fn with_runner(runner: Arc<dyn Runner>) -> Self {
        Deck {
            runner: Some(runner),
            ..Deck::default()
        }
    }

    /// Called each frame while the window is open: starts reading when
    /// servers are listed, and starts again when the list changed. With
    /// no server listed nothing runs.
    pub fn tick(&mut self, servers: &[String], wake: Arc<dyn Fn() + Send + Sync>) {
        if self.servers != servers || (self.poller.is_none() && !servers.is_empty()) {
            self.poller = None;
            self.servers = servers.to_vec();
            self.aliases = if servers.is_empty() {
                Aliases::default()
            } else {
                Aliases::load(servers)
            };
            if !servers.is_empty() {
                let runner = self
                    .runner
                    .clone()
                    .unwrap_or_else(|| Arc::new(poll::Ssh) as Arc<dyn Runner>);
                self.poller = Some(Poller::start(servers, runner, wake));
            }
        }
    }

    /// What a pick of a server line or row acts on.
    pub fn target(&self, pick: &Pick) -> Option<&Target> {
        self.keyed
            .iter()
            .find(|(keyed, _)| keyed == pick)
            .map(|(_, target)| target)
    }

    pub fn aliases(&self) -> &Aliases {
        &self.aliases
    }

    /// Whether any server is listed, so its groups show.
    pub fn is_on(&self) -> bool {
        !self.servers.is_empty()
    }

    #[cfg(test)]
    pub fn is_reading(&self) -> bool {
        self.poller.is_some()
    }

    /// Each listed server with what is known of it, in the setting's order.
    pub fn views(&self) -> Vec<(String, HostView)> {
        let known = self
            .poller
            .as_ref()
            .and_then(|poller| poller.views.lock().ok().map(|views| views.clone()))
            .unwrap_or_default();
        self.servers
            .iter()
            .map(|server| {
                (
                    server.clone(),
                    known.get(server).cloned().unwrap_or_default(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod load_tests;
#[cfg(test)]
mod tests;
