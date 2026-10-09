use std::time::{Duration, Instant};

use crate::{
    config::{
        OVERLAY_COMPACT_HEIGHT, OVERLAY_COMPACT_MAX_CHARS, OVERLAY_ERROR_MIN_WIDTH,
        OVERLAY_HOVER_LEAVE_DELAY, OVERLAY_LOADING_HEIGHT, OVERLAY_LOADING_WIDTH,
        OVERLAY_LONG_HIDE_DELAY, OVERLAY_MEDIUM_HIDE_DELAY, OVERLAY_MIN_WIDTH,
        OVERLAY_RESULT_MAX_HEIGHT, OVERLAY_RESULT_MAX_WIDTH, OVERLAY_RESULT_MIN_HEIGHT,
        OVERLAY_SHORT_HIDE_DELAY,
    },
    models::{CapturePayload, CapturePhase, ScreenRect},
    translation::TranslationMode,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OverlayPlacement {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DismissalClock {
    initial_deadline: Instant,
    hover_mode: bool,
    leave_deadline: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DismissalHandshakeState {
    Waiting,
    CancelledByHover,
    TimedOut,
    Superseded,
}

pub(crate) fn dismissal_handshake_state(
    shown_request: u64,
    request_id: u64,
    hovered: bool,
    now: Instant,
    completion_deadline: Instant,
) -> DismissalHandshakeState {
    if shown_request != request_id {
        DismissalHandshakeState::Superseded
    } else if hovered {
        DismissalHandshakeState::CancelledByHover
    } else if now >= completion_deadline {
        DismissalHandshakeState::TimedOut
    } else {
        DismissalHandshakeState::Waiting
    }
}

impl DismissalClock {
    pub(crate) fn new(started: Instant, initial_delay: Duration) -> Self {
        Self {
            initial_deadline: started + initial_delay,
            hover_mode: false,
            leave_deadline: None,
        }
    }

    pub(crate) fn should_hide(&mut self, now: Instant, hovered: bool) -> bool {
        // Preserve the original reading period regardless of early interaction.
        if now < self.initial_deadline {
            return false;
        }
        if hovered {
            self.hover_mode = true;
            self.leave_deadline = None;
            return false;
        }

        if self.hover_mode {
            let leave_deadline = self
                .leave_deadline
                .get_or_insert(now + OVERLAY_HOVER_LEAVE_DELAY);
            return now >= *leave_deadline;
        }

        now >= self.initial_deadline
    }
}

pub(crate) fn calculate_overlay_position(
    selection: ScreenRect,
    work_area: ScreenRect,
    overlay_width: i32,
    overlay_height: i32,
    gap: i32,
) -> OverlayPlacement {
    let max_x = (work_area.right - overlay_width).max(work_area.left);
    let max_y = (work_area.bottom - overlay_height).max(work_area.top);
    // Capture providers can return a selection spanning monitors or off-screen text.
    // Anchor to the visible portion on the monitor chosen by monitor_metrics.
    let selection = ScreenRect {
        left: selection.left.clamp(work_area.left, work_area.right),
        right: selection.right.clamp(work_area.left, work_area.right),
        top: selection.top.clamp(work_area.top, work_area.bottom),
        bottom: selection.bottom.clamp(work_area.top, work_area.bottom),
    };
    let x = selection.left.clamp(work_area.left, max_x);
    let y = selection.top.clamp(work_area.top, max_y);
    // Slide along the edge before changing sides. In particular, right-edge
    // selections should still get a panel below them when vertical space permits.
    let candidates = [
        (x, selection.bottom.saturating_add(gap)),
        (
            x,
            selection
                .top
                .saturating_sub(gap.saturating_add(overlay_height)),
        ),
        (selection.right.saturating_add(gap), y),
        (
            selection
                .left
                .saturating_sub(gap.saturating_add(overlay_width)),
            y,
        ),
    ];
    for (x, y) in candidates {
        if x >= work_area.left && x <= max_x && y >= work_area.top && y <= max_y {
            return OverlayPlacement { x, y };
        }
    }

    // No side fits: compare actual overlap after moving each candidate into view.
    // Stable candidate order breaks ties, retaining the usual below-first behavior.
    candidates
        .into_iter()
        .map(|(x, y)| OverlayPlacement {
            x: x.clamp(work_area.left, max_x),
            y: y.clamp(work_area.top, max_y),
        })
        .min_by_key(|p| {
            let overlap_width = (p.x.saturating_add(overlay_width).min(selection.right)
                - p.x.max(selection.left))
            .max(0) as i64;
            let overlap_height = (p.y.saturating_add(overlay_height).min(selection.bottom)
                - p.y.max(selection.top))
            .max(0) as i64;
            overlap_width * overlap_height
        })
        .unwrap()
}

pub(crate) fn clamp_overlay_position(
    current_x: i32,
    current_y: i32,
    work_area: ScreenRect,
    overlay_width: i32,
    overlay_height: i32,
) -> OverlayPlacement {
    let max_x = (work_area.right - overlay_width).max(work_area.left);
    let max_y = (work_area.bottom - overlay_height).max(work_area.top);
    OverlayPlacement {
        x: current_x.clamp(work_area.left, max_x),
        y: current_y.clamp(work_area.top, max_y),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OverlayLogicalSize {
    pub(crate) width: i32,
    pub(crate) height: i32,
}

pub(crate) fn overlay_logical_size(
    payload: &CapturePayload,
    maximum_width: i32,
) -> OverlayLogicalSize {
    if payload.phase == CapturePhase::CaptureFailed
        && payload.error_code.as_deref() == Some("NO_TEXT_SELECTED")
    {
        return OverlayLogicalSize {
            width: 188.min(maximum_width.max(1)),
            height: 88,
        };
    }
    let maximum_width = maximum_width
        .max(OVERLAY_MIN_WIDTH)
        .min(OVERLAY_RESULT_MAX_WIDTH);
    match payload.phase {
        CapturePhase::Capturing | CapturePhase::Translating => OverlayLogicalSize {
            width: OVERLAY_LOADING_WIDTH.min(maximum_width),
            height: OVERLAY_LOADING_HEIGHT,
        },
        CapturePhase::CaptureFailed | CapturePhase::TranslationFailed => {
            let message = payload.error_message.as_deref().unwrap_or_default();
            let width =
                (estimated_line_width(message) + 24).clamp(OVERLAY_ERROR_MIN_WIDTH, maximum_width);
            let content_width = (width - 24).max(1);
            let lines = estimated_wrapped_lines(message, content_width).clamp(1, 5) as i32;
            OverlayLogicalSize {
                width,
                height: (70 + lines * 22).clamp(94, 190),
            }
        }
        CapturePhase::Translated => {
            let tone_note = payload
                .tone_note
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .filter(|_| {
                    payload.success
                        && payload.translation_mode == Some(TranslationMode::Conversational)
                });
            let compact = tone_note.is_none()
                && !payload.text.contains('\n')
                && payload.text.chars().count() <= OVERLAY_COMPACT_MAX_CHARS;
            let text_width = estimated_line_width(&payload.text);
            if compact {
                let explanation_action_width = if payload.success
                    && matches!(payload.translation_mode, Some(TranslationMode::Academic))
                {
                    26
                } else {
                    0
                };
                return OverlayLogicalSize {
                    width: (text_width + 156 + explanation_action_width)
                        .clamp(OVERLAY_MIN_WIDTH, maximum_width),
                    height: OVERLAY_COMPACT_HEIGHT,
                };
            }

            let width = (text_width + 24).clamp(300, maximum_width);
            let content_width = (width - 24).max(1);
            let lines = estimated_wrapped_lines(&payload.text, content_width).clamp(1, 10) as i32;
            OverlayLogicalSize {
                width,
                height: (70
                    + lines * 23
                    + tone_note.map_or(0, |note| {
                        18 + estimated_wrapped_lines(note, content_width) as i32 * 18
                    }))
                .clamp(OVERLAY_RESULT_MIN_HEIGHT, OVERLAY_RESULT_MAX_HEIGHT),
            }
        }
    }
}

fn estimated_line_width(text: &str) -> i32 {
    text.lines()
        .map(|line| {
            line.chars()
                .map(|character| {
                    if character == '\t' {
                        28
                    } else if character.is_ascii() {
                        7
                    } else {
                        15
                    }
                })
                .sum::<i32>()
        })
        .max()
        .unwrap_or(0)
}

fn estimated_wrapped_lines(text: &str, content_width: i32) -> usize {
    text.lines()
        .map(|line| {
            let width = estimated_line_width(line).max(1) as usize;
            width.div_ceil(content_width.max(1) as usize)
        })
        .sum::<usize>()
        .max(1)
}

pub(crate) fn auto_hide_delay(payload: &CapturePayload) -> Duration {
    if payload.phase == CapturePhase::CaptureFailed
        && payload.error_code.as_deref() == Some("NO_TEXT_SELECTED")
    {
        return Duration::from_secs(2);
    }
    if payload.phase != CapturePhase::Translated {
        return OVERLAY_MEDIUM_HIDE_DELAY;
    }

    match effective_character_count(&payload.text) {
        0..=32 => OVERLAY_SHORT_HIDE_DELAY,
        33..=80 => OVERLAY_MEDIUM_HIDE_DELAY,
        _ => OVERLAY_LONG_HIDE_DELAY,
    }
}

fn effective_character_count(text: &str) -> usize {
    text.chars()
        .filter(|character| !character.is_whitespace() && !is_overlay_punctuation(*character))
        .count()
}

fn is_overlay_punctuation(character: char) -> bool {
    character.is_ascii_punctuation()
        || matches!(
            character as u32,
            0x2000..=0x206F | 0x3000..=0x303F | 0xFF01..=0xFF65
        )
}

pub(crate) fn scale_for_dpi(logical: i32, dpi: u32) -> i32 {
    ((logical as i64 * dpi as i64 + 48) / 96) as i32
}

#[cfg(test)]
mod tests {
    #[test]
    fn empty_selection_hint_is_short_but_read_errors_keep_their_normal_timeout() {
        let mut hint = payload(1, "");
        hint.phase = CapturePhase::CaptureFailed;
        hint.error_code = Some("NO_TEXT_SELECTED".into());
        assert_eq!(auto_hide_delay(&hint), Duration::from_secs(2));
        let size = overlay_logical_size(&hint, 600);
        assert_eq!((size.width, size.height), (188, 88));
        hint.error_code = Some("CLIPBOARD_TIMEOUT".into());
        assert_eq!(auto_hide_delay(&hint), OVERLAY_MEDIUM_HIDE_DELAY);
    }
    use super::*;

    const WORK: ScreenRect = ScreenRect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1040,
    };

    fn assert_inside(position: OverlayPlacement) {
        assert!(position.x >= WORK.left);
        assert!(position.y >= WORK.top);
        assert!(position.x + 430 <= WORK.right);
        assert!(position.y + 270 <= WORK.bottom);
    }

    #[test]
    fn all_screen_corners_remain_inside_work_area() {
        let selections = [
            ScreenRect {
                left: 0,
                top: 0,
                right: 20,
                bottom: 20,
            },
            ScreenRect {
                left: 1900,
                top: 0,
                right: 1920,
                bottom: 20,
            },
            ScreenRect {
                left: 0,
                top: 1020,
                right: 20,
                bottom: 1040,
            },
            ScreenRect {
                left: 1900,
                top: 1020,
                right: 1920,
                bottom: 1040,
            },
        ];
        for selection in selections {
            assert_inside(calculate_overlay_position(selection, WORK, 430, 270, 12));
        }
    }

    #[test]
    fn supports_monitors_with_negative_coordinates() {
        let work = ScreenRect {
            left: -1920,
            top: -120,
            right: 0,
            bottom: 960,
        };
        let selection = ScreenRect {
            left: -1918,
            top: -118,
            right: -1880,
            bottom: -90,
        };
        let result = calculate_overlay_position(selection, work, 430, 270, 12);
        assert!(result.x >= work.left && result.x + 430 <= work.right);
        assert!(result.y >= work.top && result.y + 270 <= work.bottom);
    }

    #[test]
    fn prefers_below_selection_when_space_is_available() {
        let selection = ScreenRect {
            left: 500,
            top: 200,
            right: 600,
            bottom: 230,
        };
        assert_eq!(
            calculate_overlay_position(selection, WORK, 430, 270, 12),
            OverlayPlacement { x: 500, y: 242 }
        );
    }

    #[test]
    fn right_edge_slides_left_without_switching_to_the_side() {
        let selection = ScreenRect {
            left: 1800,
            top: 200,
            right: 1900,
            bottom: 230,
        };
        assert_eq!(
            calculate_overlay_position(selection, WORK, 430, 270, 12),
            OverlayPlacement { x: 1490, y: 242 }
        );
    }

    #[test]
    fn bottom_right_uses_space_above_without_covering_selection() {
        let selection = ScreenRect {
            left: 1800,
            top: 950,
            right: 1900,
            bottom: 990,
        };
        assert_eq!(
            calculate_overlay_position(selection, WORK, 430, 270, 12),
            OverlayPlacement { x: 1490, y: 668 }
        );
    }

    #[test]
    fn tall_selection_uses_side_space() {
        let selection = ScreenRect {
            left: 500,
            top: 100,
            right: 800,
            bottom: 1000,
        };
        assert_eq!(
            calculate_overlay_position(selection, WORK, 430, 270, 12),
            OverlayPlacement { x: 812, y: 100 }
        );
    }

    #[test]
    fn crowded_selection_chooses_less_overlap_instead_of_always_below() {
        let selection = ScreenRect {
            left: 300,
            top: 200,
            right: 1700,
            bottom: 1000,
        };
        assert_eq!(
            calculate_overlay_position(selection, WORK, 430, 270, 12),
            OverlayPlacement { x: 300, y: 0 }
        );
    }

    #[test]
    fn offscreen_selection_anchors_to_visible_portion() {
        let selection = ScreenRect {
            left: -900,
            top: 200,
            right: 600,
            bottom: 230,
        };
        assert_eq!(
            calculate_overlay_position(selection, WORK, 430, 270, 12),
            OverlayPlacement { x: 0, y: 242 }
        );
    }

    #[test]
    fn placement_stays_inside_across_sizes_and_screen_positions() {
        for width in [180, 430, 840, 1920] {
            for height in [60, 270, 620, 1040] {
                for x in (-200..=2200).step_by(100) {
                    for y in (-200..=1200).step_by(100) {
                        let selection = ScreenRect {
                            left: x,
                            top: y,
                            right: x + 250,
                            bottom: y + 80,
                        };
                        let p = calculate_overlay_position(selection, WORK, width, height, 12);
                        assert!(
                            p.x >= 0 && p.y >= 0 && p.x + width <= 1920 && p.y + height <= 1040
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn resized_overlay_keeps_its_position_when_it_still_fits() {
        assert_eq!(
            clamp_overlay_position(500, 200, WORK, 430, 560),
            OverlayPlacement { x: 500, y: 200 }
        );
    }

    #[test]
    fn resized_overlay_moves_only_enough_to_remain_inside_the_work_area() {
        assert_eq!(
            clamp_overlay_position(1700, 820, WORK, 430, 560),
            OverlayPlacement { x: 1490, y: 480 }
        );
    }

    #[test]
    fn logical_overlay_size_scales_for_dpi() {
        assert_eq!(scale_for_dpi(430, 96), 430);
        assert_eq!(scale_for_dpi(430, 144), 645);
        assert_eq!(scale_for_dpi(270, 192), 540);
    }

    #[test]
    fn overlay_expands_horizontally_before_it_wraps() {
        let mut loading = payload(19, "");
        loading.phase = CapturePhase::Capturing;
        assert_eq!(
            overlay_logical_size(&loading, OVERLAY_RESULT_MAX_WIDTH),
            OverlayLogicalSize {
                width: OVERLAY_LOADING_WIDTH,
                height: OVERLAY_LOADING_HEIGHT,
            }
        );

        let short = payload(20, "简短译文");
        let medium = payload(21, &"适合在一行中展示的中等长度译文".repeat(2));
        let long = payload(22, &"达到最大宽度以后才开始自然换行".repeat(40));
        let short_size = overlay_logical_size(&short, OVERLAY_RESULT_MAX_WIDTH);
        let medium_size = overlay_logical_size(&medium, OVERLAY_RESULT_MAX_WIDTH);
        let long_size = overlay_logical_size(&long, OVERLAY_RESULT_MAX_WIDTH);

        assert_eq!(short_size.height, OVERLAY_COMPACT_HEIGHT);
        assert!(medium_size.width > short_size.width);
        assert!(medium_size.width <= OVERLAY_RESULT_MAX_WIDTH);
        assert_eq!(long_size.width, OVERLAY_RESULT_MAX_WIDTH);
        assert!(long_size.height > medium_size.height);
        assert!(long_size.height <= OVERLAY_RESULT_MAX_HEIGHT);
    }

    #[test]
    fn available_screen_width_caps_the_overlay() {
        let result = payload(23, &"较长译文".repeat(80));
        let size = overlay_logical_size(&result, 400);
        assert_eq!(size.width, 400);
        assert!(size.height > OVERLAY_RESULT_MIN_HEIGHT);
    }

    #[test]
    fn academic_compact_result_reserves_space_for_explanation_action() {
        let conversational = payload(25, "学术模式短译文");
        let mut academic = payload(26, "学术模式短译文");
        academic.translation_mode = Some(crate::translation::TranslationMode::Academic);

        let conversational_size = overlay_logical_size(&conversational, OVERLAY_RESULT_MAX_WIDTH);
        let academic_size = overlay_logical_size(&academic, OVERLAY_RESULT_MAX_WIDTH);

        assert_eq!(academic_size.width, conversational_size.width + 26);
        assert_eq!(academic_size.height, conversational_size.height);
    }

    #[test]
    fn conversational_note_reserves_height_without_changing_academic_layout() {
        let mut daily = payload(27, "有道理。");
        let compact = overlay_logical_size(&daily, OVERLAY_RESULT_MAX_WIDTH);
        daily.tone_note = Some("口语中表示认可对方的判断。".to_owned());
        let annotated = overlay_logical_size(&daily, OVERLAY_RESULT_MAX_WIDTH);
        assert!(annotated.height > compact.height);
        assert!(annotated.width >= 300);
        daily.translation_mode = Some(TranslationMode::Academic);
        let academic_with_note = overlay_logical_size(&daily, OVERLAY_RESULT_MAX_WIDTH);
        daily.tone_note = None;
        assert_eq!(
            academic_with_note,
            overlay_logical_size(&daily, OVERLAY_RESULT_MAX_WIDTH)
        );
    }

    #[test]
    fn five_dictionary_senses_expand_into_separate_rows() {
        let dictionary = payload(
            24,
            "1. n. first meaning\n2. v. second meaning\n3. adj. third meaning\n4. n. fourth meaning\n5. v. fifth meaning",
        );

        let size = overlay_logical_size(&dictionary, OVERLAY_RESULT_MAX_WIDTH);

        assert_eq!(size.height, 185);
        assert!(size.height > OVERLAY_COMPACT_HEIGHT);
        assert!(size.height <= OVERLAY_RESULT_MAX_HEIGHT);
    }

    #[test]
    fn reading_time_uses_three_effective_character_tiers() {
        assert_eq!(
            auto_hide_delay(&payload(20, &"译".repeat(32))),
            OVERLAY_SHORT_HIDE_DELAY
        );
        assert_eq!(
            auto_hide_delay(&payload(21, &"译".repeat(33))),
            OVERLAY_MEDIUM_HIDE_DELAY
        );
        assert_eq!(
            auto_hide_delay(&payload(22, &"译".repeat(80))),
            OVERLAY_MEDIUM_HIDE_DELAY
        );
        assert_eq!(
            auto_hide_delay(&payload(23, &"译".repeat(81))),
            OVERLAY_LONG_HIDE_DELAY
        );
    }

    #[test]
    fn whitespace_and_punctuation_do_not_inflate_reading_tier() {
        assert_eq!(effective_character_count("你，好！\n（测试）..."), 4);
        assert_eq!(
            auto_hide_delay(&payload(24, &format!("{}，！？\n", "译".repeat(32)))),
            OVERLAY_SHORT_HIDE_DELAY
        );
    }

    #[test]
    fn untouched_overlay_uses_its_initial_tier_deadline() {
        let started = Instant::now();
        let mut clock = DismissalClock::new(started, OVERLAY_SHORT_HIDE_DELAY);

        assert!(!clock.should_hide(
            started + OVERLAY_SHORT_HIDE_DELAY - Duration::from_millis(1),
            false
        ));
        assert!(clock.should_hide(started + OVERLAY_SHORT_HIDE_DELAY, false));
    }

    #[test]
    fn automatic_dismiss_handshake_waits_for_frontend_completion() {
        let started = Instant::now();
        let deadline = started + crate::config::OVERLAY_DISMISS_COMPLETION_TIMEOUT;

        assert_eq!(
            dismissal_handshake_state(31, 31, false, deadline - Duration::from_millis(1), deadline,),
            DismissalHandshakeState::Waiting
        );
        assert_eq!(
            dismissal_handshake_state(31, 31, false, deadline, deadline),
            DismissalHandshakeState::TimedOut
        );
    }

    #[test]
    fn automatic_dismiss_handshake_honors_hover_and_newer_requests() {
        let started = Instant::now();
        let deadline = started + crate::config::OVERLAY_DISMISS_COMPLETION_TIMEOUT;

        assert_eq!(
            dismissal_handshake_state(31, 31, true, started, deadline),
            DismissalHandshakeState::CancelledByHover
        );
        assert_eq!(
            dismissal_handshake_state(32, 31, false, started, deadline),
            DismissalHandshakeState::Superseded
        );
    }

    #[test]
    fn early_hover_preserves_initial_deadline() {
        let started = Instant::now();
        let mut clock = DismissalClock::new(started, Duration::from_secs(10));
        assert!(!clock.should_hide(started + Duration::from_secs(1), true));
        assert!(!clock.should_hide(started + Duration::from_secs(2), false));
        assert!(!clock.should_hide(started + Duration::from_secs(9), false));
        assert!(clock.should_hide(started + Duration::from_secs(10), false));
    }

    #[test]
    fn expiry_waits_for_release_and_reentry_restarts_half_second_grace() {
        let started = Instant::now();
        let mut clock = DismissalClock::new(started, Duration::from_secs(10));
        assert!(!clock.should_hide(started + Duration::from_secs(10), true));
        assert!(!clock.should_hide(started + Duration::from_secs(30), true));
        let left = started + Duration::from_secs(31);
        assert!(!clock.should_hide(left, false));
        assert!(!clock.should_hide(left + Duration::from_millis(499), false));
        assert!(!clock.should_hide(left + Duration::from_millis(499), true));
        let left_again = left + Duration::from_secs(1);
        assert!(!clock.should_hide(left_again, false));
        assert!(!clock.should_hide(left_again + Duration::from_millis(499), false));
        assert!(clock.should_hide(left_again + Duration::from_millis(500), false));
    }

    fn payload(request_id: u64, text: &str) -> CapturePayload {
        CapturePayload {
            request_id,
            phase: CapturePhase::Translated,
            success: true,
            text: text.to_owned(),
            application_name: "test".to_owned(),
            process_id: 1,
            capture_method: "UIA:TextPattern".to_owned(),
            elapsed_ms: 76,
            selection_rect: None,
            error_code: None,
            error_message: None,
            focus_preserved: true,
            clipboard_restored: None,
            warning_code: None,
            language_profile: Some(crate::translation::LanguageProfile::analyze(text)),
            translation_mode: Some(crate::translation::TranslationMode::Conversational),
            tone_note: None,
        }
    }
}
