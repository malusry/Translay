use std::{
    collections::VecDeque,
    ffi::c_void,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use windows::{
    Win32::{
        Foundation::POINT,
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize, SAFEARRAY,
            },
            Ole::{
                SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
                SafeArrayUnaccessData,
            },
        },
        UI::{
            Accessibility::{
                CUIAutomation8, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
                IUIAutomationTextPattern2, IUIAutomationTextRangeArray, UIA_DocumentControlTypeId,
                UIA_TextPattern2Id, UIA_TextPatternId,
            },
            WindowsAndMessaging::GetCursorPos,
        },
    },
    core::Error as WindowsError,
};

use crate::{
    capture_session::CancellationToken,
    config::{UIA_MAX_ANCESTORS, UIA_MAX_CANDIDATES, UIA_MAX_SUBTREE_ELEMENTS, UIA_SEARCH_BUDGET},
    foreground_context::ForegroundContext,
    models::ScreenRect,
};

#[derive(Debug)]
pub struct SelectionCapture {
    pub text: String,
    pub rect: Option<ScreenRect>,
    pub method: &'static str,
}

#[derive(Debug, Clone)]
pub struct SelectionFailure {
    pub code: String,
    pub message: String,
    pub method: &'static str,
    pub recoverable: bool,
}

impl SelectionFailure {
    fn static_error(code: &'static str, message: &'static str, recoverable: bool) -> Self {
        Self {
            code: code.to_owned(),
            message: message.to_owned(),
            method: "UI Automation",
            recoverable,
        }
    }

    fn windows(stage: &'static str, error: WindowsError) -> Self {
        Self {
            code: format!("{stage}:0x{:08X}", error.code().0 as u32),
            message: format!("UI Automation 在 {stage} 阶段失败"),
            method: "UI Automation",
            recoverable: true,
        }
    }
}

pub struct SelectionService;

impl SelectionService {
    /// Each provider call runs on an isolated MTA worker. A slow or hung
    /// accessibility provider can strand that worker, but it cannot block the
    /// Tauri event loop or commit a stale capture.
    pub fn capture_with_timeout(
        context: ForegroundContext,
        cancellation: std::sync::Arc<CancellationToken>,
        timeout: Duration,
    ) -> Result<SelectionCapture, SelectionFailure> {
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("translay-uia-capture".into())
            .spawn(move || {
                if cancellation.is_cancelled() {
                    return;
                }
                let result = capture_selection(context, &cancellation);
                if !cancellation.is_cancelled() {
                    let _ = sender.send(result);
                }
            })
            .map_err(|_| {
                SelectionFailure::static_error(
                    "WORKER_START_FAILED",
                    "无法启动 UI Automation 工作线程",
                    true,
                )
            })?;

        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(SelectionFailure::static_error(
                "UIA_TIMEOUT",
                "目标应用未及时响应 UI Automation",
                true,
            )),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(SelectionFailure::static_error(
                "UIA_WORKER_DISCONNECTED",
                "UI Automation 工作线程意外结束",
                true,
            )),
        }
    }
}

struct ComApartment;

impl ComApartment {
    fn initialize() -> Result<Self, WindowsError> {
        // SAFETY: called once on this dedicated worker thread.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY: paired with the successful CoInitializeEx above.
        unsafe { CoUninitialize() };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CandidateOrigin {
    Focused,
    Pointer,
    RawAncestor,
    ContentAncestor,
    ControlAncestor,
    WindowRoot,
    RootSubtree,
}

struct Candidate {
    element: IUIAutomationElement,
    #[allow(dead_code)]
    origin: CandidateOrigin,
}

fn capture_selection(
    context: ForegroundContext,
    cancellation: &CancellationToken,
) -> Result<SelectionCapture, SelectionFailure> {
    let _apartment =
        ComApartment::initialize().map_err(|error| SelectionFailure::windows("COM_INIT", error))?;
    // SAFETY: COM is initialized and CUIAutomation8 is an in-proc COM class.
    let automation: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) }
            .map_err(|error| SelectionFailure::windows("CREATE_AUTOMATION", error))?;
    let root = unsafe { automation.ElementFromHandle(context.hwnd) }
        .map_err(|error| SelectionFailure::windows("WINDOW_ROOT", error))?;
    let raw_walker = unsafe { automation.RawViewWalker() }
        .map_err(|error| SelectionFailure::windows("RAW_WALKER", error))?;

    let search_started = Instant::now();
    let mut candidates = Vec::new();

    let focused_seed = unsafe { automation.GetFocusedElement() }
        .ok()
        .filter(|element| belongs_to_root(&automation, &raw_walker, element, &root));
    if focused_seed
        .as_ref()
        .is_some_and(|element| is_password(element))
    {
        return Err(SelectionFailure::static_error(
            "PROTECTED_INPUT",
            "出于安全原因，不读取密码框或受保护的输入控件",
            false,
        ));
    }
    if let Some(focused) = focused_seed.as_ref() {
        push_unique(
            &automation,
            &mut candidates,
            focused.clone(),
            CandidateOrigin::Focused,
        );
    }

    let mut pointer = POINT::default();
    let pointer_seed = if unsafe { GetCursorPos(&mut pointer) }.is_ok() {
        unsafe { automation.ElementFromPoint(pointer) }
            .ok()
            .filter(|element| belongs_to_root(&automation, &raw_walker, element, &root))
    } else {
        None
    };
    if pointer_seed
        .as_ref()
        .is_some_and(|element| is_password(element))
    {
        return Err(SelectionFailure::static_error(
            "PROTECTED_INPUT",
            "出于安全原因，不读取密码框或受保护的输入控件",
            false,
        ));
    }
    if let Some(pointed) = pointer_seed.as_ref() {
        push_unique(
            &automation,
            &mut candidates,
            pointed.clone(),
            CandidateOrigin::Pointer,
        );
    }

    if let Some(focused) = focused_seed {
        collect_ancestors(
            &automation,
            &raw_walker,
            &mut candidates,
            focused,
            CandidateOrigin::RawAncestor,
            UIA_MAX_ANCESTORS,
        );
    }
    if let Some(pointed) = pointer_seed {
        collect_ancestors(
            &automation,
            &raw_walker,
            &mut candidates,
            pointed,
            CandidateOrigin::RawAncestor,
            UIA_MAX_ANCESTORS,
        );
    }

    if let Ok(walker) = unsafe { automation.ContentViewWalker() } {
        collect_seed_ancestors(
            &automation,
            &walker,
            &mut candidates,
            CandidateOrigin::ContentAncestor,
        );
    }
    if let Ok(walker) = unsafe { automation.ControlViewWalker() } {
        collect_seed_ancestors(
            &automation,
            &walker,
            &mut candidates,
            CandidateOrigin::ControlAncestor,
        );
    }
    push_unique(
        &automation,
        &mut candidates,
        root.clone(),
        CandidateOrigin::WindowRoot,
    );
    collect_root_subtree(
        &automation,
        &raw_walker,
        &root,
        &mut candidates,
        search_started,
    );

    let mut saw_text_pattern = false;
    let mut saw_empty_selection = false;
    let mut last_provider_error: Option<SelectionFailure> = None;

    for candidate in candidates.into_iter().take(UIA_MAX_CANDIDATES) {
        if cancellation.is_cancelled() {
            return Err(SelectionFailure::static_error(
                "CAPTURE_CANCELLED",
                "捕获请求已被新的请求取代",
                false,
            ));
        }
        if !context.is_valid_and_foreground() {
            return Err(SelectionFailure::static_error(
                "FOREGROUND_CHANGED",
                "选区捕获期间前台应用已改变",
                false,
            ));
        }
        if is_password(&candidate.element) {
            continue;
        }

        // Chromium providers may live in a renderer process, so a candidate is
        // accepted by UIA subtree membership rather than PID equality.
        if let Ok(pattern) = unsafe {
            candidate
                .element
                .GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPattern2Id)
        } {
            saw_text_pattern = true;
            match read_pattern_selection(&pattern, "UIA:TextPattern2") {
                Ok(capture) => return Ok(capture),
                Err(PatternReadError::Empty) => saw_empty_selection = true,
                Err(PatternReadError::Windows(stage, error)) => {
                    last_provider_error = Some(SelectionFailure::windows(stage, error));
                }
            }
        }

        // A provider can expose TextPattern2 but fail an individual call. Always
        // retry through its TextPattern implementation before moving on.
        if let Ok(pattern) = unsafe {
            candidate
                .element
                .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
        } {
            saw_text_pattern = true;
            match read_pattern_selection(&pattern, "UIA:TextPattern") {
                Ok(capture) => return Ok(capture),
                Err(PatternReadError::Empty) => saw_empty_selection = true,
                Err(PatternReadError::Windows(stage, error)) => {
                    last_provider_error = Some(SelectionFailure::windows(stage, error));
                }
            }
        }
    }

    if let Some(error) = last_provider_error {
        Err(error)
    } else if saw_empty_selection {
        Err(SelectionFailure::static_error(
            "NO_TEXT_SELECTED",
            "没有读取到当前选中的文字",
            true,
        ))
    } else if saw_text_pattern {
        Err(SelectionFailure::static_error(
            "NO_TEXT_SELECTED",
            "文本控件没有返回有效选区",
            true,
        ))
    } else {
        Err(SelectionFailure::static_error(
            "TEXT_PATTERN_UNSUPPORTED",
            "当前控件没有提供可读取的文字选区",
            true,
        ))
    }
}

fn is_password(element: &IUIAutomationElement) -> bool {
    unsafe { element.CurrentIsPassword() }
        .map(|value| value.as_bool())
        .unwrap_or(false)
}

fn collect_seed_ancestors(
    automation: &IUIAutomation,
    walker: &windows::Win32::UI::Accessibility::IUIAutomationTreeWalker,
    candidates: &mut Vec<Candidate>,
    origin: CandidateOrigin,
) {
    let seeds: Vec<IUIAutomationElement> = candidates
        .iter()
        .filter(|candidate| {
            matches!(
                candidate.origin,
                CandidateOrigin::Focused | CandidateOrigin::Pointer
            )
        })
        .map(|candidate| candidate.element.clone())
        .collect();
    for seed in seeds {
        collect_ancestors(
            automation,
            walker,
            candidates,
            seed,
            origin,
            UIA_MAX_ANCESTORS,
        );
    }
}

fn collect_ancestors(
    automation: &IUIAutomation,
    walker: &windows::Win32::UI::Accessibility::IUIAutomationTreeWalker,
    candidates: &mut Vec<Candidate>,
    mut element: IUIAutomationElement,
    origin: CandidateOrigin,
    limit: usize,
) {
    for _ in 0..limit {
        let Ok(parent) = (unsafe { walker.GetParentElement(&element) }) else {
            break;
        };
        push_unique(automation, candidates, parent.clone(), origin);
        element = parent;
        if candidates.len() >= UIA_MAX_CANDIDATES {
            break;
        }
    }
}

fn collect_root_subtree(
    automation: &IUIAutomation,
    walker: &windows::Win32::UI::Accessibility::IUIAutomationTreeWalker,
    root: &IUIAutomationElement,
    candidates: &mut Vec<Candidate>,
    started: Instant,
) {
    let mut queue = VecDeque::from([root.clone()]);
    let mut visited = 0_usize;
    while let Some(parent) = queue.pop_front() {
        if visited >= UIA_MAX_SUBTREE_ELEMENTS
            || candidates.len() >= UIA_MAX_CANDIDATES
            || started.elapsed() >= UIA_SEARCH_BUDGET
        {
            break;
        }
        let Ok(mut child) = (unsafe { walker.GetFirstChildElement(&parent) }) else {
            continue;
        };
        loop {
            visited += 1;
            let is_document = unsafe { child.CurrentControlType() }
                .map(|value| value == UIA_DocumentControlTypeId)
                .unwrap_or(false);
            let has_pattern = unsafe {
                child
                    .GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPattern2Id)
                    .is_ok()
                    || child
                        .GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
                        .is_ok()
            };
            if is_document || has_pattern {
                push_unique(
                    automation,
                    candidates,
                    child.clone(),
                    CandidateOrigin::RootSubtree,
                );
            }
            queue.push_back(child.clone());
            if visited >= UIA_MAX_SUBTREE_ELEMENTS
                || candidates.len() >= UIA_MAX_CANDIDATES
                || started.elapsed() >= UIA_SEARCH_BUDGET
            {
                break;
            }
            match unsafe { walker.GetNextSiblingElement(&child) } {
                Ok(sibling) => child = sibling,
                Err(_) => break,
            }
        }
    }
}

fn belongs_to_root(
    automation: &IUIAutomation,
    walker: &windows::Win32::UI::Accessibility::IUIAutomationTreeWalker,
    element: &IUIAutomationElement,
    root: &IUIAutomationElement,
) -> bool {
    let mut current = element.clone();
    for _ in 0..64 {
        if unsafe { automation.CompareElements(&current, root) }
            .map(|same| same.as_bool())
            .unwrap_or(false)
        {
            return true;
        }
        match unsafe { walker.GetParentElement(&current) } {
            Ok(parent) => current = parent,
            Err(_) => return false,
        }
    }
    false
}

fn push_unique(
    automation: &IUIAutomation,
    candidates: &mut Vec<Candidate>,
    element: IUIAutomationElement,
    origin: CandidateOrigin,
) {
    if candidates.len() >= UIA_MAX_CANDIDATES {
        return;
    }
    let duplicate = candidates.iter().any(|candidate| {
        unsafe { automation.CompareElements(&candidate.element, &element) }
            .map(|same| same.as_bool())
            .unwrap_or(false)
    });
    if !duplicate {
        candidates.push(Candidate { element, origin });
    }
}

enum PatternReadError {
    Empty,
    Windows(&'static str, WindowsError),
}

fn read_pattern_selection(
    pattern: &IUIAutomationTextPattern,
    method: &'static str,
) -> Result<SelectionCapture, PatternReadError> {
    let ranges = unsafe { pattern.GetSelection() }
        .map_err(|error| PatternReadError::Windows("GET_SELECTION", error))?;
    read_ranges(ranges, method)
}

fn read_ranges(
    ranges: IUIAutomationTextRangeArray,
    method: &'static str,
) -> Result<SelectionCapture, PatternReadError> {
    let length = unsafe { ranges.Length() }
        .map_err(|error| PatternReadError::Windows("SELECTION_LENGTH", error))?;
    if length <= 0 {
        return Err(PatternReadError::Empty);
    }

    let mut text_parts = Vec::new();
    let mut union_rect: Option<ScreenRect> = None;
    for index in 0..length {
        let range = unsafe { ranges.GetElement(index) }
            .map_err(|error| PatternReadError::Windows("SELECTION_RANGE", error))?;
        let text = unsafe { range.GetText(-1) }
            .map_err(|error| PatternReadError::Windows("GET_TEXT", error))?;
        let text = String::from_utf16_lossy(&text);
        if !text.is_empty() {
            text_parts.push(text);
        }

        // Bounding rectangles are optional. Text remains a valid capture when a
        // provider returns no geometry; the overlay will anchor to the cursor.
        if let Ok(safe_array) = unsafe { range.GetBoundingRectangles() } {
            if let Ok(rectangles) = unsafe { safe_array_rectangles(safe_array) } {
                for rect in rectangles {
                    union_rect = Some(match union_rect {
                        Some(current) => current.union(rect),
                        None => rect,
                    });
                }
            }
        }
    }

    let text = text_parts.join("\n");
    if text.trim().is_empty() {
        return Err(PatternReadError::Empty);
    }
    Ok(SelectionCapture {
        text,
        rect: union_rect.filter(|rect| rect.width() > 0 && rect.height() > 0),
        method,
    })
}

/// UIA returns physical screen pixels as groups of left/top/width/height.
unsafe fn safe_array_rectangles(
    safe_array: *mut SAFEARRAY,
) -> Result<Vec<ScreenRect>, WindowsError> {
    if safe_array.is_null() {
        return Ok(Vec::new());
    }
    let result = (|| {
        let lower = unsafe { SafeArrayGetLBound(safe_array, 1)? };
        let upper = unsafe { SafeArrayGetUBound(safe_array, 1)? };
        let count = (upper - lower + 1).max(0) as usize;
        if count == 0 || count % 4 != 0 {
            return Ok(Vec::new());
        }
        let mut raw: *mut c_void = std::ptr::null_mut();
        unsafe { SafeArrayAccessData(safe_array, &mut raw)? };
        let values = unsafe { std::slice::from_raw_parts(raw.cast::<f64>(), count) };
        let rects = values
            .chunks_exact(4)
            .filter_map(|value| {
                let left = value[0].round() as i32;
                let top = value[1].round() as i32;
                let width = value[2].round() as i32;
                let height = value[3].round() as i32;
                (width > 0 && height > 0).then_some(ScreenRect {
                    left,
                    top,
                    right: left.saturating_add(width),
                    bottom: top.saturating_add(height),
                })
            })
            .collect();
        unsafe { SafeArrayUnaccessData(safe_array)? };
        Ok(rects)
    })();
    let _ = unsafe { SafeArrayDestroy(safe_array) };
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_priority_is_explicit_and_stable() {
        let order = [
            CandidateOrigin::Focused,
            CandidateOrigin::Pointer,
            CandidateOrigin::RawAncestor,
            CandidateOrigin::ContentAncestor,
            CandidateOrigin::ControlAncestor,
            CandidateOrigin::WindowRoot,
            CandidateOrigin::RootSubtree,
        ];
        assert_eq!(order[0], CandidateOrigin::Focused);
        assert_eq!(order[1], CandidateOrigin::Pointer);
        assert_eq!(order.last(), Some(&CandidateOrigin::RootSubtree));
    }

    #[test]
    fn logical_dedup_preserves_first_candidate_priority() {
        let mut ids = Vec::new();
        for id in [10, 20, 10, 30, 20] {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        assert_eq!(ids, [10, 20, 30]);
    }

    #[test]
    fn pattern_attempt_order_falls_back_to_text_pattern() {
        let attempts = ["UIA:TextPattern2", "UIA:TextPattern"];
        assert_eq!(attempts, ["UIA:TextPattern2", "UIA:TextPattern"]);
    }

    #[test]
    fn text_without_geometry_is_a_valid_capture_shape() {
        let capture = SelectionCapture {
            text: "selected".to_owned(),
            rect: None,
            method: "UIA:TextPattern",
        };
        assert!(!capture.text.is_empty());
        assert!(capture.rect.is_none());
    }
}
