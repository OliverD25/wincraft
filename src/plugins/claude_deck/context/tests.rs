use super::*;

fn known(tokens: u64, most: u64, compactions: u32, one_million: bool) -> Context {
    Context {
        tokens: Some(tokens),
        most,
        compactions,
        one_million,
    }
}

#[test]
fn the_fill_uses_the_setting_when_there_is_one() {
    let fill = fill(&known(318_000, 600_000, 2, false), Some(777_000), None);
    assert_eq!(
        fill,
        Some(Fill {
            tokens: 318_000,
            limit: 777_000,
            compactions: 2
        })
    );
}

#[test]
fn without_a_setting_the_window_follows_the_model() {
    let small = fill(
        &known(141_000, 141_000, 0, false),
        None,
        Some("claude-sonnet-5-5"),
    );
    assert_eq!(small.map(|f| f.limit), Some(160_000));
    let large = fill(
        &known(141_000, 141_000, 0, false),
        None,
        Some("claude-opus-5-5[1m]"),
    );
    assert_eq!(large.map(|f| f.limit), Some(800_000));
    let seen = fill(&known(141_000, 141_000, 0, true), None, None);
    assert_eq!(seen.map(|f| f.limit), Some(800_000));
}

#[test]
fn an_unknown_size_has_no_fill() {
    assert_eq!(fill(&Context::default(), Some(777_000), None), None);
}
