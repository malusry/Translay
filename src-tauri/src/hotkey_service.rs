use std::sync::atomic::{AtomicBool, Ordering};

use tauri::AppHandle;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::config::GLOBAL_HOTKEY;

#[derive(Debug, Default)]
pub struct HotkeyService {
    registered: AtomicBool,
}

impl HotkeyService {
    pub fn register(&self, app: &AppHandle) -> Result<(), String> {
        if self.registered.load(Ordering::Acquire)
            || app.global_shortcut().is_registered(GLOBAL_HOTKEY)
        {
            self.registered.store(true, Ordering::Release);
            return Ok(());
        }
        app.global_shortcut()
            .register(GLOBAL_HOTKEY)
            .map_err(|error| {
                format!(
                    "无法注册全局快捷键 {GLOBAL_HOTKEY}（可能已被其他程序占用）：{error}。\
                 请修改 src-tauri/src/config.rs 中的 GLOBAL_HOTKEY。"
                )
            })?;
        self.registered.store(true, Ordering::Release);
        Ok(())
    }

    pub fn unregister(&self, app: &AppHandle) -> Result<(), String> {
        if !self.registered.swap(false, Ordering::AcqRel) {
            return Ok(());
        }
        if app.global_shortcut().is_registered(GLOBAL_HOTKEY) {
            app.global_shortcut()
                .unregister(GLOBAL_HOTKEY)
                .map_err(|error| format!("注销全局快捷键 {GLOBAL_HOTKEY} 失败：{error}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Default)]
    struct RegistrationState(AtomicBool);

    impl RegistrationState {
        fn register(&self) -> bool {
            !self.0.swap(true, Ordering::AcqRel)
        }
        fn unregister(&self) -> bool {
            self.0.swap(false, Ordering::AcqRel)
        }
    }

    #[test]
    fn repeated_register_and_unregister_are_idempotent() {
        let state = RegistrationState::default();
        assert!(state.register());
        assert!(!state.register());
        assert!(state.unregister());
        assert!(!state.unregister());
    }
}
