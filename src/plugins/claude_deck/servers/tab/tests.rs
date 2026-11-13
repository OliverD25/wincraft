//! The Servers tab's lines, words and problems, without drawing. No test
//! reaches a server.

use std::collections::BTreeSet;

use crate::core::claude_sessions::Age;
use crate::plugins::claude_deck::model::{Chat, Mark};

use super::super::load::{Kind, Proc, Service, State};
use super::super::reply::Health;
use super::super::view::{ServerLine, Shape};
use super::super::HostView;
use super::view::{self, Line};
use super::Pick;

const NOW: u64 = 1_794_490_000_000;

fn chat(id: &str, folder: &str, mark: Mark) -> Chat {
    Chat {
        id: id.to_string(),
        title: format!("Chat {id}"),
        folder: Some(folder.to_string()),
        branch: None,
        model: None,
        parent: None,
        created: 1,
        mark,
        since: Some(NOW - 60_000),
        jobs: 0,
        window: None,
        text: None,
        full: None,
        fork: None,
        terminal: None,
        news: false,
        job_on: None,
    }
}

fn claude(pid: u32, folder: &str) -> Proc {
    Proc {
        pid,
        cpu10: 125,
        rss_kb: 204_800,
        secs: Some(3_700),
        folder: Some(folder.to_string()),
        name: "claude".to_string(),
        command: "claude --resume".to_string(),
    }
}

fn service(kind: Kind, name: &str, state: State) -> Service {
    Service {
        kind,
        name: name.to_string(),
        state,
        detail: "Up 3 days".to_string(),
    }
}

fn host(online: bool) -> HostView {
    HostView {
        checked: Some(NOW - 20_000),
        online,
        last_seen: Some(NOW - 3 * 3_600_000),
        cpu: Some(34),
        ram: Some(61),
        health: Health {
            up: Some(12 * 86_400),
            reboot: true,
            disk: Some((120 * 1024 * 1024, 512 * 1024 * 1024)),
            top: None,
        },
        chats: vec![
            chat("s/a", "/home/rde/projects/wincraft", Mark::Working),
            chat("s/b", "/home/rde/projects/verdict", Mark::AsksYou),
        ],
        claude: vec![
            claude(10, "/home/rde/projects/wincraft"),
            claude(11, "/tmp/old"),
        ],
        busiest: vec![Proc {
            pid: 299,
            cpu10: 851,
            rss_kb: 1_668,
            name: "redis-server".to_string(),
            command: "/usr/bin/redis-server".to_string(),
            ..Proc::default()
        }],
        services: vec![
            service(Kind::Docker, "web", State::Running),
            service(Kind::System, "nginx.service", State::Failed),
            service(Kind::User, "backup.service", State::Restarting),
        ],
        ..HostView::default()
    }
}

fn lines_of(views: &[(String, HostView)], folded: &BTreeSet<String>) -> Vec<Line> {
    let none = BTreeSet::new();
    let shape = Shape {
        pinned: &none,
        hidden: &none,
        quiet_open: &none,
        hidden_open: &none,
        fold_cold: true,
        now: NOW,
    };
    let history = crate::plugins::claude_deck::servers::jobs::history::History::default();
    let work = view::Work {
        jobs: &[],
        history: &history,
    };
    view::lines(views, &|server| folded.contains(server), &shape, &work)
}

#[test]
fn a_card_has_its_sections_in_order_and_leaves_out_empty_ones() {
    let views = vec![("rdehome2".to_string(), host(true))];
    let lines = lines_of(&views, &BTreeSet::new());
    let sections: Vec<&str> = lines
        .iter()
        .filter_map(|line| match line {
            Line::Section(title) => Some(*title),
            _ => None,
        })
        .collect();
    assert_eq!(
        sections,
        [view::CHATS, view::CLAUDE, view::BUSIEST, view::SERVICES]
    );
    assert!(matches!(
        lines[0],
        Line::Card {
            at: 0,
            folded: false
        }
    ));
    assert!(
        !lines
            .iter()
            .any(|line| matches!(line, Line::Chat(ServerLine::Server(_)))),
        "the header takes the server line's place"
    );
    // Problems first among the services.
    let services: Vec<&str> = lines
        .iter()
        .filter_map(|line| match line {
            Line::Service { service, .. } => Some(service.name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(services, ["nginx.service", "backup.service", "web"]);
    let mut bare = host(true);
    bare.busiest.clear();
    bare.services.clear();
    bare.claude.clear();
    bare.chats.clear();
    let views = vec![("rdehome2".to_string(), bare)];
    assert_eq!(
        lines_of(&views, &BTreeSet::new()).len(),
        1,
        "only the header"
    );
}

#[test]
fn a_folded_card_shows_only_its_header_and_picks_follow_the_lines() {
    let views = vec![
        ("rdehome1".to_string(), host(true)),
        ("rdehome2".to_string(), host(true)),
    ];
    let folded: BTreeSet<String> = ["rdehome1".to_string()].into();
    let lines = lines_of(&views, &folded);
    assert!(matches!(
        lines[0],
        Line::Card {
            at: 0,
            folded: true
        }
    ));
    assert!(matches!(
        lines[1],
        Line::Card {
            at: 1,
            folded: false
        }
    ));
    let picks: Vec<Pick> = lines.iter().filter_map(|l| l.pick(&views)).collect();
    assert_eq!(picks[0], Pick::Card("rdehome1".to_string()));
    assert_eq!(picks[1], Pick::Card("rdehome2".to_string()));
    assert_eq!(picks[2], Pick::Chat("s/a".to_string()));
    assert!(picks.contains(&Pick::Proc {
        server: "rdehome2".to_string(),
        pid: 11,
        claude: true
    }));
    assert!(
        lines
            .iter()
            .filter(|l| matches!(l, Line::Section(_)))
            .count()
            == 4,
        "section titles are not picks"
    );
    assert_eq!(picks.len(), 2 + 2 + 2 + 1 + 3);
}

#[test]
fn a_claude_process_belongs_to_the_chat_in_its_folder_or_is_an_orphan() {
    let view = host(true);
    let owner = view::owner(&view.claude[0], &view.chats).map(|c| c.id.as_str());
    assert_eq!(owner, Some("s/a"));
    assert!(view::owner(&view.claude[1], &view.chats).is_none());
    // A live chat wins over a stopped one in the same folder.
    let chats = vec![
        chat("s/old", "/w", Mark::Stopped),
        chat("s/new", "/w", Mark::Idle(Age::Fresh)),
    ];
    let proc = claude(1, "/w");
    assert_eq!(
        view::owner(&proc, &chats).map(|c| c.id.as_str()),
        Some("s/new")
    );
    let lines = lines_of(&[("rdehome2".to_string(), view)], &BTreeSet::new());
    let owners: Vec<Option<Option<String>>> = lines
        .iter()
        .filter_map(|line| match line {
            Line::Proc { owner, .. } => Some(owner.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        owners,
        [Some(Some("Chat s/a".to_string())), Some(None), None],
        "two claude processes, then the busiest one"
    );
}

#[test]
fn problems_count_offline_servers_failed_services_and_orphans() {
    let views = vec![
        ("rdehome1".to_string(), host(false)),
        ("rdehome2".to_string(), host(true)),
        ("rdenew".to_string(), HostView::default()),
    ];
    let problems = view::problems(&views);
    assert_eq!(
        problems,
        [
            "home1 is offline",
            "home2: nginx.service failed",
            "home2: backup.service restarting",
            "home2: a claude process with no chat (PID 11)",
        ],
        "an offline server counts once; one not checked yet not at all"
    );
    assert_eq!(view::problems_text(&[]), "No problems on the servers.");
    let mut fine = host(true);
    fine.services.retain(|s| !s.state.is_problem());
    fine.claude.truncate(1);
    assert!(view::problems(&[("rdehome2".to_string(), fine)]).is_empty());
}

#[test]
fn the_words_of_the_header_and_the_rows() {
    let online = host(true);
    assert_eq!(
        view::card_text(&online, NOW),
        "CPU 34% \u{b7} RAM 61% \u{b7} Disk 120 GB free \u{b7} Up 12 days"
    );
    assert!(view::needs_restart(&online));
    let offline = host(false);
    assert_eq!(
        view::card_text(&offline, NOW),
        "Offline \u{b7} last seen 3 h ago"
    );
    assert!(!view::needs_restart(&offline), "old health is not shown");
    assert_eq!(
        view::card_text(&HostView::default(), NOW),
        "Checking\u{2026}"
    );
    let proc = claude(10, "/home/rde/projects/wincraft/");
    assert_eq!(view::proc_right(&proc), "12.5% \u{b7} 200 MB \u{b7} 1 h");
    assert_eq!(view::proc_note(&proc), "wincraft");
    let busy = &online.busiest[0];
    assert_eq!(view::proc_right(busy), "85.1% \u{b7} 2 MB");
    assert_eq!(view::proc_note(busy), "PID 299");
    assert_eq!(view::cpu_text(30), "3%");
    assert_eq!(view::ram_text(900), "900 kB");
    assert_eq!(view::ram_text(3 * 1024 * 1024 / 2), "1.5 GB");
    assert_eq!(
        view::service_right(&service(Kind::Docker, "web", State::Running)),
        "running \u{b7} Up 3 days"
    );
    assert_eq!(
        view::service_right(&service(Kind::System, "a", State::Failed)),
        "failed"
    );
    assert_eq!(
        view::proc_hover(&proc),
        "PID 10\n/home/rde/projects/wincraft/\nclaude --resume"
    );
}

#[test]
fn the_chats_tab_has_one_short_line_per_server_and_its_chats_that_wait_for_you() {
    use super::super::view as group_view;
    let views = vec![
        ("rdehome1".to_string(), host(false)),
        ("rdehome2".to_string(), host(true)),
    ];
    let none = BTreeSet::new();
    let hidden: BTreeSet<String> = ["s/hidden".to_string()].into();
    let shape = Shape {
        pinned: &none,
        hidden: &hidden,
        quiet_open: &none,
        hidden_open: &none,
        fold_cold: true,
        now: NOW,
    };
    let lines = group_view::short_lines(&views, &shape);
    let ids: Vec<String> = lines
        .iter()
        .map(|line| match line {
            ServerLine::Server(at) => format!("server {at}"),
            ServerLine::Chat { chat, greyed, .. } => format!("{} {greyed}", chat.id),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(
        ids,
        ["server 0", "s/b true", "server 1", "s/b false"],
        "only the chat that asks; the working one is counted, not listed"
    );
    let mut more = host(true);
    more.chats.push(chat("s/hidden", "/h", Mark::NeedsApproval));
    more.chats.push(chat("s/c", "/c", Mark::Working));
    assert_eq!(group_view::counts(&more), (2, 2));
    assert_eq!(
        group_view::line_text(&more, 0, NOW),
        "2 wait for you \u{b7} 2 working"
    );
    assert_eq!(
        group_view::line_text(&more, 1, NOW),
        "1 PC chat waits \u{b7} 2 wait for you \u{b7} 2 working"
    );
    let lines = group_view::short_lines(&[("s".to_string(), more)], &shape);
    assert_eq!(lines.len(), 2, "the hidden chat stays hidden");
}

#[test]
fn a_stopped_systemd_service_is_left_out_but_a_stopped_container_stays() {
    let mut view = host(true);
    view.services = vec![
        service(Kind::User, "verdict-backup.service", State::Stopped),
        service(Kind::System, "old.service", State::Stopped),
        service(Kind::Docker, "redis", State::Stopped),
        service(Kind::User, "verdict-api.service", State::Running),
    ];
    let views = vec![("rdehome1".to_string(), view)];
    let names: Vec<String> = lines_of(&views, &BTreeSet::new())
        .into_iter()
        .filter_map(|line| match line {
            Line::Service { service, .. } => Some(service.name),
            _ => None,
        })
        .collect();
    assert_eq!(names, ["verdict-api.service", "redis"]);
}
