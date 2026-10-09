use windows::Win32::Foundation::POINT;

use crate::{models::ScreenRect, selection_service::MouseSelectionHint};

pub(crate) fn intersects(a: ScreenRect, b: ScreenRect) -> bool {
    a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top
}

fn valid(r: &ScreenRect) -> bool {
    r.right > r.left && r.bottom > r.top
}

fn distance(p: POINT, r: ScreenRect) -> i64 {
    let dx = (i64::from(r.left) - i64::from(p.x))
        .max(0)
        .max(i64::from(p.x) - i64::from(r.right));
    let dy = (i64::from(r.top) - i64::from(p.y))
        .max(0)
        .max(i64::from(p.y) - i64::from(r.bottom));
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
}

/// Physical-pixel geometry. The full native hit window is kept on the same
/// work area; its transparent parts are clipped away from all known text.
pub(crate) fn place(
    lines: &[ScreenRect],
    nearby: Option<&[ScreenRect]>,
    hint: Option<MouseSelectionHint>,
    fallback: ScreenRect,
    work: ScreenRect,
    size: i32,
    glyph: i32,
    gap: i32,
) -> Option<ScreenRect> {
    if !valid(&work) || work.width() < size || work.height() < size {
        return None;
    }
    let focus = hint.map(|h| h.focus).unwrap_or(POINT {
        x: fallback.right,
        y: fallback.bottom,
    });
    let mut rows: Vec<_> = lines.iter().copied().filter(valid).collect();
    rows.sort_by_key(|r| (r.top, r.left));
    // UIA may split one visual line into multiple styled runs. Join nearby
    // runs on the same baseline, but do not bridge unrelated columns.
    let mut merged: Vec<ScreenRect> = Vec::new();
    for row in rows {
        if let Some(last) = merged.last_mut() {
            if (i64::from(last.top) - i64::from(row.top)).abs() <= i64::from(gap)
                && (i64::from(last.bottom) - i64::from(row.bottom)).abs() <= i64::from(gap)
                && i64::from(row.left) - i64::from(last.right) <= i64::from(glyph * 2)
            {
                *last = last.union(row);
                continue;
            }
        }
        merged.push(row);
    }
    let row = (if hint.is_some() {
        merged.iter().copied().min_by_key(|r| distance(focus, *r))
    } else {
        merged.last().copied()
    })
    .filter(|r| {
        r.height() <= size * 2
            && (hint.is_none() || distance(focus, *r) <= i64::from(size * 2).pow(2))
    });
    let reliable = row.is_some();
    let row = row.unwrap_or(ScreenRect {
        left: focus.x,
        right: focus.x.saturating_add(1),
        top: focus.y.saturating_sub(glyph / 2),
        bottom: focus.y.saturating_add(glyph / 2),
    });
    let forward = hint
        .map(|h| {
            let dy = i64::from(h.focus.y) - i64::from(h.anchor.y);
            if dy.abs() > i64::from(row.height() / 2) {
                dy > 0
            } else if h.focus.x.abs_diff(h.anchor.x) > 4 {
                h.focus.x > h.anchor.x
            } else {
                i64::from(focus.x) - i64::from(row.left)
                    >= i64::from(row.right) - i64::from(focus.x)
            }
        })
        .unwrap_or(true);
    let inset = (size - glyph) / 2;
    let cy = row.top + (row.height() - size) / 2;
    let side = if forward {
        row.right + gap - inset
    } else {
        row.left - gap - glyph - inset
    };
    let opposite = if forward {
        row.left - gap - glyph - inset
    } else {
        row.right + gap - inset
    };
    let aligned = if forward {
        row.right - glyph - inset
    } else {
        row.left - inset
    };
    let above = row.top - gap - glyph - inset;
    let below = row.bottom + gap - inset;
    let mut obstacles: Vec<_> = lines.iter().copied().filter(valid).collect();
    obstacles.extend(nearby.unwrap_or_default().iter().copied().filter(valid));
    // Unknown neighboring text is not evidence of empty space. Prefer above
    // or below in that case. Accurate line geometry enables inline placement.
    let side_known = reliable && nearby.is_some_and(|rs| rs.iter().any(|r| intersects(*r, row)));
    let mut candidates = Vec::new();
    if side_known {
        candidates.push((side, cy));
    }
    if forward {
        candidates.extend([(aligned, below), (aligned, above)]);
    } else {
        candidates.extend([(aligned, above), (aligned, below)]);
    }
    if side_known
        && (i64::from(opposite + size / 2) - i64::from(focus.x)).abs() <= i64::from(size * 2)
    {
        candidates.push((opposite, cy));
    }
    // Dense paragraphs may have no vertical gap. Use the nearest margin of
    // the inspected line; going beyond the inspected rows could cover unread
    // neighboring text. Only fall back this far when all close slots fail.
    if side_known
        && let Some(full_line) = obstacles
            .iter()
            .copied()
            .filter(|r| r.top < row.bottom && r.bottom > row.top)
            .reduce(ScreenRect::union)
    {
        let mut margins = [
            full_line.left - gap - glyph - inset,
            full_line.right + gap - inset,
        ];
        margins.sort_by_key(|x| (i64::from(*x + size / 2) - i64::from(focus.x)).abs());
        candidates.extend(margins.map(|x| (x, cy)));
    }
    for (x, y) in candidates {
        let x = x.clamp(work.left, work.right - size);
        let y = y.clamp(work.top, work.bottom - size);
        let paint = ScreenRect {
            left: x + inset,
            top: y + inset,
            right: x + inset + glyph,
            bottom: y + inset + glyph,
        };
        let padded = ScreenRect {
            left: paint.left - gap,
            top: paint.top - gap,
            right: paint.right + gap,
            bottom: paint.bottom + gap,
        };
        if obstacles.iter().any(|r| intersects(padded, *r)) {
            continue;
        }
        // Never place a fallback directly beneath the release pointer.
        if !reliable && focus.x >= x && focus.x < x + size && focus.y >= y && focus.y < y + size {
            continue;
        }
        return Some(ScreenRect {
            left: x,
            top: y,
            right: x + size,
            bottom: y + size,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    fn r(x: i32, y: i32, w: i32, h: i32) -> ScreenRect {
        ScreenRect {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        }
    }
    fn hint(ax: i32, ay: i32, x: i32, y: i32) -> Option<MouseSelectionHint> {
        Some(MouseSelectionHint {
            anchor: POINT { x: ax, y: ay },
            focus: POINT { x, y },
        })
    }
    const WORK: ScreenRect = ScreenRect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    #[test]
    fn multiline_uses_short_release_line_not_union_center() {
        let lines = [r(100, 100, 600, 24), r(100, 130, 120, 24)];
        let b = place(
            &lines,
            Some(&lines),
            hint(100, 112, 218, 142),
            lines[0],
            WORK,
            40,
            20,
            5,
        )
        .unwrap();
        assert_eq!(b.left + 10, 225);
        assert_eq!(b.top + 20, 142);
        let back = place(
            &lines,
            Some(&lines),
            hint(218, 142, 100, 112),
            lines[0],
            WORK,
            40,
            20,
            5,
        )
        .unwrap();
        assert_eq!(back.right - 10, 95);
        assert_eq!(back.top + 20, 112);
    }
    #[test]
    fn inline_neighbors_force_vertical_escape() {
        let selected = r(200, 200, 70, 24);
        let text = [r(100, 200, 450, 24)];
        let b = place(
            &[selected],
            Some(&text),
            hint(200, 210, 270, 210),
            selected,
            WORK,
            40,
            20,
            5,
        )
        .unwrap();
        assert_eq!(b.top + 10, 229);
        assert!(!intersects(r(b.left + 10, b.top + 10, 20, 20), text[0]));
    }

    #[test]
    fn dense_paragraph_uses_nearest_line_margin_not_uninspected_next_row() {
        let selected = r(160, 200, 60, 24);
        let surrounding = [
            r(100, 170, 500, 24),
            r(100, 200, 500, 24),
            r(100, 230, 500, 24),
        ];
        let b = place(
            &[selected],
            Some(&surrounding),
            hint(160, 212, 220, 212),
            selected,
            WORK,
            40,
            20,
            5,
        )
        .unwrap();
        assert_eq!(b.right - 10, 95);
        assert_eq!(b.top + 20, 212);
    }
    #[test]
    fn edge_clamping_never_pushes_painted_icon_onto_text() {
        for scale in [1, 2, 3] {
            let work = r(-1920 * scale, -200 * scale, 1920 * scale, 1080 * scale);
            let selected = r(-130 * scale, 850 * scale, 125 * scale, 24 * scale);
            let b = place(
                &[selected],
                Some(&[selected]),
                hint(-120 * scale, 860 * scale, -5 * scale, 860 * scale),
                selected,
                work,
                40 * scale,
                20 * scale,
                5 * scale,
            )
            .unwrap();
            assert!(
                b.left >= work.left
                    && b.right <= work.right
                    && b.top >= work.top
                    && b.bottom <= work.bottom
            );
            assert!(!intersects(
                r(
                    b.left + 10 * scale,
                    b.top + 10 * scale,
                    20 * scale,
                    20 * scale
                ),
                selected
            ));
        }
    }
    #[test]
    fn missing_or_remote_geometry_stays_near_release() {
        for lines in [vec![], vec![r(50, 50, 100, 20)]] {
            let b = place(
                &lines,
                None,
                hint(500, 500, 700, 700),
                r(50, 50, 100, 20),
                WORK,
                40,
                20,
                5,
            )
            .unwrap();
            assert!((b.left - 700).abs() < 50 && (b.top - 700).abs() < 70);
        }
    }
    #[test]
    fn fully_occupied_work_area_suppresses_instead_of_covering_text() {
        assert!(
            place(
                &[WORK],
                Some(&[WORK]),
                hint(100, 100, 200, 200),
                WORK,
                WORK,
                40,
                20,
                5
            )
            .is_none()
        );
        assert!(place(&[], None, None, WORK, r(0, 0, 20, 20), 40, 20, 5).is_none());
    }
    #[test]
    fn keyboard_selection_uses_last_line_and_styled_runs_join() {
        let lines = [
            r(100, 100, 600, 24),
            r(100, 130, 60, 24),
            r(160, 130, 60, 24),
        ];
        let b = place(
            &lines,
            Some(&lines),
            None,
            r(100, 100, 600, 54),
            WORK,
            40,
            20,
            5,
        )
        .unwrap();
        assert_eq!(b.left + 10, 225);
        assert_eq!(b.top + 20, 142);
    }
    #[test]
    fn fractional_dpi_and_screen_corners_keep_glyph_clear() {
        for dpi in [96, 120, 144, 168, 192] {
            let s = |v| crate::overlay_policy::scale_for_dpi(v, dpi);
            for (x, y) in [(0, 0), (1700, 0), (0, 1020), (1700, 1020), (700, 500)] {
                let work = r(-s(1920), -s(1080), s(1920), s(1080));
                let selected = r(work.left + s(x), work.top + s(y), s(210), s(22));
                let b = place(
                    &[selected],
                    Some(&[selected]),
                    hint(
                        selected.left,
                        selected.top,
                        selected.right,
                        selected.bottom - 1,
                    ),
                    selected,
                    work,
                    s(40),
                    s(20),
                    s(5),
                )
                .unwrap();
                assert!(
                    b.left >= work.left
                        && b.right <= work.right
                        && b.top >= work.top
                        && b.bottom <= work.bottom
                );
                let inset = (s(40) - s(20)) / 2;
                assert!(!intersects(
                    r(b.left + inset, b.top + inset, s(20), s(20)),
                    selected
                ));
            }
        }
    }
}
