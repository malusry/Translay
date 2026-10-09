//! Debug-only native acceptance. No model requests or user configuration access.
use super::super::windows::{self, SettingsWindow};
use super::*;
use crate::{
    model_config::{ApiKeyStatus, ModelBackend, ModelConfig},
    selection_button::SelectionButtonManager,
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[derive(Default)]
struct NativeProbe {
    dom: Mutex<Option<String>>,
    tray: Mutex<Option<AppliedTrayVisuals>>,
    frames: Mutex<Vec<(usize, bool, bool)>>,
    fail_frame: AtomicBool,
}

#[derive(Clone)]
struct AppliedTrayVisuals {
    light: bool,
    enabled: bool,
    frame: usize,
    tooltip: String,
    rgba: Vec<u8>,
    updates: u64,
}

// Only the isolated fixture owns this probe. Production has no acceptance probe.
pub(crate) fn before_tray_frame(app: &tauri::AppHandle) -> Result<(), String> {
    if app
        .try_state::<NativeProbe>()
        .is_some_and(|probe| probe.fail_frame.swap(false, Ordering::AcqRel))
    {
        return Err("injected isolated tray frame failure".into());
    }
    Ok(())
}

pub(crate) fn record_tray_visuals(
    app: &tauri::AppHandle,
    light: bool,
    enabled: bool,
    frame: usize,
    tooltip: &str,
    rgba: &[u8],
) {
    let Some(probe) = app.try_state::<NativeProbe>() else {
        return;
    };
    let mut applied = probe.tray.lock().unwrap();
    let updates = applied.as_ref().map_or(1, |previous| previous.updates + 1);
    *applied = Some(AppliedTrayVisuals {
        light,
        enabled,
        frame,
        tooltip: tooltip.to_owned(),
        rgba: rgba.to_vec(),
        updates,
    });
    probe.frames.lock().unwrap().push((frame, light, enabled));
}

fn assert_tray_visuals(app: &tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let mode = match app.state::<ModelConfigStore>().get().mode {
        crate::translation::TranslationMode::Conversational => "日常",
        crate::translation::TranslationMode::Academic => "学习",
    };
    let expected = format!(
        "模式：{mode}\n图标：{}",
        if enabled { "开启" } else { "关闭" }
    );
    wait(
        "native tray icon and tooltip agree with current state",
        || {
            app.state::<NativeProbe>()
                .tray
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|applied| {
                    applied.enabled == enabled
                        && applied.frame == if enabled { 0 } else { 20 }
                        && applied.tooltip == expected
                        && applied.rgba
                            == super::super::tray::tray_icon(applied.light, enabled).rgba()
                        && !super::super::tray::fold_snapshot(app).2
                })
        },
    )
}

fn tray_visual_updates(app: &tauri::AppHandle) -> u64 {
    app.state::<NativeProbe>()
        .tray
        .lock()
        .unwrap()
        .as_ref()
        .map_or(0, |applied| applied.updates)
}

fn wait_for_fold_pose(app: &tauri::AppHandle) -> Result<(), String> {
    wait("native folding intermediate presented", || {
        let (frame, _, running) = super::super::tray::fold_snapshot(app);
        running && (1..20).contains(&frame)
    })
}

fn exercise_fold_animation(app: &tauri::AppHandle) -> Result<(), String> {
    use super::super::tray;
    assert_tray_visuals(app, true)?;
    // Complete both directions through actual native setters, rather than just
    // inspecting the resource strip or the final preference.
    for enabled in [false, true] {
        let first = app.state::<NativeProbe>().frames.lock().unwrap().len();
        tauri::async_runtime::block_on(tray::perform_action(app, "selection-icon"))?;
        assert_tray_visuals(app, enabled)?;
        let probe = app.state::<NativeProbe>();
        let frames = probe.frames.lock().unwrap();
        let poses = &frames[first..];
        if !poses.iter().any(|(frame, _, _)| (1..20).contains(frame))
            || poses.iter().any(|(_, _, state)| *state != enabled)
            || !poses.windows(2).all(|pair| {
                if enabled {
                    pair[0].0 >= pair[1].0
                } else {
                    pair[0].0 <= pair[1].0
                }
            })
        {
            return Err(format!(
                "native fold/unfold missed monotonic intermediate poses: {poses:?}"
            ));
        }
    }
    let original_mode = app.state::<ModelConfigStore>().get().mode;
    tauri::async_runtime::block_on(tray::perform_action(app, "selection-icon"))?;
    wait_for_fold_pose(app)?;
    let handle = app.clone();
    tauri::async_runtime::block_on(windows::on_window_thread(app, move || {
        let before = tray::fold_snapshot(&handle);
        tray::handle_tray_event(
            &handle,
            tauri::tray::TrayIconEvent::DoubleClick {
                id: "main-tray".into(),
                position: PhysicalPosition::new(0.0, 0.0),
                rect: tauri::Rect {
                    position: tauri::Position::Physical(PhysicalPosition::new(0, 0)),
                    size: tauri::Size::Physical(PhysicalSize::new(24, 24)),
                },
                button: tauri::tray::MouseButton::Left,
            },
        );
        let charging = tray::fold_snapshot(&handle);
        if charging.0 != before.0 || charging.1 == before.1 || !charging.2 {
            return Err("double-click did not take over from the presented pose".into());
        }
        for light in [false, true] {
            tray::apply_tray_state_on_window_thread(&handle, light)?;
            let probe = handle.state::<NativeProbe>();
            let applied = probe.tray.lock().unwrap();
            let applied = applied.as_ref().unwrap();
            if tray::fold_snapshot(&handle) != charging
                || applied.light != light
                || applied.rgba != tray::tray_fold_frame(light, before.0).rgba()
            {
                return Err("theme refresh changed the in-flight pose or generation".into());
            }
        }
        // Reverse in the same window callback: no intermediate timer can run
        // between the before/after observations.
        handle
            .state::<ModelConfigStore>()
            .set_selection_icon_enabled(true)?;
        handle
            .state::<SelectionButtonManager>()
            .initialize_enabled(true);
        tray::apply_tray_state_on_window_thread(&handle, true)?;
        let after = tray::fold_snapshot(&handle);
        if after.0 != before.0 || after.1 == before.1 || !after.2 {
            return Err("reversal did not continue from the presented pose".into());
        }
        Ok(())
    }))?;
    drive_tray_input(app, &[TrayInput::Up])?;
    assert_tray_visuals(app, true)?;
    if app.state::<ModelConfigStore>().get().mode != tray::next_translation_mode(original_mode) {
        return Err("in-flight double click failed to change mode".into());
    }
    drive_tray_input(
        app,
        &[
            TrayInput::Down,
            TrayInput::Up,
            TrayInput::Double,
            TrayInput::Up,
        ],
    )?;
    assert_tray_visuals(app, true)?;
    // Inject a local failure before a native setter. No system theme, real
    // credential, or window is damaged to exercise the normal abort/retry path.
    tauri::async_runtime::block_on(tray::perform_action(app, "selection-icon"))?;
    wait_for_fold_pose(app)?;
    app.state::<NativeProbe>()
        .fail_frame
        .store(true, Ordering::Release);
    wait("injected frame failure releases animation", || {
        !tray::fold_snapshot(app).2
    })?;
    if app
        .state::<NativeProbe>()
        .fail_frame
        .load(Ordering::Acquire)
        || !(1..20).contains(&tray::fold_snapshot(app).0)
    {
        return Err("frame failure injection was not exercised mid-animation".into());
    }
    tray::refresh_tray_state(app)?;
    assert_tray_visuals(app, false)?;
    tauri::async_runtime::block_on(tray::perform_action(app, "selection-icon"))?;
    assert_tray_visuals(app, true)?;
    windows::hide_tray_feedback_window(app)?;
    Ok(())
}

fn exercise_queued_tray_refresh(app: &tauri::AppHandle) -> Result<(), String> {
    let (entered, started) = mpsc::channel();
    let (release, paused) = mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = entered.send(());
        let _ = paused.recv_timeout(Duration::from_secs(10));
    })
    .map_err(|e| e.to_string())?;
    started
        .recv_timeout(Duration::from_secs(10))
        .map_err(|e| e.to_string())?;
    let handle = app.clone();
    let (applied, result) = mpsc::channel();
    let queued = app.run_on_main_thread(move || {
        let _ = applied.send(super::super::tray::apply_tray_state_on_window_thread(
            &handle, true,
        ));
    });
    if let Err(error) = queued {
        let _ = release.send(());
        return Err(error.to_string());
    }
    // The queued theme callback must see this newer state when it executes.
    let saved = app
        .state::<ModelConfigStore>()
        .set_selection_icon_enabled(false);
    app.state::<SelectionButtonManager>()
        .initialize_enabled(false);
    let _ = release.send(());
    saved?;
    result
        .recv_timeout(Duration::from_secs(10))
        .map_err(|e| e.to_string())??;
    assert_tray_visuals(app, false)?;
    app.state::<ModelConfigStore>()
        .set_selection_icon_enabled(true)?;
    app.state::<SelectionButtonManager>()
        .initialize_enabled(true);
    super::super::tray::refresh_tray_state(app)?;
    assert_tray_visuals(app, true)
}

#[derive(Default)]
struct ExitFence {
    hold: AtomicBool,
    observed: AtomicBool,
}

pub(crate) struct NativeAcceptanceData(pub std::path::PathBuf);

const MANUAL_REPORT_MARKER: &str = "translay-tray-manual-v1";
pub(crate) const MANUAL_TEST_LABEL: &str = "Translay · 手动测试（临时数据）";

#[derive(Clone)]
struct ManualSession {
    report: std::path::PathBuf,
    automated: bool,
    started: Instant,
    events: Arc<Mutex<Vec<serde_json::Value>>>,
    completed: Arc<AtomicBool>,
    checks_passed: Arc<AtomicBool>,
}

impl ManualSession {
    fn new(root: &std::path::Path, automated: bool) -> Self {
        Self {
            report: root.join(if automated {
                "tray-menu-manual-check.json"
            } else {
                "tray-menu-manual.json"
            }),
            automated,
            started: Instant::now(),
            events: Default::default(),
            completed: Default::default(),
            checks_passed: Default::default(),
        }
    }

    fn write_report(&self, status: &str, credential_deleted: bool) -> Result<(), String> {
        let events = self
            .events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let report = serde_json::json!({
            "marker": MANUAL_REPORT_MARKER,
            "processId": std::process::id(),
            "status": status,
            "automationOnly": self.automated,
            "automaticFixtureChecksPassed": self.checks_passed.load(Ordering::Acquire),
            "humanVerification": "pending",
            "hiddenTrayPanel": "not_verified",
            "multiMonitorVisual": "not_verified",
            "temporaryDataOnly": true,
            "modelRequestsEnabled": false,
            "credentialDeletionVerified": credential_deleted,
            "events": events,
        });
        std::fs::write(
            &self.report,
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("写入手动验收报告失败：{e}"))
    }

    fn finish(&self, endpoint: &str, status: &str) -> Result<(), String> {
        if self.completed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let result = (|| {
            CredentialStore.clear_api_key(endpoint)?;
            if CredentialStore.api_key_hint(endpoint)?.is_some() {
                return Err("synthetic credential deletion could not be verified".into());
            }
            self.write_report(status, true)
        })();
        if result.is_err() {
            self.completed.store(false, Ordering::Release);
        }
        result
    }
}

pub(crate) fn is_manual_acceptance(app: &tauri::AppHandle) -> bool {
    app.try_state::<ManualSession>().is_some()
}

pub(crate) fn record_manual_event(app: &tauri::AppHandle, kind: &str) {
    if let Some(session) = app.try_state::<ManualSession>() {
        let mut events = session.events.lock().unwrap_or_else(|e| e.into_inner());
        if events.len() < 256 {
            events.push(serde_json::json!({"kind": kind, "elapsedMs": session.started.elapsed().as_millis()}));
        }
    }
}

// The runner can clean only this synthetic process-specific origin after an
// unexpected test exit. This command never starts Tauri or touches user scopes.
pub(crate) fn cleanup_native_credential(process_id: u32) -> Result<(), String> {
    if process_id == 0 {
        return Err("invalid test process id".into());
    }
    let endpoint = format!("https://translay-smoke-{process_id}.invalid/v1");
    CredentialStore.clear_api_key(&endpoint)?;
    if CredentialStore.api_key_hint(&endpoint)?.is_some() {
        return Err("synthetic credential deletion could not be verified".into());
    }
    Ok(())
}

#[tauri::command]
fn get_model_api_key_status(
    base_url: String,
    config: tauri::State<'_, ModelConfigStore>,
    credentials: tauri::State<'_, CredentialStore>,
) -> Result<ApiKeyStatus, String> {
    // Changing a provider in the manual form must not query a real user key.
    let api_key_hint = if base_url.trim() == config.get().api.base_url {
        credentials.api_key_hint(&base_url)?
    } else {
        None
    };
    Ok(ApiKeyStatus {
        has_api_key: api_key_hint.is_some(),
        api_key_hint,
    })
}

#[tauri::command]
fn tray_menu_smoke_probe(
    window: WebviewWindow,
    app: tauri::AppHandle,
    value: String,
) -> Result<(), String> {
    if !matches!(window.label(), "settings" | "tray-menu" | "tray-feedback") {
        return Err("invalid probe caller".into());
    }
    *app.state::<NativeProbe>().dom.lock().unwrap() = Some(value);
    Ok(())
}

fn probe_settings(app: &tauri::AppHandle, script: &str) -> Result<String, String> {
    probe_window(app, "settings", script)
}

fn probe_window(app: &tauri::AppHandle, label: &str, script: &str) -> Result<String, String> {
    *app.state::<NativeProbe>().dom.lock().unwrap() = None;
    app.get_webview_window(label).ok_or("missing probe window")?.eval(format!(
        "(async()=>{{ {script} }})().then(value=>window.__TAURI_INTERNALS__.invoke('tray_menu_smoke_probe',{{value:String(value)}})).catch(error=>window.__TAURI_INTERNALS__.invoke('tray_menu_smoke_probe',{{value:'ERROR: '+String(error)}}))"
    )).map_err(|e| e.to_string())?;
    wait("window DOM probe", || {
        app.state::<NativeProbe>().dom.lock().unwrap().is_some()
    })?;
    let value = app
        .state::<NativeProbe>()
        .dom
        .lock()
        .unwrap()
        .take()
        .unwrap();
    if value.starts_with("ERROR:") {
        return Err(value);
    }
    Ok(value)
}

fn exercise_ui_fonts(app: &tauri::AppHandle) -> Result<(), String> {
    use crate::translation::TranslationMode;
    const FONT_PROBE: &str = r#"
        async function font(selector,family) {
            const element=document.querySelector(selector);
            if(!element) throw new Error('Missing '+selector);
            const loaded=await document.fonts.load('400 13px "'+family+'"',element.textContent);
            const css=getComputedStyle(element);
            if(!loaded.length || loaded.some(face=>face.status!=='loaded') || !css.fontFamily.includes(family)) throw new Error('Font not ready: '+selector);
            return {selector,family,loaded:loaded.length,fontSize:css.fontSize};
        }
    "#;
    tauri::async_runtime::block_on(windows::show_settings_window(app))?;
    let settings = probe_settings(
        app,
        &format!(
            r#"{FONT_PROBE}
        document.querySelector('.translation-nav').click();
        await new Promise(resolve=>requestAnimationFrame(resolve));
        const fonts=await Promise.all([
            font('.settings-header h1','TranslayDisplay'),font('.daily-mode strong','TranslayDisplay'),
            font('.study-mode strong','TranslayReading'),font('.study-mode small','TranslayReading'),
            font('#translation-mode-label','TranslayInterface')]);
        const notes=[...document.querySelectorAll('.style-options small')].map(element=>{{
            const bounds=element.getBoundingClientRect(),card=element.closest('button').getBoundingClientRect();
            if(bounds.right>card.right+.2 || bounds.bottom>card.bottom+.2) throw new Error('Mode note escaped its card');
            return {{width:bounds.width,height:bounds.height}};
        }});
        await new Promise(resolve=>requestAnimationFrame(resolve));
        if(!CSS.supports('transform-box','view-box') || !CSS.supports('color','color-mix(in srgb, red 17%, transparent)') || !CSS.supports('mask-image','radial-gradient(circle closest-side, #000 86%, transparent 100%)')) throw new Error('Native mode wash CSS is unsupported');
        const modeChoices=[...document.querySelectorAll('.style-options > button')].map(button=>{{
            const marker=button.querySelector('.choice-indicator').getBoundingClientRect();
            const dot=button.querySelector('.choice-dot').getBoundingClientRect();
            const heading=button.querySelector('.mode-choice-heading').getBoundingClientRect();
            const title=button.querySelector('strong').getBoundingClientRect();
            const description=button.querySelector('small').getBoundingClientRect();
            const wash=button.querySelector('.mode-choice-wash').getBoundingClientRect();
            const center=rect=>[rect.left+rect.width/2,rect.top+rect.height/2];
            const [mx,my]=center(marker),[dx,dy]=center(dot),[wx,wy]=center(wash);
            const dotOffset=[dx-mx,dy-my],washCenterError=Math.hypot(wx-mx,wy-my);
            const titleCenterOffset=title.top+title.height/2-my,descriptionLeftOffset=description.left-title.left,descriptionGap=description.top-heading.bottom;
            const cardStyle=getComputedStyle(button),washStyle=getComputedStyle(button.querySelector('.mode-choice-wash'));
            if(dotOffset.some(offset=>Math.abs(offset)>.03) || washCenterError>.08) throw new Error('Native mode indicator alignment changed');
            if(Math.abs(titleCenterOffset)>.03 || Math.abs(descriptionLeftOffset)>.03 || Math.abs(descriptionGap-5)>.03) throw new Error('Native mode heading/description alignment changed');
            if(Math.abs(wash.width-wash.height)>.03 || cardStyle.borderRadius!=='12px' || cardStyle.overflow!=='hidden' || cardStyle.transform!=='none') throw new Error('Native mode wash shape changed');
            const reduced=matchMedia('(prefers-reduced-motion: reduce)').matches;
            if(washStyle.transitionDuration!==(reduced?'0s':'0.3s, 0.3s')) throw new Error('Native mode wash duration changed');
            return {{className:button.className,dotOffset,titleCenterOffset,descriptionLeftOffset,descriptionGap,titleTop:title.top,washCenterError,circleSize:[wash.width,wash.height],duration:washStyle.transitionDuration,reducedMotion:reduced}};
        }});
        if(Math.abs(modeChoices[0].titleTop-modeChoices[1].titleTop)>.03) throw new Error('Native mode headings are not level');
        document.querySelector('.model-nav').click();
        await new Promise(resolve=>requestAnimationFrame(resolve));
        const saveButton=document.querySelector('.provider-save-button'),saveLabel=saveButton.querySelector('.save-button-label');
        const original={{className:saveButton.className,label:saveLabel.textContent}};
        const saveStates=[];
        let savedFont;
        try {{
            // Project CSS states only; no form submit or synthetic configuration write.
            for(const [state,text] of [['idle','保存'],['dirty','保存'],['saving','保存中'],['confirmed','已保存']]) {{
                saveButton.className='provider-save-button '+state;saveLabel.textContent=text;
                await new Promise(resolve=>requestAnimationFrame(resolve));
                await Promise.all(saveButton.getAnimations({{subtree:true}}).filter(animation=>animation instanceof CSSTransition).map(animation=>animation.finished.catch(()=>{{}})));
                if(state==='confirmed') savedFont=await font('.save-button-label','TranslayReading');
                const bounds=saveButton.getBoundingClientRect(),label=saveLabel.getBoundingClientRect();
                const border=saveButton.querySelector('.save-button-border'),rect=border.querySelector('rect'),edge=getComputedStyle(border),outline=getComputedStyle(rect);
                const css=getComputedStyle(saveButton),type=getComputedStyle(saveLabel);
                const borderShape=['x','y','width','height','rx'].map(name=>Number(rect.getAttribute(name)));
                const borderStops=[...border.querySelectorAll('stop')].map(stop=>({{offset:stop.getAttribute('offset'),color:getComputedStyle(stop).stopColor}}));
                if(Math.abs(bounds.width-74)>.03 || Math.abs(bounds.height-28)>.03 || css.borderRadius!=='7px' || type.fontSize!=='12px' || css.opacity!=='1') throw new Error('Native save button size/type changed');
                if(label.left<bounds.left || label.right>bounds.right || label.top<bounds.top || label.bottom>bounds.bottom) throw new Error('Native save label escaped its button');
                if(outline.strokeWidth!=='1px' || outline.fill!=='none' || !outline.stroke.includes('settings-save-border-gradient') || JSON.stringify(borderShape)!=='[0.5,0.5,73,27,6.5]') throw new Error('Native continuous rounded outline changed');
                if(borderStops[0].color!=='rgb(80, 123, 120)' || borderStops[2].color!=='rgb(112, 96, 141)' || borderStops[3].color!==borderStops[2].color || !css.backgroundImage.includes('linear-gradient')) throw new Error('Native sage-purple gradient is missing');
                if(state==='confirmed' && (type.letterSpacing!=='0.36px' || type.color!=='rgb(83, 110, 118)')) throw new Error('Native saved typography changed');
                saveStates.push({{state,size:[bounds.width,bounds.height],fontSize:type.fontSize,fontFamily:type.fontFamily,labelColor:type.color,borderThickness:outline.strokeWidth,borderShape,borderStops,borderOpacity:edge.opacity,fill:css.backgroundImage}});
            }}
        }} finally {{saveButton.className=original.className;saveLabel.textContent=original.label;}}
        const saveButtonVisual={{states:saveStates,savedFont,scope:'Temporary CSS class/text projection in native WebView2; no form submit. Actual React save, duplicate clicks and failure retry are browser mock IPC evidence.'}};
        const testButton=document.querySelector('.provider-test-button'),feedback=document.querySelector('.connection-feedback');
        const testOriginal={{className:testButton.className,disabled:testButton.disabled,text:feedback.textContent,title:feedback.title,classNameFeedback:feedback.className}};
        const connectionStates=[];
        try {{
            for(const [state,text] of [['idle',''],['working','正在连接…'],['success','连接成功 · 326 ms'],['error','服务返回 HTTP 401：虚拟 API Key 已失效；request_id=virtual-request。'+ '原始服务端诊断。'.repeat(60)]]) {{
                testButton.className='provider-test-button '+state;testButton.disabled=state==='working';
                feedback.className='connection-feedback '+state;feedback.textContent=text;feedback.title=text;
                await new Promise(resolve=>requestAnimationFrame(resolve));
                await Promise.all(testButton.getAnimations({{subtree:true}}).filter(animation=>animation instanceof CSSTransition).map(animation=>animation.finished.catch(()=>{{}})));
                const bounds=testButton.getBoundingClientRect(),icon=testButton.querySelector('.connection-status-icon').getBoundingClientRect();
                const border=testButton.querySelector('.connection-button-border'),circle=border.querySelector('circle'),edge=getComputedStyle(border),outline=getComputedStyle(circle);
                const css=getComputedStyle(testButton),message=feedback.getBoundingClientRect(),save=saveButton.getBoundingClientRect();
                const borderShape=['cx','cy','r'].map(name=>Number(circle.getAttribute(name)));
                const borderStops=[...border.querySelectorAll('stop')].map(stop=>getComputedStyle(stop).stopColor);
                if(Math.abs(bounds.width-32)>.03 || Math.abs(bounds.height-32)>.03 || Math.abs(icon.width-16)>.03 || Math.abs(icon.height-16)>.03 || css.borderRadius!=='50%' || css.boxShadow!=='none') throw new Error('Native connection button size/shadow changed');
                if(Math.abs(icon.x+icon.width/2-bounds.x-bounds.width/2)>.03 || Math.abs(icon.y+icon.height/2-bounds.y-bounds.height/2)>.03) throw new Error('Native connection icon is not centered');
                if(outline.strokeWidth!=='1px' || !outline.stroke.includes('settings-connection-border-gradient') || JSON.stringify(borderShape)!=='[16,16,15.5]' || edge.pointerEvents!=='none') throw new Error('Native circular connection outline changed');
                if(borderStops[0]!=='rgb(80, 123, 120)' || borderStops[2]!=='rgb(112, 96, 141)' || borderStops[3]!==borderStops[2]) throw new Error('Native connection sage-purple colors changed');
                if(Math.abs(message.height-16)>.03 || message.width>240.03 || message.right>bounds.left-7.9 || save.left-bounds.right<7.9 || getComputedStyle(feedback).textOverflow!=='ellipsis') throw new Error('Native connection feedback squeezed buttons');
                if(state==='error' && (feedback.scrollWidth<=feedback.clientWidth || feedback.title!==text)) throw new Error('Native long connection diagnostics were lost');
                const position=[bounds.x,bounds.y,save.x,save.y];
                if(connectionStates.length && position.some((value,index)=>Math.abs(value-connectionStates[0].position[index])>.03)) throw new Error('Native connection feedback moved buttons');
                connectionStates.push({{state,size:[bounds.width,bounds.height],iconSize:[icon.width,icon.height],position,borderShape,borderStops,borderThickness:outline.strokeWidth,borderOpacity:edge.opacity,shadow:css.boxShadow,feedbackWidth:message.width,feedbackHeight:message.height,truncated:feedback.scrollWidth>feedback.clientWidth,fullDiagnostic:feedback.title}});
            }}
        }} finally {{testButton.className=testOriginal.className;testButton.disabled=testOriginal.disabled;feedback.className=testOriginal.classNameFeedback;feedback.textContent=testOriginal.text;feedback.title=testOriginal.title;}}
        const connectionButtonVisual={{states:connectionStates,scope:'Temporary CSS class/text projection in native WebView2, retaining the original icon. No connection IPC or model request; actual state icons, request isolation and retries are browser mock IPC evidence.'}};
        return JSON.stringify({{fonts,notes,modeChoices,saveButtonVisual,connectionButtonVisual,width:innerWidth,height:innerHeight,dpr:devicePixelRatio}});
    "#
        ),
    )?;
    windows::hide_settings_window(app)?;
    open(app)?;
    let menu = probe_window(
        app,
        "tray-menu",
        &format!(
            r#"{FONT_PROBE}
        const fonts=await Promise.all([font('.tray-menu-settings span','TranslayDisplay'),font('.tray-menu button:first-child span','TranslayInterface')]);
        const bounds=document.querySelector('.tray-menu').getBoundingClientRect();
        // WebView2 rounds the two 1px borders to physical pixels at fractional DPI.
        if(Math.abs(bounds.width-164)>.2 || Math.abs(bounds.height-136)>1) throw new Error('Menu dimensions changed: '+bounds.width+'x'+bounds.height+' at DPR '+devicePixelRatio);
        return JSON.stringify({{fonts,width:bounds.width,height:bounds.height,dpr:devicePixelRatio}});
    "#
        ),
    )?;
    tauri::async_runtime::block_on(close(app, None))?;
    let mut feedback = Vec::new();
    for (mode, message, family) in [
        (TranslationMode::Conversational, "日常", "TranslayDisplay"),
        (TranslationMode::Academic, "学习", "TranslayReading"),
    ] {
        app.state::<super::super::tray_feedback::TrayFeedbackManager>()
            .show(
                app,
                PhysicalPosition::new(100., 100.),
                Some(tauri::Rect {
                    position: tauri::Position::Physical(PhysicalPosition::new(88, 88)),
                    size: tauri::Size::Physical(PhysicalSize::new(24, 24)),
                }),
                mode,
                message.to_owned(),
            )?;
        let value = probe_window(
            app,
            "tray-feedback",
            &format!(
                r#"{FONT_PROBE}
            while(document.querySelector('.tray-feedback-label')?.textContent!=='{message}' || !document.querySelector('.tray-feedback-stage--visible')) await new Promise(resolve=>requestAnimationFrame(resolve));
            const fonts=await font('.tray-feedback-label','{family}');
            const pill=document.querySelector('.tray-feedback-pill');
            const bounds=pill.getBoundingClientRect();
            // Layout is fixed while the existing enter/leave animation scales the
            // presented rectangle. Do not await an older, already-leaving node.
            if(pill.offsetWidth!==36 || pill.offsetHeight!==24 || innerWidth!==64 || innerHeight!==48) throw new Error('Compact feedback layout changed');
            const labelBounds=document.querySelector('.tray-feedback-label').getBoundingClientRect();
            const sideSpace=[labelBounds.left-bounds.left,bounds.right-labelBounds.right];
            if(sideSpace.some(space=>space<3) || getComputedStyle(pill).fontSize!=='13px') throw new Error('Feedback text is cramped or resized');
            const background=getComputedStyle(pill).backgroundImage;
            const backgroundAlpha=[...background.matchAll(/rgba\([^)]*,\s*([\d.]+)\)/g)].map(match=>Number(match[1]));
            if(backgroundAlpha.length!==2 || backgroundAlpha.some(alpha=>alpha<=0 || alpha>=1) || backgroundAlpha[0]<=backgroundAlpha[1]) throw new Error('Feedback background must have a translucent gradient');
            return JSON.stringify({{fonts,message:'{message}',background,backgroundAlpha,sideSpace,width:pill.offsetWidth,height:pill.offsetHeight,presentedWidth:bounds.width,presentedHeight:bounds.height,phase:document.querySelector('main').className,windowWidth:innerWidth,windowHeight:innerHeight,dpr:devicePixelRatio}});
        "#
            ),
        )?;
        let window = app
            .get_webview_window("tray-feedback")
            .ok_or("feedback missing")?;
        let origin = window.outer_position().map_err(|e| e.to_string())?;
        let size = window.inner_size().map_err(|e| e.to_string())?;
        let scale = window.scale_factor().map_err(|e| e.to_string())?;
        let px = |logical: f64| (logical * scale).round() as i32;
        let visible = [
            origin.x + px(14.),
            origin.y + px(12.),
            origin.x + size.width as i32 - px(14.),
            origin.y + size.height as i32 - px(12.),
        ];
        let gaps = [
            88 - visible[3],
            visible[1] - 112,
            visible[0] - 112,
            88 - visible[2],
        ];
        if !gaps.into_iter().any(|gap| (gap - px(2.)).abs() <= 1) {
            return Err(format!(
                "Native feedback did not retain the visible icon gap: {gaps:?}"
            ));
        }
        let mut evidence =
            serde_json::from_str::<serde_json::Value>(&value).map_err(|e| e.to_string())?;
        evidence["nativePlacement"] = serde_json::json!({
            "source": "synthetic icon rectangle passed through the production feedback path",
            "icon": [88, 88, 112, 112], "windowOrigin": [origin.x, origin.y],
            "windowSize": [size.width, size.height], "visibleRect": visible,
            "directionalGaps": gaps, "expectedGap": px(2.), "scale": scale,
        });
        feedback.push(evidence);
    }
    windows::hide_tray_feedback_window(app)?;
    let evidence = serde_json::json!({
        "source": "isolated native WebView2 windows; local FontFace loads and DOM layout; no physical tray-panel interaction",
        "settings": serde_json::from_str::<serde_json::Value>(&settings).map_err(|e| e.to_string())?,
        "menu": serde_json::from_str::<serde_json::Value>(&menu).map_err(|e| e.to_string())?,
        "feedback": feedback,
    });
    std::fs::write(
        "tests/artifacts/font-design-native-fonts.json",
        serde_json::to_vec_pretty(&evidence).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn assert_single_settings(app: &tauri::AppHandle, expected: isize) -> Result<(), String> {
    use ::windows::Win32::{
        Foundation::{HWND, LPARAM},
        UI::WindowsAndMessaging::{EnumWindows, GetWindowTextW, GetWindowThreadProcessId},
    };
    use ::windows::core::BOOL;
    unsafe extern "system" fn count(hwnd: HWND, data: LPARAM) -> BOOL {
        let total = unsafe { &mut *(data.0 as *mut usize) };
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
        if pid == std::process::id() {
            let mut title = [0u16; 64];
            let len = unsafe { GetWindowTextW(hwnd, &mut title) };
            if String::from_utf16_lossy(&title[..len as usize]) == "Translay 设置" {
                *total += 1;
            }
        }
        BOOL(1)
    }
    let mut count_windows = 0usize;
    unsafe {
        EnumWindows(
            Some(count),
            LPARAM((&mut count_windows as *mut usize) as isize),
        )
    }
    .map_err(|e| e.to_string())?;
    let settings = app
        .get_webview_window("settings")
        .ok_or("missing settings")?;
    if count_windows != 1 || settings.hwnd().map_err(|e| e.to_string())?.0 as isize != expected {
        return Err(format!(
            "settings window identity/count changed: count={count_windows}"
        ));
    }
    Ok(())
}

fn exercise_settings_creation(app: &tauri::AppHandle) -> Result<isize, String> {
    let lifecycle = app.state::<SettingsWindow>();
    lifecycle.before_create(Box::new(|| Err("synthetic create failure".into())));
    let result = tauri::async_runtime::block_on(windows::show_settings_window(app));
    if result.as_ref().err().map(String::as_str) != Some("synthetic create failure")
        || lifecycle.is_opening()
        || app.get_webview_window("settings").is_some()
    {
        return Err("create failure left an unretryable state".into());
    }
    let (entered, worker) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    lifecycle.before_create(Box::new(move || {
        entered
            .send(std::thread::current().id())
            .map_err(|e| e.to_string())?;
        blocked
            .recv_timeout(Duration::from_secs(10))
            .map_err(|e| e.to_string())
    }));
    let handle = app.clone();
    let main_thread = tauri::async_runtime::block_on(windows::on_window_thread(app, move || {
        // Exercise the same dispatch used by startup's synchronous callback.
        windows::request_settings_window(&handle);
        Ok(std::thread::current().id())
    }))?;
    let creation_thread = worker
        .recv_timeout(Duration::from_secs(10))
        .map_err(|e| e.to_string())?;
    if main_thread == creation_thread {
        return Err("WebView creation stayed on the callback thread".into());
    }
    std::thread::scope(|scope| -> Result<(), String> {
        let mut opens = Vec::new();
        for _ in 0..12 {
            opens.push(
                scope.spawn(|| tauri::async_runtime::block_on(windows::show_settings_window(app))),
            );
        }
        if app.get_webview_window("settings").is_some() {
            return Err("duplicate open bypassed creation reservation".into());
        }
        release.send(()).map_err(|e| e.to_string())?;
        for open in opens {
            open.join().map_err(|_| "concurrent open panicked")??;
        }
        Ok(())
    })?;
    wait("single first settings creation and focus", || {
        !lifecycle.is_opening()
            && app
                .get_webview_window("settings")
                .is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false))
    })?;
    let settings = app.get_webview_window("settings").unwrap();
    let identity = settings.hwnd().map_err(|e| e.to_string())?.0 as isize;
    assert_single_settings(app, identity)?;
    // A fresh open issued after hide must survive a cancelled, still-running
    // worker, while the original open remains unable to present late.
    windows::hide_settings_window(app)?;
    let (entered, worker) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    lifecycle.before_create(Box::new(move || {
        entered.send(()).map_err(|e| e.to_string())?;
        blocked
            .recv_timeout(Duration::from_secs(10))
            .map_err(|e| e.to_string())
    }));
    let handle = app.clone();
    let cancelled = std::thread::spawn(move || {
        tauri::async_runtime::block_on(windows::show_settings_window(&handle))
    });
    worker
        .recv_timeout(Duration::from_secs(10))
        .map_err(|e| e.to_string())?;
    windows::hide_settings_window(app)?;
    // Poll once while the previous worker is held, proving that the fresh
    // request has joined its completion before that worker is released.
    use std::future::Future;
    let mut reopened = Box::pin(windows::show_settings_window(app));
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    if reopened.as_mut().poll(&mut context).is_ready() {
        return Err("fresh open did not wait for the cancelled worker".into());
    }
    release.send(()).map_err(|e| e.to_string())?;
    cancelled.join().map_err(|_| "cancelled open panicked")??;
    tauri::async_runtime::block_on(reopened)?;
    if !settings.is_visible().unwrap_or(false) || !settings.is_focused().unwrap_or(false) {
        return Err("fresh open after hide joined a cancelled presentation".into());
    }
    assert_single_settings(app, identity)?;
    settings.close().map_err(|e| e.to_string())?;
    wait(
        "native close request hides without destroying settings",
        || !settings.is_visible().unwrap_or(true),
    )?;
    tauri::async_runtime::block_on(windows::show_settings_window(app))?;
    assert_single_settings(app, identity)?;
    let draft = probe_settings(
        app,
        r#"
        while(document.querySelector('#model-name')?.value !== 'synthetic-local') await new Promise(resolve=>setTimeout(resolve,40));
        const input=document.querySelector('#model-name');
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,'unsaved-native-draft');
        input.dispatchEvent(new Event('input',{bubbles:true}));
        await new Promise(resolve=>requestAnimationFrame(resolve));
        return input.value;
    "#,
    )?;
    if draft != "unsaved-native-draft" {
        return Err("draft input was not applied".into());
    }
    probe_settings(
        app,
        "document.querySelector('button[aria-label=关闭]').click(); return 'closed';",
    )?;
    wait("settings close button hides", || {
        !settings.is_visible().unwrap_or(true)
    })?;
    lifecycle.before_present(Box::new(|| Err("synthetic display failure".into())));
    let result = tauri::async_runtime::block_on(windows::show_settings_window(app));
    if result.as_ref().err().map(String::as_str) != Some("synthetic display failure")
        || lifecycle.is_opening()
        || settings.is_visible().unwrap_or(true)
    {
        return Err("display failure left an unretryable state".into());
    }
    tauri::async_runtime::block_on(windows::show_settings_window(app))?;
    assert_single_settings(app, identity)?;
    if probe_settings(app, "return document.querySelector('#model-name').value;")?
        != "unsaved-native-draft"
    {
        return Err("hidden/reopened settings lost the unsaved draft".into());
    }
    // Restore only the synthetic in-memory draft; never save the form.
    probe_settings(
        app,
        r#"
        const input=document.querySelector('#model-name');
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,'synthetic-local');
        input.dispatchEvent(new Event('input',{bubbles:true})); return 'restored';
    "#,
    )?;
    windows::hide_settings_window(app)?;
    Ok(identity)
}

fn exercise_late_exit(
    app: &tauri::AppHandle,
    menu: &WebviewWindow,
    endpoint: &str,
) -> Result<(), String> {
    // Quit while frames are still scheduled. Final cleanup must invalidate the
    // animation as well as the pending single click.
    let clicks_config = app.state::<ModelConfigStore>().get();
    assert_tray_visuals(app, clicks_config.selection_icon_enabled)?;
    drive_tray_input(app, &[TrayInput::Down, TrayInput::Up])?;
    wait_for_fold_pose(app)?;
    let generation = app.state::<TrayMenu>().snapshot().generation;
    let (entered, worker) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    app.state::<SettingsWindow>()
        .before_create(Box::new(move || {
            entered.send(()).map_err(|e| e.to_string())?;
            blocked
                .recv_timeout(Duration::from_secs(10))
                .map_err(|e| e.to_string())
        }));
    let handle = app.clone();
    let late_open = std::thread::spawn(move || {
        tauri::async_runtime::block_on(windows::show_settings_window(&handle))
    });
    worker
        .recv_timeout(Duration::from_secs(10))
        .map_err(|e| e.to_string())?;
    app.state::<ExitFence>().hold.store(true, Ordering::Release);
    click(menu, 3)?;
    wait("actual quit reached exit event", || {
        app.state::<ExitFence>().observed.load(Ordering::Acquire)
    })?;
    let visual_updates = tray_visual_updates(app);
    let (stopped_pose, _, still_running) = super::super::tray::fold_snapshot(app);
    if still_running || !(1..20).contains(&stopped_pose) {
        return Err("quit did not cancel an in-flight fold".into());
    }
    settle_tray_timers(app)?;
    drive_tray_input(app, &[TrayInput::Double, TrayInput::Up])?;
    super::super::tray::refresh_tray_state(app)?;
    let handle = app.clone();
    tauri::async_runtime::block_on(windows::on_window_thread(app, move || {
        super::super::tray::apply_tray_state_on_window_thread(&handle, true)
    }))?;
    if tray_visual_updates(app) != visual_updates {
        return Err("late tray refresh changed icon or tooltip during exit".into());
    }
    let after_exit = app.state::<ModelConfigStore>().get();
    if after_exit.mode != clicks_config.mode
        || after_exit.selection_icon_enabled != clicks_config.selection_icon_enabled
    {
        return Err("late tray input changed configuration during exit".into());
    }
    release.send(()).map_err(|e| e.to_string())?;
    late_open.join().map_err(|_| "late open panicked")??;
    tauri::async_runtime::block_on(windows::show_settings_window(app))?;
    tauri::async_runtime::block_on(tray_menu_painted(app.clone(), menu.clone(), generation))?;
    tauri::async_runtime::block_on(tray_menu_action(
        app.clone(),
        menu.clone(),
        generation,
        "settings".into(),
    ))?;
    tauri::async_runtime::block_on(show(
        app,
        PhysicalPosition::new(0.0, 0.0),
        tauri::Rect {
            position: tauri::Position::Physical(PhysicalPosition::new(0, 0)),
            size: tauri::Size::Physical(PhysicalSize::new(24, 24)),
        },
    ))?;
    if app.state::<SettingsWindow>().is_opening()
        || app.state::<TrayMenu>().accepts_open()
        || app
            .webview_windows()
            .values()
            .any(|w| w.is_visible().unwrap_or(true))
    {
        return Err("late operation displayed a window during exit".into());
    }
    app.state::<CredentialStore>().clear_api_key(endpoint)?;
    if app
        .state::<CredentialStore>()
        .api_key_hint(endpoint)?
        .is_some()
    {
        return Err("synthetic credential cleanup could not be verified".into());
    }
    let frames = app.state::<NativeProbe>().frames.lock().unwrap().clone();
    let evidence = serde_json::json!({
        "source": "successful native setters on the real window thread; no Windows Shell readback",
        "foldSteps": 20,
        "durationMs": 260,
        "chargedPose": 24,
        "quitCancelledAtPose": stopped_pose,
        "frames": frames.iter().map(|(pose, light, enabled)| serde_json::json!({"pose":pose,"lightTaskbar":light,"selectionEnabled":enabled})).collect::<Vec<_>>(),
    });
    std::fs::write(
        "tests/artifacts/tray-response-frames.json",
        serde_json::to_vec_pretty(&evidence).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::write("tests/artifacts/tray-menu-native.txt", concat!(
        "PASS: create/display injected failures release reservation and retry; synchronous callback dispatch creates on a different worker thread; concurrent first-open x12 coalesced; fresh open after hide waits for cancelled worker then presents; one native settings HWND; unsaved model draft survives close/reopen; ",
        "settings menu open/restore/focus x6 with same HWND; menu action joins an existing settings open and rejects reopening until focus completes; native close/close-button/Escape hide; selection persistence x4; model switch persistence x4; missing-key disabled; double DOM click for all actions; Escape/padding x3; stale action/close/paint isolation; native focus-loss and reopen; ",
        "single/double tray gestures: singles toggle only selection preference and persist; double pairs toggle only mode, cancel their singles and suppress the trailing release; right click cancels a pending single and opens the menu; pending single cancelled on quit; ",
        "selection startup rule: saved-off isolated config starts enabled without rewriting the file; selection manager and initial native tray icon/tooltip agree; explicitly disable before existing toggle checks; ",
        "tray visuals: enabled startup and explicit disabled fixture; original/collapsed icon and two-line tooltip follow single/menu toggles; both theme variants while disabled; late queued refresh reads current state; native setters succeeded; late exit refresh suppressed; ",
        "tray fold animation: native intermediate poses in both directions; theme retains pose and generation; double click takes over current pose; reversal continues current pose; injected frame failure and explicit retry; pending frames cancelled on quit; ",
        "tray immediate response: preview before preference commit; no unchanged first-pose resubmission; charge and return in both saved states; consecutive independent singles; redundant refresh skipped; ",
            "local UI fonts: three font faces loaded in native WebView2; settings notes contained; menu remains 164x136; two-character feedback remains 36x24 in 64x48 with clear side spacing; native feedback position retains the shared visible icon gap; ",
            "settings mode wash: native WebView2 CSS support, circular wash origin, rounded clipping and duration verified; animated progression and save recovery covered by browser mock IPC; ",
            "settings mode alignment: concentric SVG ring/dot, centered mode headings and descriptions aligned below titles verified; ",
            "settings save styling: native CSS state projection, compact 74x28 continuous sage-purple outline and saved reading font verified; actual save recovery covered by browser mock IPC; ",
            "settings connection styling: native CSS state projection, circular 32x32 sage-purple outline, centered 16px icon and stable long feedback verified; actual request isolation covered by browser mock IPC; ",
        "actual quit plus blocked late settings open, late menu paint/action/show and late tray input all suppressed (debug harness holds the exit event for assertions). Runner must verify exit=0.\n",
        "BOUNDARY: synthetic Tauri tray events on the real window thread, programmatic tray-menu opening and DOM clicks; Windows hidden tray panel, physical double-click and visual multi-monitor placement NOT exercised. No model requests or user configuration access.\n",
        "CLEANUP: synthetic credential deletion verified.\n"
    )).map_err(|e| e.to_string())?;
    app.state::<ExitFence>()
        .hold
        .store(false, Ordering::Release);
    app.exit(0);
    Ok(())
}

fn wait(label: &str, mut ready: impl FnMut() -> bool) -> Result<(), String> {
    let until = Instant::now() + Duration::from_secs(10);
    while !ready() {
        if Instant::now() >= until {
            return Err(format!("timed out: {label}"));
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    Ok(())
}

fn open(app: &tauri::AppHandle) -> Result<WebviewWindow, String> {
    let menu = app.get_webview_window("tray-menu").ok_or("missing menu")?;
    let monitor = menu
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("missing monitor")?;
    let pos = monitor.position();
    let size = monitor.size();
    let point = PhysicalPosition::new(
        (pos.x + size.width as i32 / 2) as f64,
        (pos.y + size.height as i32 - 28) as f64,
    );
    let rect = tauri::Rect {
        position: tauri::Position::Physical(PhysicalPosition::new(
            point.x as i32 - 12,
            point.y as i32 - 12,
        )),
        size: tauri::Size::Physical(PhysicalSize::new(24, 24)),
    };
    wait("previous menu action settled", || {
        app.state::<TrayMenu>().accepts_open()
    })?;
    tauri::async_runtime::block_on(show(app, point, rect))?;
    wait("menu visible", || menu.is_visible().unwrap_or(false))?;
    Ok(menu)
}

fn click(menu: &WebviewWindow, index: usize) -> Result<(), String> {
    menu.eval(format!(
        "{{ const button=document.querySelectorAll('.tray-menu button')[{index}]; button.click(); button.click(); }}"
    ))
    .map_err(|e| e.to_string())
}

#[derive(Clone, Copy)]
enum TrayInput {
    Down,
    Up,
    Double,
    RightDown,
    RightUp,
}

fn drive_tray_input(app: &tauri::AppHandle, inputs: &[TrayInput]) -> Result<(), String> {
    let app_handle = app.clone();
    let inputs = inputs.to_vec();
    tauri::async_runtime::block_on(windows::on_window_thread(app, move || {
        drive_tray_input_on_window_thread(&app_handle, &inputs);
        Ok(())
    }))
}

fn drive_tray_input_on_window_thread(app: &tauri::AppHandle, inputs: &[TrayInput]) {
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
    for input in inputs {
        let id = "main-tray".into();
        let position = PhysicalPosition::new(0.0, 0.0);
        let rect = tauri::Rect {
            position: tauri::Position::Physical(PhysicalPosition::new(0, 0)),
            size: tauri::Size::Physical(PhysicalSize::new(24, 24)),
        };
        let event = match input {
            TrayInput::Double => TrayIconEvent::DoubleClick {
                id,
                position,
                rect,
                button: MouseButton::Left,
            },
            _ => TrayIconEvent::Click {
                id,
                position,
                rect,
                button: if matches!(input, TrayInput::RightDown | TrayInput::RightUp) {
                    MouseButton::Right
                } else {
                    MouseButton::Left
                },
                button_state: if matches!(input, TrayInput::Down | TrayInput::RightDown) {
                    MouseButtonState::Down
                } else {
                    MouseButtonState::Up
                },
            },
        };
        super::super::tray::handle_tray_event(app, event);
    }
}

fn exercise_immediate_response(app: &tauri::AppHandle) -> Result<(), String> {
    use super::super::tray;
    for enabled in [true, false] {
        if app.state::<ModelConfigStore>().get().selection_icon_enabled != enabled {
            tauri::async_runtime::block_on(tray::perform_action(app, "selection-icon"))?;
        }
        tray::refresh_tray_state(app)?;
        assert_tray_visuals(app, enabled)?;
        let first = app.state::<NativeProbe>().frames.lock().unwrap().len();
        let mode = app.state::<ModelConfigStore>().get().mode;
        let handle = app.clone();
        tauri::async_runtime::block_on(windows::on_window_thread(app, move || {
            let updates = tray_visual_updates(&handle);
            drive_tray_input_on_window_thread(&handle, &[TrayInput::Down, TrayInput::Up]);
            if !tray::fold_snapshot(&handle).2
                || tray_visual_updates(&handle) != updates
                || handle
                    .state::<ModelConfigStore>()
                    .get()
                    .selection_icon_enabled
                    != enabled
            {
                return Err(
                    "first release waited for confirmation or resubmitted unchanged pose".into(),
                );
            }
            Ok(())
        }))?;
        wait_for_fold_pose(app)?;
        if app.state::<ModelConfigStore>().get().selection_icon_enabled != enabled
            || app.state::<SelectionButtonManager>().is_enabled() != enabled
        {
            return Err(
                "visual preview changed the real selection flag before confirmation".into(),
            );
        }
        drive_tray_input(app, &[TrayInput::Double, TrayInput::Up])?;
        assert_tray_visuals(app, enabled)?;
        if app.state::<ModelConfigStore>().get().mode != tray::next_translation_mode(mode) {
            return Err("previewed double click failed to switch mode exactly once".into());
        }
        let probe = app.state::<NativeProbe>();
        if !probe.frames.lock().unwrap()[first..]
            .iter()
            .any(|(pose, _, _)| *pose > 20)
        {
            return Err("double click did not present its tighter charge pose".into());
        }
        let updates = tray_visual_updates(app);
        tray::refresh_tray_state(app)?;
        if tray_visual_updates(app) != updates {
            return Err("unchanged refresh still resubmitted tray state".into());
        }
    }
    // Two distinct singles are still two preference changes. Their previews
    // can supersede each other without leaving a pending visual target.
    drive_tray_input(
        app,
        &[
            TrayInput::Down,
            TrayInput::Up,
            TrayInput::Down,
            TrayInput::Up,
        ],
    )?;
    settle_tray_timers(app)?;
    assert_tray_visuals(app, false)?;
    if app.state::<ModelConfigStore>().get().selection_icon_enabled {
        return Err("two independent singles did not return to initial state".into());
    }
    tauri::async_runtime::block_on(tray::perform_action(app, "selection-icon"))?;
    assert_tray_visuals(app, true)?;
    windows::hide_tray_feedback_window(app)?;
    Ok(())
}

fn settle_tray_timers(app: &tauri::AppHandle) -> Result<(), String> {
    tauri::async_runtime::block_on(async {
        tokio::time::sleep(super::super::tray::double_click_interval() + Duration::from_millis(40))
            .await;
        windows::on_window_thread(app, || Ok(())).await
    })
}

fn exercise_tray_clicks(
    app: &tauri::AppHandle,
    config_path: &std::path::Path,
) -> Result<(), String> {
    let config = app.state::<ModelConfigStore>();
    assert_tray_visuals(app, false)?;
    config.set_selection_icon_enabled(true)?;
    app.state::<SelectionButtonManager>()
        .initialize_enabled(true);
    super::super::tray::refresh_tray_state(app)?;
    assert_tray_visuals(app, true)?;
    exercise_queued_tray_refresh(app)?;
    let initial = config.get();
    for enabled in [
        !initial.selection_icon_enabled,
        initial.selection_icon_enabled,
    ] {
        drive_tray_input(app, &[TrayInput::Down, TrayInput::Up])?;
        wait("single tray click changes selection preference", || {
            config.get().selection_icon_enabled == enabled
        })?;
        assert_tray_visuals(app, enabled)?;
        if !enabled {
            for light in [false, true] {
                let handle = app.clone();
                tauri::async_runtime::block_on(windows::on_window_thread(app, move || {
                    super::super::tray::apply_tray_state_on_window_thread(&handle, light)
                }))?;
                assert_tray_visuals(app, false)?;
                if app
                    .state::<NativeProbe>()
                    .tray
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .light
                    != light
                {
                    return Err("simulated theme was not applied to disabled tray icon".into());
                }
            }
        }
        if config.get().mode != initial.mode
            || app.state::<SelectionButtonManager>().is_enabled() != enabled
        {
            return Err(
                "single tray click changed mode or missed the running selection state".into(),
            );
        }
        let persisted: ModelConfig =
            serde_json::from_slice(&std::fs::read(config_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if persisted.selection_icon_enabled != enabled || persisted.mode != initial.mode {
            return Err("single tray click was not persisted correctly".into());
        }
    }
    for mode in [
        super::super::tray::next_translation_mode(initial.mode),
        initial.mode,
    ] {
        drive_tray_input(
            app,
            &[
                TrayInput::Down,
                TrayInput::Up,
                TrayInput::Double,
                TrayInput::Up,
            ],
        )?;
        if config.get().mode != mode
            || config.get().selection_icon_enabled != initial.selection_icon_enabled
        {
            return Err("double tray click did not switch only mode once".into());
        }
        assert_tray_visuals(app, initial.selection_icon_enabled)?;
    }
    settle_tray_timers(app)?;
    if config.get().selection_icon_enabled != initial.selection_icon_enabled
        || config.get().mode != initial.mode
    {
        return Err("cancelled double-click single fired late".into());
    }
    drive_tray_input(
        app,
        &[
            TrayInput::Down,
            TrayInput::Up,
            TrayInput::RightDown,
            TrayInput::RightUp,
        ],
    )?;
    wait("right tray click still opens menu", || {
        app.get_webview_window("tray-menu")
            .is_some_and(|w| w.is_visible().unwrap_or(false))
    })
    .map_err(|error| {
        let snapshot = app.state::<TrayMenu>().snapshot();
        let window = app.get_webview_window("tray-menu");
        format!(
            "{error}; session={} open={} accepts_open={} visible={:?} focused={:?}",
            snapshot.generation,
            snapshot.open,
            app.state::<TrayMenu>().accepts_open(),
            window.as_ref().map(|w| w.is_visible()),
            window.as_ref().map(|w| w.is_focused())
        )
    })?;
    settle_tray_timers(app)?;
    if config.get().selection_icon_enabled != initial.selection_icon_enabled {
        return Err("right-click did not cancel a pending single".into());
    }
    tauri::async_runtime::block_on(close(app, None))?;
    windows::hide_tray_feedback_window(app)?;
    Ok(())
}

fn exercise(
    app: &tauri::AppHandle,
    config_path: &std::path::Path,
    endpoint: &str,
) -> Result<(), String> {
    let config = app.state::<ModelConfigStore>();
    let selection = app.state::<SelectionButtonManager>();
    let saved: ModelConfig =
        serde_json::from_slice(&std::fs::read(config_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if saved.selection_icon_enabled
        || !config.get().selection_icon_enabled
        || !selection.is_enabled()
    {
        return Err("saved-off startup did not enable selection consistently".into());
    }
    assert_tray_visuals(app, true)?;
    // Keep the existing disabled-state and toggle assertions after checking startup.
    config.set_selection_icon_enabled(false)?;
    selection.initialize_enabled(false);
    super::super::tray::refresh_tray_state(app)?;
    assert_tray_visuals(app, false)?;
    exercise_tray_clicks(app, config_path)?;
    exercise_fold_animation(app)?;
    exercise_immediate_response(app)?;
    let settings_identity = exercise_settings_creation(app)?;
    exercise_ui_fonts(app)?;
    let (entered, worker) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    app.state::<SettingsWindow>()
        .before_create(Box::new(move || {
            entered.send(()).map_err(|e| e.to_string())?;
            blocked
                .recv_timeout(Duration::from_secs(10))
                .map_err(|e| e.to_string())
        }));
    let handle = app.clone();
    let existing_open = std::thread::spawn(move || {
        tauri::async_runtime::block_on(windows::show_settings_window(&handle))
    });
    worker
        .recv_timeout(Duration::from_secs(10))
        .map_err(|e| e.to_string())?;
    let menu = open(app)?;
    let generation = app.state::<TrayMenu>().snapshot().generation;
    click(&menu, 2)?;
    wait("menu action joins the existing settings open", || {
        !app.state::<TrayMenu>().accepts_open()
    })?;
    tauri::async_runtime::block_on(show(
        app,
        PhysicalPosition::new(0.0, 0.0),
        tauri::Rect {
            position: tauri::Position::Physical(PhysicalPosition::new(0, 0)),
            size: tauri::Size::Physical(PhysicalSize::new(24, 24)),
        },
    ))?;
    tauri::async_runtime::block_on(tray_menu_action(
        app.clone(),
        menu.clone(),
        generation,
        "selection-icon".into(),
    ))?;
    tauri::async_runtime::block_on(tray_menu_painted(app.clone(), menu.clone(), generation))?;
    if app.state::<TrayMenu>().accepts_open()
        || menu.is_visible().unwrap_or(true)
        || app.state::<TrayMenu>().snapshot().generation != generation
        || !config.get().selection_icon_enabled
    {
        return Err("new open/duplicate action raced with pending settings focus".into());
    }
    release.send(()).map_err(|e| e.to_string())?;
    existing_open
        .join()
        .map_err(|_| "existing open panicked")??;
    wait("settings menu action settled", || {
        app.state::<TrayMenu>().accepts_open()
    })?;
    windows::hide_settings_window(app)?;
    for _ in 0..3 {
        let menu = open(app)?;
        click(&menu, 2)?;
        wait("settings opens and menu closes", || {
            app.get_webview_window("settings")
                .is_some_and(|w| w.is_visible().unwrap_or(false))
                && !menu.is_visible().unwrap_or(true)
        })?;
        let settings = app.get_webview_window("settings").unwrap();
        assert_single_settings(app, settings_identity)?;
        probe_settings(
            app,
            "document.querySelector('button[aria-label=最小化]').click(); return 'minimized';",
        )?;
        wait("settings minimizes", || {
            settings.is_minimized().unwrap_or(false)
        })?;
        let menu = open(app)?;
        click(&menu, 2)?;
        wait("settings restores", || {
            settings.is_visible().unwrap_or(false)
                && settings.is_focused().unwrap_or(false)
                && !settings.is_minimized().unwrap_or(true)
                && !menu.is_visible().unwrap_or(true)
        })?;
        assert_single_settings(app, settings_identity)?;
        probe_settings(
            app,
            "window.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true})); return 'escaped';",
        )?;
        wait("settings Escape hides", || {
            !settings.is_visible().unwrap_or(true)
        })?;
    }
    for enabled in [false, true, false, true] {
        let menu = open(app)?;
        click(&menu, 1)?;
        wait("selection preference and runtime agree", || {
            config.get().selection_icon_enabled == enabled
                && selection.is_enabled() == enabled
                && !menu.is_visible().unwrap_or(true)
        })?;
        assert_tray_visuals(app, enabled)?;
        let persisted: ModelConfig =
            serde_json::from_slice(&std::fs::read(config_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if persisted.selection_icon_enabled != enabled {
            return Err("selection preference not persisted".into());
        }
    }
    for backend in [
        ModelBackend::Api,
        ModelBackend::Local,
        ModelBackend::Api,
        ModelBackend::Local,
    ] {
        let menu = open(app)?;
        if !app.state::<TrayMenu>().snapshot().model_enabled {
            return Err("configured model disabled".into());
        }
        click(&menu, 0)?;
        wait("backend switch and surface sync", || {
            config.get().backend == backend && !menu.is_visible().unwrap_or(true)
        })?;
        wait("backend action and refresh settled", || {
            app.state::<TrayMenu>().accepts_open()
        })?;
        let persisted: ModelConfig =
            serde_json::from_slice(&std::fs::read(config_path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if persisted.backend != backend {
            return Err("backend not persisted".into());
        }
    }
    app.state::<CredentialStore>().clear_api_key(endpoint)?;
    let menu = open(app)?;
    if app.state::<TrayMenu>().snapshot().model_enabled {
        return Err("missing credential did not disable model".into());
    }
    click(&menu, 0)?;
    std::thread::sleep(Duration::from_millis(100));
    if config.get().backend != ModelBackend::Local || !menu.is_visible().unwrap_or(false) {
        return Err("disabled model changed state".into());
    }
    tauri::async_runtime::block_on(close(app, None))?;
    for _ in 0..3 {
        let menu = open(app)?;
        menu.eval("document.querySelector('.tray-menu').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}))").map_err(|e|e.to_string())?;
        wait("Escape dismissal", || !menu.is_visible().unwrap_or(true))?;
        let menu = open(app)?;
        menu.eval("document.querySelector('.tray-menu-stage').dispatchEvent(new PointerEvent('pointerdown',{bubbles:true}))").map_err(|e|e.to_string())?;
        wait("padding dismissal", || !menu.is_visible().unwrap_or(true))?;
    }
    open(app)?;
    let previous = app.state::<TrayMenu>().snapshot().generation;
    tauri::async_runtime::block_on(close(app, None))?;
    let menu = open(app)?;
    tauri::async_runtime::block_on(close(app, Some(previous)))?;
    tauri::async_runtime::block_on(tray_menu_painted(app.clone(), menu.clone(), previous))?;
    tauri::async_runtime::block_on(tray_menu_action(
        app.clone(),
        menu.clone(),
        previous,
        "selection-icon".into(),
    ))?;
    if !menu.is_visible().unwrap_or(false) || !config.get().selection_icon_enabled {
        return Err("stale action affected new menu".into());
    }
    assert_tray_visuals(app, true)?;
    tauri::async_runtime::block_on(windows::show_settings_window(app))?;
    wait("native focus-loss dismissal", || {
        !menu.is_visible().unwrap_or(true)
    })?;
    windows::hide_settings_window(app)?;
    let menu = open(app)?;
    // The runner must also verify a normal process exit after this report.
    exercise_late_exit(app, &menu, endpoint)?;
    Ok(())
}

fn exercise_manual_fixture(app: &tauri::AppHandle) -> Result<(), String> {
    let menu = open(app)?;
    click(&menu, 2)?;
    wait("manual fixture settings opens and focuses", || {
        app.state::<TrayMenu>().accepts_open()
            && !menu.is_visible().unwrap_or(true)
            && app
                .get_webview_window("settings")
                .is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false))
    })?;
    let settings = app.get_webview_window("settings").unwrap();
    let identity = settings.hwnd().map_err(|e| e.to_string())?.0 as isize;
    let value = probe_settings(
        app,
        r#"
        while(document.querySelector('#model-name')?.value !== 'synthetic-local') await new Promise(resolve=>setTimeout(resolve,40));
        const status=await window.__TAURI_INTERNALS__.invoke('get_model_api_key_status',{baseUrl:'https://api.openai.com/v1'});
        if(status.hasApiKey || status.apiKeyHint !== null) throw new Error('Real provider credential query was not isolated');
        document.querySelector('button[aria-label=关闭]').click(); return 'closed';
    "#,
    )?;
    if value != "closed" {
        return Err("manual fixture form failed to close".into());
    }
    wait("manual fixture close hides", || {
        !settings.is_visible().unwrap_or(true)
    })?;
    let menu = open(app)?;
    click(&menu, 2)?;
    wait("manual fixture reopens the same settings", || {
        app.state::<TrayMenu>().accepts_open()
            && settings.is_visible().unwrap_or(false)
            && settings.is_focused().unwrap_or(false)
    })?;
    assert_single_settings(app, identity)?;
    windows::hide_settings_window(app)?;
    let menu = open(app)?;
    menu.eval("document.querySelector('.tray-menu').dispatchEvent(new KeyboardEvent('keydown',{key:'Escape',bubbles:true}))").map_err(|e| e.to_string())?;
    wait("manual fixture Escape dismissal", || {
        !menu.is_visible().unwrap_or(true)
    })?;
    let menu = open(app)?;
    app.state::<ManualSession>()
        .checks_passed
        .store(true, Ordering::Release);
    click(&menu, 3)?;
    Ok(())
}

pub(crate) fn run_native_smoke() {
    let manual_check = std::env::args().any(|arg| arg == "--tray-menu-manual-check");
    let manual = manual_check || std::env::args().any(|arg| arg == "--tray-menu-manual");
    let root = std::env::current_dir().unwrap().join(if manual {
        "tests/artifacts/tray-menu-manual-sandbox"
    } else {
        "tests/artifacts/tray-menu-sandbox"
    });
    std::fs::create_dir_all(&root).unwrap();
    let manual_session = manual.then(|| ManualSession::new(root.parent().unwrap(), manual_check));
    let config_path = root.join("model-config.json");
    let endpoint = format!("https://translay-smoke-{}.invalid/v1", std::process::id());
    let mut fixture = ModelConfig::default();
    fixture.local.model = "synthetic-local".into();
    fixture.api.model = "synthetic-api".into();
    fixture.api.base_url = endpoint.clone();
    fixture.selection_icon_enabled = manual;
    let store = ModelConfigStore::isolated_for_acceptance(config_path.clone(), fixture).unwrap();
    let store = if manual {
        store
    } else {
        let original = std::fs::read(&config_path).unwrap();
        let startup_store = ModelConfigStore::load_from_path(config_path.clone()).unwrap();
        assert_eq!(std::fs::read(&config_path).unwrap(), original);
        startup_store
    };
    let credentials = CredentialStore;
    credentials
        .write_api_key(&endpoint, "synthetic-test-only-not-a-real-key")
        .unwrap();
    // A blocked UI thread must not strand the synthetic credential or test process.
    let watchdog_endpoint = endpoint.clone();
    let watchdog_session = manual_session.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(if manual && !manual_check {
            30 * 60
        } else {
            35
        }));
        if let Some(session) = watchdog_session {
            let _ = session.finish(&watchdog_endpoint, "timed-out");
        } else {
            let _ = CredentialStore.clear_api_key(&watchdog_endpoint);
            let _ = std::fs::write(
                "tests/artifacts/tray-menu-native.txt",
                "FAIL: native acceptance watchdog timeout",
            );
        }
        std::process::exit(1);
    });
    let latest = crate::latest_capture_store::LatestCaptureStore::default();
    let coordinator = crate::capture_coordinator::CaptureCoordinator::new(
        crate::overlay_manager::OverlayManager::new(latest),
    );
    let shutdown = super::super::shutdown::ShutdownCoordinator::new(
        Arc::new(crate::hotkey_service::HotkeyService::default()),
        coordinator,
    );
    let cleanup_endpoint = endpoint.clone();
    let setup_session = manual_session.clone();
    let exit_endpoint = endpoint.clone();
    let mut builder = tauri::Builder::default()
        .manage(NativeAcceptanceData(root))
        .manage(TrayMenu::default())
        .manage(NativeProbe::default())
        .manage(ExitFence::default())
        .manage(super::super::windows::SettingsWindow::default())
        .manage(store)
        .manage(credentials)
        .manage(SelectionButtonManager::default())
        .manage(shutdown)
        .manage(super::super::tray_feedback::TrayFeedbackManager::default())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_tray_menu,
            tray_menu_painted,
            dismiss_tray_menu,
            tray_menu_action,
            tray_menu_smoke_probe,
            super::super::commands::get_model_config,
            get_model_api_key_status,
            super::super::commands::hide_settings_window,
            super::super::commands::minimize_settings_window,
            super::super::commands::start_settings_dragging
        ]);
    if let Some(session) = &manual_session {
        builder = builder.manage(session.clone());
    }
    let result = builder
        .setup(move |app| {
            app.state::<SelectionButtonManager>()
                .initialize_enabled(app.state::<ModelConfigStore>().get().selection_icon_enabled);
            create(app.handle()).map_err(std::io::Error::other)?;
            super::super::windows::create_selection_button_window(app.handle())
                .map_err(std::io::Error::other)?;
            super::super::windows::create_tray_feedback_window(app.handle())
                .map_err(std::io::Error::other)?;
            super::super::tray::setup_tray(app)?;
            if let Some(session) = &setup_session {
                record_manual_event(app.handle(), "fixture-ready");
                session
                    .write_report("ready-for-user", false)
                    .map_err(std::io::Error::other)?;
            }
            // A manual run opens only in response to actual tray input.
            if manual && !manual_check {
                return Ok(());
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let result = if manual_check {
                    exercise_manual_fixture(&handle)
                } else {
                    exercise(&handle, &config_path, &endpoint)
                };
                let _ = handle.state::<CredentialStore>().clear_api_key(&endpoint);
                if let Err(error) = result {
                    if handle.try_state::<ManualSession>().is_some() {
                        record_manual_event(&handle, &format!("fixture-error:{error}"));
                    } else {
                        let _ = std::fs::write(
                            "tests/artifacts/tray-menu-native.txt",
                            format!("FAIL: {error}"),
                        );
                    }
                    handle.exit(1);
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!());
    match result {
        Ok(app) => app.run(move |handle, event| {
            if let tauri::RunEvent::WindowEvent { label, event, .. } = &event {
                if label == "settings" || label == "tray-menu" {
                    match event {
                        tauri::WindowEvent::Focused(focused) => {
                            record_manual_event(handle, &format!("{label}-focused:{focused}"))
                        }
                        tauri::WindowEvent::CloseRequested { .. } => {
                            record_manual_event(handle, &format!("{label}-native-close"))
                        }
                        _ => {}
                    }
                }
            }
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if let Some(session) = handle.try_state::<ManualSession>() {
                    let status = if code == Some(0) {
                        if session.automated && session.checks_passed.load(Ordering::Acquire) {
                            "fixture-check-passed"
                        } else if session.automated {
                            "failed"
                        } else {
                            "closed"
                        }
                    } else {
                        "failed"
                    };
                    if let Err(error) = session.finish(&exit_endpoint, status) {
                        eprintln!("手动验收清理失败：{error}");
                        api.prevent_exit();
                        handle.exit(1);
                    }
                    return;
                }
                if code != Some(0) {
                    return;
                }
                let fence = handle.state::<ExitFence>();
                if fence.hold.load(Ordering::Acquire) {
                    api.prevent_exit();
                    fence.observed.store(true, Ordering::Release);
                }
            }
        }),
        Err(error) => {
            if let Some(session) = &manual_session {
                let _ = session.finish(&cleanup_endpoint, "failed");
            }
            let _ = CredentialStore.clear_api_key(&cleanup_endpoint);
            panic!("native tray acceptance failed: {error}");
        }
    }
    let _ = CredentialStore.clear_api_key(&cleanup_endpoint);
}
