//! The one place a hotkey is changed: a dialog over the window that records
//! keys as soon as it opens. General, the Hotkeys page, a plugin's page and
//! the palette's Ctrl+K all open it, and it saves through
//! `HostRequest::SetShortcuts` by the command's id.
//!
//! Keys come from `hotkey_capture`'s hook, which hides them from Windows and
//! from egui alike, so every key the dialog reacts to (Enter, Esc,
//! Backspace, Tab) arrives through the hook and is decided by `Dialog`,
//! which needs no window and is tested on its own.

use std::time::{Duration, Instant};

use egui::{pos2, vec2, Align, Color32, Id, Order, Rect, Response, Sense, StrokeKind, Ui, UiKind};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_NOREPEAT, MOD_SHIFT};

use crate::core::hotkeys::{self, Shortcut};
use crate::core::shortcuts::{self, Check, CHORD_WAIT};
use crate::core::theme::{self, Tokens};
use crate::core::traits::Hotkey;
use crate::core::ui_bridge::{ShortcutChange, ShortcutInfo};
use crate::plugins::shortcut_detector::{known, probe};
use crate::ui::widgets::button::{self, Kind};
use crate::ui::widgets::icons::{self, Icon};
use crate::ui::widgets::keycap::{self, Caps, Large};
use crate::ui::widgets::{focus, hotkey_capture, text};

const ENTER: u32 = 0x0D;
const BACKSPACE: u32 = 0x08;
const TAB: u32 = 0x09;
const SPACE: u32 = 0x20;
const ESCAPE: u32 = 0x1B;

/// The command whose hotkey is changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// Its id on the Hotkeys page and in config.json.
    pub id: String,
    /// Its name, the dialog's title.
    pub label: String,
    /// A second key press within the chord wait makes a chord.
    pub chords: bool,
    /// Backspace on an empty dialog removes the hotkey. The palette's own
    /// hotkey can only be changed, never removed.
    pub removable: bool,
}

impl Target {
    pub fn of(info: &ShortcutInfo) -> Self {
        Self {
            id: info.id.clone(),
            label: info.label.clone(),
            chords: !info.one_press,
            removable: !info.one_press,
        }
    }

    pub fn find(all: &[ShortcutInfo], id: &str) -> Option<Self> {
        all.iter().find(|info| info.id == id).map(Self::of)
    }
}

/// What the dialog says about the keys pressed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Free,
    /// Another WinCraft command has these keys; Move here takes them from it.
    Conflict {
        id: String,
        text: String,
    },
    /// Another app or Windows holds the keys, or WinCraft cannot use them.
    Taken {
        text: String,
    },
}

/// The dialog's buttons, shown only for a conflict or taken keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Cancel,
    MoveHere,
}

impl Button {
    fn label(self) -> &'static str {
        match self {
            Button::Cancel => "Cancel",
            Button::MoveHere => "Move here",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Send these as `HostRequest::SetShortcuts`.
    Save(Vec<ShortcutChange>),
    /// The old binding stays.
    Cancel,
}

/// What the keys row shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase<'a> {
    /// Nothing pressed yet: one dashed cap.
    Waiting,
    /// These modifiers are held, the key still to come.
    Holding(u32),
    Captured {
        keys: Shortcut,
        verdict: &'a Verdict,
    },
}

/// The dialog's state, without any window or hook.
#[derive(Clone, Debug)]
pub struct Dialog {
    target: Target,
    first: Option<Hotkey>,
    second: Option<Hotkey>,
    first_at: Option<Instant>,
    verdict: Verdict,
    held: u32,
    /// A short line after a key that needs a modifier.
    note: Option<String>,
    /// The button Tab moved to; None until Tab is pressed.
    focus: Option<Button>,
}

impl Dialog {
    pub fn new(target: Target) -> Self {
        Self {
            target,
            first: None,
            second: None,
            first_at: None,
            verdict: Verdict::Free,
            held: 0,
            note: None,
            focus: None,
        }
    }

    pub fn keys(&self) -> Option<Shortcut> {
        self.first.map(|first| Shortcut {
            first,
            second: self.second,
        })
    }

    pub fn phase(&self) -> Phase<'_> {
        match self.keys() {
            Some(keys) => Phase::Captured {
                keys,
                verdict: &self.verdict,
            },
            None if self.held != 0 => Phase::Holding(self.held),
            None => Phase::Waiting,
        }
    }

    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    pub fn focus(&self) -> Option<Button> {
        self.focus
    }

    pub fn buttons(&self) -> &'static [Button] {
        if self.first.is_none() {
            return &[];
        }
        match self.verdict {
            Verdict::Free => &[],
            Verdict::Conflict { .. } => &[Button::Cancel, Button::MoveHere],
            Verdict::Taken { .. } => &[Button::Cancel],
        }
    }

    /// Whether a second press would still make a chord.
    pub fn chord_open(&self, now: Instant) -> bool {
        self.target.chords
            && self.second.is_none()
            && self
                .first_at
                .is_some_and(|at| now.duration_since(at) < CHORD_WAIT)
    }

    /// How much of the chord wait is left, from 1 down to 0; None when no
    /// chord can follow.
    pub fn chord_left(&self, now: Instant) -> Option<f32> {
        if !self.chord_open(now) {
            return None;
        }
        let at = self.first_at?;
        let used = now.duration_since(at).as_secs_f32() / CHORD_WAIT.as_secs_f32();
        Some((1.0 - used).clamp(0.0, 1.0))
    }

    /// The modifiers held right now, from the hook.
    pub fn hold(&mut self, modifiers: u32) {
        self.held = modifiers & !MOD_NOREPEAT;
    }

    /// One key press from the hook. Enter, Backspace, Tab and Space alone
    /// are the dialog's own keys; everything else is recorded.
    pub fn press(
        &mut self,
        hotkey: Hotkey,
        now: Instant,
        judge: impl Fn(&Shortcut) -> Verdict,
    ) -> Option<Outcome> {
        let modifiers = hotkey.modifiers & !MOD_NOREPEAT;
        if modifiers == 0 {
            match hotkey.vk {
                ENTER => return self.enter(),
                BACKSPACE => return self.backspace(),
                ESCAPE => return Some(Outcome::Cancel),
                TAB => {
                    self.move_focus(true);
                    return None;
                }
                SPACE => {
                    if let Some(button) = self.focus {
                        return self.click(button);
                    }
                }
                _ => {}
            }
        }
        if modifiers == MOD_SHIFT && hotkey.vk == TAB && !self.buttons().is_empty() {
            self.move_focus(false);
            return None;
        }
        self.note = None;
        self.focus = None;
        if self.first.is_some() && self.chord_open(now) {
            self.second = Some(hotkey);
        } else if modifiers == 0 && !shortcuts::stands_alone(hotkey.vk) {
            self.clear();
            self.note = Some(format!(
                "{} needs Win, Ctrl or Alt with it.",
                hotkeys::format(hotkey)
            ));
            return None;
        } else {
            self.first = Some(hotkey);
            self.second = None;
            self.first_at = Some(now);
        }
        if let Some(keys) = self.keys() {
            self.verdict = judge(&keys);
        }
        None
    }

    /// A click on a button, or Space on the one Tab moved to.
    pub fn click(&mut self, button: Button) -> Option<Outcome> {
        match button {
            Button::Cancel => Some(Outcome::Cancel),
            Button::MoveHere => self.save(),
        }
    }

    fn enter(&mut self) -> Option<Outcome> {
        if self.focus == Some(Button::Cancel) {
            return Some(Outcome::Cancel);
        }
        self.save()
    }

    /// The changes to send, or None while the keys cannot be saved.
    fn save(&self) -> Option<Outcome> {
        let keys = self.keys()?;
        let changes = match &self.verdict {
            Verdict::Free => vec![ShortcutChange {
                id: self.target.id.clone(),
                binding: Some(hotkeys::format_shortcut(&keys)),
            }],
            Verdict::Conflict { id, .. } => shortcuts::replace(&self.target.id, &keys, id),
            Verdict::Taken { .. } => return None,
        };
        Some(Outcome::Save(changes))
    }

    /// Clears the keys pressed; with none pressed, removes the hotkey.
    fn backspace(&mut self) -> Option<Outcome> {
        if self.first.is_some() {
            self.clear();
            return None;
        }
        self.note = None;
        self.target.removable.then(|| {
            Outcome::Save(vec![ShortcutChange {
                id: self.target.id.clone(),
                binding: Some(String::new()),
            }])
        })
    }

    fn clear(&mut self) {
        self.first = None;
        self.second = None;
        self.first_at = None;
        self.verdict = Verdict::Free;
        self.focus = None;
        self.note = None;
    }

    fn move_focus(&mut self, forward: bool) {
        let buttons = self.buttons();
        if buttons.is_empty() {
            return;
        }
        let at = self
            .focus
            .and_then(|focus| buttons.iter().position(|button| *button == focus));
        let next = match (at, forward) {
            (None, true) => 0,
            (None, false) => buttons.len() - 1,
            (Some(at), true) => (at + 1) % buttons.len(),
            (Some(at), false) => (at + buttons.len() - 1) % buttons.len(),
        };
        self.focus = Some(buttons[next]);
    }
}

/// The dialog's words for a check's result. `windows` is what Windows
/// does with the first press, when it is a known Windows shortcut.
pub fn verdict_of(check: Check, windows: Option<&str>, target: &str) -> Verdict {
    let windows_text = || windows.map(|name| format!("Windows shortcut: {name}"));
    match check {
        Check::UsedBy {
            id, label, group, ..
        } => Verdict::Conflict {
            id,
            text: format!("Used by {group}: {label}. Saving removes it there."),
        },
        Check::HeldByApp { .. } => Verdict::Taken {
            text: windows_text().unwrap_or_else(|| "Taken by another app".to_string()),
        },
        Check::Reserved { ref keys } => Verdict::Taken {
            text: windows_text().unwrap_or_else(|| format!("Windows keeps {keys} for itself.")),
        },
        blocked if blocked.blocks() => Verdict::Taken {
            text: blocked
                .message(target)
                .map(|(line, _)| line)
                .unwrap_or_default(),
        },
        _ => Verdict::Free,
    }
}

/// The verdict on `keys` for `target`: WinCraft's own rules from
/// `core::shortcuts`, then a live probe of whether another app holds the
/// first press. The probe is plain Win32 and safe on the UI thread.
pub fn verdict(keys: &Shortcut, target: &Target, all: &[ShortcutInfo]) -> Verdict {
    let held = |hotkey: Hotkey| {
        probe::verdict(std::ptr::null_mut(), hotkey, &[]).status == probe::Status::TakenByApp
    };
    let check = shortcuts::check(keys, &target.id, all, held);
    verdict_of(check, known::meaning(keys.first), &target.label)
}

/// The dialog with the keyboard hook that feeds it. Dropping it removes
/// the hook, so no path can leave the user's keys swallowed.
pub struct Editor {
    dialog: Dialog,
    recording: bool,
}

impl Editor {
    /// Opens the dialog and starts recording at once.
    pub fn open(target: Target) -> Self {
        hotkey_capture::stop();
        hotkey_capture::start();
        Self {
            dialog: Dialog::new(target),
            recording: true,
        }
    }

    /// The dialog without the hook, for test scenes and tests: it shows,
    /// but no key reaches it.
    pub fn preview(dialog: Dialog) -> Self {
        Self {
            dialog,
            recording: false,
        }
    }

    #[cfg(test)]
    pub fn dialog(&self) -> &Dialog {
        &self.dialog
    }

    /// Takes what the hook recorded since the last frame.
    fn poll(&mut self, ctx: &egui::Context, all: &[ShortcutInfo]) -> Option<Outcome> {
        if !self.recording {
            return None;
        }
        // The hook runs between frames; without a timed repaint held
        // modifiers and the chord bar would wait for the mouse to move.
        ctx.request_repaint_after(Duration::from_millis(30));
        if hotkey_capture::take_cancelled() {
            return Some(Outcome::Cancel);
        }
        // The hook swallows keys system-wide, so it must not outlive the
        // window's focus.
        if ctx.input(|input| input.viewport().focused) == Some(false) {
            return Some(Outcome::Cancel);
        }
        self.dialog.hold(hotkey_capture::live_modifiers());
        let hotkey = hotkey_capture::take_result()?;
        let target = self.dialog.target.clone();
        self.dialog
            .press(hotkey, Instant::now(), |keys| verdict(keys, &target, all))
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        if self.recording {
            hotkey_capture::stop();
        }
    }
}

pub const WIDTH: f32 = 400.0;
const PADDING: f32 = 28.0;
const GAP: f32 = 18.0;
const KEYS_ROW: f32 = 52.0;
const BUTTONS_ROW: f32 = 32.0;
const RADIUS: u8 = 8;
/// The handout's backdrop, black at 55 % in every theme: it only dims what
/// is behind the dialog.
const BACKDROP: Color32 = Color32::from_black_alpha(140);
const HINT: &str = "Enter saves \u{00B7} Esc cancels \u{00B7} Backspace clears";
const CHORD_HINT: &str = "Enter saves \u{00B7} a second key makes a chord";

/// The pieces of the keys row for this phase.
pub fn pieces(dialog: &Dialog, now: Instant) -> Vec<Large> {
    let modifiers = |held: u32| -> Vec<Large> {
        hotkeys::format_modifiers(held)
            .split('+')
            .filter(|key| !key.is_empty())
            .map(|key| Large::Held(key.to_string()))
            .collect()
    };
    match dialog.phase() {
        Phase::Waiting => vec![Large::Waiting],
        Phase::Holding(held) => {
            let mut pieces = modifiers(held);
            pieces.extend([Large::Plus, Large::Waiting]);
            pieces
        }
        Phase::Captured { keys, .. } => {
            let mut pieces = Vec::new();
            for (at, step) in keys.steps().into_iter().enumerate() {
                if at > 0 {
                    pieces.push(Large::Then);
                }
                pieces.extend(step.into_iter().map(Large::Key));
            }
            if dialog.chord_open(now) {
                pieces.extend([Large::Then, Large::Waiting]);
            }
            pieces
        }
    }
}

/// The line under the keys: the warning for a conflict or taken keys, or
/// the note after a key that needs a modifier.
fn message(dialog: &Dialog) -> Option<(&str, bool)> {
    match dialog.phase() {
        Phase::Captured {
            verdict: Verdict::Conflict { text, .. } | Verdict::Taken { text },
            ..
        } => Some((text.as_str(), true)),
        Phase::Waiting => dialog.note().map(|note| (note, false)),
        _ => None,
    }
}

/// Draws the dialog centred over `cover` (the window, or the palette's
/// panel) with the backdrop over all of `cover`, and returns what the user
/// chose. `all` is every command, for the conflict check.
pub fn show(
    ctx: &egui::Context,
    editor: &mut Editor,
    all: &[ShortcutInfo],
    cover: Rect,
    cover_radius: u8,
) -> Option<Outcome> {
    if let Some(outcome) = editor.poll(ctx, all) {
        return Some(outcome);
    }
    let now = Instant::now();
    let area = egui::Area::new(Id::new("wincraft-hotkey-dialog"))
        .kind(UiKind::Modal)
        .order(Order::Foreground)
        .fixed_pos(cover.min)
        .constrain(false);
    ctx.memory_mut(|memory| memory.set_modal_layer(area.layer()));
    area.show(ctx, |ui| {
        let mut backdrop =
            ui.new_child(egui::UiBuilder::new().sense(Sense::click()).max_rect(cover));
        backdrop.set_min_size(cover.size());
        let backdrop = backdrop.response();
        ui.painter().rect_filled(cover, cover_radius, BACKDROP);
        let chosen = panel(ui, editor, cover, now);
        if backdrop.clicked() {
            return Some(Outcome::Cancel);
        }
        chosen.and_then(|button| editor.dialog.click(button))
    })
    .inner
}

fn panel(ui: &mut Ui, editor: &Editor, cover: Rect, now: Instant) -> Option<Button> {
    let tokens = Tokens::get(ui.ctx());
    let dialog = &editor.dialog;
    let inner_width = WIDTH - 2.0 * PADDING;
    let painter = ui.painter().clone();
    let centred = |text: &str, font: egui::FontId, colour: Color32, rows: usize| {
        let mut job = text::job(text, font, colour, None);
        job.halign = Align::Center;
        job.wrap = egui::text::TextWrapping {
            max_width: inner_width,
            max_rows: rows,
            break_anywhere: false,
            overflow_character: Some('\u{2026}'),
        };
        painter.layout_job(job)
    };
    let kicker = centred(
        "Change hotkey",
        theme::regular(12.0),
        tokens.text_disabled,
        1,
    );
    let title = centred(
        &dialog.target.label,
        theme::semibold(18.0),
        tokens.text_primary,
        2,
    );
    let message = message(dialog).map(|(line, warning)| {
        let colour = if warning {
            tokens.warning
        } else {
            tokens.text_secondary
        };
        centred(line, theme::regular(13.0), colour, 3)
    });
    let buttons = dialog.buttons();
    let hint = buttons.is_empty().then(|| {
        let line = if dialog.chord_open(now) {
            CHORD_HINT
        } else {
            HINT
        };
        centred(line, theme::regular(12.0), tokens.text_disabled, 2)
    });

    let mut height = 2.0 * PADDING + kicker.size().y + GAP + title.size().y + GAP + KEYS_ROW;
    if let Some(message) = &message {
        height += GAP + message.size().y;
    }
    if let Some(hint) = &hint {
        height += GAP + hint.size().y;
    }
    if !buttons.is_empty() {
        height += GAP + BUTTONS_ROW;
    }
    let rect = Rect::from_center_size(cover.center(), vec2(WIDTH, height));
    // Clicks on the panel's empty parts must not reach the backdrop.
    let _ = ui.interact(rect, ui.id().with("hotkey-dialog-panel"), Sense::click());
    painter.add(theme::window_shadow(&tokens).as_shape(rect, RADIUS));
    painter.rect(
        rect,
        RADIUS,
        tokens.panel_bg,
        theme::line(ui.ctx(), tokens.border),
        StrokeKind::Inside,
    );

    let x = rect.center().x;
    let mut y = rect.top() + PADDING;
    let place = |galley: std::sync::Arc<egui::Galley>, y: &mut f32| {
        let height = galley.size().y;
        painter.galley(pos2(x, *y), galley, Color32::PLACEHOLDER);
        *y += height + GAP;
    };
    place(kicker, &mut y);
    place(title, &mut y);
    let keys = Rect::from_min_size(pos2(rect.left() + PADDING, y), vec2(inner_width, KEYS_ROW));
    keycap::large(ui, keys, &pieces(dialog, now));
    if let Some(left) = dialog.chord_left(now) {
        let bar = Rect::from_center_size(
            pos2(x, keys.bottom() - 1.5),
            vec2(inner_width * 0.5 * left, 3.0),
        );
        ui.painter().rect_filled(bar, 1, tokens.accent);
    }
    y += KEYS_ROW + GAP;
    if let Some(message) = message {
        place(message, &mut y);
    }
    if let Some(hint) = hint {
        place(hint, &mut y);
    }
    if buttons.is_empty() {
        return None;
    }
    let widths: Vec<f32> = buttons
        .iter()
        .map(|button| button::width(ui, button.label()))
        .collect();
    let spacing = 8.0;
    let total = widths.iter().sum::<f32>() + spacing * (widths.len() - 1) as f32;
    let mut left = x - total / 2.0;
    let mut chosen = None;
    for (button, width) in buttons.iter().zip(widths) {
        let cell = Rect::from_min_size(pos2(left, y), vec2(width, BUTTONS_ROW));
        let kind = match button {
            Button::Cancel => Kind::Secondary,
            Button::MoveHere => Kind::Primary,
        };
        let response = ui
            .scope_builder(egui::UiBuilder::new().max_rect(cell), |ui| {
                button::button(ui, button.label(), kind)
            })
            .inner;
        if response.clicked() {
            chosen = Some(*button);
        }
        if dialog.focus() == Some(*button) {
            focus::paint(ui, response.rect, f32::from(tokens.radius_row));
        }
        left += width + spacing;
    }
    chosen
}

const TRIGGER_HEIGHT: f32 = 34.0;

/// A settings row's hotkey: the keys as caps in a 34 px box with a pencil,
/// or "Not set". It is the control: a click, or Enter or Space while it has
/// the keyboard, opens the dialog.
pub fn trigger(ui: &mut Ui, binding: &str) -> Response {
    trigger_box(ui, binding, Caps::page(), Sense::click(), false)
}

/// The trigger's box with any look and sense. On the Hotkeys page the
/// whole row is the control, so its box only senses hover and is lit
/// while the row is (`lit`).
pub fn trigger_box(ui: &mut Ui, binding: &str, look: Caps, sense: Sense, lit: bool) -> Response {
    let tokens = Tokens::get(ui.ctx());
    let keys = shortcuts::parse(binding);
    let steps = keys.map(|keys| keys.steps()).unwrap_or_default();
    let not_set = ui.painter().layout_no_wrap(
        "Not set".to_string(),
        theme::regular(13.0),
        tokens.text_disabled,
    );
    let pad = 6.0;
    let gap = 8.0;
    let pencil = 14.0;
    let caps_width = if steps.is_empty() {
        not_set.size().x
    } else {
        keycap::caps_width(ui, &steps, look)
    };
    let size = vec2(pad + caps_width + gap + pencil + pad + 4.0, TRIGGER_HEIGHT);
    let (rect, mut response) = ui.allocate_exact_size(size, sense);
    if sense.senses_click() {
        response = response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Change hotkey");
    }
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let hover =
        ui.ctx()
            .animate_bool_with_time(response.id.with("hover"), lit || response.hovered(), 0.08);
    ui.painter().rect(
        rect,
        tokens.radius_row,
        tokens.hover_bg.gamma_multiply(hover),
        theme::line(ui.ctx(), tokens.border),
        StrokeKind::Inside,
    );
    let inner = Rect::from_min_max(pos2(rect.left() + pad, rect.top()), rect.max);
    if steps.is_empty() {
        ui.painter().galley(
            pos2(inner.left(), rect.center().y - not_set.size().y / 2.0),
            not_set,
            tokens.text_disabled,
        );
    } else {
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(Align::Center)),
            |ui| keycap::caps(ui, &steps, look),
        );
    }
    let icon = Rect::from_center_size(
        pos2(rect.right() - pad - 2.0 - pencil / 2.0, rect.center().y),
        vec2(pencil, pencil),
    );
    icons::paint(ui.painter(), icon, Icon::Pencil, tokens.text_secondary);
    focus::ring(ui, &response, f32::from(tokens.radius_row));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_CONTROL, MOD_WIN};

    fn key(modifiers: u32, vk: u32) -> Hotkey {
        Hotkey {
            modifiers: modifiers | MOD_NOREPEAT,
            vk,
        }
    }

    fn target() -> Target {
        Target {
            id: "b".to_string(),
            label: "Save current layout".to_string(),
            chords: true,
            removable: true,
        }
    }

    fn free(_: &Shortcut) -> Verdict {
        Verdict::Free
    }

    fn conflict(_: &Shortcut) -> Verdict {
        Verdict::Conflict {
            id: "a".to_string(),
            text: "Used by LayoutKeeper: Arrange strip. Saving removes it there.".to_string(),
        }
    }

    fn taken(_: &Shortcut) -> Verdict {
        Verdict::Taken {
            text: "Taken by another app".to_string(),
        }
    }

    const WIN_ALT: u32 = MOD_WIN | MOD_ALT;

    fn saved(binding: &str) -> Option<Outcome> {
        Some(Outcome::Save(vec![ShortcutChange {
            id: "b".to_string(),
            binding: Some(binding.to_string()),
        }]))
    }

    #[test]
    fn it_waits_shows_held_modifiers_and_then_the_keys() {
        let now = Instant::now();
        let mut dialog = Dialog::new(target());
        assert_eq!(dialog.phase(), Phase::Waiting);
        assert_eq!(pieces(&dialog, now), vec![Large::Waiting]);
        dialog.hold(WIN_ALT | MOD_NOREPEAT);
        assert_eq!(dialog.phase(), Phase::Holding(WIN_ALT));
        assert_eq!(
            pieces(&dialog, now),
            vec![
                Large::Held("Win".into()),
                Large::Held("Alt".into()),
                Large::Plus,
                Large::Waiting
            ]
        );
        assert_eq!(dialog.press(key(WIN_ALT, 0x4B), now, free), None);
        assert!(matches!(
            dialog.phase(),
            Phase::Captured {
                verdict: Verdict::Free,
                ..
            }
        ));
        assert_eq!(
            hotkeys::format_shortcut(&dialog.keys().unwrap()),
            "Win+Alt+K"
        );
        assert!(dialog.buttons().is_empty(), "free keys show only the hint");
    }

    #[test]
    fn releasing_every_modifier_goes_back_to_waiting() {
        let mut dialog = Dialog::new(target());
        dialog.hold(MOD_CONTROL);
        assert_eq!(dialog.phase(), Phase::Holding(MOD_CONTROL));
        dialog.hold(0);
        assert_eq!(dialog.phase(), Phase::Waiting);
    }

    #[test]
    fn backspace_once_clears_and_twice_removes_the_hotkey() {
        let now = Instant::now();
        let mut dialog = Dialog::new(target());
        dialog.press(key(WIN_ALT, 0x4B), now, free);
        assert_eq!(dialog.press(key(0, BACKSPACE), now, free), None);
        assert_eq!(dialog.phase(), Phase::Waiting);
        assert_eq!(dialog.press(key(0, BACKSPACE), now, free), saved(""));
    }

    #[test]
    fn the_palettes_hotkey_cannot_be_removed() {
        let mut dialog = Dialog::new(Target {
            removable: false,
            chords: false,
            ..target()
        });
        assert_eq!(dialog.press(key(0, BACKSPACE), Instant::now(), free), None);
        assert_eq!(dialog.phase(), Phase::Waiting);
    }

    #[test]
    fn enter_saves_free_keys_and_does_nothing_while_waiting() {
        let now = Instant::now();
        let mut dialog = Dialog::new(target());
        assert_eq!(dialog.press(key(0, ENTER), now, free), None);
        dialog.hold(WIN_ALT);
        assert_eq!(dialog.press(key(0, ENTER), now, free), None, "holding");
        dialog.press(key(WIN_ALT, 0x46), now, free);
        assert_eq!(dialog.press(key(0, ENTER), now, free), saved("Win+Alt+F"));
    }

    #[test]
    fn enter_with_a_modifier_is_a_key_not_save() {
        let now = Instant::now();
        let mut dialog = Dialog::new(target());
        assert_eq!(dialog.press(key(MOD_CONTROL, ENTER), now, free), None);
        assert_eq!(
            hotkeys::format_shortcut(&dialog.keys().unwrap()),
            "Ctrl+Enter"
        );
    }

    #[test]
    fn esc_cancels_in_every_state() {
        let now = Instant::now();
        let mut dialog = Dialog::new(target());
        assert_eq!(
            dialog.press(key(0, ESCAPE), now, free),
            Some(Outcome::Cancel)
        );
        dialog.press(key(WIN_ALT, 0x4B), now, conflict);
        assert_eq!(
            dialog.press(key(0, ESCAPE), now, conflict),
            Some(Outcome::Cancel)
        );
    }

    #[test]
    fn a_key_without_a_modifier_stays_waiting_with_a_hint() {
        let now = Instant::now();
        let mut dialog = Dialog::new(target());
        assert_eq!(dialog.press(key(0, 0x4B), now, free), None);
        assert_eq!(dialog.phase(), Phase::Waiting);
        assert_eq!(dialog.note(), Some("K needs Win, Ctrl or Alt with it."));
        assert!(matches!(message(&dialog), Some((_, false))));
        assert_eq!(
            dialog.press(key(0, ENTER), now, free),
            None,
            "nothing to save"
        );
        dialog.press(key(WIN_ALT, 0x4B), now, free);
        assert_eq!(dialog.note(), None);
    }

    #[test]
    fn f13_to_f24_stand_alone() {
        let now = Instant::now();
        for vk in [0x7C, 0x80, 0x87] {
            let mut dialog = Dialog::new(target());
            dialog.press(key(0, vk), now, free);
            assert!(matches!(dialog.phase(), Phase::Captured { .. }), "{vk:#x}");
        }
        let mut dialog = Dialog::new(target());
        dialog.press(key(0, 0x7B), now, free);
        assert_eq!(dialog.phase(), Phase::Waiting, "F12 alone is not a hotkey");
    }

    #[test]
    fn a_conflict_offers_move_here_and_enter_takes_the_keys() {
        let now = Instant::now();
        let mut dialog = Dialog::new(target());
        dialog.press(key(WIN_ALT, 0x41), now, conflict);
        assert_eq!(dialog.buttons(), &[Button::Cancel, Button::MoveHere]);
        let Some(Outcome::Save(changes)) = dialog.press(key(0, ENTER), now, conflict) else {
            panic!("Enter moves the keys");
        };
        assert_eq!(
            changes,
            vec![
                ShortcutChange {
                    id: "a".to_string(),
                    binding: Some(String::new())
                },
                ShortcutChange {
                    id: "b".to_string(),
                    binding: Some("Win+Alt+A".to_string())
                },
            ]
        );
        assert!(matches!(
            dialog.click(Button::MoveHere),
            Some(Outcome::Save(_))
        ));
        assert_eq!(dialog.click(Button::Cancel), Some(Outcome::Cancel));
    }

    #[test]
    fn tab_walks_the_buttons_and_enter_or_space_acts_on_the_one_it_reached() {
        let now = Instant::now();
        let mut dialog = Dialog::new(target());
        dialog.press(key(0, TAB), now, conflict);
        assert_eq!(dialog.focus(), None, "no buttons yet");
        dialog.press(key(WIN_ALT, 0x41), now, conflict);
        dialog.press(key(0, TAB), now, conflict);
        assert_eq!(dialog.focus(), Some(Button::Cancel));
        dialog.press(key(0, TAB), now, conflict);
        assert_eq!(dialog.focus(), Some(Button::MoveHere));
        dialog.press(key(MOD_SHIFT, TAB), now, conflict);
        assert_eq!(dialog.focus(), Some(Button::Cancel));
        assert_eq!(
            dialog.press(key(0, ENTER), now, conflict),
            Some(Outcome::Cancel)
        );
        dialog.press(key(0, TAB), now, conflict);
        assert!(matches!(
            dialog.press(key(0, SPACE), now, conflict),
            Some(Outcome::Save(_))
        ));
    }

    #[test]
    fn keys_taken_by_another_app_only_cancel() {
        let now = Instant::now();
        let mut dialog = Dialog::new(target());
        dialog.press(key(WIN_ALT, 0x45), now, taken);
        assert_eq!(dialog.buttons(), &[Button::Cancel]);
        assert_eq!(dialog.press(key(0, ENTER), now, taken), None);
        assert_eq!(dialog.click(Button::MoveHere), None);
        assert!(matches!(
            message(&dialog),
            Some(("Taken by another app", true))
        ));
        // New keys replace the taken ones.
        let later = now + CHORD_WAIT;
        dialog.press(key(WIN_ALT, 0x46), later, free);
        assert_eq!(dialog.press(key(0, ENTER), later, free), saved("Win+Alt+F"));
    }

    #[test]
    fn a_second_key_within_the_chord_wait_makes_a_chord() {
        let start = Instant::now();
        let mut dialog = Dialog::new(target());
        dialog.press(key(WIN_ALT, 0x4B), start, free);
        let soon = start + Duration::from_secs(1);
        assert!(dialog.chord_open(soon));
        assert!(dialog
            .chord_left(soon)
            .is_some_and(|left| left > 0.5 && left < 0.8));
        assert_eq!(pieces(&dialog, soon).last(), Some(&Large::Waiting));
        dialog.press(key(0, 0x53), soon, free);
        assert_eq!(
            hotkeys::format_shortcut(&dialog.keys().unwrap()),
            "Win+Alt+K, S"
        );
        assert!(!dialog.chord_open(soon));
        assert_eq!(dialog.chord_left(soon), None);
        assert!(pieces(&dialog, soon).contains(&Large::Then));
        assert_eq!(
            dialog.press(key(0, ENTER), soon, free),
            saved("Win+Alt+K, S")
        );
    }

    #[test]
    fn a_key_after_the_chord_wait_starts_over() {
        let start = Instant::now();
        let mut dialog = Dialog::new(target());
        dialog.press(key(WIN_ALT, 0x4B), start, free);
        dialog.press(key(WIN_ALT, 0x47), start + CHORD_WAIT, free);
        assert_eq!(
            hotkeys::format_shortcut(&dialog.keys().unwrap()),
            "Win+Alt+G"
        );
        // A plain key after the wait needs a modifier again.
        dialog.press(key(0, 0x53), start + CHORD_WAIT * 2, free);
        assert_eq!(dialog.phase(), Phase::Waiting);
    }

    #[test]
    fn the_palettes_hotkey_takes_no_chord() {
        let start = Instant::now();
        let mut dialog = Dialog::new(Target {
            chords: false,
            removable: false,
            ..target()
        });
        dialog.press(key(WIN_ALT, 0x50), start, free);
        assert!(!dialog.chord_open(start));
        dialog.press(key(WIN_ALT, 0x51), start, free);
        assert_eq!(
            hotkeys::format_shortcut(&dialog.keys().unwrap()),
            "Win+Alt+Q",
            "a second press replaces the first"
        );
    }

    #[test]
    fn checks_become_the_dialogs_lines() {
        let used = Check::UsedBy {
            keys: "Win+Alt+A".to_string(),
            id: "a".to_string(),
            label: "Arrange strip".to_string(),
            group: "WindowDeck".to_string(),
        };
        assert_eq!(
            verdict_of(used, None, "x"),
            Verdict::Conflict {
                id: "a".to_string(),
                text: "Used by WindowDeck: Arrange strip. Saving removes it there.".to_string()
            }
        );
        let held = || Check::HeldByApp {
            keys: "Win+E".to_string(),
        };
        assert_eq!(
            verdict_of(held(), None, "x"),
            Verdict::Taken {
                text: "Taken by another app".to_string()
            }
        );
        assert_eq!(
            verdict_of(held(), Some("Open File Explorer"), "x"),
            Verdict::Taken {
                text: "Windows shortcut: Open File Explorer".to_string()
            }
        );
        let reserved = || Check::Reserved {
            keys: "Win+3".to_string(),
        };
        assert_eq!(
            verdict_of(reserved(), None, "x"),
            Verdict::Taken {
                text: "Windows keeps Win+3 for itself.".to_string()
            }
        );
        assert_eq!(
            verdict_of(reserved(), Some("Lock the PC"), "x"),
            Verdict::Taken {
                text: "Windows shortcut: Lock the PC".to_string()
            }
        );
        assert!(matches!(
            verdict_of(Check::OnePressOnly, None, "x"),
            Verdict::Taken { text } if text.contains("one key press")
        ));
        assert_eq!(verdict_of(Check::Fine, Some("Copy"), "x"), Verdict::Free);
    }

    fn info(id: &str, label: &str, binding: &str, one_press: bool) -> ShortcutInfo {
        ShortcutInfo {
            id: id.to_string(),
            command: None,
            group: "LayoutKeeper".to_string(),
            plugin: None,
            category: String::new(),
            icon: None,
            label: label.to_string(),
            description: String::new(),
            binding: binding.to_string(),
            default_binding: String::new(),
            registered: true,
            one_press,
        }
    }

    #[test]
    fn a_target_comes_from_its_row() {
        let all = vec![
            info("wincraft.palette", "Open the palette", "Win+Alt+P", true),
            info("b", "Save current layout", "", false),
        ];
        let palette = Target::find(&all, "wincraft.palette").unwrap();
        assert!(!palette.chords && !palette.removable);
        assert_eq!(Target::find(&all, "b"), Some(target()));
        assert_eq!(Target::find(&all, "nothing"), None);
    }

    #[test]
    fn the_verdict_uses_wincrafts_own_rules_first() {
        let all = vec![
            info("a", "Arrange strip", "Win+Alt+A", false),
            info("b", "Save current layout", "", false),
        ];
        let keys = hotkeys::parse_shortcut("Win+Alt+A").unwrap();
        assert!(matches!(
            verdict(&keys, &target(), &all),
            Verdict::Conflict { id, .. } if id == "a"
        ));
        let keys = hotkeys::parse_shortcut("Win+L").unwrap();
        assert_eq!(
            verdict(&keys, &target(), &all),
            Verdict::Taken {
                text: "Windows shortcut: Lock the PC".to_string()
            }
        );
    }

    #[test]
    fn a_preview_never_records_and_dropping_it_leaves_no_hook() {
        let mut editor = Editor::preview(Dialog::new(target()));
        let ctx = egui::Context::default();
        assert_eq!(editor.poll(&ctx, &[]), None);
        assert_eq!(editor.dialog.phase(), Phase::Waiting);
    }

    /// Draws the dialog in every phase and theme, and the trigger with and
    /// without keys.
    #[test]
    fn the_dialog_and_trigger_draw_in_every_theme() {
        use crate::core::config::{ThemeChoice, ThemeName};
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let now = Instant::now();
        let mut holding = Dialog::new(target());
        holding.hold(WIN_ALT);
        let mut chord = Dialog::new(target());
        chord.press(key(WIN_ALT, 0x4B), now, free);
        let mut moved = Dialog::new(target());
        moved.press(key(WIN_ALT, 0x41), now, conflict);
        moved.press(key(0, TAB), now, conflict);
        let mut blocked = Dialog::new(target());
        blocked.press(key(WIN_ALT, 0x45), now, taken);
        let mut wide = Dialog::new(target());
        wide.press(key(WIN_ALT | MOD_CONTROL | MOD_SHIFT, 0x7B), now, free);
        wide.press(key(WIN_ALT | MOD_CONTROL | MOD_SHIFT, 0x7A), now, free);
        let mut noted = Dialog::new(target());
        noted.press(key(0, 0x4B), now, free);
        let dialogs = [
            Dialog::new(target()),
            holding,
            chord,
            moved,
            blocked,
            wide,
            noted,
        ];
        for name in ThemeName::ALL {
            for mode in [ThemeChoice::Dark, ThemeChoice::Light] {
                theme::apply(&ctx, name, mode);
                for dialog in &dialogs {
                    let mut editor = Editor::preview(dialog.clone());
                    for _ in 0..2 {
                        let input = egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                pos2(0.0, 0.0),
                                vec2(800.0, 600.0),
                            )),
                            ..Default::default()
                        };
                        let mut output = ctx.run_ui(input, |ui| {
                            trigger(ui, "Win+Alt+K, S");
                            trigger(ui, "");
                            let cover = ui.ctx().content_rect();
                            assert_eq!(show(ui.ctx(), &mut editor, &[], cover, 0), None);
                        });
                        output.textures_delta.clear();
                    }
                }
            }
        }
    }
}
