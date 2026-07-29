use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tauri::Manager;
use tracing::warn;

use crate::{capture_coordinator::CaptureCoordinator, hotkey_service::HotkeyService};

#[derive(Clone)]
pub(super) struct ShutdownCoordinator {
    started: Arc<AtomicBool>,
    hotkeys: Arc<HotkeyService>,
    captures: CaptureCoordinator,
}

impl ShutdownCoordinator {
    pub(super) fn new(hotkeys: Arc<HotkeyService>, captures: CaptureCoordinator) -> Self {
        Self {
            started: Arc::new(AtomicBool::new(false)),
            hotkeys,
            captures,
        }
    }

    pub(super) fn request_exit(&self, app: &tauri::AppHandle) {
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }

        self.captures.cancel_current();
        if let Err(error) = self.hotkeys.unregister(app) {
            warn!(
                application = "Translay",
                capture_method = "shutdown",
                text_length = 0,
                elapsed_ms = 0,
                error_code = "HOTKEY_UNREGISTER_FAILED",
                %error,
                "global shortcut unregister failed during explicit exit"
            );
        }
        for window in app.webview_windows().into_values() {
            let _ = window.hide();
        }
        app.exit(0);
    }
}
