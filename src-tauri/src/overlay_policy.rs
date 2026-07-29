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

    let candidates = [
        (selection.left, selection.bottom.saturating_add(gap)),
        (
            selection.left,
            selection.top.saturating_sub(gap + overlay_height),
        ),
        (selection.right.saturating_add(gap), selection.top),
        (
            selection.left.saturating_sub(gap + overlay_width),
            selection.top,
        ),
    ];

    for (x, y) in candidates {
        if x >= work_area.left
            && y >= work_area.top
            && x.saturating_add(overlay_width) <= work_area.right
            && y.saturating_add(overlay_height) <= work_area.bottom
        {
            return OverlayPlacement { x, y };
        }
    }

    OverlayPlacement {
        x: selection.left.clamp(work_area.left, max_x),
        y: selection
            .bottom
            .saturating_add(gap)
            .clamp(work_area.top, max_y),
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
            let compact = !payload.text.contains('\n')
                && payload.text.chars().count() <= OVERLAY_COMPACT_MAX_CHARS;
            let text_width = estimated_line_width(&payload.text);
            if compact {
                return OverlayLogicalSize {
                    width: (text_width + 156).clamp(OVERLAY_MIN_WIDTH, maximum_width),
                    height: OVERLAY_COMPACT_HEIGHT,
                };
            }

            let width = (text_width + 24).clamp(300, maximum_width);
            let content_width = (width - 24).max(1);
            let lines = estimated_wrapped_lines(&payload.text, content_width).clamp(1, 10) as i32;
            OverlayLogicalSize {
                width,
                height: (70 + lines * 23)
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
    fn hover_replaces_the_initial_tier_with_a_fresh_leave_delay() {
        let started = Instant::now();
        let mut clock = DismissalClock::new(started, OVERLAY_SHORT_HIDE_DELAY);

        assert!(!clock.should_hide(started + Duration::from_secs(1), true));
        assert!(!clock.should_hide(started + Duration::from_secs(2), false));
        assert!(!clock.should_hide(
            started + Duration::from_secs(5) - Duration::from_millis(1),
            false
        ));
        assert!(clock.should_hide(started + Duration::from_secs(5), false));
    }

    #[test]
    fn returning_to_the_overlay_restarts_the_leave_delay() {
        let started = Instant::now();
        let mut clock = DismissalClock::new(started, OVERLAY_SHORT_HIDE_DELAY);

        assert!(!clock.should_hide(started + Duration::from_secs(1), true));
        assert!(!clock.should_hide(started + Duration::from_secs(2), false));
        assert!(!clock.should_hide(started + Duration::from_secs(4), true));
        assert!(!clock.should_hide(started + Duration::from_secs(5), false));
        assert!(!clock.should_hide(
            started + Duration::from_secs(8) - Duration::from_millis(1),
            false
        ));
        assert!(clock.should_hide(started + Duration::from_secs(8), false));
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
        }
    }
}
