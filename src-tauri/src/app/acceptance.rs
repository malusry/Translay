// This module is compiled only under cfg(debug_assertions).
use crate::{
    explanation::ExplanationContent,
    foreground_context::ForegroundContext,
    latest_capture_store::LatestCaptureStore,
    models::{CapturePayload, CapturePhase, ScreenRect},
    overlay_manager::OverlayManager,
    translation::TranslationMode,
};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use tauri::Manager;

struct Acceptance {
    corner: String,
    sample: String,
    automatic: bool,
    started: AtomicBool,
    acknowledged: AtomicBool,
    records: Mutex<Vec<Value>>,
}

fn anchor(work: ScreenRect, corner: &str) -> ScreenRect {
    let x = if corner.ends_with("right") {
        work.right - 20
    } else {
        work.left + 20
    };
    let y = if corner.starts_with("bottom") {
        work.bottom - 20
    } else {
        work.top + 20
    };
    ScreenRect {
        left: x,
        top: y,
        right: x + 1,
        bottom: y + 1,
    }
}

fn fixture(sample: &str) -> CapturePayload {
    let text = match sample {
        "short" => "注意力机制会根据相关程度，为输入分配不同的权重。".to_owned(),
        "math" => "缩放点积注意力使用 $1/\\sqrt{d_k}$ 调整数值范围。\n\n$$\\operatorname{Attention}(Q,K,V)=\\operatorname{softmax}(QK^T/\\sqrt{d_k})V$$\n\n该公式说明了查询、键和值之间的关系。".to_owned(),
        _ => "这是固定的验收译文，用于检查长内容的阅读位置。展开解释时，译文应保留原来的宽度和滚动位置。\n\n".repeat(12),
    };
    CapturePayload {
        request_id: 1,
        phase: CapturePhase::Translated,
        success: true,
        text,
        application_name: "Translay Acceptance (synthetic)".into(),
        process_id: 0,
        capture_method: "acceptance-fixture".into(),
        elapsed_ms: 0,
        selection_rect: None,
        error_code: None,
        error_message: None,
        focus_preserved: true,
        clipboard_restored: None,
        warning_code: None,
        language_profile: None,
        translation_mode: Some(TranslationMode::Academic),
        tone_note: None,
    }
}

#[tauri::command]
fn overlay_frontend_ready(app: tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<Acceptance>();
    if state.started.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    let monitor = app
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("No primary monitor")?;
    let area = monitor.work_area();
    let work = ScreenRect {
        left: area.position.x,
        top: area.position.y,
        right: area.position.x + area.size.width as i32,
        bottom: area.position.y + area.size.height as i32,
    };
    let context = ForegroundContext::capture().map_err(str::to_owned)?;
    let overlay = app.state::<OverlayManager>();
    overlay.begin_request(1);
    overlay.show(
        &app,
        &fixture(&state.sample),
        &context,
        anchor(work, &state.corner),
    )?;
    Ok(())
}

#[tauri::command]
fn ack_capture(app: tauri::AppHandle, request_id: u64) -> bool {
    let state = app.state::<Acceptance>();
    // Acknowledge delivery without starting the production auto-hide timer.
    app.state::<LatestCaptureStore>().acknowledge(request_id);
    if state.automatic && !state.acknowledged.swap(true, Ordering::AcqRel) {
        if let Some(window) = app.get_webview_window("overlay") {
            let _ = window.eval(include_str!("acceptance.js"));
        }
    }
    true
}

#[tauri::command]
fn explain_translation() -> ExplanationContent {
    ExplanationContent {core_explanation:"这是一段固定解释，用于验收真实解释面板的展开、滚动及收起。内容不来自模型，也不会发送网络请求。".repeat(10),
        key_concepts:vec![],caveat:"仅为开发验收样例。".into()}
}

#[tauri::command]
fn cancel_explanation() -> bool {
    true
}
#[tauri::command]
fn copy_translation() {} // Test fixtures never replace the user's clipboard.
#[tauri::command]
fn dismiss_overlay(app: tauri::AppHandle) -> bool {
    app.exit(0);
    true
}

#[tauri::command]
fn acceptance_checkpoint(app: tauri::AppHandle, stage: String, dom: Value) -> Result<(), String> {
    let window = app.get_webview_window("overlay").ok_or("Missing overlay")?;
    let p = window.outer_position().map_err(|e| e.to_string())?;
    let s = window.outer_size().map_err(|e| e.to_string())?;
    let monitor = window
        .current_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("No monitor")?;
    let work = monitor.work_area();
    let inside = p.x >= work.position.x
        && p.y >= work.position.y
        && p.x + s.width as i32 <= work.position.x + work.size.width as i32
        && p.y + s.height as i32 <= work.position.y + work.size.height as i32;
    let record = json!({"stage":stage,"x":p.x,"y":p.y,"width":s.width,"height":s.height,
        "dpiScale":monitor.scale_factor(),"insideWorkArea":inside,
        "work":{"x":work.position.x,"y":work.position.y,"width":work.size.width,"height":work.size.height},"dom":dom});
    app.state::<Acceptance>()
        .records
        .lock()
        .unwrap()
        .push(record);
    Ok(())
}

#[tauri::command]
fn acceptance_finish(app: tauri::AppHandle, error: Option<String>) -> Result<(), String> {
    let state = app.state::<Acceptance>();
    let records = state.records.lock().unwrap();
    let passed = error.is_none() && valid_records(&records);
    let report = json!({"kind":"Translay debug native acceptance","corner":state.corner,"sample":state.sample,
        "passed":passed,"error":error,"records":*records,
        "scope":"real overlay and native layout; synthetic data; no real model or mouse-input validation"});
    let directory = std::env::current_dir()
        .map_err(|e| e.to_string())?
        .join("tests/artifacts");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    std::fs::write(
        directory.join(format!("acceptance-{}-{}.json", state.corner, state.sample)),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    println!("{report}");
    app.exit(if passed { 0 } else { 1 });
    Ok(())
}

pub(super) fn run() {
    let args: Vec<String> = std::env::args().collect();
    let corner = args
        .iter()
        .find_map(|a| a.strip_prefix("--acceptance="))
        .unwrap_or("bottom-right");
    let sample = args
        .iter()
        .find_map(|a| a.strip_prefix("--acceptance-sample="))
        .unwrap_or("long");
    if !["bottom-right", "bottom-left", "top-right", "top-left"].contains(&corner)
        || !["short", "long", "math"].contains(&sample)
    {
        eprintln!("Invalid acceptance scenario");
        std::process::exit(2);
    }
    let automatic = args.iter().any(|a| a == "--acceptance-auto");
    let store = LatestCaptureStore::default();
    tauri::Builder::default()
        .manage(OverlayManager::new(store.clone()))
        .manage(store)
        .manage(Acceptance {
            corner: corner.into(),
            sample: sample.into(),
            automatic,
            started: AtomicBool::new(false),
            acknowledged: AtomicBool::new(false),
            records: Mutex::new(vec![]),
        })
        .invoke_handler(tauri::generate_handler![
            overlay_frontend_ready,
            ack_capture,
            explain_translation,
            cancel_explanation,
            copy_translation,
            dismiss_overlay,
            acceptance_checkpoint,
            acceptance_finish,
            super::commands::get_latest_capture,
            super::commands::fit_overlay_height,
            super::commands::set_overlay_hovered
        ])
        .setup(move |app| {
            if automatic {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(25));
                    let _ = acceptance_finish(handle, Some("Acceptance watchdog expired".into()));
                    // Also terminate when WebView creation blocks the main event loop.
                    std::process::exit(3);
                });
            }
            let data = std::env::current_dir()?.join("tests/artifacts/acceptance-webview");
            std::fs::create_dir_all(&data)?;
            super::windows::create_overlay_window_with_data(app.handle(), data)
                .map_err(std::io::Error::other)?;
            app.get_webview_window("overlay")
                .unwrap()
                .set_title("Translay development acceptance")?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Acceptance runner failed");
}

fn valid_records(records: &[Value]) -> bool {
    if records.len() != 3
        || !records
            .iter()
            .all(|r| r["insideWorkArea"] == true && r["dom"]["controlsInside"] == true)
    {
        return false;
    }
    let (before, expanded, after) = (&records[0], &records[1], &records[2]);
    let y = before["y"].as_i64().unwrap_or(0);
    let bottom = expanded["work"]["y"].as_i64().unwrap_or(0)
        + expanded["work"]["height"].as_i64().unwrap_or(0);
    let expected_y = y.min(bottom - expanded["height"].as_i64().unwrap_or(0));
    before["x"] == expanded["x"]
        && expanded["x"] == after["x"]
        && expanded["y"].as_i64() == Some(expected_y)
        && expanded["y"] == after["y"]
        && before["width"] == expanded["width"]
        && expanded["width"] == after["width"]
        // Logical/physical conversion can round differently at fractional DPI.
        && before["height"].as_i64().zip(after["height"].as_i64())
            .is_some_and(|(a, b)| (a - b).abs() <= 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn anchors_follow_negative_monitor_origin() {
        let work = ScreenRect {
            left: -1920,
            top: 40,
            right: 0,
            bottom: 1080,
        };
        assert_eq!(anchor(work, "bottom-right").left, -20);
        assert_eq!(anchor(work, "bottom-left").left, -1900);
        assert_eq!(anchor(work, "top-left").top, 60);
    }
    #[test]
    fn fixtures_are_offline_academic_results() {
        for sample in ["short", "long", "math"] {
            let p = fixture(sample);
            assert!(p.success);
            assert_eq!(p.translation_mode, Some(TranslationMode::Academic));
            assert!(p.text.len() > 20);
            assert!(p.error_message.is_none());
        }
    }
}
