//! The name a person knows an app's taskbar group by, for the strip's app
//! list and LayoutKeeper's messages: what the app calls itself, its Start
//! menu entry, the description in its program file, or its file name.
//! Never a path or an app ID.

use crate::core::appid;
use crate::core::live_windows::{self, LiveWindow};
use crate::core::window_identity::{browser_name, program_name};
use crate::search::providers::apps;

/// What one of its windows says the window's group is called, and whether
/// that is final: it is not while the palette's apps index is still being
/// built, so the caller asks again later.
pub fn window_label(window: &LiveWindow) -> (String, bool) {
    let has_id = window.group != window.exe_path.to_lowercase();
    let name = window.app_name.clone().or_else(|| {
        has_id
            .then(|| appid::registered_name(&window.group))
            .flatten()
    });
    let exe = &window.identity.exe;
    let browser = browser_name(exe);
    let (name, resolved) = if browser.is_some() {
        (name, true)
    } else {
        with_start_menu_name(name, &window.group)
    };
    let label = match browser {
        Some(browser) => {
            appid::group_label(browser, name.as_deref(), &window.group, &window.exe_path)
        }
        None => app_label(
            name.as_deref(),
            live_windows::file_description(&window.exe_path).as_deref(),
            exe,
        ),
    };
    (label, resolved)
}

/// A readable name for a group key when nothing better is known: the program
/// file of a path key, else the last word of an app ID. Never the key whole.
pub fn friendly_key(key: &str) -> String {
    if key.contains('\\') {
        return program_name(&live_windows::file_name(key));
    }
    key.rsplit(['.', '!', '_'])
        .find(|part| !part.is_empty())
        .unwrap_or(key)
        .to_string()
}

/// The name an app gets when it gives none of its own: the name of its Start
/// menu entry or app list entry, found by app ID or program file in the apps
/// index the palette keeps. Without a match the label stays what the
/// program file says ("Windows Terminal Host"). The flag says whether the
/// answer is final: it is not while the index is still being built.
pub fn with_start_menu_name(name: Option<String>, group: &str) -> (Option<String>, bool) {
    match apps::published() {
        Some(index) => start_menu_name(name, group, Some(&|key: &str| apps::name_in(&index, key))),
        None => start_menu_name(name, group, None),
    }
}

pub type NameLookup<'a> = dyn Fn(&str) -> Option<String> + 'a;

/// `lookup` is None while the index is not built yet.
pub fn start_menu_name(
    name: Option<String>,
    group: &str,
    lookup: Option<&NameLookup<'_>>,
) -> (Option<String>, bool) {
    if name.is_some() {
        return (name, true);
    }
    match lookup {
        Some(lookup) => (lookup(group), true),
        None => (None, false),
    }
}

/// What the strip calls any other app's group: the name the app gives its
/// windows or its Start menu entry, else the description in its program
/// file, else its file name.
pub fn app_label(name: Option<&str>, description: Option<&str>, exe: &str) -> String {
    name.or(description)
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| program_name(exe))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_app_that_gives_no_name_gets_its_start_menu_name() {
        let lookup = |key: &str| match key {
            "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App" => Some("Terminal".to_string()),
            "Com.Codewulf.WulfScreen" => Some("WulfScreen".to_string()),
            _ => None,
        };
        let (name, done) = start_menu_name(
            None,
            "Microsoft.WindowsTerminal_8wekyb3d8bbwe!App",
            Some(&lookup),
        );
        assert!(done);
        // The Start menu name beats the program file's description.
        assert_eq!(
            app_label(
                name.as_deref(),
                Some("Windows Terminal Host"),
                "openconsole.exe"
            ),
            "Terminal"
        );
        let (name, _) = start_menu_name(None, "Com.Codewulf.WulfScreen", Some(&lookup));
        assert_eq!(
            app_label(name.as_deref(), Some("Com.Codewulf.WulfScreen"), "wulf.exe"),
            "WulfScreen"
        );
    }

    #[test]
    fn a_name_the_app_gave_itself_is_kept() {
        let lookup = |_: &str| Some("From the index".to_string());
        let (name, done) = start_menu_name(Some("Mine".to_string()), "any", Some(&lookup));
        assert_eq!((name.as_deref(), done), (Some("Mine"), true));
        // Not even asked while the index is missing.
        assert_eq!(
            start_menu_name(Some("Mine".to_string()), "any", None),
            (Some("Mine".to_string()), true)
        );
    }

    #[test]
    fn with_no_match_the_old_label_stays_and_a_missing_index_is_not_final() {
        let none = |_: &str| None;
        let (name, done) = start_menu_name(None, "Some.App", Some(&none));
        assert_eq!(
            (name.clone(), done),
            (None, true),
            "an answer: there is none"
        );
        assert_eq!(
            app_label(name.as_deref(), Some("Windows Terminal Host"), "x.exe"),
            "Windows Terminal Host"
        );
        // The index is still being built: the label is made again later.
        assert_eq!(start_menu_name(None, "Some.App", None), (None, false));
    }

    #[test]
    fn a_group_key_alone_gives_a_readable_name() {
        assert_eq!(friendly_key("Pkg_8wekyb!App"), "App");
        assert_eq!(friendly_key("Telegram.TelegramDesktop"), "TelegramDesktop");
        assert_eq!(friendly_key("plain"), "plain");
        assert_eq!(friendly_key("ends."), "ends");
    }

    #[test]
    fn a_blank_name_or_description_falls_back_to_the_program() {
        assert_eq!(app_label(None, Some(" Notes "), "x.exe"), "Notes");
        assert_eq!(app_label(Some(" "), Some("Notes"), "claude.exe"), "Claude");
        assert_eq!(app_label(None, Some(""), "claude.exe"), "Claude");
    }
}
