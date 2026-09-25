/// Each monitor's place word by where its left edge is: "Left", "Middle",
/// "Right" for three, "Left" and "Right" for two, "Monitor N" counted from
/// the left past three, and nothing for a single monitor.
pub fn position_names(lefts: &[i32]) -> Vec<Option<String>> {
    const NAMES: [&[&str]; 4] = [&[], &[], &["Left", "Right"], &["Left", "Middle", "Right"]];
    let mut order: Vec<usize> = (0..lefts.len()).collect();
    order.sort_by_key(|index| lefts[*index]);
    let mut result = vec![None; lefts.len()];
    for (rank, index) in order.into_iter().enumerate() {
        result[index] = match NAMES.get(lefts.len()) {
            Some(names) => names.get(rank).map(|name| name.to_string()),
            None => Some(format!("Monitor {}", rank + 1)),
        };
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(names: &[&str]) -> Vec<Option<String>> {
        names.iter().map(|name| Some(name.to_string())).collect()
    }

    #[test]
    fn one_monitor_has_no_name() {
        assert_eq!(position_names(&[0]), [None]);
        assert!(position_names(&[]).is_empty());
    }

    #[test]
    fn two_monitors_are_left_and_right() {
        assert_eq!(position_names(&[0, 2560]), words(&["Left", "Right"]));
        assert_eq!(position_names(&[2560, 0]), words(&["Right", "Left"]));
    }

    #[test]
    fn three_monitors_with_the_primary_in_the_middle() {
        // The primary monitor is at 0, whatever order Windows lists them in.
        assert_eq!(
            position_names(&[0, -1920, 3840]),
            words(&["Middle", "Left", "Right"])
        );
        assert_eq!(
            position_names(&[1920, -1920, 0]),
            words(&["Right", "Left", "Middle"])
        );
    }

    #[test]
    fn four_or_more_monitors_are_numbered_from_the_left() {
        assert_eq!(
            position_names(&[3840, 0, -1920, 1920]),
            words(&["Monitor 4", "Monitor 2", "Monitor 1", "Monitor 3"])
        );
    }
}
