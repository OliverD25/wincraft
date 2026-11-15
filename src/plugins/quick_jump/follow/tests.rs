use super::super::matching::tests::window;
use super::super::matching::{find, slot_views, State};
use super::super::pins::tests::pin;
use super::super::pins::Slots;
use super::*;

const CHAT_A: &str = "local_aaaa-1111";
const CHAT_B: &str = "local_bbbb-2222";

fn chat_window(hwnd: isize, title: &str, chat: Option<&str>) -> Live {
    let mut live = window(hwnd, "Claude", title);
    live.chat = chat.map(str::to_string);
    live
}

fn chat_pin(title: &str, chat: Option<&str>) -> Pin {
    let mut made = pin("Claude", None);
    made.title = title.to_string();
    made.claude_session = chat.map(str::to_string);
    made
}

#[test]
fn a_chat_pin_finds_its_chat_after_a_rename_and_not_the_old_name() {
    let pinned = chat_pin("Design session", Some(CHAT_A));
    let live = vec![
        chat_window(1, "Design session (old)", Some(CHAT_B)),
        chat_window(2, "Rust master", Some(CHAT_A)),
    ];
    assert_eq!(
        find_how(&pinned, &live).map(|(w, how)| (w.hwnd, how)),
        Some((2, How::Chat))
    );
    let others = vec![chat_window(1, "Design session (old)", Some(CHAT_B))];
    assert_eq!(find(&pinned, &others), None, "another chat is not this one");
}

#[test]
fn a_chat_pin_never_keeps_a_window_that_moved_on_to_another_chat() {
    let mut pinned = chat_pin("Design session", Some(CHAT_A));
    pinned.hwnd = Some(1);
    let main = vec![chat_window(1, "Other chat", Some(CHAT_B))];
    assert_eq!(find(&pinned, &main), None);
    let unknown = vec![chat_window(1, "Other chat", None)];
    assert_eq!(find(&pinned, &unknown).map(|w| w.hwnd), Some(1), "handle");
}

#[test]
fn a_chat_pin_takes_no_only_window_but_a_title_before_chats_are_known() {
    let pinned = chat_pin("Design session", Some(CHAT_A));
    assert_eq!(find(&pinned, &[chat_window(1, "Claude", None)]), None);
    let titled = [chat_window(3, "Design session", None)];
    assert_eq!(
        find_how(&pinned, &titled).map(|(w, how)| (w.hwnd, how)),
        Some((3, How::Title))
    );
}

#[test]
fn a_closed_chat_pin_opens_its_chat_by_the_link() {
    let pinned = chat_pin("Design session", Some(CHAT_A));
    assert_eq!(when_closed(&pinned), Closed::OpenChat(CHAT_A.to_string()));
    let mut slots = Slots::default();
    slots.add(pinned);
    slots.add(pin("Chrome", None));
    let views = slot_views(&slots, &[], 0);
    assert_eq!(views[0].state, State::NotOpen);
    assert!(views[0].opens_chat);
    assert!(!views[1].opens_chat, "other pins stay \"not open\"");
}

#[test]
fn a_pin_without_a_chat_or_link_starts_the_app_or_says_so() {
    assert_eq!(when_closed(&pin("Chrome", None)), Closed::Start);
    let bad = chat_pin("x", Some("not-a-local-id"));
    assert_eq!(when_closed(&bad), Closed::Start, "no link: the old way");
    let mut quiet = pin("Chrome", None);
    quiet.start = false;
    assert_eq!(when_closed(&quiet), Closed::Tell);
}

#[test]
fn a_new_pin_of_a_chat_window_remembers_the_chat() {
    let made = chat_window(4, "Design session", Some(CHAT_A)).to_pin();
    assert_eq!(made.claude_session.as_deref(), Some(CHAT_A));
    assert!(made.sure && made.title_auto);
}

#[test]
fn an_old_pin_learns_its_chat_only_while_the_window_shows_it() {
    let mut pinned = chat_pin("Design session", None);
    pinned.hwnd = Some(1);
    pinned.sure = true;
    let moved_on = [chat_window(1, "Other chat", Some(CHAT_B))];
    assert!(!follow(&mut pinned, &moved_on));
    assert_eq!(pinned.claude_session, None);
    let showing = [chat_window(1, "Design session", Some(CHAT_A))];
    assert!(follow(&mut pinned, &showing));
    assert_eq!(pinned.claude_session.as_deref(), Some(CHAT_A));
}

#[test]
fn the_title_follows_the_same_window_and_its_chat() {
    let mut tab = pin("Chrome", Some(5));
    tab.title = "Inbox".to_string();
    tab.sure = true;
    let live = [window(5, "Chrome", "Docs - Google Chrome")];
    assert!(follow(&mut tab, &live));
    assert_eq!(tab.title, "Docs");
    assert!(!follow(&mut tab, &live), "saved only on a change");

    let mut chat = chat_pin("Design session", Some(CHAT_A));
    let live = [chat_window(2, "Rust master", Some(CHAT_A))];
    assert!(follow(&mut chat, &live));
    assert_eq!(chat.title, "Rust master");
    assert_eq!(chat.hwnd, Some(2));
}

#[test]
fn the_only_window_rule_teaches_nothing() {
    let mut claude = chat_pin("Design session", None);
    let live = [chat_window(3, "WinCraft Rust Master", Some(CHAT_B))];
    assert!(!follow(&mut claude, &live));
    assert_eq!(claude.hwnd, Some(3), "it still jumps there");
    assert_eq!(claude.title, "Design session");
    assert_eq!(claude.claude_session, None);
    assert!(!claude.sure);
    assert!(!follow(&mut claude, &live), "nor later by its handle");
}

#[test]
fn a_typed_title_or_an_empty_one_is_not_followed() {
    let mut typed = pin("Chrome", Some(5));
    typed.title = "Mail".to_string();
    typed.title_auto = false;
    typed.sure = true;
    let live = [window(5, "Chrome", "Docs - Google Chrome")];
    assert!(!follow(&mut typed, &live));
    assert_eq!(typed.title, "Mail");

    let mut blank = pin("Chrome", Some(6));
    blank.title = "Docs".to_string();
    blank.sure = true;
    for title in ["", "Untitled", "New Tab - Google Chrome", "Chrome"] {
        assert!(
            !follow(&mut blank, &[window(6, "Chrome", title)]),
            "{title}"
        );
        assert_eq!(blank.title, "Docs");
    }
}

#[test]
fn a_title_typed_in_the_details_stops_following_and_drops_the_chat() {
    let before = chat_pin("Design session", Some(CHAT_A));
    let mut old = before.clone();
    old.title = "Rust master".to_string();
    let mut draft = before.clone();
    draft.label = Some("Design".to_string());
    let kept = edited(&old, &before, draft.clone());
    assert_eq!(kept.title, "Rust master", "the followed title stays");
    assert_eq!(kept.label.as_deref(), Some("Design"));
    assert!(kept.title_auto);
    assert_eq!(kept.claude_session.as_deref(), Some(CHAT_A));

    draft.title = "Mail".to_string();
    let typed = edited(&old, &before, draft);
    assert_eq!(typed.title, "Mail");
    assert!(!typed.title_auto);
    assert_eq!(typed.claude_session, None);
}

#[test]
fn old_files_without_the_new_fields_load_and_follow() {
    let text = r#"{"pins":[{"exe":"C:\\Apps\\Claude.exe","app_name":"Claude","title":"Chat"}]}"#;
    let (slots, old) = Slots::read(text);
    assert!(!old);
    let loaded = slots.get(0).expect("one pin");
    assert_eq!(loaded.claude_session, None);
    assert!(loaded.title_auto, "old pins follow");
    assert!(
        !slots.to_json().contains("title_auto"),
        "the default is not written"
    );
    let mut chat = loaded.clone();
    chat.claude_session = Some(CHAT_A.to_string());
    chat.title_auto = false;
    let mut slots = Slots::default();
    slots.add(chat.clone());
    let (back, _) = Slots::read(&slots.to_json());
    assert_eq!(back.get(0), Some(&chat));
}
