use std::{
    mem::size_of,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use windows::{
    Win32::{
        Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND},
        System::{
            Com::IDataObject,
            DataExchange::{
                CloseClipboard, CountClipboardFormats, EmptyClipboard, GetClipboardData,
                GetClipboardSequenceNumber, OpenClipboard, SetClipboardData,
            },
            Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
            Ole::{
                CF_UNICODETEXT, OleFlushClipboard, OleGetClipboard, OleInitialize, OleSetClipboard,
                OleUninitialize,
            },
        },
        UI::Input::KeyboardAndMouse::{
            GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
            SendInput, VIRTUAL_KEY, VK_C, VK_CONTROL, VK_SHIFT,
        },
    },
    core::Error as WindowsError,
};

use crate::{
    capture_session::CancellationToken,
    config::{
        CLIPBOARD_CHANGE_TIMEOUT, CLIPBOARD_MAX_TEXT_CHARS, CLIPBOARD_RETRY_DELAY,
        CLIPBOARD_TIMEOUT,
    },
    foreground_context::ForegroundContext,
};

pub const CLIPBOARD_METHOD: &str = "ClipboardCopy";

#[derive(Debug)]
pub struct ClipboardCapture {
    pub text: String,
    pub restored: bool,
    pub warning_code: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClipboardFailure {
    pub code: String,
    pub message: String,
    pub restored: bool,
    pub warning_code: Option<String>,
}

impl ClipboardFailure {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            restored: false,
            warning_code: None,
        }
    }

    fn windows(stage: &'static str, error: WindowsError) -> Self {
        Self::new(
            format!("{stage}:0x{:08X}", error.code().0 as u32),
            "剪贴板操作失败",
        )
    }
}

pub struct ClipboardService;

impl ClipboardService {
    /// Clipboard capture is isolated in a dedicated OLE STA and bounded by the
    /// caller. It is used after an explicit translation action, either as a
    /// hotkey fallback or to validate the passive selection-button candidate.
    pub fn capture_with_timeout(
        context: ForegroundContext,
        cancellation: Arc<CancellationToken>,
    ) -> Result<ClipboardCapture, ClipboardFailure> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker_cancellation = cancellation.clone();
        let operation_cancelled = Arc::new(AtomicBool::new(false));
        let worker_operation_cancelled = operation_cancelled.clone();
        thread::Builder::new()
            .name("translay-clipboard-capture".into())
            .spawn(move || {
                if worker_cancellation.is_cancelled() {
                    return;
                }
                let result =
                    capture_clipboard(context, &worker_cancellation, &worker_operation_cancelled);
                if !worker_cancellation.is_cancelled() {
                    let _ = sender.send(result);
                }
            })
            .map_err(|_| {
                ClipboardFailure::new("CLIPBOARD_WORKER_START_FAILED", "无法启动剪贴板工作线程")
            })?;

        match receiver.recv_timeout(CLIPBOARD_TIMEOUT) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // Prevent a worker delayed before SendInput from producing a
                // late copy after the caller has already returned.
                operation_cancelled.store(true, Ordering::Release);
                Err(ClipboardFailure::new("CLIPBOARD_TIMEOUT", "复制选区超时"))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(ClipboardFailure::new(
                "CLIPBOARD_WORKER_DISCONNECTED",
                "剪贴板工作线程意外结束",
            )),
        }
    }

    pub fn write_text(text: &str) -> Result<(), String> {
        if text.trim().is_empty() {
            return Err("没有可复制的译文".to_owned());
        }
        if text.chars().count() > CLIPBOARD_MAX_TEXT_CHARS {
            return Err("译文过长，无法复制".to_owned());
        }

        let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let byte_len = units.len() * size_of::<u16>();
        let global = unsafe { GlobalAlloc(GMEM_MOVEABLE, byte_len) }
            .map_err(|error| format!("无法分配剪贴板内存：{error}"))?;
        let pointer = unsafe { GlobalLock(global) }.cast::<u16>();
        if pointer.is_null() {
            let _ = unsafe { GlobalFree(Some(global)) };
            return Err("无法写入剪贴板内存".to_owned());
        }
        unsafe { std::ptr::copy_nonoverlapping(units.as_ptr(), pointer, units.len()) };
        let _ = unsafe { GlobalUnlock(global) };

        let result = with_open_clipboard(HWND::default(), || {
            unsafe { EmptyClipboard() }
                .map_err(|error| ClipboardFailure::windows("EMPTY_CLIPBOARD", error))?;
            unsafe { SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(global.0))) }
                .map_err(|error| ClipboardFailure::windows("SET_CLIPBOARD_DATA", error))?;
            Ok(())
        });
        if result.is_err() {
            let _ = unsafe { GlobalFree(Some(global)) };
        }
        result.map_err(|failure| failure.message)
    }
}

struct OleApartment;

impl OleApartment {
    fn initialize() -> Result<Self, WindowsError> {
        // SAFETY: called once on this dedicated STA thread.
        unsafe { OleInitialize(None)? };
        Ok(Self)
    }
}

impl Drop for OleApartment {
    fn drop(&mut self) {
        // SAFETY: paired with successful OleInitialize on this thread.
        unsafe { OleUninitialize() };
    }
}

enum ClipboardSnapshot {
    Empty,
    Data(IDataObject),
}

fn capture_clipboard(
    context: ForegroundContext,
    cancellation: &CancellationToken,
    operation_cancelled: &AtomicBool,
) -> Result<ClipboardCapture, ClipboardFailure> {
    static CLIPBOARD_CAPTURE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _capture_lock = CLIPBOARD_CAPTURE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if capture_cancelled(cancellation, operation_cancelled) {
        return Err(ClipboardFailure::new(
            "CAPTURE_CANCELLED",
            "捕获请求已被新的请求取代",
        ));
    }
    let _apartment =
        OleApartment::initialize().map_err(|error| ClipboardFailure::windows("OLE_INIT", error))?;
    let (initial_sequence, snapshot) = snapshot_clipboard()?;

    if capture_cancelled(cancellation, operation_cancelled) {
        return Err(ClipboardFailure::new(
            "CAPTURE_CANCELLED",
            "捕获请求已被新的请求取代",
        ));
    }
    if !context.is_valid_and_foreground() {
        return Err(ClipboardFailure::new(
            "FOREGROUND_CHANGED",
            "复制前前台应用已经改变",
        ));
    }

    wait_for_hotkey_release()?;
    if capture_cancelled(cancellation, operation_cancelled) {
        return Err(ClipboardFailure::new(
            "CAPTURE_CANCELLED",
            "捕获请求已被新的请求取代",
        ));
    }
    if !context.is_valid_and_foreground() {
        return Err(ClipboardFailure::new(
            "FOREGROUND_CHANGED",
            "复制前前台应用已经改变",
        ));
    }
    send_ctrl_c(&WindowsInputBackend)?;
    let copied_sequence = wait_for_sequence_change(
        initial_sequence,
        CLIPBOARD_CHANGE_TIMEOUT,
        || unsafe { GetClipboardSequenceNumber() },
        thread::sleep,
    )
    .ok_or_else(|| {
        ClipboardFailure::new("CLIPBOARD_SEQUENCE_UNCHANGED", "目标应用没有响应复制操作")
    })?;

    if capture_cancelled(cancellation, operation_cancelled) || !context.is_valid_and_foreground() {
        let (restored, warning_code) = restore_if_unchanged(&snapshot, copied_sequence);
        return Err(ClipboardFailure {
            code: "FOREGROUND_CHANGED".to_owned(),
            message: "复制期间前台应用已经改变".to_owned(),
            restored,
            warning_code,
        });
    }

    let text_result = read_unicode_text();
    // A third party may change the clipboard while we are opening it.
    let post_read_sequence = unsafe { GetClipboardSequenceNumber() };
    let (restored, warning_code) = restore_if_unchanged(&snapshot, post_read_sequence);

    match text_result {
        Ok(text) => Ok(ClipboardCapture {
            text,
            restored,
            warning_code,
        }),
        Err(mut failure) => {
            failure.restored = restored;
            failure.warning_code = warning_code;
            Err(failure)
        }
    }
}

fn capture_cancelled(cancellation: &CancellationToken, operation_cancelled: &AtomicBool) -> bool {
    cancellation.is_cancelled() || operation_cancelled.load(Ordering::Acquire)
}

fn wait_for_hotkey_release() -> Result<(), ClipboardFailure> {
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(180) {
        // The high bit indicates the key is currently down. Waiting prevents the
        // hotkey's Shift modifier from turning our Ctrl+C into Ctrl+Shift+C.
        let control_down = unsafe { GetAsyncKeyState(VK_CONTROL.0 as i32) } < 0;
        let shift_down = unsafe { GetAsyncKeyState(VK_SHIFT.0 as i32) } < 0;
        if !control_down && !shift_down {
            return Ok(());
        }
        thread::sleep(CLIPBOARD_RETRY_DELAY);
    }
    Err(ClipboardFailure::new(
        "HOTKEY_MODIFIERS_STILL_DOWN",
        "请松开快捷键后重新尝试",
    ))
}

fn snapshot_clipboard() -> Result<(u32, ClipboardSnapshot), ClipboardFailure> {
    for _ in 0..8 {
        let before = unsafe { GetClipboardSequenceNumber() };
        let formats = with_open_clipboard(HWND::default(), || {
            // SAFETY: clipboard is open on this thread.
            Ok(unsafe { CountClipboardFormats() })
        })?;
        let snapshot = if formats == 0 {
            ClipboardSnapshot::Empty
        } else {
            match unsafe { OleGetClipboard() } {
                Ok(data) => ClipboardSnapshot::Data(data),
                Err(_) => {
                    thread::sleep(CLIPBOARD_RETRY_DELAY);
                    continue;
                }
            }
        };
        let after = unsafe { GetClipboardSequenceNumber() };
        if before == after {
            return Ok((after, snapshot));
        }
        thread::sleep(CLIPBOARD_RETRY_DELAY);
    }
    Err(ClipboardFailure::new(
        "CLIPBOARD_SNAPSHOT_UNSTABLE",
        "剪贴板正在被其他程序持续修改",
    ))
}

fn read_unicode_text() -> Result<String, ClipboardFailure> {
    with_open_clipboard(HWND::default(), || {
        let handle = unsafe { GetClipboardData(CF_UNICODETEXT.0 as u32) }.map_err(|_| {
            ClipboardFailure::new(
                "CLIPBOARD_UNICODE_UNAVAILABLE",
                "复制结果不包含 Unicode 文字",
            )
        })?;
        let global = HGLOBAL(handle.0);
        let byte_len = unsafe { GlobalSize(global) };
        if byte_len < 2 || byte_len / 2 > CLIPBOARD_MAX_TEXT_CHARS + 1 {
            return Err(ClipboardFailure::new(
                "CLIPBOARD_TEXT_TOO_LARGE",
                "复制的文字为空或过长",
            ));
        }
        let pointer = unsafe { GlobalLock(global) }.cast::<u16>();
        if pointer.is_null() {
            return Err(ClipboardFailure::new(
                "CLIPBOARD_GLOBAL_LOCK_FAILED",
                "无法读取复制的文字",
            ));
        }
        let units = unsafe { std::slice::from_raw_parts(pointer, byte_len / 2) };
        let length = units
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(units.len());
        let text = String::from_utf16_lossy(&units[..length]);
        // The text has been copied, so GlobalUnlock's ambiguous zero-lock-count
        // return does not alter this result.
        let _ = unsafe { GlobalUnlock(global) };
        let count = text.chars().count();
        if text.trim().is_empty() {
            return Err(ClipboardFailure::new(
                "CLIPBOARD_TEXT_EMPTY",
                "复制结果没有可用文字",
            ));
        }
        if count > CLIPBOARD_MAX_TEXT_CHARS {
            return Err(ClipboardFailure::new(
                "CLIPBOARD_TEXT_TOO_LARGE",
                "复制的文字超过安全长度限制",
            ));
        }
        Ok(text)
    })
}

fn restore_if_unchanged(
    snapshot: &ClipboardSnapshot,
    captured_sequence: u32,
) -> (bool, Option<String>) {
    let current = unsafe { GetClipboardSequenceNumber() };
    if !should_restore(captured_sequence, current) {
        return (false, Some("CLIPBOARD_CHANGED_EXTERNALLY".to_owned()));
    }

    let set_result = retry_ole(|| match snapshot {
        ClipboardSnapshot::Empty => unsafe { OleSetClipboard(None::<&IDataObject>) },
        ClipboardSnapshot::Data(data) => unsafe { OleSetClipboard(data) },
    });
    if set_result.is_err() {
        return (false, Some("CLIPBOARD_RESTORE_FAILED".to_owned()));
    }

    if matches!(snapshot, ClipboardSnapshot::Data(_))
        && retry_ole(|| unsafe { OleFlushClipboard() }).is_err()
    {
        return (false, Some("CLIPBOARD_FLUSH_FAILED".to_owned()));
    }
    (true, None)
}

fn retry_ole(
    mut operation: impl FnMut() -> windows::core::Result<()>,
) -> windows::core::Result<()> {
    let mut result = operation();
    for _ in 0..7 {
        if result.is_ok() {
            break;
        }
        thread::sleep(CLIPBOARD_RETRY_DELAY);
        result = operation();
    }
    result
}

struct ClipboardGuard;

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: this guard exists only after OpenClipboard succeeds.
        let _ = unsafe { CloseClipboard() };
    }
}

fn with_open_clipboard<T>(
    owner: HWND,
    operation: impl FnOnce() -> Result<T, ClipboardFailure>,
) -> Result<T, ClipboardFailure> {
    let mut opened = false;
    for _ in 0..8 {
        if unsafe { OpenClipboard(Some(owner)) }.is_ok() {
            opened = true;
            break;
        }
        thread::sleep(CLIPBOARD_RETRY_DELAY);
    }
    if !opened {
        return Err(ClipboardFailure::new(
            "CLIPBOARD_BUSY",
            "剪贴板正被其他程序占用",
        ));
    }
    let _guard = ClipboardGuard;
    operation()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyStroke {
    Down(VIRTUAL_KEY),
    Up(VIRTUAL_KEY),
}

trait InputBackend {
    fn send(&self, strokes: &[KeyStroke]) -> usize;
}

struct WindowsInputBackend;

impl InputBackend for WindowsInputBackend {
    fn send(&self, strokes: &[KeyStroke]) -> usize {
        let inputs: Vec<INPUT> = strokes.iter().copied().map(key_input).collect();
        unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) as usize }
    }
}

fn key_input(stroke: KeyStroke) -> INPUT {
    let (key, flags) = match stroke {
        KeyStroke::Down(key) => (key, Default::default()),
        KeyStroke::Up(key) => (key, KEYEVENTF_KEYUP),
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                dwFlags: flags,
                ..Default::default()
            },
        },
    }
}

fn send_ctrl_c(backend: &impl InputBackend) -> Result<(), ClipboardFailure> {
    let sequence = [
        KeyStroke::Down(VK_CONTROL),
        KeyStroke::Down(VK_C),
        KeyStroke::Up(VK_C),
        KeyStroke::Up(VK_CONTROL),
    ];
    let sent = backend.send(&sequence);
    if sent == sequence.len() {
        return Ok(());
    }

    // SendInput can be rejected by UIPI or return a partial count.
    let releases = [KeyStroke::Up(VK_C), KeyStroke::Up(VK_CONTROL)];
    let _ = backend.send(&releases);
    Err(ClipboardFailure::new(
        "SEND_INPUT_FAILED",
        "目标应用拒绝了复制按键",
    ))
}

fn wait_for_sequence_change(
    initial: u32,
    timeout: Duration,
    mut sequence: impl FnMut() -> u32,
    mut sleep: impl FnMut(Duration),
) -> Option<u32> {
    let started = Instant::now();
    loop {
        let current = sequence();
        if current != initial {
            return Some(current);
        }
        if started.elapsed() >= timeout {
            return None;
        }
        sleep(CLIPBOARD_RETRY_DELAY);
    }
}

fn should_restore(captured_sequence: u32, current_sequence: u32) -> bool {
    captured_sequence == current_sequence
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct RecordingBackend {
        calls: Mutex<Vec<Vec<KeyStroke>>>,
        first_result: usize,
    }

    impl InputBackend for RecordingBackend {
        fn send(&self, strokes: &[KeyStroke]) -> usize {
            let mut calls = self.calls.lock().unwrap();
            calls.push(strokes.to_vec());
            if calls.len() == 1 {
                self.first_result
            } else {
                strokes.len()
            }
        }
    }

    #[test]
    fn clipboard_sequence_unchanged_times_out() {
        let result = wait_for_sequence_change(
            41,
            Duration::ZERO,
            || 41,
            |_| panic!("zero timeout should not sleep"),
        );
        assert_eq!(result, None);
    }

    #[test]
    fn third_party_clipboard_change_prevents_overwrite() {
        assert!(should_restore(10, 10));
        assert!(!should_restore(10, 11));
    }

    #[test]
    fn ctrl_and_c_are_released_after_send_error() {
        let backend = RecordingBackend {
            calls: Mutex::new(Vec::new()),
            first_result: 1,
        };
        assert!(send_ctrl_c(&backend).is_err());
        let calls = backend.calls.lock().unwrap();
        assert_eq!(calls[1], [KeyStroke::Up(VK_C), KeyStroke::Up(VK_CONTROL)]);
    }
}
