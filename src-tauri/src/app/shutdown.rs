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

        app.state::<super::windows::SettingsWindow>().stop();
        app.state::<super::tray_menu::TrayMenu>().stop();
        super::tray::stop_clicks(app);

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
        // Final hiding shares the presentation thread. An already-running
        // presentation finishes before this callback, never after exit cleanup.
        let handle = app.clone();
        if let Err(error) = app.run_on_main_thread(move || {
            for window in handle.webview_windows().into_values() {
                if let Err(error) = window.hide() {
                    warn!(%error, "window hide failed during exit");
                }
            }
            handle.exit(0);
        }) {
            warn!(%error, "exit window cleanup dispatch failed");
            app.exit(0);
        }
    }

    pub(super) fn is_exiting(&self) -> bool {
        self.started.load(Ordering::Acquire)
    }
}
