use std::collections::BTreeMap;

use super::*;

#[test]
fn strokes_snap_up_to_whole_physical_pixels() {
    assert_eq!(snap(1.0, 1.0), 1.0);
    assert!((snap(1.0, 1.5) - 2.0 / 1.5).abs() < 1e-6);
    assert!((snap(2.0, 1.5) - 3.0 / 1.5).abs() < 1e-6);
    assert!((snap(1.0, 1.25) - 2.0 / 1.25).abs() < 1e-6);
    assert_eq!(snap(1.0, 2.0), 1.0);
}

fn first_faces(ctx: &egui::Context) -> [Option<String>; 3] {
    for _ in 0..2 {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.label(egui::RichText::new("WinCraft").font(semibold(20.0)));
            ui.label(egui::RichText::new("Toggle monitor 1").font(regular(14.0)));
            ui.label(egui::RichText::new("Win + Alt + F1").font(mono(12.0)));
        });
        // There is no renderer in a test to upload the font atlas to.
        output.textures_delta.clear();
    }
    let chains = ctx.fonts(|fonts| fonts.definitions().families.clone());
    let first = |family: FontFamily| chains.get(&family).and_then(|c| c.first()).cloned();
    [
        first(FontFamily::Proportional),
        first(semibold_family()),
        first(FontFamily::Monospace),
    ]
}

/// Glyph coverage is not asserted: egui's has_glyph reports every
/// character as missing once the first face can draw the replacement
/// glyph.
#[test]
fn without_segoe_ui_the_bundled_selawik_leads() {
    let ctx = egui::Context::default();
    install_fonts_from(&ctx, None);
    apply(&ctx, ThemeName::Classic, ThemeChoice::Dark);
    let [regular, semibold, mono] = first_faces(&ctx);
    assert_eq!(regular.as_deref(), Some(SELAWIK));
    assert_eq!(semibold.as_deref(), Some(SELAWIK_SEMIBOLD));
    assert_eq!(mono.as_deref(), Some(MONO));
}

#[test]
fn system_fonts_lead_when_windows_has_them() {
    let bundled = |bytes: &[u8]| bytes.to_vec();
    let ctx = egui::Context::default();
    install_fonts_from(
        &ctx,
        Some((bundled(SELAWIK_REGULAR_TTF), bundled(SELAWIK_SEMIBOLD_TTF))),
    );
    apply(&ctx, ThemeName::Classic, ThemeChoice::Dark);
    let [regular, semibold, _] = first_faces(&ctx);
    assert_eq!(regular.as_deref(), Some(SEGOE));
    assert_eq!(semibold.as_deref(), Some(SEGOE_SEMIBOLD));
}

#[test]
fn gdi_colours_are_blue_green_red() {
    assert_eq!(colorref(hex(0x1c1a19)), 0x00191a1c);
}

#[test]
fn the_current_theme_round_trips() {
    for name in ThemeName::ALL {
        for choice in ThemeChoice::ALL {
            set_current(name, choice);
            assert_eq!(current(), choice);
            assert_eq!(current_theme(), name);
        }
    }
    set_current(ThemeName::Classic, ThemeChoice::System);
}

#[test]
fn the_token_tables_match_the_handout() {
    let dark = Tokens::dark();
    assert_eq!(dark.window_bg, hex(0x1c1a19));
    assert_eq!(dark.accent, hex(0xe1ad66));
    assert_eq!(dark.focus_ring, dark.accent);
    assert_eq!(dark.selection_bg.a(), with_alpha(dark.accent, 0.18).a());

    let light = Tokens::light();
    assert_eq!(light.window_bg, hex(0xf3f2f2));
    assert_eq!(light.accent, hex(0xb68235));
    assert_eq!(light.text_primary, hex(0x201f1d));
    assert_ne!(dark, light);
}

/// Every value WinCraft drew with before themes, written out again, so
/// a change to Classic cannot slip in through a shared default.
#[test]
fn classic_is_exactly_the_look_from_before_themes() {
    let shape = |tokens: &Tokens| {
        assert_eq!(tokens.theme, ThemeName::Classic);
        assert_eq!(tokens.radius_window, 8);
        assert_eq!(tokens.radius_row, 4);
        assert_eq!(tokens.radius_tile, 6);
        assert_eq!(tokens.row_height, 44.0);
        assert_eq!(tokens.border_width, 1.0);
        assert_eq!(tokens.pill_width, 2.0);
        assert_eq!(tokens.pill_height, None);
        assert_eq!(tokens.pill_radius, 1);
        assert_eq!(tokens.top_highlight, Color32::TRANSPARENT);
        assert_eq!(tokens.glass_alpha, 1.0);
        assert!(!tokens.icon_tiles);
        assert_eq!(tokens.search_line, 0.0);
        assert!(!tokens.selection_ring);
        assert!(tokens.tile_fill);
        assert!(tokens.shadows);
        assert_eq!(tokens.fonts, FontSet::System);
        assert_eq!(tokens.rim, tokens.border);
        assert_eq!(tokens.focus_ring, tokens.accent);
    };

    let dark = Tokens::of(ThemeName::Classic, true);
    assert_eq!(dark, Tokens::dark());
    shape(&dark);
    let colours = [
        (dark.window_bg, hex(0x1c1a19)),
        (dark.panel_bg, hex(0x262322)),
        (dark.elevated_bg, hex(0x2d2b2b)),
        (dark.border, hex(0x444141)),
        (dark.text_primary, hex(0xf3f2f2)),
        (dark.text_secondary, hex(0xbab6b6)),
        (dark.text_disabled, hex(0x7d7979)),
        (dark.accent, hex(0xe1ad66)),
        (dark.accent_hover, hex(0xfacb8d)),
        (dark.accent_pressed, hex(0xc28d41)),
        (dark.accent_tint, with_alpha(hex(0xe1ad66), 0.10)),
        (dark.selection_bg, with_alpha(hex(0xe1ad66), 0.18)),
        (dark.hover_bg, with_alpha(hex(0xf3f2f2), 0.05)),
        (dark.success, hex(0x8fbf7f)),
        (dark.warning, hex(0xe88a4a)),
        (dark.danger, hex(0xe07a6d)),
        (dark.info, hex(0x8fb0d0)),
        (
            dark.palette_shadow,
            Color32::from_rgba_unmultiplied(0, 0, 0, 115),
        ),
    ];
    for (index, (got, want)) in colours.into_iter().enumerate() {
        assert_eq!(got, want, "dark colour {index}");
    }

    let light = Tokens::of(ThemeName::Classic, false);
    assert_eq!(light, Tokens::light());
    shape(&light);
    let colours = [
        (light.window_bg, hex(0xf3f2f2)),
        (light.panel_bg, hex(0xeae9e9)),
        (light.elevated_bg, hex(0xfaf8f7)),
        (light.border, hex(0xd7d3d3)),
        (light.text_primary, hex(0x201f1d)),
        (light.text_secondary, hex(0x605d5d)),
        (light.text_disabled, hex(0x9b9797)),
        (light.accent, hex(0xb68235)),
        (light.accent_hover, hex(0xa06f24)),
        (light.accent_pressed, hex(0x7d5411)),
        (light.accent_tint, with_alpha(hex(0xb68235), 0.10)),
        (light.selection_bg, with_alpha(hex(0xb68235), 0.16)),
        (light.hover_bg, with_alpha(hex(0x201f1d), 0.05)),
        (light.success, hex(0x3f7a4a)),
        (light.warning, hex(0xb45f14)),
        (light.danger, hex(0x9f3a2f)),
        (light.info, hex(0x4a6a8a)),
        (
            light.palette_shadow,
            Color32::from_rgba_unmultiplied(45, 43, 43, 56),
        ),
    ];
    for (index, (got, want)) in colours.into_iter().enumerate() {
        assert_eq!(got, want, "light colour {index}");
    }
}

#[test]
fn every_theme_has_a_full_token_set_in_both_modes() {
    for name in ThemeName::ALL {
        for dark in [true, false] {
            let tokens = Tokens::of(name, dark);
            let what = format!("{name:?} dark={dark}");
            assert_eq!(tokens.theme, name, "{what}");
            assert_eq!(tokens.dark, dark, "{what}");
            for colour in [
                tokens.window_bg,
                tokens.panel_bg,
                tokens.elevated_bg,
                tokens.border,
                tokens.rim,
                tokens.text_primary,
                tokens.text_secondary,
                tokens.text_disabled,
                tokens.accent,
                tokens.accent_hover,
                tokens.accent_pressed,
                tokens.success,
                tokens.warning,
                tokens.danger,
                tokens.info,
                tokens.focus_ring,
            ] {
                assert_eq!(colour.a(), 255, "{what}: solid colours are solid");
            }
            assert_ne!(tokens.text_primary, tokens.window_bg, "{what}");
            assert_ne!(tokens.accent, tokens.window_bg, "{what}");
            assert!(tokens.hover_bg.a() > 0, "{what}");
            assert!(tokens.row_height >= 34.0, "{what}");
            assert!(tokens.border_width >= 1.0, "{what}");
            assert!(tokens.radius_window <= 12, "{what}");
            assert!(
                tokens.selection_ring || tokens.selection_bg.a() > 0,
                "{what}: a selected row must show"
            );
            assert!(
                tokens.pill_width > 0.0 || tokens.selection_ring,
                "{what}: a selected row must show without colour too"
            );
        }
    }
}

#[test]
fn the_new_themes_have_the_values_from_the_design_page() {
    let terminal = Tokens::of(ThemeName::Terminal, true);
    assert_eq!(terminal.window_bg, hex(0x0f0e0d));
    assert_eq!(terminal.accent, hex(0xf0a640));
    assert_eq!(terminal.selection_bg, with_alpha(hex(0xf0a640), 0.12));
    assert_eq!((terminal.radius_window, terminal.radius_row), (4, 0));
    assert_eq!(terminal.row_height, 34.0);
    assert_eq!(terminal.fonts, FontSet::Mono);
    assert!(!terminal.tile_fill);
    let terminal_light = Tokens::of(ThemeName::Terminal, false);
    assert_eq!(terminal_light.rim, hex(0xd9d4cb));
    assert_eq!(terminal_light.border, hex(0xe3ded5));

    let paper = Tokens::of(ThemeName::Paper, true);
    assert_eq!(paper.window_bg, hex(0x1f1b17));
    assert_eq!(paper.accent, hex(0xd0a060));
    assert_eq!(paper.pill_width, 1.0);
    assert_eq!(paper.fonts, FontSet::Book);
    let paper_light = Tokens::of(ThemeName::Paper, false);
    assert_eq!(paper_light.elevated_bg, hex(0xf7f5f2));
    assert_eq!(paper_light.selection_bg, hex(0xfff3e4));

    let black = Tokens::of(ThemeName::HighContrast, true);
    assert_eq!(black.window_bg, Color32::BLACK);
    assert_eq!(black.accent, hex(0xffff00));
    assert_eq!(black.border_width, 2.0);
    assert_eq!(black.search_line, 2.0);
    assert!(black.selection_ring && !black.shadows);
    assert_eq!(black.selection_bg, Color32::TRANSPARENT);
    let white = Tokens::of(ThemeName::HighContrast, false);
    assert_eq!(white.window_bg, Color32::WHITE);
    assert_eq!(white.accent, hex(0x5a00b0));
    assert_eq!(white.text_disabled, hex(0x3a3a3a));
}

#[test]
fn fluent_has_the_values_from_the_design_page_with_solid_glass() {
    for dark in [true, false] {
        let fluent = Tokens::of(ThemeName::Fluent, dark);
        assert_eq!(
            (fluent.radius_window, fluent.radius_row, fluent.radius_tile),
            (12, 6, 7)
        );
        assert_eq!(fluent.pill_width, 3.0);
        assert_eq!(fluent.pill_height, Some(16.0));
        assert_eq!(fluent.pill_radius, 2);
        assert_eq!(fluent.search_line, 2.0);
        assert!(fluent.icon_tiles && fluent.shadows);
        assert_eq!(fluent.text_disabled, hex(0x8a8585));
        assert_eq!(fluent.elevated_bg.a(), 255, "the glass is solid");
        assert!(fluent.top_highlight.a() > 0);
    }
    let dark = Tokens::of(ThemeName::Fluent, true);
    assert_eq!(dark.window_bg, hex(0x1c1a19));
    assert_eq!(dark.elevated_bg, hex(0x242120));
    assert_eq!(dark.accent, hex(0xe1ad66));
    assert_eq!(dark.selection_bg, with_alpha(hex(0xe1ad66), 0.14));
    assert_eq!(dark.accent_tint, with_alpha(hex(0xe1ad66), 0.12));
    assert_eq!(dark.border, mix(hex(0x242120), Color32::WHITE, 0.08));
    let light = Tokens::of(ThemeName::Fluent, false);
    assert_eq!(light.window_bg, hex(0xf3f2f2));
    assert_eq!(light.elevated_bg, hex(0xfaf8f7));
    assert_eq!(light.accent, hex(0xb68235));
    assert_eq!(light.rim, mix(hex(0xfaf8f7), Color32::BLACK, 0.08));
}

#[test]
fn windows_high_contrast_wins_over_the_choice_and_a_preview() {
    assert_eq!(effective(ThemeName::Paper, None, false), ThemeName::Paper);
    assert_eq!(
        effective(ThemeName::Paper, Some(ThemeName::Terminal), false),
        ThemeName::Terminal
    );
    assert_eq!(
        effective(ThemeName::Paper, None, true),
        ThemeName::HighContrast
    );
    assert_eq!(
        effective(ThemeName::Classic, Some(ThemeName::Terminal), true),
        ThemeName::HighContrast
    );
}

fn families(ctx: &egui::Context) -> BTreeMap<FontFamily, Vec<String>> {
    // set_fonts takes effect in the next pass.
    let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
    output.textures_delta.clear();
    ctx.fonts(|fonts| fonts.definitions().families.clone())
}

/// The tests run on PCs with Windows high contrast both on and off, so
/// they only look at a theme when it is not forced.
fn shown(ctx: &egui::Context) -> ThemeName {
    Tokens::get(ctx).theme
}

#[test]
fn paper_puts_its_faces_first_and_classic_puts_the_old_chains_back() {
    let ctx = egui::Context::default();
    install_fonts_from(&ctx, None);
    apply(&ctx, ThemeName::Classic, ThemeChoice::Dark);
    let classic = families(&ctx);
    assert_eq!(
        classic.get(&head_family()),
        classic.get(&semibold_family()),
        "Classic titles use the same faces as before"
    );
    if windows_high_contrast() {
        return;
    }

    apply(&ctx, ThemeName::Paper, ThemeChoice::Dark);
    assert_eq!(shown(&ctx), ThemeName::Paper);
    let paper = families(&ctx);
    let first = |chains: &BTreeMap<FontFamily, Vec<String>>, family: FontFamily| {
        chains.get(&family).and_then(|c| c.first()).cloned()
    };
    assert_eq!(
        first(&paper, FontFamily::Proportional).as_deref(),
        Some(LORA)
    );
    assert_eq!(
        first(&paper, semibold_family()).as_deref(),
        Some(LORA_SEMIBOLD)
    );
    assert_eq!(
        first(&paper, head_family()).as_deref(),
        Some(CORMORANT_SEMIBOLD)
    );
    assert_eq!(
        first(&paper, FontFamily::Monospace).as_deref(),
        Some(MONO),
        "paths stay in JetBrains Mono"
    );

    apply(&ctx, ThemeName::Classic, ThemeChoice::Dark);
    assert_eq!(families(&ctx), classic);
}

#[test]
fn terminal_uses_a_windows_monospace_face_when_there_is_one() {
    let ctx = egui::Context::default();
    install_fonts_from(&ctx, None);
    apply(&ctx, ThemeName::Terminal, ThemeChoice::Light);
    if windows_high_contrast() {
        return;
    }
    let chains = families(&ctx);
    let first = chains
        .get(&FontFamily::Proportional)
        .and_then(|c| c.first())
        .cloned();
    if terminal_faces().is_some() {
        assert_eq!(first.as_deref(), Some(TERMINAL));
    } else {
        assert_eq!(first.as_deref(), Some(SELAWIK));
    }
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.label(egui::RichText::new("Settings").font(semibold(20.0)));
        ui.label(egui::RichText::new("Toggle monitor 1").font(regular(14.0)));
    });
    output.textures_delta.clear();
}

#[test]
fn a_preview_shows_until_it_is_cleared_and_survives_a_re_apply() {
    let ctx = egui::Context::default();
    install_fonts_from(&ctx, None);
    apply(&ctx, ThemeName::Classic, ThemeChoice::Dark);
    preview(&ctx, Some(ThemeName::Paper));
    assert_eq!(previewing(&ctx), Some(ThemeName::Paper));
    // A new monitor scale or a Windows colour change re-applies.
    apply(&ctx, ThemeName::Classic, ThemeChoice::Dark);
    assert_eq!(previewing(&ctx), Some(ThemeName::Paper));
    if !windows_high_contrast() {
        assert_eq!(shown(&ctx), ThemeName::Paper);
    }
    preview(&ctx, None);
    assert_eq!(previewing(&ctx), None);
    if !windows_high_contrast() {
        assert_eq!(shown(&ctx), ThemeName::Classic);
        assert_eq!(Tokens::get(&ctx), Tokens::dark());
    }
}

#[test]
fn every_theme_draws_text_in_both_modes() {
    let ctx = egui::Context::default();
    install_fonts_from(&ctx, None);
    for name in ThemeName::ALL {
        for mode in [ThemeChoice::Dark, ThemeChoice::Light] {
            apply(&ctx, name, mode);
            for _ in 0..2 {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    ui.label(egui::RichText::new("WinCraft").font(semibold(28.0)));
                    ui.label(egui::RichText::new("General").font(semibold(13.0)));
                    ui.label(egui::RichText::new("Start with Windows").font(regular(14.0)));
                    ui.label(egui::RichText::new("C:\\Windows").font(mono(13.0)));
                });
                output.textures_delta.clear();
            }
        }
    }
}
