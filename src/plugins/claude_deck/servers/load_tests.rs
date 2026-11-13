//! The probe's lines for the Servers tab: `claude` processes, the busiest
//! processes, services and containers. The samples are the probe's real
//! output on a Linux box (WSL, 2026-11-13); no test reaches a server.

use super::load::{self, Kind, State};
use super::poll::{self, Cache, HostView};
use super::reply;

const REPLY: &str = "NOW 100\nCPU 2\nMEM 1000 400\n\
PROC /home/rde/projects/wincraft\n\
CLP 2377 12.5 204800 3700 /home/rde/projects/wincraft\tclaude --dangerously-skip-permissions \n\
CLP 2400 0.0 99000 86400 /tmp/old\tclaude\n\
BUSY 299 85.1 1668 redis-server\t/usr/bin/redis-server 127.0.0.1:6379        \n\
BUSY 303 0.0 27416 snapd\t/snap/snapd/current/usr/lib/snapd/snapd \n\
BUSY 2 0.0 1152 init-systemd(Ub\t/init \n\
BUSY 4 0.0 10 a\ta\nBUSY 5 0.0 10 b\tb\nBUSY 6 0.0 10 c\tc\n\
SVC system wsl-pro.service activating auto-restart\n\
SVC system nginx.service failed failed\n\
SVC system nginx.service failed failed\n\
SVC user claude-rc@wincraft.service active running\n\
SVC user backup.service inactive dead\n\
DOCK web\trunning\tUp 3 days\n\
DOCK db\texited\tExited (0) 2 hours ago\n\
DOCK job\texited\tExited (1) 5 minutes ago\n\
DOCK stopped\texited\tExited (137) 1 day ago\n\
DOCK loop\trestarting\tRestarting (1) 4 seconds ago\n\
DONE\n";

#[test]
fn the_tab_lines_are_read_from_the_reply() {
    let reply = reply::parse(REPLY).expect("a whole reply");
    assert_eq!(reply.claude.len(), 2);
    let first = &reply.claude[0];
    assert_eq!(first.pid, 2377);
    assert_eq!(first.cpu10, 125);
    assert_eq!(first.rss_kb, 204_800);
    assert_eq!(first.secs, Some(3700));
    assert_eq!(first.folder.as_deref(), Some("/home/rde/projects/wincraft"));
    assert_eq!(first.command, "claude --dangerously-skip-permissions");
    assert_eq!(reply.busiest.len(), 5, "only the top five are kept");
    assert_eq!(reply.busiest[0].name, "redis-server");
    assert_eq!(reply.busiest[0].cpu10, 851);
    assert_eq!(
        reply.busiest[0].command,
        "/usr/bin/redis-server 127.0.0.1:6379"
    );
    assert_eq!(reply.busiest[2].name, "init-systemd(Ub");
    let states: Vec<(Kind, &str, State)> = reply
        .services
        .iter()
        .map(|s| (s.kind, s.name.as_str(), s.state))
        .collect();
    assert_eq!(
        states,
        [
            (Kind::System, "wsl-pro.service", State::Restarting),
            (Kind::System, "nginx.service", State::Failed),
            (Kind::User, "claude-rc@wincraft.service", State::Running),
            (Kind::User, "backup.service", State::Stopped),
            (Kind::Docker, "web", State::Running),
            (Kind::Docker, "db", State::Stopped),
            (Kind::Docker, "job", State::Failed),
            (Kind::Docker, "stopped", State::Stopped),
            (Kind::Docker, "loop", State::Restarting),
        ],
        "a unit listed twice is kept once"
    );
    assert_eq!(reply.services[4].detail, "Up 3 days");
}

#[test]
fn broken_tab_lines_are_skipped() {
    for line in [
        "x 1 2 3 /a",
        "1 x 2 3 /a",
        "1 2 3 4 relative\tclaude",
        "1 2 3",
        "",
    ] {
        assert_eq!(load::claude_proc(line), None, "{line:?}");
    }
    assert_eq!(load::busy_proc("1 2 3 \tcmd"), None, "no name");
    assert_eq!(load::busy_proc("1 2.x 3 n"), None);
    let bare = load::busy_proc("7 1.0 3 kworker").expect("no command line");
    assert_eq!(bare.command, "kworker");
    assert_eq!(load::unit("other a.service active running"), None);
    assert_eq!(load::unit("system a.service active"), None);
    assert_eq!(load::unit("system a.service active running extra"), None);
    assert_eq!(load::container("\trunning\tUp"), None);
    assert_eq!(load::container("x\tweird\tUp"), None);
    assert_eq!(
        load::container("x\tdead\t").map(|s| s.state),
        Some(State::Failed)
    );
}

#[test]
fn the_view_keeps_the_tab_data_while_the_server_is_offline() {
    let mut cache = Cache::new();
    let mut view = HostView::default();
    poll::take_reply("s", Ok(REPLY.to_string()), &mut cache, &mut view, 1_000);
    assert_eq!(view.claude.len(), 2);
    assert_eq!(view.busiest.len(), 5);
    assert_eq!(view.services.len(), 9);
    poll::take_reply("s", Err("down".to_string()), &mut cache, &mut view, 2_000);
    assert!(!view.online);
    assert_eq!(view.claude.len(), 2, "the last data stays");
    assert_eq!(view.services.len(), 9);
}

#[test]
fn the_probe_reads_the_tab_data_in_the_same_call_and_changes_nothing() {
    let script = super::probe::script(&[]);
    for word in ["CLP ", "BUSY ", "SVC ", "DOCK "] {
        assert!(script.contains(word), "{word}");
    }
    assert!(!script.contains("ssh "), "no second ssh call");
    for change in [
        "systemctl start",
        "systemctl stop",
        "systemctl restart",
        "docker rm",
        "docker stop",
        "kill ",
        " rm ",
    ] {
        assert!(!script.contains(change), "{change}");
    }
}
