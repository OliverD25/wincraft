mod about;
mod general;
mod plugins;
mod store;

use std::sync::{Arc, Mutex};

use egui::{Margin, TextStyle, Ui};

use crate::core::hotkeys;
use crate::core::theme::{self, Tokens};
use crate::core::traits::Hotkey;
use crate::core::ui_bridge::{HostChannel, Page, UiSnapshot};
use crate::plugins::shortcut_detector::probe::{self, Status};
use crate::ui::widgets::capture::{self, Verdict};
use crate::ui::widgets::{focus, hotkey_capture, row, text};

/// The settings window is a viewport of its own, so a background thread that
/// finishes work has to wake that viewport by id. Waking only the root leaves
/// the settings window showing "Loading..." until the user moves the mouse.
pub fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("wincraft-settings")
}

enum Block<'a> {
    Heading(usize, &'a str),
    Markdown(String),
}

/// Splits a README into its `#` headings and the Markdown between them.
/// Headings inside fenced code blocks stay Markdown.
fn split_headings(source: &str) -> Vec<Block<'_>> {
    let mut blocks = Vec::new();
    let mut chunk = String::new();
    let mut fenced = false;
    for line in source.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        }
        let level = line.chars().take_while(|c| *c == '#').count();
        let is_heading = !fenced && level > 0 && line[level..].starts_with(' ');
        if is_heading {
            if !chunk.trim().is_empty() {
                blocks.push(Block::Markdown(std::mem::take(&mut chunk)));
            }
            chunk.clear();
            blocks.push(Block::Heading(level, line[level..].trim()));
        } else {
            chunk.push_str(line);
            chunk.push('\n');
        }
    }
    if !chunk.trim().is_empty() {
        blocks.push(Block::Markdown(chunk));
    }
    blocks
}

pub struct Readme(egui_commonmark::CommonMarkCache);

impl Default for Readme {
    fn default() -> Self {
        Self(egui_commonmark::CommonMarkCache::default())
    }
}

impl Readme {
    /// egui_commonmark can only scale the body font for H2 and below, never
    /// switch it to Cormorant, so the headings are drawn here in the README
    /// scale (H1 22, H2 16) and only the text between them goes through it.
    pub fn show(&mut self, ui: &mut Ui, source: &str) {
        let tokens = Tokens::get(ui.ctx());
        ui.scope(|ui| {
            let style = ui.style_mut();
            style.text_styles.insert(TextStyle::Body, theme::lora(13.0));
            style
                .text_styles
                .insert(TextStyle::Monospace, theme::mono(12.0));
            style
                .text_styles
                .insert(TextStyle::Heading, theme::cormorant(22.0));
            ui.spacing_mut().item_spacing.y = 8.0;
            for (index, block) in split_headings(source).into_iter().enumerate() {
                match block {
                    Block::Heading(1, heading) => {
                        text::single(
                            ui,
                            text::job(
                                heading,
                                theme::cormorant(22.0),
                                tokens.text_primary,
                                Some(24.0),
                            ),
                        );
                    }
                    Block::Heading(_, heading) => {
                        ui.add_space(4.0);
                        text::single(
                            ui,
                            text::job(heading, theme::cormorant(16.0), tokens.text_primary, None),
                        );
                    }
                    Block::Markdown(markdown) => {
                        ui.push_id(index, |ui| {
                            egui_commonmark::CommonMarkViewer::new().show(
                                ui,
                                &mut self.0,
                                &markdown,
                            );
                        });
                    }
                }
            }
        });
    }

    /// The bordered box the README sits in on a plugin's page and in the store.
    pub fn boxed(&mut self, ui: &mut Ui, source: &str) {
        let tokens = Tokens::get(ui.ctx());
        egui::Frame::NONE
            .stroke(theme::stroke(ui.ctx(), 1.0, tokens.border))
            .corner_radius(4)
            .inner_margin(Margin::symmetric(20, 16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.show(ui, source);
            });
    }
}

#[derive(Default)]
pub struct SettingsState {
    pub page: Page,
    pub open_plugin: Option<String>,
    pub capture: Option<capture::Session>,
    pub readme: Readme,
    pub store: store::Store,
    pub update: about::Update,
}

impl SettingsState {
    pub fn begin_capture(&mut self, owner: String) {
        hotkey_capture::stop();
        self.capture = Some(capture::Session {
            owner,
            pending: None,
        });
        hotkey_capture::start();
    }

    pub fn end_capture(&mut self) {
        hotkey_capture::stop();
        self.capture = None;
    }
}

/// What the capture box says under a combination. WinCraft's own bindings
/// come from the snapshot; everything else from a live probe, which is plain
/// Win32 and safe to run on this thread.
pub(crate) fn verdict(hotkey: Hotkey, snapshot: &UiSnapshot, tokens: &Tokens) -> Verdict {
    let mine: Vec<(String, String, Hotkey)> = snapshot
        .plugins
        .iter()
        .flat_map(|plugin| {
            plugin.hotkeys.iter().filter_map(move |info| {
                hotkeys::parse(&info.binding)
                    .ok()
                    .map(|parsed| (plugin.name.clone(), info.label.clone(), parsed))
            })
        })
        .collect();
    let entry = probe::verdict(std::ptr::null_mut(), hotkey, &mine);
    let colour = match entry.status {
        Status::Free => tokens.success,
        Status::TakenByApp => tokens.warning,
        Status::Windows | Status::WinCraft => tokens.info,
    };
    Verdict {
        text: probe::verdict_text(&entry),
        colour,
    }
}

/// Paths are long; "%LOCALAPPDATA%\WinCraft\config.json" says the same in a
/// third of the width.
pub(crate) fn display_path(path: &std::path::Path) -> String {
    let full = path.display().to_string();
    match std::env::var("LOCALAPPDATA") {
        Ok(base) if !base.is_empty() && full.starts_with(&base) => {
            format!("%LOCALAPPDATA%{}", &full[base.len()..])
        }
        _ => full,
    }
}

/// Title row, optional description and the spacing the handout puts under
/// them. `right` is laid out right to left, so its first widget is rightmost.
pub(crate) fn page_header(
    ui: &mut Ui,
    title: &str,
    meta: Option<&str>,
    description: &str,
    right: impl FnOnce(&mut Ui),
) {
    ui.horizontal(|ui| {
        text::title(ui, title, meta);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
    if !description.is_empty() {
        ui.add_space(6.0);
        text::page_description(ui, description);
    }
}

/// A titled section: header 20 below the previous content, rows 6 below it.
pub(crate) fn section_header(ui: &mut Ui, title: &str) {
    ui.add_space(20.0);
    text::group_header(ui, title);
    ui.add_space(6.0);
}

/// The deferred viewport callback must be Send + Sync + 'static, so everything
/// it draws lives behind one lock instead of being borrowed from the app.
pub struct Settings {
    shared: Arc<Mutex<Shared>>,
}

struct Shared {
    state: SettingsState,
    snapshot: UiSnapshot,
    to_host: Arc<HostChannel>,
    closed: bool,
}

impl Settings {
    pub fn new(to_host: Arc<HostChannel>, snapshot: UiSnapshot) -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                state: SettingsState::default(),
                snapshot,
                to_host,
                closed: false,
            })),
        }
    }

    pub fn set_snapshot(&self, snapshot: UiSnapshot) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.snapshot = snapshot;
        }
    }

    pub fn open_at(&self, page: Page) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.state.page = page;
            shared.state.open_plugin = None;
            shared.closed = false;
        }
    }

    pub fn was_closed(&self) -> bool {
        self.shared
            .lock()
            .map(|shared| shared.closed)
            .unwrap_or(false)
    }

    pub fn hidden(&self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.state.end_capture();
            shared.closed = false;
        }
    }

    pub fn show(&self, ctx: &egui::Context, icon: Option<Arc<egui::IconData>>) {
        let shared = Arc::clone(&self.shared);
        let mut builder = egui::ViewportBuilder::default()
            .with_title("WinCraft")
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([720.0, 480.0]);
        if let Some(icon) = icon {
            builder = builder.with_icon(icon);
        }

        ctx.show_viewport_deferred(viewport_id(), builder, move |ui, _class| {
            focus::track(ui.ctx());
            let Ok(mut shared) = shared.lock() else {
                return;
            };
            let Shared {
                state,
                snapshot,
                to_host,
                closed,
            } = &mut *shared;
            let tokens = Tokens::get(ui.ctx());

            // 200 wide with 32 px page margins from 960 up; at the 720 minimum
            // the handout narrows the nav to 168 and the margins to 24.
            let wide = ui.max_rect().width() >= 960.0;
            let nav_width = if wide { 200.0 } else { 168.0 };
            let side = if wide { 32 } else { 24 };

            egui::Panel::left("wincraft-nav")
                .resizable(false)
                .exact_size(nav_width)
                .frame(
                    egui::Frame::NONE
                        .fill(tokens.window_bg)
                        .inner_margin(Margin {
                            left: 20,
                            right: 12,
                            top: 8,
                            bottom: 16,
                        }),
                )
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        text::single(
                            ui,
                            text::job(
                                "WinCraft",
                                theme::cormorant(26.0),
                                tokens.text_primary,
                                Some(26.0),
                            ),
                        );
                    });
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        text::mono(
                            ui,
                            &format!("v{}", snapshot.version),
                            11.0,
                            tokens.text_disabled,
                        );
                    });
                    ui.add_space(20.0);
                    for (page, label) in PAGES {
                        if row::nav_item(ui, label, state.page == *page).clicked() {
                            state.page = *page;
                            state.open_plugin = None;
                            state.end_capture();
                        }
                    }
                });

            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(tokens.window_bg)
                        .inner_margin(Margin {
                            left: side,
                            right: side,
                            top: 24,
                            bottom: 24,
                        }),
                )
                .show(ui, |ui| match state.page {
                    Page::General => general::show(ui, snapshot, state, to_host),
                    Page::Plugins => plugins::show(ui, snapshot, state, to_host),
                    Page::Store => store::show(ui, snapshot, state, to_host),
                    Page::About => about::show(ui, snapshot, state),
                });

            if ui.ctx().input(|i| i.viewport().close_requested()) {
                *closed = true;
            }
        });
    }
}

const PAGES: &[(Page, &str)] = &[
    (Page::General, "General"),
    (Page::Plugins, "Plugins"),
    (Page::Store, "Plugin Store"),
    (Page::About, "About"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_split_out_and_code_fences_stay_whole() {
        let source =
            "# Title\n\nIntro text.\n\n## Settings\n\n```\n# not a heading\n```\n\nMore.\n";
        let blocks = split_headings(source);
        let kinds: Vec<String> = blocks
            .iter()
            .map(|block| match block {
                Block::Heading(level, text) => format!("h{level}:{text}"),
                Block::Markdown(text) => format!("md:{}", text.trim().lines().count()),
            })
            .collect();
        assert_eq!(kinds, vec!["h1:Title", "md:1", "h2:Settings", "md:5"]);
    }

    #[test]
    fn a_heading_needs_a_space_after_its_hashes() {
        let blocks = split_headings("#hashtag\n");
        assert!(matches!(blocks.as_slice(), [Block::Markdown(_)]));
    }
}
