/// Left, top, right, bottom in screen pixels, as `GetWindowRect` reports it.
pub type Rect = [i32; 4];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowIdentity {
    pub exe: String,
    pub name: Option<String>,
    pub title: String,
    pub rect: Rect,
    pub maximized: bool,
}

/// The browsers append their product name to the page title. A window the
/// user named through "Name window…" shows the bare name instead, and keeps
/// it across tab changes and reboots, which makes it the best key there is.
/// Edge writes a zero-width space inside its product name.
const PRODUCT_SUFFIXES: &[&str] = &[
    " - Google Chrome",
    " - Microsoft Edge",
    " - Microsoft\u{200b} Edge",
    " - Mozilla Firefox",
    " - Brave",
];

pub fn name_from_title(title: &str) -> Option<String> {
    if title.is_empty()
        || PRODUCT_SUFFIXES
            .iter()
            .any(|suffix| title.ends_with(suffix))
    {
        return None;
    }
    Some(title.to_string())
}

impl WindowIdentity {
    /// The label a person recognises: the name when there is one.
    pub fn label(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.title)
    }

    pub fn new(exe: &str, title: &str, rect: Rect, maximized: bool) -> Self {
        Self {
            exe: exe.to_string(),
            name: name_from_title(title),
            title: title.to_string(),
            rect,
            maximized,
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Matching {
    /// (saved index, live index)
    pub pairs: Vec<(usize, usize)>,
    pub unmatched_saved: Vec<usize>,
    pub unmatched_live: Vec<usize>,
}

/// Pairs saved windows with live ones, strongest evidence first. Each live
/// window is used at most once, and a pass only sees what earlier passes left.
pub fn match_windows(saved: &[WindowIdentity], live: &[WindowIdentity]) -> Matching {
    let mut saved_used = vec![false; saved.len()];
    let mut live_used = vec![false; live.len()];
    let mut pairs = Vec::new();

    let unique_rect = |rect: &Rect, exe: &str| {
        live.iter()
            .filter(|window| window.exe == exe && &window.rect == rect)
            .count()
            == 1
    };

    let rules: [&dyn Fn(&WindowIdentity, &WindowIdentity) -> bool; 4] = [
        &|s, l| s.name.is_some() && s.name == l.name,
        &|s, l| s.title == l.title && s.rect == l.rect,
        &|s, l| s.title == l.title,
        // Most windows are maximized and share one rectangle, so a rectangle
        // only identifies a window that is neither maximized nor duplicated.
        &|s, l| !s.maximized && !l.maximized && s.rect == l.rect && unique_rect(&l.rect, &l.exe),
    ];

    for rule in rules {
        for (s_index, s) in saved.iter().enumerate() {
            if saved_used[s_index] {
                continue;
            }
            let found = live
                .iter()
                .enumerate()
                .find(|(l_index, l)| !live_used[*l_index] && s.exe == l.exe && rule(s, l));
            if let Some((l_index, _)) = found {
                saved_used[s_index] = true;
                live_used[l_index] = true;
                pairs.push((s_index, l_index));
            }
        }
    }

    pairs.sort_unstable();
    Matching {
        pairs,
        unmatched_saved: (0..saved.len()).filter(|i| !saved_used[*i]).collect(),
        unmatched_live: (0..live.len()).filter(|i| !live_used[*i]).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: Rect = [-11, -11, 3851, 2099];

    fn chrome(title: &str, rect: Rect, maximized: bool) -> WindowIdentity {
        WindowIdentity::new("chrome.exe", title, rect, maximized)
    }

    #[test]
    fn a_named_window_has_no_product_suffix() {
        assert_eq!(
            name_from_title("Dev Sandbox Workspace").as_deref(),
            Some("Dev Sandbox Workspace")
        );
        assert_eq!(name_from_title("Dashboard - SerpApi - Google Chrome"), None);
        assert_eq!(name_from_title("New tab - Microsoft\u{200b} Edge"), None);
        assert_eq!(name_from_title(""), None);
    }

    #[test]
    fn the_name_beats_a_matching_title() {
        // The title matches the second live window, the name the first.
        let saved = vec![chrome("Research", FULL, true)];
        let mut other = chrome("x - Google Chrome", FULL, true);
        other.title = "Research".to_string();
        other.name = Some("Something else".to_string());
        let live = vec![chrome("Research", [0, 0, 10, 10], false), other];

        let matching = match_windows(&saved, &live);
        assert_eq!(matching.pairs, vec![(0, 0)]);
    }

    #[test]
    fn a_maximized_rect_never_matches_alone() {
        let saved = vec![chrome("Old page - Google Chrome", FULL, true)];
        let live = vec![chrome("New page - Google Chrome", FULL, true)];
        let matching = match_windows(&saved, &live);
        assert!(matching.pairs.is_empty());
        assert_eq!(matching.unmatched_saved, vec![0]);
        assert_eq!(matching.unmatched_live, vec![0]);
    }

    #[test]
    fn a_unique_restored_rect_matches_when_the_title_changed() {
        let rect = [100, 100, 900, 700];
        let saved = vec![chrome("Old page - Google Chrome", rect, false)];
        let live = vec![
            chrome("New page - Google Chrome", rect, false),
            chrome("Other - Google Chrome", FULL, true),
        ];
        assert_eq!(match_windows(&saved, &live).pairs, vec![(0, 0)]);
    }

    #[test]
    fn a_shared_restored_rect_is_not_evidence() {
        let rect = [100, 100, 900, 700];
        let saved = vec![chrome("Old - Google Chrome", rect, false)];
        let live = vec![
            chrome("A - Google Chrome", rect, false),
            chrome("B - Google Chrome", rect, false),
        ];
        assert!(match_windows(&saved, &live).pairs.is_empty());
    }

    #[test]
    fn leftovers_on_both_sides_are_reported_and_each_window_is_used_once() {
        let saved = vec![
            chrome("Mail", FULL, true),
            chrome("Mail", FULL, true),
            chrome("Gone", FULL, true),
        ];
        let live = vec![
            chrome("Mail", FULL, true),
            chrome("Fresh - Google Chrome", FULL, true),
        ];
        let matching = match_windows(&saved, &live);
        assert_eq!(matching.pairs, vec![(0, 0)]);
        assert_eq!(matching.unmatched_saved, vec![1, 2]);
        assert_eq!(matching.unmatched_live, vec![1]);
    }

    #[test]
    fn another_program_never_matches() {
        let saved = vec![chrome("Notes", FULL, true)];
        let live = vec![WindowIdentity::new("msedge.exe", "Notes", FULL, true)];
        assert!(match_windows(&saved, &live).pairs.is_empty());
    }
}
