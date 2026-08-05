use serde::Serialize;

use crate::foreground_context::ForegroundContext;
use crate::translation::{LanguageProfile, TranslationMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CapturePhase {
    Capturing,
    Translating,
    Translated,
    CaptureFailed,
    TranslationFailed,
}

impl CapturePhase {
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Capturing | Self::Translating)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl ScreenRect {
    pub fn width(self) -> i32 {
        (self.right - self.left).max(0)
    }

    pub fn height(self) -> i32 {
        (self.bottom - self.top).max(0)
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturePayload {
    pub request_id: u64,
    pub phase: CapturePhase,
    pub success: bool,
    pub text: String,
    pub application_name: String,
    pub process_id: u32,
    pub capture_method: String,
    pub elapsed_ms: u128,
    pub selection_rect: Option<ScreenRect>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub focus_preserved: bool,
    pub clipboard_restored: Option<bool>,
    pub warning_code: Option<String>,
    pub language_profile: Option<LanguageProfile>,
    pub translation_mode: Option<TranslationMode>,
}

#[derive(Clone, Debug)]
pub struct CapturedSelection {
    pub text: String,
    pub context_before: Option<String>,
    pub context_after: Option<String>,
    pub source: String,
    pub selection_rect: Option<ScreenRect>,
    pub foreground_context: ForegroundContext,
    pub elapsed_ms: u128,
    pub clipboard_restored: Option<bool>,
    pub warning_code: Option<String>,
}
