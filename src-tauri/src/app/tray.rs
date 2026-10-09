use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use tauri::{
    Manager,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tracing::warn;

use crate::{
    credential_store::CredentialStore,
    model_config::{ModelConfig, ModelConfigStore},
    selection_button::SelectionButtonManager,
    translation::TranslationMode,
};

use super::{
    model_backend::{alternate_model_backend, switch_model_backend, sync_model_backend_surfaces},
    shutdown::ShutdownCoordinator,
    tray_feedback::TrayFeedbackManager,
};

const MAIN_TRAY_ID: &str = "main-tray";
const FOLD_STEPS: usize = 20;
const CHARGED_POSE: usize = 24;
const FOLD_DURATION: Duration = Duration::from_millis(260);
const CHARGE_DURATION: Duration = Duration::from_millis(90);
const FRAME_INTERVAL: Duration = Duration::from_millis(16);

struct TrayVisuals(Mutex<FoldAnimation>);

struct FoldMotion {
    generation: u64,
    from: usize,
    started: Instant,
    duration: Duration,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FoldIntent {
    Config,
    Preview { click: u64 },
    Pulse { charging: bool },
}

// Only the last successfully presented pose is retained. Retargeting starts
// there, so a reversal never jumps to an endpoint. No system call holds this lock.
struct FoldAnimation {
    frame: usize,
    target: usize,
    light: bool,
    presented_light: bool,
    config_enabled: bool,
    intent: FoldIntent,
    generation: u64,
    motion: Option<FoldMotion>,
    stopped: bool,
    tooltip: Option<String>,
}

impl FoldAnimation {
    fn new(light: bool, enabled: bool) -> Self {
        let frame = fold_target(enabled);
        Self {
            frame,
            target: frame,
            light,
            presented_light: light,
            config_enabled: enabled,
            intent: FoldIntent::Config,
            generation: 0,
            motion: None,
            stopped: false,
            tooltip: None,
        }
    }

    fn retarget(&mut self, target: usize, now: Instant) -> Option<u64> {
        let duration =
            FOLD_DURATION.mul_f64(self.frame.abs_diff(target) as f64 / FOLD_STEPS as f64);
        self.retarget_for(target, now, duration)
    }

    fn retarget_for(&mut self, target: usize, now: Instant, duration: Duration) -> Option<u64> {
        if self.stopped
            || (self.target == target && (self.motion.is_some() || self.frame == target))
        {
            return None;
        }
        self.target = target;
        self.generation = self.generation.wrapping_add(1);
        self.motion = (self.frame != target).then(|| FoldMotion {
            generation: self.generation,
            from: self.frame,
            started: now,
            duration,
        });
        self.motion.as_ref().map(|motion| motion.generation)
    }

    fn reconcile(&mut self, enabled: bool, now: Instant) -> Option<u64> {
        // A real preference change supersedes any speculative gesture feedback.
        if self.config_enabled != enabled {
            self.config_enabled = enabled;
            self.intent = FoldIntent::Config;
        }
        if self.frame == CHARGED_POSE && self.intent == (FoldIntent::Pulse { charging: true }) {
            self.intent = FoldIntent::Pulse { charging: false };
        }
        match self.intent {
            FoldIntent::Config => self.retarget(fold_target(enabled), now),
            FoldIntent::Preview { .. } => self.retarget(fold_target(!enabled), now),
            FoldIntent::Pulse { charging: true } => {
                self.retarget_for(CHARGED_POSE, now, CHARGE_DURATION)
            }
            FoldIntent::Pulse { charging: false } => self.retarget_for(
                fold_target(enabled),
                now,
                Duration::from_millis(if enabled { 220 } else { 140 }),
            ),
        }
    }

    fn sample(&self, generation: u64, now: Instant) -> Option<(usize, bool)> {
        let motion = self.motion.as_ref()?;
        if self.stopped || motion.generation != generation {
            return None;
        }
        let t = (now.saturating_duration_since(motion.started).as_secs_f64()
            / motion.duration.as_secs_f64())
        .min(1.0);
        let eased = t * (2.0 - t);
        let frame = (motion.from as f64 + (self.target as f64 - motion.from as f64) * eased).round()
            as usize;
        Some((frame, t >= 1.0))
    }

    fn finish_frame(
        &mut self,
        generation: u64,
        frame: usize,
        done: bool,
        now: Instant,
    ) -> Option<u64> {
        if self
            .motion
            .as_ref()
            .is_some_and(|motion| motion.generation == generation)
            && !self.stopped
        {
            self.frame = frame;
            if done {
                self.motion = None;
                match self.intent {
                    FoldIntent::Pulse { charging: true } => {
                        self.intent = FoldIntent::Pulse { charging: false };
                        return self.reconcile(self.config_enabled, now);
                    }
                    FoldIntent::Pulse { charging: false } => self.intent = FoldIntent::Config,
                    _ => {}
                }
            }
        }
        None
    }

    fn cancel(&mut self, generation: u64) {
        if self.generation == generation {
            self.motion = None;
            self.intent = FoldIntent::Config;
        }
    }

    fn cancel_preview(&mut self, click: Option<u64>) -> bool {
        if matches!(self.intent, FoldIntent::Preview { click: current } if click.is_none_or(|expected| expected == current))
        {
            self.intent = FoldIntent::Config;
            true
        } else {
            false
        }
    }

    fn stop(&mut self) {
        self.stopped = true;
        self.motion = None;
        self.intent = FoldIntent::Config;
    }
}

fn fold_target(enabled: bool) -> usize {
    if enabled { 0 } else { FOLD_STEPS }
}

#[derive(Default)]
struct TrayClicks(Mutex<LeftClicks>);

#[derive(Default)]
struct LeftClicks {
    generation: u64,
    pending: Option<u64>,
    double_release: bool,
    stopped: bool,
}

impl LeftClicks {
    fn pressed(&mut self) -> bool {
        // A normal second down is a separate click; Windows replaces it with
        // DoubleClick for a double. It also recovers a release lost off-icon.
        self.double_release = false;
        !self.stopped && self.pending.take().is_some()
    }

    fn released(&mut self) -> Option<u64> {
        if self.stopped || std::mem::take(&mut self.double_release) {
            return None;
        }
        self.generation = self.generation.wrapping_add(1);
        self.pending = Some(self.generation);
        self.pending
    }

    fn double_pressed(&mut self) -> bool {
        self.pending = None;
        if self.stopped || self.double_release {
            return false;
        }
        self.double_release = true;
        true
    }

    fn expire(&mut self, generation: u64) -> bool {
        if self.stopped || self.pending != Some(generation) {
            return false;
        }
        self.pending = None;
        true
    }

    fn cancel(&mut self) {
        self.pending = None;
        self.double_release = false;
    }

    fn stop(&mut self) {
        self.stopped = true;
        self.cancel();
    }
}

pub(super) fn stop_clicks(app: &tauri::AppHandle) {
    if let Some(clicks) = app.try_state::<TrayClicks>() {
        clicks.0.lock().unwrap_or_else(|e| e.into_inner()).stop();
    }
    if let Some(visuals) = app.try_state::<TrayVisuals>() {
        visuals.0.lock().unwrap_or_else(|e| e.into_inner()).stop();
    }
}

pub(super) fn double_click_interval() -> Duration {
    Duration::from_millis(unsafe {
        ::windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime()
    } as u64)
}

fn toggle_selection_icon(app: &tauri::AppHandle) -> Result<(), String> {
    let config = app.state::<ModelConfigStore>();
    let next = !config.get().selection_icon_enabled;
    config.set_selection_icon_enabled(next)?;
    let result = app.state::<SelectionButtonManager>().set_enabled(app, next);
    if let Err(error) = refresh_tray_state(app) {
        warn!(%error, "tray selection state refresh failed");
    }
    result
}

fn apply_single_click(app: &tauri::AppHandle) {
    if app.state::<ShutdownCoordinator>().is_exiting() {
        return;
    }
    // The visual preview already moved; committing must not play it again.
    app.state::<TrayVisuals>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .intent = FoldIntent::Config;
    if let Err(error) = toggle_selection_icon(app) {
        warn!(%error, "tray selection icon toggle failed");
        if let Err(error) = refresh_tray_state(app) {
            warn!(%error, "tray preview rollback failed");
        }
    }
    #[cfg(debug_assertions)]
    super::tray_menu::record_manual_event(app, "tray-single-selection-toggle");
}

fn preview_single_click(app: &tauri::AppHandle, click: u64) {
    let enabled = app.state::<ModelConfigStore>().get().selection_icon_enabled;
    {
        let visuals = app.state::<TrayVisuals>();
        let mut animation = visuals.0.lock().unwrap_or_else(|e| e.into_inner());
        animation.config_enabled = enabled;
        animation.intent = FoldIntent::Preview { click };
    }
    if let Err(error) = refresh_tray_state(app) {
        warn!(%error, "tray click preview failed");
    }
}

fn cancel_click_preview(app: &tauri::AppHandle, click: Option<u64>) {
    let cancelled = {
        let visuals = app.state::<TrayVisuals>();
        let mut animation = visuals.0.lock().unwrap_or_else(|e| e.into_inner());
        animation.cancel_preview(click)
    };
    if cancelled {
        if let Err(error) = refresh_tray_state(app) {
            warn!(%error, "tray preview cancellation failed");
        }
    }
}

fn animate_double_click(app: &tauri::AppHandle) {
    let enabled = app.state::<ModelConfigStore>().get().selection_icon_enabled;
    {
        let visuals = app.state::<TrayVisuals>();
        let mut animation = visuals.0.lock().unwrap_or_else(|e| e.into_inner());
        animation.config_enabled = enabled;
        animation.intent = FoldIntent::Pulse { charging: true };
    }
    if let Err(error) = refresh_tray_state(app) {
        warn!(%error, "tray double-click animation failed");
    }
}

fn schedule_single_click(app: &tauri::AppHandle, generation: u64) {
    let app = app.clone();
    let delay = double_click_interval();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(delay).await;
        let handle = app.clone();
        if let Err(error) = app.run_on_main_thread(move || {
            let claimed = handle
                .state::<TrayClicks>()
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .expire(generation);
            if claimed {
                apply_single_click(&handle);
            }
        }) {
            let claimed = app
                .state::<TrayClicks>()
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .expire(generation);
            warn!(%error, "tray single click dispatch failed");
            // No preference was committed. Reconcile the preview on the next
            // available window callback; no repeated dispatch or retry loop.
            if claimed {
                let handle = app.clone();
                if let Err(error) =
                    app.run_on_main_thread(move || cancel_click_preview(&handle, Some(generation)))
                {
                    warn!(%error, "tray failed click preview cancellation dispatch failed");
                }
            }
        }
    });
}

// Called on the window thread by Tauri, also by the isolated native fixture.
pub(super) fn handle_tray_event(app: &tauri::AppHandle, event: TrayIconEvent) {
    if app.state::<ShutdownCoordinator>().is_exiting() {
        return;
    }
    match event {
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Down,
            ..
        } => {
            let claimed = app
                .state::<TrayClicks>()
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .pressed();
            if claimed {
                apply_single_click(app);
            }
        }
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } => {
            let ticket = app
                .state::<TrayClicks>()
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .released();
            if let Some(ticket) = ticket {
                preview_single_click(app, ticket);
                schedule_single_click(app, ticket);
            }
        }
        TrayIconEvent::DoubleClick {
            button: MouseButton::Left,
            position,
            rect,
            ..
        } => {
            let claimed = app
                .state::<TrayClicks>()
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .double_pressed();
            if !claimed {
                return;
            }
            animate_double_click(app);
            #[cfg(debug_assertions)]
            super::tray_menu::record_manual_event(app, "tray-double-mode-toggle");
            match toggle_translation_mode(app) {
                Ok(mode) => {
                    if let Err(error) = app.state::<TrayFeedbackManager>().show(
                        app,
                        position,
                        Some(rect),
                        mode,
                        translation_mode_feedback(mode).to_owned(),
                    ) {
                        warn!(%error, "tray style feedback failed");
                    }
                }
                Err(error) => warn!(%error, "tray style toggle failed"),
            }
        }
        TrayIconEvent::Click {
            button: MouseButton::Right,
            button_state,
            position,
            rect,
            ..
        } => {
            app.state::<TrayClicks>()
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .cancel();
            cancel_click_preview(app, None);
            if button_state == MouseButtonState::Up {
                #[cfg(debug_assertions)]
                super::tray_menu::record_manual_event(app, "native-tray-right-click");
                super::tray_menu::request_show(app, position, rect);
            }
        }
        _ => {}
    }
}

pub(super) fn refresh_model_switch(
    app: &tauri::AppHandle,
    _config: &ModelConfig,
    credentials: &CredentialStore,
) -> Result<(), String> {
    let _ = credentials;
    super::tray_menu::request_close(app);
    refresh_tray_state(app)
}

pub(super) fn refresh_tray_state(app: &tauri::AppHandle) -> Result<(), String> {
    if app.state::<ShutdownCoordinator>().is_exiting() {
        return Ok(());
    }
    let handle = app.clone();
    app.tray_by_id(MAIN_TRAY_ID)
        .ok_or_else(|| "找不到系统托盘图标".to_owned())?
        .with_inner_tray_icon(move |_| {
            apply_tray_state_on_window_thread(&handle, taskbar_uses_light_theme())
        })
        .map_err(|error| format!("调度托盘状态更新失败：{error}"))?
}

// Both setters and frame decisions run on the window thread. A mode/theme
// refresh updates the current pose, without restarting an ongoing fold.
pub(super) fn apply_tray_state_on_window_thread(
    app: &tauri::AppHandle,
    light: bool,
) -> Result<(), String> {
    if app.state::<ShutdownCoordinator>().is_exiting() {
        return Ok(());
    }
    let config = app.state::<ModelConfigStore>().get();
    let (frame, generation, start) = {
        let visuals = app.state::<TrayVisuals>();
        let mut animation = visuals.0.lock().unwrap_or_else(|e| e.into_inner());
        animation.light = light;
        let start = animation.reconcile(config.selection_icon_enabled, Instant::now());
        (animation.frame, animation.generation, start)
    };
    if let Err(error) = present_tray_frame(app, light, frame, &config) {
        cancel_fold(app, generation);
        return Err(error);
    }
    if let Some(generation) = start {
        schedule_fold(app, generation);
    }
    Ok(())
}

fn present_tray_frame(
    app: &tauri::AppHandle,
    light: bool,
    frame: usize,
    config: &ModelConfig,
) -> Result<(), String> {
    let icon = tray_fold_frame(light, frame);
    let tooltip = tray_tooltip_for_app(app, config);
    let (icon_changed, tooltip_changed) = {
        let visuals = app.state::<TrayVisuals>();
        let animation = visuals.0.lock().unwrap_or_else(|e| e.into_inner());
        (
            frame != animation.frame || light != animation.presented_light,
            animation.tooltip.as_deref() != Some(&tooltip),
        )
    };
    // In particular, starting a fold must not resubmit its unchanged first pose.
    if !icon_changed && !tooltip_changed {
        return Ok(());
    }
    #[cfg(debug_assertions)]
    super::tray_menu::before_tray_frame(app)?;
    let tray = app
        .tray_by_id(MAIN_TRAY_ID)
        .ok_or_else(|| "找不到系统托盘图标".to_owned())?;
    if icon_changed {
        tray.set_icon(Some(icon.clone()))
            .map_err(|error| format!("更新托盘图案失败：{error}"))?;
        let visuals = app.state::<TrayVisuals>();
        let mut animation = visuals.0.lock().unwrap_or_else(|e| e.into_inner());
        animation.frame = frame;
        animation.presented_light = light;
    }
    if tooltip_changed {
        tray.set_tooltip(Some(&tooltip))
            .map_err(|error| format!("更新托盘提示信息失败：{error}"))?;
        app.state::<TrayVisuals>()
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .tooltip = Some(tooltip.clone());
    }
    #[cfg(debug_assertions)]
    super::tray_menu::record_tray_visuals(
        app,
        light,
        config.selection_icon_enabled,
        frame,
        &tooltip,
        icon.rgba(),
    );
    Ok(())
}

fn cancel_fold(app: &tauri::AppHandle, generation: u64) {
    app.state::<TrayVisuals>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .cancel(generation);
}

#[cfg(debug_assertions)]
pub(super) fn fold_snapshot(app: &tauri::AppHandle) -> (usize, u64, bool) {
    let visuals = app.state::<TrayVisuals>();
    let animation = visuals.0.lock().unwrap_or_else(|e| e.into_inner());
    (
        animation.frame,
        animation.generation,
        animation.motion.is_some(),
    )
}

fn advance_fold_on_window_thread(
    app: &tauri::AppHandle,
    generation: u64,
) -> Result<Option<u64>, String> {
    if app.state::<ShutdownCoordinator>().is_exiting() {
        return Ok(None);
    }
    let config = app.state::<ModelConfigStore>().get();
    let (frame, done, light, ticket) = {
        let visuals = app.state::<TrayVisuals>();
        let mut animation = visuals.0.lock().unwrap_or_else(|e| e.into_inner());
        if animation.generation != generation {
            return Ok(None);
        }
        let ticket = animation
            .reconcile(config.selection_icon_enabled, Instant::now())
            .unwrap_or(generation);
        let Some((frame, done)) = animation.sample(ticket, Instant::now()) else {
            return Ok(None);
        };
        (frame, done, animation.light, ticket)
    };
    if let Err(error) = present_tray_frame(app, light, frame, &config) {
        cancel_fold(app, ticket);
        return Err(error);
    }
    let next = app
        .state::<TrayVisuals>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .finish_frame(ticket, frame, done, Instant::now());
    Ok(next.or_else(|| (!done).then_some(ticket)))
}

fn schedule_fold(app: &tauri::AppHandle, generation: u64) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut generation = generation;
        loop {
            tokio::time::sleep(FRAME_INTERVAL).await;
            let handle = app.clone();
            // Await each actual presentation; never queue a backlog of frames.
            // Elapsed time selects the pose, so a busy window thread skips ahead.
            match super::windows::on_window_thread(&app, move || {
                advance_fold_on_window_thread(&handle, generation)
            })
            .await
            {
                Ok(Some(next)) => generation = next,
                Ok(None) => break,
                Err(error) => {
                    cancel_fold(&app, generation);
                    warn!(%error, "tray fold animation stopped; next state refresh can retry");
                    break;
                }
            }
        }
    });
}

fn tray_tooltip(config: &ModelConfig) -> String {
    let mode = match config.mode {
        TranslationMode::Conversational => "日常",
        TranslationMode::Academic => "学习",
    };
    let icon = if config.selection_icon_enabled {
        "开启"
    } else {
        "关闭"
    };
    format!("模式：{mode}\n图标：{icon}")
}

fn tray_tooltip_for_app(app: &tauri::AppHandle, config: &ModelConfig) -> String {
    let tooltip = tray_tooltip(config);
    #[cfg(debug_assertions)]
    if super::tray_menu::is_manual_acceptance(app) {
        return format!("{}\n{tooltip}", super::tray_menu::MANUAL_TEST_LABEL);
    }
    let _ = app;
    tooltip
}

pub(super) fn next_translation_mode(mode: TranslationMode) -> TranslationMode {
    match mode {
        TranslationMode::Conversational => TranslationMode::Academic,
        TranslationMode::Academic => TranslationMode::Conversational,
    }
}

fn translation_mode_feedback(mode: TranslationMode) -> &'static str {
    match mode {
        TranslationMode::Conversational => "日常",
        TranslationMode::Academic => "学习",
    }
}

fn toggle_translation_mode(app: &tauri::AppHandle) -> Result<TranslationMode, String> {
    let config = app.state::<ModelConfigStore>();
    let credentials = app.state::<CredentialStore>();
    let mut next = config.get();
    next.mode = next_translation_mode(next.mode);
    let mode = next.mode;
    config.save(next)?;
    if let Err(error) = sync_model_backend_surfaces(app, config.inner(), credentials.inner()) {
        if let Err(error) = refresh_tray_state(app) {
            warn!(%error, "tray mode state refresh failed");
        }
        warn!(
            application = "Translay",
            capture_method = "tray-style-toggle",
            text_length = 0,
            elapsed_ms = 0,
            error_code = "TRAY_STYLE_SURFACE_SYNC_FAILED",
            %error,
            "translation style changed but dependent surfaces could not be refreshed"
        );
    }
    Ok(mode)
}

pub(super) fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    app.manage(TrayClicks::default());
    let config = app.state::<ModelConfigStore>().get();
    let tooltip = tray_tooltip_for_app(app.handle(), &config);
    let light_taskbar = taskbar_uses_light_theme();
    app.manage(TrayVisuals(Mutex::new(FoldAnimation::new(
        light_taskbar,
        config.selection_icon_enabled,
    ))));
    let icon = tray_icon(light_taskbar, config.selection_icon_enabled);
    TrayIconBuilder::with_id(MAIN_TRAY_ID)
        .tooltip(&tooltip)
        .icon(icon.clone())
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| handle_tray_event(tray.app_handle(), event))
        .build(app)?;
    app.state::<TrayVisuals>()
        .0
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .tooltip = Some(tooltip.clone());
    #[cfg(debug_assertions)]
    super::tray_menu::record_tray_visuals(
        app.handle(),
        light_taskbar,
        config.selection_icon_enabled,
        fold_target(config.selection_icon_enabled),
        &tooltip,
        icon.rgba(),
    );
    // Taskbar color follows the Windows system theme, not the app theme.
    let handle = app.handle().clone();
    std::thread::spawn(move || {
        let mut previous = light_taskbar;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3));
            if handle.state::<ShutdownCoordinator>().is_exiting()
                || handle.tray_by_id(MAIN_TRAY_ID).is_none()
            {
                break;
            }
            let current = taskbar_uses_light_theme();
            if current != previous {
                match refresh_tray_state(&handle) {
                    Ok(()) => previous = current,
                    Err(error) => warn!(%error, "tray theme state refresh failed"),
                }
            }
        }
    });
    Ok(())
}

pub(super) async fn perform_action(app: &tauri::AppHandle, action: &str) -> Result<(), String> {
    if app.state::<ShutdownCoordinator>().is_exiting() {
        return Ok(());
    }
    if action == "settings" {
        return super::windows::show_settings_window(app).await;
    }
    let handle = app.clone();
    let action = action.to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        if handle.state::<ShutdownCoordinator>().is_exiting() {
            return Ok(());
        }
        perform_blocking_action(&handle, &action)
    })
    .await
    .map_err(|error| format!("菜单操作执行失败：{error}"))?
}

fn perform_blocking_action(app: &tauri::AppHandle, action: &str) -> Result<(), String> {
    match action {
        "switch-model-backend" => {
            let config = app.state::<ModelConfigStore>();
            let credentials = app.state::<CredentialStore>();
            let target = alternate_model_backend(config.get().backend);
            switch_model_backend(config.inner(), credentials.inner(), target)?;
            sync_model_backend_surfaces(app, config.inner(), credentials.inner()).map(|_| ())
        }
        "selection-icon" => toggle_selection_icon(app),
        "quit" => {
            app.state::<ShutdownCoordinator>().request_exit(app);
            Ok(())
        }
        _ => Err("未知菜单操作".into()),
    }
}

pub(super) fn tray_action_error(error: &str) {
    warn!(application = "Translay", capture_method = "tray-menu-action", %error, "tray menu action failed");
    let message: Vec<u16> = format!("菜单操作未完成：{error}")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        ::windows::Win32::UI::WindowsAndMessaging::MessageBoxW(
            None,
            ::windows::core::PCWSTR(message.as_ptr()),
            ::windows::core::w!("Translay"),
            ::windows::Win32::UI::WindowsAndMessaging::MB_OK
                | ::windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
        );
    }
}

pub(super) fn tray_icon(light: bool, enabled: bool) -> tauri::image::Image<'static> {
    let bytes: &'static [u8] = match (light, enabled) {
        (true, true) => include_bytes!("../../icons/tray-light-32x32.rgba"),
        (false, true) => include_bytes!("../../icons/tray-dark-32x32.rgba"),
        (true, false) => include_bytes!("../../icons/tray-collapsed-light-32x32.rgba"),
        (false, false) => include_bytes!("../../icons/tray-collapsed-dark-32x32.rgba"),
    };
    tauri::image::Image::new(bytes, 32, 32)
}

pub(super) fn tray_fold_frame(light: bool, frame: usize) -> tauri::image::Image<'static> {
    if frame == 0 {
        return tray_icon(light, true);
    }
    if frame == FOLD_STEPS {
        return tray_icon(light, false);
    }
    // Pose 20 still uses the exact existing collapsed endpoint. The strip skips
    // it and appends poses 21..24 for the brief double-click charge.
    let frames: &'static [u8; (CHARGED_POSE - 1) * 4096] = if light {
        include_bytes!("../../icons/tray-fold-light-32x32.rgba")
    } else {
        include_bytes!("../../icons/tray-fold-dark-32x32.rgba")
    };
    let index = if frame < FOLD_STEPS {
        frame - 1
    } else {
        frame - 2
    };
    tauri::image::Image::new(&frames[index * 4096..(index + 1) * 4096], 32, 32)
}

fn taskbar_uses_light_theme() -> bool {
    use windows::{
        Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
        core::w,
    };
    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    // Read-only preference lookup. Fall back to the dark-taskbar variant.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("SystemUsesLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    };
    result.is_ok() && value != 0
}

#[cfg(test)]
mod tests {
    use super::{
        CHARGED_POSE, FOLD_DURATION, FOLD_STEPS, FoldAnimation, FoldIntent, LeftClicks,
        next_translation_mode, translation_mode_feedback, tray_fold_frame, tray_icon, tray_tooltip,
    };
    use crate::{model_config::ModelConfig, translation::TranslationMode};
    use std::time::{Duration, Instant};

    #[test]
    fn fold_and_unfold_start_promptly_and_ease_out_in_260ms() {
        for enabled in [true, false] {
            let now = Instant::now();
            let mut animation = FoldAnimation::new(true, enabled);
            let target = if enabled { FOLD_STEPS } else { 0 };
            let ticket = animation.retarget(target, now).unwrap();
            for (ms, folded) in [(0, 0), (65, 9), (130, 15), (195, 19), (260, 20)] {
                let expected = if enabled { folded } else { 20 - folded };
                assert_eq!(
                    animation.sample(ticket, now + Duration::from_millis(ms)),
                    Some((expected, ms == 260))
                );
            }
            assert_eq!(
                animation.sample(ticket, now + Duration::from_secs(5)),
                Some((target, true))
            );
            animation.finish_frame(ticket, target, true, now + FOLD_DURATION);
            assert!(animation.sample(ticket, now + FOLD_DURATION).is_none());
            assert!(animation.retarget(target, now + FOLD_DURATION).is_none());
        }
    }

    #[test]
    fn reversal_continues_from_presented_pose_and_rejects_old_callbacks() {
        let now = Instant::now();
        let mut animation = FoldAnimation::new(true, true);
        let old = animation.retarget(20, now).unwrap();
        animation.finish_frame(old, 10, false, now);
        let reverse = animation
            .retarget(0, now + Duration::from_millis(150))
            .unwrap();
        assert_eq!(
            animation.sample(reverse, now + Duration::from_millis(150)),
            Some((10, false))
        );
        assert_eq!(
            animation.sample(reverse, now + Duration::from_millis(215)),
            Some((3, false))
        );
        assert_eq!(
            animation.sample(reverse, now + Duration::from_millis(280)),
            Some((0, true))
        );
        assert!(
            animation
                .sample(old, now + Duration::from_secs(1))
                .is_none()
        );
        animation.finish_frame(old, 20, true, now);
        animation.cancel(old);
        assert_eq!(animation.frame, 10);
        assert!(animation.motion.is_some());
    }

    #[test]
    fn mode_theme_refresh_and_duplicate_target_do_not_restart_animation() {
        let now = Instant::now();
        let mut animation = FoldAnimation::new(true, true);
        let ticket = animation.retarget(20, now).unwrap();
        animation.finish_frame(ticket, 10, false, now);
        animation.light = false;
        assert!(
            animation
                .retarget(20, now + Duration::from_millis(150))
                .is_none()
        );
        assert_eq!(
            animation.sample(ticket, now + FOLD_DURATION),
            Some((20, true))
        );
        assert_eq!(animation.generation, ticket);
    }

    #[test]
    fn failed_frame_can_retry_and_shutdown_invalidates_every_ticket() {
        let now = Instant::now();
        let mut animation = FoldAnimation::new(false, true);
        let old = animation.retarget(20, now).unwrap();
        animation.finish_frame(old, 10, false, now);
        animation.cancel(old);
        assert!(animation.sample(old, now + FOLD_DURATION).is_none());
        let retry = animation.retarget(20, now + FOLD_DURATION).unwrap();
        assert_eq!(
            animation.sample(retry, now + FOLD_DURATION),
            Some((10, false))
        );
        animation.stop();
        assert!(
            animation
                .sample(retry, now + Duration::from_secs(1))
                .is_none()
        );
        assert!(
            animation
                .retarget(0, now + Duration::from_secs(1))
                .is_none()
        );
    }

    #[test]
    fn reversal_before_first_frame_cancels_without_a_new_timer() {
        let now = Instant::now();
        let mut animation = FoldAnimation::new(true, true);
        let old = animation.retarget(20, now).unwrap();
        assert!(animation.retarget(0, now).is_none());
        assert_eq!(animation.frame, 0);
        assert!(animation.sample(old, now + FOLD_DURATION).is_none());
    }

    #[test]
    fn preview_moves_before_confirmation_and_commit_does_not_restart_it() {
        let now = Instant::now();
        let mut animation = FoldAnimation::new(true, true);
        animation.intent = FoldIntent::Preview { click: 1 };
        let ticket = animation.reconcile(true, now).unwrap();
        assert_eq!(
            animation.sample(ticket, now + Duration::from_millis(16)),
            Some((2, false))
        );
        assert!(animation.config_enabled);
        animation.finish_frame(ticket, 2, false, now + Duration::from_millis(16));
        animation.intent = FoldIntent::Config;
        assert!(
            animation
                .reconcile(false, now + Duration::from_millis(16))
                .is_none()
        );
        assert_eq!(animation.generation, ticket);
        assert_eq!(
            animation.sample(ticket, now + FOLD_DURATION),
            Some((20, true))
        );
    }

    #[test]
    fn double_charge_returns_to_the_same_saved_state_in_both_cases() {
        for enabled in [true, false] {
            let now = Instant::now();
            let mut animation = FoldAnimation::new(true, enabled);
            animation.intent = FoldIntent::Preview { click: 1 };
            let preview = animation.reconcile(enabled, now).unwrap();
            animation.finish_frame(preview, 10, false, now + Duration::from_millis(80));
            animation.intent = FoldIntent::Pulse { charging: true };
            let charge = animation
                .reconcile(enabled, now + Duration::from_millis(80))
                .unwrap();
            assert_eq!(
                animation.sample(charge, now + Duration::from_millis(80)),
                Some((10, false))
            );
            assert_eq!(
                animation.sample(charge, now + Duration::from_millis(170)),
                Some((24, true))
            );
            let returning = animation
                .finish_frame(charge, 24, true, now + Duration::from_millis(170))
                .unwrap();
            let endpoint = if enabled { 0 } else { 20 };
            assert_eq!(
                animation.sample(returning, now + Duration::from_secs(1)),
                Some((endpoint, true))
            );
            assert!(
                animation
                    .finish_frame(returning, endpoint, true, now + Duration::from_secs(1))
                    .is_none()
            );
            assert!(animation.intent == FoldIntent::Config);
            assert_eq!(animation.config_enabled, enabled);
            assert!(
                animation
                    .sample(preview, now + Duration::from_secs(1))
                    .is_none()
            );
            assert!(
                animation
                    .sample(charge, now + Duration::from_secs(1))
                    .is_none()
            );
        }
    }

    #[test]
    fn another_double_at_the_charge_peak_continues_release_without_getting_stuck() {
        let now = Instant::now();
        let mut animation = FoldAnimation::new(true, true);
        animation.frame = CHARGED_POSE;
        animation.target = CHARGED_POSE;
        animation.intent = FoldIntent::Pulse { charging: true };
        let ticket = animation.reconcile(true, now).unwrap();
        assert_eq!(
            animation.sample(ticket, now + Duration::from_millis(220)),
            Some((0, true))
        );
    }

    #[test]
    fn stale_preview_cancel_cannot_undo_a_new_click_or_double_pulse() {
        let mut animation = FoldAnimation::new(true, true);
        animation.intent = FoldIntent::Preview { click: 2 };
        assert!(!animation.cancel_preview(Some(1)));
        assert!(animation.intent == FoldIntent::Preview { click: 2 });
        assert!(animation.cancel_preview(Some(2)));
        animation.intent = FoldIntent::Pulse { charging: true };
        assert!(!animation.cancel_preview(None));
    }

    #[test]
    fn right_click_rollback_and_external_preference_supersede_preview() {
        let now = Instant::now();
        let mut animation = FoldAnimation::new(true, true);
        animation.intent = FoldIntent::Preview { click: 1 };
        let preview = animation.reconcile(true, now).unwrap();
        animation.finish_frame(preview, 12, false, now);
        assert!(animation.cancel_preview(None));
        let rollback = animation.reconcile(true, now).unwrap();
        assert_eq!(
            animation.sample(rollback, now + Duration::from_secs(1)),
            Some((0, true))
        );
        assert!(animation.sample(preview, now).is_none());
        animation.intent = FoldIntent::Pulse { charging: true };
        let pulse = animation.reconcile(true, now).unwrap();
        let new_setting = animation.reconcile(false, now).unwrap();
        assert!(animation.intent == FoldIntent::Config);
        assert!(animation.sample(pulse, now).is_none());
        assert_eq!(
            animation.sample(new_setting, now + Duration::from_secs(1)),
            Some((20, true))
        );
    }

    #[test]
    fn fold_frames_are_distinct_transparent_poses_with_exact_original_endpoints() {
        for light in [false, true] {
            assert_eq!(
                tray_fold_frame(light, 0).rgba(),
                tray_icon(light, true).rgba()
            );
            assert_eq!(
                tray_fold_frame(light, 20).rgba(),
                tray_icon(light, false).rgba()
            );
            let mut previous = tray_fold_frame(light, 0);
            for frame in 1..=CHARGED_POSE {
                let icon = tray_fold_frame(light, frame);
                assert_eq!(icon.rgba().len(), 4096);
                assert_ne!(icon.rgba(), previous.rgba());
                assert!(icon.rgba().chunks_exact(4).any(|pixel| pixel[3] == 0));
                assert!(icon.rgba().chunks_exact(4).any(|pixel| pixel[3] == 255));
                previous = icon;
            }
        }
    }

    #[test]
    fn tray_resources_preserve_enabled_logo_and_distinguish_disabled_in_both_themes() {
        for (light, original) in [
            (true, include_bytes!("../../icons/tray-light-32x32.rgba")),
            (false, include_bytes!("../../icons/tray-dark-32x32.rgba")),
        ] {
            assert_eq!(tray_icon(light, true).rgba(), original);
            assert_ne!(
                tray_icon(light, true).rgba(),
                tray_icon(light, false).rgba()
            );
            for enabled in [true, false] {
                let icon = tray_icon(light, enabled);
                assert_eq!(
                    (icon.width(), icon.height(), icon.rgba().len()),
                    (32, 32, 4096)
                );
                let pixels: Vec<_> = icon.rgba().chunks_exact(4).collect();
                assert!(pixels.iter().any(|pixel| pixel[3] == 0));
                assert!(pixels.iter().any(|pixel| pixel[3] == 255));
            }
        }
        assert_ne!(
            tray_icon(true, false).rgba(),
            tray_icon(false, false).rgba()
        );
    }

    #[test]
    fn tooltip_is_two_lines_and_describes_mode_and_icon_state() {
        let mut config = ModelConfig::default();
        for (mode, enabled, expected) in [
            (
                TranslationMode::Conversational,
                true,
                "模式：日常\n图标：开启",
            ),
            (
                TranslationMode::Conversational,
                false,
                "模式：日常\n图标：关闭",
            ),
            (TranslationMode::Academic, true, "模式：学习\n图标：开启"),
            (TranslationMode::Academic, false, "模式：学习\n图标：关闭"),
        ] {
            config.mode = mode;
            config.selection_icon_enabled = enabled;
            let tooltip = tray_tooltip(&config);
            assert_eq!(tooltip, expected);
            assert_eq!(tooltip.lines().count(), 2);
            assert!(tooltip.lines().all(|line| line.chars().count() == 5));
        }
    }

    #[test]
    fn tooltip_size_is_independent_of_model_names() {
        let mut config = ModelConfig::default();
        let expected = tray_tooltip(&config);
        config.local.model = "isolated-local-model-with-a-long-name".repeat(8);
        assert_eq!(tray_tooltip(&config), expected);
        config.backend = crate::model_config::ModelBackend::Api;
        config.api.model = "isolated-api-model-with-a-long-name".repeat(8);
        assert_eq!(tray_tooltip(&config), expected);
    }

    #[test]
    fn double_click_alternates_usage_modes_and_feedback() {
        assert_eq!(
            next_translation_mode(TranslationMode::Conversational),
            TranslationMode::Academic
        );
        assert_eq!(translation_mode_feedback(TranslationMode::Academic), "学习");
        assert_eq!(
            next_translation_mode(TranslationMode::Academic),
            TranslationMode::Conversational
        );
        assert_eq!(
            translation_mode_feedback(TranslationMode::Conversational),
            "日常"
        );
    }

    #[test]
    fn single_release_waits_and_can_be_claimed_only_once() {
        let mut clicks = LeftClicks::default();
        assert!(!clicks.pressed());
        let ticket = clicks.released().unwrap();
        assert!(clicks.expire(ticket));
        assert!(!clicks.expire(ticket));
    }

    #[test]
    fn double_click_cancels_single_and_ignores_its_final_release() {
        let mut clicks = LeftClicks::default();
        let single = clicks.released().unwrap();
        assert!(clicks.double_pressed());
        assert!(!clicks.double_pressed());
        assert!(!clicks.expire(single));
        assert!(clicks.released().is_none());
        assert!(clicks.pending.is_none());
    }

    #[test]
    fn consecutive_double_clicks_each_switch_mode_once() {
        let mut clicks = LeftClicks::default();
        for _ in 0..2 {
            assert!(!clicks.pressed());
            let ticket = clicks.released().unwrap();
            assert!(clicks.double_pressed());
            assert!(clicks.released().is_none());
            assert!(!clicks.expire(ticket));
        }
    }

    #[test]
    fn normal_next_press_commits_the_previous_independent_single() {
        let mut clicks = LeftClicks::default();
        let first = clicks.released().unwrap();
        assert!(clicks.pressed());
        assert!(!clicks.expire(first));
        let second = clicks.released().unwrap();
        assert!(clicks.expire(second));
    }

    #[test]
    fn double_then_third_click_allows_an_independent_single() {
        let mut clicks = LeftClicks::default();
        clicks.released();
        assert!(clicks.double_pressed());
        assert!(clicks.released().is_none());
        assert!(!clicks.pressed());
        let third = clicks.released().unwrap();
        assert!(clicks.expire(third));
    }

    #[test]
    fn stale_timer_cannot_claim_a_new_single() {
        let mut clicks = LeftClicks::default();
        let old = clicks.released().unwrap();
        clicks.cancel();
        let current = clicks.released().unwrap();
        assert!(!clicks.expire(old));
        assert!(clicks.expire(current));
    }

    #[test]
    fn normal_press_recovers_a_double_release_lost_off_icon() {
        let mut clicks = LeftClicks::default();
        assert!(clicks.double_pressed());
        assert!(!clicks.pressed());
        let ticket = clicks.released().unwrap();
        assert!(clicks.expire(ticket));
    }

    #[test]
    fn exit_discards_pending_single_and_rejects_further_clicks() {
        let mut clicks = LeftClicks::default();
        let ticket = clicks.released().unwrap();
        clicks.stop();
        assert!(!clicks.expire(ticket));
        assert!(!clicks.pressed());
        assert!(clicks.released().is_none());
        assert!(!clicks.double_pressed());
    }
}
