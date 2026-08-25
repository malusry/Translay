use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use serde::Serialize;
use tauri::{Emitter, Manager, PhysicalPosition};

use super::windows::{hide_tray_feedback_window, show_tray_feedback_window};

const FEEDBACK_VISIBLE_DURATION: Duration = Duration::from_millis(1050);
const FEEDBACK_FADE_DURATION: Duration = Duration::from_millis(160);
const RAPID_CLICK_GUARD: Duration = Duration::from_millis(280);

#[derive(Clone, Default)]
pub(super) struct TrayFeedbackManager {
    inner: Arc<TrayFeedbackState>,
}

#[derive(Default)]
struct TrayFeedbackState {
    generation: AtomicU64,
    last_toggle: Mutex<Option<Instant>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrayFeedbackPayload {
    generation: u64,
    message: String,
}

impl TrayFeedbackManager {
    pub(super) fn accepts_toggle(&self) -> bool {
        let now = Instant::now();
        let mut last_toggle = self.inner.last_toggle.lock().unwrap();
        if last_toggle.is_some_and(|last| now.duration_since(last) < RAPID_CLICK_GUARD) {
            return false;
        }
        *last_toggle = Some(now);
        true
    }

    pub(super) fn show(
        &self,
        app: &tauri::AppHandle,
        anchor: PhysicalPosition<f64>,
        message: String,
    ) -> Result<(), String> {
        let generation = self.inner.generation.fetch_add(1, Ordering::AcqRel) + 1;
        show_tray_feedback_window(app, anchor)?;
        let window = app
            .get_webview_window("tray-feedback")
            .ok_or_else(|| "找不到托盘风格切换提示".to_owned())?;
        window
            .emit(
                "tray-style-feedback-show",
                TrayFeedbackPayload {
                    generation,
                    message,
                },
            )
            .map_err(|error| format!("更新托盘风格切换提示失败：{error}"))?;

        let app = app.clone();
        let state = self.inner.clone();
        std::thread::spawn(move || {
            std::thread::sleep(FEEDBACK_VISIBLE_DURATION);
            if state.generation.load(Ordering::Acquire) != generation {
                return;
            }
            if let Some(window) = app.get_webview_window("tray-feedback") {
                let _ = window.emit("tray-style-feedback-dismiss", generation);
            }
            std::thread::sleep(FEEDBACK_FADE_DURATION);
            if state.generation.load(Ordering::Acquire) == generation {
                let _ = hide_tray_feedback_window(&app);
            }
        });
        Ok(())
    }
}
