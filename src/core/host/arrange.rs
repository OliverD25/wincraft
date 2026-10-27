use super::*;

impl Host {
    /// Asks an enabled plugin that keeps a window order for its groups, or
    /// else the strip's own plugin, and sends them to the Arrange window.
    pub(super) fn send_arrange(&mut self, open: bool) {
        self.send_arrange_for(open, false);
    }

    /// `send_arrange`, but with `need_app` nothing is sent unless one of the
    /// groups is the anchor's app. Returns whether the strip was sent.
    pub(super) fn send_arrange_for(&mut self, open: bool, need_app: bool) -> bool {
        let anchor = if open {
            self.arrange_anchor.take()
        } else {
            None
        };
        let look = self.plugins.slots.iter().find_map(|slot| {
            if !slot.enabled {
                return None;
            }
            let look = slot.plugin.strip_look()?;
            Some((slot.plugin.metadata().id, look))
        });
        let Some((look_plugin, look)) = look else {
            if open {
                log::info!("WindowDeck is off, so the strip does not open");
            }
            return false;
        };
        // Read before the plugin builds its groups, so the mouse is still
        // where it was when the hotkey came.
        let hovered = match (&anchor, open) {
            (Some((app, _)), _) => Some(app.clone()),
            (None, true) => taskbar::app_under_cursor(),
            (None, false) => None,
        };
        let Some((id, mut groups)) = self.plugins.window_groups(look_plugin) else {
            log::info!("no enabled plugin gives the strip its windows");
            return false;
        };
        let under_cursor = hovered.as_deref().and_then(|app| {
            groups
                .groups
                .iter()
                .position(|group| taskbar::same_app(app, &group.key))
        });
        if need_app && under_cursor.is_none() {
            return false;
        }
        ATTENTION.with(|attention| {
            let attention = attention.borrow();
            for window in groups.groups.iter_mut().flat_map(|g| g.windows.iter_mut()) {
                window.attention = attention.wants(window.hwnd);
            }
        });
        let mut focus = groups.focus;
        if open {
            let asked = self.arrange_focus.take().and_then(|wanted| {
                let found = ArrangeGroup::pick(&groups.groups, &wanted);
                if found.is_none() {
                    log::info!("the scene asked for group \"{wanted}\", which is not open");
                }
                found
            });
            let rule = match (asked, under_cursor) {
                (Some(index), _) => {
                    focus = index;
                    "asked for by the test scene"
                }
                (None, Some(index)) => {
                    focus = index;
                    if anchor.is_some() {
                        "mouse resting on its taskbar button"
                    } else {
                        "taskbar button under cursor"
                    }
                }
                (None, None) => "front window",
            };
            match groups.groups.get(focus) {
                Some(group) => log::info!("arrange opened on {} ({rule})", group.label),
                None => log::info!("arrange opened with no app windows"),
            }
        }
        let snapshot = ArrangeSnapshot {
            plugin: id.to_string(),
            groups: groups.groups,
            desktops: groups.desktops,
            focus,
            watched: groups.watched,
            preview: look.preview,
            can_rename: self.plugins.is_on(NAMER_ID),
            monitors: groups.monitors,
            view: look.view,
            look_plugin: look_plugin.to_string(),
            hints: look.hints,
            keeps_order: id != look_plugin,
            anchor: anchor.map(|(_, rect)| rect),
        };
        self.to_ui.send(if open {
            UiCommand::ShowArrange(snapshot)
        } else {
            UiCommand::ArrangeUpdate(snapshot)
        });
        if open {
            if let Some(card) = self.peek_card.take() {
                self.to_ui.send(UiCommand::PeekCard(card));
            }
        }
        true
    }

    /// A digit typed in the strip, or with Win+Alt still held after
    /// Win+Alt+A: the strip for that taskbar button.
    pub(super) fn arrange_digit(&mut self, digit: u8) {
        let started = std::time::Instant::now();
        let now = self.clock.elapsed().as_millis() as u64;
        // The badges show the buttons read when the wait started, and the
        // digits must count those; a second digit counts the same list.
        let fresh = self
            .buttons_at
            .is_some_and(|at| at.elapsed() < BUTTONS_FRESH);
        if !(self.wait_hint.is_some()
            || self.reveal.pending()
            || self.digits.may_follow(now)
            || fresh)
        {
            self.read_buttons();
        }
        let number = self.digits.press(digit, now, self.buttons.len());
        // The badges go at once, or never come; the hook stays only as long
        // as a second digit typed with Win+Alt still held could follow.
        self.stop_reveal();
        self.wait_hint = None;
        if self.digits.may_follow(now) {
            let wait = std::time::Duration::from_millis(taskbar_number::SECOND_DIGIT_MS);
            let hotkey = WAIT_HOTKEY.with(|cell| cell.get());
            chord::listen(self.hwnd as isize, WM_APP_WAIT_KEY, wait, wait_rule(hotkey));
        } else {
            chord::stop_for(self.hwnd as isize);
        }
        self.open_button(number);
        log::info!(
            "digit {digit}: the strip for taskbar button {number} was asked for {} ms after the digit came",
            started.elapsed().as_millis()
        );
    }

    pub(super) fn read_buttons(&mut self) {
        self.buttons = taskbar::main_buttons().unwrap_or_else(|reason| {
            log::info!("the taskbar's buttons could not be read: {reason}");
            Vec::new()
        });
        self.buttons_at = Some(std::time::Instant::now());
    }

    /// The Arrange strip's hotkey. The first press waits 3 s for a digit,
    /// with a number over each taskbar button; nothing opens yet. A second
    /// press during the wait opens the strip for the window in front, with
    /// that window picked.
    pub(super) fn arrange_hotkey(&mut self, hotkey: Hotkey, keys: &str) {
        if self.chord_wait.is_some() {
            self.end_wait();
        }
        if self.wait_hint.is_some() || chord::waiting_for(self.hwnd as isize) {
            self.end_wait();
            let front = unsafe { GetForegroundWindow() } as isize;
            log::info!("the Arrange hotkey twice: the strip for the window in front");
            self.arrange_anchor = None;
            self.send_arrange(true);
            self.to_ui.send(UiCommand::ArrangeSelectWindow(front));
            return;
        }
        self.read_buttons();
        self.digits = taskbar_number::Digits::default();
        WAIT_HOTKEY.with(|cell| cell.set(Some(hotkey)));
        let wait = std::time::Duration::from_millis(taskbar_number::WAIT_MS);
        if !chord::listen(
            self.hwnd as isize,
            WM_APP_WAIT_KEY,
            wait,
            wait_rule(Some(hotkey)),
        ) {
            // No hook: the digits cannot be caught, so the strip opens at
            // once, as the hotkey did before.
            self.arrange_anchor = None;
            self.send_arrange(true);
            return;
        }
        log::info!(
            "the Arrange hotkey: waiting 3 s for a taskbar number ({} buttons)",
            self.buttons.len()
        );
        self.reveal = taskbar_number::Reveal::start(self.now_ms());
        self.reveal_line = taskbar_number::wait_line(keys);
        unsafe {
            SetTimer(
                self.hwnd,
                BADGE_TIMER,
                taskbar_number::REVEAL_MS as u32,
                None,
            )
        };
    }

    pub(super) fn now_ms(&self) -> u64 {
        self.clock.elapsed().as_millis() as u64
    }

    /// The badge timer came round: the badges and the line show if the
    /// wait still runs and no digit came.
    pub(super) fn reveal_tick(&mut self) {
        unsafe { KillTimer(self.hwnd, BADGE_TIMER) };
        let now = self.now_ms();
        if !chord::waiting_for(self.hwnd as isize) {
            self.reveal.cancel();
            return;
        }
        if self.reveal.take_due(now) {
            log::info!(
                "no digit after {} ms: the badges show",
                taskbar_number::REVEAL_MS
            );
            self.wait_hint = Some(ui::taskbar_badges::WaitHint::show(
                &self.buttons,
                &self.reveal_line,
            ));
        } else if let Some(left) = self.reveal.left(now) {
            unsafe { SetTimer(self.hwnd, BADGE_TIMER, left.max(1) as u32, None) };
        }
    }

    pub(super) fn stop_reveal(&mut self) {
        if self.reveal.pending() {
            self.reveal.cancel();
            unsafe { KillTimer(self.hwnd, BADGE_TIMER) };
        }
    }

    /// The wait after the Arrange hotkey is over: the badges and the line go,
    /// and a first digit is no longer joined by a second one.
    pub(super) fn end_wait(&mut self) {
        chord::stop_for(self.hwnd as isize);
        self.chord_wait = None;
        self.stop_reveal();
        self.wait_hint = None;
        self.digits = taskbar_number::Digits::default();
    }

    /// Keys the hook kept during the wait, or the news that it ended.
    pub(super) fn wait_news(&mut self, news: chord::News) {
        if let Some(press) = self.chord_wait.clone() {
            for stroke in news.taken {
                match shortcuts::chord_key(&press, stroke) {
                    shortcuts::ChordKey::Run(command) => {
                        self.end_wait();
                        log::info!(
                            "chord {}: second key matched",
                            hotkeys::format(press.hotkey)
                        );
                        self.run_command(command);
                        return;
                    }
                    shortcuts::ChordKey::Cancel => {
                        self.end_wait();
                        return;
                    }
                    shortcuts::ChordKey::Other => {}
                }
            }
            if news.ended {
                self.end_wait();
            }
            return;
        }
        let hotkey = WAIT_HOTKEY.with(|cell| cell.get());
        for stroke in news.taken {
            let key = hotkey.map_or(taskbar_number::WaitKey::Other, |hotkey| {
                taskbar_number::wait_key(stroke, hotkey)
            });
            match key {
                taskbar_number::WaitKey::Digit(digit) => self.arrange_digit(digit),
                taskbar_number::WaitKey::Cancel => self.end_wait(),
                taskbar_number::WaitKey::Again | taskbar_number::WaitKey::Other => {}
            }
        }
        if news.ended {
            self.end_wait();
        }
    }

    /// Opens the strip above taskbar button `number` with its window
    /// picked, where the hover strip would open for that button. A number
    /// with no button does nothing.
    pub(super) fn open_button(&mut self, number: usize) {
        let Some((button, place)) = taskbar_number::button_number(&self.buttons, number) else {
            log::info!("taskbar button {number} does not exist");
            return;
        };
        let (app, rect) = (button.app.clone(), button.rect);
        self.arrange_anchor = Some((app.clone(), rect));
        // One look at the windows: the strip opens only if the app has some.
        if !self.send_arrange_for(true, true) {
            log::info!("taskbar button {number} ({app}) has no windows the strip shows");
            self.to_ui
                .send(UiCommand::ArrangeNote(taskbar_number::no_windows(number)));
            return;
        }
        log::info!("arrange opens for taskbar button {number} ({app}), window {place}");
        self.to_ui.send(UiCommand::ArrangeSelect(place));
    }

    /// Win and a digit, kept by WindowDeck's hook: the strip for that
    /// taskbar button with its window picked, or the next card when the
    /// same digit comes again while Win stayed down. The caller hands a
    /// `GiveBack` to Windows.
    pub(super) fn win_digit(&mut self, digit: u8, again: bool) -> WinDigit {
        let coming = self
            .win_digit_opened
            .is_some_and(|at| at.elapsed() < STRIP_COMING);
        if again && (coming || ui_bridge::arrange_window().is_some()) {
            log::info!("Win+{digit} again: the next card");
            self.to_ui.send(UiCommand::ArrangeSelectNext);
            return WinDigit::NextCard;
        }
        let fresh = self
            .buttons_at
            .is_some_and(|at| at.elapsed() < BUTTONS_FRESH);
        if !fresh {
            self.read_buttons();
        }
        let number = if digit == 0 { 10 } else { usize::from(digit) };
        let Some((button, place)) = taskbar_number::button_number(&self.buttons, number) else {
            log::info!("Win+{digit}: taskbar button {number} does not exist; Windows has it");
            return WinDigit::GiveBack;
        };
        let (app, rect) = (button.app.clone(), button.rect);
        self.arrange_anchor = Some((app.clone(), rect));
        if !self.send_arrange_for(true, true) {
            self.arrange_anchor = None;
            log::info!("Win+{digit}: {app} has no windows the strip shows; Windows has it");
            return WinDigit::GiveBack;
        }
        log::info!("Win+{digit}: the strip for taskbar button {number} ({app}), window {place}");
        self.to_ui.send(UiCommand::ArrangeSelect(place));
        self.win_digit_opened = Some(std::time::Instant::now());
        WinDigit::Opened
    }

    /// Asks the first enabled plugin that keeps a layout for its saved
    /// windows and sends them to the Layout page.
    pub(super) fn send_layout(&mut self) {
        let can_rename = self.plugins.is_on(NAMER_ID);
        let snapshot = self.plugins.slots.iter_mut().find_map(|slot| {
            if !slot.enabled {
                return None;
            }
            slot.plugin.layout_snapshot()
        });
        self.to_ui
            .send(UiCommand::LayoutUpdate(snapshot.map(|mut s| {
                s.can_rename = can_rename;
                s
            })));
    }
}

thread_local! {
    /// The Arrange strip's hotkey while it waits for a digit.
    pub(super) static WAIT_HOTKEY: Cell<Option<Hotkey>> = const { Cell::new(None) };
    /// The hotkey that came last, as pressed and as the user names it, for
    /// a plugin that asks for `arrange_hotkey` from its `on_hotkey`.
    pub(super) static LAST_HOTKEY: RefCell<Option<(Hotkey, String)>> = const { RefCell::new(None) };
}

/// The keyboard hook's rule while a chord with first press `press` waits
/// for its second press. It runs on the hook's thread, so it owns `press`.
pub(super) fn chord_rule(press: FirstPress) -> impl Fn(chord::Stroke) -> chord::Verdict + Send {
    move |stroke| shortcuts::chord_verdict(shortcuts::chord_key(&press, stroke))
}

/// The part of a WinCraft command's shortcut id after "wincraft.".
pub(super) fn host_slug(command: HostCommand) -> &'static str {
    match command {
        HostCommand::OpenSettings => "open-settings",
        HostCommand::OpenStore => "plugin-store",
        HostCommand::OpenLayout => "saved-layout",
        HostCommand::OpenAbout => "about",
        HostCommand::OpenConfig => "config-file",
        HostCommand::OpenLog => "log-file",
        HostCommand::UpdateWinCraft => "update",
        HostCommand::Exit => "quit",
        HostCommand::TogglePlugin(_) => "turn-on-off",
    }
}

/// The keyboard hook's rule while the Arrange hotkey `hotkey` waits; see
/// `taskbar_number::wait_key`. It runs on the hook's thread, so it owns
/// the hotkey.
pub(super) fn wait_rule(hotkey: Option<Hotkey>) -> impl Fn(chord::Stroke) -> chord::Verdict + Send {
    move |stroke| match hotkey {
        Some(hotkey) => taskbar_number::wait_verdict(taskbar_number::wait_key(stroke, hotkey)),
        None => chord::Verdict::End,
    }
}

/// A shell message that a window came, went or took the front: the open
/// Arrange strip shows a new window's card, and takes a closed one's away.
/// A new window is often still hidden when it is created, so the moment it
/// comes to the front counts too.
pub(super) fn window_list_changed(code: usize) -> bool {
    const CREATED: u32 = 1;
    const DESTROYED: u32 = 2;
    const ACTIVATED: u32 = 4;
    const RUDE_ACTIVATED: u32 = 0x8004;
    matches!(
        code as u32,
        CREATED | DESTROYED | ACTIVATED | RUDE_ACTIVATED
    )
}
