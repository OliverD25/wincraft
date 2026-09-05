use serde_json::Value;

use crate::core::traits::FieldKind;
use crate::core::ui_bridge::FieldInfo;

/// Draws one declared setting and returns its new value when the user moved it.
/// Everything a plugin author writes is the SettingField; this is the only code
/// that knows what a slider looks like.
pub fn show(ui: &mut egui::Ui, field: &FieldInfo) -> Option<Value> {
    let mut changed = None;

    ui.horizontal(|ui| {
        ui.label(&field.label).on_hover_text(&field.help);
        ui.add_space(8.0);

        match field.kind {
            FieldKind::Toggle => {
                let mut on = field.value.as_bool().unwrap_or(false);
                if ui.checkbox(&mut on, "").changed() {
                    changed = Some(Value::Bool(on));
                }
            }
            FieldKind::Slider { min, max, step } => {
                let mut value = field.value.as_f64().unwrap_or(min);
                let slider = egui::Slider::new(&mut value, min..=max)
                    .step_by(step)
                    .fixed_decimals(2);
                if ui.add(slider).changed() {
                    changed = number(value);
                }
            }
            FieldKind::Number { min, max } => {
                let mut value = field.value.as_f64().unwrap_or(min);
                let drag = egui::DragValue::new(&mut value).range(min..=max);
                if ui.add(drag).changed() {
                    changed = number(value);
                }
            }
            FieldKind::Text => {
                let mut text = field.value.as_str().unwrap_or_default().to_string();
                if ui.text_edit_singleline(&mut text).changed() {
                    changed = Some(Value::String(text));
                }
            }
            FieldKind::Choice(options) => {
                let current = field.value.as_str().unwrap_or_default().to_string();
                egui::ComboBox::from_id_salt(&field.key)
                    .selected_text(if current.is_empty() {
                        "\u{2014}"
                    } else {
                        &current
                    })
                    .show_ui(ui, |ui| {
                        for option in options {
                            if ui.selectable_label(current == *option, *option).clicked() {
                                changed = Some(Value::String((*option).to_string()));
                            }
                        }
                    });
            }
            FieldKind::Path => {
                let mut text = field.value.as_str().unwrap_or_default().to_string();
                let edit = egui::TextEdit::singleline(&mut text).desired_width(320.0);
                if ui.add(edit).changed() {
                    changed = Some(Value::String(text));
                }
            }
        }
    });

    if !field.help.is_empty() {
        ui.label(egui::RichText::new(&field.help).weak().small());
    }
    ui.add_space(4.0);
    changed
}

fn number(value: f64) -> Option<Value> {
    serde_json::Number::from_f64(value).map(Value::Number)
}
