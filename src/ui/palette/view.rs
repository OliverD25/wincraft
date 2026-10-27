use super::*;

impl Palette {
    pub(super) fn show_view(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &UiSnapshot,
        to_host: &Arc<HostChannel>,
        appear: f32,
        interactive: bool,
    ) -> Outcome {
        if self.prompt.is_some() {
            self.thumbnail = None;
            return self.show_prompt(ui, snapshot, to_host, appear, interactive);
        }
        let ctx = ui.ctx().clone();
        self.refresh(snapshot, &ctx);
        self.sync_tips(snapshot);
        self.refresh_details();
        let tokens = Tokens::get(&ctx);
        let mut outcome = Outcome::Stay;
        let mut run: Option<(Run, Via)> = None;
        let mut actions = self.current_actions();
        let listed = inspector::listed(&actions);

        if interactive {
            let pressed = |key| ctx.input(|input| input.key_pressed(key));
            // The confirmation goes at the first key, so typing starts on a
            // clean palette.
            if self.saved.is_some()
                && ctx.input(|input| {
                    input.events.iter().any(|event| {
                        matches!(
                            event,
                            egui::Event::Key { pressed: true, .. } | egui::Event::Text(_)
                        )
                    })
                })
            {
                self.saved = None;
            }
            // Any key but Enter cancels a pending "press Enter again".
            let other_key = ctx.input(|input| {
                input.events.iter().any(|event| match event {
                    // Ctrl+Delete is a "press again" itself, like Enter.
                    egui::Event::Key {
                        key: egui::Key::Delete,
                        pressed: true,
                        modifiers,
                        ..
                    } if modifiers.command => false,
                    egui::Event::Key { key, pressed, .. } => *pressed && *key != egui::Key::Enter,
                    egui::Event::Text(_) => true,
                    _ => false,
                })
            });
            if other_key {
                self.router.interrupted();
            }
            if pressed(egui::Key::Escape) {
                let left_actions =
                    self.action_cursor.take().is_some() | std::mem::take(&mut self.overlay);
                if left_actions {
                    // Back to the list first.
                } else if self.marks.clear() {
                    actions = self.current_actions();
                } else if !self.router.escape(self.prefix.as_deref()) {
                    outcome = Outcome::Hide;
                }
            }
            let ctrl = |key| {
                key_modifiers(&ctx, key).is_some_and(|modifiers| {
                    modifiers.command && !modifiers.shift && !modifiers.alt
                })
            };
            if ctrl(egui::Key::Comma) {
                outcome = Outcome::OpenSettings(Page::General);
            }
            if ctrl(egui::Key::P) {
                ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::P));
                outcome = Outcome::OpenSettings(Page::Plugins);
            }
            if let Some(at) = self.action_cursor {
                if pressed(egui::Key::ArrowDown) && !listed.is_empty() {
                    self.action_cursor = Some((at + 1) % listed.len());
                }
                if pressed(egui::Key::ArrowUp) && !listed.is_empty() {
                    self.action_cursor = Some((at + listed.len() - 1) % listed.len());
                }
                if key_modifiers(&ctx, egui::Key::ArrowLeft).is_some() {
                    ctx.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft)
                    });
                    self.action_cursor = None;
                    if self.narrow {
                        self.overlay = false;
                    }
                }
            } else if !self.results.items.is_empty() {
                let count = self.results.items.len();
                if pressed(egui::Key::ArrowDown) {
                    self.selected = (self.selected + 1) % count;
                    self.follow_selection = true;
                }
                if pressed(egui::Key::ArrowUp) {
                    self.selected = (self.selected + count - 1) % count;
                    self.follow_selection = true;
                }
            }
            match key_modifiers(&ctx, egui::Key::ArrowRight) {
                Some(modifiers) if modifiers.shift && !modifiers.command => {
                    if let Some(action) = actions.iter().find(|a| a.key == Key::ShiftRight) {
                        ctx.input_mut(|input| {
                            input.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowRight)
                        });
                        run = Some((action.run.clone(), Via::Other));
                    }
                }
                Some(modifiers)
                    if modifiers.is_none()
                        && self.action_cursor.is_none()
                        && !listed.is_empty()
                        && self.text_cursor_at_end(&ctx) =>
                {
                    ctx.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight)
                    });
                    self.action_cursor = Some(0);
                    self.overlay = self.narrow;
                }
                _ => {}
            }
            if let Some(modifiers) = key_modifiers(&ctx, egui::Key::Enter) {
                let chosen = match self.action_cursor {
                    Some(at) => listed.get(at).map(|index| &actions[*index]),
                    None => {
                        let key = if modifiers.command {
                            Key::CtrlEnter
                        } else if modifiers.shift {
                            Key::ShiftEnter
                        } else {
                            Key::Enter
                        };
                        actions.iter().find(|action| action.key == key)
                    }
                };
                if let Some(action) = chosen {
                    let via = if self.marks.is_empty()
                        && matches!(action.key, Key::Enter | Key::ShiftEnter | Key::CtrlEnter)
                    {
                        Via::Pick
                    } else {
                        Via::Other
                    };
                    run = Some((action.run.clone(), via));
                }
            }
            // These act on the row; the key is taken here so the text box
            // does not also act on it.
            let chords = [
                (egui::Key::B, Key::CtrlB),
                (egui::Key::Delete, Key::CtrlDelete),
                (egui::Key::K, Key::CtrlK),
                (egui::Key::W, Key::CtrlW),
                (egui::Key::M, Key::CtrlM),
                (egui::Key::Space, Key::CtrlSpace),
                (egui::Key::C, Key::CtrlC),
            ];
            for (key, slot) in chords {
                if !ctrl(key) {
                    continue;
                }
                if slot == Key::CtrlC && !self.nothing_selected_in_box(&ctx) {
                    continue;
                }
                if let Some(action) = actions.iter().find(|action| action.key == slot) {
                    ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, key));
                    run = run.or(Some((action.run.clone(), Via::Other)));
                }
            }
            if ctrl(egui::Key::A) {
                let marked = self.marks.mark_all(&self.results.items, Self::window_place);
                if marked > 0 {
                    ctx.input_mut(|input| {
                        input.consume_key(egui::Modifiers::COMMAND, egui::Key::A)
                    });
                    actions = self.current_actions();
                }
            }
            let copy_all = key_modifiers(&ctx, egui::Key::C)
                .is_some_and(|modifiers| modifiers.command && modifiers.shift);
            if copy_all {
                if let Some(text) = self.router.copy_all(self.prefix.as_deref()) {
                    to_host.send(HostRequest::RunAction(Action::Copy(text)));
                }
            }
            if pressed(egui::Key::Tab) {
                if let Some(action) = self.complete() {
                    run = run.or(Some((Run::Action(action), Via::Other)));
                }
            }
            // Checked before the text box sees the key: it is the Backspace
            // pressed on an already empty box that takes the prefix away.
            if pressed(egui::Key::Backspace) && self.query.is_empty() && self.prefix.is_some() {
                self.set_prefix(None);
            }
        }

        let strip = self.current_strip(to_host);
        let panel_width = self.panel_width();
        let (panel, painter, line) = paint_panel(ui, &tokens, appear, panel_width, self.glass);
        self.panel = panel;

        // Search bar: 56 high, 16 px sides, the search icon, the scope chip
        // when one is on, the query, and the Ask AI pill at the right.
        let search = Rect::from_min_size(panel.min, vec2(panel_width, SEARCH_HEIGHT));
        let rule = search_rule(ui.ctx(), &tokens, line);
        painter.hline(search.x_range(), search.bottom() - rule.width / 2.0, rule);
        let icon = Rect::from_center_size(
            pos2(search.left() + 16.0 + 8.0, search.center().y),
            vec2(16.0, 16.0),
        );
        icons::paint(&painter, icon, Icon::Search, tokens.text_disabled);
        let mut field_left = icon.right() + 12.0;
        let ask_prefix = self.router.ask_prefix();
        let asking = ask_prefix.is_some() && self.prefix == ask_prefix;
        let pill = ask_prefix.as_ref().map(|_| {
            let size = ask_pill_size(&painter);
            Rect::from_min_size(
                pos2(
                    search.right() - 16.0 - size.x,
                    search.center().y - size.y / 2.0,
                ),
                size,
            )
        });
        if let Some(pill) = pill {
            if ask_pill(ui, pill, asking, interactive) && !asking {
                self.prefix = ask_prefix.clone();
                self.selected = 0;
                self.cursor_to_end = true;
            }
        }
        let placeholder = match &self.prefix {
            Some(prefix) => {
                let (name, placeholder) = self.router.chip(prefix, &self.query);
                let chip = prefix_chip(
                    &ctx,
                    &painter,
                    pos2(field_left, search.center().y),
                    prefix,
                    name,
                    &tokens,
                );
                field_left = chip.right() + 10.0;
                placeholder
            }
            None => "Type a command, app or window\u{2026}",
        };
        let field_right = pill.map_or(search.right() - 16.0, |pill| pill.left() - 12.0);
        let field = Rect::from_min_max(
            pos2(field_left, search.top()),
            pos2(field_right, search.bottom() - line.width),
        );
        let hint = egui::RichText::new(placeholder)
            .color(tokens.text_disabled)
            .font(theme::regular(16.0));
        let edit = egui::TextEdit::singleline(&mut self.query)
            .id_salt("palette-query")
            .hint_text(hint)
            .font(theme::regular(16.0))
            .text_color(tokens.text_primary)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .vertical_align(Align::Center)
            .lock_focus(true)
            .desired_width(field.width());
        let response = ui.put(field, edit);
        self.query_id = Some(response.id);
        // Asked for on every frame, not once: the window only receives focus a
        // frame or two after it is shown, and egui drops a focus request made
        // before that, which left the first keystrokes going nowhere.
        if interactive && !response.has_focus() {
            response.request_focus();
        }
        if response.changed() {
            self.selected = 0;
            self.follow_selection = true;
            self.action_cursor = None;
            if self.prefix.is_none() {
                if let Some((prefix, rest)) = self.router.split(&self.query) {
                    let rest = rest.to_string();
                    self.prefix = Some(prefix);
                    self.set_query(rest);
                }
            }
        }
        if std::mem::take(&mut self.cursor_to_end) {
            if let Some(mut state) = egui::TextEdit::load_state(&ctx, response.id) {
                let end = egui::text::CCursor::new(self.query.chars().count());
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(end)));
                state.store(&ctx, response.id);
            }
        }

        // Footer: 40 high, the gear, the selected row's keys, the version.
        let footer = Rect::from_min_max(
            pos2(panel.left(), panel.bottom() - FOOTER_HEIGHT),
            panel.max,
        );
        let escape = if !self.marks.is_empty() {
            "Clear marks"
        } else if self.action_cursor.is_some() || self.overlay {
            "Back"
        } else {
            self.results.escape_label.unwrap_or("Close")
        };
        let clear_scope = self.prefix.is_some() && self.query.is_empty();
        let hints = actions::footer_hints(&actions, clear_scope, escape);
        if paint_footer(
            ui,
            footer,
            &tokens,
            line,
            hints,
            &snapshot.version,
            interactive,
        ) {
            outcome = Outcome::OpenSettings(Page::General);
        }

        // The list on the left, the inspector at the right; on a narrow
        // monitor the inspector opens over the list.
        let body_top = search.bottom();
        let body_bottom = footer.top();
        let inspector_rect = Rect::from_min_max(
            pos2(panel.right() - inspector::WIDTH, body_top),
            pos2(panel.right(), body_bottom),
        );
        let list_right = if self.narrow {
            panel.right()
        } else {
            inspector_rect.left()
        };

        // The tip takes the bottom of the list area; the window never changes
        // size and the list scrolls in what is left.
        let mut list_bottom = body_bottom - 4.0;
        if let Some(strip) = &strip {
            let rect = Rect::from_min_max(
                pos2(panel.left() + 8.0, list_bottom - STRIP_HEIGHT),
                pos2(list_right - 8.0, list_bottom),
            );
            list_bottom = rect.top() - 4.0;
            let (line, icon, colour, dismissable) = match strip {
                Strip::Hint(place) => (
                    tips::bookmark_hint(*place),
                    Icon::Lightbulb,
                    tokens.text_secondary,
                    true,
                ),
                Strip::Saved(phrase) => (
                    tips::bookmark_saved(phrase),
                    Icon::Check,
                    tokens.success,
                    false,
                ),
            };
            let dismissed = tip::strip(ui, rect, icon, colour, &line, dismissable);
            if dismissed && interactive && self.tips.finish(&tips::BOOKMARK_CTRL_B) {
                self.send_tips(to_host);
            }
        }
        let list = Rect::from_min_max(
            pos2(panel.left() + 8.0, body_top + 4.0),
            pos2(list_right - 8.0, list_bottom),
        );
        let waiting = self.prefix.is_some() && self.query.trim().is_empty();
        let mut clicked_row: Option<(usize, bool)> = None;
        let verb = actions
            .first()
            .filter(|action| action.key == Key::Enter)
            .map(|action| actions::enter_verb(&action.label));
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(list)
                .layout(Layout::top_down(Align::Min)),
            |ui| {
                ui.set_clip_rect(list.intersect(ui.clip_rect()));
                if self.results.items.is_empty() {
                    // Right after a prefix the grey text in the box says what
                    // to type; "Nothing matches" would be wrong.
                    if !waiting {
                        nothing_matches(ui, list, &tokens);
                    }
                    return;
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(list.height())
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if self.trying {
                            no_match_line(ui, &tokens);
                        }
                        let icons = self.icons.get_or_insert_with(|| IconCache::new(&ctx));
                        let needle = self.results.needle.as_str();
                        let column = !self.marks.is_empty();
                        let mut last_group: Option<&str> = None;
                        for (position, item) in self.results.items.iter().enumerate() {
                            let best = position == 0 && self.results.best_match && !self.trying;
                            let group = if best {
                                "Best match"
                            } else {
                                item.group.as_str()
                            };
                            if last_group != Some(group) {
                                last_group = Some(group);
                                group_header(ui, group);
                            }
                            let selected = position == self.selected;
                            let look = RowLook {
                                needle,
                                selected,
                                interactive,
                                column,
                                marked: item.window().is_some_and(|hwnd| self.marks.contains(hwnd)),
                                best,
                                verb: if selected && self.marks.is_empty() {
                                    verb.as_deref()
                                } else {
                                    None
                                },
                            };
                            let row = result_row(ui, item, &look, icons);
                            if selected && std::mem::take(&mut self.follow_selection) {
                                ui.scroll_to_rect(row.rect, None);
                            }
                            if row.clicked {
                                clicked_row = Some((position, row.shift));
                            }
                        }
                    });
            },
        );
        if let Some((position, shift)) = clicked_row {
            let item = &self.results.items[position];
            if shift && item.window().is_some() {
                let place = Self::window_place(item);
                let item = item.clone();
                self.marks.toggle(&item, || place);
                actions = self.current_actions();
            } else {
                self.selected = position;
                self.action_cursor = None;
                run = item
                    .enter
                    .as_ref()
                    .map(|choice| (Run::Action(choice.action.clone()), Via::Pick));
            }
        }

        // The inspector, for the row under the cursor.
        let show_inspector = !self.narrow || self.overlay;
        let mut preview = None;
        if show_inspector {
            let rect = if self.narrow {
                let rect = Rect::from_min_max(
                    pos2(panel.right() - inspector::WIDTH - 8.0, body_top + 8.0),
                    pos2(panel.right() - 8.0, body_bottom - 8.0),
                );
                if tokens.shadows {
                    painter.add(
                        theme::window_shadow(&tokens)
                            .as_shape(rect, CornerRadius::same(tokens.radius_window)),
                    );
                }
                painter.rect(
                    rect,
                    tokens.radius_window,
                    tokens.elevated_bg,
                    theme::line(&ctx, tokens.rim),
                    StrokeKind::Inside,
                );
                rect
            } else {
                painter.vline(
                    inspector_rect.left() + line.width / 2.0,
                    inspector_rect.y_range(),
                    line,
                );
                inspector_rect
            };
            let item = self.selected_item().cloned();
            let pick = match (&item, self.on_best_match()) {
                (Some(item), true) => item.usage_key.as_ref().and_then(|key| {
                    self.router
                        .pick_info(key, self.prefix.as_deref(), &self.query)
                        .map(|info| (self.query.trim().to_string(), info))
                }),
                _ => None,
            };
            let source = item
                .as_ref()
                .map(|item| self.source_line(item, snapshot))
                .unwrap_or_default();
            let details = self
                .details
                .as_ref()
                .filter(|(hwnd, _)| item.as_ref().and_then(ResultItem::window) == Some(*hwnd))
                .map(|(_, details)| details);
            let inspected = Inspected {
                item: item.as_ref(),
                marks: &self.marks,
                actions: &actions,
                cursor: self.action_cursor,
                details,
                pick,
                best: self.on_best_match(),
                source,
                interactive,
            };
            let clicked = ui
                .scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                    inspector::show(ui, rect, &inspected)
                })
                .inner;
            if let Some(index) = clicked.action {
                run = Some((actions[index].run.clone(), Via::Other));
            }
            if let (Some(on), Some(hwnd)) =
                (clicked.pin, item.as_ref().and_then(ResultItem::window))
            {
                to_host.send(HostRequest::RunAction(Action::AllDesktops { hwnd, on }));
                if let Some((_, details)) = self.details.as_mut() {
                    details.pinned = Some(on);
                }
            }
            if let (Some(rect), Some(hwnd)) =
                (clicked.preview, item.as_ref().and_then(ResultItem::window))
            {
                preview = Some((hwnd, rect));
            }
        }
        self.place_thumbnail(&ctx, &painter, &tokens, preview, appear);

        if let Some(until) = self.saved.as_ref().and_then(|saved| saved.until) {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                outcome = Outcome::Hide;
            } else {
                ctx.request_repaint_after(left);
            }
        }

        // Focus arrives a frame or two after the window is shown, so hiding on
        // "not focused" before it has ever been focused would close the palette
        // in the same breath as opening it.
        match ctx.input(|input| input.viewport().focused) {
            Some(true) => self.focused_once = true,
            Some(false) if self.focused_once && interactive => outcome = Outcome::Hide,
            _ => {}
        }

        if let Some((_, Via::Pick)) = &run {
            if let Some(key) = self.selected_item().and_then(|item| item.usage_key.clone()) {
                self.router.record_pick(
                    &key,
                    self.prefix.as_deref(),
                    &self.query,
                    &snapshot.search,
                );
            }
        }
        self.router.save_usage_soon();
        if let Some((run, _)) = run {
            let next = self.run(run, snapshot, to_host);
            if !matches!(next, Outcome::Stay) {
                outcome = next;
            }
        }
        outcome
    }
}
