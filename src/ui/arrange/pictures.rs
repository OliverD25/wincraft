use super::*;

/// The window's picture, registered on first use. Only possible once the
/// strip's own window exists, since it is the destination.
pub(super) fn thumbnail(shared: &mut Shared, hwnd: isize) -> Option<&mut Thumbnail> {
    registered(&mut shared.thumbs, shared.window, hwnd)
}

/// The second picture of a window that a ghost shows.
pub(super) fn ghost_thumbnail(shared: &mut Shared, hwnd: isize) -> Option<&mut Thumbnail> {
    registered(&mut shared.ghost_thumbs, shared.window, hwnd)
}

fn registered(
    thumbs: &mut HashMap<isize, Option<Thumbnail>>,
    window: isize,
    hwnd: isize,
) -> Option<&mut Thumbnail> {
    let destination = window as HWND;
    thumbs
        .entry(hwnd)
        .or_insert_with(|| {
            let thumb = (!destination.is_null())
                .then(|| Thumbnail::register(destination, hwnd as HWND))
                .flatten();
            if thumb.is_some() {
                open_timing::mark_latest(OpenStep::Pictures);
            }
            thumb
        })
        .as_mut()
}

/// Finds the part of each of the group's windows its picture shows, which
/// gives its card its shape; returns whether any changed. Only possible once
/// the strip's own window exists.
pub(super) fn refresh_sources(shared: &mut Shared, group: &ArrangeGroup) -> bool {
    if shared.window == 0 {
        return false;
    }
    shared
        .sources
        .retain(|hwnd, _| group.windows.iter().any(|window| window.hwnd == *hwnd));
    let mut changed = false;
    for window in &group.windows {
        let source = thumbnail(shared, window.hwnd)
            .and_then(|thumb| thumb.source_size())
            .map(|size| picture_source(dwm_thumbs::visible_frame(window.hwnd as HWND), size));
        if shared.sources.get(&window.hwnd).copied() != source {
            changed = true;
            match source {
                Some(source) => shared.sources.insert(window.hwnd, source),
                None => shared.sources.remove(&window.hwnd),
            };
        }
    }
    changed
}

/// Puts every picture where its card is. DWM draws pictures on top of
/// everything in the window, so one under the dragged card is hidden, and
/// pictures of windows without a card this frame are hidden too.
/// What is drawn over the cards this frame: the dragged card, and the
/// covers that hide every picture they touch.
pub(super) struct Over<'a> {
    pub(super) floating: Option<Rect>,
    pub(super) covers: &'a [Rect],
}

/// Whether a picture hides this frame: under the dragged card (except the
/// dragged window's own picture, which rides with it) or under any cover.
/// A picture on the open group's layer is above them all.
pub(super) fn is_covered(item: &Pending, over: &Over<'_>, dragged: Option<isize>) -> bool {
    if item.layer {
        return false;
    }
    let under_drag = over.floating.is_some_and(|rect| {
        (item.ghost || Some(item.hwnd) != dragged) && rect.intersects(item.dest)
    });
    under_drag || over.covers.iter().any(|rect| rect.intersects(item.dest))
}

pub(super) fn place_pictures(
    shared: &mut Shared,
    pending: &[Pending],
    over: Over<'_>,
    dragged: Option<isize>,
    pixels_per_point: f32,
) {
    for (thumbs, ghost) in [
        (&mut shared.thumbs, false),
        (&mut shared.ghost_thumbs, true),
    ] {
        for (hwnd, thumb) in thumbs.iter_mut() {
            let placed = pending.iter().any(|p| p.hwnd == *hwnd && p.ghost == ghost);
            if let Some(thumb) = thumb.as_mut().filter(|_| !placed) {
                thumb.place(None);
            }
        }
    }
    for item in pending {
        // The dragged window's own picture rides with the pointer; its
        // ghost stays behind like any other.
        let covered = is_covered(item, &over, dragged);
        let known = shared.sources.get(&item.hwnd).copied();
        let thumb = if item.ghost {
            ghost_thumbnail(shared, item.hwnd)
        } else {
            thumbnail(shared, item.hwnd)
        };
        let Some(thumb) = thumb else {
            continue;
        };
        let Some(size) = thumb.source_size() else {
            thumb.place(None);
            continue;
        };
        if covered {
            thumb.place(None);
            continue;
        }
        let source = known
            .unwrap_or_else(|| picture_source(dwm_thumbs::visible_frame(item.hwnd as HWND), size));
        // The card has the window's shape, so this only leaves a margin for
        // a window narrower or wider than a card can be.
        let dest = dwm_thumbs::fit_clear(item.dest, source.size(), item.clear);
        let placement =
            dwm_thumbs::crop(dest, item.clip, source).map(|(shown, cropped)| Placement {
                dest: dwm_thumbs::physical(shown, pixels_per_point),
                source: dwm_thumbs::physical(cropped, 1.0),
                opacity: item.opacity,
            });
        thumb.place(placement);
    }
}
