//! The homelab servers part of ClaudeDeck: the reply, the names, the
//! statuses, the waits, the lines and the reader. No test reaches a real
//! server: the reader runs a fake.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::core::claude_sessions::Age;
use crate::core::claude_turns::{End, SshCall, Turn};
use crate::plugins::claude_deck::data;
use crate::plugins::claude_deck::model::{Chat, Mark};

use super::poll::{self, Cache, HostView, Runner};
use super::reply::{self, RawChat};
use super::status::{self, Running};
use super::view::{self, ServerLine, Shape};
use super::*;

const NOW_S: u64 = 1_794_490_000;
const NOW: u64 = NOW_S * 1000;

fn b64(bytes: &[u8]) -> String {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (at, b)| n | (u32::from(*b) << (16 - 8 * at)));
        for at in 0..4 {
            if at <= chunk.len() {
                out.push(ABC[(n >> (18 - 6 * at) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn user(text: &str) -> String {
    format!(
        r#"{{"parentUuid":"p","isSidechain":false,"type":"user","message":{{"role":"user","content":"{text}"}},"cwd":"/home/rde/projects/verdict-ai","uuid":"u"}}"#
    )
}

fn said(text: &str) -> String {
    format!(
        r#"{{"parentUuid":"p","isSidechain":false,"message":{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}]}},"cwd":"/home/rde/projects/verdict-ai","type":"assistant","uuid":"a"}}"#
    )
}

fn tool(name: &str) -> String {
    format!(
        r#"{{"parentUuid":"p","isSidechain":false,"message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t","name":"{name}","input":{{"questions":[{{"question":"Keep the old token?"}}]}}}}]}},"cwd":"/home/rde/projects/verdict-ai","type":"assistant","uuid":"a"}}"#
    )
}

fn tail(lines: &[String]) -> Vec<u8> {
    let mut text = lines.join("\n");
    text.push('\n');
    text.into_bytes()
}

/// A reply with two chats, as the probe prints it.
fn two_chat_reply() -> String {
    let one = b64(&tail(&[user("Run the tests"), said("All tests pass.")]));
    let two = b64(&tail(&[user("Deploy"), tool("Bash")]));
    // base64 wraps its lines at 76 characters.
    let wrap = |text: &str| -> String {
        let lines: Vec<String> = text
            .as_bytes()
            .chunks(76)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect();
        lines.join("\n")
    };
    format!(
        "NOW {NOW_S}\nCPU 34\nMEM 16000000 6240000\nRC verdict-ai\nPROC /home/rde/projects/unicomos-dev\n\
         CHAT -home-rde-projects-verdict-ai a.jsonl {} 100\n{}\nEND\n\
         CHAT -home-rde-projects-unicomos-dev b.jsonl {} 200\n{}\nEND\nDONE\n",
        NOW_S - 7200,
        wrap(&one),
        NOW_S - 10,
        wrap(&two),
    )
}

// The reply.

#[test]
fn a_two_chat_reply_is_read_whole() {
    let reply = reply::parse(&two_chat_reply()).expect("a whole reply");
    assert_eq!(reply.now, NOW_S);
    assert_eq!(reply.cpu, Some(34));
    assert_eq!(reply.ram(), Some(61));
    assert_eq!(reply.rc, vec!["verdict-ai"]);
    assert_eq!(reply.procs, vec!["/home/rde/projects/unicomos-dev"]);
    assert_eq!(reply.chats.len(), 2);
    assert_eq!(reply.chats[0].dir, "-home-rde-projects-verdict-ai");
    assert!(reply.chats[0]
        .tail
        .as_ref()
        .is_some_and(|t| t.ends_with(b"\n")));
}

#[test]
fn an_empty_or_cut_reply_means_offline() {
    assert_eq!(reply::parse(""), None);
    assert_eq!(
        reply::parse("NOW 5\nCPU 3\n"),
        None,
        "no DONE: the call was cut"
    );
    assert_eq!(reply::parse("DONE\n"), None, "no NOW");
}

#[test]
fn garbage_lines_are_skipped_never_a_panic() {
    let text = format!(
        "junk\nNOW {NOW_S}\nCPU 300\nMEM a b\nRC\nCHAT x\nCHAT d f 1 2\n%%%not base64%%%\nEND\n\
         CHAT d2 f 1 2\nQUJD\nEND\nSAME d3 f 9 9\nSAME broken\n\u{0}\u{fffd}\nDONE\n"
    );
    let reply = reply::parse(&text).expect("still a reply");
    assert_eq!(reply.cpu, None, "300 % is not a CPU load");
    assert_eq!(reply.mem, None);
    assert!(reply.rc.is_empty());
    let dirs: Vec<&str> = reply.chats.iter().map(|c| c.dir.as_str()).collect();
    assert_eq!(dirs, vec!["d2", "d3"]);
    assert_eq!(reply.chats[0].tail.as_deref(), Some(&b"ABC"[..]));
    assert_eq!(reply.chats[1].tail, None, "SAME carries no tail");
}

#[test]
fn base64_decodes_and_refuses_broken_text() {
    assert_eq!(reply::decode("aGk=").as_deref(), Some(&b"hi"[..]));
    assert_eq!(reply::decode("aGk").as_deref(), None);
    assert_eq!(reply::decode("a=Gk").as_deref(), None);
    assert_eq!(reply::decode("").as_deref(), Some(&b""[..]));
    for text in ["", "a", "ab", "abc", "hello, server"] {
        assert_eq!(
            reply::decode(&b64(text.as_bytes())).as_deref(),
            Some(text.as_bytes())
        );
    }
}

// Names.

#[test]
fn the_setting_keeps_only_plain_ssh_names() {
    assert_eq!(
        parse_servers("rdehome1, rdehome2  rdeoffice1,,RDEHOME1, -oProxyCommand=x, a;b"),
        vec!["rdehome1", "rdehome2", "rdeoffice1"]
    );
    assert!(parse_servers("").is_empty());
    assert_eq!(group_name("rdehome2"), "home2");
    assert_eq!(group_name("rde"), "rde");
    assert_eq!(group_name("office"), "office");
}

#[test]
fn every_name_on_a_host_line_means_that_server() {
    let config = "Host rdehome1 rdeserverpc homelab\n  HostName 192.168.88.166\n\
                  Host rdehomelab2 rdeoffice1\nHost *.example ?x !no\nhost=other\nHost rdehome2 # note\n";
    let servers = vec![
        "rdehome1".to_string(),
        "rdeoffice1".to_string(),
        "solo".to_string(),
    ];
    let aliases = Aliases::build(&servers, Some(config));
    assert_eq!(aliases.server_of("rdeserverpc"), Some("rdehome1"));
    assert_eq!(aliases.server_of("HOMELAB"), Some("rdehome1"));
    assert_eq!(aliases.server_of("rdehomelab2"), Some("rdeoffice1"));
    assert_eq!(aliases.server_of("solo"), Some("solo"));
    assert_eq!(aliases.server_of("rdehome2"), None, "not listed");
    assert_eq!(aliases.server_of("other"), None);
    assert_eq!(Aliases::build(&servers, None).server_of("homelab"), None);
}

// Statuses from the end of a transcript.

fn raw(dir: &str, quiet_s: u64) -> RawChat {
    RawChat {
        dir: dir.to_string(),
        file: "x.jsonl".to_string(),
        mtime: NOW_S - quiet_s,
        size: 10,
        tail: None,
    }
}

fn status_of(lines: &[String], rc: &[&str], procs: &[&str], quiet_s: u64) -> Chat {
    let parsed = status::parse_tail(&tail(lines), true);
    let rc: Vec<String> = rc.iter().map(|s| s.to_string()).collect();
    let procs: Vec<String> = procs.iter().map(|s| s.to_string()).collect();
    let running = Running {
        rc: &rc,
        procs: &procs,
        rc_failed: &[],
    };
    status::chat(
        "rdehome2",
        &raw("-home-rde-projects-verdict-ai", quiet_s),
        &parsed,
        &running,
        NOW,
        NOW + 5_000,
    )
}

#[test]
fn the_end_of_a_transcript_gives_the_status() {
    let done = [user("Run"), said("All tests pass.")];
    let idle = status_of(&done, &["verdict-ai"], &[], 7200);
    assert_eq!(idle.mark, Mark::Idle(Age::Cooling));
    assert_eq!(idle.title, "verdict-ai");
    assert_eq!(
        idle.folder.as_deref(),
        Some("/home/rde/projects/verdict-ai")
    );
    assert_eq!(idle.since, Some(NOW + 5_000 - 7_200_000), "the PC's clock");
    assert_eq!(idle.id, "rdehome2/-home-rde-projects-verdict-ai");

    let fresh = status_of(&done, &[], &["/home/rde/projects/verdict-ai"], 20);
    assert_eq!(fresh.mark, Mark::Working, "a write in the last 60 s");

    let running = status_of(&[user("Go"), tool("Bash")], &["verdict-ai"], &[], 600);
    assert_eq!(running.mark, Mark::Working);
    assert_eq!(running.text.as_deref(), Some("Running a command"));

    let asks = status_of(&[tool("AskUserQuestion")], &["verdict-ai"], &[], 600);
    assert_eq!(asks.mark, Mark::AsksYou);
    assert_eq!(asks.text.as_deref(), Some("Keep the old token?"));

    let gone = status_of(&[user("Go"), tool("Bash")], &[], &[], 600);
    assert_eq!(gone.mark, Mark::Stopped, "no unit and no process");
}

#[test]
fn a_cut_first_line_is_skipped() {
    let mut bytes = b"t\":\"half a record\"}\n".to_vec();
    bytes.extend(tail(&[said("Done.")]));
    let parsed = status::parse_tail(&bytes, false);
    assert_eq!(parsed.turn.end, End::Replied);
    assert_eq!(parsed.cwd.as_deref(), Some("/home/rde/projects/verdict-ai"));
}

// PC chats waiting on a server.

fn working(id: &str) -> Chat {
    Chat {
        id: id.to_string(),
        title: "Verdict deploy check".to_string(),
        folder: None,
        branch: None,
        model: None,
        parent: None,
        created: 1,
        mark: Mark::Working,
        since: Some(NOW - 60_000),
        jobs: 0,
        window: Some(5),
        text: None,
        full: None,
    }
}

fn ssh_turn(hosts: &[&str], command: &str) -> Turn {
    Turn {
        end: End::ToolCalled,
        tool: Some("Bash".to_string()),
        ssh: Some(SshCall {
            hosts: hosts.iter().map(|h| h.to_string()).collect(),
            command: command.to_string(),
            at: Some(NOW - 240_000),
        }),
        ..Turn::default()
    }
}

fn home_aliases() -> Aliases {
    Aliases::build(
        &["rdehome2".to_string()],
        Some("Host rdehome2 home2alias\n"),
    )
}

#[test]
fn a_foreground_ssh_to_a_listed_server_waits_on_it() {
    let turn = ssh_turn(
        &["home2alias"],
        "ssh home2alias 'cd ~/projects/verdict-ai && docker compose up -d'",
    );
    let mut waits = Vec::new();
    let chat = data::waits_on_server(working("c1"), Some(&turn), &home_aliases(), &mut waits);
    assert_eq!(chat.mark, Mark::WaitsOnServer);
    assert_eq!(chat.since, Some(NOW - 240_000), "since the command started");
    assert_eq!(
        chat.text.as_deref(),
        Some("Waits on home2 \u{b7} docker compose up -d")
    );
    assert!(chat
        .full
        .as_deref()
        .is_some_and(|f| f.contains("docker compose up")));
    assert_eq!(waits.len(), 1);
    assert_eq!(waits[0].server, "rdehome2");
    assert_eq!(waits[0].chat, "c1");
}

#[test]
fn no_wait_without_a_foreground_ssh_to_a_listed_server() {
    let aliases = home_aliases();
    let mut waits = Vec::new();
    // Another host.
    let other = ssh_turn(&["elsewhere"], "ssh elsewhere ls");
    let chat = data::waits_on_server(working("a"), Some(&other), &aliases, &mut waits);
    assert_eq!(chat.mark, Mark::Working);
    // No ssh call kept: a background job or another command.
    let plain = Turn {
        end: End::ToolCalled,
        tool: Some("Bash".to_string()),
        ..Turn::default()
    };
    let chat = data::waits_on_server(working("b"), Some(&plain), &aliases, &mut waits);
    assert_eq!(chat.mark, Mark::Working);
    // A chat that needs approval for the command still needs approval.
    let turn = ssh_turn(&["rdehome2"], "ssh rdehome2 ls");
    let asking = Chat {
        mark: Mark::NeedsApproval,
        ..working("c")
    };
    let chat = data::waits_on_server(asking, Some(&turn), &aliases, &mut waits);
    assert_eq!(chat.mark, Mark::NeedsApproval);
    // With no server listed, nothing waits.
    let chat = data::waits_on_server(working("d"), Some(&turn), &Aliases::default(), &mut waits);
    assert_eq!(chat.mark, Mark::Working);
    assert!(waits.is_empty());
}

#[test]
fn waits_on_a_server_ranks_right_after_working() {
    let at = |mark: Mark| Mark::URGENCY.iter().position(|m| *m == mark);
    assert_eq!(at(Mark::WaitsOnServer), at(Mark::Working).map(|w| w + 1));
    assert_eq!(Mark::WaitsOnServer.rank(), Mark::Working.rank() + 1);
}

// The lines and their words.

fn view_of(chats: Vec<Chat>, online: bool) -> HostView {
    HostView {
        checked: Some(NOW - 20_000),
        online,
        last_seen: Some(NOW - 3 * 3_600_000),
        cpu: Some(34),
        ram: Some(61),
        mem: Some((16_000_000, 6_240_000)),
        chats,
        health: reply::Health::default(),
    }
}

fn server_chat(id: &str, mark: Mark) -> Chat {
    Chat {
        id: id.to_string(),
        mark,
        window: None,
        ..working(id)
    }
}

fn shape_lines(views: &[(String, HostView)], hidden: &BTreeSet<String>) -> Vec<ServerLine> {
    let none = BTreeSet::new();
    view::lines(
        views,
        &Shape {
            pinned: &none,
            hidden,
            quiet_open: &none,
            hidden_open: &none,
            fold_cold: true,
            now: NOW,
        },
    )
}

#[test]
fn a_server_group_shows_its_line_and_its_chats() {
    let chats = vec![
        server_chat("s/a", Mark::Working),
        server_chat("s/b", Mark::AsksYou),
        server_chat("s/c", Mark::Idle(Age::Cold)),
    ];
    let host = view_of(chats, true);
    assert_eq!(
        view::line_text(&host, NOW),
        "CPU 34% \u{b7} RAM 61% \u{b7} 3"
    );
    let views = vec![("rdehome2".to_string(), host)];
    let lines = shape_lines(&views, &BTreeSet::new());
    assert_eq!(lines.len(), 4, "server, two chats, the quiet line");
    assert!(matches!(lines[0], ServerLine::Server(0)));
    assert!(matches!(&lines[1], ServerLine::Chat { greyed: false, .. }));
    assert!(matches!(
        &lines[3],
        ServerLine::Quiet {
            chats: 1,
            open: false,
            ..
        }
    ));
    let hidden: BTreeSet<String> = ["s/a".to_string()].into();
    let lines = shape_lines(&views, &hidden);
    assert!(matches!(
        &lines.last(),
        Some(ServerLine::Hidden { chats: 1, .. })
    ));
}

#[test]
fn an_offline_server_keeps_its_chats_greyed() {
    let host = view_of(vec![server_chat("s/a", Mark::Working)], false);
    assert_eq!(
        view::line_text(&host, NOW),
        "Offline \u{b7} last seen 3 h ago"
    );
    let views = vec![("rdehome2".to_string(), host)];
    let lines = shape_lines(&views, &BTreeSet::new());
    assert!(matches!(&lines[1], ServerLine::Chat { greyed: true, .. }));
    let fresh = HostView::default();
    assert_eq!(view::line_text(&fresh, NOW), "Checking\u{2026}");
    assert_eq!(
        view::line_text(&view_of(Vec::new(), true), NOW),
        "No Claude chats"
    );
}

#[test]
fn the_server_hover_names_the_pc_chats_that_wait() {
    let host = view_of(vec![server_chat("s/a", Mark::Working)], true);
    let waits = vec![Wait {
        chat: "c1".to_string(),
        server: "rdehome2".to_string(),
        since: Some(NOW - 240_000),
        title: "Verdict deploy check".to_string(),
    }];
    let words = view::hover_text("rdehome2", &host, &waits, NOW);
    assert!(
        words.starts_with("home2 \u{b7} online \u{b7} checked 20 s ago"),
        "{words}"
    );
    assert!(words.contains("1 Claude chat: 1 working."), "{words}");
    assert!(
        words.contains("CPU 34%, RAM 61% (9.3 of 15 GB)."),
        "{words}"
    );
    assert!(words.contains("1 PC chat waits on home2:"), "{words}");
    assert!(
        words.contains("Verdict deploy check \u{b7} 4 min"),
        "{words}"
    );
    assert!(!view::hover_text("rdehome1", &host, &waits, NOW).contains("waits"));
}

// The probe and the terminal.

#[test]
fn the_probe_is_told_what_it_need_not_send_and_ssh_never_prompts() {
    let script = probe::script(&[
        ("-home-a".to_string(), 5, 6),
        ("bad name'; rm".to_string(), 1, 1),
    ]);
    assert!(script.starts_with("known='-home-a:5:6'\n"));
    assert!(!script.contains("rm"), "a strange folder name is left out");
    let args = probe::ssh_args("rdehome2");
    assert!(args.windows(2).any(|w| w == ["-o", "BatchMode=yes"]));
    assert!(args.windows(2).any(|w| w == ["--", "rdehome2"]));
    assert_eq!(args[args.len() - 2..], ["sh", "-s"]);
}

#[test]
fn the_terminal_starts_a_shell_in_the_folder_when_the_path_is_safe() {
    assert_eq!(
        terminal::remote_command(Some("/home/rde/projects/verdict-ai")),
        "cd '/home/rde/projects/verdict-ai' && exec $SHELL -l"
    );
    assert_eq!(
        terminal::remote_command(Some("/x'; rm -rf /")),
        "exec $SHELL -l"
    );
    assert_eq!(terminal::remote_command(None), "exec $SHELL -l");
    assert_eq!(
        terminal::ssh_args("rdehome2", None)[..3],
        ["-t", "--", "rdehome2"]
    );
}

// The reader.

struct Fake {
    calls: AtomicUsize,
    reply: Mutex<String>,
}

impl Runner for Fake {
    fn run(&self, _host: &str, _script: &str) -> Result<String, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.reply.lock().map(|r| r.clone()).unwrap_or_default())
    }
}

fn fake() -> Arc<Fake> {
    Arc::new(Fake {
        calls: AtomicUsize::new(0),
        reply: Mutex::new(two_chat_reply()),
    })
}

fn wait_for(what: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if what() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn nothing_is_read_and_no_group_shows_while_no_server_is_listed() {
    let runner = fake();
    let mut deck = Deck::with_runner(runner.clone());
    deck.tick(&[], Arc::new(|| {}));
    assert!(!deck.is_on());
    assert!(!deck.is_reading());
    assert!(deck.views().is_empty());
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(runner.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn the_reader_runs_only_while_the_window_is_open() {
    let runner = fake();
    // The window is closed: the deck was never ticked, so nothing runs.
    let deck = Deck::with_runner(runner.clone());
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(runner.calls.load(Ordering::SeqCst), 0);
    drop(deck);
    // Open: the first call comes at once, and the window is woken.
    let woken = Arc::new(AtomicUsize::new(0));
    let mut deck = Deck::with_runner(runner.clone());
    let counter = woken.clone();
    deck.tick(
        &["test-reader-host".to_string()],
        Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }),
    );
    assert!(deck.is_reading());
    assert!(wait_for(|| woken.load(Ordering::SeqCst) >= 1));
    let views = deck.views();
    assert_eq!(views[0].0, "test-reader-host");
    assert!(views[0].1.online);
    assert_eq!(views[0].1.chats.len(), 2);
    // Closed again: dropping the deck stops the reader before its next call.
    drop(deck);
    let calls = runner.calls.load(Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(runner.calls.load(Ordering::SeqCst), calls);
    assert_eq!(calls, 1);
}

#[test]
fn an_unchanged_transcript_is_not_sent_again_and_a_failed_call_keeps_the_chats() {
    let mut cache = Cache::new();
    let mut host = HostView::default();
    poll::take_reply("s", Ok(two_chat_reply()), &mut cache, &mut host, NOW);
    assert_eq!(host.chats.len(), 2);
    let script = poll::script_for(&cache);
    assert!(
        script.contains("-home-rde-projects-verdict-ai:"),
        "{script}"
    );
    // The server says both are the same: the chats are built from the cache.
    let same = format!(
        "NOW {NOW_S}\nRC verdict-ai\nPROC /home/rde/projects/unicomos-dev\nSAME -home-rde-projects-verdict-ai a.jsonl {} 100\nSAME -home-rde-projects-unicomos-dev b.jsonl {} 200\nDONE\n",
        NOW_S - 7200,
        NOW_S - 10
    );
    let before = host.chats.clone();
    poll::take_reply("s", Ok(same), &mut cache, &mut host, NOW);
    assert_eq!(host.chats.len(), 2);
    assert_eq!(host.chats[0].mark, before[0].mark);
    // ssh failed: offline, the chats as last known.
    poll::take_reply(
        "s",
        Err("timed out".to_string()),
        &mut cache,
        &mut host,
        NOW + 1,
    );
    assert!(!host.online);
    assert_eq!(host.last_seen, Some(NOW));
    assert_eq!(host.chats.len(), 2);
}

// Extra 1: a broken Remote Control service shows red.

#[test]
fn a_failed_remote_control_unit_makes_its_chat_failed() {
    let text =
        format!("NOW {NOW_S}\nRC verdict-ai\nRCFAIL unicomos-dev\nRCFAIL\nSAME x y 1 2\nDONE\n");
    let reply = reply::parse(&text).expect("a reply");
    assert_eq!(reply.rc_failed, vec!["unicomos-dev"]);
    let parsed = status::parse_tail(&tail(&[said("Done.")]), true);
    let failed = vec!["unicomos-dev".to_string()];
    let running = Running {
        rc: &[],
        procs: &[],
        rc_failed: &failed,
    };
    let chat = status::chat(
        "rdehome2",
        &raw("-home-rde-projects-unicomos-dev", 600),
        &parsed,
        &running,
        NOW,
        NOW,
    );
    assert_eq!(chat.mark, Mark::Failed);
    assert_eq!(chat.text.as_deref(), Some(status::RC_STOPPED));
    let other = status::chat(
        "rdehome2",
        &raw("-home-rde-projects-verdict-ai", 600),
        &parsed,
        &running,
        NOW,
        NOW,
    );
    assert_eq!(other.mark, Mark::Stopped, "another unit is not touched");
    assert!(probe::script(&[]).contains("RCFAIL"));
}

// Extra 4: the server's health in the hover panel.

#[test]
fn the_servers_health_is_read_and_shown() {
    let text = format!(
        "NOW {NOW_S}\nUP 1036800\nREBOOT 1\nDISK 125829120 536870912\nTOP 85 dockerd\nDONE\n"
    );
    let reply = reply::parse(&text).expect("a reply");
    assert_eq!(reply.health.up, Some(1_036_800));
    assert!(reply.health.reboot);
    let lines = view::health_lines(&reply.health);
    assert_eq!(
        lines,
        vec![
            "Up 12 days \u{b7} Restart needed",
            "Disk 120 GB free of 512 GB",
            "Busiest: dockerd 85%",
        ]
    );
    let broken = format!("NOW {NOW_S}\nUP x\nDISK 9 1\nTOP a b\nREBOOT 0\nDONE\n");
    let reply = reply::parse(&broken).expect("a reply");
    assert_eq!(reply.health, reply::Health::default());
    assert!(view::health_lines(&reply.health).is_empty());
    let mut host = view_of(Vec::new(), true);
    host.health = reply::parse(&text).expect("a reply").health;
    let words = view::hover_text("rdehome2", &host, &[], NOW);
    assert!(words.contains("Busiest: dockerd 85%"), "{words}");
}
