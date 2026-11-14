//! The waits WinCraft's own hook keeps by chat (`session:<id>`).

use super::*;

#[test]
fn a_chat_waits_until_it_is_answered() {
    let mut attention = Attention::default();
    assert_eq!(attention.session_waiting("abc"), None);
    attention.session_wait("abc", Ask::Approval);
    let (first, ask) = attention.session_waiting("abc").expect("waits");
    assert_eq!(ask, Ask::Approval);
    // A second word keeps the first time and takes the new kind.
    attention.session_wait("abc", Ask::Question);
    assert_eq!(
        attention.session_waiting("abc"),
        Some((first, Ask::Question))
    );
    assert!(attention.session_answered("abc"));
    assert!(!attention.session_answered("abc"));
    assert_eq!(attention.session_waiting("abc"), None);
}

#[test]
fn mark_all_as_seen_forgets_the_chats_too() {
    let mut attention = Attention::default();
    attention.session_wait("abc", Ask::Question);
    attention.clear_all();
    assert_eq!(attention.session_waiting("abc"), None);
}
