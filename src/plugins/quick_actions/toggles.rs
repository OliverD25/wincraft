//! The Windows settings QuickActions switches on and off, and how each one
//! maps to the values Windows keeps. Reading and writing go through
//! `System`, so the tests run on a fake and never touch the real settings.
//!
//! Undo keeps the exact values read before the change, not just "on" or
//! "off": dark mode has two values, and a user who had dark apps with a
//! light taskbar gets exactly that back (Principle 1).

use super::system::System;

pub const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
pub const ADVANCED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced";

/// `ABS_AUTOHIDE` in the taskbar's state.
pub const AUTOHIDE: u32 = 0x1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    DarkMode,
    DesktopIcons,
    TaskbarAutoHide,
    HiddenFiles,
    FileExtensions,
}

/// The raw values of one setting, in the order its `Setting` lists them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Values(pub Vec<u32>);

impl Setting {
    #[cfg(test)]
    pub const ALL: [Setting; 5] = [
        Setting::DarkMode,
        Setting::DesktopIcons,
        Setting::TaskbarAutoHide,
        Setting::HiddenFiles,
        Setting::FileExtensions,
    ];

    /// The words in the notice: "Dark mode on".
    pub fn name(self) -> &'static str {
        match self {
            Setting::DarkMode => "Dark mode",
            Setting::DesktopIcons => "Desktop icons",
            Setting::TaskbarAutoHide => "Taskbar auto-hide",
            Setting::HiddenFiles => "Hidden files",
            Setting::FileExtensions => "File extensions",
        }
    }

    /// The registry values under HKCU, for the settings that live there,
    /// with the value Windows uses when one was never written.
    pub fn registry(self) -> Option<(&'static str, &'static [(&'static str, u32)])> {
        match self {
            Setting::DarkMode => Some((
                PERSONALIZE,
                &[("AppsUseLightTheme", 1), ("SystemUsesLightTheme", 1)],
            )),
            Setting::HiddenFiles => Some((ADVANCED, &[("Hidden", 2)])),
            Setting::FileExtensions => Some((ADVANCED, &[("HideFileExt", 1)])),
            Setting::DesktopIcons | Setting::TaskbarAutoHide => None,
        }
    }

    /// Whether these values mean the setting is on: dark mode on, desktop
    /// icons shown, the taskbar hiding, hidden files and file extensions
    /// shown.
    pub fn is_on(self, values: &Values) -> bool {
        let first = values.0.first().copied().unwrap_or(0);
        match self {
            Setting::DarkMode => first == 0,
            Setting::DesktopIcons => first != 0,
            Setting::TaskbarAutoHide => first & AUTOHIDE != 0,
            Setting::HiddenFiles => first == 1,
            Setting::FileExtensions => first == 0,
        }
    }

    /// The values that turn it `on` or off, starting from `before` so
    /// anything else in them stays as it was.
    pub fn values_for(self, on: bool, before: &Values) -> Values {
        match self {
            // Apps and Windows switch together (the user's decision).
            Setting::DarkMode => {
                let light = u32::from(!on);
                Values(vec![light, light])
            }
            Setting::DesktopIcons => Values(vec![u32::from(on)]),
            Setting::TaskbarAutoHide => {
                let state = before.0.first().copied().unwrap_or(0);
                Values(vec![if on {
                    state | AUTOHIDE
                } else {
                    state & !AUTOHIDE
                }])
            }
            Setting::HiddenFiles => Values(vec![if on { 1 } else { 2 }]),
            Setting::FileExtensions => Values(vec![u32::from(!on)]),
        }
    }
}

/// The setting's values now; None when Windows would not say.
pub fn read(system: &dyn System, setting: Setting) -> Option<Values> {
    if let Some((key, values)) = setting.registry() {
        return Some(Values(
            values
                .iter()
                .map(|(name, default)| system.read_dword(key, name).unwrap_or(*default))
                .collect(),
        ));
    }
    match setting {
        Setting::DesktopIcons => system
            .desktop_icons_shown()
            .map(|shown| Values(vec![u32::from(shown)])),
        Setting::TaskbarAutoHide => system.taskbar_state().map(|state| Values(vec![state])),
        _ => None,
    }
}

/// Writes the values and tells Windows and the open apps about them.
pub fn write(system: &mut dyn System, setting: Setting, values: &Values) -> Result<(), String> {
    if let Some((key, names)) = setting.registry() {
        for ((name, _), value) in names.iter().zip(&values.0) {
            system.write_dword(key, name, *value)?;
        }
        match setting {
            Setting::DarkMode => system.announce("ImmersiveColorSet"),
            _ => system.refresh_explorer(),
        }
        return Ok(());
    }
    let wanted = values.0.first().copied().unwrap_or(0);
    match setting {
        Setting::TaskbarAutoHide => system.set_taskbar_state(wanted),
        Setting::DesktopIcons => {
            let shown = system
                .desktop_icons_shown()
                .ok_or("Windows did not show where the desktop icons are")?;
            if shown == (wanted != 0) {
                return Ok(());
            }
            system.toggle_desktop_icons()?;
            match system.desktop_icons_shown() {
                Some(now) if now == (wanted != 0) => Ok(()),
                _ => Err("Windows did not answer the request".to_string()),
            }
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::system::tests::Fake;
    use super::*;

    #[test]
    fn every_setting_has_a_name_and_a_way_to_read_it() {
        let fake = Fake::default();
        for setting in Setting::ALL {
            assert!(!setting.name().is_empty());
            assert!(read(&fake, setting).is_some(), "{setting:?}");
        }
    }

    #[test]
    fn missing_registry_values_read_as_the_windows_defaults() {
        let fake = Fake::default();
        assert_eq!(read(&fake, Setting::DarkMode), Some(Values(vec![1, 1])));
        assert!(!Setting::DarkMode.is_on(&Values(vec![1, 1])));
        assert_eq!(read(&fake, Setting::HiddenFiles), Some(Values(vec![2])));
        assert!(!Setting::HiddenFiles.is_on(&Values(vec![2])));
        assert_eq!(read(&fake, Setting::FileExtensions), Some(Values(vec![1])));
        assert!(!Setting::FileExtensions.is_on(&Values(vec![1])));
    }

    #[test]
    fn the_values_for_on_and_off_mean_on_and_off() {
        for setting in Setting::ALL {
            let before = Values(vec![0x2, 0x2]);
            for on in [true, false] {
                let values = setting.values_for(on, &before);
                assert_eq!(setting.is_on(&values), on, "{setting:?} {on}");
            }
        }
    }

    #[test]
    fn dark_mode_switches_apps_and_windows_together() {
        let none = Values(vec![]);
        assert_eq!(
            Setting::DarkMode.values_for(true, &none),
            Values(vec![0, 0])
        );
        assert_eq!(
            Setting::DarkMode.values_for(false, &none),
            Values(vec![1, 1])
        );
        assert!(Setting::DarkMode.is_on(&Values(vec![0, 1])), "apps decide");
    }

    #[test]
    fn auto_hide_keeps_the_other_taskbar_flags() {
        let before = Values(vec![0x2]);
        assert_eq!(
            Setting::TaskbarAutoHide.values_for(true, &before),
            Values(vec![0x3])
        );
        let on = Values(vec![0x3]);
        assert_eq!(
            Setting::TaskbarAutoHide.values_for(false, &on),
            Values(vec![0x2])
        );
    }

    #[test]
    fn registry_values_are_written_and_the_change_announced() {
        let mut fake = Fake::default();
        write(&mut fake, Setting::DarkMode, &Values(vec![0, 0])).expect("written");
        assert_eq!(fake.dword(PERSONALIZE, "AppsUseLightTheme"), Some(0));
        assert_eq!(fake.dword(PERSONALIZE, "SystemUsesLightTheme"), Some(0));
        assert_eq!(fake.calls(), ["announce ImmersiveColorSet"]);

        let mut fake = Fake::default();
        write(&mut fake, Setting::HiddenFiles, &Values(vec![1])).expect("written");
        assert_eq!(fake.dword(ADVANCED, "Hidden"), Some(1));
        assert_eq!(fake.calls(), ["refresh explorer"]);
        write(&mut fake, Setting::FileExtensions, &Values(vec![0])).expect("written");
        assert_eq!(fake.dword(ADVANCED, "HideFileExt"), Some(0));
    }

    #[test]
    fn undo_puts_back_two_values_that_differed() {
        let mut fake = Fake::default();
        fake.set_dword(PERSONALIZE, "AppsUseLightTheme", 0);
        fake.set_dword(PERSONALIZE, "SystemUsesLightTheme", 1);
        let before = read(&fake, Setting::DarkMode).expect("read");
        write(
            &mut fake,
            Setting::DarkMode,
            &Setting::DarkMode.values_for(false, &before),
        )
        .expect("written");
        assert_eq!(fake.dword(PERSONALIZE, "AppsUseLightTheme"), Some(1));
        write(&mut fake, Setting::DarkMode, &before).expect("undone");
        assert_eq!(fake.dword(PERSONALIZE, "AppsUseLightTheme"), Some(0));
        assert_eq!(fake.dword(PERSONALIZE, "SystemUsesLightTheme"), Some(1));
    }

    #[test]
    fn the_taskbar_state_is_set_whole() {
        let mut fake = Fake::default();
        write(&mut fake, Setting::TaskbarAutoHide, &Values(vec![0x3])).expect("set");
        assert_eq!(
            read(&fake, Setting::TaskbarAutoHide),
            Some(Values(vec![0x3]))
        );
    }

    #[test]
    fn desktop_icons_toggle_only_when_they_differ_and_a_silent_windows_is_an_error() {
        let mut fake = Fake::default();
        write(&mut fake, Setting::DesktopIcons, &Values(vec![1])).expect("already shown");
        assert!(fake.calls().is_empty());
        write(&mut fake, Setting::DesktopIcons, &Values(vec![0])).expect("hidden");
        assert_eq!(fake.calls(), ["toggle desktop icons"]);
        assert_eq!(read(&fake, Setting::DesktopIcons), Some(Values(vec![0])));

        let mut deaf = Fake {
            icons_ignore: true,
            ..Fake::default()
        };
        let error = write(&mut deaf, Setting::DesktopIcons, &Values(vec![0])).unwrap_err();
        assert!(error.contains("did not answer"), "{error}");

        let mut gone = Fake {
            icons: None,
            ..Fake::default()
        };
        assert!(read(&gone, Setting::DesktopIcons).is_none());
        assert!(write(&mut gone, Setting::DesktopIcons, &Values(vec![0])).is_err());
    }
}
