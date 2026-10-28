use super::*;

pub(super) enum UpdateEvent {
    Checked(Result<Checked, String>),
    /// Percent downloaded, when the server said how much there is.
    Progress(Option<u8>),
    Downloaded(Result<std::path::PathBuf, String>),
    /// The release notes of the installed version, fetched after an update.
    InstalledNotes(String),
    /// A download ahead of this release tag ended.
    Staged(String, Result<(), String>),
}

pub(super) struct Updates {
    pub(super) checking: bool,
    /// The check running now was asked for in About, so its failure is
    /// shown; an automatic check stays silent when offline.
    pub(super) manual: bool,
    /// When the last check started, in unix seconds; 0 before the first.
    pub(super) last_try: u64,
    pub(super) status: UpdateStatus,
    pub(super) newer: Option<Release>,
    /// Downloading or swapping; a second Update now waits for it.
    pub(super) busy: bool,
    /// A download ahead runs on its own thread.
    pub(super) staging: bool,
    /// The release tag downloaded ahead and kept, when there is one.
    pub(super) staged: Option<String>,
    /// Where a download ahead is kept: the installed copy's own folder, set
    /// at start only in a copy that can update itself. None in every other
    /// copy and in the tests, so they never touch a real install folder.
    pub(super) stage_dir: Option<std::path::PathBuf>,
    /// About's Cancel was clicked: the download stops and its file is deleted.
    pub(super) cancel: Arc<AtomicBool>,
    /// Why this copy cannot update itself, worked out once at start.
    pub(super) blocked: Option<String>,
    /// What went wrong with the update a rollback came back from.
    pub(super) failed: Option<String>,
    /// The version that started this one and waits to hear it is healthy.
    pub(super) healthy_pending: Option<String>,
    /// Update as soon as the check finds a newer release: About's Update
    /// now, the palette's "Update WinCraft" or a test scene's `--update-now`.
    pub(super) auto_update: bool,
    /// The palette's "Update WinCraft" started this check, so its answer is
    /// said in a notice whatever it is.
    pub(super) report: bool,
    /// What About shows under "What's new": the notes of this version.
    pub(super) installed_notes: Option<String>,
    /// Where the user started the update that is under way, for the next
    /// start to open again. None for the tray menu and for unattended ones.
    pub(super) reopen: Option<update::Reopen>,
    pub(super) tx: Sender<UpdateEvent>,
    pub(super) rx: Receiver<UpdateEvent>,
}

impl Default for Updates {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            checking: false,
            manual: false,
            last_try: 0,
            status: UpdateStatus::Idle,
            newer: None,
            busy: false,
            staging: false,
            staged: None,
            stage_dir: None,
            cancel: Arc::new(AtomicBool::new(false)),
            blocked: None,
            failed: None,
            healthy_pending: None,
            auto_update: false,
            report: false,
            installed_notes: None,
            reopen: None,
            tx,
            rx,
        }
    }
}

/// What the host does after an update check, decided by `Updates::checked`.
#[derive(Debug, Default, PartialEq)]
pub(super) struct AfterCheck {
    /// GitHub answered; the config remembers when, and what it said.
    pub(super) answered: bool,
    /// The balloon that says a newer version is there, once per new tag.
    pub(super) notice: Option<String>,
    /// Read this version's notes again: the newest release is this one.
    pub(super) reload_notes: bool,
    /// Install the newer version now: Update now started this check.
    pub(super) update_now: bool,
}

impl Updates {
    pub(super) fn checked(
        &mut self,
        result: Result<Checked, String>,
        config: &mut UpdateConfig,
        now: u64,
    ) -> AfterCheck {
        self.checking = false;
        let manual = std::mem::take(&mut self.manual);
        let auto_update = std::mem::take(&mut self.auto_update);
        let report = std::mem::take(&mut self.report);
        let mut after = AfterCheck::default();
        let checked = match result {
            Ok(checked) => checked,
            Err(err) => {
                log::info!("update check failed: {err}");
                if manual {
                    self.status = UpdateStatus::Offline;
                }
                if report {
                    after.notice =
                        Some("WinCraft could not check for updates. Is the PC online?".to_string());
                }
                return after;
            }
        };
        after.answered = true;
        config.last_check = now;
        match checked {
            Checked::NoRelease => {
                log::info!("update check: no release is published yet");
                self.status = UpdateStatus::NoRelease;
                self.newer = None;
                if report {
                    after.notice = Some("No WinCraft release is published yet".to_string());
                }
            }
            Checked::UpToDate(tag) => {
                log::info!("update check: {tag} is the newest release, nothing to update");
                self.status = UpdateStatus::UpToDate;
                self.newer = None;
                // The check keeps the notes when the newest release is this one.
                after.reload_notes = true;
                config.latest = tag;
                if report {
                    after.notice = Some(format!(
                        "WinCraft is up to date ({})",
                        env!("CARGO_PKG_VERSION")
                    ));
                }
            }
            Checked::Newer(release) => {
                log::info!("update check: {} is available", release.tag);
                self.status = UpdateStatus::Idle;
                if config.latest != release.tag {
                    after.notice = Some(format!(
                        "WinCraft {} is available \u{2014} open About to update",
                        release.tag.trim_start_matches(['v', 'V'])
                    ));
                    config.latest = release.tag.clone();
                }
                if report && auto_update {
                    after.notice = Some(format!(
                        "Updating WinCraft to {}. It restarts when the new version is in place.",
                        release.tag.trim_start_matches(['v', 'V'])
                    ));
                }
                self.newer = Some(release);
                after.update_now = auto_update;
            }
        }
        after
    }

    /// The version waiting to hear that this one is healthy, taken so it
    /// hears once, and whether the old version's files go afterwards: not
    /// when quitting, nor from a copy that cannot update itself.
    pub(super) fn healthy_report(&mut self, quitting: bool) -> Option<(String, bool)> {
        let from = self.healthy_pending.take()?;
        Some((from, !quitting && self.blocked.is_none()))
    }
}

impl Host {
    pub(super) fn update_view(&self) -> UpdateView {
        let tag = self
            .updates
            .newer
            .as_ref()
            .map(|release| release.tag.as_str());
        UpdateView {
            check_daily: self.config.updates.check_daily,
            download_ahead: self.config.updates.download_ahead,
            downloaded: tag.is_some() && self.updates.staged.as_deref() == tag,
            hidden: tag.is_some_and(|tag| {
                super::view::update_hidden(tag, &self.config.updates.hidden_version)
            }),
            status: self.updates.status.clone(),
            last_check: self.config.updates.last_check,
            available: self.updates.newer.as_ref().map(|release| NewRelease {
                tag: release.tag.clone(),
                notes: release.notes.clone(),
                page: release.page.clone(),
            }),
            blocked: self.updates.blocked.clone(),
            failed: self.updates.failed.clone(),
            installed_notes: self.updates.installed_notes.clone(),
        }
    }

    /// What this start means for updates: whether this copy may update
    /// itself, whether an update started it, whether a rollback did.
    pub(super) fn start_updates(&mut self, flags: &StartupFlags) {
        let exe = std::env::current_exe().unwrap_or_default();
        self.updates.blocked = update::blocked(&exe, update::current_package().as_deref());
        if let Some(reason) = &self.updates.blocked {
            log::info!("updates: {reason}");
        }
        if let Some(from) = &flags.updated_from {
            log::info!(
                "started by the update from {from}; it hears that this version is healthy in {} seconds",
                update::HEALTHY_AFTER.as_secs()
            );
            self.updates.healthy_pending = Some(from.clone());
            if !instance::is_test() {
                self.show_updated_notice();
            }
            self.fetch_installed_notes();
            unsafe {
                SetTimer(
                    self.hwnd,
                    HEALTHY_TIMER,
                    update::HEALTHY_AFTER.as_millis() as u32,
                    None,
                )
            };
        } else if self.updates.blocked.is_none() {
            update::remove_leftovers_soon(install::dir());
        }
        if self.updates.blocked.is_none() {
            let dir = install::dir();
            if self.config.updates.download_ahead {
                update_stage::tidy(&dir, env!("CARGO_PKG_VERSION"));
            } else {
                update_stage::remove(&dir);
            }
            self.updates.staged = update_stage::staged_tag(&dir);
            self.updates.stage_dir = Some(dir);
        }
        if let Some(to) = &flags.update_failed {
            let text = format!("Update to {to} failed, kept {}", env!("CARGO_PKG_VERSION"));
            log::warn!("{text}; the log above says why");
            self.updates.failed = Some(text.clone());
            self.update_notice(&text);
        }
        if flags.scene.update_notice {
            self.show_updated_notice();
        }
        if flags.scene.update_progress {
            self.updates.newer = Some(Release {
                tag: "v9.9.9".to_string(),
                notes: "A sample release: nothing is downloaded.".to_string(),
                page: github::repo_url().to_string(),
                assets: Vec::new(),
            });
            self.updates.busy = true;
            self.updates.status = UpdateStatus::Downloading(Some(42));
            self.publish();
        }
        if flags.scene.update_available {
            self.updates.newer = Some(Release {
                tag: "v9.9.9".to_string(),
                notes: "- A sample release note
- Another sample line

Nothing is downloaded."
                    .to_string(),
                page: github::repo_url().to_string(),
                assets: Vec::new(),
            });
            self.publish();
        }
        if flags.scene.update_now {
            self.updates.auto_update = true;
            self.check_for_updates(true);
        }
    }

    /// The version that started this one waits for this; a clean quit
    /// before the timer counts too, since nothing crashed.
    pub(super) fn report_healthy(&mut self, quitting: bool) {
        unsafe { KillTimer(self.hwnd, HEALTHY_TIMER) };
        let Some((from, tidy)) = self.updates.healthy_report(quitting) else {
            return;
        };
        if update::report_healthy() {
            log::info!("told {from} that this version is healthy");
        } else {
            log::info!("no WinCraft {from} was waiting to hear that this version is healthy");
        }
        if tidy {
            update::remove_leftovers_soon(install::dir());
        }
    }

    /// The palette's "Update WinCraft": choosing it is the consent. It
    /// checks now and installs a newer release at once, by the same path as
    /// About's Update now; any other answer is said in a notice.
    pub(super) fn update_from_palette(&mut self) {
        if self.updates.busy {
            log::info!("update WinCraft: an update is already running");
            return;
        }
        if let Some(reason) = self.updates.blocked.clone() {
            self.update_notice(&format!("This WinCraft cannot update itself: {reason}"));
            return;
        }
        self.updates.auto_update = true;
        self.updates.report = true;
        self.updates.reopen = Some(update::Reopen::Palette);
        self.check_for_updates(true);
    }

    /// Downloads on its own thread; the swap and the restart follow in
    /// `downloaded`.
    pub(super) fn update_now(&mut self) {
        if self.updates.busy {
            return;
        }
        if let Some(reason) = self.updates.blocked.clone() {
            self.update_notice(&reason);
            return;
        }
        let Some(release) = self.updates.newer.clone() else {
            self.updates.auto_update = true;
            self.check_for_updates(true);
            return;
        };
        log::info!("downloading {}", release.tag);
        self.updates.busy = true;
        self.updates.staged = None;
        self.updates.cancel.store(false, Ordering::Relaxed);
        self.updates.status = UpdateStatus::Downloading(None);
        self.publish();
        let cancel = Arc::clone(&self.updates.cancel);
        let tx = self.updates.tx.clone();
        let hwnd = self.hwnd as isize;
        let dir = install::dir();
        let local_fake = github::releases_override().is_some();
        let started = std::thread::Builder::new()
            .name("wincraft-update-download".to_string())
            .spawn(move || {
                let post = |event: UpdateEvent| {
                    if tx.send(event).is_ok() {
                        unsafe { PostMessageW(hwnd as HWND, WM_APP_UPDATE, 0, 0) };
                    }
                };
                let mut shown = None;
                let mut progress = |done: u64, total: Option<u64>| {
                    if cancel.load(Ordering::Relaxed) {
                        return false;
                    }
                    let percent = total
                        .filter(|total| *total > 0)
                        .map(|total| (done.saturating_mul(100) / total).min(100) as u8);
                    if shown != Some(percent) {
                        shown = Some(percent);
                        post(UpdateEvent::Progress(percent));
                    }
                    true
                };
                // Downloaded ahead: seconds instead of the whole download.
                if let Some(path) = update_stage::take(&dir, &release.tag) {
                    log::info!("{} was downloaded ahead and still matches", release.tag);
                    post(UpdateEvent::Downloaded(Ok(path)));
                    return;
                }
                let result =
                    update::download(&release, &dir, &update::Web, local_fake, &mut progress);
                post(UpdateEvent::Downloaded(result));
            });
        if let Err(err) = started {
            log::warn!("could not start the download: {err}");
            self.updates.busy = false;
            self.updates.status =
                UpdateStatus::Error(format!("Could not start the download: {err}"));
        }
    }

    pub(super) fn downloaded(&mut self, result: Result<std::path::PathBuf, String>) {
        let tag = self
            .updates
            .newer
            .as_ref()
            .map(|release| release.tag.clone())
            .unwrap_or_default();
        // A click on Cancel that came too late to stop the download still
        // stops the update: the file is deleted and the old version stays.
        if self.updates.cancel.swap(false, Ordering::Relaxed) {
            if let Ok(path) = &result {
                let _ = std::fs::remove_file(path);
            }
            log::info!("the download of {tag} was cancelled; the old version stays");
            self.updates.busy = false;
            self.updates.reopen = None;
            self.updates.status = UpdateStatus::Idle;
            return;
        }
        if let Err(err) = result {
            log::warn!("the update to {tag} was not installed: {err}");
            self.updates.busy = false;
            self.updates.reopen = None;
            self.updates.status = UpdateStatus::Error(err);
            return;
        }
        log::info!("{tag} is downloaded and matches its checksum");
        if let Some(release) = &self.updates.newer {
            release_notes::store(&release.tag, &release.notes);
        }
        self.updates.status = UpdateStatus::Installing;
        self.publish();
        let dir = install::dir();
        if let Err(err) = update::swap_in(&dir, &mut update::Disk) {
            log::error!("could not put {tag} in place: {err}");
            self.updates.busy = false;
            self.updates.reopen = None;
            self.updates.status =
                UpdateStatus::Error(format!("Could not put the new version in place: {err}"));
            return;
        }
        if let Some(stage) = &self.updates.stage_dir {
            update_stage::remove(stage);
        }
        log::info!("{tag} is in place; WinCraft closes and starts it");
        update::hand_over_at_exit(Handover {
            dir,
            from: env!("CARGO_PKG_VERSION").to_string(),
            to: tag,
            reopen: self.updates.reopen.take(),
        });
        self.shutdown();
    }

    /// On its own thread: GitHub can take seconds to answer, or never.
    pub(super) fn check_for_updates(&mut self, manual: bool) {
        if self.updates.busy {
            return;
        }
        if manual {
            self.updates.manual = true;
            self.updates.status = UpdateStatus::Checking;
            self.publish();
        }
        if self.updates.checking {
            return;
        }
        self.updates.checking = true;
        self.updates.last_try = update::now_secs();
        let tx = self.updates.tx.clone();
        let hwnd = self.hwnd as isize;
        let started = std::thread::Builder::new()
            .name("wincraft-update-check".to_string())
            .spawn(move || {
                let result = update::check(env!("CARGO_PKG_VERSION"));
                if tx.send(UpdateEvent::Checked(result)).is_ok() {
                    unsafe { PostMessageW(hwnd as HWND, WM_APP_UPDATE, 0, 0) };
                }
            });
        if let Err(err) = started {
            log::warn!("could not start the update check: {err}");
            self.updates.checking = false;
        }
    }

    /// The hourly timer: asks GitHub each time it comes round.
    pub(super) fn timed_check(&mut self) {
        unsafe { SetTimer(self.hwnd, UPDATE_TIMER, CHECK_TIMER_MS, None) };
        self.automatic_check(update_timing::Trigger::Timer);
    }

    /// The PC woke from sleep: a check follows once the network is back.
    pub(super) fn woke(&mut self) {
        if update::automatic_checks_allowed() {
            let delay = update_timing::WAKE_DELAY.as_millis() as u32;
            unsafe { SetTimer(self.hwnd, WAKE_TIMER, delay, None) };
        }
    }

    pub(super) fn wake_check(&mut self) {
        unsafe { KillTimer(self.hwnd, WAKE_TIMER) };
        self.automatic_check(update_timing::Trigger::Wake);
    }

    /// A check nobody clicked for, when the rules in `update_timing` say it
    /// is due. A test instance asks only a fake releases API.
    pub(super) fn automatic_check(&mut self, trigger: update_timing::Trigger) {
        if !update::automatic_checks_allowed() {
            return;
        }
        let last = self.updates.last_try.max(self.config.updates.last_check);
        let due = update_timing::due(
            trigger,
            update::now_secs(),
            last,
            self.config.updates.check_daily,
            self.updates.checking || self.updates.busy,
        );
        if due {
            log::info!("update check ({trigger:?})");
            self.check_for_updates(false);
        }
    }

    /// The panel that says the update worked. A click opens About.
    pub(super) fn show_updated_notice(&mut self) {
        let version = env!("CARGO_PKG_VERSION");
        log::info!("update notice: Updated to {version}");
        self.close_update_notice();
        match UpdateNotice::show(version, self.hwnd, WM_APP_NOTICE_CLICK) {
            Ok(notice) => {
                self.update_notice = Some(notice);
                unsafe { SetTimer(self.hwnd, NOTICE_TIMER, update_notice::TICK_MS, None) };
            }
            Err(err) => log::warn!("could not show the update notice: {err}"),
        }
    }

    pub(super) fn notice_tick(&mut self) {
        let showing = self.update_notice.as_mut().is_some_and(UpdateNotice::tick);
        if !showing {
            self.close_update_notice();
        }
    }

    pub(super) fn close_update_notice(&mut self) {
        unsafe { KillTimer(self.hwnd, NOTICE_TIMER) };
        if let Some(mut notice) = self.update_notice.take() {
            notice.close();
        }
    }

    /// The notes of the version that just started, for About: from the file
    /// the update kept, else from GitHub on a thread of its own.
    pub(super) fn fetch_installed_notes(&mut self) {
        let version = env!("CARGO_PKG_VERSION");
        self.updates.installed_notes = release_notes::load(version);
        if self.updates.installed_notes.is_some() || !update::automatic_checks_allowed() {
            return;
        }
        let tx = self.updates.tx.clone();
        let hwnd = self.hwnd as isize;
        let started = std::thread::Builder::new()
            .name("wincraft-release-notes".to_string())
            .spawn(
                move || match github::release_by_tag(&format!("v{version}")) {
                    Ok(Some(release)) => {
                        release_notes::store(&release.tag, &release.notes);
                        if !release.notes.trim().is_empty()
                            && tx.send(UpdateEvent::InstalledNotes(release.notes)).is_ok()
                        {
                            unsafe { PostMessageW(hwnd as HWND, WM_APP_UPDATE, 0, 0) };
                        }
                    }
                    Ok(None) => log::info!("no release notes are published for v{version}"),
                    Err(err) => {
                        log::info!("the release notes of v{version} could not be fetched: {err}")
                    }
                },
            );
        if let Err(err) = started {
            log::warn!("could not start the release notes fetch: {err}");
        }
    }

    /// A balloon that opens About when clicked. A test instance only logs
    /// it: a fake release must never put a notice on the user's screen.
    pub(super) fn update_notice(&mut self, text: &str) {
        log::info!("update notice: {text}");
        if !instance::is_test() {
            self.balloon_click = BalloonClick::About;
            self.tray.balloon("WinCraft", text);
        }
    }

    pub(super) fn update_events(&mut self) {
        while let Ok(event) = self.updates.rx.try_recv() {
            match event {
                UpdateEvent::Checked(result) => self.checked(result),
                UpdateEvent::Progress(percent) => {
                    if self.updates.busy {
                        self.updates.status = UpdateStatus::Downloading(percent);
                    }
                }
                UpdateEvent::Downloaded(result) => self.downloaded(result),
                UpdateEvent::InstalledNotes(notes) => self.updates.installed_notes = Some(notes),
                UpdateEvent::Staged(tag, result) => self.staged(tag, result),
            }
        }
        self.publish();
    }

    pub(super) fn checked(&mut self, result: Result<Checked, String>) {
        let after = self
            .updates
            .checked(result, &mut self.config.updates, update::now_secs());
        if after.reload_notes {
            self.updates.installed_notes = release_notes::load(env!("CARGO_PKG_VERSION"));
        }
        if let Some(text) = after.notice {
            self.update_notice(&text);
        }
        if !after.update_now && !self.updates.busy {
            self.updates.reopen = None;
        }
        if after.update_now {
            self.update_now();
        } else if self.updates.newer.is_some() {
            self.stage_ahead();
        }
        if after.answered {
            self.save_config();
        }
    }

    /// Downloads the newer release ahead, on its own thread, when About's
    /// "Download updates ahead" is on. Never on a metered connection, never
    /// in a test instance without a fake releases API, and never installed
    /// from here: Update now still waits for the user.
    pub(super) fn stage_ahead(&mut self) {
        let (Some(release), Some(dir)) =
            (self.updates.newer.clone(), self.updates.stage_dir.clone())
        else {
            return;
        };
        let wanted = update_stage::wanted(
            self.config.updates.download_ahead,
            self.updates.blocked.is_none() && update::automatic_checks_allowed(),
            self.updates.busy || self.updates.staging,
            self.updates.staged.as_deref(),
            &release.tag,
        );
        if !wanted {
            return;
        }
        self.updates.staging = true;
        let tx = self.updates.tx.clone();
        let hwnd = self.hwnd as isize;
        let local_fake = github::releases_override().is_some();
        let started = std::thread::Builder::new()
            .name("wincraft-update-ahead".to_string())
            .spawn(move || {
                let result = if update_stage::metered_now() {
                    Err("the connection is metered".to_string())
                } else {
                    update_stage::stage(&release, &dir, &update::Web, local_fake)
                };
                if tx.send(UpdateEvent::Staged(release.tag, result)).is_ok() {
                    unsafe { PostMessageW(hwnd as HWND, WM_APP_UPDATE, 0, 0) };
                }
            });
        if let Err(err) = started {
            log::warn!("could not start the download ahead: {err}");
            self.updates.staging = false;
        }
    }

    pub(super) fn staged(&mut self, tag: String, result: Result<(), String>) {
        self.updates.staging = false;
        match result {
            Ok(()) => {
                log::info!("{tag} is downloaded ahead and matches its checksum");
                self.updates.staged = Some(tag);
            }
            Err(err) => {
                log::info!("{tag} was not downloaded ahead: {err}");
                self.updates.staged = None;
            }
        }
    }

    /// About's "Download updates ahead". Off deletes what was kept, so the
    /// setting leaves nothing behind (Principle 3).
    pub(super) fn set_download_ahead(&mut self, on: bool) {
        self.config.updates.download_ahead = on;
        log::info!("download updates ahead: {on}");
        if on {
            self.stage_ahead();
        } else if let Some(dir) = &self.updates.stage_dir {
            update_stage::remove(dir);
            self.updates.staged = None;
        }
    }

    /// Ctrl+Del on the palette's "Update ready" row. Typing "Update
    /// WinCraft" still finds the update with its badge.
    pub(super) fn hide_update(&mut self, tag: String) {
        log::info!("the palette hides the update to {tag} until a newer one");
        self.config.updates.hidden_version = tag;
        self.save_config();
        self.publish();
    }
}
