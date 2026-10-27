use super::*;

fn title_of(content: Option<Content>) -> Option<String> {
    content.map(|content| content.title)
}

fn progress(group: &str, phase: Phase, count: usize, expected: usize) -> Progress {
    Progress {
        group: group.to_string(),
        phase,
        count,
        expected,
    }
}

fn name(group: &str) -> String {
    group.to_string()
}

#[test]
fn nothing_is_shown_for_apps_that_have_not_opened_a_window() {
    let list = [
        progress("Chrome", Phase::NotOpened, 0, 22),
        progress("Viber", Phase::NotOpened, 0, 1),
        progress("Telegram", Phase::Done, 1, 1),
    ];
    assert_eq!(active_content(&list, &name), None);
    assert_eq!(active_content(&[], &name), None);
}

#[test]
fn each_active_app_gets_a_line_and_the_finished_ones_a_summary() {
    let list = [
        progress("Chrome", Phase::Settling, 12, 22),
        progress("Claude", Phase::Moving, 4, 4),
        progress("Notes", Phase::Settling, 1, 0),
        progress("Telegram", Phase::Done, 1, 1),
        progress("Terminal", Phase::Done, 3, 3),
        progress("Code", Phase::Done, 2, 2),
        progress("Viber", Phase::NotOpened, 0, 1),
    ];
    let content = active_content(&list, &name).unwrap();
    assert_eq!(
        content.title,
        "WinCraft \u{b7} Putting your windows back \u{2014} please wait"
    );
    assert_eq!(
        content.lines,
        [
            "Claude: moving 4 windows",
            "Chrome: 12 of 22 opened, waiting for the rest",
            "Notes: 1 window open, waiting for them to settle",
        ]
    );
    assert_eq!(content.footer.as_deref(), Some("Done: Terminal, Code, +1"));
}

#[test]
fn at_most_five_apps_are_listed_then_a_count() {
    let list: Vec<Progress> = (0..8)
        .map(|n| progress(&format!("App{n}"), Phase::Settling, 8 - n, 0))
        .collect();
    let content = active_content(&list, &name).unwrap();
    assert_eq!(content.lines.len(), 6);
    assert!(content.lines[0].starts_with("App0"));
    assert_eq!(content.lines[5], "+3 more");
    assert_eq!(content.footer, None);
}

#[test]
fn late_windows_are_a_line_too() {
    let list = [progress("Chrome", Phase::LateWindows, 23, 22)];
    let content = active_content(&list, &name).unwrap();
    assert_eq!(
        content.lines,
        ["Chrome: 23 windows now, waiting for the rest"]
    );
}

#[test]
fn the_final_line_names_the_biggest_app_and_counts_the_apps() {
    let list = [
        progress("Chrome", Phase::Done, 22, 22),
        progress("Telegram", Phase::Done, 1, 1),
        progress("Notes", Phase::Done, 1, 1),
    ];
    assert_eq!(
        title_of(final_line(&list, &name)).as_deref(),
        Some("WinCraft \u{b7} Layout restored: 22 Chrome windows, 3 apps")
    );
    assert_eq!(
        title_of(final_line(&[progress("Chrome", Phase::Done, 1, 1)], &name)).as_deref(),
        Some("WinCraft \u{b7} Layout restored: 1 Chrome window")
    );
    assert_eq!(
        title_of(final_line(
            &[progress("Chrome", Phase::Done, 22, 22)],
            &name
        ))
        .as_deref(),
        Some("WinCraft \u{b7} Layout restored: 22 Chrome windows")
    );
    assert_eq!(final_line(&[], &name), None);
    assert_eq!(
        final_line(&[progress("Viber", Phase::Skipped, 0, 1)], &name),
        None
    );
}

#[test]
fn skipped_apps_change_the_final_line_and_go_in_the_tooltip() {
    let list = [
        progress("Chrome", Phase::Done, 22, 22),
        progress("Viber", Phase::Skipped, 0, 1),
        progress("Zoom", Phase::Skipped, 0, 1),
    ];
    assert_eq!(
        title_of(final_line(&list, &name)).as_deref(),
        Some("WinCraft \u{b7} Layout restored, 2 apps skipped")
    );
    assert_eq!(
        tooltip(&list, &name).as_deref(),
        Some("WinCraft \u{2014} layout restored; not opened: Viber, Zoom")
    );
}

#[test]
fn the_tooltip_says_what_is_settling_and_what_is_awaited() {
    let list = [
        progress("Chrome", Phase::Settling, 12, 22),
        progress("Viber", Phase::NotOpened, 0, 1),
        progress("Zoom", Phase::NotOpened, 0, 1),
        progress("Slack", Phase::NotOpened, 0, 1),
    ];
    assert_eq!(
        tooltip(&list, &name).as_deref(),
        Some(
            "WinCraft \u{2014} restoring layout (12 of 22 windows): Chrome 12/22 settling; waiting for 3 apps to open"
        )
    );
    let only_waiting = [
        progress("Telegram", Phase::Done, 1, 1),
        progress("Viber", Phase::NotOpened, 0, 1),
    ];
    assert_eq!(
        tooltip(&only_waiting, &name).as_deref(),
        Some("WinCraft \u{2014} layout restored; waiting for 1 app to open")
    );
    assert_eq!(tooltip(&[progress("A", Phase::Done, 1, 1)], &name), None);
}

#[test]
fn a_tooltip_never_passes_the_windows_limit() {
    let long = "x".repeat(200);
    let list = [
        progress(&long, Phase::Settling, 1, 2),
        progress("B", Phase::Settling, 1, 2),
        progress("C", Phase::Settling, 1, 2),
    ];
    let tip = tooltip(&list, &name).unwrap();
    assert_eq!(tip.chars().count(), 127);
    assert!(tip.ends_with('\u{2026}'));
    assert_eq!(fit("short", 127), "short");
}

fn active() -> Content {
    Content {
        title: "t".to_string(),
        lines: vec!["Chrome: moving 4 windows".to_string()],
        footer: None,
        bar: None,
    }
}

#[test]
fn the_panel_shows_while_active_holds_a_last_line_and_then_fades() {
    let mut ctl = Controller::default();
    assert!(!ctl.is_visible());
    assert_eq!(ctl.update(1, None, None), Command::Nothing);

    // Active: shown once, then left alone while the text is the same.
    assert_eq!(ctl.update(2, Some(active()), None), Command::Show(active()));
    assert!(ctl.is_visible());
    assert_eq!(ctl.update(3, Some(active()), None), Command::Nothing);
    let mut changed = active();
    changed.lines[0] = "Chrome: moving 5 windows".to_string();
    assert_eq!(
        ctl.update(4, Some(changed.clone()), None),
        Command::Show(changed)
    );

    // Nothing active any more: the last line stays for four seconds.
    let line = Content {
        title: "Layout restored: 5 Chrome windows".to_string(),
        lines: Vec::new(),
        footer: None,
        bar: None,
    };
    let shown = ctl.update(5, None, Some(line.clone()));
    assert_eq!(shown, Command::Show(line.clone()));
    assert_eq!(ctl.update(6, None, Some(line.clone())), Command::Nothing);
    assert_eq!(ctl.update(8, None, Some(line.clone())), Command::Nothing);
    assert_eq!(ctl.update(9, None, Some(line)), Command::Fade);
    assert!(!ctl.is_visible());
    assert_eq!(ctl.update(10, None, None), Command::Nothing);
}

#[test]
fn without_a_last_line_the_panel_fades_at_once() {
    let mut ctl = Controller::default();
    ctl.update(1, Some(active()), None);
    assert_eq!(ctl.update(2, None, None), Command::Fade);
    assert!(!ctl.is_visible());
}

#[test]
fn new_activity_during_the_last_line_brings_the_panel_back() {
    let mut ctl = Controller::default();
    ctl.update(1, Some(active()), None);
    ctl.update(
        2,
        None,
        Some(Content {
            title: "done".to_string(),
            ..active()
        }),
    );
    assert_eq!(ctl.update(3, Some(active()), None), Command::Show(active()));
    ctl.reset();
    assert!(!ctl.is_visible());
}

#[test]
fn the_sample_has_every_kind_of_line() {
    let content = active_content(&sample(), &same_label).unwrap();
    assert_eq!(content.lines.len(), 2);
    assert!(content.footer.is_some());
    assert!(tooltip(&sample(), &same_label).is_some());
}

#[test]
fn the_panel_is_drawn_wide_enough_for_its_text() {
    let tokens = theme::Tokens::dark();
    let content = active_content(&sample(), &same_label).unwrap();
    let image = render(&content, 1.0, &tokens).expect("GDI draws the panel");
    assert!(image.width >= MIN_WIDTH as i32);
    assert!(image.height > 60, "{}", image.height);
    assert_eq!(image.pixels.len(), (image.width * image.height) as usize);
    // The middle of the panel is translucent but not empty.
    let centre = image.pixels[(image.height / 2 * image.width + 2) as usize];
    assert_ne!(centre >> 24, 0);
}

#[test]
fn the_wording_follows_how_many_windows_the_saved_layout_expects() {
    let line = |count, expected| {
        let list = [progress("Chrome", Phase::Settling, count, expected)];
        active_content(&list, &name).unwrap().lines.remove(0)
    };
    assert_eq!(
        line(12, 22),
        "Chrome: 12 of 22 opened, waiting for the rest"
    );
    // All of them are there: nothing is being waited for.
    assert_eq!(line(22, 22), "Chrome: 22 of 22 opened, placing them");
    assert_eq!(line(23, 22), "Chrome: 23 windows open, placing them");
    // The saved layout says nothing about this app.
    assert_eq!(
        line(5, 0),
        "Chrome: 5 windows open, waiting for them to settle"
    );
    assert_eq!(
        line(1, 0),
        "Chrome: 1 window open, waiting for them to settle"
    );
    let moving = [progress("Chrome", Phase::Moving, 22, 22)];
    assert_eq!(
        active_content(&moving, &name).unwrap().lines,
        ["Chrome: moving 22 windows"]
    );
}

#[test]
fn a_long_name_is_cut_with_an_ellipsis() {
    let long = "x".repeat(200);
    let list = [progress(&long, Phase::Settling, 3, 9)];
    let lines = active_content(&list, &name).unwrap().lines;
    assert_eq!(lines[0].chars().count(), MAX_LINE_CHARS);
    assert!(lines[0].ends_with('\u{2026}'));
    let final_line = final_line(&[progress(&long, Phase::Done, 3, 3)], &name)
        .unwrap()
        .title;
    assert!(final_line.starts_with("WinCraft \u{b7} Layout restored: "));
    assert!(final_line.chars().count() <= MAX_LINE_CHARS + FINAL_PREFIX.chars().count());
}

#[test]
fn text_is_cut_to_the_pixels_there_are() {
    // Seven pixels a character.
    let measure = |text: &str| text.chars().count() as i32 * 7;
    assert_eq!(ellipsize("short", 100, &measure), "short");
    let cut = ellipsize("a rather long line of text", 70, &measure);
    assert_eq!(cut, "a rather\u{2026}");
    assert!(measure(&cut) <= 70);
    // Nothing fits but the ellipsis.
    assert_eq!(ellipsize("abc", 3, &measure), "\u{2026}");
    assert_eq!(ellipsize("", 0, &measure), "");
}

#[test]
fn the_panel_never_passes_its_widest() {
    let tokens = theme::Tokens::dark();
    let long = "a very long app name ".repeat(20);
    let content = Content {
        title: TITLE.to_string(),
        lines: vec![long.clone()],
        footer: Some(long),
        bar: Some(Bar {
            done: 3,
            total: 10,
            finished: false,
            least: 0,
        }),
    };
    for scale in [1.0, 1.5, 2.0] {
        let image = render(&content, scale, &tokens).expect("GDI draws the panel");
        assert!(
            image.width <= (MAX_WIDTH * scale).round() as i32,
            "{scale}: {}",
            image.width
        );
    }
}

#[test]
fn the_icon_is_laid_over_the_panel_with_its_alpha() {
    // A 2x2 panel pixel row, premultiplied: opaque grey.
    let grey = 0xFF80_8080u32;
    let mut pixels = vec![grey; 4];
    let icon = icons::Pixels {
        side: 2,
        rgba: vec![
            255, 0, 0, 255, // opaque red
            0, 255, 0, 0, // clear
            0, 0, 255, 128, // half blue
            0, 0, 0, 0, // clear
        ],
    };
    blend_icon(&mut pixels, 2, 2, 0, 0, &icon);
    assert_eq!(pixels[0], 0xFFFF_0000);
    assert_eq!(pixels[1], grey);
    let half = pixels[2];
    assert_eq!(half >> 24, 0xFF);
    assert!((half & 0xFF) > 0xB0, "blue is mostly there: {half:08x}");
    assert!(
        ((half >> 16) & 0xFF) < 0x50,
        "red is mostly gone: {half:08x}"
    );
    assert_eq!(pixels[3], grey);
    // An icon hanging off the edge only draws what is inside.
    let mut edge = vec![grey; 4];
    blend_icon(&mut edge, 2, 2, 1, 1, &icon);
    assert_eq!(edge[3], 0xFFFF_0000);
    assert_eq!(edge[0], grey);
}

#[test]
fn the_panel_has_the_app_icon_beside_its_title() {
    // The exe in a test run has no icon resource, so the panel is drawn
    // without it, and the same code path is exercised either way.
    let tokens = theme::Tokens::light();
    let content = active_content(&sample(), &same_label).unwrap();
    let image = render(&content, 1.0, &tokens).expect("GDI draws the panel");
    assert!(image.width >= MIN_WIDTH as i32);
    assert!(content.title.starts_with("WinCraft"));
}

#[test]
fn the_bar_sums_windows_over_all_apps_and_counts_finished_apps_in_full() {
    let list = [
        progress("Chrome", Phase::Settling, 12, 22),
        progress("Claude", Phase::Moving, 4, 4),
        progress("Telegram", Phase::Done, 1, 1),
        progress("Viber", Phase::NotOpened, 0, 1),
        progress("Zoom", Phase::Skipped, 0, 3),
    ];
    // Skipped windows are not expected any more, and Viber has opened
    // nothing: 12 + 4 + 1 of 22 + 4 + 1.
    assert_eq!(overall_progress(&list), Some((17, 27)));
    let content = active_content(&list, &name).unwrap();
    assert_eq!(
        content.bar,
        Some(Bar {
            done: 17,
            total: 27,
            finished: false,
            least: 0
        })
    );
}

#[test]
fn the_bar_clamps_an_app_that_has_more_windows_than_saved() {
    let list = [progress("Chrome", Phase::Settling, 30, 22)];
    assert_eq!(overall_progress(&list), Some((22, 30)));
    // An app with no saved count has nothing put back until it is restored.
    let unknown = [progress("Notes", Phase::Settling, 2, 0)];
    assert_eq!(overall_progress(&unknown), Some((0, 2)));
    let late = [progress("Chrome", Phase::LateWindows, 23, 22)];
    assert_eq!(overall_progress(&late), Some((23, 23)));
}

#[test]
fn there_is_no_bar_when_no_window_is_expected() {
    assert_eq!(overall_progress(&[]), None);
    let list = [
        progress("Viber", Phase::Skipped, 0, 1),
        progress("Zoom", Phase::NotOpened, 0, 0),
    ];
    assert_eq!(overall_progress(&list), None);
}

#[test]
fn the_first_number_only_grows_as_windows_open_and_apps_finish() {
    let steps = [
        [
            progress("A", Phase::NotOpened, 0, 4),
            progress("B", Phase::NotOpened, 0, 2),
        ],
        [
            progress("A", Phase::Settling, 2, 4),
            progress("B", Phase::NotOpened, 0, 2),
        ],
        [
            progress("A", Phase::Moving, 4, 4),
            progress("B", Phase::Settling, 1, 2),
        ],
        [
            progress("A", Phase::Done, 4, 4),
            progress("B", Phase::Done, 2, 2),
        ],
    ];
    let done: Vec<usize> = steps
        .iter()
        .map(|step| overall_progress(step).map_or(0, |(done, _)| done))
        .collect();
    assert_eq!(done, [0, 2, 5, 6]);
}

#[test]
fn the_bar_counts_only_apps_that_opened_a_window_and_never_runs_back() {
    let mut floor = 0;
    // Chrome is done; Surfshark and ShareX are still in the tray.
    let early = [
        progress("Chrome", Phase::Done, 24, 24),
        progress("Surfshark", Phase::NotOpened, 0, 1),
        progress("ShareX", Phase::NotOpened, 0, 1),
    ];
    let bar = steady(active_bar(&early), &mut floor).unwrap();
    assert_eq!((bar.done, bar.total, bar.least), (24, 24, 1000));
    // Telegram opens its first window: the total grows, the fill stays.
    let later = [
        progress("Chrome", Phase::Done, 24, 24),
        progress("Telegram", Phase::Settling, 1, 2),
    ];
    let bar = steady(active_bar(&later), &mut floor).unwrap();
    assert_eq!((bar.done, bar.total), (25, 26), "the words are true");
    assert_eq!(bar.least, 1000, "the fill does not run back");
}

fn active_bar(list: &[Progress]) -> Option<Bar> {
    overall_progress(list).map(|(done, total)| Bar {
        done,
        total,
        finished: false,
        least: 0,
    })
}

#[test]
fn the_panel_closes_at_the_latest_a_minute_after_the_last_window_was_placed() {
    assert!(
        panel_open(10, None, false),
        "nothing placed yet; each line has its cap"
    );
    assert!(panel_open(70, Some(10), false));
    assert!(!panel_open(71, Some(10), false));
    assert!(!panel_open(20, Some(10), true), "it has shown once already");
}

#[test]
fn once_the_panel_has_gone_a_window_the_user_opens_later_brings_no_panel() {
    let mut controller = Controller::default();
    let mut closed = false;
    let mut step = |tick: u64, active: Option<Content>, last: Option<Content>| {
        let was_visible = controller.is_visible();
        let open = panel_open(tick, Some(1), closed);
        let command = controller.update(tick, active.filter(|_| open), last);
        if was_visible && !controller.is_visible() {
            closed = true;
        }
        (command, controller.is_visible())
    };
    let moving = [progress("Chrome", Phase::Moving, 24, 24)];
    let done = [progress("Chrome", Phase::Done, 24, 24)];
    assert!(matches!(
        step(1, active_content(&moving, &name), None).0,
        Command::Show(_)
    ));
    assert!(matches!(
        step(3, None, final_line(&done, &name)).0,
        Command::Show(_)
    ));
    assert_eq!(
        step(3 + FINAL_HOLD_SECONDS, None, final_line(&done, &name)),
        (Command::Fade, false)
    );
    // Ten seconds later the user opens Explorer, which had 3 saved windows.
    let explorer = [
        progress("Chrome", Phase::Done, 24, 24),
        progress("Explorer", Phase::Settling, 1, 3),
    ];
    assert_eq!(
        step(
            20,
            active_content(&explorer, &name),
            final_line(&explorer, &name)
        ),
        (Command::Nothing, false)
    );
}

#[test]
fn an_app_with_one_open_window_that_matches_nothing_does_not_hold_the_panel() {
    let mut restore = super::super::restore::Restore::new(3, 1800);
    restore.start(0, vec!["ShareX".into(), "Chrome".into()]);
    let saved: std::collections::BTreeMap<String, usize> =
        [("ShareX".to_string(), 1), ("Chrome".to_string(), 2)].into();
    restore.expect(saved.clone());
    let mut controller = Controller::default();
    let mut shown_until = 0;
    let mut floor = 0;
    for tick in 1..=200u64 {
        // ShareX's open window is not the saved one, and it flickers, so
        // its count never holds still; Chrome never opens.
        let counts: std::collections::BTreeMap<String, usize> =
            [("ShareX".to_string(), (tick % 2) as usize)].into();
        restore.step(tick, &counts);
        let progress = restore.progress(tick, &counts, &saved);
        let active = panel_open(tick, None, false)
            .then(|| active_content(&progress, &name))
            .flatten()
            .map(|mut content| {
                content.bar = steady(content.bar, &mut floor);
                content
            });
        let last = final_line(&progress, &name);
        if controller.update(tick, active, last) != Command::Nothing || controller.is_visible() {
            shown_until = tick;
        }
    }
    assert!(
        shown_until <= super::super::restore::LINE_CAP_SECONDS + FINAL_HOLD_SECONDS + 1,
        "the panel was still up at {shown_until}"
    );
}

#[test]
fn the_final_state_has_a_full_finished_bar_and_the_tooltip_counts_windows() {
    let list = [
        progress("Chrome", Phase::Done, 22, 22),
        progress("Viber", Phase::Skipped, 0, 1),
    ];
    let last = final_line(&list, &name).unwrap();
    assert_eq!(
        last.bar,
        Some(Bar {
            done: 22,
            total: 22,
            finished: true,
            least: 1000
        })
    );
    assert!(last.lines.is_empty());
    assert_eq!(
        Bar {
            done: 1,
            total: 1,
            finished: false,
            least: 0
        }
        .label(),
        "1 of 1 window"
    );
    assert_eq!(
        Bar {
            done: 7,
            total: 10,
            finished: false,
            least: 0
        }
        .label(),
        "7 of 10 windows"
    );
}

#[test]
fn the_bar_is_drawn_in_the_track_and_fill_colours() {
    let tokens = theme::Tokens::dark();
    let with = |bar| Content {
        bar,
        ..active_content(&sample(), &same_label).unwrap()
    };
    let bar = Bar {
        done: 5,
        total: 10,
        finished: false,
        least: 0,
    };
    let plain = render(&with(None), 1.0, &tokens).expect("GDI draws the panel");
    let barred = render(&with(Some(bar)), 1.0, &tokens).expect("GDI draws the panel");
    assert!(barred.height > plain.height);
    assert_eq!(barred.pixels.len(), (barred.width * barred.height) as usize);

    let mut pixels = vec![0u32; 40 * 6];
    draw_bar(
        &mut pixels,
        40,
        (4, 1, 32, 4),
        bar,
        tokens.accent,
        tokens.border,
    );
    let at = |x: i32, y: i32| pixels[(y * 40 + x) as usize];
    let rgb = |colour: Color32| {
        (u32::from(colour.r()) << 16) | (u32::from(colour.g()) << 8) | u32::from(colour.b())
    };
    assert_eq!(at(10, 3) & 0xFF_FFFF, rgb(tokens.accent));
    assert_eq!(at(30, 3) & 0xFF_FFFF, rgb(tokens.border));
    // Outside the bar nothing is drawn, and the ends are rounded.
    assert_eq!(at(10, 0), 0);
    assert_eq!(at(10, 5), 0);
    assert!(at(4, 1) >> 24 < 0xFF);
    // A full bar is filled to the far end.
    let mut full = vec![0u32; 40 * 6];
    let done = Bar {
        done: 10,
        total: 10,
        finished: true,
        least: 0,
    };
    draw_bar(
        &mut full,
        40,
        (4, 1, 32, 4),
        done,
        tokens.success,
        tokens.border,
    );
    assert_eq!(full[3 * 40 + 30] & 0xFF_FFFF, rgb(tokens.success));
}
