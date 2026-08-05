use std::time::Duration;

/// Single source of truth for the global shortcut.
///
/// Change this value if another application owns Ctrl+Shift+T.
pub const GLOBAL_HOTKEY: &str = "Ctrl+Shift+T";
pub const UIA_TIMEOUT: Duration = Duration::from_millis(900);
pub const UIA_SEARCH_BUDGET: Duration = Duration::from_millis(220);
pub const UIA_MAX_CANDIDATES: usize = 96;
pub const UIA_MAX_ANCESTORS: usize = 12;
pub const UIA_MAX_SUBTREE_ELEMENTS: usize = 128;
pub const ACADEMIC_CONTEXT_SIDE_CHARS: i32 = 480;
pub const CLIPBOARD_TIMEOUT: Duration = Duration::from_millis(900);
pub const CLIPBOARD_CHANGE_TIMEOUT: Duration = Duration::from_millis(450);
pub const CLIPBOARD_RETRY_DELAY: Duration = Duration::from_millis(12);
pub const CLIPBOARD_MAX_TEXT_CHARS: usize = 1_000_000;
pub const OVERLAY_SHORT_HIDE_DELAY: Duration = Duration::from_secs(3);
pub const OVERLAY_MEDIUM_HIDE_DELAY: Duration = Duration::from_secs(5);
pub const OVERLAY_LONG_HIDE_DELAY: Duration = Duration::from_secs(7);
pub const OVERLAY_HOVER_LEAVE_DELAY: Duration = Duration::from_secs(3);
pub const OVERLAY_DISMISS_COMPLETION_TIMEOUT: Duration = Duration::from_millis(1000);
pub const OVERLAY_DELIVERY_TIMEOUT: Duration = Duration::from_secs(12);
pub const OVERLAY_MIN_WIDTH: i32 = 220;
pub const OVERLAY_LOADING_WIDTH: i32 = 148;
pub const OVERLAY_ERROR_MIN_WIDTH: i32 = 320;
pub const OVERLAY_RESULT_MAX_WIDTH: i32 = 560;
pub const OVERLAY_COMPACT_MAX_CHARS: usize = 18;
pub const OVERLAY_COMPACT_HEIGHT: i32 = 64;
pub const OVERLAY_LOADING_HEIGHT: i32 = 58;
pub const OVERLAY_RESULT_MIN_HEIGHT: i32 = 94;
pub const OVERLAY_RESULT_MAX_HEIGHT: i32 = 320;
pub const OVERLAY_GAP: i32 = 10;
pub const SELECTION_BUTTON_SIZE: i32 = 40;
pub const SELECTION_BUTTON_GLYPH_SIZE: i32 = 20;
pub const SELECTION_BUTTON_GAP: i32 = 3;
pub const SELECTION_BUTTON_SETTLE_DELAY: Duration = Duration::from_millis(55);
pub const SELECTION_BUTTON_CAPTURE_TIMEOUT: Duration = Duration::from_millis(650);
pub const SELECTION_BUTTON_VISIBLE_DURATION: Duration = Duration::from_secs(5);
