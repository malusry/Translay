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
                IUIAutomationTextPattern2, IUIAutomationTextRange, IUIAutomationTextRangeArray,
                TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start, TextUnit_Character,
                TextUnit_Line, UIA_DocumentControlTypeId, UIA_TextPattern2Id, UIA_TextPatternId,
            },
            WindowsAndMessaging::GetCursorPos,
        },
    },
    core::Error as WindowsError,
};

use crate::{
    capture_session::CancellationToken,
    config::{
        ACADEMIC_CONTEXT_SIDE_CHARS, UIA_MAX_ANCESTORS, UIA_MAX_CANDIDATES,
        UIA_MAX_SUBTREE_ELEMENTS, UIA_SEARCH_BUDGET,
    },
    foreground_context::ForegroundContext,
    models::ScreenRect,
};

#[derive(Debug)]
pub struct SelectionCapture {
    #[cfg(debug_assertions)]
    pub document_evidence: Option<crate::document_probe::Evidence>,
    pub text: String,
    pub context_before: Option<String>,
    pub context_after: Option<String>,
    pub rect: Option<ScreenRect>,
    pub line_rects: Vec<ScreenRect>,
    pub nearby_rects: Option<Vec<ScreenRect>>,
    pub method: &'static str,
}

#[derive(Clone, Copy)]
pub struct MouseSelectionHint {
    pub anchor: POINT,
    pub focus: POINT,
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
        mouse_hint: Option<MouseSelectionHint>,
        capture_academic_context: bool,
    ) -> Result<SelectionCapture, SelectionFailure> {
        Self::capture_internal(
            context,
            cancellation,
            timeout,
            mouse_hint,
            capture_academic_context,
            false,
        )
    }

    pub fn capture_for_button(
        context: ForegroundContext,
        cancellation: std::sync::Arc<CancellationToken>,
        timeout: Duration,
        mouse_hint: Option<MouseSelectionHint>,
        capture_academic_context: bool,
    ) -> Result<SelectionCapture, SelectionFailure> {
        Self::capture_internal(
            context,
            cancellation,
            timeout,
            mouse_hint,
            capture_academic_context,
            true,
        )
    }

    fn capture_internal(
        context: ForegroundContext,
        cancellation: std::sync::Arc<CancellationToken>,
        timeout: Duration,
        mouse_hint: Option<MouseSelectionHint>,
        capture_academic_context: bool,
        button_geometry: bool,
    ) -> Result<SelectionCapture, SelectionFailure> {
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("translay-uia-capture".into())
            .spawn(move || {
                if cancellation.is_cancelled() {
                    return;
                }
                let result = capture_selection(
                    context,
                    &cancellation,
                    mouse_hint,
                    capture_academic_context,
                    button_geometry.then_some(&sender),
                );
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
            Ok(Ok(basic)) if button_geometry => {
                // Optional neighboring geometry must not make valid text wait
                // for a slow provider. Keep the basic capture after 80 ms.
                Ok(receive_optional_geometry(
                    &receiver,
                    basic,
                    Duration::from_millis(80),
                ))
            }
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

fn receive_optional_geometry(
    receiver: &mpsc::Receiver<Result<SelectionCapture, SelectionFailure>>,
    basic: SelectionCapture,
    budget: Duration,
) -> SelectionCapture {
    match receiver.recv_timeout(budget) {
        Ok(Ok(enriched)) => enriched,
        _ => basic,
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
    mouse_hint: Option<MouseSelectionHint>,
    capture_academic_context: bool,
    button_progress: Option<&mpsc::SyncSender<Result<SelectionCapture, SelectionFailure>>>,
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
    #[cfg(debug_assertions)]
    if crate::document_probe::allows_background(&context) {
        let documents = crate::document_probe::document_candidates(
            &automation,
            &root,
            context.is_valid_and_foreground(),
        );
        if crate::document_probe::ambiguous_selection(&documents) {
            return Err(SelectionFailure::static_error(
                "PROBE_AMBIGUOUS_SELECTION",
                "测试文档存在多个选区或读取不完整，不能确定归属",
                false,
            ));
        }
        for element in documents {
            push_unique(
                &automation,
                &mut candidates,
                element,
                CandidateOrigin::RootSubtree,
            );
        }
    }

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
        let foreground_valid = context.is_valid_and_foreground();
        #[cfg(debug_assertions)]
        let foreground_valid =
            foreground_valid || crate::document_probe::allows_background(&context);
        if !foreground_valid {
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
            match read_pattern_selection(
                &pattern,
                "UIA:TextPattern2",
                mouse_hint,
                capture_academic_context,
                button_progress,
            ) {
                Ok(capture) => {
                    #[cfg(debug_assertions)]
                    let capture = {
                        let mut capture = capture;
                        if crate::document_probe::enabled() {
                            capture.document_evidence = Some(crate::document_probe::inspect(
                                &automation,
                                &raw_walker,
                                &candidate.element,
                                &root,
                            ));
                        }
                        capture
                    };
                    return Ok(capture);
                }
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
            match read_pattern_selection(
                &pattern,
                "UIA:TextPattern",
                mouse_hint,
                capture_academic_context,
                button_progress,
            ) {
                Ok(capture) => {
                    #[cfg(debug_assertions)]
                    let capture = {
                        let mut capture = capture;
                        if crate::document_probe::enabled() {
                            capture.document_evidence = Some(crate::document_probe::inspect(
                                &automation,
                                &raw_walker,
                                &candidate.element,
                                &root,
                            ));
                        }
                        capture
                    };
                    return Ok(capture);
                }
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
    mouse_hint: Option<MouseSelectionHint>,
    capture_academic_context: bool,
    button_progress: Option<&mpsc::SyncSender<Result<SelectionCapture, SelectionFailure>>>,
) -> Result<SelectionCapture, PatternReadError> {
    let ranges = unsafe { pattern.GetSelection() }
        .map_err(|error| PatternReadError::Windows("GET_SELECTION", error))?;
    read_ranges(
        pattern,
        ranges,
        method,
        mouse_hint,
        capture_academic_context,
        button_progress,
    )
}

fn read_ranges(
    pattern: &IUIAutomationTextPattern,
    ranges: IUIAutomationTextRangeArray,
    method: &'static str,
    mouse_hint: Option<MouseSelectionHint>,
    capture_academic_context: bool,
    button_progress: Option<&mpsc::SyncSender<Result<SelectionCapture, SelectionFailure>>>,
) -> Result<SelectionCapture, PatternReadError> {
    let length = unsafe { ranges.Length() }
        .map_err(|error| PatternReadError::Windows("SELECTION_LENGTH", error))?;
    if length <= 0 {
        return Err(PatternReadError::Empty);
    }

    let mut text_parts = Vec::new();
    let mut rectangles = Vec::new();
    let mut point_refined = false;
    let mut context_before = None;
    let mut context_after = None;
    let mut geometry_range = None;
    for index in 0..length {
        let selected_range = unsafe { ranges.GetElement(index) }
            .map_err(|error| PatternReadError::Windows("SELECTION_RANGE", error))?;
        let range = if length == 1 {
            mouse_hint
                .and_then(|hint| refine_text_range_to_mouse_focus(pattern, &selected_range, hint))
                .map(|range| {
                    point_refined = true;
                    range
                })
                .unwrap_or(selected_range)
        } else {
            selected_range
        };
        if button_progress.is_some() && length == 1 {
            geometry_range = Some(range.clone());
        }
        if capture_academic_context && length == 1 {
            (context_before, context_after) = read_adjacent_context(&range);
        }
        let text = unsafe { range.GetText(-1) }
            .map_err(|error| PatternReadError::Windows("GET_TEXT", error))?;
        let text = String::from_utf16_lossy(&text);
        if !text.is_empty() {
            text_parts.push(text);
        }

        // Bounding rectangles are optional. Text remains a valid capture when a
        // provider returns no geometry; the overlay will anchor to the cursor.
        if let Ok(safe_array) = unsafe { range.GetBoundingRectangles() } {
            if let Ok(range_rectangles) = unsafe { safe_array_rectangles(safe_array) } {
                rectangles.extend(range_rectangles);
            }
        }
    }

    let text = text_parts.join("\n");
    let (text, rectangles, geometry_refined) =
        refine_selection_with_mouse_hint(text, rectangles, mouse_hint);
    if text.trim().is_empty() {
        return Err(PatternReadError::Empty);
    }
    let union_rect = rectangles.iter().copied().reduce(ScreenRect::union);
    let mut capture = SelectionCapture {
        #[cfg(debug_assertions)]
        document_evidence: None,
        text,
        context_before,
        context_after,
        rect: union_rect.filter(|rect| rect.width() > 0 && rect.height() > 0),
        line_rects: rectangles,
        nearby_rects: None,
        method: if point_refined {
            match method {
                "UIA:TextPattern2" => "UIA:TextPattern2:MousePoint",
                _ => "UIA:TextPattern:MousePoint",
            }
        } else if geometry_refined {
            match method {
                "UIA:TextPattern2" => "UIA:TextPattern2:MouseGeometry",
                _ => "UIA:TextPattern:MouseGeometry",
            }
        } else {
            method
        },
    };
    if let Some(sender) = button_progress {
        let basic = SelectionCapture {
            #[cfg(debug_assertions)]
            document_evidence: None,
            text: capture.text.clone(),
            context_before: capture.context_before.clone(),
            context_after: capture.context_after.clone(),
            rect: capture.rect,
            line_rects: capture.line_rects.clone(),
            nearby_rects: None,
            method: capture.method,
        };
        let _ = sender.try_send(Ok(basic));
        if let Some(range) = geometry_range {
            capture.nearby_rects = read_nearby_geometry(pattern, &range, mouse_hint);
        }
    }
    Ok(capture)
}

// Geometry only, on the existing timeout-isolated worker. Never change the
// user's selection or add surrounding text to the translation request.
fn read_nearby_geometry(
    pattern: &IUIAutomationTextPattern,
    selected: &IUIAutomationTextRange,
    hint: Option<MouseSelectionHint>,
) -> Option<Vec<ScreenRect>> {
    unsafe {
        let range = if let Some(hint) = hint {
            pattern.RangeFromPoint(hint.focus).ok()?
        } else {
            let range = selected.Clone().ok()?;
            range
                .MoveEndpointByRange(
                    TextPatternRangeEndpoint_Start,
                    selected,
                    TextPatternRangeEndpoint_End,
                )
                .ok()?;
            range
        };
        range.ExpandToEnclosingUnit(TextUnit_Line).ok()?;
        range
            .MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Line, -1)
            .ok()?;
        range
            .MoveEndpointByUnit(TextPatternRangeEndpoint_End, TextUnit_Line, 1)
            .ok()?;
        let rects = safe_array_rectangles(range.GetBoundingRectangles().ok()?).ok()?;
        (!rects.is_empty()).then_some(rects)
    }
}

/// Edge's PDF provider can occasionally report a selection whose final range
/// reaches into the following visual line. RangeFromPoint gives us the text
/// insertion position nearest the actual mouse-up coordinate, so only the
/// release-side endpoint is allowed to move inward to that position.
fn refine_text_range_to_mouse_focus(
    pattern: &IUIAutomationTextPattern,
    selected_range: &IUIAutomationTextRange,
    mouse_hint: MouseSelectionHint,
) -> Option<IUIAutomationTextRange> {
    let anchor_range = unsafe { pattern.RangeFromPoint(mouse_hint.anchor) }.ok()?;
    let focus_range = unsafe { pattern.RangeFromPoint(mouse_hint.focus) }.ok()?;
    let anchor_vs_focus = unsafe {
        anchor_range.CompareEndpoints(
            TextPatternRangeEndpoint_Start,
            &focus_range,
            TextPatternRangeEndpoint_Start,
        )
    }
    .ok()?;
    let selection_start_vs_focus = unsafe {
        selected_range.CompareEndpoints(
            TextPatternRangeEndpoint_Start,
            &focus_range,
            TextPatternRangeEndpoint_Start,
        )
    }
    .ok()?;
    let selection_end_vs_focus = unsafe {
        selected_range.CompareEndpoints(
            TextPatternRangeEndpoint_End,
            &focus_range,
            TextPatternRangeEndpoint_Start,
        )
    }
    .ok()?;

    let endpoint = endpoint_to_clamp(
        anchor_vs_focus,
        selection_start_vs_focus,
        selection_end_vs_focus,
    )?;
    let refined_range = unsafe { selected_range.Clone() }.ok()?;
    unsafe {
        refined_range.MoveEndpointByRange(endpoint, &focus_range, TextPatternRangeEndpoint_Start)
    }
    .ok()?;
    let text = unsafe { refined_range.GetText(-1) }.ok()?;
    (!String::from_utf16_lossy(&text).trim().is_empty()).then_some(refined_range)
}

fn endpoint_to_clamp(
    anchor_vs_focus: i32,
    selection_start_vs_focus: i32,
    selection_end_vs_focus: i32,
) -> Option<windows::Win32::UI::Accessibility::TextPatternRangeEndpoint> {
    // The focus point must be inside the provider's selected range. This keeps
    // RangeFromPoint from ever expanding the selection when a provider returns
    // an unrelated nearby text position.
    if selection_start_vs_focus > 0 || selection_end_vs_focus < 0 {
        return None;
    }

    if anchor_vs_focus <= 0 && selection_end_vs_focus > 0 {
        Some(TextPatternRangeEndpoint_End)
    } else if anchor_vs_focus > 0 && selection_start_vs_focus < 0 {
        Some(TextPatternRangeEndpoint_Start)
    } else {
        None
    }
}

fn read_adjacent_context(
    selected_range: &IUIAutomationTextRange,
) -> (Option<String>, Option<String>) {
    let context_before = (|| {
        let range = unsafe { selected_range.Clone() }.ok()?;
        unsafe {
            range.MoveEndpointByRange(
                TextPatternRangeEndpoint_End,
                selected_range,
                TextPatternRangeEndpoint_Start,
            )
        }
        .ok()?;
        unsafe {
            range.MoveEndpointByUnit(
                TextPatternRangeEndpoint_Start,
                TextUnit_Character,
                -ACADEMIC_CONTEXT_SIDE_CHARS,
            )
        }
        .ok()?;
        read_context_range(&range, true)
    })();

    let context_after = (|| {
        let range = unsafe { selected_range.Clone() }.ok()?;
        unsafe {
            range.MoveEndpointByRange(
                TextPatternRangeEndpoint_Start,
                selected_range,
                TextPatternRangeEndpoint_End,
            )
        }
        .ok()?;
        unsafe {
            range.MoveEndpointByUnit(
                TextPatternRangeEndpoint_End,
                TextUnit_Character,
                ACADEMIC_CONTEXT_SIDE_CHARS,
            )
        }
        .ok()?;
        read_context_range(&range, false)
    })();

    (context_before, context_after)
}

fn read_context_range(range: &IUIAutomationTextRange, before: bool) -> Option<String> {
    let text = unsafe { range.GetText(-1) }.ok()?;
    let text = String::from_utf16_lossy(&text);
    nearest_sentence_context(&text, before)
}

fn nearest_sentence_context(text: &str, before: bool) -> Option<String> {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let clipped = if before {
        take_suffix_chars(&normalized, ACADEMIC_CONTEXT_SIDE_CHARS as usize)
    } else {
        normalized
            .chars()
            .take(ACADEMIC_CONTEXT_SIDE_CHARS as usize)
            .collect()
    };
    let characters = clipped.chars().collect::<Vec<_>>();
    if characters.is_empty() {
        return None;
    }

    let sentence = if before {
        let search_end = characters
            .len()
            .saturating_sub(usize::from(is_sentence_boundary(
                &characters,
                characters.len() - 1,
            )));
        let start = (0..search_end)
            .rev()
            .find(|index| is_sentence_boundary(&characters, *index))
            .map_or(0, |index| index + 1);
        characters[start..].iter().collect::<String>()
    } else {
        let end = (0..characters.len())
            .find(|index| is_sentence_boundary(&characters, *index))
            .map_or(characters.len(), |index| index + 1);
        characters[..end].iter().collect::<String>()
    };
    let sentence = sentence.trim().to_owned();
    (!sentence.is_empty()).then_some(sentence)
}

fn take_suffix_chars(text: &str, limit: usize) -> String {
    let character_count = text.chars().count();
    text.chars()
        .skip(character_count.saturating_sub(limit))
        .collect()
}

fn is_sentence_boundary(characters: &[char], index: usize) -> bool {
    match characters[index] {
        '!' | '?' | '。' | '！' | '？' => true,
        '.' => characters
            .get(index + 1)
            .is_none_or(|character| character.is_whitespace()),
        _ => false,
    }
}

fn refine_selection_with_mouse_hint(
    text: String,
    rectangles: Vec<ScreenRect>,
    mouse_hint: Option<MouseSelectionHint>,
) -> (String, Vec<ScreenRect>, bool) {
    let Some(mouse_hint) = mouse_hint else {
        return (text, rectangles, false);
    };
    if rectangles.len() <= 1 {
        return (text, rectangles, false);
    }

    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines = normalized.lines().collect::<Vec<_>>();
    let visible_line_indices = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (!line.trim().is_empty()).then_some(index))
        .collect::<Vec<_>>();
    if visible_line_indices.len() != rectangles.len() {
        return (text, rectangles, false);
    }

    let anchor_line = nearest_rectangle(mouse_hint.anchor, &rectangles);
    let focus_line = nearest_rectangle(mouse_hint.focus, &rectangles);
    let first_line = anchor_line.min(focus_line);
    let last_line = anchor_line.max(focus_line);
    if first_line == 0 && last_line + 1 == rectangles.len() {
        return (text, rectangles, false);
    }

    let first_text_line = visible_line_indices[first_line];
    let last_text_line = visible_line_indices[last_line];
    let refined_text = lines[first_text_line..=last_text_line].join("\n");
    let refined_rectangles = rectangles[first_line..=last_line].to_vec();
    (refined_text, refined_rectangles, true)
}

fn nearest_rectangle(point: POINT, rectangles: &[ScreenRect]) -> usize {
    rectangles
        .iter()
        .enumerate()
        .min_by_key(|(_, rectangle)| squared_distance_to_rectangle(point, **rectangle))
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn squared_distance_to_rectangle(point: POINT, rectangle: ScreenRect) -> i64 {
    let horizontal = if point.x < rectangle.left {
        i64::from(rectangle.left) - i64::from(point.x)
    } else if point.x >= rectangle.right {
        i64::from(point.x) - i64::from(rectangle.right) + 1
    } else {
        0
    };
    let vertical = if point.y < rectangle.top {
        i64::from(rectangle.top) - i64::from(point.y)
    } else if point.y >= rectangle.bottom {
        i64::from(point.y) - i64::from(rectangle.bottom) + 1
    } else {
        0
    };
    horizontal * horizontal + vertical * vertical
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
                // Reject nonsensical provider coordinates before converting
                // to integer screen geometry or doing placement arithmetic.
                if value
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 10_000_000.0)
                {
                    return None;
                }
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
    fn optional_geometry_timeout_or_disconnect_keeps_valid_selection() {
        let basic = || SelectionCapture {
            #[cfg(debug_assertions)]
            document_evidence: None,
            text: "selected text".to_owned(),
            context_before: None,
            context_after: None,
            rect: Some(line_rect(100)),
            line_rects: vec![line_rect(100)],
            nearby_rects: None,
            method: "UIA:TextPattern",
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let timed_out = receive_optional_geometry(&receiver, basic(), Duration::ZERO);
        assert_eq!(timed_out.text, "selected text");
        assert_eq!(timed_out.line_rects, vec![line_rect(100)]);
        let mut enriched = basic();
        enriched.nearby_rects = Some(vec![line_rect(124)]);
        sender.send(Ok(enriched)).unwrap();
        assert!(
            receive_optional_geometry(&receiver, basic(), Duration::ZERO)
                .nearby_rects
                .is_some()
        );
        drop(sender);
        assert_eq!(
            receive_optional_geometry(&receiver, basic(), Duration::ZERO).text,
            "selected text"
        );
    }

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
            #[cfg(debug_assertions)]
            document_evidence: None,
            text: "selected".to_owned(),
            context_before: None,
            context_after: None,
            rect: None,
            line_rects: Vec::new(),
            nearby_rects: None,
            method: "UIA:TextPattern",
        };
        assert!(!capture.text.is_empty());
        assert!(capture.rect.is_none());
    }

    #[test]
    fn academic_context_keeps_only_the_nearest_sentence_on_each_side() {
        assert_eq!(
            nearest_sentence_context("An older sentence. The immediately preceding claim.", true,)
                .as_deref(),
            Some("The immediately preceding claim.")
        );
        assert_eq!(
            nearest_sentence_context(
                "The immediately following limitation. A later sentence.",
                false,
            )
            .as_deref(),
            Some("The immediately following limitation.")
        );
    }

    #[test]
    fn academic_context_preserves_a_partial_neighboring_sentence() {
        assert_eq!(
            nearest_sentence_context("Earlier sentence. because the baseline", true).as_deref(),
            Some("because the baseline")
        );
        assert_eq!(
            nearest_sentence_context("depends on the initialization. Later sentence.", false)
                .as_deref(),
            Some("depends on the initialization.")
        );
    }

    #[test]
    fn mouse_geometry_removes_a_provider_only_trailing_line() {
        let rectangles = vec![line_rect(100), line_rect(124)];
        let hint = MouseSelectionHint {
            anchor: POINT { x: 120, y: 108 },
            focus: POINT { x: 480, y: 108 },
        };

        let (text, rectangles, refined) = refine_selection_with_mouse_hint(
            "selected line\nunexpected next line".to_owned(),
            rectangles,
            Some(hint),
        );

        assert_eq!(text, "selected line");
        assert_eq!(rectangles, [line_rect(100)]);
        assert!(refined);
    }

    #[test]
    fn mouse_geometry_preserves_an_intentional_multiline_selection() {
        let rectangles = vec![line_rect(100), line_rect(124)];
        let hint = MouseSelectionHint {
            anchor: POINT { x: 120, y: 108 },
            focus: POINT { x: 480, y: 132 },
        };

        let (text, kept_rectangles, refined) = refine_selection_with_mouse_hint(
            "first line\nsecond line".to_owned(),
            rectangles.clone(),
            Some(hint),
        );

        assert_eq!(text, "first line\nsecond line");
        assert_eq!(kept_rectangles, rectangles);
        assert!(!refined);
    }

    #[test]
    fn mouse_geometry_preserves_blank_paragraph_spacing_while_trimming_the_next_line() {
        let rectangles = vec![line_rect(100), line_rect(148), line_rect(172)];
        let hint = MouseSelectionHint {
            anchor: POINT { x: 120, y: 108 },
            focus: POINT { x: 480, y: 156 },
        };

        let (text, kept_rectangles, refined) = refine_selection_with_mouse_hint(
            "first paragraph line\n\nsecond paragraph ending\nunexpected next line".to_owned(),
            rectangles,
            Some(hint),
        );

        assert_eq!(text, "first paragraph line\n\nsecond paragraph ending");
        assert_eq!(kept_rectangles, [line_rect(100), line_rect(148)]);
        assert!(refined);
    }

    #[test]
    fn mouse_geometry_refuses_to_guess_when_text_and_rows_do_not_match() {
        let rectangles = vec![line_rect(100), line_rect(124)];
        let hint = MouseSelectionHint {
            anchor: POINT { x: 120, y: 108 },
            focus: POINT { x: 480, y: 108 },
        };

        let (text, kept_rectangles, refined) = refine_selection_with_mouse_hint(
            "provider text without a line boundary".to_owned(),
            rectangles.clone(),
            Some(hint),
        );

        assert_eq!(text, "provider text without a line boundary");
        assert_eq!(kept_rectangles, rectangles);
        assert!(!refined);
    }

    #[test]
    fn mouse_geometry_handles_a_backward_selection() {
        let rectangles = vec![line_rect(100), line_rect(124), line_rect(148)];
        let hint = MouseSelectionHint {
            anchor: POINT { x: 480, y: 156 },
            focus: POINT { x: 120, y: 132 },
        };

        let (text, kept_rectangles, refined) = refine_selection_with_mouse_hint(
            "unexpected prior line\nselected middle line\nselected final line".to_owned(),
            rectangles,
            Some(hint),
        );

        assert_eq!(text, "selected middle line\nselected final line");
        assert_eq!(kept_rectangles, [line_rect(124), line_rect(148)]);
        assert!(refined);
    }

    #[test]
    fn mouse_point_clamps_a_provider_range_after_the_forward_drag_focus() {
        assert_eq!(
            endpoint_to_clamp(-1, -1, 1),
            Some(TextPatternRangeEndpoint_End)
        );
    }

    #[test]
    fn mouse_point_clamps_a_provider_range_before_the_backward_drag_focus() {
        assert_eq!(
            endpoint_to_clamp(1, -1, 1),
            Some(TextPatternRangeEndpoint_Start)
        );
    }

    #[test]
    fn mouse_point_does_not_expand_or_change_an_already_exact_range() {
        assert_eq!(endpoint_to_clamp(-1, -1, 0), None);
        assert_eq!(endpoint_to_clamp(-1, 1, 1), None);
        assert_eq!(endpoint_to_clamp(1, -1, -1), None);
    }

    fn line_rect(top: i32) -> ScreenRect {
        ScreenRect {
            left: 100,
            top,
            right: 500,
            bottom: top + 18,
        }
    }
}
