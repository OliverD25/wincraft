use std::collections::BTreeMap;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MOD_ALT, MOD_WIN};

use super::*;
use crate::core::hotkeys;
use crate::core::traits::FieldKind;
use crate::core::ui_bridge::ActionKind;

#[test]
fn every_plugin_has_its_own_id_and_a_full_description() {
    let plugins = load_active_plugins();
    let mut ids = Vec::new();
    for plugin in &plugins {
        let meta = plugin.metadata();
        assert!(!meta.id.is_empty() && !meta.id.contains(' '), "{}", meta.id);
        assert!(!meta.name.is_empty(), "{} has no name", meta.id);
        assert!(!meta.description.is_empty(), "{} has no text", meta.id);
        assert!(!meta.category.is_empty(), "{} has no category", meta.id);
        assert!(!meta.readme.is_empty(), "{} has no README", meta.id);
        let parts: Vec<&str> = meta.version.split('.').collect();
        assert_eq!(parts.len(), 3, "{} version {}", meta.id, meta.version);
        assert!(
            parts.iter().all(|part| part.parse::<u32>().is_ok()),
            "{} version {}",
            meta.id,
            meta.version
        );
        ids.push(meta.id);
    }
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "two plugins share an id: {ids:?}");
}

#[test]
fn every_setting_field_has_a_default_of_the_right_kind() {
    for plugin in load_active_plugins() {
        let id = plugin.metadata().id;
        let defaults = plugin.default_settings();
        let defaults = defaults.as_object().expect("settings are an object");
        let mut keys = Vec::new();
        for field in plugin.settings_fields() {
            let at = format!("{id}.{}", field.key);
            assert!(!field.label.is_empty() && !field.help.is_empty(), "{at}");
            assert!(!keys.contains(&field.key), "{at} is listed twice");
            keys.push(field.key);
            let value = defaults
                .get(field.key)
                .unwrap_or_else(|| panic!("{at} has no default"));
            match field.kind {
                FieldKind::Toggle => assert!(value.is_boolean(), "{at}"),
                FieldKind::Text | FieldKind::Path => assert!(value.is_string(), "{at}"),
                FieldKind::Choice(options) => {
                    assert!(!options.is_empty(), "{at}");
                    let chosen = value.as_str().unwrap_or_default();
                    assert!(options.contains(&chosen), "{at} default {chosen}");
                }
                FieldKind::Number { min, max } => {
                    let number = value.as_f64().unwrap_or(f64::NAN);
                    assert!(min < max, "{at}");
                    assert!((min..=max).contains(&number), "{at} default {number}");
                }
                FieldKind::Slider { min, max, step } => {
                    let number = value.as_f64().unwrap_or(f64::NAN);
                    assert!(min < max && step > 0.0, "{at}");
                    assert!((min..=max).contains(&number), "{at} default {number}");
                }
            }
        }
    }
}

#[test]
fn migrating_the_default_settings_and_hotkeys_changes_nothing() {
    for plugin in load_active_plugins() {
        let id = plugin.metadata().id;
        let defaults = plugin.default_settings();
        let mut settings = defaults.clone();
        plugin.migrate_settings(&mut settings);
        assert_eq!(settings, defaults, "{id} rewrites its own defaults");
        let mut hotkeys = BTreeMap::new();
        plugin.migrate_hotkeys(&mut hotkeys, &mut settings);
        assert!(hotkeys.is_empty(), "{id} invents hotkeys");
    }
}

#[test]
fn default_hotkeys_are_a_family_that_starts_with_win_alt() {
    let mut seen: Vec<(u32, u32)> = Vec::new();
    for plugin in load_active_plugins() {
        let id = plugin.metadata().id;
        let mut names = Vec::new();
        for action in plugin.hotkey_actions() {
            assert!(!action.label.is_empty() && !action.name.is_empty(), "{id}");
            assert!(!names.contains(&action.name), "{id}.{} twice", action.name);
            names.push(action.name);
            let hotkey = action.default;
            assert!(
                hotkey.modifiers & MOD_WIN != 0 && hotkey.modifiers & MOD_ALT != 0,
                "{id}.{} is {}",
                action.name,
                hotkeys::format(hotkey)
            );
            let key = (hotkey.modifiers, hotkey.vk);
            assert!(!seen.contains(&key), "{id}.{} clashes", action.name);
            seen.push(key);
            let text = hotkeys::format(hotkey);
            let again = hotkeys::parse(&text).expect("a formatted hotkey parses");
            assert_eq!(hotkeys::format(again), text);
        }
    }
}

#[test]
fn palette_tray_and_status_texts_are_filled_in() {
    for plugin in load_active_plugins() {
        let id = plugin.metadata().id;
        let mut ids = Vec::new();
        for command in plugin.palette_commands() {
            assert!(!command.label.is_empty(), "{id}");
            assert!(!ids.contains(&command.id), "{id} palette id {}", command.id);
            ids.push(command.id);
            for kind in [ActionKind::Hotkey, ActionKind::Tray, ActionKind::Palette] {
                if let Some(text) = plugin.palette_subtitle(kind, command.id) {
                    assert!(!text.is_empty(), "{id}");
                }
            }
        }
        for action in plugin.hotkey_actions() {
            if let Some(text) = plugin.palette_subtitle(ActionKind::Hotkey, action.id) {
                assert!(!text.is_empty(), "{id}");
            }
        }
        for action in plugin.tray_actions() {
            assert!(!action.label.is_empty(), "{id}");
        }
        for toggle in plugin.tray_toggles() {
            assert!(!toggle.label.is_empty(), "{id}");
        }
        for field in plugin.dynamic_fields() {
            assert!(
                field.key.starts_with('/') && !field.label.is_empty(),
                "{id}"
            );
        }
        if let Some(text) = plugin.status() {
            assert!(!text.is_empty(), "{id}");
        }
    }
}

#[test]
fn a_plugin_split_out_of_another_names_a_plugin_that_exists() {
    let plugins = load_active_plugins();
    let ids: Vec<&str> = plugins.iter().map(|p| p.metadata().id).collect();
    for plugin in &plugins {
        if let Some(parent) = plugin.split_from() {
            assert!(ids.contains(&parent), "{parent} is not a plugin");
            assert_ne!(parent, plugin.metadata().id);
        }
    }
}
