use serde_json::Value;
use windows_sys::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};

use crate::core::theme::Tokens;
use crate::core::traits::FieldKind;
use crate::core::ui_bridge::FieldInfo;
use crate::core::wide;
use crate::ui::widgets::button::{self, Kind};
use crate::ui::widgets::icons::Icon;
use crate::ui::widgets::row::{self, RowText};
use crate::ui::widgets::{choice, input, slider, toggle};

const TEXT_WIDTH: f32 = 240.0;
const PATH_WIDTH: f32 = 280.0;

/// Draws one declared setting as a settings row and returns its new value
/// when the user changed it. This is the only code that knows what each field
/// kind looks like; a plugin author only declares the field.
pub fn show(ui: &mut egui::Ui, field: &FieldInfo) -> Option<Value> {
    let tokens = Tokens::get(ui.ctx());
    let mut changed = None;
    let text = RowText::new(&field.label).desc(&field.help, tokens.text_secondary);

    row::settings_row(ui, &field.key, text, false, |ui| match field.kind {
        FieldKind::Toggle => {
            let mut on = field.value.as_bool().unwrap_or(false);
            if toggle::toggle(ui, &mut on).changed() {
                changed = Some(Value::Bool(on));
            }
        }
        FieldKind::Slider { min, max, step } => {
            let mut value = field.value.as_f64().unwrap_or(min);
            if slider::slider(ui, &mut value, min, max, step).changed() {
                changed = number(value);
            }
        }
        FieldKind::Number { min, max } => {
            let mut value = field.value.as_f64().unwrap_or(min);
            if input::number(ui, &field.key, &mut value, min, max) {
                changed = number(value);
            }
        }
        FieldKind::Text => {
            let mut value = field.value.as_str().unwrap_or_default().to_string();
            if input::text(ui, &field.key, &mut value, input::Face::Text, TEXT_WIDTH).changed() {
                changed = Some(Value::String(value));
            }
        }
        FieldKind::Choice(options) => {
            let current = field.value.as_str().unwrap_or_default();
            let selected = options.iter().position(|option| *option == current);
            if let Some(index) = choice::choice(ui, &field.key, options, selected) {
                changed = Some(Value::String(options[index].to_string()));
            }
        }
        FieldKind::Path => {
            let mut value = field.value.as_str().unwrap_or_default().to_string();
            if input::text(ui, &field.key, &mut value, input::Face::Mono, PATH_WIDTH).changed() {
                changed = Some(Value::String(value.clone()));
            }
            if button::with_icon(
                ui,
                "Browse\u{2026}",
                Kind::Secondary,
                Some(Icon::FolderOpen),
            )
            .clicked()
            {
                if let Some(path) = browse_for_file() {
                    changed = Some(Value::String(path));
                }
            }
        }
    });
    changed
}

fn number(value: f64) -> Option<Value> {
    serde_json::Number::from_f64(value).map(Value::Number)
}

/// The standard Windows open dialog. It is modal and runs its own message
/// loop, which pauses this window until the user picks a file or cancels.
fn browse_for_file() -> Option<String> {
    let mut buffer = vec![0u16; 1024];
    let filter = wide("All files\0*.*\0");
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    dialog.lpstrFilter = filter.as_ptr();
    dialog.lpstrFile = buffer.as_mut_ptr();
    dialog.nMaxFile = buffer.len() as u32;
    dialog.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
    if unsafe { GetOpenFileNameW(&mut dialog) } == 0 {
        return None;
    }
    let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..end]))
}
