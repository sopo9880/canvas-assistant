//! A deliberately narrow bridge to `codex app-server` (JSON-RPC over stdio).
//!
//! The WebView can only call the whitelisted methods below with parameters the
//! bridge builds itself. Every thread runs read-only with approvals disabled;
//! command, file and permission requests from Codex are always declined. The
//! only tools Codex gains are the Canvas MCP tools served by this executable.
use crate::config;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

type Pending = HashMap<u64, mpsc::Sender<Result<Value, String>>>;
type Watchers = HashMap<String, mpsc::Sender<Value>>;

struct Running {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    pending: Arc<Mutex<Pending>>,
}

#[derive(Default)]
pub struct CodexState {
    current: Mutex<Option<Running>>,
    sequence: AtomicU64,
    watchers: Arc<Mutex<Watchers>>,
    start_lock: Mutex<()>,
}

/// Notifications forwarded to the WebView.
fn forwarded_event(method: &str) -> bool {
    matches!(
        method,
        "error"
            | "account/updated"
            | "account/login/completed"
            | "account/rateLimits/updated"
            | "item/agentMessage/delta"
            | "item/reasoning/summaryTextDelta"
            | "item/started"
            | "item/completed"
            | "turn/started"
            | "turn/completed"
            | "thread/started"
            | "thread/name/updated"
            | "thread/archived"
            | "thread/status/changed"
            | "mcpServer/startupStatus/updated"
            | "model/rerouted"
    )
}

/// Response for requests Codex sends to us. Nothing is ever approved.
fn answer_server_request(id: &Value, method: &str) -> Value {
    match method {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            json!({"id": id, "result": {"decision": "decline"}})
        }
        "mcpServer/elicitation/request" => json!({"id": id, "result": {"action": "decline"}}),
        "applyPatchApproval" | "execCommandApproval" => json!({"id": id, "result": {"decision": "denied"}}),
        _ => json!({"id": id, "error": {"code": -32601, "message": "Canvas Assistant does not allow this action"}}),
    }
}

/// Locates the Codex executable. npm installs `codex.cmd`, which Rust cannot spawn
/// directly, so the native binary inside the npm package is preferred.
/// Where the Codex executable came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodexSource {
    Custom,
    Bundled,
    System,
}

/// The codex.exe shipped inside the installer (resources/codex/codex.exe).
pub fn bundled_codex(app: &AppHandle) -> Option<PathBuf> {
    let exe = if cfg!(windows) { "codex.exe" } else { "codex" };
    app.path().resource_dir().ok().map(|d| d.join("codex").join(exe)).filter(|p| p.is_file())
}

/// Codex runs model tool calls in Code Mode through this helper, which it expects next to the
/// codex executable. Missing helper = every tool call the model makes fails.
pub fn code_mode_host_beside(program: &std::path::Path) -> bool {
    let name = if cfg!(windows) { "codex-code-mode-host.exe" } else { "codex-code-mode-host" };
    program.parent().is_some_and(|d| d.join(name).is_file())
}

/// Resolution order: a path the user set explicitly, the bundled binary, then PATH / npm installs.
pub fn locate_codex(custom: Option<&str>, bundled: Option<PathBuf>) -> Result<(PathBuf, Vec<String>, CodexSource), String> {
    if let Some(path) = custom.map(str::trim).filter(|p| !p.is_empty()) {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Ok((p, vec![], CodexSource::Custom));
        }
        return Err(format!("설정한 Codex 경로에 파일이 없습니다: {path}"));
    }
    if let Some(p) = bundled {
        return Ok((p, vec![], CodexSource::Bundled));
    }
    resolve_codex(None).map(|(p, a)| (p, a, CodexSource::System))
}

pub fn resolve_codex(custom: Option<&str>) -> Result<(PathBuf, Vec<String>), String> {
    if let Some(path) = custom.map(str::trim).filter(|p| !p.is_empty()) {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Ok((p, vec![]));
        }
        return Err(format!("설정한 Codex 경로에 파일이 없습니다: {path}"));
    }
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let dirs: Vec<PathBuf> = std::env::split_paths(&path_var).collect();
    let exe = if cfg!(windows) { "codex.exe" } else { "codex" };
    if let Some(found) = dirs.iter().map(|d| d.join(exe)).find(|p| p.is_file()) {
        return Ok((found, vec![]));
    }
    if cfg!(windows) {
        for dir in &dirs {
            if !dir.join("codex.cmd").is_file() {
                continue;
            }
            let arch = if cfg!(target_arch = "aarch64") { ("win32-arm64", "aarch64-pc-windows-msvc") } else { ("win32-x64", "x86_64-pc-windows-msvc") };
            let package = format!("codex-{}", arch.0);
            let candidates = [
                dir.join("node_modules/@openai/codex/node_modules/@openai").join(&package).join("vendor").join(arch.1).join("bin/codex.exe"),
                dir.join("node_modules/@openai").join(&package).join("vendor").join(arch.1).join("bin/codex.exe"),
                dir.join("node_modules/@openai/codex/vendor").join(arch.1).join("codex/codex.exe"),
            ];
            if let Some(found) = candidates.into_iter().find(|p| p.is_file()) {
                return Ok((found, vec![]));
            }
            let script = dir.join("node_modules/@openai/codex/bin/codex.js");
            if script.is_file() {
                if let Some(node) = dirs.iter().map(|d| d.join("node.exe")).find(|p| p.is_file()) {
                    return Ok((node, vec![script.to_string_lossy().into_owned()]));
                }
            }
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let p = PathBuf::from(local).join("Programs/OpenAI/Codex/bin/codex.exe");
            if p.is_file() {
                return Ok((p, vec![]));
            }
        }
    }
    Err("앱에 포함된 Codex를 찾을 수 없습니다. 설치 파일로 앱을 다시 설치하거나, 설정 → 고급에서 codex.exe 경로를 지정하세요.".into())
}

pub fn workspace_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("codex-workspace");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Codex 작업 폴더를 만들 수 없습니다: {e}"))?;
    Ok(dir)
}

/// `codex app-server` arguments: stdio transport, login stored in the OS keyring
/// (Windows Credential Manager) instead of auth.json, and no shell, sub-agents or web search.
pub fn app_server_args() -> Vec<String> {
    [
        "app-server", "--listen", "stdio://",
        "-c", "cli_auth_credentials_store=\"keyring\"",
        "-c", "features.shell_tool=false",
        "-c", "features.multi_agent=false",
        "-c", "web_search=\"disabled\"",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn developer_instructions() -> String {
    let now = chrono::Local::now();
    format!(
        "당신은 Canvas Assistant 안에서 동작하는 학습 비서입니다. 사용자는 대학생이며 한국어로 답하세요.\n\
         현재 시각: {} ({}).\n\
         \n\
         [도구 사용 원칙]\n\
         - 당신에게는 mcp__canvas 네임스페이스의 Canvas 도구가 있고, 지금 바로 호출할 수 있습니다. 강의·과제·공지·성적·제출물·파일·강의자료·일정에 관한 질문이면 답하기 전에 반드시 도구부터 호출하세요. 묻지 말고, 미리 '도구를 쓸 수 없다'고 판단하지 마세요.\n\
         - 필요한 만큼 여러 도구를 연달아 호출하세요. 강의 ID를 모르면 canvas_list_courses로 먼저 찾으세요.\n\
         - 도구가 실패하면 실패한 도구 이름과 오류 문장을 그대로 짧게 알려주고, 다른 도구로 계속 시도하세요 (예: canvas_list_files가 거부되면 canvas_learningx_resources, canvas_list_modules, canvas_api_get). 한 도구가 실패했다고 'Canvas 도구를 실행할 수 없다'고 일반화하지 마세요. 'Canvas가 연결되지 않았습니다'나 '토큰이 유효하지 않습니다' 오류일 때만 설정에서 Canvas 연결을 확인하라고 안내하세요.\n\
         - 예전 대화에서 도구가 실패했더라도 이번 요청에서는 다시 호출해 보세요.\n\
         \n\
         [자료와 제출물 읽기]\n\
         - 강의자료 내용이 필요하면 목록만 보지 말고 내용을 읽으세요. Canvas 파일은 canvas_read_file(file_id). '강의자료실'이 LearningX인 학교(목록이 비었거나 파일 메뉴가 거부됨)는 canvas_learningx_resources로 목록을 받고 canvas_learningx_read(content_id)로 읽으세요.\n\
         - 사용자가 '내가 냈던 코드/보고서'를 물으면 canvas_read_submission(course_id, assignment_id)으로 실제 제출 파일 내용을 읽으세요. 예전 시도는 attempt로 고릅니다. 과제를 모르면 canvas_list_submissions로 제출한 과제를 찾으세요.\n\
         - 긴 파일은 next_offset이 오면 필요한 만큼 이어서 읽으세요.\n\
         \n\
         [첨부 파일]\n\
         - 사용자가 메시지에 [첨부 파일] 목록을 붙이면 read_attachment(id)로 내용을 읽고 답하세요. 이미지는 메시지에 이미지로 들어 있으니 직접 보고 답하세요.\n\
         - 대화 중간에 사용자가 메시지를 추가로 보낼 수 있습니다. 새 메시지가 오면 그 내용을 반영해 이어서 작업하세요.\n\
         \n\
         [파일 만들기]\n\
         - 사용자가 PDF·Word·ZIP·코드·CSV 같은 파일을 원하거나, 정리한 내용이 길어 파일로 주는 게 나으면 make_file 도구로 만드세요. PDF/DOCX는 Markdown(#, 목록, ``` 코드, | 표 |)으로 내용을 쓰고, ZIP은 files 목록으로 여러 파일을 넣습니다.\n\
         - 만든 뒤에는 대화에 '열기'·'폴더에서 보기' 버튼이 나오므로, 파일 이름과 들어 있는 내용만 짧게 알려주세요. 파일 내용 전체를 대화에 다시 붙여넣지 마세요.\n\
         \n\
         [답변]\n\
         - 날짜는 사용자의 현지 시간으로 바꾸고, 마감까지 남은 시간도 함께 알려주세요.\n\
         - '주차학습', '이번 주 들을 강의', '출석/진도', '안 들은 강의'를 물으면 canvas_learningx_weekly로 주차별 강의·기한·완료·출결·할 일을 읽고 이번 주 기준으로 정리하세요. 주차 안에 들어 있는 과제는 assignments_in_weeks로 찾고, 상세는 canvas_get_assignment로 확인하세요. 이 도구는 읽기만 하며, 영상을 재생하거나 출석을 대신 처리하지 않습니다. 필드 이름이 학교마다 다르니 값의 의미가 불확실하면 추측하지 말고 불확실하다고 말하세요.\n\
         - 과제 분석을 요청받으면: canvas_planner나 canvas_upcoming으로 대상을 찾고, 과제마다 canvas_get_assignment(설명·루브릭·허용 형식·제출 횟수), canvas_assignment_groups(배점 비중), 관련 강의자료(canvas_read_file / canvas_learningx_read), 필요하면 canvas_read_submission(이전 제출)과 canvas_syllabus를 확인하세요. 마감 순서로 표를 만들고, 과제별로 해야 할 일, 채점 기준, 제출 방식, 성적 비중을 정리하세요.\n\
         - 사용자가 메시지에 [첨부된 Canvas 항목]이나 [도구 결과]를 붙이면 그 ID와 데이터를 우선 사용하세요.\n\
         - propose_ 로 시작하는 도구는 Canvas를 바로 바꾸지 않고 '승인 대기' 목록에 요청을 올립니다. 제안한 뒤에는 앱에서 승인해야 실행된다고 반드시 안내하세요. 제출은 propose_submit_files / propose_submit_url / propose_submit_text를 쓰고, 파일은 사용자가 승인 카드에서 직접 첨부합니다.\n\
         - 이 환경은 읽기 전용이라 셸 명령이나 PC 파일 수정은 할 수 없습니다. Canvas 도구와 make_file(새 파일 만들기)은 쓸 수 있습니다.",
        now.format("%Y-%m-%d %H:%M (%A)"),
        now.format("UTC%:z"),
    )
}

/// Folder for files the AI makes (make_file): Downloads/Canvas Assistant/AI 생성.
pub fn generated_dir(app: &AppHandle) -> Result<PathBuf, String> {
    match app.path().download_dir() {
        Ok(d) => Ok(d.join("Canvas Assistant").join(crate::generate::FOLDER)),
        Err(_) => Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join(crate::generate::FOLDER)),
    }
}

/// Config overrides that register this executable as the `canvas` MCP server.
fn canvas_mcp_config(app: &AppHandle) -> Result<Value, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let out = generated_dir(app)?;
    Ok(json!({
        "mcp_servers.canvas": {
            "command": exe.to_string_lossy(),
            "args": ["--mcp-canvas", "--data-dir", data.to_string_lossy(), "--output-dir", out.to_string_lossy()],
            "startup_timeout_sec": 20,
            "tool_timeout_sec": 240,
            // Proposals only queue a request; real Canvas changes need in-app approval.
            "default_tools_approval_mode": "approve"
        }
    }))
}

fn safe_identifier(v: &Value, key: &str) -> Result<String, String> {
    let id = v.get(key).and_then(Value::as_str).ok_or(format!("{key} is required"))?;
    if id.is_empty() || id.len() > 256 || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
        return Err(format!("Invalid {key}"));
    }
    Ok(id.to_string())
}

fn safe_model(v: &Value) -> Result<Option<String>, String> {
    let Some(model) = v.get("model").and_then(Value::as_str).filter(|m| !m.is_empty()) else { return Ok(None) };
    if model.len() > 100 || !model.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_.:/".contains(&c)) {
        return Err("Invalid model identifier".into());
    }
    Ok(Some(model.to_owned()))
}

fn safe_effort(v: &Value) -> Result<Option<String>, String> {
    let Some(effort) = v.get("effort").and_then(Value::as_str).filter(|e| !e.is_empty()) else { return Ok(None) };
    if effort.len() > 20 || !effort.bytes().all(|c| c.is_ascii_lowercase()) {
        return Err("Unsupported reasoning effort".into());
    }
    Ok(Some(effort.to_owned()))
}

/// Builds the parameters actually sent to Codex. Anything not listed is refused.
pub fn validated_request(method: &str, input: &Value, workspace: &str, mcp: Option<&Value>) -> Result<Value, String> {
    validated_request_with_images(method, input, workspace, mcp, &[])
}

/// Message input: the text plus images the user attached. Image paths are resolved by the
/// caller from attachment ids (never taken from the UI as paths).
fn message_input(input: &Value, images: &[PathBuf]) -> Result<Value, String> {
    let text = input.get("text").and_then(Value::as_str).unwrap_or("").trim();
    if (text.is_empty() && images.is_empty()) || text.len() > 40_000 {
        return Err("메시지는 1~40,000바이트여야 합니다".into());
    }
    let mut items = Vec::new();
    if !text.is_empty() {
        items.push(json!({"type": "text", "text": text}));
    }
    for p in images.iter().take(10) {
        items.push(json!({"type": "localImage", "path": p.to_string_lossy()}));
    }
    Ok(json!(items))
}

pub fn validated_request_with_images(method: &str, input: &Value, workspace: &str, mcp: Option<&Value>, images: &[PathBuf]) -> Result<Value, String> {
    match method {
        "account/read" => Ok(json!({"refreshToken": false})),
        "account/logout" => Ok(json!({})),
        "account/login/start" => match input.get("type").and_then(Value::as_str).unwrap_or("chatgpt") {
            "chatgpt" => Ok(json!({"type": "chatgpt"})),
            "chatgptDeviceCode" => Ok(json!({"type": "chatgptDeviceCode"})),
            _ => Err("Unsupported login type".into()),
        },
        "account/login/cancel" => Ok(json!({"loginId": safe_identifier(input, "loginId")?})),
        "account/rateLimits/read" => Ok(json!({})),
        "model/list" => Ok(json!({"limit": 100, "includeHidden": false})),
        "thread/list" => {
            let mut p = json!({"limit": 50, "sortKey": "updated_at", "sortDirection": "desc", "archived": false});
            // Normally scoped to this app's workspace folder. "byOriginator" lists everything so the
            // UI can filter on originator == canvas_assistant if Codex normalized the folder path.
            if input.get("scope").and_then(Value::as_str) != Some("byOriginator") {
                p["cwd"] = json!(workspace);
            }
            if let Some(c) = input.get("cursor").and_then(Value::as_str).filter(|c| c.len() < 512) {
                p["cursor"] = json!(c);
            }
            Ok(p)
        }
        "thread/start" => {
            let mut params = json!({
                "approvalPolicy": "never",
                "sandbox": "read-only",
                "cwd": workspace,
                "developerInstructions": developer_instructions(),
                "serviceName": "canvas_assistant"
            });
            if let Some(model) = safe_model(input)? {
                params["model"] = json!(model);
            }
            if let Some(cfg) = mcp {
                params["config"] = cfg.clone();
            }
            if input.get("ephemeral").and_then(Value::as_bool) == Some(true) {
                params["ephemeral"] = json!(true);
            }
            Ok(params)
        }
        "thread/resume" => {
            let mut params = json!({
                "threadId": safe_identifier(input, "threadId")?,
                "excludeTurns": true,
                "approvalPolicy": "never",
                "sandbox": "read-only",
                "developerInstructions": developer_instructions()
            });
            if let Some(cfg) = mcp {
                params["config"] = cfg.clone();
            }
            Ok(params)
        }
        "thread/turns/list" => Ok(json!({
            "threadId": safe_identifier(input, "threadId")?,
            "limit": 60, "sortDirection": "desc", "itemsView": "full"
        })),
        "thread/name/set" => {
            let name = input.get("name").and_then(Value::as_str).map(str::trim).unwrap_or("");
            if name.is_empty() || name.chars().count() > 80 {
                return Err("대화 이름은 1~80자여야 합니다".into());
            }
            Ok(json!({"threadId": safe_identifier(input, "threadId")?, "name": name}))
        }
        "thread/archive" | "thread/delete" => Ok(json!({"threadId": safe_identifier(input, "threadId")?})),
        "turn/start" => {
            let mut params = json!({
                "threadId": safe_identifier(input, "threadId")?,
                "input": message_input(input, images)?,
            });
            if let Some(model) = safe_model(input)? {
                params["model"] = json!(model);
            }
            if let Some(effort) = safe_effort(input)? {
                params["effort"] = json!(effort);
            }
            Ok(params)
        }
        // A message sent while the AI is still working joins the running turn.
        "turn/steer" => Ok(json!({
            "threadId": safe_identifier(input, "threadId")?,
            "expectedTurnId": safe_identifier(input, "expectedTurnId")?,
            "input": message_input(input, images)?,
        })),
        // Editing an earlier message: drop that turn and everything after it from the history.
        "thread/revert" => Ok(json!({
            "threadId": safe_identifier(input, "threadId")?,
            "beforeTurnId": safe_identifier(input, "beforeTurnId")?,
        })),
        "turn/interrupt" => Ok(json!({
            "threadId": safe_identifier(input, "threadId")?,
            "turnId": safe_identifier(input, "turnId")?
        })),
        _ => Err("This Codex operation is not exposed by Canvas Assistant".into()),
    }
}

impl CodexState {
    fn request_raw(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let (sender, receiver) = mpsc::channel();
        let id = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let pending;
        {
            let mut guard = self.current.lock().map_err(|_| "Codex state is unavailable")?;
            let running = guard.as_mut().ok_or("Codex가 아직 연결되지 않았습니다")?;
            if running.child.try_wait().map_err(|e| e.to_string())?.is_some() {
                *guard = None;
                return Err("Codex가 종료되었습니다. 다시 연결하세요.".into());
            }
            pending = Arc::clone(&running.pending);
            pending.lock().map_err(|_| "Codex pending map is unavailable")?.insert(id, sender);
            let request = json!({"id": id, "method": method, "params": params});
            let result = running.stdin.lock().map_err(|_| "Codex input unavailable".to_string())?.write_all(format!("{request}\n").as_bytes());
            if result.is_err() {
                pending.lock().map_err(|_| "Codex request failed")?.remove(&id);
                return Err("Codex에 요청을 보낼 수 없습니다".into());
            }
        }
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(_) => {
                if let Ok(mut map) = pending.lock() {
                    map.remove(&id);
                }
                Err("Codex 응답 시간이 초과되었습니다".into())
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.current.lock().ok().and_then(|mut g| g.as_mut().map(|r| matches!(r.child.try_wait(), Ok(None)))).unwrap_or(false)
    }

    pub fn stop(&self) {
        if let Ok(mut guard) = self.current.lock() {
            if let Some(mut running) = guard.take() {
                let _ = running.child.kill();
                let _ = running.child.wait();
            }
        }
    }

    pub fn start(&self, app: &AppHandle) -> Result<(), String> {
        let _serial = self.start_lock.lock().map_err(|_| "Codex state is unavailable")?;
        if self.is_running() {
            return Ok(());
        }
        self.stop();
        let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let settings = config::load(&data);
        let (program, mut args, _) = locate_codex(settings.codex_path.as_deref(), bundled_codex(app))?;
        args.extend(app_server_args());
        let cwd = workspace_dir(app)?;
        // Canvas Assistant keeps its own Codex home, separate from any Codex CLI/app on this PC.
        let home = data.join("codex");
        std::fs::create_dir_all(&home).map_err(|e| format!("Codex 폴더를 만들 수 없습니다: {e}"))?;
        let mut command = Command::new(&program);
        command
            .args(&args)
            .current_dir(&cwd)
            .env("CODEX_HOME", &home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = command.spawn().map_err(|e| format!("Codex를 실행할 수 없습니다 ({}): {e}", program.display()))?;
        let stdin = Arc::new(Mutex::new(child.stdin.take().ok_or("Codex stdin unavailable")?));
        let stdout = child.stdout.take().ok_or("Codex stdout unavailable")?;
        let pending: Arc<Mutex<Pending>> = Arc::new(Mutex::new(HashMap::new()));
        let reader_stdin = Arc::clone(&stdin);
        let reader_pending = Arc::clone(&pending);
        let watchers = Arc::clone(&self.watchers);
        let emit_app = app.clone();

        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
                let id = message.get("id");
                let method = message.get("method").and_then(Value::as_str);
                match (id, method) {
                    (Some(request_id), Some(name)) => {
                        let answer = answer_server_request(request_id, name);
                        if let Ok(mut input) = reader_stdin.lock() {
                            let _ = input.write_all(format!("{answer}\n").as_bytes());
                        }
                    }
                    (Some(id), None) => {
                        let Some(number) = id.as_u64() else { continue };
                        if let Some(sender) = reader_pending.lock().ok().and_then(|mut m| m.remove(&number)) {
                            let response = match message.get("error") {
                                Some(error) => Err(error.get("message").and_then(Value::as_str).unwrap_or("Codex request failed").to_string()),
                                None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                            };
                            let _ = sender.send(response);
                        }
                    }
                    (None, Some(name)) => {
                        let thread = message.pointer("/params/threadId").and_then(Value::as_str);
                        if let Some(thread) = thread {
                            if let Some(tx) = watchers.lock().ok().and_then(|w| w.get(thread).cloned()) {
                                let _ = tx.send(message.clone());
                                continue; // headless runs are not shown in the chat UI
                            }
                        }
                        if forwarded_event(name) {
                            let _ = emit_app.emit("codex-event", &message);
                        }
                    }
                    _ => {}
                }
            }
            if let Ok(mut map) = reader_pending.lock() {
                for (_, sender) in map.drain() {
                    let _ = sender.send(Err("Codex 연결이 끊어졌습니다".into()));
                }
            }
            let _ = emit_app.emit("codex-disconnected", ());
        });

        if let Ok(mut current) = self.current.lock() {
            *current = Some(Running { child, stdin, pending });
        }
        let init = self.request_raw("initialize", json!({
            "clientInfo": {"name": "canvas_assistant", "title": "Canvas Assistant", "version": env!("CARGO_PKG_VERSION")}
        }), Duration::from_secs(30));
        match init {
            Ok(_) => {
                let guard = self.current.lock().map_err(|_| "Codex state unavailable")?;
                if let Some(running) = guard.as_ref() {
                    running.stdin.lock().map_err(|_| "Codex input unavailable")?
                        .write_all(b"{\"method\":\"initialized\",\"params\":{}}\n")
                        .map_err(|_| "Codex 초기화 확인을 보낼 수 없습니다")?;
                }
                Ok(())
            }
            Err(e) => {
                self.stop();
                Err(format!("Codex 초기화 실패: {e}"))
            }
        }
    }

    pub fn call(&self, app: &AppHandle, method: &str, params: &Value) -> Result<Value, String> {
        let workspace = workspace_dir(app)?.to_string_lossy().into_owned();
        let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let mcp = if matches!(method, "thread/start" | "thread/resume") && config::load(&data_dir).canvas_tools_in_chat {
            Some(canvas_mcp_config(app)?)
        } else {
            None
        };
        // Attached images: the UI sends attachment ids; only files in the attachment store qualify.
        let images: Vec<PathBuf> = if matches!(method, "turn/start" | "turn/steer") {
            params.get("images").and_then(Value::as_array).map(|ids| {
                ids.iter().filter_map(Value::as_str).take(10)
                    .map(|id| crate::attachments::path_for(&data_dir, id))
                    .collect::<Result<Vec<_>, _>>()
            }).transpose()?.unwrap_or_default()
                .into_iter().filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(crate::attachments::is_image)).collect()
        } else {
            Vec::new()
        };
        let safe = validated_request_with_images(method, params, &workspace, mcp.as_ref(), &images)?;
        let timeout = if method == "thread/start" || method == "thread/resume" { Duration::from_secs(60) } else { Duration::from_secs(30) };
        self.request_raw(method, safe, timeout)
    }

    /// Starts an ephemeral thread with the Canvas tools, reports their startup status and
    /// calls `canvas_profile` once. Nothing is sent to the model.
    pub fn diagnose(&self, app: &AppHandle) -> Result<Value, String> {
        self.start(app)?;
        let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let workspace = workspace_dir(app)?.to_string_lossy().into_owned();
        let mcp = canvas_mcp_config(app)?;
        let mut params = validated_request("thread/start", &json!({"ephemeral": true}), &workspace, Some(&mcp))?;
        params["ephemeral"] = json!(true);
        let started = self.request_raw("thread/start", params, Duration::from_secs(60))?;
        let thread_id = started.pointer("/thread/id").and_then(Value::as_str).ok_or("Codex가 대화 ID를 주지 않았습니다")?.to_string();
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut server = Value::Null;
        while Instant::now() < deadline {
            let list = self.request_raw("mcpServerStatus/list", json!({"threadId": thread_id, "serverName": "canvas"}), Duration::from_secs(30))?;
            server = list.pointer("/data/0").cloned().unwrap_or(Value::Null);
            let status = server.get("runtimeStatus").and_then(Value::as_str).unwrap_or("");
            if status != "starting" && !status.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(700));
        }
        let tools = server.get("tools").and_then(Value::as_object).map(|t| t.len()).unwrap_or(0);
        let call = if tools > 0 {
            self.request_raw("mcpServer/tool/call", json!({"threadId": thread_id, "server": "canvas", "tool": "canvas_profile", "arguments": {}}), Duration::from_secs(60))
                .map(|r| json!({"isError": r.get("isError"), "text": r.pointer("/content/0/text").and_then(Value::as_str).map(|t| t.chars().take(400).collect::<String>())}))
                .unwrap_or_else(|e| json!({"isError": true, "text": e}))
        } else {
            Value::Null
        };
        Ok(json!({
            "canvasToolsInChat": config::load(&data_dir).canvas_tools_in_chat,
            "canvasConfigured": config::load(&data_dir).canvas_base_url.is_some() && config::get_token().is_some(),
            "server": {
                "status": server.get("runtimeStatus"),
                "error": server.get("toolsError").or_else(|| server.get("error")),
                "tools": tools,
            },
            "profileCall": call,
            "codeModeHost": locate_codex(config::load(&data_dir).codex_path.as_deref(), bundled_codex(app)).map(|(p, _, _)| code_mode_host_beside(&p)).unwrap_or(false),
            "helper": std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        }))
    }

    /// Runs one prompt in an ephemeral thread and returns the final answer.
    /// Used by scheduled AI briefings; nothing is shown in the chat list.
    pub fn run_prompt(&self, app: &AppHandle, prompt: &str, max_wait: Duration) -> Result<String, String> {
        self.start(app)?;
        let account = self.call(app, "account/read", &json!({}))?;
        if account.get("account").map_or(true, Value::is_null) {
            return Err("Codex에 로그인되어 있지 않습니다".into());
        }
        let started = self.call(app, "thread/start", &json!({"ephemeral": true}))?;
        let thread_id = started.pointer("/thread/id").and_then(Value::as_str).ok_or("Codex가 대화 ID를 주지 않았습니다")?.to_string();
        let (tx, rx) = mpsc::channel();
        self.watchers.lock().map_err(|_| "watchers unavailable")?.insert(thread_id.clone(), tx);
        let cleanup = |s: &Self| {
            if let Ok(mut w) = s.watchers.lock() {
                w.remove(&thread_id);
            }
        };
        if let Err(e) = self.call(app, "turn/start", &json!({"threadId": thread_id, "text": prompt})) {
            cleanup(self);
            return Err(e);
        }
        let deadline = Instant::now() + max_wait;
        let mut answer = String::new();
        let outcome = loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break Err("AI 작업 시간이 초과되었습니다".to_string());
            }
            match rx.recv_timeout(left) {
                Ok(event) => match event.get("method").and_then(Value::as_str) {
                    Some("item/completed") => {
                        if event.pointer("/params/item/type").and_then(Value::as_str) == Some("agentMessage") {
                            if let Some(text) = event.pointer("/params/item/text").and_then(Value::as_str) {
                                answer = text.to_string();
                            }
                        }
                    }
                    Some("turn/completed") => {
                        let status = event.pointer("/params/turn/status").and_then(Value::as_str).unwrap_or("completed");
                        break if status == "completed" && !answer.is_empty() {
                            Ok(answer.clone())
                        } else {
                            let err = event.pointer("/params/turn/error/message").and_then(Value::as_str).unwrap_or("AI 응답을 받지 못했습니다");
                            Err(err.to_string())
                        };
                    }
                    _ => {}
                },
                Err(_) => break Err("AI 작업 시간이 초과되었습니다".to_string()),
            }
        };
        cleanup(self);
        outcome
    }
}

pub fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

pub fn path_display(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn steer_revert_and_images_are_validated() {
        let st = validated_request("turn/steer", &json!({"threadId": "t1", "expectedTurnId": "u1", "text": "추가"}), "/w", None).unwrap();
        assert_eq!(st, json!({"threadId": "t1", "expectedTurnId": "u1", "input": [{"type": "text", "text": "추가"}]}));
        assert!(validated_request("turn/steer", &json!({"threadId": "t1", "text": "x"}), "/w", None).is_err(), "expectedTurnId required");
        let rv = validated_request("thread/revert", &json!({"threadId": "t1", "beforeTurnId": "u2", "extra": 1}), "/w", None).unwrap();
        assert_eq!(rv, json!({"threadId": "t1", "beforeTurnId": "u2"}));
        assert!(validated_request("thread/revert", &json!({"threadId": "t1", "beforeTurnId": "../x"}), "/w", None).is_err());
        // Paths in the UI input are ignored; only resolved attachment images are sent.
        let p = validated_request("turn/start", &json!({"threadId": "t1", "text": "", "input": [{"type": "localImage", "path": "/etc/passwd"}]}), "/w", None);
        assert!(p.is_err());
        let img = PathBuf::from("/data/attachments/x/a.png");
        let p = validated_request_with_images("turn/start", &json!({"threadId": "t1", "text": ""}), "/w", None, &[img]).unwrap();
        assert_eq!(p["input"], json!([{"type": "localImage", "path": "/data/attachments/x/a.png"}]));
    }

    #[test]
    fn finds_code_mode_host_next_to_codex() {
        let dir = std::env::temp_dir().join(format!("ca-host-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join(if cfg!(windows) { "codex.exe" } else { "codex" });
        std::fs::write(&exe, b"x").unwrap();
        assert!(!code_mode_host_beside(&exe));
        std::fs::write(dir.join(if cfg!(windows) { "codex-code-mode-host.exe" } else { "codex-code-mode-host" }), b"x").unwrap();
        assert!(code_mode_host_beside(&exe));
        let _ = std::fs::remove_dir_all(dir);
    }

    use super::*;

    #[test]
    fn locate_prefers_custom_then_bundled() {
        let dir = std::env::temp_dir().join(format!("ca-codex-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bundled = dir.join("codex.exe");
        std::fs::write(&bundled, b"x").unwrap();
        let (p, _, src) = locate_codex(None, Some(bundled.clone())).unwrap();
        assert_eq!((p, src), (bundled.clone(), CodexSource::Bundled));
        let custom = dir.join("custom.exe");
        std::fs::write(&custom, b"x").unwrap();
        let (p, _, src) = locate_codex(Some(custom.to_str().unwrap()), Some(bundled)).unwrap();
        assert_eq!((p, src), (custom, CodexSource::Custom));
        assert!(locate_codex(Some("/nope/codex.exe"), None).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn app_server_is_locked_down() {
        let a = app_server_args().join(" ");
        assert!(a.starts_with("app-server --listen stdio://"));
        assert!(a.contains("cli_auth_credentials_store=\"keyring\""));
        assert!(a.contains("features.shell_tool=false"));
        assert!(a.contains("web_search=\"disabled\""));
    }

    #[test]
    fn whitelist_rejects_unknown_methods() {
        assert!(validated_request("command/exec", &json!({}), "/w", None).is_err());
        assert!(validated_request("config/value/write", &json!({}), "/w", None).is_err());
    }

    #[test]
    fn threads_are_read_only_and_scoped() {
        let mcp = json!({"mcp_servers.canvas": {"command": "x"}});
        let p = validated_request("thread/start", &json!({"model": "gpt-x", "sandbox": "danger-full-access", "approvalPolicy": "on-request"}), "/w", Some(&mcp)).unwrap();
        assert_eq!(p["sandbox"], "read-only");
        assert_eq!(p["approvalPolicy"], "never");
        assert_eq!(p["cwd"], "/w");
        assert_eq!(p["config"], mcp);
        let l = validated_request("thread/list", &json!({"cwd": "/etc"}), "/w", None).unwrap();
        assert_eq!(l["cwd"], "/w");
        let all = validated_request("thread/list", &json!({"scope": "byOriginator"}), "/w", None).unwrap();
        assert!(all.get("cwd").is_none());
    }

    #[test]
    fn turn_input_is_validated() {
        assert!(validated_request("turn/start", &json!({"threadId": "abc", "text": "  "}), "/w", None).is_err());
        assert!(validated_request("turn/start", &json!({"threadId": "../x", "text": "hi"}), "/w", None).is_err());
        assert!(validated_request("turn/start", &json!({"threadId": "abc", "text": "hi", "effort": "HIGH; rm"}), "/w", None).is_err());
        let ok = validated_request("turn/start", &json!({"threadId": "a-1", "text": "hi", "effort": "xhigh", "sandboxPolicy": {"type": "dangerFullAccess"}}), "/w", None).unwrap();
        assert!(ok.get("sandboxPolicy").is_none());
        assert_eq!(ok["effort"], "xhigh");
    }

    #[test]
    fn server_requests_are_never_approved() {
        let id = json!(7);
        assert_eq!(answer_server_request(&id, "item/commandExecution/requestApproval")["result"]["decision"], "decline");
        assert_eq!(answer_server_request(&id, "item/fileChange/requestApproval")["result"]["decision"], "decline");
        assert_eq!(answer_server_request(&id, "mcpServer/elicitation/request")["result"]["action"], "decline");
        assert!(answer_server_request(&id, "item/permissions/requestApproval").get("error").is_some());
        assert!(answer_server_request(&id, "item/tool/call").get("error").is_some());
    }
}
