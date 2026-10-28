use std::sync::Arc;

use crate::core::theme::{self, Tokens};
use crate::core::traits::FieldKind;
use crate::core::ui_bridge::{
    HostChannel, HostRequest, HostSetting, NewRelease, UiSnapshot, UpdateStatus, UpdateView,
};
use crate::core::{clock, update};
use crate::store::github;
use crate::ui::settings::{self as page, Readme, SettingsState};
use crate::ui::widgets::button::{self, Kind};
use crate::ui::widgets::row::{settings_row, settings_row_ruled, RowText};
use crate::ui::widgets::{text, toggle};

/// The release notes of a newer version, drawn like a plugin's README.
#[derive(Default)]
pub struct Update {
    notes: Readme,
    /// The notes of the installed version.
    installed: Readme,
}

pub fn show(
    ui: &mut egui::Ui,
    snapshot: &UiSnapshot,
    state: &mut SettingsState,
    to_host: &Arc<HostChannel>,
) {
    let tokens = Tokens::get(ui.ctx());

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            page::page_header(
                ui,
                "About",
                None,
                "WinCraft is open source under the MIT licence.",
                |_| {},
            );
            ui.add_space(16.0);
            ui.spacing_mut().item_spacing.y = 0.0;

            let version = format!(
                "{} \u{00B7} built {}",
                snapshot.version,
                env!("WINCRAFT_BUILD_DATE")
            );
            settings_row(ui, "version", RowText::new("Version"), false, |ui| {
                text::mono(ui, &version, 13.0, tokens.text_secondary);
            });

            let update = &snapshot.update;
            updates_block(ui, update, &mut state.update, to_host);

            settings_row(
                ui,
                "check-daily",
                RowText::new("Check for updates automatically").desc(
                    "Asks GitHub every hour and tells you. Nothing installs until you click Update now.",
                    tokens.text_secondary,
                ),
                false,
                |ui| {
                    let mut on = update.check_daily;
                    if toggle::toggle(ui, &mut on).changed() {
                        to_host.send(HostRequest::SetHostSetting(HostSetting::CheckUpdatesDaily(
                            on,
                        )));
                    }
                },
            );

            settings_row(ui, "links", RowText::new("Links"), false, |ui| {
                if button::button(ui, "Repository", Kind::Secondary).clicked() {
                    ui.ctx()
                        .open_url(egui::OpenUrl::new_tab(github::repo_url()));
                }
                if button::button(ui, "Report an issue", Kind::Secondary).clicked() {
                    ui.ctx()
                        .open_url(egui::OpenUrl::new_tab(github::issues_url()));
                }
            });

            settings_row(
                ui,
                "fonts",
                RowText::new("Fonts").desc(
                    "Segoe UI from Windows. Bundled: Selawik and JetBrains Mono, \
                     both under the SIL Open Font License 1.1.",
                    tokens.text_secondary,
                ),
                false,
                |_| {},
            );

            if let Some(notes) = &update.installed_notes {
                page::section_header(ui, &format!("What's new in {}", snapshot.version));
                ui.spacing_mut().item_spacing.y = 8.0;
                state.update.installed.boxed(ui, notes);
                ui.spacing_mut().item_spacing.y = 0.0;
            }

            ui.add_space(20.0);
            text::group_header(ui, "For plugin authors");
            ui.add_space(6.0);
            authors_note(ui);
        });
}

/// Which colour a status line takes; the tokens are looked up when drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    Accent,
    Secondary,
    Disabled,
    Warning,
}

impl Tone {
    fn colour(self, tokens: &Tokens) -> egui::Color32 {
        match self {
            Tone::Accent => tokens.accent,
            Tone::Secondary => tokens.text_secondary,
            Tone::Disabled => tokens.text_disabled,
            Tone::Warning => tokens.warning,
        }
    }
}

/// "today, 09:14" for "Today, 09:14": the day word reads inside a sentence.
fn checked_phrase(when: &str) -> String {
    for day in ["Today", "Yesterday"] {
        if let Some(rest) = when.strip_prefix(day) {
            return format!("{}{rest}", day.to_lowercase());
        }
    }
    when.to_string()
}

/// The line under "Updates": what the updater is doing or found. `checked`
/// is the time of the last answer, as `clock::when` writes it; empty when
/// there was none.
fn status_line(update: &UpdateView, checked: &str) -> (String, Tone) {
    let up_to_date = || {
        if checked.is_empty() {
            ("Not checked yet".to_string(), Tone::Disabled)
        } else {
            (
                format!("Up to date \u{00B7} checked {}", checked_phrase(checked)),
                Tone::Secondary,
            )
        }
    };
    let tag = update
        .available
        .as_ref()
        .map(|release| release.tag.as_str());
    match &update.status {
        UpdateStatus::Idle => match tag {
            Some(tag) => (format!("{tag} available"), Tone::Accent),
            None => up_to_date(),
        },
        UpdateStatus::UpToDate => up_to_date(),
        UpdateStatus::Checking => ("Checking\u{2026}".to_string(), Tone::Disabled),
        UpdateStatus::NoRelease => ("No release published yet".to_string(), Tone::Secondary),
        UpdateStatus::Offline => (
            "Could not check \u{2014} offline".to_string(),
            Tone::Warning,
        ),
        UpdateStatus::Downloading(Some(100)) => {
            ("Checking the download\u{2026}".to_string(), Tone::Secondary)
        }
        UpdateStatus::Downloading(_) => (
            match tag {
                Some(tag) => format!("Downloading {tag} \u{00B7} WinCraft restarts when done"),
                None => "Downloading\u{2026} WinCraft restarts when done".to_string(),
            },
            Tone::Secondary,
        ),
        UpdateStatus::Installing => (
            "Installing \u{2014} WinCraft restarts".to_string(),
            Tone::Secondary,
        ),
        UpdateStatus::Error(_) => ("Update failed".to_string(), Tone::Warning),
    }
}

/// What the one button of the Updates row does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Check,
    UpdateNow,
    Cancel,
}

/// What stands on the right of the Updates row, in this order: the link, the
/// progress bar, the button.
#[derive(Clone, Debug, PartialEq)]
struct Controls {
    /// "Release page", while a newer version waits.
    release_link: bool,
    /// A bar with its fill, None for the sliding bar of an unknown size.
    bar: Option<Option<f32>>,
    /// The label, kind and job of the button; none while the restart is
    /// under way and while the finished download is checked.
    button: Option<(&'static str, Kind, Action)>,
    /// The button cannot be pressed (a check is running).
    disabled: bool,
}

fn controls_for(update: &UpdateView) -> Controls {
    let bar = progress_step(&update.status).map(|(_, fraction)| fraction);
    let waiting = update.available.is_some();
    let button = match &update.status {
        UpdateStatus::Installing | UpdateStatus::Downloading(Some(100)) => None,
        UpdateStatus::Downloading(_) => Some(("Cancel", Kind::Secondary, Action::Cancel)),
        _ if waiting && update.blocked.is_none() => {
            Some(("Update now", Kind::Primary, Action::UpdateNow))
        }
        _ => Some(("Check for updates", Kind::Secondary, Action::Check)),
    };
    Controls {
        release_link: waiting && bar.is_none(),
        bar,
        button,
        disabled: update.status == UpdateStatus::Checking,
    }
}

/// The Updates row and, under it, the newer version's notes: one block with
/// one rule under it.
fn updates_block(
    ui: &mut egui::Ui,
    update: &UpdateView,
    notes: &mut Update,
    to_host: &Arc<HostChannel>,
) {
    let tokens = Tokens::get(ui.ctx());
    let checked = match update.last_check {
        0 => String::new(),
        seconds => clock::when(seconds as i64, update::now_secs() as i64),
    };
    let (status, tone) = status_line(update, &checked);
    let controls = controls_for(update);
    let show_notes = update.available.is_some();
    settings_row_ruled(
        ui,
        "updates",
        RowText::new("Updates").desc(&status, tone.colour(&tokens)),
        false,
        !show_notes,
        |ui| {
            if controls.release_link {
                if let Some(release) = &update.available {
                    if text::link(ui, "Release page").clicked() {
                        ui.ctx()
                            .open_url(egui::OpenUrl::new_tab(release.page.clone()));
                    }
                    ui.add_space(8.0);
                }
            }
            if let Some(fraction) = controls.bar {
                progress_bar(ui, fraction);
                ui.add_space(8.0);
            }
            if let Some((label, kind, action)) = controls.button {
                let clicked = ui
                    .add_enabled_ui(!controls.disabled, |ui| button::button(ui, label, kind))
                    .inner
                    .clicked();
                if clicked {
                    to_host.send(match action {
                        Action::Check => HostRequest::CheckForUpdates,
                        Action::UpdateNow => HostRequest::UpdateNow,
                        Action::Cancel => HostRequest::CancelUpdate,
                    });
                }
            }
        },
    );
    if let Some(failed) = &update.failed {
        ui.add_space(2.0);
        text::secondary(ui, failed, tokens.warning);
        ui.add_space(8.0);
    }
    if let UpdateStatus::Error(err) = &update.status {
        ui.add_space(2.0);
        text::secondary(ui, err, tokens.warning);
        ui.add_space(8.0);
    }
    if let Some(release) = &update.available {
        ui.add_space(4.0);
        notes_box(ui, release, update, notes);
        ui.add_space(16.0);
        let line = theme::line(ui.ctx(), tokens.border);
        let y = ui.cursor().top() - line.width / 2.0;
        ui.painter().hline(ui.max_rect().x_range(), y, line);
    }
}

/// "What's new in 0.20.32", the release notes and what Update now does. A
/// copy that cannot update itself says why instead.
fn notes_box(ui: &mut egui::Ui, release: &NewRelease, view: &UpdateView, update: &mut Update) {
    let tokens = Tokens::get(ui.ctx());
    let version = release.tag.trim_start_matches(['v', 'V']);
    let (hint, colour) = match &view.blocked {
        Some(reason) => (reason.as_str(), tokens.warning),
        None => (
            "Downloads the new version, checks it and restarts WinCraft. \
             If it does not start properly, this version comes back.",
            tokens.text_disabled,
        ),
    };
    egui::Frame::NONE
        .stroke(theme::line(ui.ctx(), tokens.border))
        .corner_radius(tokens.radius_row)
        .inner_margin(egui::Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            text::caption(
                ui,
                &format!("What's new in {version}"),
                tokens.text_disabled,
            );
            if !release.notes.trim().is_empty() {
                update.notes.show(ui, &release.notes);
            }
            text::caption(ui, hint, colour);
        });
}

/// The line above the update's progress bar and how full the bar is: None
/// for no update under way, and a None fraction when the server gave no size.
fn progress_step(status: &UpdateStatus) -> Option<(String, Option<f32>)> {
    match status {
        // The checksum is checked once the last byte is in.
        UpdateStatus::Downloading(Some(100)) => {
            Some(("Checking the download\u{2026}".to_string(), Some(1.0)))
        }
        UpdateStatus::Downloading(Some(percent)) => Some((
            format!("Step 1 of 2 \u{00B7} Downloading\u{2026} {percent}%"),
            Some(f32::from(*percent) / 100.0),
        )),
        UpdateStatus::Downloading(None) => {
            Some(("Step 1 of 2 \u{00B7} Downloading\u{2026}".to_string(), None))
        }
        UpdateStatus::Installing => Some((
            "Step 2 of 2 \u{00B7} Installing \u{2014} WinCraft restarts in a moment".to_string(),
            Some(1.0),
        )),
        _ => None,
    }
}

/// A 160 x 4 px bar: accent fill on a border-coloured track. With no size
/// known, a short segment slides along so it still looks alive.
fn progress_bar(ui: &mut egui::Ui, fraction: Option<f32>) {
    let tokens = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(160.0, 4.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, tokens.border);
    let fill = match fraction {
        Some(fraction) => {
            let mut fill = rect;
            fill.set_width(rect.width() * fraction.clamp(0.0, 1.0));
            fill
        }
        None => {
            let time = ui.input(|input| input.time) as f32;
            let width = rect.width() * 0.25;
            let left = rect.left() + (time * 0.6).fract() * (rect.width() + width) - width;
            ui.ctx().request_repaint();
            egui::Rect::from_x_y_ranges(left..=left + width, rect.y_range()).intersect(rect)
        }
    };
    if fill.width() > 0.0 {
        ui.painter().rect_filled(fill, 2.0, tokens.accent);
    }
}

fn authors_note(ui: &mut egui::Ui) {
    let tokens = Tokens::get(ui.ctx());
    let kinds = FieldKind::ALL
        .iter()
        .map(FieldKind::name)
        .collect::<Vec<_>>()
        .join(" \u{00B7} ");
    let body = || text::format(theme::regular(13.0), tokens.text_secondary, Some(19.0));
    let mut job = egui::text::LayoutJob::default();
    job.append("Declare fields of kind ", 0.0, body());
    job.append(
        &kinds,
        0.0,
        text::format(theme::mono(12.0), tokens.text_primary, Some(19.0)),
    );
    job.append(
        "; the settings page draws and saves them for you. See ",
        0.0,
        body(),
    );
    job.append(
        "src/plugins/<id>/README.md",
        0.0,
        text::format(theme::regular(13.0), tokens.accent, Some(19.0)),
    );
    job.append(".", 0.0, body());
    let width = ui.available_width().min(560.0);
    ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
        text::wrapped(ui, job);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_page_no_longer_lists_hotkeys() {
        use crate::core::config::{ThemeChoice, ThemeName};
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(&ctx, ThemeName::Classic, ThemeChoice::Dark);
        let bridge = crate::core::ui_bridge::create();
        let snapshot = UiSnapshot {
            palette_hotkey: "Win+Alt+P".to_string(),
            ..UiSnapshot::default()
        };
        let mut state = SettingsState::default();
        let mut words = String::new();
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::vec2(800.0, 900.0),
                )),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                show(ui, &snapshot, &mut state, &bridge.to_host);
            });
            output.textures_delta.clear();
            words.clear();
            for clipped in &output.shapes {
                if let egui::Shape::Text(shape) = &clipped.shape {
                    words.push_str(shape.galley.text());
                    words.push(' ');
                }
            }
        }
        assert!(words.contains("About"), "the page drew: {words}");
        assert!(!words.contains("Hotkeys"), "{words}");
        assert!(!words.contains("Open the palette"), "{words}");
    }

    #[test]
    fn the_progress_bar_follows_the_update_steps() {
        assert_eq!(progress_step(&UpdateStatus::Idle), None);
        assert_eq!(progress_step(&UpdateStatus::Checking), None);
        assert_eq!(progress_step(&UpdateStatus::Error("x".into())), None);
        let (label, fraction) = progress_step(&UpdateStatus::Downloading(Some(35))).unwrap();
        assert!(
            label.starts_with("Step 1 of 2") && label.ends_with("35%"),
            "{label}"
        );
        assert_eq!(fraction, Some(0.35));
        let (_, fraction) = progress_step(&UpdateStatus::Downloading(None)).unwrap();
        assert_eq!(fraction, None);
        let (label, fraction) = progress_step(&UpdateStatus::Downloading(Some(100))).unwrap();
        assert!(label.starts_with("Checking the download"), "{label}");
        assert_eq!(fraction, Some(1.0));
        let (label, _) = progress_step(&UpdateStatus::Installing).unwrap();
        assert!(label.starts_with("Step 2 of 2"), "{label}");
    }

    fn newer(blocked: Option<&str>, status: UpdateStatus) -> UpdateView {
        UpdateView {
            status,
            available: Some(NewRelease {
                tag: "v0.20.32".to_string(),
                notes: "- Faster".to_string(),
                page: "https://example.test/release".to_string(),
            }),
            blocked: blocked.map(str::to_string),
            ..UpdateView::default()
        }
    }

    #[test]
    fn the_button_follows_the_update_state() {
        let button = |view: &UpdateView| controls_for(view).button.map(|b| (b.0, b.1, b.2));
        // Up to date, idle, offline, no release, failed: Check for updates.
        for status in [
            UpdateStatus::Idle,
            UpdateStatus::UpToDate,
            UpdateStatus::Offline,
            UpdateStatus::NoRelease,
            UpdateStatus::Error("x".into()),
        ] {
            let view = UpdateView {
                status,
                ..UpdateView::default()
            };
            let controls = controls_for(&view);
            assert_eq!(
                button(&view),
                Some(("Check for updates", Kind::Secondary, Action::Check))
            );
            assert!(!controls.release_link && controls.bar.is_none() && !controls.disabled);
        }
        // Checking: the same button, but it cannot be pressed.
        let checking = UpdateView {
            status: UpdateStatus::Checking,
            ..UpdateView::default()
        };
        assert!(controls_for(&checking).disabled);

        // A newer version: Update now with accent, and the link.
        let waiting = newer(None, UpdateStatus::Idle);
        let controls = controls_for(&waiting);
        assert_eq!(
            button(&waiting),
            Some(("Update now", Kind::Primary, Action::UpdateNow))
        );
        assert!(controls.release_link && controls.bar.is_none());
        // A failed try can be repeated.
        assert_eq!(
            button(&newer(None, UpdateStatus::Error("x".into()))).map(|b| b.2),
            Some(Action::UpdateNow)
        );
        // This copy cannot update itself: no Update now, a check instead.
        assert_eq!(
            button(&newer(Some("a development build"), UpdateStatus::Idle)).map(|b| b.2),
            Some(Action::Check)
        );

        // Downloading: Cancel and a bar, no link.
        let downloading = newer(None, UpdateStatus::Downloading(Some(35)));
        let controls = controls_for(&downloading);
        assert_eq!(
            button(&downloading),
            Some(("Cancel", Kind::Secondary, Action::Cancel))
        );
        assert_eq!(controls.bar, Some(Some(0.35)));
        assert!(!controls.release_link);
        let unknown = controls_for(&newer(None, UpdateStatus::Downloading(None)));
        assert_eq!(unknown.bar, Some(None));
        assert_eq!(unknown.button.map(|b| b.2), Some(Action::Cancel));

        // The finished download is being checked, then the restart: no button.
        for status in [
            UpdateStatus::Downloading(Some(100)),
            UpdateStatus::Installing,
        ] {
            let controls = controls_for(&newer(None, status));
            assert_eq!(controls.button, None);
            assert_eq!(controls.bar, Some(Some(1.0)));
        }
    }

    #[test]
    fn the_status_line_says_what_the_updater_found() {
        let line = |view: &UpdateView, checked: &str| status_line(view, checked);
        let plain = |status| UpdateView {
            status,
            ..UpdateView::default()
        };
        assert_eq!(
            line(&plain(UpdateStatus::UpToDate), "Today, 09:14"),
            (
                "Up to date \u{00B7} checked today, 09:14".to_string(),
                Tone::Secondary
            )
        );
        assert_eq!(
            line(&plain(UpdateStatus::Idle), "Yesterday, 18:40").0,
            "Up to date \u{00B7} checked yesterday, 18:40"
        );
        assert_eq!(
            line(&plain(UpdateStatus::UpToDate), "21 Sep, 09:12").0,
            "Up to date \u{00B7} checked 21 Sep, 09:12"
        );
        assert_eq!(
            line(&plain(UpdateStatus::Idle), ""),
            ("Not checked yet".to_string(), Tone::Disabled)
        );
        assert_eq!(
            line(&newer(None, UpdateStatus::Idle), "Today, 09:14"),
            ("v0.20.32 available".to_string(), Tone::Accent)
        );
        assert_eq!(
            line(&newer(None, UpdateStatus::Downloading(Some(5))), "").0,
            "Downloading v0.20.32 \u{00B7} WinCraft restarts when done"
        );
        assert_eq!(
            line(&newer(None, UpdateStatus::Downloading(Some(100))), "").0,
            "Checking the download\u{2026}"
        );
        assert_eq!(
            line(&newer(None, UpdateStatus::Installing), "").0,
            "Installing \u{2014} WinCraft restarts"
        );
        assert_eq!(
            line(&plain(UpdateStatus::Checking), ""),
            ("Checking\u{2026}".to_string(), Tone::Disabled)
        );
        assert_eq!(
            line(&plain(UpdateStatus::NoRelease), "").0,
            "No release published yet"
        );
        assert_eq!(
            line(&plain(UpdateStatus::Offline), ""),
            (
                "Could not check \u{2014} offline".to_string(),
                Tone::Warning
            )
        );
        assert_eq!(
            line(&plain(UpdateStatus::Error("x".into())), ""),
            ("Update failed".to_string(), Tone::Warning)
        );
    }

    #[test]
    fn the_checked_time_reads_inside_a_sentence() {
        use crate::core::clock::{day_and_time, LocalTime};
        let at = |day, hour, minute| LocalTime {
            year: 2026,
            month: 10,
            day,
            hour,
            minute,
        };
        let (today, yesterday) = (at(27, 12, 0), at(26, 12, 0));
        assert_eq!(
            checked_phrase(&day_and_time(at(27, 9, 14), today, yesterday)),
            "today, 09:14"
        );
        assert_eq!(
            checked_phrase(&day_and_time(at(26, 18, 40), today, yesterday)),
            "yesterday, 18:40"
        );
        assert_eq!(
            checked_phrase(&day_and_time(at(3, 7, 5), today, yesterday)),
            "3 Oct, 07:05"
        );
    }

    /// Draws the page a few frames so the rows have measured themselves, and
    /// returns every text with its box.
    fn draw(snapshot: &UiSnapshot, width: f32) -> Vec<(String, egui::Rect)> {
        use crate::core::config::{ThemeChoice, ThemeName};
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(&ctx, ThemeName::Classic, ThemeChoice::Dark);
        let bridge = crate::core::ui_bridge::create();
        let mut state = SettingsState::default();
        let mut found = Vec::new();
        for _ in 0..4 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::vec2(width, 1000.0),
                )),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                show(ui, snapshot, &mut state, &bridge.to_host);
            });
            output.textures_delta.clear();
            found.clear();
            for clipped in &output.shapes {
                if let egui::Shape::Text(shape) = &clipped.shape {
                    found.push((
                        shape.galley.text().to_string(),
                        shape.visual_bounding_rect(),
                    ));
                }
            }
        }
        found
    }

    #[test]
    fn buttons_the_link_and_the_version_stay_on_one_line() {
        let snapshot = UiSnapshot {
            version: "0.20.32".to_string(),
            update: newer(None, UpdateStatus::Idle),
            ..UiSnapshot::default()
        };
        for width in [560.0, 760.0, 1000.0] {
            let texts = draw(&snapshot, width);
            for word in [
                "Release page",
                "Update now",
                "Repository",
                "Report an issue",
            ] {
                let (_, rect) = texts
                    .iter()
                    .find(|(text, _)| text == word)
                    .unwrap_or_else(|| panic!("{word} was drawn at {width}"));
                assert!(rect.height() < 22.0, "{word} wrapped at {width}: {rect:?}");
            }
            let (_, rect) = texts
                .iter()
                .find(|(text, _)| text.starts_with("0.20.32"))
                .expect("the version line");
            assert!(rect.height() < 22.0, "the version wrapped at {width}");
        }
    }

    #[test]
    fn the_updates_block_follows_a_newer_release() {
        let words = |snapshot: &UiSnapshot| {
            draw(snapshot, 800.0)
                .into_iter()
                .map(|(text, _)| text)
                .collect::<Vec<_>>()
        };
        let has = |drawn: &[String], word: &str| drawn.iter().any(|text| text == word);
        let waiting = words(&UiSnapshot {
            update: newer(None, UpdateStatus::Idle),
            ..UiSnapshot::default()
        });
        assert!(has(&waiting, "v0.20.32 available"), "{waiting:?}");
        assert!(has(&waiting, "Update now"), "{waiting:?}");
        assert!(has(&waiting, "What's new in 0.20.32"), "{waiting:?}");
        assert!(
            waiting
                .iter()
                .any(|t| t.starts_with("Downloads the new version")),
            "{waiting:?}"
        );
        assert!(
            !waiting.iter().any(|t| t.ends_with("is available")),
            "no separate section"
        );

        let blocked = words(&UiSnapshot {
            update: newer(Some("a development build"), UpdateStatus::Idle),
            ..UiSnapshot::default()
        });
        assert!(has(&blocked, "a development build"), "{blocked:?}");
        assert!(!has(&blocked, "Update now"), "{blocked:?}");

        let busy = words(&UiSnapshot {
            update: newer(None, UpdateStatus::Downloading(Some(40))),
            ..UiSnapshot::default()
        });
        assert!(has(&busy, "Cancel"), "{busy:?}");
        assert!(!has(&busy, "Release page"), "{busy:?}");

        let quiet = words(&UiSnapshot::default());
        assert!(has(&quiet, "Check for updates"), "{quiet:?}");
        assert!(
            !quiet.iter().any(|t| t.starts_with("What's new in")),
            "{quiet:?}"
        );
    }
}
