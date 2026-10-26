mod about;
mod bookmarks;
mod catalog;
mod general;
mod hotkey_status;
mod layout;
mod plugins;
mod shortcuts;
mod store;
mod themes;

use std::sync::{Arc, Mutex};

use egui::{Margin, TextStyle, Ui};
use windows_sys::core::BOOL;
use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE,
};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::WindowsAndMessaging::{EnumThreadWindows, GetWindowTextW};

use crate::core::hotkeys;
use crate::core::theme::{self, Tokens};
use crate::core::ui_bridge::{HostChannel, HostRequest, LayoutSnapshot, Page, UiSnapshot};
use crate::ui::widgets::hotkey_dialog::{self, Dialog, Editor, Outcome, Target};
use crate::ui::widgets::{focus, row, text, theme_tile};

/// A window of this thread by its title. The palette and the settings window
/// are both titled "WinCraft" and winit keeps WS_CAPTION on the undecorated
/// palette too, so the palette is skipped by its handle.
pub(crate) fn find_window(title: &str, palette: isize) -> isize {
    struct Search<'a> {
        title: &'a str,
        skip: HWND,
        found: HWND,
    }

    unsafe extern "system" fn find(hwnd: HWND, search: LPARAM) -> BOOL {
        let search = unsafe { &mut *(search as *mut Search) };
        let mut title = [0u16; 64];
        let len = unsafe { GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32) };
        let titled = String::from_utf16_lossy(&title[..len.max(0) as usize]) == search.title;
        if hwnd != search.skip && titled {
            search.found = hwnd;
            return 0;
        }
        1
    }

    let mut search = Search {
        title,
        skip: palette as HWND,
        found: std::ptr::null_mut(),
    };
    unsafe {
        EnumThreadWindows(
            GetCurrentThreadId(),
            Some(find),
            &mut search as *mut Search as LPARAM,
        );
    }
    search.found as isize
}

/// The OS title bar follows the Windows app mode, which left a dark bar over
/// a light page. winit's own theme switch does not reach the Windows 11
/// caption, and Windows puts the dark caption back when the window is shown
/// or activated, so the attribute is checked on every frame, not set once.
pub(crate) fn sync_title_bar(hwnd: isize, dark: bool) {
    let hwnd = hwnd as HWND;
    let mut current: BOOL = 0;
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            &mut current as *mut BOOL as *mut core::ffi::c_void,
            std::mem::size_of::<BOOL>() as u32,
        );
    }
    if (current != 0) == dark {
        return;
    }
    let value = BOOL::from(dark);
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
            &value as *const BOOL as *const core::ffi::c_void,
            std::mem::size_of::<BOOL>() as u32,
        );
    }
}

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

#[derive(Default)]
pub struct Readme(egui_commonmark::CommonMarkCache);

impl Readme {
    /// egui_commonmark can only scale the body font for headings, never
    /// switch it to the semibold family, so the headings are drawn here in the
    /// README scale (H1 20, H2 15, both semibold) and only the text between
    /// them goes through it.
    pub fn show(&mut self, ui: &mut Ui, source: &str) {
        let tokens = Tokens::get(ui.ctx());
        ui.scope(|ui| {
            let style = ui.style_mut();
            style
                .text_styles
                .insert(TextStyle::Body, theme::regular(13.0));
            style
                .text_styles
                .insert(TextStyle::Monospace, theme::mono(12.0));
            style
                .text_styles
                .insert(TextStyle::Heading, theme::semibold(20.0));
            ui.spacing_mut().item_spacing.y = 8.0;
            for (index, block) in split_headings(source).into_iter().enumerate() {
                match block {
                    Block::Heading(1, heading) => {
                        text::single(
                            ui,
                            text::job(
                                heading,
                                theme::semibold(20.0),
                                tokens.text_primary,
                                Some(26.0),
                            ),
                        );
                    }
                    Block::Heading(_, heading) => {
                        ui.add_space(4.0);
                        text::single(
                            ui,
                            text::job(
                                heading,
                                theme::semibold(15.0),
                                tokens.text_primary,
                                Some(20.0),
                            ),
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
            .stroke(theme::line(ui.ctx(), tokens.border))
            .corner_radius(tokens.radius_row)
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
    /// The Plugins page's search and category chips.
    pub plugins_view: catalog::View,
    /// The hotkey dialog, over whichever page opened it.
    pub hotkey_dialog: Option<Editor>,
    /// A test scene's dialog, opened once its command is in the snapshot:
    /// the command's id and the keys to show as pressed.
    pub scene_dialog: Option<(String, Option<String>)>,
    pub readme: Readme,
    pub store: store::Store,
    pub update: about::Update,
    /// When "Clear usage history" was last clicked, for its "Cleared" note.
    pub usage_cleared: Option<std::time::Instant>,
    /// When "Show all tips again" was last clicked, for its "Done" note.
    pub tips_reset: Option<std::time::Instant>,
    /// The saved windows the Layout page shows, as LayoutKeeper last sent
    /// them, and when the page last asked for them.
    pub layout: Option<LayoutSnapshot>,
    pub layout_asked: Option<std::time::Instant>,
    /// The Layout page's "Recent activity" list.
    pub log: layout::LogView,
    /// The Bookmarks page's form boxes and what it knows about the places.
    pub bookmarks: bookmarks::BookmarksView,
    /// The theme the General page's tiles are previewing.
    pub theme_preview: theme_tile::HoverPreview,
    /// The Hotkeys page's search, filter and recording.
    pub shortcuts: shortcuts::View,
}

impl SettingsState {
    /// Opens the hotkey dialog on `target`; it records at once.
    pub fn open_hotkey(&mut self, target: Target) {
        // Search by keys shares the keyboard hook, and the old dialog's
        // drop removes the hook, so both go before the new one starts it.
        self.shortcuts.leave();
        self.hotkey_dialog = None;
        self.hotkey_dialog = Some(Editor::open(target));
    }

    pub fn close_hotkey(&mut self) {
        self.hotkey_dialog = None;
    }

    /// The dialog over the page, and the requests that open it.
    fn draw_hotkey_dialog(
        &mut self,
        ctx: &egui::Context,
        snapshot: &UiSnapshot,
        to_host: &HostChannel,
    ) {
        if let Some((id, keys)) = self.scene_dialog.take() {
            match Target::find(&snapshot.shortcuts, &id) {
                Some(target) => self.hotkey_dialog = Some(scene_editor(target, keys, snapshot)),
                None => self.scene_dialog = Some((id, keys)),
            }
        }
        if let Some(id) = self.shortcuts.take_dialog_request() {
            if let Some(target) = Target::find(&snapshot.shortcuts, &id) {
                self.open_hotkey(target);
            }
        }
        let Some(editor) = self.hotkey_dialog.as_mut() else {
            return;
        };
        let cover = ctx.content_rect();
        if let Some(outcome) = hotkey_dialog::show(ctx, editor, &snapshot.shortcuts, cover, 0) {
            if let Outcome::Save(changes) = outcome {
                to_host.send(HostRequest::SetShortcuts(changes));
            }
            self.close_hotkey();
        }
    }
}

/// A test scene's dialog: no keyboard hook, and the keys shown as if
/// pressed, so a picture can show a conflict or taken keys.
fn scene_editor(target: Target, keys: Option<String>, snapshot: &UiSnapshot) -> Editor {
    let mut dialog = Dialog::new(target.clone());
    if let Some(keys) = keys.and_then(|text| hotkeys::parse_shortcut(&text).ok()) {
        let now = std::time::Instant::now();
        let judge = |pressed: &hotkeys::Shortcut| {
            hotkey_dialog::verdict(pressed, &target, &snapshot.shortcuts)
        };
        dialog.press(keys.first, now, judge);
        if let Some(second) = keys.second {
            dialog.press(second, now, judge);
        }
    }
    Editor::preview(dialog)
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
    palette_hwnd: isize,
}

struct Shared {
    state: SettingsState,
    snapshot: UiSnapshot,
    to_host: Arc<HostChannel>,
    closed: bool,
    /// The settings window's HWND, looked up again on every open because a
    /// closed viewport's window is destroyed.
    window: isize,
}

impl Settings {
    pub fn new(to_host: Arc<HostChannel>, snapshot: UiSnapshot, palette_hwnd: isize) -> Self {
        Self {
            palette_hwnd,
            shared: Arc::new(Mutex::new(Shared {
                state: SettingsState::default(),
                snapshot,
                to_host,
                closed: false,
                window: 0,
            })),
        }
    }

    pub fn set_snapshot(&self, snapshot: UiSnapshot) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.snapshot = snapshot;
        }
    }

    pub fn set_layout(&self, layout: Option<LayoutSnapshot>) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.state.layout = layout;
        }
    }

    pub fn open_at(&self, page: Page) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.state.page = page;
            shared.state.open_plugin = None;
            shared.closed = false;
            shared.window = 0;
        }
    }

    /// A test scene: the Hotkeys page with the dialog open on a command,
    /// and the keys it shows as pressed.
    pub fn open_hotkey_scene(&self, id: &str, keys: Option<String>) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.state.page = Page::Shortcuts;
            shared.state.open_plugin = None;
            shared.state.scene_dialog = Some((id.to_string(), keys));
            shared.closed = false;
            shared.window = 0;
        }
    }

    /// The palette's "Change hotkey…" when its command is not in the
    /// snapshot: Hotkeys on that command.
    pub fn open_shortcut(&self, id: &str, label: &str) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.state.page = Page::Shortcuts;
            shared.state.open_plugin = None;
            shared.state.shortcuts.open_at(id, label);
            shared.closed = false;
            shared.window = 0;
        }
    }

    /// Used when the palette runs a command whose plugin is off: its page is
    /// where the plugin can be switched back on.
    pub fn open_plugin(&self, id: &str) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.state.page = Page::Plugins;
            shared.state.open_plugin = Some(id.to_string());
            shared.closed = false;
            shared.window = 0;
        }
    }

    pub fn was_closed(&self) -> bool {
        self.shared
            .lock()
            .map(|shared| shared.closed)
            .unwrap_or(false)
    }

    /// The caller also ends a theme preview, which needs the egui context.
    pub fn hidden(&self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.state.close_hotkey();
            shared.state.shortcuts.leave();
            shared.state.theme_preview = theme_tile::HoverPreview::default();
            shared.closed = false;
            shared.window = 0;
        }
    }

    pub fn show(&self, ctx: &egui::Context, icon: Option<Arc<egui::IconData>>) {
        let shared = Arc::clone(&self.shared);
        let palette_hwnd = self.palette_hwnd;
        let mut builder = egui::ViewportBuilder::default()
            .with_title("WinCraft")
            .with_inner_size([1024.0, 640.0])
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
                window,
            } = &mut *shared;
            let tokens = Tokens::get(ui.ctx());

            if *window == 0 {
                *window = find_window("WinCraft", palette_hwnd);
            }
            if *window != 0 {
                sync_title_bar(*window, tokens.dark);
            }

            // The design page draws a 240 px nav with 44/36 px page margins. The
            // window opens at 1024 so pages keep the width they had with the
            // old 200 px nav; below 960 the nav narrows to 168 and the margins to 24.
            let wide = ui.max_rect().width() >= 960.0;
            let nav_width = if wide { 240.0 } else { 168.0 };
            let side = if wide { 44 } else { 24 };
            let top = if wide { 36 } else { 24 };

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
                                theme::semibold(20.0),
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
                            13.0,
                            tokens.text_disabled,
                        );
                    });
                    ui.add_space(20.0);
                    for (page, label) in PAGES {
                        if row::nav_item(ui, label, state.page == *page).clicked() {
                            state.page = *page;
                            state.open_plugin = None;
                            state.close_hotkey();
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
                            top,
                            bottom: top,
                        }),
                )
                .show(ui, |ui| match state.page {
                    Page::General => general::show(ui, snapshot, state, to_host),
                    Page::Themes => themes::show(ui, snapshot, state, to_host),
                    Page::Plugins => plugins::show(ui, snapshot, state, to_host),
                    Page::Shortcuts => {
                        state.shortcuts.editing = state.hotkey_dialog.is_some();
                        shortcuts::show(ui, snapshot, &mut state.shortcuts, to_host)
                    }
                    Page::Layout => layout::show(ui, snapshot, state, to_host),
                    Page::Bookmarks => bookmarks::show(ui, snapshot, state, to_host),
                    Page::Store => store::show(ui, snapshot, state, to_host),
                    Page::About => about::show(ui, snapshot, state, to_host),
                });
            if state.page != Page::Shortcuts {
                state.shortcuts.leave();
            }
            state.draw_hotkey_dialog(ui.ctx(), snapshot, to_host);
            // The tiles end their own preview when the pointer leaves them,
            // but not when the page they are on is no longer drawn.
            if state.page != Page::Themes && state.theme_preview.shown().is_some() {
                state.theme_preview = theme_tile::HoverPreview::default();
                theme::preview(ui.ctx(), None);
            }

            // After the page, so a tile grid that has focus takes the arrows
            // first. Typing in a box or recording a hotkey keeps them too.
            let free = !ui.ctx().egui_wants_keyboard_input() && state.hotkey_dialog.is_none();
            let step = ui.ctx().input(|i| {
                if !free || i.modifiers.any() {
                    0
                } else if i.key_pressed(egui::Key::ArrowDown) {
                    1
                } else if i.key_pressed(egui::Key::ArrowUp) {
                    -1
                } else {
                    0
                }
            });
            if let Some(page) = next_page(state.page, step) {
                state.page = page;
                state.open_plugin = None;
                ui.ctx().request_repaint();
            }

            if ui.ctx().input(|i| i.viewport().close_requested()) {
                *closed = true;
            }
        });
    }
}

const PAGES: &[(Page, &str)] = &[
    (Page::General, "General"),
    (Page::Themes, "Themes"),
    (Page::Plugins, "Plugins"),
    (Page::Shortcuts, "Hotkeys"),
    (Page::Layout, "Layout"),
    (Page::Bookmarks, "Bookmarks"),
    (Page::Store, "Plugin Store"),
    (Page::About, "About"),
];

/// The page `step` places down (1) or up (-1) the side menu, stopping at
/// either end; None when nothing changes.
fn next_page(current: Page, step: i32) -> Option<Page> {
    if step == 0 {
        return None;
    }
    let at = PAGES.iter().position(|(page, _)| *page == current)? as i32;
    let to = (at + step).clamp(0, PAGES.len() as i32 - 1);
    (to != at).then(|| PAGES[to as usize].0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_side_menu_keeps_its_order_and_calls_the_page_hotkeys() {
        let labels: Vec<&str> = PAGES.iter().map(|(_, label)| *label).collect();
        assert_eq!(
            labels,
            [
                "General",
                "Themes",
                "Plugins",
                "Hotkeys",
                "Layout",
                "Bookmarks",
                "Plugin Store",
                "About"
            ]
        );
    }

    #[test]
    fn the_arrows_walk_the_side_menu_and_stop_at_its_ends() {
        assert_eq!(next_page(Page::General, 1), Some(Page::Themes));
        assert_eq!(next_page(Page::Themes, -1), Some(Page::General));
        assert_eq!(next_page(Page::Plugins, -1), Some(Page::Themes));
        assert_eq!(next_page(Page::General, -1), None);
        assert_eq!(next_page(Page::About, 1), None);
        assert_eq!(next_page(Page::Layout, 0), None);
    }

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
    fn a_scene_opens_the_dialog_once_its_command_is_listed_and_shows_its_keys() {
        use crate::core::ui_bridge::ShortcutInfo;
        use crate::ui::widgets::hotkey_dialog::{Phase, Verdict};
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let bridge = crate::core::ui_bridge::create();
        let mut state = SettingsState {
            scene_dialog: Some(("wincraft.palette".to_string(), Some("Win+L".to_string()))),
            ..SettingsState::default()
        };
        let mut snapshot = UiSnapshot::default();
        let run = |state: &mut SettingsState, snapshot: &UiSnapshot| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                state.draw_hotkey_dialog(ui.ctx(), snapshot, &bridge.to_host);
            });
            output.textures_delta.clear();
        };
        run(&mut state, &snapshot);
        assert!(state.hotkey_dialog.is_none(), "waits for the snapshot");
        assert!(state.scene_dialog.is_some());
        snapshot.shortcuts.push(ShortcutInfo {
            id: "wincraft.palette".to_string(),
            command: None,
            group: "WinCraft".to_string(),
            plugin: None,
            category: String::new(),
            icon: None,
            label: "Open the palette".to_string(),
            description: String::new(),
            binding: "Win+Alt+P".to_string(),
            default_binding: "Win+Alt+P".to_string(),
            registered: true,
            one_press: true,
        });
        run(&mut state, &snapshot);
        run(&mut state, &snapshot);
        let editor = state.hotkey_dialog.as_ref().expect("the scene's dialog");
        assert!(matches!(
            editor.dialog().phase(),
            Phase::Captured {
                verdict: Verdict::Taken { .. },
                ..
            }
        ));
        assert!(bridge.host_rx.try_recv().is_err(), "a scene saves nothing");
        state.close_hotkey();
        assert!(state.hotkey_dialog.is_none());
    }

    #[test]
    fn a_heading_needs_a_space_after_its_hashes() {
        let blocks = split_headings("#hashtag\n");
        assert!(matches!(blocks.as_slice(), [Block::Markdown(_)]));
    }
}
