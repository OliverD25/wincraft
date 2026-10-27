use super::*;

impl Palette {
    /// The prompt: the plugin's chip, the text box, and one row saying what
    /// Enter will do. Enter answers, Esc or a click elsewhere cancels.
    pub(super) fn show_prompt(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &UiSnapshot,
        to_host: &Arc<HostChannel>,
        appear: f32,
        interactive: bool,
    ) -> Outcome {
        let Some(prompt) = self.prompt.clone() else {
            return Outcome::Hide;
        };
        let ctx = ui.ctx().clone();
        let tokens = with_window_corners(Tokens::get(&ctx), self.glass);
        let mut outcome = Outcome::Stay;
        let mut answer = false;
        if interactive {
            if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
                outcome = Outcome::Hide;
            }
            answer = key_modifiers(&ctx, egui::Key::Enter).is_some();
        }

        let panel_width = self.panel_width();
        let (panel, painter, line) = paint_panel(ui, &tokens, appear, panel_width, self.glass);
        let search = Rect::from_min_size(panel.min, vec2(panel_width, SEARCH_HEIGHT));
        let rule = search_rule(ui.ctx(), &tokens, line);
        painter.hline(search.x_range(), search.bottom() - rule.width / 2.0, rule);
        let icon = Rect::from_center_size(
            pos2(search.left() + 16.0 + 8.0, search.center().y),
            vec2(16.0, 16.0),
        );
        icons::paint(&painter, icon, Icon::ChevronRight, tokens.text_disabled);
        let chip = prefix_chip(
            &ctx,
            &painter,
            pos2(icon.right() + 12.0, search.center().y),
            "",
            &prompt.chip,
            &tokens,
        );
        let field = Rect::from_min_max(
            pos2(chip.right() + 10.0, search.top()),
            pos2(search.right() - 16.0, search.bottom() - line.width),
        );
        let hint = egui::RichText::new(&prompt.placeholder)
            .color(tokens.text_disabled)
            .font(theme::regular(16.0));
        let edit = egui::TextEdit::singleline(&mut self.query)
            .id_salt("palette-prompt")
            .hint_text(hint)
            .font(theme::regular(16.0))
            .text_color(tokens.text_primary)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .vertical_align(Align::Center)
            .lock_focus(true)
            .desired_width(field.width());
        let response = ui.put(field, edit);
        if interactive && !response.has_focus() {
            response.request_focus();
        }
        // Kept until the box has focus: the window takes a frame or two to
        // get it, and a selection made before that is lost.
        if self.select_all && response.has_focus() {
            if let Some(mut state) = egui::TextEdit::load_state(&ctx, response.id) {
                let end = egui::text::CCursor::new(self.query.chars().count());
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        end,
                    )));
                state.store(&ctx, response.id);
                self.select_all = false;
            }
        }

        let typed = self.query.trim().to_string();
        let can_answer = !typed.is_empty() || prompt.empty_action.is_some();
        let item = ResultItem {
            group: prompt.chip.clone(),
            title: prompt_row_title(&prompt, &typed),
            subtitle: prompt.subtitle.clone(),
            icon: IconRef::Glyph(Glyph::Command),
            // Only the label is used, for the footer; Enter is handled here.
            enter: can_answer.then(|| Choice {
                label: if typed.is_empty() {
                    prompt.empty_action.clone().unwrap_or_default()
                } else {
                    prompt.chip.clone()
                },
                action: Action::SetPrefix(String::new()),
            }),
            ..Default::default()
        };

        let footer = Rect::from_min_max(
            pos2(panel.left(), panel.bottom() - FOOTER_HEIGHT),
            panel.max,
        );
        let mut hints: Vec<Hint> = Vec::new();
        if let Some(choice) = &item.enter {
            hints.push(Hint {
                key: Key::Enter.caps(),
                label: choice.label.clone(),
                accent: false,
            });
        }
        hints.push(Hint {
            key: "Esc",
            label: "Cancel".to_string(),
            accent: false,
        });
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

        let list = Rect::from_min_max(
            pos2(panel.left() + 8.0, search.bottom() + 4.0),
            pos2(panel.right() - 8.0, footer.top() - 4.0),
        );
        let icons = self.icons.get_or_insert_with(|| IconCache::new(&ctx));
        let look = RowLook {
            needle: "",
            selected: true,
            interactive,
            column: false,
            marked: false,
            best: false,
            verb: None,
        };
        let clicked = ui
            .scope_builder(
                UiBuilder::new()
                    .max_rect(list)
                    .layout(Layout::top_down(Align::Min)),
                |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.add_space(4.0);
                    result_row(ui, &item, &look, icons).clicked
                },
            )
            .inner;
        answer |= clicked;

        match ctx.input(|input| input.viewport().focused) {
            Some(true) => self.focused_once = true,
            Some(false) if self.focused_once && interactive => outcome = Outcome::Hide,
            _ => {}
        }

        if answer && can_answer {
            outcome = self.answer_prompt(&prompt, typed, snapshot, to_host);
        }
        outcome
    }

    /// Enter in the prompt: a plugin's question goes to the host; a search
    /// provider's is answered here, and it may answer with another question.
    pub(super) fn answer_prompt(
        &mut self,
        prompt: &Prompt,
        typed: String,
        snapshot: &UiSnapshot,
        to_host: &Arc<HostChannel>,
    ) -> Outcome {
        self.sync_tips(snapshot);
        match prompt.plugin.strip_prefix(SEARCH_PROMPT) {
            Some(provider) => {
                match self
                    .router
                    .answered(provider, &prompt.payload, &typed, &context(snapshot))
                {
                    Some(ask) => {
                        self.ask_for(ask);
                        Outcome::Stay
                    }
                    None => self.after_answer(provider, &typed, to_host),
                }
            }
            None => {
                to_host.send(HostRequest::PromptAnswer {
                    plugin: prompt.plugin.clone(),
                    id: prompt.id,
                    target: prompt.target,
                    text: typed,
                });
                Outcome::Hide
            }
        }
    }
}

/// The prompt's row: what Enter does with the text typed, or with none.
pub(super) fn prompt_row_title(prompt: &Prompt, typed: &str) -> String {
    if !typed.is_empty() {
        return format!("{} \u{201c}{typed}\u{201d}", prompt.action);
    }
    prompt
        .empty_action
        .clone()
        .unwrap_or_else(|| "Type, then press Enter".to_string())
}
