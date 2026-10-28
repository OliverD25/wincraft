use super::*;

#[test]
fn the_notice_says_which_version_and_where_to_read_more() {
    let content = content("0.16.2");
    assert_eq!(content.title, "WinCraft \u{b7} Updated to 0.16.2");
    assert_eq!(content.lines, ["See what's new in Settings \u{2192} About"]);
    assert_eq!(content.footer, None);
}

#[test]
fn it_holds_for_six_seconds_then_fades_and_goes() {
    assert_eq!(alpha_at(0), Some(255));
    assert_eq!(alpha_at(5_999), Some(255));
    assert_eq!(alpha_at(HOLD_MS), Some(255));
    let half = alpha_at(HOLD_MS + FADE_MS / 2).unwrap();
    assert!((120..=135).contains(&half), "{half}");
    let late = alpha_at(HOLD_MS + FADE_MS - 1).unwrap();
    assert!(late < 5, "{late}");
    assert_eq!(alpha_at(HOLD_MS + FADE_MS), None);
    assert_eq!(alpha_at(60_000), None);
}

#[test]
fn the_fade_never_gets_stronger() {
    let mut last = 255;
    for elapsed in (HOLD_MS..HOLD_MS + FADE_MS).step_by(10) {
        let alpha = alpha_at(elapsed).unwrap();
        assert!(alpha <= last);
        last = alpha;
    }
}

#[test]
fn a_failed_update_stays_eight_seconds() {
    assert_eq!(alpha_after(FAILED_HOLD_MS - 1, FAILED_HOLD_MS), Some(255));
    assert!(alpha_after(FAILED_HOLD_MS + 10, FAILED_HOLD_MS).unwrap() < 255);
    assert_eq!(alpha_after(FAILED_HOLD_MS + FADE_MS, FAILED_HOLD_MS), None);
}

fn moving(content: &Content) -> bool {
    matches!(
        content.bar,
        Some(Bar {
            kind: BarKind::Moving(_),
            ..
        })
    )
}

#[test]
fn each_stage_says_one_thing_and_names_wincraft() {
    let v = || "0.20.69".to_string();
    let checking = stage_content(&Stage::Checking, 0);
    assert_eq!(checking.title, "WinCraft \u{b7} Updating");
    assert_eq!(checking.lines, ["Checking for the new version\u{2026}"]);
    assert!(moving(&checking));

    let down = stage_content(
        &Stage::Downloading {
            version: v(),
            percent: Some(42),
        },
        0,
    );
    assert_eq!(down.title, "WinCraft \u{b7} Updating to 0.20.69");
    assert_eq!(down.lines, ["Downloading WinCraft 0.20.69 \u{b7} 42 %"]);
    let bar = down.bar.unwrap();
    assert_eq!((bar.done, bar.total, bar.kind), (42, 100, BarKind::Plain));

    let unknown = stage_content(
        &Stage::Downloading {
            version: v(),
            percent: None,
        },
        300,
    );
    assert_eq!(unknown.lines, ["Downloading WinCraft 0.20.69\u{2026}"]);
    assert_eq!(unknown.bar.unwrap().kind, BarKind::Moving(300));

    let staged = stage_content(&Stage::UsingStaged { version: v() }, 0);
    assert_eq!(staged.lines, ["Already downloaded, checking it\u{2026}"]);
    assert!(moving(&staged));

    let verifying = stage_content(&Stage::Verifying { version: v() }, 0);
    assert_eq!(verifying.lines, ["Checking the download\u{2026}"]);
    assert!(moving(&verifying));

    let restarting = stage_content(&Stage::Restarting { version: v() }, 0);
    assert_eq!(restarting.lines, ["Restarting WinCraft\u{2026}"]);
    assert_eq!(restarting.bar.unwrap().done, 100);

    let failed = stage_content(
        &Stage::Failed {
            reason: "The download stopped. Is the PC online?".to_string(),
            kept: "0.20.68".to_string(),
        },
        0,
    );
    assert_eq!(failed.title, "WinCraft \u{b7} Update failed");
    assert_eq!(
        failed.lines,
        [
            "The download stopped. Is the PC online?",
            "WinCraft 0.20.68 stays"
        ]
    );
    assert_eq!(failed.bar, None, "no bar on a failure");
    assert!(failed.footer.unwrap().contains("About"));
}

#[test]
fn a_percent_over_a_hundred_fills_the_bar_and_no_more() {
    let content = stage_content(
        &Stage::Downloading {
            version: "1.0.0".to_string(),
            percent: Some(130),
        },
        0,
    );
    assert_eq!(content.bar.unwrap().done, 100);
}

#[test]
fn only_a_failure_ends_the_running_update() {
    assert!(Stage::Checking.running());
    assert!(!Stage::Failed {
        reason: String::new(),
        kept: String::new()
    }
    .running());
    let at = |percent| Stage::Downloading {
        version: "1.0.0".to_string(),
        percent,
    };
    assert!(at(Some(1)).same_step(&at(Some(80))));
    assert!(!at(None).same_step(&Stage::Checking));
}

#[test]
fn a_moving_bar_sweeps_and_starts_again() {
    assert_eq!(sweep_phase(0), 0);
    assert_eq!(sweep_phase(SWEEP_MS / 2), 500);
    assert_eq!(sweep_phase(SWEEP_MS), 0);
    assert!(sweep_phase(SWEEP_MS - 1) > 990);
}

#[test]
fn errors_are_said_in_plain_words() {
    assert_eq!(
        plain_reason(
            "the download does not match its checksum (expected a, got b), so it was deleted"
        ),
        "The download did not match its checksum, so it was deleted."
    );
    assert_eq!(
        plain_reason("Could not put the new version in place: access denied"),
        "The new version could not be saved next to the old one."
    );
    assert_eq!(
        plain_reason("release v1.0.0 has no wincraft.exe"),
        "This release has no WinCraft download."
    );
    assert_eq!(
        plain_reason("http status: 503"),
        "GitHub did not give the download. Try again later."
    );
    assert_eq!(
        plain_reason("the download stopped: connection reset"),
        "The download stopped. Is the PC online?"
    );
    assert_eq!(plain_version("v0.20.69"), "0.20.69");
}
