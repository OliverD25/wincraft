use egui::{Id, Ui};

/// How long a clicked value is kept while the host has not confirmed it. After
/// this the host's value wins, so a refused change shows as refused.
const HOLD: f64 = 1.0;

/// A value the user just chose and the time they chose it.
type Choice<T> = (T, f64);

/// Which value to draw. The host answers a click a moment later, and until it
/// does the snapshot still holds the old value. Drawing that would make a
/// switch slide back and then forward again.
fn settle<T: PartialEq + Copy>(
    snapshot: T,
    pending: Option<Choice<T>>,
    now: f64,
) -> (T, Option<Choice<T>>) {
    match pending {
        Some((value, since)) if value != snapshot && now - since <= HOLD => {
            (value, Some((value, since)))
        }
        _ => (snapshot, None),
    }
}

/// The value to draw now, given the one the host last reported.
pub fn shown<T: PartialEq + Copy + Send + Sync + 'static>(ui: &Ui, id: Id, snapshot: T) -> T {
    let now = ui.input(|i| i.time);
    let stored = ui.data(|d| d.get_temp::<Choice<T>>(id));
    let (value, keep) = settle(snapshot, stored, now);
    match keep {
        Some(_) => ui.ctx().request_repaint_after_secs(HOLD as f32 + 0.05),
        None => ui.data_mut(|d| d.remove::<Choice<T>>(id)),
    }
    value
}

/// Remembers a value the user just chose, to show until the host agrees.
pub fn choose<T: Send + Sync + Clone + 'static>(ui: &Ui, id: Id, value: T) {
    let now = ui.input(|i| i.time);
    ui.data_mut(|d| d.insert_temp(id, (value, now)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_pending_shows_the_snapshot() {
        assert_eq!(settle(false, None, 5.0), (false, None));
    }

    #[test]
    fn the_clicked_value_shows_at_once_while_the_snapshot_is_stale() {
        let (value, keep) = settle(false, Some((true, 5.0)), 5.0);
        assert!(value);
        assert_eq!(keep, Some((true, 5.0)));
        assert!(settle(false, keep, 5.5).0);
    }

    #[test]
    fn the_choice_is_dropped_when_the_snapshot_agrees() {
        assert_eq!(settle(true, Some((true, 5.0)), 5.2), (true, None));
    }

    #[test]
    fn the_snapshot_wins_after_the_timeout() {
        assert_eq!(settle(false, Some((true, 5.0)), 6.5), (false, None));
    }

    #[test]
    fn it_works_for_a_choice_of_option() {
        assert_eq!(settle(1_usize, Some((2, 0.0)), 0.1).0, 2);
        assert_eq!(settle(2_usize, Some((2, 0.0)), 0.1).0, 2);
    }
}
