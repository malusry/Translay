//! Explicit debug-only experiment. No model, clipboard, tray or production hooks.
use crate::{
    capture_session::CaptureSession, foreground_context::ForegroundContext,
    selection_service::SelectionService,
};
use serde::Serialize;
use std::{
    collections::hash_map::RandomState,
    hash::BuildHasher,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};
use windows::Win32::UI::{
    Accessibility::{
        IUIAutomation, IUIAutomationElement, IUIAutomationLegacyIAccessiblePattern,
        IUIAutomationTreeWalker, IUIAutomationValuePattern, UIA_DocumentControlTypeId,
        UIA_LegacyIAccessiblePatternId, UIA_ValuePatternId,
    },
    Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_F8},
    WindowsAndMessaging::GetWindowTextW,
};

static ENABLED: AtomicBool = AtomicBool::new(false);
static TARGET_PID: AtomicU32 = AtomicU32::new(0);
static DOCUMENT_SIGNALS: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());

// Runtime IDs describe UIA nodes, not article identities. Hash and observe only.
fn runtime_token(element: &IUIAutomationElement) -> Option<String> {
    use windows::Win32::System::Ole::{
        SafeArrayDestroy, SafeArrayGetDim, SafeArrayGetElement, SafeArrayGetElemsize,
        SafeArrayGetLBound, SafeArrayGetUBound,
    };
    let array = unsafe { element.GetRuntimeId() }.ok()?;
    if array.is_null() {
        return None;
    }
    let result = (|| {
        if unsafe { SafeArrayGetDim(array) } != 1 || unsafe { SafeArrayGetElemsize(array) } != 4 {
            return None;
        }
        let lower = unsafe { SafeArrayGetLBound(array, 1) }.ok()?;
        let upper = unsafe { SafeArrayGetUBound(array, 1) }.ok()?;
        let length = upper.checked_sub(lower)?.checked_add(1)?;
        if !(1..=64).contains(&length) {
            return None;
        }
        let mut values = Vec::new();
        for index in lower..=upper {
            let mut value = 0i32;
            unsafe { SafeArrayGetElement(array, &index, (&mut value as *mut i32).cast()) }.ok()?;
            values.push(value);
        }
        Some(token(&format!("{values:?}")))
    })();
    let _ = unsafe { SafeArrayDestroy(array) };
    result
}

fn focused_within(automation: &IUIAutomation, document: &IUIAutomationElement) -> Option<bool> {
    let mut focused = unsafe { automation.GetFocusedElement() }.ok()?;
    let walker = unsafe { automation.RawViewWalker() }.ok()?;
    for _ in 0..32 {
        if unsafe { automation.CompareElements(&focused, document) }
            .ok()?
            .as_bool()
        {
            return Some(true);
        }
        match unsafe { walker.GetParentElement(&focused) } {
            Ok(parent) => focused = parent,
            Err(_) => return Some(false),
        }
    }
    None
}
pub(crate) fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

// Only the explicit debug runner can opt one process into background sampling.
// This is never enabled for normal app startup, nor compiled into release.
pub(crate) fn allows_background(context: &ForegroundContext) -> bool {
    let target = TARGET_PID.load(Ordering::Relaxed);
    let mut actual = 0;
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(
            context.hwnd,
            Some(&mut actual),
        );
    }
    enabled() && target != 0 && context.process_id == target && actual == target
}

pub(crate) fn document_candidates(
    automation: &IUIAutomation,
    root: &IUIAutomationElement,
    is_foreground: bool,
) -> Vec<IUIAutomationElement> {
    use windows::Win32::UI::Accessibility::TreeScope_Descendants;
    let Ok(condition) = (unsafe { automation.CreateTrueCondition() }) else {
        return vec![];
    };
    let Ok(elements) = (unsafe { root.FindAll(TreeScope_Descendants, &condition) }) else {
        return vec![];
    };
    let count = unsafe { elements.Length() }.unwrap_or(0).min(1024);
    let documents: Vec<_> = (0..count)
        .filter_map(|i| unsafe { elements.GetElement(i) }.ok())
        .filter(|e| unsafe { e.CurrentControlType() }.ok() == Some(UIA_DocumentControlTypeId))
        .filter(|e| {
            !unsafe { e.CurrentIsOffscreen() }
                .map(|b| b.as_bool())
                .unwrap_or(true)
        })
        .take(8)
        .collect();
    let focus: Vec<_> = documents
        .iter()
        .map(|document| focused_within(automation, document))
        .collect();
    let focused_indices: Vec<_> = focus
        .iter()
        .enumerate()
        .filter_map(|(i, v)| (*v == Some(true)).then_some(i))
        .collect();
    let chosen = if is_foreground && focus.iter().all(Option::is_some) && focused_indices.len() == 1
    {
        Some(focused_indices[0])
    } else {
        None
    };
    let signals = documents.iter().enumerate().map(|(index, document)| serde_json::json!({
        "runtimeToken":runtime_token(document),
        "hasKeyboardFocus":unsafe { document.CurrentHasKeyboardFocus() }.ok().map(|v| v.as_bool()),
        "containsGlobalFocus":focus[index],
        "chosenByGlobalFocus":chosen == Some(index)
    })).collect();
    if let Ok(mut stored) = DOCUMENT_SIGNALS.lock() {
        *stored = signals;
    }
    // Only an actual foreground window may use global focus to narrow candidates.
    // Uncertain/background cases retain the existing ambiguity rejection.
    match chosen {
        Some(index) => vec![documents[index].clone()],
        None => documents,
    }
}

pub(crate) fn ambiguous_selection(documents: &[IUIAutomationElement]) -> bool {
    use windows::Win32::UI::Accessibility::{IUIAutomationTextPattern, UIA_TextPatternId};
    let mut selected_documents = 0;
    for document in documents {
        let Ok(pattern) = (unsafe {
            document.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        }) else {
            continue;
        };
        let Ok(ranges) = (unsafe { pattern.GetSelection() }) else {
            return true;
        };
        let Ok(length) = (unsafe { ranges.Length() }) else {
            return true;
        };
        if length > 16 {
            return true;
        }
        for index in 0..length {
            let Ok(range) = (unsafe { ranges.GetElement(index) }) else {
                return true;
            };
            let Ok(text) = (unsafe { range.GetText(1) }) else {
                return true;
            };
            if !text.is_empty() {
                selected_documents += 1;
                break;
            }
        }
    }
    selected_documents > 1
}

fn targeted_context(pid: u32) -> Option<ForegroundContext> {
    use windows::{
        Win32::{
            Foundation::{CloseHandle, HWND, LPARAM, RECT},
            System::Threading::{
                OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
                QueryFullProcessImageNameW,
            },
            UI::WindowsAndMessaging::{
                EnumWindows, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible,
            },
        },
        core::{BOOL, PWSTR},
    };
    struct Target {
        pid: u32,
        hwnd: HWND,
        area: i64,
    }
    unsafe extern "system" fn visit(hwnd: HWND, param: LPARAM) -> BOOL {
        let target = unsafe { &mut *(param.0 as *mut Target) };
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
        if pid == target.pid && unsafe { IsWindowVisible(hwnd).as_bool() } {
            let mut rect = RECT::default();
            if unsafe { GetWindowRect(hwnd, &mut rect) }.is_ok() {
                let area = i64::from((rect.right - rect.left).max(0))
                    * i64::from((rect.bottom - rect.top).max(0));
                if area > target.area {
                    target.hwnd = hwnd;
                    target.area = area;
                }
            }
        }
        BOOL(1)
    }
    // Verify the supplied process really is a supported browser before reading UIA.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut path = vec![0u16; 32768];
    let mut length = path.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut length,
        )
    };
    let _ = unsafe { CloseHandle(process) };
    result.ok()?;
    let image = String::from_utf16_lossy(&path[..length as usize]);
    let name = std::path::Path::new(&image)
        .file_stem()?
        .to_str()?
        .to_ascii_lowercase();
    if !matches!(name.as_str(), "msedge" | "chrome") {
        return None;
    }
    let mut target = Target {
        pid,
        hwnd: HWND::default(),
        area: 0,
    };
    unsafe { EnumWindows(Some(visit), LPARAM(&mut target as *mut Target as isize)) }.ok()?;
    if target.hwnd.0.is_null() {
        return None;
    }
    Some(ForegroundContext {
        hwnd: target.hwnd,
        process_id: pid,
        application_name: name,
    })
}

fn token(value: &str) -> String {
    static HASH: OnceLock<[RandomState; 2]> = OnceLock::new();
    let h = HASH.get_or_init(|| [RandomState::new(), RandomState::new()]);
    format!("{:016x}{:016x}", h[0].hash_one(value), h[1].hash_one(value))
}

fn document_url(value: &str) -> Option<(String, &'static str)> {
    let url = reqwest::Url::parse(value.trim()).ok()?;
    let kind = match url.scheme() {
        "http" | "https" if url.host_str().is_some() => "web",
        "file" if !url.path().is_empty() => "file",
        _ => return None,
    };
    // Preserve query and fragment: they may identify a different document/route.
    Some((url.to_string(), kind))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Evidence {
    document_found: bool,
    runtime_token: Option<String>,
    url_token: Option<String>,
    title_token: Option<String>,
    url_kind: Option<String>,
    source: Option<String>,
    reason: &'static str,
    terminology_enabled: bool,
}

impl Evidence {
    fn unknown(reason: &'static str) -> Self {
        Self {
            document_found: false,
            runtime_token: None,
            url_token: None,
            title_token: None,
            url_kind: None,
            source: None,
            reason,
            terminology_enabled: false,
        }
    }
}

pub(crate) fn inspect(
    automation: &IUIAutomation,
    walker: &IUIAutomationTreeWalker,
    selected: &IUIAutomationElement,
    root: &IUIAutomationElement,
) -> Evidence {
    let start = Instant::now();
    let mut element = selected.clone();
    for _ in 0..24 {
        if start.elapsed() > Duration::from_millis(150) {
            return Evidence::unknown("identity_budget_exceeded");
        }
        if unsafe { element.CurrentIsPassword() }
            .map(|v| v.as_bool())
            .unwrap_or(true)
        {
            return Evidence::unknown("protected_or_unavailable_element");
        }
        if unsafe { element.CurrentControlType() }.ok() == Some(UIA_DocumentControlTypeId) {
            let title = unsafe { element.CurrentName() }.ok().map(|s| s.to_string());
            let direct = unsafe {
                element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            }
            .ok()
            .and_then(|p| unsafe { p.CurrentValue() }.ok())
            .and_then(|s| document_url(&s.to_string()));
            let legacy = unsafe {
                element.GetCurrentPatternAs::<IUIAutomationLegacyIAccessiblePattern>(
                    UIA_LegacyIAccessiblePatternId,
                )
            }
            .ok()
            .and_then(|p| unsafe { p.CurrentValue() }.ok())
            .and_then(|s| document_url(&s.to_string()));
            let ambiguous = matches!((&direct, &legacy), (Some(a), Some(b)) if a.0 != b.0);
            let source = if direct.is_some() {
                "document_value"
            } else {
                "document_legacy_value"
            };
            let identity = if ambiguous { None } else { direct.or(legacy) };
            return Evidence {
                document_found: true,
                runtime_token: runtime_token(&element),
                title_token: title.filter(|t| !t.is_empty()).map(|t| token(&t)),
                url_token: identity.as_ref().map(|(url, _)| token(url)),
                url_kind: identity.as_ref().map(|(_, kind)| (*kind).into()),
                source: identity.as_ref().map(|_| source.into()),
                reason: if ambiguous {
                    "conflicting_document_values"
                } else if identity.is_some() {
                    "candidate_requires_validation"
                } else {
                    "document_has_no_supported_url"
                },
                terminology_enabled: false,
            };
        }
        if unsafe { automation.CompareElements(&element, root) }
            .map(|b| b.as_bool())
            .unwrap_or(true)
        {
            break;
        }
        let Ok(parent) = (unsafe { walker.GetParentElement(&element) }) else {
            break;
        };
        element = parent;
    }
    Evidence::unknown("no_document_ancestor")
}

fn window_title(context: &ForegroundContext) -> String {
    let mut buffer = [0u16; 512];
    let n = unsafe { GetWindowTextW(context.hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..n.max(0) as usize])
}

fn sample(session: &CaptureSession, expectation: Option<&serde_json::Value>) -> serde_json::Value {
    let target = TARGET_PID.load(Ordering::Relaxed);
    let context = if target != 0 {
        targeted_context(target)
    } else {
        ForegroundContext::capture().ok()
    };
    let Some(context) = context else {
        return serde_json::json!({"status":"target_unavailable"});
    };
    let browser = context.application_name.to_ascii_lowercase();
    if !matches!(browser.as_str(), "msedge" | "chrome") {
        return serde_json::json!({"status":"unsupported_foreground"});
    }
    // Explicit test request only; Windows may decline foreground activation.
    // Never attach input threads or bypass the OS foreground lock.
    if target != 0 && expectation.is_some_and(|e| e["activateWindow"] == true) {
        let _ =
            unsafe { windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(context.hwnd) };
        std::thread::sleep(Duration::from_millis(80));
    }
    let title_before = window_title(&context);
    if let Ok(mut signals) = DOCUMENT_SIGNALS.lock() {
        signals.clear();
    }
    let request = session.begin();
    let capture = SelectionService::capture_with_timeout(
        context.clone(),
        request.cancellation.clone(),
        Duration::from_secs(2),
        None,
        false,
    );
    request.cancellation.cancel();
    let document_signals = DOCUMENT_SIGNALS
        .lock()
        .map(|mut s| std::mem::take(&mut *s))
        .unwrap_or_default();
    let is_foreground = context.is_valid_and_foreground();
    if !(context.is_valid_and_foreground() || allows_background(&context))
        || title_before != window_title(&context)
    {
        return serde_json::json!({"status":"foreground_or_title_changed","browser":browser});
    }
    let mut result = match capture {
        Ok(capture) => {
            // Expected values are test-only checks AFTER capture, never used to
            // choose an element. Only booleans are written to the report.
            let text_matches = expectation
                .and_then(|e| e["expectedText"].as_str())
                .map(|expected| capture.text == expected);
            let url_matches = expectation
                .and_then(|e| e["expectedUrl"].as_str())
                .and_then(document_url)
                .map(|(url, _)| {
                    capture
                        .document_evidence
                        .as_ref()
                        .and_then(|e| e.url_token.as_ref())
                        .is_some_and(|actual| *actual == token(&url))
                });
            serde_json::json!({"status":"sampled","browser":browser,"captureMode":if target == 0 {"foreground"} else {"explicit_browser_process"},"windowToken":token(&format!("{}:{:?}", context.process_id, context.hwnd)),"evidence":capture.document_evidence,"selectionMatchesExpected":text_matches,"documentMatchesExpected":url_matches})
        }
        Err(error) => {
            serde_json::json!({"status":"selection_unavailable","browser":browser,"code":error.code.split(':').next().unwrap_or("UIA_ERROR")})
        }
    };
    result["isForeground"] = serde_json::json!(is_foreground);
    result["documentSignals"] = serde_json::json!(document_signals);
    result
}

pub(crate) fn run() {
    ENABLED.store(true, Ordering::Relaxed);
    if let Some(arg) =
        std::env::args().find_map(|a| a.strip_prefix("--probe-pid=").map(str::to_owned))
    {
        let Ok(pid) = arg.parse::<u32>() else {
            std::process::exit(2);
        };
        if pid == 0 {
            std::process::exit(2);
        }
        TARGET_PID.store(pid, Ordering::Relaxed);
    }
    let once = std::env::args().any(|s| s == "--document-probe-once");
    let piped = std::env::args().any(|s| s == "--probe-pipe");
    let (sender, receiver) = std::sync::mpsc::channel();
    if piped {
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
            let _ = sender.send("stop".into());
        });
    }
    let session = CaptureSession::default();
    let mut records = Vec::new();
    eprintln!(
        "Translay document identity probe: select text in Edge/Chrome, press F8 to sample; Esc exits. Max 24 samples / 3 minutes. No model calls."
    );
    let started = Instant::now();
    let mut was_down = true;
    while records.len() < 24 && started.elapsed() < Duration::from_secs(180) {
        let down = unsafe { GetAsyncKeyState(VK_F8.0 as i32) as u16 & 0x8000 != 0 };
        let command = receiver.try_recv().ok();
        if command.as_deref() == Some("stop") {
            break;
        }
        let expectation = command
            .as_deref()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
        if expectation.as_ref().is_some_and(|e| e["sample"] == true)
            || once
            || command.as_deref() == Some("sample")
            || (!piped && down && !was_down)
        {
            let record = sample(&session, expectation.as_ref());
            println!("{record}");
            let timed_out = record["code"] == "UIA_TIMEOUT";
            records.push(record);
            if once || timed_out {
                break;
            }
        }
        was_down = down;
        if unsafe { GetAsyncKeyState(VK_ESCAPE.0 as i32) as u16 & 0x8000 != 0 } {
            break;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    // Automated runner writes the consolidated report for both browsers.
    if piped {
        return;
    }
    let path = std::path::Path::new("tests/artifacts/document-probe.json");
    let report = serde_json::json!({"kind":"Translay debug document identity probe","records":records,"scope":"candidate evidence only; no reliable document identity established; no terminology memory"});
    if let Err(error) = std::fs::create_dir_all(path.parent().unwrap())
        .and_then(|_| std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()))
    {
        eprintln!("Could not save probe report: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_document_urls_are_candidates() {
        for value in [
            "An article",
            "about:blank",
            "chrome-extension://id/viewer",
            "blob:https://example.org/id",
            "javascript:alert(1)",
        ] {
            assert!(document_url(value).is_none());
        }
        assert_eq!(
            document_url("https://example.org/paper?id=1#section")
                .unwrap()
                .1,
            "web"
        );
        assert_eq!(document_url("file:///C:/papers/a.pdf").unwrap().1, "file");
    }
    #[test]
    fn routes_and_same_title_different_urls_remain_distinct() {
        let a = document_url("https://example.org/paper?a=1#one").unwrap().0;
        let b = document_url("https://example.org/paper?a=2#one").unwrap().0;
        let c = document_url("https://example.org/paper?a=1#two").unwrap().0;
        assert_ne!(token(&a), token(&b));
        assert_ne!(token(&a), token(&c));
        assert_eq!(token(&a), token(&a));
        let report = serde_json::to_string(&Evidence::unknown("test")).unwrap();
        assert!(!report.contains("https://"));
        assert!(!Evidence::unknown("test").terminology_enabled);
    }
}
