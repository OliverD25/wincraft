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

fn of(tokens: u64, limit: u64, compactions: u32) -> Fill {
    Fill {
        tokens,
        limit,
        compactions,
    }
}

#[test]
fn the_percent_is_rounded_and_can_pass_100() {
    assert_eq!(percent(of(318_000, 777_000, 0)), 41);
    assert_eq!(percent(of(141_000, 160_000, 0)), 88);
    assert_eq!(percent(of(900_000, 777_000, 0)), 116);
    assert_eq!(percent(of(1, 0, 0)), 100);
}

#[test]
fn the_band_changes_at_15_45_and_75_percent() {
    assert_eq!(band(0), Band::Fresh);
    assert_eq!(band(14), Band::Fresh);
    assert_eq!(band(15), Band::Fine);
    assert_eq!(band(44), Band::Fine);
    assert_eq!(band(45), Band::GettingFull);
    assert_eq!(band(74), Band::GettingFull);
    assert_eq!(band(75), Band::NearCompaction);
    assert_eq!(band(116), Band::NearCompaction);
}

#[test]
fn the_arc_is_at_least_3_percent_and_at_most_full() {
    assert_eq!(arc(0), 0.03);
    assert_eq!(arc(1), 0.03);
    assert_eq!(arc(3), 0.03);
    assert_eq!(arc(41), 0.41);
    assert_eq!(arc(100), 1.0);
    assert_eq!(arc(116), 1.0);
}

#[test]
fn the_hover_line_names_size_limit_percent_and_compactions() {
    assert_eq!(
        hover_text(Some(of(318_000, 777_000, 2))),
        "Context 318k of 777k (41%) \u{b7} compacted 2 times"
    );
    assert_eq!(
        hover_text(Some(of(318_000, 777_000, 1))),
        "Context 318k of 777k (41%) \u{b7} compacted 1 time"
    );
    assert_eq!(
        hover_text(Some(of(141_000, 160_000, 0))),
        "Context 141k of 160k (88%)"
    );
    assert_eq!(hover_text(None), "Context size not known yet");
}

#[test]
fn thousands_are_rounded_and_a_few_tokens_show_as_1k() {
    assert_eq!(thousands(318_499), "318k");
    assert_eq!(thousands(318_500), "319k");
    assert_eq!(thousands(120), "1k");
    assert_eq!(thousands(0), "0k");
}

#[test]
fn the_ring_sits_left_of_the_time_in_one_column() {
    // A short time leaves more room than the column: the column wins.
    let short = ring_rect(300.0, 280.0, 11.5);
    assert_eq!(short.right(), 280.0);
    assert_eq!(short.width(), SIZE);
    assert_eq!(short.center().y, 11.5);
    // A time wider than the column pushes the ring left of it.
    let long = ring_rect(270.0, 280.0, 11.5);
    assert_eq!(long.right(), 270.0);
    // Two rows with different short times line up.
    assert_eq!(ring_rect(296.0, 280.0, 0.0).left(), short.left());
}
