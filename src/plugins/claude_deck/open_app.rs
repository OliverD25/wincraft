//! What ClaudeDeck asks of the Claude app for a chat: open it,
//! rename it. A chat with a window of its own is brought forward; the rest
//! goes through the app's main window (see `core::claude_app`). When a step
//! fails, WinCraft says so in a notice that names
//! ClaudeDeck. Rename asks for the new name in WinCraft's rename box, then
//! changes it in the app, so there is one name for the chat.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::core::ui_bridge::{ArrangeAction, HostChannel, HostRequest, Prompt};
use crate::core::{claude_app, host, window_names};

use super::window::bring_forward;

/// The `host::post_to_plugins` ids a worker or the list sends: a notice
/// is waiting, or the rename box should open. Both are handled on the host
/// thread.
pub(crate) const NOTICE_POST_ID: usize = 0xC1A0_DEC1;
pub(crate) const RENAME_POST_ID: usize = 0xC1A0_DEC2;
/// The number of the rename box's question.
pub(crate) const PROMPT_RENAME_CHAT: u32 = 1;

static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// The chat being renamed: its title now, and its window if it has one.
static RENAMING: Mutex<Option<(String, Option<isize>)>> = Mutex::new(None);
/// Windows whose WindowNamer name is to be cleared by the list.
static CLEARS: Mutex<Vec<isize>> = Mutex::new(Vec::new());

/// Opens the chat: its window when it has one, else its button in the app.
pub(super) fn open_chat(window: Option<isize>, title: &str, to_host: &HostChannel) {
    if window.is_some() {
        bring_forward(window, to_host);
    } else {
        open_in_app(title);
    }
}

/// The words of the notice when the app cannot show the chat. It names
/// ClaudeDeck, so the user knows who is talking.
pub fn show_failed_notice(title: &str) -> String {
    format!("ClaudeDeck could not show \"{title}\" in the Claude app.")
}

/// Makes the app's main window show the chat, also when the chat has a
/// window of its own.
pub(super) fn show_in_app(id: &str, title: &str) {
    if !crate::core::claude_app::show_chat(id) {
        say(show_failed_notice(title.trim()));
    }
}

/// What the row says while the chat is being opened in the app.
pub(crate) const OPENING_LINE: &str = "Opening in the Claude app\u{2026}";
/// A second open of the same chat within this long of the first is ignored.
const REPEAT: Duration = Duration::from_secs(2);
/// An open that never reported its end is forgotten after this long.
const STALE: Duration = Duration::from_secs(15);

/// The chats being opened in the app, and when each open began and ended.
#[derive(Default)]
pub(crate) struct Opening {
    entries: Vec<(String, Instant, Option<Instant>)>,
}

impl Opening {
    /// Whether the open may start. False for a chat that is being opened,
    /// or was opened a moment ago: a double-click must open it once.
    pub(crate) fn begin(&mut self, title: &str, now: Instant) -> bool {
        self.entries.retain(|(_, began, ended)| match ended {
            Some(ended) => now.saturating_duration_since(*ended) < REPEAT,
            None => now.saturating_duration_since(*began) < STALE,
        });
        if self.entries.iter().any(|(name, _, _)| name == title) {
            return false;
        }
        self.entries.push((title.to_string(), now, None));
        true
    }

    pub(crate) fn end(&mut self, title: &str, now: Instant) {
        for (name, _, ended) in &mut self.entries {
            if name == title && ended.is_none() {
                *ended = Some(now);
            }
        }
    }

    /// Whether the chat's open is still running.
    pub(crate) fn is_opening(&self, title: &str, now: Instant) -> bool {
        self.entries.iter().any(|(name, began, ended)| {
            name == title && ended.is_none() && now.saturating_duration_since(*began) < STALE
        })
    }
}

static OPENING: Mutex<Opening> = Mutex::new(Opening {
    entries: Vec::new(),
});

fn open_in_app(title: &str) {
    let allowed = OPENING
        .lock()
        .map(|mut opening| opening.begin(title, Instant::now()))
        .unwrap_or(true);
    if !allowed {
        log::info!("ClaudeDeck: \"{title}\" is already being opened; the repeat is ignored");
        return;
    }
    let name = title.to_string();
    claude_app::open_chat(title, say, move || {
        if let Ok(mut opening) = OPENING.lock() {
            opening.end(&name, Instant::now());
        }
    });
}

/// Puts "Opening in the Claude app…" on the rows of the chats being
/// opened, so a click shows something at once.
pub(super) fn show_opening(chats: &mut [super::model::Chat]) {
    let Ok(opening) = OPENING.lock() else {
        return;
    };
    let now = Instant::now();
    for chat in chats {
        if chat.window.is_none() && opening.is_opening(&chat.title, now) {
            chat.text = Some(OPENING_LINE.to_string());
        }
    }
}

/// Starts a rename: the host thread opens the rename box.
pub(super) fn ask_rename(title: &str, window: Option<isize>) {
    log::info!("ClaudeDeck: asking for a new name for \"{title}\"");
    if let Ok(mut renaming) = RENAMING.lock() {
        *renaming = Some((title.to_string(), window));
    }
    host::post_to_plugins(RENAME_POST_ID);
}

/// Opens the rename box, started with the chat's name now. Host thread.
pub(crate) fn ask_pending_rename(plugin: &str) {
    let Some((title, window)) = RENAMING.lock().ok().and_then(|renaming| renaming.clone()) else {
        log::info!("ClaudeDeck: the rename box was asked for with no chat waiting");
        return;
    };
    host::ask(Prompt {
        plugin: plugin.to_string(),
        id: PROMPT_RENAME_CHAT,
        target: window.unwrap_or(0),
        chip: "Rename".to_string(),
        placeholder: format!("Name for \u{201c}{title}\u{201d}"),
        text: title,
        action: "Rename to".to_string(),
        subtitle: "Claude chat".to_string(),
        ..Default::default()
    });
}

/// What a typed name asks for: the chat's app title now, the new name, and
/// the window whose WindowNamer name is to be cleared.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RenamePlan {
    pub title: String,
    pub name: String,
    pub clear: Option<isize>,
    /// The chat's own window, when it has one.
    pub window: Option<isize>,
}

/// Decides what the answer of the rename box asks for. The chat is named by
/// its title in the Claude app (the session title), as kept when the box
/// was opened, never by a window title or a WindowNamer name.
pub(crate) fn plan_rename(
    renaming: Option<(String, Option<isize>)>,
    window: isize,
    typed: &str,
    namer_name: bool,
) -> Option<RenamePlan> {
    let Some((title, _)) = renaming else {
        log::info!("ClaudeDeck: a rename answer came with no rename waiting; ignored");
        return None;
    };
    let name = typed.trim();
    if name.is_empty() || name == title {
        log::info!("ClaudeDeck: rename \"{title}\": the name is empty or the same; nothing done");
        return None;
    }
    Some(RenamePlan {
        title,
        name: name.to_string(),
        clear: (window != 0 && namer_name).then_some(window),
        window: (window != 0).then_some(window),
    })
}

/// The new name was typed. Renames the chat in the app, and drops the
/// WindowNamer name of its window: the app sets the window title itself.
pub(crate) fn rename_answered(window: isize, name: &str) {
    let renaming = RENAMING
        .lock()
        .ok()
        .and_then(|mut renaming| renaming.take());
    rename_answered_with(renaming, window, name, |title, name, window| {
        claude_app::rename_chat(title, name, window, say);
    });
}

/// `rename_answered` with the rename that waited, and the start of the
/// rename in the app, given.
pub(crate) fn rename_answered_with(
    renaming: Option<(String, Option<isize>)>,
    window: isize,
    typed: &str,
    start: impl FnOnce(&str, &str, Option<isize>),
) {
    let namer_name = window != 0 && window_names::custom(window).is_some();
    let Some(plan) = plan_rename(renaming, window, typed, namer_name) else {
        return;
    };
    log::info!(
        "ClaudeDeck: rename answer: \"{}\" becomes \"{}\" in the Claude app",
        plan.title,
        plan.name
    );
    start(&plan.title, &plan.name, plan.window);
    if let Some(window) = plan.clear {
        if let Ok(mut clears) = CLEARS.lock() {
            clears.push(window);
        }
    }
}

/// Clears the WindowNamer names that are waiting. Runs in the list's
/// refresh, which can reach WindowNamer through the host.
pub(super) fn send_pending_clears(to_host: &HostChannel) {
    let windows = CLEARS
        .lock()
        .map(|mut clears| std::mem::take(&mut *clears))
        .unwrap_or_default();
    for hwnd in windows {
        to_host.send(HostRequest::Arrange {
            plugin: super::ID.to_string(),
            action: ArrangeAction::Rename { hwnd, name: None },
        });
    }
}

fn say(text: String) {
    log::info!("ClaudeDeck: notice waiting: {text}");
    if let Ok(mut pending) = PENDING.lock() {
        pending.push(text);
    }
    host::post_to_plugins(NOTICE_POST_ID);
}

/// Shows the waiting notices. Runs on the host thread.
pub(crate) fn show_pending() {
    let texts = PENDING
        .lock()
        .map(|mut pending| std::mem::take(&mut *pending))
        .unwrap_or_default();
    for text in texts {
        host::notify("ClaudeDeck", &text);
    }
}

/// For tests: the rename that waits for the host thread.
#[cfg(test)]
pub(super) fn pending_rename() -> Option<(String, Option<isize>)> {
    RENAMING.lock().ok().and_then(|renaming| renaming.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(title: &str) -> crate::plugins::claude_deck::model::Chat {
        use crate::plugins::claude_deck::model::{Chat, Mark};
        Chat {
            id: title.to_string(),
            title: title.to_string(),
            folder: None,
            branch: None,
            model: None,
            parent: None,
            created: 0,
            mark: Mark::Unknown,
            since: None,
            jobs: 0,
            window: None,
            text: None,
            full: None,
            fork: None,
            terminal: None,
            news: false,
        }
    }

    #[test]
    fn the_notice_for_a_chat_that_cannot_be_shown_names_claudedeck_and_the_chat() {
        assert_eq!(
            show_failed_notice("Fix login"),
            "ClaudeDeck could not show \"Fix login\" in the Claude app."
        );
    }

    #[test]
    fn a_waiting_notice_is_handed_over_once() {
        PENDING.lock().unwrap().push("one".to_string());
        let first = PENDING.lock().map(|mut p| std::mem::take(&mut *p)).unwrap();
        assert_eq!(first, ["one"]);
        assert!(PENDING.lock().unwrap().is_empty());
    }

    #[test]
    fn the_answer_of_the_rename_box_starts_the_rename_in_the_app_by_its_title() {
        let mut started = Vec::new();
        let waiting = Some(("App title".to_string(), Some(7isize)));
        rename_answered_with(waiting, 7, "  New name ", |title, name, window| {
            started.push((title.to_string(), name.to_string(), window));
        });
        assert_eq!(
            started,
            [("App title".to_string(), "New name".to_string(), Some(7))]
        );
    }

    #[test]
    fn an_empty_or_unchanged_answer_or_a_stray_one_starts_nothing() {
        let waiting = || Some(("Same".to_string(), None));
        let mut started = 0;
        rename_answered_with(waiting(), 0, "   ", |_, _, _| started += 1);
        rename_answered_with(waiting(), 0, "Same", |_, _, _| started += 1);
        rename_answered_with(None, 0, "Anything", |_, _, _| started += 1);
        assert_eq!(started, 0);
    }

    #[test]
    fn the_plan_clears_the_windowname_only_when_there_is_one() {
        let waiting = || Some(("T".to_string(), Some(9isize)));
        let with = plan_rename(waiting(), 9, "New", true).unwrap();
        assert_eq!(with.clear, Some(9));
        let without = plan_rename(waiting(), 9, "New", false).unwrap();
        assert_eq!(without.clear, None);
        let no_window = plan_rename(waiting(), 0, "New", true).unwrap();
        assert_eq!(no_window.clear, None);
        assert_eq!(with.title, "T");
    }

    #[test]
    fn a_repeat_open_of_the_same_chat_is_ignored_while_it_runs_and_for_two_seconds() {
        let start = Instant::now();
        let mut opening = Opening::default();
        assert!(opening.begin("A", start));
        assert!(
            !opening.begin("A", start + Duration::from_millis(300)),
            "running"
        );
        assert!(opening.begin("B", start), "another chat is not blocked");
        opening.end("A", start + Duration::from_secs(1));
        assert!(
            !opening.begin("A", start + Duration::from_millis(2500)),
            "just ended"
        );
        assert!(
            opening.begin("A", start + Duration::from_millis(3500)),
            "after the window"
        );
    }

    #[test]
    fn an_open_that_never_ended_is_forgotten() {
        let start = Instant::now();
        let mut opening = Opening::default();
        assert!(opening.begin("A", start));
        assert!(opening.is_opening("A", start + Duration::from_secs(1)));
        assert!(!opening.is_opening("A", start + STALE + Duration::from_secs(1)));
        assert!(opening.begin("A", start + STALE + Duration::from_secs(1)));
    }

    #[test]
    fn the_row_of_a_chat_being_opened_reads_opening_until_the_open_ends() {
        let mut chat = plain("Row being opened");
        chat.text = Some("Last: done".into());
        OPENING
            .lock()
            .unwrap()
            .begin("Row being opened", Instant::now());
        let mut chats = vec![chat];
        show_opening(&mut chats);
        assert_eq!(chats[0].text.as_deref(), Some(OPENING_LINE));
        OPENING
            .lock()
            .unwrap()
            .end("Row being opened", Instant::now());
        let mut again = vec![plain("Row being opened")];
        show_opening(&mut again);
        assert_ne!(again[0].text.as_deref(), Some(OPENING_LINE));
        // A chat with a window of its own is not "opened in the app".
        OPENING.lock().unwrap().begin("Windowed", Instant::now());
        let mut windowed = plain("Windowed");
        windowed.window = Some(5);
        let mut chats = vec![windowed];
        show_opening(&mut chats);
        assert_ne!(chats[0].text.as_deref(), Some(OPENING_LINE));
    }
}
