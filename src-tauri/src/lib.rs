//! Canvas Assistant: a Windows desktop assistant for Canvas LMS powered by Codex.
pub mod canvas;
pub mod codex;
pub mod config;
pub mod db;
pub mod mcp;
pub mod scheduler;

use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WindowEvent,
};
use tauri_plugin_opener::OpenerExt;

pub const RELEASES_URL: &str = "https://github.com/sopo9880/canvas-assistant/releases";
const RELEASES_API: &str = "https://api.github.com/repos/sopo9880/canvas-assistant/releases?per_page=10";

pub struct AppState {
    pub data_dir: PathBuf,
    pub codex: Arc<codex::CodexState>,
    pub db: Mutex<db::Db>,
    pub scheduler: Mutex<Option<mpsc::Sender<scheduler::Signal>>>,
}

type Shared<'a> = State<'a, Arc<AppState>>;

/// Runs blocking work (network, Codex, SQLite) off the UI thread.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_minimized().unwrap_or(false) {
            let _ = w.unminimize();
        }
        let _ = w.show();
        let _ = w.set_focus();
    }
}

// ---------------- app & settings ----------------

#[tauri::command]
async fn app_info(state: Shared<'_>) -> Result<Value, String> {
    let s = state.inner().clone();
    blocking(move || {
        let settings = config::load(&s.data_dir);
        Ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "dataDir": s.data_dir.to_string_lossy(),
            "hasToken": config::get_token().is_some(),
            "settings": settings,
            "pending": s.db.lock().map(|d| d.pending_count()).unwrap_or(0),
        }))
    }).await
}

#[tauri::command]
async fn settings_save(state: Shared<'_>, patch: Value) -> Result<config::Settings, String> {
    let s = state.inner().clone();
    blocking(move || {
        let mut settings = config::load(&s.data_dir);
        if let Some(v) = patch.get("closeToTray").and_then(Value::as_bool) {
            settings.close_to_tray = v;
        }
        if let Some(v) = patch.get("notifications").and_then(Value::as_bool) {
            settings.notifications = v;
        }
        if let Some(v) = patch.get("canvasToolsInChat").and_then(Value::as_bool) {
            settings.canvas_tools_in_chat = v;
        }
        if let Some(v) = patch.get("codexPath") {
            settings.codex_path = v.as_str().map(str::trim).filter(|p| !p.is_empty()).map(str::to_string);
            if let Some(p) = &settings.codex_path {
                if !p.to_ascii_lowercase().ends_with(".exe") || !std::path::Path::new(p).is_file() {
                    return Err("codex.exe 파일의 전체 경로를 입력하세요.".into());
                }
            }
        }
        for (key, slot) in [("defaultModel", &mut settings.default_model), ("defaultEffort", &mut settings.default_effort)] {
            if let Some(v) = patch.get(key) {
                *slot = v.as_str().filter(|x| !x.is_empty() && x.len() < 100).map(str::to_string);
            }
        }
        config::save(&s.data_dir, &settings)?;
        Ok(settings)
    }).await
}

#[tauri::command]
async fn check_update() -> Result<Value, String> {
    blocking(|| {
        let current = env!("CARGO_PKG_VERSION");
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(15)))
            .user_agent(concat!("CanvasAssistant/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        let mut resp = agent.get(RELEASES_API).header("Accept", "application/vnd.github+json").call().map_err(|e| e.to_string())?;
        if resp.status().as_u16() != 200 {
            // Private repository or rate limit: let the user check the page directly.
            return Ok(json!({"available": false, "current": current, "url": RELEASES_URL}));
        }
        let list: Value = resp.body_mut().read_json().map_err(|e| e.to_string())?;
        let latest = list.as_array().and_then(|a| a.iter().find(|r| r.get("draft") != Some(&json!(true)))).cloned().unwrap_or(Value::Null);
        let tag = latest.get("tag_name").and_then(Value::as_str).unwrap_or("").trim_start_matches('v').to_string();
        Ok(json!({
            "available": true,
            "current": current,
            "latest": tag,
            "newer": !tag.is_empty() && version_newer(&tag, current),
            "url": latest.get("html_url").and_then(Value::as_str).unwrap_or(RELEASES_URL),
            "prerelease": latest.get("prerelease"),
        }))
    }).await
}

/// Semver-ish comparison: 0.2.0 > 0.1.0, 0.1.0 > 0.1.0-beta.1.
pub fn version_newer(candidate: &str, current: &str) -> bool {
    fn parts(v: &str) -> (Vec<u64>, Option<String>) {
        let (core, pre) = match v.split_once('-') {
            Some((c, p)) => (c, Some(p.to_string())),
            None => (v, None),
        };
        (core.split('.').map(|x| x.parse().unwrap_or(0)).collect(), pre)
    }
    let (a, ap) = parts(candidate);
    let (b, bp) = parts(current);
    match a.cmp(&b) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => match (ap, bp) {
            (None, Some(_)) => true,
            (Some(x), Some(y)) => x > y,
            _ => false,
        },
    }
}

// ---------------- Canvas ----------------

#[tauri::command]
async fn canvas_connect(state: Shared<'_>, base_url: String, token: Option<String>) -> Result<Value, String> {
    let s = state.inner().clone();
    blocking(move || {
        let base = canvas::normalize_base_url(&base_url)?;
        let new_token = token.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
        let use_token = match &new_token {
            Some(t) => t.clone(),
            None => config::get_token().ok_or("액세스 토큰을 입력하세요.")?,
        };
        // Verify before saving anything.
        let client = canvas::Client::new(&base, &use_token)?;
        let profile = client.profile()?;
        if let Some(t) = &new_token {
            config::set_token(t)?;
        }
        let mut settings = config::load(&s.data_dir);
        settings.canvas_base_url = Some(base.clone());
        config::save(&s.data_dir, &settings)?;
        if let Ok(db) = s.db.lock() {
            db.audit("canvas_connect", &base, true);
        }
        if let Some(tx) = s.scheduler.lock().ok().and_then(|g| g.clone()) {
            let _ = tx.send(scheduler::Signal::Wake);
        }
        Ok(json!({"baseUrl": base, "profile": profile}))
    }).await
}

#[tauri::command]
async fn canvas_disconnect(state: Shared<'_>) -> Result<(), String> {
    let s = state.inner().clone();
    blocking(move || {
        config::delete_token()?;
        let mut settings = config::load(&s.data_dir);
        settings.canvas_base_url = None;
        config::save(&s.data_dir, &settings)?;
        if let Ok(db) = s.db.lock() {
            db.audit("canvas_disconnect", "토큰 삭제", true);
        }
        Ok(())
    }).await
}

#[tauri::command]
async fn canvas_query(state: Shared<'_>, tool: String, args: Option<Value>) -> Result<Value, String> {
    let s = state.inner().clone();
    blocking(move || {
        if !tool.starts_with("canvas_") {
            return Err("unknown query".into());
        }
        let client = config::canvas_client(&s.data_dir)?;
        mcp::read_tool(&client, &tool, &args.unwrap_or_else(|| json!({})))
    }).await
}

#[tauri::command]
async fn canvas_download(app: AppHandle, state: Shared<'_>, file_id: String) -> Result<String, String> {
    let s = state.inner().clone();
    let dir = app.path().download_dir().map_err(|e| e.to_string())?.join("Canvas Assistant");
    let path = blocking(move || {
        let id = canvas::require_id(&json!({"id": file_id}), "id")?;
        let client = config::canvas_client(&s.data_dir)?;
        Ok(client.download_file(id, &dir)?)
    }).await?;
    let _ = app.opener().reveal_item_in_dir(&path);
    Ok(path.to_string_lossy().into_owned())
}

// ---------------- approval queue ----------------

#[tauri::command]
async fn pending_list(state: Shared<'_>, include_done: bool) -> Result<Vec<Value>, String> {
    let s = state.inner().clone();
    blocking(move || s.db.lock().map_err(|_| "db busy".to_string())?.list_pending(include_done)).await
}

#[tauri::command]
async fn pending_decide(app: AppHandle, state: Shared<'_>, id: i64, approve: bool) -> Result<Value, String> {
    let s = state.inner().clone();
    let out = blocking(move || {
        let db = db::Db::open(&s.data_dir)?;
        if !approve {
            db.reject_pending(id)?;
            db.audit("reject", &format!("요청 #{id} 거절"), true);
            return Ok(json!({"status": "rejected"}));
        }
        let (kind, payload) = db.claim_pending(id)?;
        let summary = canvas::describe_write(&kind, &payload);
        let result = config::canvas_client(&s.data_dir).and_then(|c| c.execute_write(&kind, &payload).map_err(String::from));
        match result {
            Ok(r) => {
                db.finish_pending(id, "done", &r.to_string())?;
                db.audit("execute", &summary, true);
                Ok(json!({"status": "done", "result": r}))
            }
            Err(e) => {
                db.finish_pending(id, "failed", &e)?;
                db.audit("execute", &format!("{summary} — 실패: {e}"), false);
                Err(e)
            }
        }
    }).await;
    let _ = app.emit("pending-changed", json!({}));
    out
}

#[tauri::command]
async fn audit_list(state: Shared<'_>) -> Result<Vec<Value>, String> {
    let s = state.inner().clone();
    blocking(move || s.db.lock().map_err(|_| "db busy".to_string())?.audit_list()).await
}

// ---------------- automation ----------------

#[tauri::command]
async fn jobs_list(state: Shared<'_>) -> Result<Vec<Value>, String> {
    let s = state.inner().clone();
    blocking(move || s.db.lock().map_err(|_| "db busy".to_string())?.jobs()).await
}

#[tauri::command]
async fn jobs_update(state: Shared<'_>, kind: String, enabled: bool, interval_min: i64, params: Value) -> Result<(), String> {
    let s = state.inner().clone();
    blocking(move || {
        if kind == "ai_briefing" {
            let time = params.get("time").and_then(Value::as_str).unwrap_or("");
            if chrono::NaiveTime::parse_from_str(time, "%H:%M").is_err() {
                return Err("브리핑 시각은 HH:MM 형식이어야 합니다".into());
            }
            if params.get("prompt").and_then(Value::as_str).map_or(0, |p| p.chars().count()) > 2000 {
                return Err("브리핑 요청은 2000자 이하여야 합니다".into());
            }
        }
        s.db.lock().map_err(|_| "db busy".to_string())?.update_job(&kind, enabled, interval_min, &params)
    }).await
}

#[tauri::command]
async fn jobs_run_now(state: Shared<'_>, kind: String) -> Result<(), String> {
    let s = state.inner().clone();
    blocking(move || {
        if !db::JOB_KINDS.contains(&kind.as_str()) {
            return Err("알 수 없는 작업입니다".into());
        }
        {
            // Clearing the last run makes the job due on the next scheduler tick.
            s.db.lock().map_err(|_| "db busy".to_string())?.force_job_due(&kind);
        }
        if let Some(tx) = s.scheduler.lock().ok().and_then(|g| g.clone()) {
            let _ = tx.send(scheduler::Signal::Wake);
        }
        Ok(())
    }).await
}

#[tauri::command]
async fn briefings_list(state: Shared<'_>) -> Result<Vec<Value>, String> {
    let s = state.inner().clone();
    blocking(move || s.db.lock().map_err(|_| "db busy".to_string())?.briefings()).await
}

// ---------------- Codex ----------------

#[tauri::command]
async fn codex_start(app: AppHandle, state: Shared<'_>) -> Result<(), String> {
    let s = state.inner().clone();
    blocking(move || s.codex.start(&app)).await
}

#[tauri::command]
async fn codex_call(app: AppHandle, state: Shared<'_>, method: String, params: Value) -> Result<Value, String> {
    let s = state.inner().clone();
    blocking(move || s.codex.call(&app, &method, &params)).await
}

#[tauri::command]
fn codex_locate(state: Shared<'_>) -> Value {
    let settings = config::load(&state.data_dir);
    match codex::resolve_codex(settings.codex_path.as_deref()) {
        Ok((p, args)) => json!({"found": true, "path": p.to_string_lossy(), "viaNode": !args.is_empty()}),
        Err(e) => json!({"found": false, "error": e}),
    }
}

// ---------------- setup ----------------

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "열기", true, None::<&str>)?;
    let check = MenuItem::with_id(app, "check", "지금 Canvas 확인", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "종료", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &check, &sep, &quit])?;
    let mut builder = TrayIconBuilder::with_id("main-tray").tooltip("Canvas Assistant").menu(&menu).show_menu_on_left_click(false);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "check" => {
                let state = app.state::<Arc<AppState>>();
                if let Ok(db) = state.db.lock() {
                    for k in ["deadline_alert", "new_announcements", "grade_updates"] {
                        db.force_job_due(k);
                    }
                }
                if let Some(tx) = state.scheduler.lock().ok().and_then(|g| g.clone()) {
                    let _ = tx.send(scheduler::Signal::Wake);
                }
            }
            "quit" => {
                app.state::<Arc<AppState>>().codex.stop();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let database = db::Db::open(&data_dir).map_err(|e| Box::<dyn std::error::Error>::from(e))?;
            database.recover_stuck();
            let state = Arc::new(AppState {
                data_dir,
                codex: Arc::new(codex::CodexState::default()),
                db: Mutex::new(database),
                scheduler: Mutex::new(None),
            });
            app.manage(state.clone());
            build_tray(app.handle())?;
            let tx = scheduler::spawn(app.handle().clone());
            if let Ok(mut slot) = state.scheduler.lock() {
                *slot = Some(tx);
            }
            if std::env::args().any(|a| a == "--minimized") {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<Arc<AppState>>();
                if config::load(&state.data_dir).close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                    let key = "tray-hint";
                    let first = state.db.lock().map(|d| d.first_time(key)).unwrap_or(false);
                    if first {
                        scheduler::notify(window.app_handle(), "Canvas Assistant가 트레이에서 실행 중입니다", "알림과 예약 작업이 계속 동작합니다. 완전히 끄려면 트레이 아이콘에서 '종료'를 누르세요.");
                    }
                } else {
                    state.codex.stop();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_info, settings_save, check_update,
            canvas_connect, canvas_disconnect, canvas_query, canvas_download,
            pending_list, pending_decide, audit_list,
            jobs_list, jobs_update, jobs_run_now, briefings_list,
            codex_start, codex_call, codex_locate
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Canvas Assistant")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                app.state::<Arc<AppState>>().codex.stop();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::version_newer;

    #[test]
    fn compares_versions() {
        assert!(version_newer("0.2.0", "0.1.0"));
        assert!(version_newer("0.1.0", "0.1.0-beta.1"));
        assert!(version_newer("0.1.0-beta.2", "0.1.0-beta.1"));
        assert!(!version_newer("0.1.0", "0.1.0"));
        assert!(!version_newer("0.1.0-beta.1", "0.1.0"));
        assert!(!version_newer("0.0.9", "0.1.0"));
    }
}
