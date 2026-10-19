use serde::Serialize;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessibilityStatus {
    pub supported: bool,
    pub trusted: Option<bool>,
}

pub fn accessibility_status() -> AccessibilityStatus {
    #[cfg(target_os = "macos")]
    {
        AccessibilityStatus {
            supported: true,
            // SAFETY: this system query has no arguments or ownership transfer.
            trusted: Some(unsafe { macos::AXIsProcessTrusted() } != 0),
        }
    }
    #[cfg(not(target_os = "macos"))]
    AccessibilityStatus {
        supported: false,
        trusted: None,
    }
}

pub fn request_accessibility() -> Result<AccessibilityStatus, String> {
    #[cfg(target_os = "macos")]
    {
        // Both dictionary entries are exported, process-lifetime CF objects.
        // Null callbacks keep this short-lived dictionary from owning them.
        unsafe {
            let keys = [macos::kAXTrustedCheckOptionPrompt];
            let values = [macos::kCFBooleanTrue];
            let options = macos::CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                std::ptr::null(),
                std::ptr::null(),
            );
            if options.is_null() {
                return Err("无法创建辅助功能授权请求".into());
            }
            macos::AXIsProcessTrustedWithOptions(options);
            macos::CFRelease(options);
        }
        // The system prompt is asynchronous. Recheck when the window regains focus.
        Ok(accessibility_status())
    }
    #[cfg(not(target_os = "macos"))]
    Err("辅助功能授权需要在 macOS 原生程序中操作".into())
}

pub fn open_system_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("/usr/bin/open")
            .args(["-b", "com.apple.systempreferences"])
            .status()
            .map_err(|error| format!("无法打开系统设置：{error}"))?;
        if !status.success() {
            return Err("系统设置未能打开，请从 Apple 菜单手动打开".into());
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    Err("系统设置需要在 macOS 原生程序中打开".into())
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        pub fn AXIsProcessTrusted() -> u8;
        pub fn AXIsProcessTrustedWithOptions(options: *const c_void) -> u8;
        pub static kAXTrustedCheckOptionPrompt: *const c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        pub static kCFBooleanTrue: *const c_void;
        pub fn CFDictionaryCreate(
            allocator: *const c_void,
            keys: *const *const c_void,
            values: *const *const c_void,
            count: isize,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> *const c_void;
        pub fn CFRelease(object: *const c_void);
    }
}
