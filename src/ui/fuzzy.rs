/// The score of a query that equals the whole text. Ordinary matches score
/// about 12 per letter plus up to 30 for a short text, so a long query can pass
/// 100; this stays above anything a partial match can reach.
pub const EXACT: i32 = 10_000;

/// Subsequence match with a score, the same idea every command palette uses:
/// the query letters must appear in order, and matches that start a word or run
/// together score higher, so "dim2" beats a scattered accidental match.
///
/// Returns None when the query is not a subsequence at all.
pub fn score(query: &str, text: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    if query.to_lowercase() == text.to_lowercase() {
        return Some(EXACT);
    }
    let needle: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    let hay: Vec<char> = text.chars().collect();
    let lower: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    if lower.len() != hay.len() {
        // A character whose lowercase form is more than one char would break the
        // index-for-index comparison below; fall back to a plain contains test.
        return text
            .to_lowercase()
            .contains(&query.to_lowercase())
            .then_some(1);
    }

    let mut total = 0;
    let mut at = 0usize;
    let mut previous: Option<usize> = None;

    for wanted in needle {
        let found = (at..hay.len()).find(|index| lower[*index] == wanted)?;
        let mut points = 1;
        if previous == Some(found.wrapping_sub(1)) {
            points += 5;
        }
        if found == 0 || is_boundary(hay[found - 1]) {
            points += 4;
        }
        if hay[found].is_uppercase() {
            points += 2;
        }
        total += points;
        previous = Some(found);
        at = found + 1;
    }

    // A short label that used most of its characters is a better answer than a
    // long one that happened to contain the same letters.
    total += (30 - hay.len().min(30)) as i32;
    Some(total)
}

/// Scores against the label and the group name together, so typing a plugin
/// name lists everything it offers.
pub fn score_command(query: &str, group: &str, label: &str) -> Option<i32> {
    let direct = score(query, label);
    let combined = score(query, &format!("{group} {label}"));
    match (direct, combined) {
        (Some(a), Some(b)) => Some(a.max(b - 2)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b - 2),
        (None, None) => None,
    }
}

/// The character positions in `text` that the query matched, found the same
/// way `score` finds them, so the palette can underline exactly those.
pub fn positions(query: &str, text: &str) -> Option<Vec<usize>> {
    let needle: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    let lower: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    if lower.len() != text.chars().count() {
        return None;
    }
    let mut found = Vec::with_capacity(needle.len());
    let mut at = 0usize;
    for wanted in needle {
        let index = (at..lower.len()).find(|index| lower[*index] == wanted)?;
        found.push(index);
        at = index + 1;
    }
    Some(found)
}

fn is_boundary(c: char) -> bool {
    c.is_whitespace() || matches!(c, ':' | '-' | '_' | '/' | '.' | '+' | '(')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_query_matches_everything() {
        assert_eq!(score("", "anything"), Some(0));
    }

    #[test]
    fn letters_must_appear_in_order() {
        assert!(score("dim", "Dim monitor").is_some());
        assert!(score("mid", "Dim monitor").is_none());
    }

    #[test]
    fn matching_ignores_case() {
        assert!(score("DIM", "dim monitor").is_some());
        assert!(score("dim", "DIM MONITOR").is_some());
    }

    #[test]
    fn word_starts_beat_letters_in_the_middle() {
        let starts = score("tm", "Toggle monitor 1").unwrap();
        let middle = score("tm", "668 attempt made").unwrap();
        assert!(starts > middle, "{starts} should beat {middle}");
    }

    #[test]
    fn consecutive_letters_beat_scattered_ones() {
        let together = score("mon", "monitor").unwrap();
        let apart = score("mon", "my own network").unwrap();
        assert!(together > apart, "{together} should beat {apart}");
    }

    #[test]
    fn the_shorter_of_two_equal_matches_wins() {
        let short = score("dim", "Dim").unwrap();
        let long = score("dim", "Dim every single monitor now").unwrap();
        assert!(short > long);
    }

    #[test]
    fn the_plugin_name_is_searchable_too() {
        assert!(score_command("screen 2", "ScreenDimmer", "Toggle monitor 2").is_some());
        assert!(score_command("zzz", "ScreenDimmer", "Toggle monitor 2").is_none());
    }

    #[test]
    fn a_query_the_label_answers_directly_outranks_one_needing_the_group() {
        let direct = score_command("toggle", "ScreenDimmer", "Toggle monitor 1").unwrap();
        let viagroup = score_command("screendim", "ScreenDimmer", "Toggle monitor 1").unwrap();
        assert!(direct > 0 && viagroup > 0);
    }

    #[test]
    fn an_exact_match_beats_every_partial_one() {
        assert_eq!(score("notepad", "Notepad"), Some(EXACT));
        let partial = score("notepad", "Notepad++ portable notepad edition").unwrap();
        assert!(partial < EXACT);
        let long = "abcdefghijklmnopqrstuvwxyz";
        assert!(score(long, &format!("{long}!")).unwrap() < EXACT);
    }

    #[test]
    fn positions_mark_the_letters_that_matched() {
        assert_eq!(positions("mon", "Toggle monitor 1"), Some(vec![7, 8, 9]));
        assert_eq!(positions("tm1", "Toggle monitor 1"), Some(vec![0, 7, 15]));
        assert_eq!(positions("zz", "Toggle monitor 1"), None);
        assert_eq!(positions("", "anything"), Some(vec![]));
    }

    #[test]
    fn ranking_a_real_query_puts_the_right_command_first() {
        let commands = [
            ("ScreenDimmer", "Toggle monitor 1"),
            ("ScreenDimmer", "Toggle monitor 2"),
            ("ScreenDimmer", "Wake all monitors"),
            ("ShortcutDetector", "Open shortcut detector"),
        ];
        let mut ranked: Vec<(i32, &str)> = commands
            .iter()
            .filter_map(|(group, label)| {
                score_command("dim 2", group, label).map(|points| (points, *label))
            })
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0));
        assert_eq!(
            ranked.first().map(|entry| entry.1),
            Some("Toggle monitor 2")
        );
    }
}
