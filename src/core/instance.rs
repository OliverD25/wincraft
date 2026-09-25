//! What tells one running WinCraft from another: its single-instance mutex,
//! its host window class and its autostart value. A test run sets
//! `WINCRAFT_INSTANCE`, and every one of those names gets that suffix, so a
//! test instance, and a `--quit` sent to it, can never reach the WinCraft
//! the user is running. A test instance is also read-only towards the
//! user's real windows (see LayoutKeeper).

pub const ENV: &str = "WINCRAFT_INSTANCE";

/// `base`, with this process's instance suffix when `WINCRAFT_INSTANCE` is
/// set; exactly `base` when it is not.
pub fn name(base: &str) -> String {
    with_suffix(base, &std::env::var(ENV).unwrap_or_default())
}

/// Whether this process is a test instance: `WINCRAFT_INSTANCE` names a
/// usable suffix. The one place that decides it.
pub fn is_test() -> bool {
    is_test_suffix(&std::env::var(ENV).unwrap_or_default())
}

pub fn is_test_suffix(raw: &str) -> bool {
    !clean(raw).is_empty()
}

fn clean(suffix: &str) -> String {
    suffix
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect()
}

/// The suffix keeps letters, digits, '-' and '_' only: the names end up in a
/// mutex name, where a backslash would mean a namespace, and in the registry.
pub fn with_suffix(base: &str, suffix: &str) -> String {
    let clean = clean(suffix);
    if clean.is_empty() {
        base.to_string()
    } else {
        format!("{base}.{clean}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_suffix_the_name_stays_exactly_as_it_was() {
        assert_eq!(
            with_suffix(r"Local\WinCraft.SingleInstance", ""),
            r"Local\WinCraft.SingleInstance"
        );
        assert_eq!(with_suffix("WinCraftHost", ""), "WinCraftHost");
    }

    #[test]
    fn a_suffix_is_appended_after_a_dot() {
        assert_eq!(with_suffix("WinCraftHost", "test"), "WinCraftHost.test");
        assert_eq!(
            with_suffix(r"Local\WinCraft.SingleInstance", "shot-2"),
            r"Local\WinCraft.SingleInstance.shot-2"
        );
    }

    #[test]
    fn characters_that_could_change_the_meaning_are_dropped() {
        assert_eq!(with_suffix("WinCraftHost", r"a\b c"), "WinCraftHost.abc");
        assert_eq!(with_suffix("WinCraftHost", "  test\n"), "WinCraftHost.test");
        assert_eq!(with_suffix("WinCraftHost", "x_1"), "WinCraftHost.x_1");
    }

    #[test]
    fn a_usable_suffix_makes_a_test_instance() {
        assert!(is_test_suffix("test"));
        assert!(is_test_suffix("shot"));
        assert!(is_test_suffix(" shot-2 "));
        assert!(!is_test_suffix(""));
        assert!(!is_test_suffix("   "));
        assert!(!is_test_suffix(r"\ /"));
        // Exactly when the names change: a test instance never shares them.
        for raw in ["", "test", r"\", "a b"] {
            assert_eq!(
                is_test_suffix(raw),
                with_suffix("WinCraftHost", raw) != "WinCraftHost",
                "{raw:?}"
            );
        }
    }

    #[test]
    fn a_suffix_with_nothing_usable_counts_as_none() {
        assert_eq!(with_suffix("WinCraftHost", r"\\ /"), "WinCraftHost");
        assert_eq!(with_suffix("WinCraftHost", "тест"), "WinCraftHost");
    }
}
