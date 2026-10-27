mod arrange;
pub(crate) mod dwm_thumbs;
pub mod fuzzy;
mod palette;
mod peek;
pub(crate) mod settings;
pub mod taskbar_badges;
pub(crate) mod widgets;

use std::sync::mpsc::Receiver;
use std::sync::Arc;

use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::core::open_timing::{self, OpenStep};
use crate::core::theme;
use crate::core::traits::{PluginWindow, WindowFrame};
use crate::core::ui_bridge::{
    HostChannel, HostRequest, MonitorRect, UiChannel, UiCommand, UiSnapshot,
};
use crate::search::Router;
use crate::ui::arrange::Arrange;
use crate::ui::palette::{Outcome, Palette};
use crate::ui::settings::Settings;

pub fn start(
    rx: Receiver<UiCommand>,
    to_ui: Arc<UiChannel>,
    to_host: Arc<HostChannel>,
    snapshot: UiSnapshot,
    router: Router,
    windows: Vec<(String, Box<dyn PluginWindow>)>,
) -> Option<std::thread::JoinHandle<()>> {
    let started = std::thread::Builder::new()
        .name("wincraft-ui".to_string())
        .spawn(move || run(rx, to_ui, to_host, snapshot, router, windows));
    match started {
        Ok(thread) => Some(thread),
        Err(err) => {
            log::error!("could not start the UI thread: {err}");
            None
        }
    }
}

fn run(
    rx: Receiver<UiCommand>,
    to_ui: Arc<UiChannel>,
    to_host: Arc<HostChannel>,
    snapshot: UiSnapshot,
    router: Router,
    windows: Vec<(String, Box<dyn PluginWindow>)>,
) {
    // The palette is the root viewport because only the root exposes a Win32
    // handle, which the host needs for SetForegroundWindow and to switch off the
    // system frame. It starts hidden so the first press of the hotkey is instant.
    let viewport = egui::ViewportBuilder::default()
        .with_title("WinCraft")
        .with_visible(false)
        .with_decorations(false)
        .with_transparent(true)
        .with_window_level(egui::WindowLevel::AlwaysOnTop)
        .with_resizable(false)
        .with_taskbar(false)
        .with_active(true)
        .with_inner_size([palette::WIDTH, palette::HEIGHT]);

    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        event_loop_builder: Some(Box::new(|builder| {
            use winit::platform::windows::EventLoopBuilderExtWindows;
            builder.with_any_thread(true);
        })),
        ..Default::default()
    };

    let result = eframe::run_native(
        "WinCraft",
        options,
        Box::new(move |cc| {
            to_ui.attach(cc.egui_ctx.clone());
            theme::install_fonts(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx, snapshot.theme, snapshot.mode);
            let palette_hwnd = report_palette_window(cc, &to_host);
            Ok(Box::new(App::new(
                rx,
                to_host,
                snapshot,
                palette_hwnd,
                router,
                windows,
            )))
        }),
    );
    if let Err(err) = result {
        log::error!("the UI thread stopped: {err}");
    }
}

fn report_palette_window(cc: &eframe::CreationContext<'_>, to_host: &Arc<HostChannel>) -> isize {
    let handle = match cc.window_handle() {
        Ok(handle) => handle,
        Err(err) => {
            log::warn!("no window handle for the palette: {err}");
            return 0;
        }
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return 0;
    };
    let palette_hwnd = isize::from(win32.hwnd);
    log::info!("palette window ready");
    to_host.send(HostRequest::UiReady { palette_hwnd });
    palette_hwnd
}

struct App {
    /// The palette's HWND, and its own style while the glass is on.
    palette_window: isize,
    palette_style: Option<isize>,
    rx: Receiver<UiCommand>,
    to_host: Arc<HostChannel>,
    snapshot: UiSnapshot,
    palette: Palette,
    palette_visible: bool,
    pending_position: Option<MonitorRect>,
    settings_open: bool,
    settings: Settings,
    arrange_open: bool,
    arrange: Arrange,
    quitting: bool,
    applied_scale: f32,
    painted_once: bool,
    rehide_root: bool,
    closing: bool,
    appear: f32,
    icon: Arc<egui::IconData>,
    palette_hwnd: isize,
    plugin_windows: Vec<(String, Box<dyn PluginWindow>)>,
}

fn motion_id() -> egui::Id {
    egui::Id::new("wincraft-palette-motion")
}

/// The handout's app icon as raw pixels: eframe takes RGBA, and decoding a PNG
/// would need an image crate.
fn window_icon() -> Arc<egui::IconData> {
    Arc::new(egui::IconData {
        rgba: include_bytes!("../../assets/wincraft-64.rgba").to_vec(),
        width: 64,
        height: 64,
    })
}

impl App {
    fn new(
        rx: Receiver<UiCommand>,
        to_host: Arc<HostChannel>,
        snapshot: UiSnapshot,
        palette_hwnd: isize,
        router: Router,
        plugin_windows: Vec<(String, Box<dyn PluginWindow>)>,
    ) -> Self {
        let to_host_for_settings = Arc::clone(&to_host);
        let snapshot_for_settings = snapshot.clone();
        let mut palette = Palette::new(router);
        palette.set_window(palette_hwnd);
        Self {
            palette_window: palette_hwnd,
            palette_style: None,
            rx,
            to_host,
            snapshot,
            palette,
            palette_visible: false,
            pending_position: None,
            settings_open: false,
            arrange_open: false,
            arrange: Arrange::new(Arc::clone(&to_host_for_settings), palette_hwnd),
            settings: Settings::new(to_host_for_settings, snapshot_for_settings, palette_hwnd),
            quitting: false,
            applied_scale: 0.0,
            painted_once: false,
            rehide_root: false,
            closing: false,
            appear: 0.0,
            icon: window_icon(),
            palette_hwnd,
            plugin_windows,
        }
    }

    fn drain(&mut self, ctx: &egui::Context) {
        while let Ok(command) = self.rx.try_recv() {
            match command {
                UiCommand::ShowPalette(monitor) => {
                    self.update_glass(ctx);
                    self.palette.opened();
                    self.fit_palette(ctx, monitor);
                    self.palette_visible = true;
                    self.closing = false;
                    self.pending_position = Some(monitor);
                    self.set_palette_visible(ctx, true);
                    log::info!("palette shown");
                }
                UiCommand::ShowPrompt(prompt, monitor) => {
                    self.update_glass(ctx);
                    self.palette.opened();
                    self.fit_palette(ctx, monitor);
                    self.palette.ask(prompt);
                    self.palette_visible = true;
                    self.closing = false;
                    self.pending_position = Some(monitor);
                    self.set_palette_visible(ctx, true);
                    log::info!("palette shown with a question");
                }
                UiCommand::ShowSettings(page) => {
                    self.settings.open_at(page);
                    self.settings_open = true;
                    ctx.request_repaint();
                    log::info!("settings shown");
                }
                UiCommand::ShowArrange(snapshot) => {
                    open_timing::mark(OpenStep::UiGot);
                    self.arrange.open(snapshot);
                    self.arrange_open = true;
                    ctx.request_repaint();
                    log::info!("arrange windows shown");
                }
                UiCommand::ArrangeUpdate(snapshot) => {
                    self.arrange.update(snapshot);
                    ctx.request_repaint_of(arrange::viewport_id());
                }
                UiCommand::ArrangeSelect(place) => {
                    self.arrange.select_place(place);
                    ctx.request_repaint_of(arrange::viewport_id());
                }
                UiCommand::ArrangeSelectWindow(hwnd) => {
                    self.arrange.select_window(hwnd);
                    ctx.request_repaint_of(arrange::viewport_id());
                }
                UiCommand::ArrangeSelectNext => {
                    self.arrange.select_next();
                    ctx.request_repaint_of(arrange::viewport_id());
                }
                UiCommand::ArrangeNote(text) => {
                    self.arrange.note(text);
                    ctx.request_repaint_of(arrange::viewport_id());
                }
                UiCommand::HideArrange => {
                    if self.arrange_open {
                        self.arrange_open = false;
                        self.arrange.hidden();
                        log::info!("arrange windows hidden");
                    }
                }
                UiCommand::LayoutUpdate(layout) => {
                    self.settings.set_layout(layout);
                    ctx.request_repaint_of(settings::viewport_id());
                }
                UiCommand::PluginWindow {
                    plugin,
                    window,
                    open,
                } => {
                    for (_, found) in self
                        .plugin_windows
                        .iter_mut()
                        .filter(|(id, found)| *id == plugin && found.id() == window)
                    {
                        if open {
                            found.open();
                        } else {
                            found.close();
                        }
                    }
                    ctx.request_repaint();
                }
                UiCommand::ClosePluginWindows(plugin) => {
                    for (_, found) in self
                        .plugin_windows
                        .iter_mut()
                        .filter(|(id, _)| *id == plugin)
                    {
                        found.close();
                    }
                    ctx.request_repaint();
                }
                UiCommand::ShowPluginPage(id) => {
                    self.settings.open_plugin(&id);
                    self.settings_open = true;
                    ctx.request_repaint();
                    log::info!("settings shown on the {id} page");
                }
                UiCommand::ShowHotkeyDialog { id, keys } => {
                    self.settings.open_hotkey_scene(&id, keys);
                    self.settings_open = true;
                    ctx.request_repaint();
                    log::info!("settings shown with the hotkey dialog on {id}");
                }
                UiCommand::TypeInPalette(text) => {
                    self.palette.type_in(&text);
                    ctx.request_repaint();
                    log::info!("palette query set to \"{text}\"");
                }
                UiCommand::ClearPaletteUsage => self.palette.clear_usage(),
                UiCommand::PeekCard(card) => {
                    self.arrange.peek_card(card);
                    ctx.request_repaint_of(arrange::viewport_id());
                }
                UiCommand::HideAll => {
                    self.hide_palette(ctx);
                    theme::preview(ctx, None);
                    self.settings_open = false;
                    self.arrange_open = false;
                    self.arrange.hidden();
                }
                UiCommand::Snapshot(snapshot) => {
                    self.settings.set_snapshot((*snapshot).clone());
                    // The settings window is its own viewport and sleeps when
                    // idle, so it must be told to draw the new values.
                    if self.settings_open {
                        ctx.request_repaint_of(settings::viewport_id());
                    }
                    // ThemeChanged reaches the UI before the snapshot that
                    // carries the new choice, so the choice is applied here.
                    let look = (snapshot.theme, snapshot.mode);
                    if look != (self.snapshot.theme, self.snapshot.mode) {
                        theme::apply(ctx, look.0, look.1);
                        self.update_glass(ctx);
                    }
                    self.snapshot = *snapshot;
                    self.palette.invalidate();
                }
                UiCommand::ThemeChanged => {
                    theme::apply(ctx, self.snapshot.theme, self.snapshot.mode);
                    self.update_glass(ctx);
                }
                UiCommand::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    fn set_palette_visible(&self, ctx: &egui::Context, visible: bool) {
        if visible {
            crate::core::glass::set_corners(self.palette_window, self.palette.glass());
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(visible));
        ctx.request_repaint();
    }

    /// Starts the 80 ms fade; the window is hidden when it reaches zero.
    fn begin_hide(&mut self, ctx: &egui::Context) {
        if self.palette_visible && !self.closing {
            self.closing = true;
            ctx.request_repaint();
        }
    }

    fn hide_palette(&mut self, ctx: &egui::Context) {
        if !self.palette_visible {
            return;
        }
        self.palette.flush_usage();
        self.palette.hidden();
        self.palette_visible = false;
        self.closing = false;
        self.set_palette_visible(ctx, false);
    }

    /// Puts Windows' glass behind the palette, or takes it away, when the
    /// theme or the Windows settings that decide it changed. Asked when the
    /// palette opens and when the theme or Windows' colours change, which
    /// covers the transparency switch; battery saver is seen at the next
    /// opening, the only time the glass can be seen anyway.
    fn update_glass(&mut self, ctx: &egui::Context) {
        let theme = theme::Tokens::get(ctx).theme;
        let wanted = self.palette_window != 0
            && crate::core::glass::allowed(theme, crate::core::glass::conditions());
        if wanted == self.palette.glass() {
            return;
        }
        crate::core::glass::set_palette(self.palette_window, wanted, &mut self.palette_style);
        self.palette.set_glass(wanted);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            self.palette.window_width(),
            self.palette.window_height(),
        )));
    }

    /// On a monitor narrower than 1100 px the inspector does not fit beside
    /// the list, so the window narrows to the list and the inspector opens
    /// over it.
    fn fit_palette(&mut self, ctx: &egui::Context, monitor: MonitorRect) {
        let scale = ctx.pixels_per_point().max(0.1);
        let narrow = palette::is_narrow((monitor.right - monitor.left) as f32 / scale);
        let before = self.palette.window_width();
        self.palette.set_narrow(narrow);
        if (self.palette.window_width() - before).abs() > f32::EPSILON {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                self.palette.window_width(),
                self.palette.window_height(),
            )));
        }
    }

    /// Placing the window through the builder is unreliable across monitors
    /// with different scaling, so the position is sent once the window exists.
    fn place_palette(&mut self, ctx: &egui::Context) {
        let Some(monitor) = self.pending_position.take() else {
            return;
        };
        let scale = ctx.pixels_per_point().max(0.1);
        let width = self.palette.window_width() * scale;
        let left = monitor.left as f32 + ((monitor.right - monitor.left) as f32 - width) / 2.0;
        // 18 % down is where the panel's top edge goes; the window starts the
        // shadow margin above it.
        let top = monitor.top as f32 + (monitor.bottom - monitor.top) as f32 * 0.18
            - self.palette.margin() * scale;
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            left / scale,
            top / scale,
        )));
        ctx.request_repaint();
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain(ctx);
        // Stroke widths are snapped to physical pixels when the theme is
        // applied, so a move to a monitor with other scaling re-applies it.
        let scale = ctx.pixels_per_point();
        if (scale - self.applied_scale).abs() > f32::EPSILON {
            self.applied_scale = scale;
            theme::apply(ctx, self.snapshot.theme, self.snapshot.mode);
        }
        if self.quitting {
            return;
        }
        self.place_palette(ctx);

        // Show: 120 ms fade and 4 px rise, easing out. Hide: 80 ms fade.
        let target = self.palette_visible && !self.closing;
        let (time, easing): (f32, fn(f32) -> f32) = if target {
            (0.12, egui::emath::easing::cubic_out)
        } else {
            (0.08, egui::emath::easing::linear)
        };
        self.appear = ctx.animate_bool_with_time_and_easing(motion_id(), target, time, easing);
        if self.closing && self.appear <= 0.0 {
            self.hide_palette(ctx);
        }

        if std::mem::take(&mut self.rehide_root) && !self.palette_visible {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            ctx.request_repaint();
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_palette(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // eframe shows the root window after its first painted frame, to avoid
        // a white flash at startup. The root is the palette and starts hidden,
        // so that first frame only comes when the settings window opens, and
        // the palette would then sit on screen, transparent but always on top,
        // catching clicks meant for the windows under it. eframe does this
        // once, so hiding it once on the next frame is enough. Its own
        // visibility flag cannot be used: it stays unknown on Windows.
        if !std::mem::replace(&mut self.painted_once, true) && !self.palette_visible {
            self.rehide_root = true;
            ctx.request_repaint();
        }

        if self.settings_open {
            if self.settings.was_closed() {
                self.settings_open = false;
                self.settings.hidden();
                theme::preview(&ctx, None);
            } else {
                self.settings.show(&ctx, Some(Arc::clone(&self.icon)));
            }
        }
        if self.arrange_open {
            if self.arrange.was_closed() {
                self.arrange_open = false;
                self.arrange.hidden();
            } else {
                self.arrange.show(&ctx, Some(Arc::clone(&self.icon)));
            }
        }

        if self
            .plugin_windows
            .iter()
            .any(|(_, window)| window.is_open())
        {
            let frame = WindowFrame {
                icon: Some(Arc::clone(&self.icon)),
                palette_hwnd: self.palette_hwnd,
                to_host: Arc::clone(&self.to_host),
            };
            for (_, window) in self.plugin_windows.iter_mut() {
                if window.is_open() {
                    window.show(&ctx, &frame);
                }
            }
        }

        if !self.palette_visible {
            return;
        }
        let interactive = !self.closing;
        match self
            .palette
            .show(ui, &self.snapshot, &self.to_host, self.appear, interactive)
        {
            Outcome::Stay => {}
            Outcome::Hide => self.begin_hide(&ctx),
            Outcome::OpenPlugin(id) => {
                self.settings.open_plugin(&id);
                self.settings_open = true;
                self.begin_hide(&ctx);
            }
            Outcome::OpenShortcut { id, label } => {
                self.settings.open_shortcut(&id, &label);
                self.settings_open = true;
                self.begin_hide(&ctx);
            }
            Outcome::OpenSettings(page) => {
                self.settings.open_at(page);
                self.settings_open = true;
                self.begin_hide(&ctx);
            }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.arrange.hidden();
        for (_, window) in self.plugin_windows.iter_mut() {
            window.close();
        }
        self.palette.flush_usage();
        if !self.quitting {
            self.to_host.send(HostRequest::Exit);
        }
    }
}
