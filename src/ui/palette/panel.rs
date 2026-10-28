use super::*;

/// The rule under the search field: the plain border, or an accent line
/// in themes that draw one.
pub(super) fn search_rule(
    ctx: &egui::Context,
    tokens: &Tokens,
    line: egui::Stroke,
) -> egui::Stroke {
    if tokens.search_line > 0.0 {
        theme::stroke(ctx, tokens.search_line, tokens.accent)
    } else {
        line
    }
}

/// Windows clips the glass window to its own rounded corners, 8 px at any
/// theme radius. The panel uses that radius while the glass is on, or a thin
/// strip of bare glass shows between Windows' curve and the painted one.
pub(super) const GLASS_CORNER_RADIUS: u8 = 8;

pub(super) fn with_window_corners(mut tokens: Tokens, glass: bool) -> Tokens {
    if glass {
        tokens.radius_window = GLASS_CORNER_RADIUS;
    }
    tokens
}

/// The panel with its shadow and border, rising as it fades in. Returns
/// the panel's rectangle, a painter and the 1 px rule.
pub(super) fn paint_panel(
    ui: &mut egui::Ui,
    tokens: &Tokens,
    appear: f32,
    width: f32,
    glass: bool,
) -> (Rect, egui::Painter, egui::Stroke) {
    let ctx = ui.ctx().clone();
    ui.multiply_opacity(appear);
    let window = ui.max_rect();
    // With the glass the panel is the window: no margin, and no room to rise.
    let (margin, rise) = if glass {
        (0.0, 0.0)
    } else {
        (MARGIN, (1.0 - appear) * RISE)
    };
    let panel = Rect::from_min_size(
        window.min + vec2(margin, margin + rise),
        vec2(width, PANEL_HEIGHT),
    );
    let radius = CornerRadius::same(tokens.radius_window);
    let painter = ui.painter().clone();
    if tokens.shadows && !glass {
        painter.add(theme::window_shadow(tokens).as_shape(panel, radius));
    }
    let fill = if glass {
        theme::with_alpha(tokens.elevated_bg, tokens.glass_alpha)
    } else {
        tokens.elevated_bg
    };
    painter.rect(
        panel,
        radius,
        fill,
        theme::line(&ctx, tokens.rim),
        StrokeKind::Inside,
    );
    if tokens.top_highlight.a() > 0 {
        let inset = theme::line(&ctx, tokens.rim).width;
        let r = f32::from(tokens.radius_window);
        painter.hline(
            panel.left() + r..=panel.right() - r,
            panel.top() + inset + 0.5,
            theme::stroke(&ctx, 1.0, tokens.top_highlight),
        );
    }
    (panel, painter, theme::line(&ctx, tokens.border))
}

/// The scope chip, such as "w Windows": 24 high, 1 px accent border,
/// radius 4, the prefix in mono and the scope's name in accent, both 12 px.
/// Returns the chip's rectangle so the query can start after it. A
/// prompt's chip has a name and no prefix.
pub(super) fn prefix_chip(
    ctx: &egui::Context,
    painter: &egui::Painter,
    left_centre: egui::Pos2,
    prefix: &str,
    name: &str,
    tokens: &Tokens,
) -> Rect {
    let mark = painter.layout_no_wrap(prefix.to_string(), theme::mono(12.0), tokens.accent);
    let label = painter.layout_no_wrap(name.to_string(), theme::regular(12.0), tokens.accent);
    let height = 24.0;
    let gap = if prefix.is_empty() { 0.0 } else { 6.0 };
    let width = 8.0 + mark.size().x + gap + label.size().x + 8.0;
    let rect = Rect::from_min_size(
        pos2(left_centre.x, left_centre.y - height / 2.0),
        vec2(width, height),
    );
    painter.rect(
        rect,
        4,
        Color32::TRANSPARENT,
        theme::line(ctx, tokens.accent),
        StrokeKind::Inside,
    );
    let mark_width = mark.size().x;
    painter.galley(
        pos2(rect.left() + 8.0, rect.center().y - mark.size().y / 2.0),
        mark,
        tokens.accent,
    );
    painter.galley(
        pos2(
            rect.left() + 8.0 + mark_width + gap,
            rect.center().y - label.size().y / 2.0,
        ),
        label,
        tokens.accent,
    );
    rect
}

const ASK_LABEL: &str = "Ask AI";

pub(super) fn ask_pill_size(painter: &egui::Painter) -> egui::Vec2 {
    let label = painter.layout_no_wrap(ASK_LABEL.to_string(), theme::regular(13.0), Color32::WHITE);
    vec2(12.0 + 14.0 + 6.0 + label.size().x + 8.0 + 20.0 + 6.0, 28.0)
}

/// The search bar's "Ask AI" pill: 28 high, round, a sparkles icon in the
/// accent colour, the label and a "?" keycap. Returns true when clicked.
pub(super) fn ask_pill(ui: &mut egui::Ui, rect: Rect, active: bool, interactive: bool) -> bool {
    let tokens = Tokens::get(ui.ctx());
    let sense = if interactive {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui
        .interact(rect, ui.id().with("ask-ai-pill"), sense)
        .on_hover_text("Ask AI in your browser \u{b7} type ? first");
    let painter = ui.painter();
    let fill = if response.hovered() {
        tokens.hover_bg
    } else {
        Color32::TRANSPARENT
    };
    let edge = if active { tokens.accent } else { tokens.border };
    painter.rect(
        rect,
        14,
        fill,
        theme::line(ui.ctx(), edge),
        StrokeKind::Inside,
    );
    let sparkles = Rect::from_min_size(
        pos2(rect.left() + 12.0, rect.center().y - 7.0),
        vec2(14.0, 14.0),
    );
    icons::paint(painter, sparkles, Icon::Sparkles, tokens.accent);
    let label = painter.layout_no_wrap(
        ASK_LABEL.to_string(),
        theme::regular(13.0),
        tokens.text_secondary,
    );
    let label_left = sparkles.right() + 6.0;
    let label_width = label.size().x;
    painter.galley(
        pos2(label_left, rect.center().y - label.size().y / 2.0),
        label,
        tokens.text_secondary,
    );
    let cap = Rect::from_min_size(
        pos2(label_left + label_width + 8.0, rect.center().y - 10.0),
        vec2(20.0, 20.0),
    );
    painter.rect(
        cap,
        tokens.radius_row,
        tokens.panel_bg,
        theme::line(ui.ctx(), tokens.border),
        StrokeKind::Inside,
    );
    painter.text(
        cap.center(),
        Align2::CENTER_CENTER,
        "?",
        theme::regular(12.0),
        tokens.text_secondary,
    );
    response.clicked()
}

/// The modifiers held when `key` went down this frame. Read from the key's
/// own event: a quick Ctrl+Enter can arrive together with the Ctrl release
/// in one frame, and the frame's modifier state would then say no Ctrl.
pub(super) fn key_modifiers(ctx: &egui::Context, key: egui::Key) -> Option<egui::Modifiers> {
    ctx.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::Key {
                key: pressed_key,
                pressed: true,
                modifiers,
                ..
            } if *pressed_key == key => Some(*modifiers),
            _ => None,
        })
    })
}

/// What every provider may read, from the host's latest snapshot.
pub(super) fn context(snapshot: &UiSnapshot) -> Context<'_> {
    Context {
        commands: &snapshot.commands,
        plugins: &snapshot.plugins,
        search: &snapshot.search,
        folder: None,
    }
}

/// The footer: 40 high on the elevated fill with a rule above it, the
/// Settings gear at the far left and a divider, the key hints, and the
/// version at the right. Returns true when the gear was clicked.
pub(super) fn paint_footer(
    ui: &mut egui::Ui,
    footer: Rect,
    tokens: &Tokens,
    line: egui::Stroke,
    hints: Vec<Hint>,
    version: &str,
    interactive: bool,
) -> bool {
    let painter = ui.painter().clone();
    let radius = tokens.radius_window;
    painter.rect_filled(
        footer.shrink(line.width),
        CornerRadius {
            nw: 0,
            ne: 0,
            sw: radius,
            se: radius,
        },
        tokens.elevated_bg,
    );
    painter.hline(footer.x_range(), footer.top() + line.width / 2.0, line);
    let gear = Rect::from_min_size(
        pos2(footer.left() + 8.0, footer.center().y - 14.0),
        vec2(28.0, 28.0),
    );
    let sense = if interactive {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui
        .interact(gear, ui.id().with("palette-settings"), sense)
        .on_hover_text("Settings \u{b7} Ctrl+,");
    if response.hovered() {
        painter.rect_filled(gear, tokens.radius_row, tokens.hover_bg);
    }
    icons::paint(
        &painter,
        Rect::from_center_size(gear.center(), vec2(16.0, 16.0)),
        Icon::Settings,
        tokens.text_secondary,
    );
    let divider_x = gear.right() + 8.0;
    painter.vline(
        divider_x,
        footer.center().y - 8.0..=footer.center().y + 8.0,
        line,
    );

    let version_text = format!("v{version}");
    let version_galley =
        painter.layout_no_wrap(version_text, theme::mono(12.0), tokens.text_disabled);
    let name_galley = painter.layout_no_wrap(
        "WinCraft".to_string(),
        theme::semibold(13.0),
        tokens.text_disabled,
    );
    let version_left = footer.right() - 16.0 - version_galley.size().x;
    let name_left = version_left - 6.0 - name_galley.size().x;
    painter.galley(
        pos2(name_left, footer.center().y - name_galley.size().y / 2.0),
        name_galley,
        tokens.text_disabled,
    );
    painter.galley(
        pos2(
            version_left,
            footer.center().y - version_galley.size().y / 2.0,
        ),
        version_galley,
        tokens.text_disabled,
    );

    let hints_left = divider_x + 12.0;
    let room = (name_left - 16.0 - hints_left).max(0.0);
    let hints = actions::fit_hints(hints, room);
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                pos2(hints_left, footer.top()),
                pos2(name_left - 16.0, footer.bottom()),
            ))
            .layout(Layout::left_to_right(Align::Center)),
        |ui| footer_hints(ui, tokens, &hints),
    );
    response.clicked()
}

/// Group header inside the list: 28 high, 12 px, sitting at the bottom
/// with 4 px under it, 12 px in from the left. `accent` draws it in the
/// accent colour, as "Update ready" is.
pub(super) fn group_header(ui: &mut egui::Ui, name: &str, accent: bool) {
    let tokens = Tokens::get(ui.ctx());
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), GROUP_HEADER), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter().text(
        pos2(rect.left() + 12.0, rect.bottom() - 4.0),
        Align2::LEFT_BOTTOM,
        name,
        theme::regular(12.0),
        if accent {
            tokens.accent
        } else {
            tokens.text_disabled
        },
    );
}

/// "No apps, windows or commands match." over the "Try instead" rows.
pub(super) fn no_match_line(ui: &mut egui::Ui, tokens: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), 12.0 + 18.0 + 16.0),
        Sense::hover(),
    );
    ui.painter().text(
        pos2(rect.left() + 12.0, rect.top() + 12.0),
        Align2::LEFT_TOP,
        "No apps, windows or commands match.",
        theme::regular(13.0),
        tokens.text_disabled,
    );
}

pub(super) fn nothing_matches(ui: &mut egui::Ui, list: Rect, tokens: &Tokens) {
    let block = 24.0 + 4.0 + 16.0;
    ui.add_space(((list.height() - block) / 2.0).max(0.0));
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            text::single(
                ui,
                text::job(
                    "Nothing matches",
                    theme::semibold(18.0),
                    tokens.text_primary,
                    Some(24.0),
                ),
            );
            text::caption(
                ui,
                "Try a shorter word, or press Esc to close.",
                tokens.text_secondary,
            );
        });
    });
}

/// Such as `↵ Switch  Shift+→ Next monitor  Esc Close`, 12 px with the keys
/// as 20 px chips. The arrows are drawn in the monospace family: they exist
/// only in egui's monospace fallback font.
pub(super) fn footer_hints(ui: &mut egui::Ui, tokens: &Tokens, hints: &[Hint]) {
    for (index, hint) in hints.iter().enumerate() {
        if index > 0 {
            ui.add_space(16.0);
        }
        let font = if hint.key.is_ascii() {
            theme::regular(12.0)
        } else {
            theme::mono(12.0)
        };
        keycap::hint(ui, hint.key, font);
        ui.add_space(6.0);
        let colour = if hint.accent {
            tokens.accent
        } else {
            tokens.text_secondary
        };
        text::single(
            ui,
            text::job(&hint.label, theme::regular(12.0), colour, None),
        );
    }
}
