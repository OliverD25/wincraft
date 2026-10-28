use super::*;

fn group(key: &str) -> ArrangeGroup {
    ArrangeGroup {
        key: key.to_string(),
        label: key.to_string(),
        windows: Vec::new(),
    }
}

#[test]
fn auto_is_by_monitor_with_two_monitors_or_more() {
    assert_eq!(ArrangeView::Auto.resolve(1), ArrangeView::Flat);
    assert_eq!(ArrangeView::Auto.resolve(0), ArrangeView::Flat);
    assert_eq!(ArrangeView::Auto.resolve(2), ArrangeView::ByMonitor);
    assert_eq!(ArrangeView::Flat.resolve(3), ArrangeView::Flat);
    assert_eq!(ArrangeView::ByMonitor.resolve(1), ArrangeView::ByMonitor);
    for view in [ArrangeView::Auto, ArrangeView::ByMonitor, ArrangeView::Flat] {
        assert_eq!(ArrangeView::from_setting(view.setting()), view);
    }
    assert_eq!(ArrangeView::from_setting("sideways"), ArrangeView::Auto);
}

#[test]
fn a_group_is_picked_by_place_or_by_key() {
    let groups = [
        group("Chrome.UserData.Profile3"),
        group("Telegram.TelegramDesktop"),
    ];
    assert_eq!(ArrangeGroup::pick(&groups, "0"), Some(0));
    assert_eq!(ArrangeGroup::pick(&groups, "1"), Some(1));
    assert_eq!(ArrangeGroup::pick(&groups, "2"), None);
    assert_eq!(
        ArrangeGroup::pick(&groups, "telegram.telegramdesktop"),
        Some(1)
    );
    assert_eq!(
        ArrangeGroup::pick(&groups, " Chrome.UserData.Profile3 "),
        Some(0)
    );
    assert_eq!(ArrangeGroup::pick(&groups, "Viber"), None);
    assert_eq!(ArrangeGroup::pick(&[], "0"), None);
}

#[test]
fn a_toggle_state_says_on_off_or_nothing_to_switch() {
    assert_eq!(ToggleState::on("Shown").on, Some(true));
    assert_eq!(ToggleState::off("Hidden").on, Some(false));
    let none = ToggleState::without_switch("No window");
    assert_eq!((none.on, none.line.as_str()), (None, "No window"));
}

#[test]
fn an_unknown_state_has_no_switch_and_says_so() {
    let unknown = ToggleState::unknown();
    assert_eq!(unknown.on, None);
    assert_eq!(unknown.line, "State unknown");
}

#[test]
fn only_rename_and_ask_name_go_to_window_namer() {
    assert!(ArrangeAction::Rename {
        hwnd: 1,
        name: None
    }
    .is_about_names());
    assert!(ArrangeAction::AskName(1).is_about_names());
    assert!(!ArrangeAction::Activate(1).is_about_names());
    assert!(!ArrangeAction::Close(1).is_about_names());
    let reorder = ArrangeAction::Reorder {
        group: "g".into(),
        order: vec![1, 2],
    };
    assert!(!reorder.is_about_names());
}

#[test]
fn a_search_question_becomes_a_prompt_for_that_provider() {
    let ask = crate::search::Ask {
        provider: "bookmarks",
        chip: "Bookmark".into(),
        placeholder: "Name".into(),
        text: "Docs".into(),
        action: "Save bookmark as".into(),
        subtitle: "docs-path".into(),
        payload: "docs-path".into(),
    };
    let prompt = Prompt::from_ask(ask);
    assert_eq!(prompt.plugin, "search:bookmarks");
    assert_eq!(prompt.chip, "Bookmark");
    assert_eq!(prompt.text, "Docs");
    assert_eq!(prompt.payload, "docs-path");
    assert_eq!((prompt.id, prompt.target), (0, 0));
    assert_eq!(prompt.empty_action, None);
}
