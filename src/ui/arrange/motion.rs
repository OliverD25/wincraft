use super::*;

/// How far a card's motion is after `elapsed`, from 0 to 1, slowing down at
/// the end.
pub fn motion_progress(elapsed: Duration) -> f32 {
    let t = (elapsed.as_secs_f32() / CARD_MOTION.as_secs_f32()).clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// A card's size, as a part of its full size, and its opacity. A card whose
/// window came while the strip was open grows from 90 % and fades in; a
/// card whose window was asked to close (`left`, since the ask) shrinks to
/// 85 % and fades out, and one whose window did not close (`back`, since it
/// was seen to stay) grows back.
pub fn card_motion(
    appeared: Option<Duration>,
    left: Option<Duration>,
    back: Option<Duration>,
) -> (f32, f32) {
    let (mut scale, mut fade) = (1.0, 1.0);
    if let Some(elapsed) = appeared {
        let progress = motion_progress(elapsed);
        scale = CARD_GROW_FROM + (1.0 - CARD_GROW_FROM) * progress;
        fade = progress;
    }
    if let Some(elapsed) = left {
        let (shrink, gone) = slide::close_curve(elapsed);
        scale *= shrink;
        fade *= gone;
    }
    if let Some(elapsed) = back {
        let (grow, here) = slide::regrow_curve(elapsed);
        scale *= grow;
        fade *= here;
    }
    (scale, fade)
}

/// How long a card has been shrinking: since its window left, else since
/// the ask to close it. With the Windows setting "Animation effects" off a
/// card that was only asked does not shrink.
pub fn shrink_age(
    gone: Option<Duration>,
    asked: Option<Duration>,
    animate: bool,
) -> Option<Duration> {
    gone.or(asked.filter(|_| animate))
}

/// The card ↑ (`down` false) or ↓ picks from window `from`: on the nearest
/// line of cards above or below it, the one nearest across. `cards` are the
/// cards drawn last frame, each with what names it: its window, or in the
/// By monitor view a card or a ghost. None at the top or the bottom.
pub fn card_above_or_below<T: Copy + PartialEq>(
    cards: &[(T, Rect)],
    from: T,
    down: bool,
) -> Option<T> {
    let here = cards.iter().find(|(card, _)| *card == from)?.1;
    let beyond = |rect: &Rect| {
        if down {
            rect.top() >= here.bottom() - 1.0
        } else {
            rect.bottom() <= here.top() + 1.0
        }
    };
    let line = cards
        .iter()
        .filter(|(_, rect)| beyond(rect))
        .map(|(_, rect)| rect.top())
        .reduce(|a, b| if down { a.min(b) } else { a.max(b) })?;
    let across = |rect: &Rect| (rect.center().x - here.center().x).abs();
    cards
        .iter()
        .filter(|(_, rect)| beyond(rect) && (rect.top() - line).abs() < here.height() / 2.0)
        .min_by(|a, b| across(&a.1).total_cmp(&across(&b.1)))
        .map(|(card, _)| *card)
}

/// `rect` at `scale` of its size, around its centre.
pub(super) fn grown(rect: Rect, scale: f32) -> Rect {
    Rect::from_center_size(rect.center(), rect.size() * scale)
}

/// The window a middle-click closes, as in Windows' own taskbar popup: a
/// card's window, never a ghost's, whose window has a card of its own.
pub(super) fn middle_close(hwnd: isize, ghost: bool) -> Option<isize> {
    (!ghost).then_some(hwnd)
}

/// The card picked after Ctrl+W closes `closed`: the next one in `order`,
/// else the one before. None when it was the only card.
pub fn after_close(order: &[isize], closed: isize) -> Option<isize> {
    let at = order.iter().position(|hwnd| *hwnd == closed)?;
    order
        .get(at + 1)
        .or_else(|| at.checked_sub(1).and_then(|before| order.get(before)))
        .copied()
}

/// A window that is gone, kept on screen while its card fades out.
#[derive(Clone, Debug)]
pub(super) struct Leaving {
    pub(super) group: String,
    /// Its place in the group's windows.
    pub(super) place: usize,
    pub(super) window: ArrangeWindow,
    pub(super) since: Instant,
}

/// Notes the windows that came and went between two looks. A window counts
/// as new only the first time the strip sees it, so a closed window that
/// shows up once more does not grow in again.
pub(super) fn note_changes(
    known: &mut HashSet<isize>,
    appeared: &mut HashMap<isize, Instant>,
    leaving: &mut Vec<Leaving>,
    old: Option<&ArrangeGroup>,
    groups: &[ArrangeGroup],
    now: Instant,
) {
    for window in groups.iter().flat_map(|group| &group.windows) {
        if known.insert(window.hwnd) {
            appeared.insert(window.hwnd, now);
        }
    }
    let Some(old) = old else {
        return;
    };
    let new = groups.iter().find(|group| group.key == old.key);
    for (place, window) in old.windows.iter().enumerate() {
        let gone = new.is_none_or(|new| new.windows.iter().all(|w| w.hwnd != window.hwnd));
        if gone && leaving.iter().all(|l| l.window.hwnd != window.hwnd) {
            leaving.push(Leaving {
                group: old.key.clone(),
                place,
                window: window.clone(),
                since: now,
            });
        }
    }
}

/// Puts the windows that are gone back where they were, until their cards
/// have faded out.
pub(super) fn keep_leaving(leaving: &[Leaving], groups: &mut [ArrangeGroup]) {
    for gone in leaving {
        if let Some(group) = groups.iter_mut().find(|group| group.key == gone.group) {
            if group.windows.iter().all(|w| w.hwnd != gone.window.hwnd) {
                let at = gone.place.min(group.windows.len());
                group.windows.insert(at, gone.window.clone());
            }
        }
    }
}
