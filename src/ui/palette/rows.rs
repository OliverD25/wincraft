use super::*;

/// A label with only the main run of the match in the accent colour, as the
/// design handout asks; the semibold weight is kept for headings, so the
/// match is not shown in bold.
fn label_job(
    label: &str,
    query: &str,
    font: FontId,
    colour: Color32,
    line_height: f32,
    tokens: &Tokens,
) -> LayoutJob {
    let matched = fuzzy::main_run(query, label).unwrap_or(0..0);
    let plain = text::format(font.clone(), colour, Some(line_height));
    let hit = text::format(font, tokens.accent, Some(line_height));
    let mut job = LayoutJob::default();
    for (index, character) in label.chars().enumerate() {
        let format = if matched.contains(&index) {
            hit.clone()
        } else {
            plain.clone()
        };
        let mut buffer = [0u8; 4];
        job.append(character.encode_utf8(&mut buffer), 0.0, format);
    }
    job
}

/// A small outlined pill after a title.
#[derive(Debug, PartialEq)]
pub(crate) enum Badge {
    /// "open", with a success dot.
    Open,
    /// "often picked", with a clock.
    OftenPicked,
    /// A text such as "0.20.20 available", with an accent dot.
    Update(String),
}

/// The badges a row carries: "open" on a window beside other rows, "often
/// picked" on the best match, and the command's own text, such as "0.20.20
/// available" on Update WinCraft.
pub(crate) fn row_badges(item: &ResultItem, best: bool) -> Vec<Badge> {
    let mut badges = Vec::new();
    if item.open && item.group == SUGGESTED_GROUP {
        badges.push(Badge::Open);
    }
    if best {
        badges.push(Badge::OftenPicked);
    }
    if let Some(text) = &item.badge {
        badges.push(Badge::Update(text.clone()));
    }
    badges
}

/// Draws one badge with its left edge at `at.x` and its top at `at.y`: 18
/// high, 1 px border, 12 px (the design's 11 px raised to the 12 px floor).
pub(crate) fn badge(ui: &egui::Ui, at: egui::Pos2, kind: &Badge, tokens: &Tokens) -> Rect {
    let painter = ui.painter();
    let (text, lead) = match kind {
        Badge::Open => ("open", 6.0),
        Badge::OftenPicked => ("often picked", 12.0),
        Badge::Update(text) => (text.as_str(), 6.0),
    };
    let ink = match kind {
        Badge::Update(_) => tokens.accent,
        _ => tokens.text_secondary,
    };
    let line = match kind {
        Badge::Update(_) => tokens.accent,
        _ => tokens.border,
    };
    let galley = painter.layout_no_wrap(text.to_string(), theme::regular(12.0), ink);
    let rect = Rect::from_min_size(at, vec2(8.0 + lead + 4.0 + galley.size().x + 8.0, 18.0));
    painter.rect(
        rect,
        9,
        Color32::TRANSPARENT,
        theme::line(ui.ctx(), line),
        StrokeKind::Inside,
    );
    let mark = pos2(rect.left() + 8.0 + lead / 2.0, rect.center().y);
    match kind {
        Badge::Open => {
            painter.circle_filled(mark, 3.0, tokens.success);
        }
        Badge::Update(_) => {
            painter.circle_filled(mark, 3.0, tokens.accent);
        }
        Badge::OftenPicked => icons::paint(
            painter,
            Rect::from_center_size(mark, vec2(12.0, 12.0)),
            Icon::Clock,
            tokens.text_secondary,
        ),
    }
    painter.galley(
        pos2(
            rect.left() + 8.0 + lead + 4.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink,
    );
    rect
}

/// How one row is drawn.
pub(super) struct RowLook<'a> {
    pub(super) needle: &'a str,
    pub(super) selected: bool,
    pub(super) interactive: bool,
    /// Windows are marked, so the checkbox column is there.
    pub(super) column: bool,
    pub(super) marked: bool,
    pub(super) best: bool,
    /// The Enter pill's word on the selected row, in place of its keycaps.
    pub(super) verb: Option<&'a str>,
    /// A toggle command's look and, once the host said it, its state.
    pub(super) toggle: Option<RowToggle<'a>>,
}

#[derive(Clone, Copy)]
pub(super) struct RowToggle<'a> {
    pub(super) look: ToggleLook,
    pub(super) state: Option<&'a ToggleState>,
}

impl RowToggle<'_> {
    /// On or off when there is something to switch; None while the state
    /// is not known yet, cannot be read, or has no target.
    pub(super) fn on(&self) -> Option<bool> {
        self.state.and_then(|state| state.on)
    }

    /// The line under the title: the state in words, or the command's own
    /// subtitle until the host has answered.
    pub(super) fn line<'b>(&'b self, subtitle: &'b str) -> &'b str {
        self.state.map_or(subtitle, |state| state.line.as_str())
    }

    /// The icon on the tile: the on icon only when on.
    pub(super) fn icon(&self) -> Icon {
        let name = if self.on() == Some(true) {
            self.look.icon_on
        } else {
            self.look.icon_off
        };
        Icon::named(name).unwrap_or(Icon::ChevronRight)
    }
}

pub(super) struct RowResponse {
    pub(super) clicked: bool,
    /// Shift was held for the click: it marks the window instead.
    pub(super) shift: bool,
    pub(super) rect: Rect,
}

/// A 44 px row: the checkbox column while windows are marked, the 26 px
/// icon slot, the title 15 (20 for an answer) with its badges and an
/// optional 12 px subtitle, and at the right the keycaps, or the Enter pill
/// on the selected row. A compact row is one 24 px line of monospace output.
/// The cursor row is drawn from state, never from hover; hover only adds the
/// 5 % tint. A command whose plugin is off stays listed at 45 % with an
/// italic "plugin off", so its hotkey does not seem to vanish.
pub(super) fn result_row(
    ui: &mut egui::Ui,
    entry: &ResultItem,
    look: &RowLook,
    icons: &mut IconCache,
) -> RowResponse {
    let tokens = Tokens::get(ui.ctx());
    let sense = if look.interactive {
        Sense::click()
    } else {
        Sense::hover()
    };
    let height = if entry.compact {
        COMPACT_HEIGHT
    } else {
        tokens.row_height.max(ROW_MIN_HEIGHT)
    };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), sense);
    let shift = response.clicked() && ui.input(|input| input.modifiers.shift);
    let answer = RowResponse {
        clicked: response.clicked(),
        shift,
        rect,
    };
    if !ui.is_rect_visible(rect) {
        return answer;
    }
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.08);
    if look.selected {
        row::paint_selection(ui, rect);
    } else if hover > 0.0 {
        ui.painter().rect_filled(
            rect,
            tokens.radius_row,
            tokens.hover_bg.gamma_multiply(hover),
        );
    }

    let inner = rect.shrink2(vec2(ROW_PADDING, 0.0));
    // A child that does not move the list's cursor: the row already took its
    // 44 px, and a scope would put the cursor back at the row's middle.
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let ui = &mut child;
    if entry.disabled {
        ui.multiply_opacity(0.45);
    }
    let painter = ui.painter().clone();
    let mut left = inner.left();
    if look.column && !entry.compact {
        if entry.window().is_some() {
            let check = Rect::from_min_size(
                pos2(left, rect.center().y - CHECK_SIZE / 2.0),
                vec2(CHECK_SIZE, CHECK_SIZE),
            );
            let edge = if look.marked {
                tokens.accent
            } else {
                tokens.border
            };
            painter.rect(
                check,
                3,
                Color32::TRANSPARENT,
                theme::line(ui.ctx(), edge),
                StrokeKind::Inside,
            );
            if look.marked {
                icons::paint(&painter, check.shrink(2.0), Icon::Check, tokens.accent);
            }
        }
        left += CHECK_SIZE + 10.0;
    }
    let (slot, text_left) = if entry.compact {
        let slot = Rect::from_min_size(pos2(left, rect.center().y - 8.0), vec2(16.0, 16.0));
        (slot, slot.right() + 12.0)
    } else {
        let slot = Rect::from_min_size(
            pos2(left, rect.center().y - ICON_SLOT / 2.0),
            vec2(ICON_SLOT, ICON_SLOT),
        );
        (slot, slot.right() + 12.0)
    };
    match &look.toggle {
        Some(toggle) => paint_state_tile(ui.ctx(), &painter, slot, toggle, &tokens),
        None => paint_icon(
            ui.ctx(),
            &painter,
            slot,
            entry,
            look.selected,
            icons,
            &tokens,
        ),
    }

    let mut text_right = inner.right();
    // A toggle ends in its switch, selected or not; Enter flips it, so it
    // needs no Enter pill. Its hotkey, if it has one, sits before it.
    let switch = look.toggle.as_ref().and_then(RowToggle::on);
    if let Some(on) = switch {
        let drawn = ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (rect, _) = ui.allocate_exact_size(toggle::SIZE, Sense::hover());
            toggle::paint_state(ui.painter(), ui.ctx(), rect, on);
            let mut left = rect.left();
            if let Some(keys) = crate::core::shortcuts::parse(&entry.keys) {
                ui.add_space(12.0);
                left = keycap::caps(ui, &keys.steps(), keycap::Caps::palette())
                    .rect
                    .left();
            }
            left
        });
        text_right = drawn.inner - 12.0;
    } else if let Some(verb) = look.verb.filter(|_| !entry.compact) {
        let pill = ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            enter_pill(ui, verb)
        });
        text_right = pill.inner.left() - 12.0;
    } else if let Some(keys) = crate::core::shortcuts::parse(&entry.keys) {
        let chips = ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            keycap::caps(ui, &keys.steps(), keycap::Caps::palette()).rect
        });
        text_right = chips.inner.left() - 12.0;
    } else if !entry.hint.is_empty() {
        let chips = ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            keycap::binding(ui, &keycap::spaced(&entry.hint), false).rect
        });
        text_right = chips.inner.left() - 12.0;
    }

    let off = entry.disabled.then(|| {
        let mut job = text::job(
            "plugin off",
            theme::regular(12.0),
            tokens.text_disabled,
            None,
        );
        if let Some(section) = job.sections.first_mut() {
            section.format.italics = true;
        }
        painter.layout_job(job)
    });
    let badges = if entry.compact {
        Vec::new()
    } else {
        row_badges(entry, look.best)
    };
    let badges_width: f32 = badges
        .iter()
        .map(|kind| match kind {
            Badge::Open => 52.0,
            Badge::OftenPicked => 104.0,
            Badge::Update(text) => {
                let width = painter
                    .layout_no_wrap(text.clone(), theme::regular(12.0), tokens.accent)
                    .size()
                    .x;
                8.0 + 6.0 + 4.0 + width + 8.0 + 6.0
            }
        })
        .sum();
    let off_width = off.as_ref().map_or(0.0, |galley| galley.size().x + 10.0);
    let title_width = text_right - text_left - off_width - badges_width;

    let colour = match entry.tone {
        Tone::Normal => tokens.text_primary,
        Tone::Warning => tokens.warning,
        Tone::Danger => tokens.danger,
    };
    let (font, line_height) = if entry.compact {
        (theme::mono(13.0), TITLE_LINE)
    } else if entry.answer {
        (theme::regular(20.0), ANSWER_LINE)
    } else {
        (theme::regular(15.0), TITLE_LINE)
    };
    let mut title = label_job(
        &entry.title,
        look.needle,
        font,
        colour,
        line_height,
        &tokens,
    );
    title.wrap = one_line(title_width);
    let title = painter.layout_job(title);
    let subtitle = look
        .toggle
        .as_ref()
        .map_or(entry.subtitle.as_str(), |toggle| {
            toggle.line(&entry.subtitle)
        });
    let top = if subtitle.is_empty() || entry.compact {
        rect.center().y - line_height / 2.0
    } else {
        rect.center().y - (line_height + SUBTITLE_LINE) / 2.0
    };
    let title_drawn = title.size().x;
    painter.galley(pos2(text_left, top), title, colour);
    let mut after = text_left + title_drawn + 10.0;
    if let Some(off) = off {
        let y = top + (line_height - off.size().y) / 2.0;
        let width = off.size().x;
        painter.galley(pos2(after, y), off, tokens.text_disabled);
        after += width + 10.0;
    }
    for kind in &badges {
        let at = pos2(after, top + (line_height - 18.0) / 2.0);
        after = badge(ui, at, kind, &tokens).right() + 6.0;
    }
    if !subtitle.is_empty() && !entry.compact {
        let mut subtitle = text::job(
            subtitle,
            theme::regular(12.0),
            tokens.text_secondary,
            Some(SUBTITLE_LINE),
        );
        subtitle.wrap = one_line(text_right - text_left);
        let subtitle = painter.layout_job(subtitle);
        painter.galley(
            pos2(text_left, top + line_height),
            subtitle,
            tokens.text_secondary,
        );
    }
    answer
}

/// The selected row's Enter pill: the verb in accent 12 px and a small ↵
/// keycap, in a 24 high outline of the accent colour.
fn enter_pill(ui: &mut egui::Ui, verb: &str) -> Rect {
    let tokens = Tokens::get(ui.ctx());
    let painter = ui.painter();
    let word = painter.layout_no_wrap(verb.to_string(), theme::regular(12.0), tokens.accent);
    let arrow = painter.layout_no_wrap("\u{21B5}".to_string(), theme::mono(12.0), tokens.accent);
    let cap_width = arrow.size().x + 8.0;
    let size = vec2(8.0 + word.size().x + 6.0 + cap_width + 4.0, 24.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        4,
        Color32::TRANSPARENT,
        theme::line(ui.ctx(), tokens.accent),
        StrokeKind::Inside,
    );
    let word_width = word.size().x;
    painter.galley(
        pos2(rect.left() + 8.0, rect.center().y - word.size().y / 2.0),
        word,
        tokens.accent,
    );
    let cap = Rect::from_min_size(
        pos2(rect.left() + 8.0 + word_width + 6.0, rect.center().y - 8.0),
        vec2(cap_width, 16.0),
    );
    painter.rect(
        cap,
        3,
        Color32::TRANSPARENT,
        theme::line(ui.ctx(), tokens.accent),
        StrokeKind::Inside,
    );
    painter.galley(cap.center() - arrow.size() / 2.0, arrow, tokens.accent);
    rect
}

/// A toggle's 28 px state tile on the icon slot. On: the command's own
/// colour with the on icon in white. Off, or not known: the theme's faint
/// fill and border with the off icon in the secondary text colour.
fn paint_state_tile(
    ctx: &egui::Context,
    painter: &egui::Painter,
    slot: Rect,
    toggle: &RowToggle,
    tokens: &Tokens,
) {
    let tile = Rect::from_center_size(slot.center(), vec2(STATE_TILE, STATE_TILE));
    let glyph = Rect::from_center_size(tile.center(), vec2(16.0, 16.0));
    if toggle.on() == Some(true) {
        let [r, g, b] = toggle.look.tile;
        painter.rect_filled(tile, 7, Color32::from_rgb(r, g, b));
        icons::paint(painter, glyph, toggle.icon(), Color32::WHITE);
    } else {
        painter.rect(
            tile,
            7,
            tokens.hover_bg,
            theme::line(ctx, tokens.border),
            StrokeKind::Inside,
        );
        icons::paint(painter, glyph, toggle.icon(), tokens.text_secondary);
    }
}

/// Long window titles and paths end in an ellipsis instead of running under
/// the keycaps.
fn one_line(width: f32) -> egui::text::TextWrapping {
    egui::text::TextWrapping {
        max_width: width.max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    }
}

/// The 26 px icon slot: a program's or file's own icon at 24 px, a Lucide
/// glyph at 18 px (accent on the selected row), or a monogram tile when the
/// row has no picture. A shell icon appears a frame or two after its row,
/// once the icon thread has read it; until then the slot stays empty rather
/// than flashing a stand-in.
fn paint_icon(
    ctx: &egui::Context,
    painter: &egui::Painter,
    slot: Rect,
    entry: &ResultItem,
    selected: bool,
    icons: &mut IconCache,
    tokens: &Tokens,
) {
    match &entry.icon {
        IconRef::None if entry.compact => {}
        IconRef::None => {
            painter.rect(
                slot,
                6,
                Color32::TRANSPARENT,
                theme::line(ctx, tokens.border),
                StrokeKind::Inside,
            );
            let letter: String = entry
                .title
                .chars()
                .find(|c| c.is_alphanumeric())
                .map(|c| c.to_uppercase().collect())
                .unwrap_or_default();
            painter.text(
                slot.center(),
                Align2::CENTER_CENTER,
                letter,
                theme::semibold(13.0),
                tokens.text_secondary,
            );
        }
        IconRef::Glyph(glyph) => {
            let drawn = match glyph {
                Glyph::Command => Icon::ChevronRight,
                Glyph::Search => Icon::Search,
                Glyph::Calculator => Icon::Calculator,
                Glyph::Globe => Icon::Globe,
                Glyph::Terminal => Icon::Terminal,
                Glyph::Sparkles => Icon::Sparkles,
            };
            if tokens.icon_tiles {
                let glyph_rect = Rect::from_center_size(slot.center(), vec2(18.0, 18.0));
                icons::paint_on_tile(
                    painter,
                    slot,
                    glyph_rect,
                    drawn,
                    f32::from(tokens.radius_row),
                );
            } else {
                let colour = if selected {
                    tokens.accent
                } else {
                    tokens.text_secondary
                };
                let size = if entry.compact { 16.0 } else { 18.0 };
                icons::paint(
                    painter,
                    Rect::from_center_size(slot.center(), vec2(size, size)),
                    drawn,
                    colour,
                );
            }
        }
        IconRef::Path(_) | IconRef::Window { .. } => {
            if let Some(texture) = icons.get(ctx, &entry.icon) {
                let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                let size = if entry.compact { 16.0 } else { 24.0 };
                painter.image(
                    texture.id(),
                    Rect::from_center_size(slot.center(), vec2(size, size)),
                    whole,
                    egui::Color32::WHITE,
                );
            }
        }
    }
}

/// A row's toggle look and state: for a row that runs a command marked as
/// a toggle, with the state the host last sent for it.
pub(super) fn row_toggle<'a>(
    item: &ResultItem,
    snapshot: &UiSnapshot,
    states: &'a [(CommandId, ToggleState)],
) -> Option<RowToggle<'a>> {
    let Some(Action::Command(id)) = item.enter.as_ref().map(|choice| &choice.action) else {
        return None;
    };
    let look = snapshot
        .commands
        .iter()
        .find(|entry| entry.id == *id)?
        .toggle?;
    let state = states
        .iter()
        .find(|(known, _)| known == id)
        .map(|(_, state)| state);
    Some(RowToggle { look, state })
}
