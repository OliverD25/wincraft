//! The thread that reads the servers while ClaudeDeck's window is open:
//! one thread per server, one ssh call every 30 s, the first right away.
//! It stops when the window closes (the `Poller` is dropped). A server is
//! never read by two calls at once, even when the window was closed and
//! opened again while a slow call still ran. The window only reads the
//! results; it never waits on ssh.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::core::claude_sessions;
use crate::core::claude_turns::Reply;
use crate::plugins::claude_deck::model::Chat;

use super::probe;
use super::reply::{self, RawChat};
use super::status::{self, Parsed, Running};

/// How often each server is read.
pub const EVERY: Duration = Duration::from_secs(30);
/// How often a waiting thread looks at the stop flag.
const STEP: Duration = Duration::from_millis(200);

/// Runs the probe on a server and returns what it printed. A trait, so
/// the tests use a fake and never reach a real server.
pub trait Runner: Send + Sync {
    fn run(&self, host: &str, script: &str) -> Result<String, String>;
}

/// The real one: Windows' ssh.
pub struct Ssh;

impl Runner for Ssh {
    fn run(&self, host: &str, script: &str) -> Result<String, String> {
        probe::run(host, script)
    }
}

/// What the window shows of one server.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostView {
    /// When the last call ended, in the PC's Unix ms; None before.
    pub checked: Option<u64>,
    pub online: bool,
    /// When the server last answered.
    pub last_seen: Option<u64>,
    pub cpu: Option<u8>,
    pub ram: Option<u8>,
    /// MemTotal and MemAvailable, in kB.
    pub mem: Option<(u64, u64)>,
    pub health: super::reply::Health,
    /// Its chats, as last known; kept while it is offline.
    pub chats: Vec<Chat>,
    /// The finished replies of each chat, by the chat's id, oldest first,
    /// with their times in the PC's clock.
    pub replies: HashMap<String, Vec<Reply>>,
}

/// The transcripts already read from one server, by folder.
pub type Cache = HashMap<String, (RawChat, Parsed)>;

pub type Views = Arc<Mutex<HashMap<String, HostView>>>;

/// The servers being read now, across every poller.
static BUSY: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn take_busy(host: &str) -> bool {
    let Ok(mut busy) = BUSY.lock() else {
        return false;
    };
    if busy.iter().any(|h| h == host) {
        return false;
    }
    busy.push(host.to_string());
    true
}

fn end_busy(host: &str) {
    if let Ok(mut busy) = BUSY.lock() {
        busy.retain(|h| h != host);
    }
}

/// The script for a server, told which transcripts it need not send.
pub fn script_for(cache: &Cache) -> String {
    let known: Vec<(String, u64, u64)> = cache
        .values()
        .map(|(raw, _)| (raw.dir.clone(), raw.mtime, raw.size))
        .collect();
    probe::script(&known)
}

/// Puts one call's result into the server's view. A failed call keeps
/// the chats as last known and marks the server offline.
pub fn take_reply(
    server: &str,
    result: Result<String, String>,
    cache: &mut Cache,
    view: &mut HostView,
    now: u64,
) {
    view.checked = Some(now);
    let Some(reply) = result.ok().as_deref().and_then(reply::parse) else {
        view.online = false;
        return;
    };
    view.online = true;
    view.last_seen = Some(now);
    view.cpu = reply.cpu;
    view.ram = reply.ram();
    view.mem = reply.mem;
    view.health = reply.health.clone();
    let running = Running {
        rc: &reply.rc,
        procs: &reply.procs,
        rc_failed: &reply.rc_failed,
    };
    let mut listed = HashSet::new();
    let mut chats = Vec::new();
    let mut replies = HashMap::new();
    for raw in &reply.chats {
        let parsed = match &raw.tail {
            Some(bytes) => status::parse_tail(bytes, raw.size <= probe::TAIL_BYTES),
            None => match cache.get(&raw.dir) {
                Some((_, parsed)) => parsed.clone(),
                // Told "same" for a file never read: wait for the next call.
                None => continue,
            },
        };
        listed.insert(raw.dir.clone());
        replies.insert(
            format!("{server}/{}", raw.dir),
            in_pc_clock(&parsed.replies, reply.now * 1000, now),
        );
        let meta = RawChat {
            tail: None,
            ..raw.clone()
        };
        chats.push(status::chat(
            server,
            &meta,
            &parsed,
            &running,
            reply.now * 1000,
            now,
        ));
        cache.insert(raw.dir.clone(), (meta, parsed));
    }
    cache.retain(|dir, _| listed.contains(dir));
    let ready = status::ready_chats(
        server,
        reply.home.as_deref(),
        &running,
        &reply.ready,
        &chats,
        reply.now * 1000,
        now,
    );
    chats.extend(ready);
    chats.sort_by_key(|chat| std::cmp::Reverse(chat.created));
    view.chats = chats;
    view.replies = replies;
}

/// Replies with their times moved from the server's clock to the PC's, so
/// a server clock that is off does not matter.
fn in_pc_clock(replies: &[Reply], server_now: u64, now: u64) -> Vec<Reply> {
    replies
        .iter()
        .map(|reply| Reply {
            at: now.saturating_sub(server_now.saturating_sub(reply.at)),
            text: reply.text.clone(),
        })
        .collect()
}

/// Reads the servers on threads of their own until dropped.
pub struct Poller {
    stop: Arc<AtomicBool>,
    pub views: Views,
}

impl Poller {
    /// Starts one thread per server. `wake` asks the window to paint.
    pub fn start(
        servers: &[String],
        runner: Arc<dyn Runner>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let views: Views = Arc::default();
        for name in servers {
            let (server, runner, wake) = (name.clone(), runner.clone(), wake.clone());
            let (stop, views) = (stop.clone(), views.clone());
            let spawned = std::thread::Builder::new()
                .name(format!("deck-server-{server}"))
                .spawn(move || poll(&server, &*runner, &*wake, &stop, &views));
            if let Err(err) = spawned {
                log::warn!("ClaudeDeck: could not start reading {name}: {err}");
            }
        }
        Poller { stop, views }
    }
}

impl Drop for Poller {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn poll(server: &str, runner: &dyn Runner, wake: &dyn Fn(), stop: &AtomicBool, views: &Views) {
    let mut cache = Cache::new();
    while !stop.load(Ordering::SeqCst) {
        if !take_busy(server) {
            // A call from before the window closed still runs.
            std::thread::sleep(STEP);
            continue;
        }
        let result = runner.run(server, &script_for(&cache));
        end_busy(server);
        if stop.load(Ordering::SeqCst) {
            return;
        }
        if let Ok(mut views) = views.lock() {
            let view = views.entry(server.to_string()).or_default();
            take_reply(server, result, &mut cache, view, claude_sessions::now_ms());
        }
        wake();
        let mut waited = Duration::ZERO;
        while waited < EVERY && !stop.load(Ordering::SeqCst) {
            std::thread::sleep(STEP);
            waited += STEP;
        }
    }
}
