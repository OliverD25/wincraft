//! Server jobs started by PC chats. The probe lines are real output of
//! home1 and home2 (2026-11-13); no test reaches a server.

use super::super::poll::{self, Cache, HostView};
use super::super::{probe, reply};
use super::load;

const JOBS_REPLY: &str = "NOW 1794532915\nHOME /home/rde\n\
JOB verdict-archive-write.service t 170233 7309 2.4 2774592\t/home/rde/projects/verdict-ai-archive/api\t/home/rde/projects/verdict-ai-archive/api/.venv/bin/python -m tools.archive write --time-limit 19800 \n\
JOB verdict-fetch-remote.service e 248625 4724 2.4 42796\t/home/rde/projects/verdict-ai/homelab\t/home/rde/projects/verdict-ai/homelab/.venv/bin/python office_fetch.py run --delay-min 1.1\n\
JOB - - 2438864 379 0.1 50532\t/home/rde/projects/verdict-ai/api\tbash /tmp/topup_b.sh \n\
JOB verdict-queue-fix-20261113.service - 2449026 9 0.1 3588\t/home/rde\t/usr/bin/bash /tmp/queue_fix.sh \n\
JOB bad line\n\
JOB - - 12 x 0.1 5\t/home/rde\tx\n\
JOB - - 13 5 0.1 5\trelative\tx\n\
UNIT Result=success ExecMainStartTimestamp=@1794525606 ExecMainExitTimestamp= ExecMainStatus=0 Id=verdict-archive-write.service LoadState=loaded ActiveState=activating \n\
UNIT Result=exit-code ExecMainStartTimestamp=@1794520000 ExecMainExitTimestamp=@1794524000 ExecMainStatus=1 Id=verdict-import.service LoadState=loaded ActiveState=failed \n\
UNIT Result=success ExecMainStartTimestamp= ExecMainExitTimestamp= ExecMainStatus=0 Id=nothere.service LoadState=not-found ActiveState=inactive \n\
DONE\n";

#[test]
fn job_and_unit_lines_are_read_and_broken_ones_skipped() {
    let reply = reply::parse(JOBS_REPLY).expect("a whole reply");
    assert_eq!(reply.jobs.len(), 4);
    let first = &reply.jobs[0];
    assert_eq!(first.unit.as_deref(), Some("verdict-archive-write.service"));
    assert!(first.timer && !first.enabled);
    assert_eq!(
        (first.pid, first.secs, first.cpu10, first.rss_kb),
        (170_233, 7309, 24, 2_774_592)
    );
    assert_eq!(first.cwd, "/home/rde/projects/verdict-ai-archive/api");
    assert!(first
        .args
        .ends_with("tools.archive write --time-limit 19800"));
    assert!(reply.jobs[1].enabled && !reply.jobs[1].timer);
    assert_eq!(reply.jobs[2].unit, None);
    assert!(!reply.jobs[3].enabled && !reply.jobs[3].timer);
    assert_eq!(
        reply.units.len(),
        2,
        "a unit systemd does not know is left out"
    );
    let failed = &reply.units[1];
    assert_eq!(failed.name, "verdict-import.service");
    assert!(failed.is_over() && !failed.ended_well());
    assert_eq!(failed.status, Some(1));
    assert_eq!(failed.ended, Some(1_794_524_000_000));
    assert!(!reply.units[0].is_over());
}

#[test]
fn unit_times_move_to_the_pcs_clock() {
    let mut view = HostView::default();
    // The PC's clock is 10 s ahead of the server's.
    let now = 1_794_532_915_000 + 10_000;
    poll::take_reply(
        "rdehome2",
        Ok(JOBS_REPLY.to_string()),
        &mut Cache::new(),
        &mut view,
        now,
    );
    assert_eq!(view.jobs.len(), 4);
    assert_eq!(view.home.as_deref(), Some("/home/rde"));
    assert_eq!(view.units[0].started, Some(1_794_525_606_000 + 10_000));
}

#[test]
fn the_probe_asks_only_about_plain_unit_names() {
    let units = vec![
        "verdict-archive-write.service".to_string(),
        "x'; rm -rf ~; '".to_string(),
    ];
    let script = probe::script_with(&[], &units);
    assert!(script.contains("units='verdict-archive-write.service'\n"));
    assert!(!script.contains("rm -rf"));
    assert!(script.contains("JOB %s"));
    assert!(load::plain_unit("claude-rc@wincraft.service"));
    assert!(!load::plain_unit("a b"));
}
