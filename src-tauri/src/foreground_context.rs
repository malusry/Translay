use std::path::Path;

use windows::Win32::{
    Foundation::{CloseHandle, HWND},
    System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    },
    UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, IsWindow,
    },
};
use windows::core::PWSTR;

#[derive(Clone, Debug)]
pub struct ForegroundContext {
    pub hwnd: HWND,
    pub process_id: u32,
    pub application_name: String,
}

// HWND is an opaque, process-local identifier. We only pass it between threads
// for Win32 calls during the lifetime of this process.
unsafe impl Send for ForegroundContext {}
unsafe impl Sync for ForegroundContext {}

impl ForegroundContext {
    pub fn capture() -> Result<Self, &'static str> {
        // SAFETY: GetForegroundWindow has no preconditions.
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.0.is_null() {
            return Err("NO_FOREGROUND_WINDOW");
        }

        let mut process_id = 0_u32;
        // SAFETY: hwnd was returned by Windows and process_id is a valid output pointer.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
        if process_id == 0 {
            return Err("FOREGROUND_PROCESS_UNAVAILABLE");
        }

        let window_title = window_text(hwnd);
        let executable = process_image_path(process_id);
        let application_name = executable
            .as_deref()
            .and_then(|value| Path::new(value).file_stem())
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| (!window_title.is_empty()).then(|| window_title.clone()))
            .unwrap_or_else(|| format!("PID {process_id}"));

        Ok(Self {
            hwnd,
            process_id,
            application_name,
        })
    }

    pub fn is_still_foreground(&self) -> bool {
        // SAFETY: GetForegroundWindow has no preconditions.
        unsafe { GetForegroundWindow() == self.hwnd }
    }

    pub fn is_valid_and_foreground(&self) -> bool {
        // SAFETY: IsWindow accepts a possibly stale HWND; GetForegroundWindow has
        // no preconditions.
        unsafe { IsWindow(Some(self.hwnd)).as_bool() && GetForegroundWindow() == self.hwnd }
    }
}

fn window_text(hwnd: HWND) -> String {
    let mut buffer = vec![0_u16; 512];
    // SAFETY: buffer is writable and hwnd is a captured window handle.
    let length = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..length.max(0) as usize])
}

fn process_image_path(process_id: u32) -> Option<String> {
    // SAFETY: process_id was returned by GetWindowThreadProcessId.
    let process =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }.ok()?;
    let mut buffer = vec![0_u16; 32_768];
    let mut length = buffer.len() as u32;
    // SAFETY: process is valid and the buffer/length pair follows the API contract.
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        )
    };
    // SAFETY: process was opened above and is closed exactly once here.
    let _ = unsafe { CloseHandle(process) };
    result
        .ok()
        .map(|_| String::from_utf16_lossy(&buffer[..length as usize]))
}
