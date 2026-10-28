use super::*;

impl Arrange {
    pub fn new(to_host: Arc<HostChannel>, palette_hwnd: isize) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                snapshot: None,
                holds: Vec::new(),
                key: None,
                cursor: None,
                cursor_ghost: false,
                items: Vec::new(),
                drag: None,
                row_rects: Vec::new(),
                card_rects: Vec::new(),
                zones: Vec::new(),
                card_hits: Vec::new(),
                only_monitor: None,
                taskbars: MultiMonitor::default(),
                rail_focus: None,
                to_host,
                closed: false,
                window: 0,
                monitor: None,
                focused_once: false,
                thumbs: HashMap::new(),
                ghost_thumbs: HashMap::new(),
                sources: HashMap::new(),
                peek: Peek::default(),
                hover: Hover::default(),
                forced_peek: None,
                keep_open_until: None,
                others: others::Others::default(),
                known: HashSet::new(),
                appeared: HashMap::new(),
                leaving: Vec::new(),
                select: None,
                select_next: false,
                wants_keyboard: false,
                select_window: None,
                asked_at: None,
                note: None,
                anchor: None,
                compact_left: None,
                reveal: reveal::Reveal::Shown,
                reveal_window: 0,
                focus_wanted: false,
                pointer: pointer::Pointer::default(),
                close_when_empty: false,
                rename: None,
                focus_lost_at: None,
                took_back: false,
                parked: false,
            })),
            palette_hwnd,
        }
    }

    pub fn open(&self, snapshot: ArrangeSnapshot) {
        let Ok(mut shared) = self.shared.lock() else {
            return;
        };
        if open_timing::is_active() {
            let existed = settings::find_window(TITLE, self.palette_hwnd) != 0;
            open_timing::set_new_window(!existed);
        }
        // A strip already on screen takes the new data where it is.
        if shared.window == 0 && !shared.reveal.pending() {
            shared.reveal = reveal::Reveal::waiting(Instant::now());
        }
        shared.pointer.rest(cursor_position());
        shared.parked = false;
        shared.focus_lost_at = None;
        shared.took_back = false;
        shared.close_when_empty = false;
        shared.key = snapshot
            .groups
            .get(snapshot.focus)
            .map(|group| group.key.clone());
        shared.known = snapshot
            .groups
            .iter()
            .flat_map(|group| group.windows.iter().map(|w| w.hwnd))
            .collect();
        shared.appeared.clear();
        shared.leaving.clear();
        shared.select = None;
        shared.wants_keyboard = false;
        shared.select_window = None;
        shared.select_next = false;
        shared.asked_at = Some(Instant::now());
        shared.compact_left = None;
        shared.note = None;
        shared.rename = None;
        shared.snapshot = Some(snapshot);
        shared.holds.clear();
        shared.closed = false;
        shared.window = 0;
        shared.focused_once = false;
        shared.cursor = None;
        shared.cursor_ghost = false;
        shared.drag = None;
        shared.others.reset();
        shared.row_rects.clear();
        shared.card_rects.clear();
        shared.zones.clear();
        shared.card_hits.clear();
        shared.anchor = shared.snapshot.as_ref().and_then(|s| s.anchor);
        // Read again at every hover: the user may have changed the setting.
        shared.taskbars = MultiMonitor::read();
        let taskbars = shared.taskbars;
        shared.only_monitor = shared
            .snapshot
            .as_ref()
            .and_then(|s| only_monitor(s.anchor, &s.monitors, taskbars));
        shared.monitor = match shared.anchor {
            Some(button) => Monitor::at(POINT {
                x: (button[0] + button[2]) / 2,
                y: (button[1] + button[3]) / 2,
            }),
            None => Monitor::under_pointer(),
        };
    }

    /// Takes fresh data and keeps the same taskbar group on screen.
    pub fn update(&self, mut snapshot: ArrangeSnapshot) {
        let Ok(mut shared) = self.shared.lock() else {
            return;
        };
        let old = shared.group().cloned();
        let still_there = shared
            .key
            .as_ref()
            .is_some_and(|key| snapshot.groups.iter().any(|group| &group.key == key));
        if !still_there {
            shared.key = snapshot
                .groups
                .get(snapshot.focus)
                .map(|group| group.key.clone());
        }
        let now = Instant::now();
        hold::filter(&mut shared.holds, &mut snapshot.groups, now);
        let Shared {
            known,
            appeared,
            leaving,
            ..
        } = &mut *shared;
        note_changes(
            known,
            appeared,
            leaving,
            old.as_ref(),
            &snapshot.groups,
            now,
        );
        keep_leaving(&shared.leaving, &mut snapshot.groups);
        shared.snapshot = Some(snapshot);
    }

    /// The strip was just opened for a taskbar button: pick the window with
    /// this number in its row and take the keyboard.
    pub fn select_place(&self, place: usize) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.select = Some(place);
            shared.wants_keyboard = true;
            if shared.window != 0 {
                shared.focus_wanted = true;
            }
        }
    }

    /// Win and the same number came again: pick the next card on this
    /// desktop, round to the first after the last.
    pub fn select_next(&self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.select_next = true;
        }
    }

    /// The strip was just opened for the window in front: pick its card.
    pub fn select_window(&self, hwnd: isize) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.select_window = Some(hwnd);
        }
    }

    /// Shows a short line in the open strip for a moment.
    pub fn note(&self, text: String) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.note = Some((text, Instant::now()));
        }
    }

    /// Test scenes only: peek at one card as if the mouse rested on it.
    pub fn peek_card(&self, card: PeekCard) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.forced_peek = Some(card);
        }
    }

    pub fn was_closed(&self) -> bool {
        self.shared
            .lock()
            .map(|shared| shared.closed)
            .unwrap_or(false)
    }

    /// Unregisters every picture, hides the peek and hides the strip's
    /// window, which is kept for the next opening; runs on the UI thread,
    /// which owns them.
    pub fn hidden(&self) {
        if let Ok(mut shared) = self.shared.lock() {
            // Hidden, so Windows gives the keyboard back to the window the
            // user was in, as when the window was destroyed.
            let window = settings::find_window(TITLE, self.palette_hwnd);
            if window != 0 {
                unsafe { ShowWindow(window as HWND, SW_HIDE) };
            }
            shared.parked = true;
            shared.asked_at = None;
            shared.closed = false;
            shared.window = 0;
            ui_bridge::set_arrange_window(0);
            shared.anchor = None;
            shared.reveal = reveal::Reveal::Shown;
            shared.reveal_window = 0;
            shared.focus_wanted = false;
            shared.only_monitor = None;
            shared.drag = None;
            shared.others.reset();
            shared.known.clear();
            shared.appeared.clear();
            shared.leaving.clear();
            shared.thumbs.clear();
            shared.ghost_thumbs.clear();
            shared.sources.clear();
            shared.hide_peek();
            shared.forced_peek = None;
            shared.focus_lost_at = None;
            shared.took_back = false;
        }
    }

    /// The palette thread's part of `reveal`: finds the strip's hidden
    /// window, puts it in its place, cloaks it and lets it show; and shows a
    /// strip that took too long to draw.
    fn reveal_step(&self, ctx: &egui::Context) {
        let Ok(mut shared) = self.shared.lock() else {
            return;
        };
        let now = Instant::now();
        let found = settings::find_window(TITLE, self.palette_hwnd);
        if found != 0 && !shared.reveal.pending() && unsafe { IsWindowVisible(found as HWND) } == 0
        {
            // Made again by egui, or made after the wait had given up.
            shared.reveal = reveal::Reveal::waiting(now);
        }
        if found != 0 && shared.reveal.found(now) {
            open_timing::mark(OpenStep::WindowMade);
            if let Some(rect) = shared.wanted_rect() {
                keep_placed(found as HWND, rect);
            }
            let border = theme::colorref(Tokens::get(ctx).border);
            dwm_thumbs::round_corners(found as HWND, border);
            dwm_thumbs::cloak(found as HWND, true);
            shared.reveal_window = found;
            // Shown here, not through winit, which activates a window it
            // shows a second time: a hover strip must leave the keyboard
            // where it is, and the strip asks for it itself when it wants
            // it. egui keeps the window's own flag at hidden throughout.
            unsafe { ShowWindow(found as HWND, SW_SHOWNOACTIVATE) };
            ctx.request_repaint_of(viewport_id());
        }
        let cloaked = matches!(shared.reveal, reveal::Reveal::Cloaked { .. });
        if shared.reveal.overdue(now) {
            if cloaked && shared.reveal_window != 0 {
                dwm_thumbs::cloak(shared.reveal_window as HWND, false);
                log::warn!(
                    "the arrange strip was shown before it had drawn itself in place ({} ms)",
                    reveal::LONGEST.as_millis()
                );
            } else {
                log::warn!("the arrange strip's window did not come");
            }
        }
        match shared.reveal {
            reveal::Reveal::Waiting { .. } => {
                ctx.request_repaint_after(Duration::from_millis(16));
            }
            reveal::Reveal::Cloaked { .. } => {
                if let Some(left) = shared.reveal.left(now) {
                    ctx.request_repaint_after(left);
                }
            }
            reveal::Reveal::Shown => {}
        }
    }

    /// Draws the open strip, or keeps its hidden window alive between
    /// openings while WindowDeck is on: the next opening then reuses the
    /// window and its GL context instead of making new ones.
    pub fn show(&self, ctx: &egui::Context, icon: Option<Arc<egui::IconData>>) {
        let shared = Arc::clone(&self.shared);
        let palette_hwnd = self.palette_hwnd;
        let (panel, parked) = self
            .shared
            .lock()
            .map(|shared| (shared.panel(), shared.parked))
            .unwrap_or((EMPTY_PANEL, true));
        if !parked {
            self.reveal_step(ctx);
        }
        // Every value here stays the same from one opening to the next:
        // egui makes the window again when some of them change.
        let mut builder = egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_decorations(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_taskbar(false)
            .with_resizable(false)
            .with_active(false)
            // Shown by `reveal_step` once it is in its place.
            .with_visible(false)
            .with_inner_size(panel);
        if let Some(icon) = icon {
            builder = builder.with_icon(icon);
        }

        ctx.show_viewport_deferred(viewport_id(), builder, move |ui, _class| {
            let Ok(mut shared) = shared.lock() else {
                return;
            };
            if shared.parked {
                return;
            }
            focus::track(ui.ctx());
            if shared.asked_at.is_some() {
                open_timing::mark(OpenStep::FirstFrame);
            }
            if shared.window == 0 {
                shared.window = settings::find_window(TITLE, palette_hwnd);
                if shared.window != 0 {
                    let border = theme::colorref(Tokens::get(ui.ctx()).border);
                    dwm_thumbs::round_corners(shared.window as HWND, border);
                    ui_bridge::set_arrange_window(shared.window);
                    if shared.anchor.is_none() || shared.wants_keyboard {
                        shared.focus_wanted = true;
                    }
                }
            }
            let mut placed = true;
            if shared.window != 0 {
                if let Some(rect) = shared.wanted_rect() {
                    placed = keep_placed(shared.window as HWND, rect);
                }
            }
            if shared.reveal.frame(placed, Instant::now()) && shared.reveal_window != 0 {
                dwm_thumbs::cloak(shared.reveal_window as HWND, false);
            }
            if shared.reveal.pending() {
                ui.ctx().request_repaint();
            } else if shared.window != 0 {
                if let Some(asked) = shared.asked_at.take() {
                    if !open_timing::finish() {
                        log::info!(
                            "arrange strip on screen {} ms after the host asked for it",
                            asked.elapsed().as_millis()
                        );
                    }
                }
                // A cloaked window is not brought to the front.
                if std::mem::take(&mut shared.focus_wanted) {
                    shared.to_host.send(HostRequest::FocusWindow(shared.window));
                }
            }

            let (escape, focused) = ui
                .ctx()
                .input(|i| (i.key_pressed(egui::Key::Escape), i.viewport().focused));
            if focused == Some(true) {
                shared.focused_once = true;
                shared.focus_lost_at = None;
                shared.took_back = false;
            }
            // A middle-click on the hover strip's own taskbar button opens a
            // new window of the app, which takes the keyboard. The strip
            // stays, as a hover strip that was never clicked into does.
            if focused == Some(false) && shared.anchor.is_some_and(under_cursor) {
                shared.focused_once = false;
            }
            let now = Instant::now();
            let grace = shared
                .keep_open_until
                .and_then(|until| until.checked_duration_since(now));
            if let Some(left) = grace {
                ui.ctx().request_repaint_after(left);
            }
            // Esc ends a title edit and nothing more.
            let escape = escape && shared.rename.is_none();
            if escape {
                log::info!("WindowDeck closes: Esc");
                shared.closed = true;
            } else if shared.focused_once && focused == Some(false) && grace.is_none() {
                let lost = *shared.focus_lost_at.get_or_insert(now);
                let (class, exe) = focus_loss::front_window();
                let verdict = focus_loss::verdict(
                    now.saturating_duration_since(lost),
                    focus_loss::win_keys(),
                    focus_loss::is_shell(&class, &exe),
                    shared.took_back,
                );
                match verdict {
                    focus_loss::Verdict::Keep(after) => ui.ctx().request_repaint_after(after),
                    focus_loss::Verdict::TakeBack => {
                        log::info!(
                            "WindowDeck takes the keyboard back from {class} ({exe}), which took it right after a Windows key"
                        );
                        shared.took_back = true;
                        shared.to_host.send(HostRequest::FocusWindow(shared.window));
                        ui.ctx().request_repaint_after(focus_loss::WAIT);
                    }
                    focus_loss::Verdict::Close => {
                        log::info!(
                            "WindowDeck closes: it lost the keyboard to another window, {class} ({exe})"
                        );
                        shared.closed = true;
                    }
                }
            }
            if ui.ctx().input(|i| i.viewport().close_requested()) {
                log::info!("WindowDeck closes: Windows asked its window to close");
                shared.closed = true;
            }

            strip(ui, &mut shared);

            // The root viewport is the hidden palette and only repaints when
            // asked; it is what stops showing this viewport once closed.
            if shared.closed {
                ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
            }
        });
    }
}

/// The key of a top-row digit.
pub(super) fn digit_key(digit: u8) -> egui::Key {
    use egui::Key;
    [
        Key::Num0,
        Key::Num1,
        Key::Num2,
        Key::Num3,
        Key::Num4,
        Key::Num5,
        Key::Num6,
        Key::Num7,
        Key::Num8,
        Key::Num9,
    ][digit.min(9) as usize]
}

/// A short line over the top of the cards, above everything, for a moment.
pub(super) fn note_line(ctx: &egui::Context, panel: Rect, text: &str) {
    let tokens = Tokens::get(ctx);
    let layer = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("arrange-note"),
    ));
    let galley = layer.layout_no_wrap(text.to_string(), theme::regular(12.0), tokens.text_primary);
    let size = galley.size() + vec2(24.0, 12.0);
    let rect = Rect::from_center_size(
        pos2(panel.center().x, panel.top() + 72.0 + size.y / 2.0),
        size,
    );
    layer.rect(
        rect,
        tokens.radius_row,
        tokens.elevated_bg,
        theme::line(ctx, tokens.border),
        StrokeKind::Inside,
    );
    layer.galley(
        rect.center() - galley.size() / 2.0,
        galley,
        tokens.text_primary,
    );
}

/// Where the mouse is, in screen pixels.
pub(super) fn cursor_position() -> Option<(i32, i32)> {
    let mut point = POINT { x: 0, y: 0 };
    (unsafe { GetCursorPos(&mut point) } != 0).then_some((point.x, point.y))
}

/// Whether the mouse is on this rectangle, in screen pixels.
fn under_cursor(rect: [i32; 4]) -> bool {
    let mut point = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut point) };
    point.x >= rect[0] && point.x < rect[2] && point.y >= rect[1] && point.y < rect[3]
}

/// Moves the window where it belongs whenever it is not there: winit resizes
/// it itself when it lands on a monitor with other scaling. True when it was
/// there already.
fn keep_placed(hwnd: HWND, wanted: [i32; 4]) -> bool {
    let mut now = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe { GetWindowRect(hwnd, &mut now) };
    let current = [
        now.left,
        now.top,
        now.right - now.left,
        now.bottom - now.top,
    ];
    if current == wanted {
        return true;
    }
    unsafe {
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            wanted[0],
            wanted[1],
            wanted[2],
            wanted[3],
            SWP_NOACTIVATE,
        )
    };
    false
}
