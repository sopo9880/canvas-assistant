//! An intentionally narrow, local-only Codex app-server stdio bridge.
//! No shell execution or arbitrary RPC surface is exposed to the WebView.
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State};

type Pending = HashMap<u64, mpsc::Sender<Result<Value, String>>>;

struct Running {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    pending: Arc<Mutex<Pending>>,
}

#[derive(Default)]
struct CodexState {
    current: Mutex<Option<Running>>,
    sequence: AtomicU64,
}

impl CodexState {
    fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let (sender, receiver) = mpsc::channel();
        let id = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let pending;
        {
            let mut guard = self.current.lock().map_err(|_| "Codex state is unavailable")?;
            let running = guard.as_mut().ok_or("Codex app-server is not started")?;
            if running.child.try_wait().map_err(|e| e.to_string())?.is_some() {
                return Err("Codex app-server has exited; reconnect in Settings".into());
            }
            pending = Arc::clone(&running.pending);
            pending.lock().map_err(|_| "Codex pending map is unavailable")?.insert(id, sender);
            let request = json!({"id": id, "method": method, "params": params});
            let result = running.stdin.lock().map_err(|_| "Codex input unavailable".to_string())?
                .write_all(format!("{request}\n").as_bytes());
            if result.is_err() {
                pending.lock().map_err(|_| "Codex request failed")?.remove(&id);
                return Err("Could not send a request to Codex app-server".into());
            }
        }

        match receiver.recv_timeout(Duration::from_secs(30)) {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(message)) => Err(message),
            Err(_) => {
                if let Ok(mut map) = pending.lock() {
                    map.remove(&id);
                }
                Err("Codex request timed out; check your connection".into())
            }
        }
    }

    fn stop(&self) {
        if let Ok(mut guard) = self.current.lock() {
            if let Some(mut running) = guard.take() {
                let _ = running.child.kill();
                let _ = running.child.wait();
            }
        }
    }
}

fn safe_server_event(method: &str) -> bool {
    matches!(
        method,
        "account/updated"
            | "account/login/completed"
            | "item/agentMessage/delta"
            | "item/completed"
            | "turn/started"
            | "turn/completed"
            | "turn/failed"
            | "thread/started"
            | "thread/name/updated"
    )
}

#[tauri::command]
fn codex_start(app: AppHandle, state: State<'_, CodexState>) -> Result<(), String> {
    {
        let mut current = state.current.lock().map_err(|_| "Codex state is unavailable")?;
        if let Some(proc) = current.as_mut() {
            if proc.child.try_wait().map_err(|e| e.to_string())?.is_none() {
                return Ok(());
            }
        }
        *current = None;

        let cwd = app.path().app_data_dir().map_err(|e| e.to_string())?.join("codex-workspace");
        std::fs::create_dir_all(&cwd).map_err(|e| format!("Could not prepare Codex workspace: {e}"))?;
        let mut child = Command::new("codex")
            .arg("app-server")
            .current_dir(&cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| {
                "Codex CLI was not found. Install the official Codex CLI and put 'codex' on PATH, then restart this app."
                    .to_string()
            })?;
        let stdin = Arc::new(Mutex::new(child.stdin.take().ok_or("Codex stdin unavailable")?));
        let stdout = child.stdout.take().ok_or("Codex stdout unavailable")?;
        let pending: Arc<Mutex<Pending>> = Arc::new(Mutex::new(HashMap::new()));
        let reader_stdin = Arc::clone(&stdin);
        let reader_pending = Arc::clone(&pending);
        let emit_app = app.clone();

        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
                let id = message.get("id");
                let method = message.get("method").and_then(Value::as_str);
                if let (Some(request_id), Some(name)) = (id, method) {
                    // The embedded chat never grants host-side tools, command or file permissions.
                    let answer = if name.ends_with("requestApproval") {
                        json!({"id": request_id, "result": {"decision": "decline"}})
                    } else {
                        json!({"id": request_id, "error": {"code": -32601, "message": "Host action unavailable in Canvas Assistant"}})
                    };
                    if let Ok(mut input) = reader_stdin.lock() {
                        let _ = input.write_all(format!("{answer}\n").as_bytes());
                    }
                } else if let Some(number) = id.and_then(Value::as_u64) {
                    if let Ok(mut map) = reader_pending.lock() {
                        if let Some(sender) = map.remove(&number) {
                            let response = if let Some(error) = message.get("error") {
                                Err(error.get("message").and_then(Value::as_str)
                                    .unwrap_or("Codex request failed").to_string())
                            } else {
                                Ok(message.get("result").cloned().unwrap_or(Value::Null))
                            };
                            let _ = sender.send(response);
                        }
                    }
                } else if let Some(name) = method {
                    if safe_server_event(name) {
                        let _ = emit_app.emit("codex-event", &message);
                    }
                }
            }
            if let Ok(mut map) = reader_pending.lock() {
                for (_, sender) in map.drain() {
                    let _ = sender.send(Err("Codex app-server disconnected".into()));
                }
            }
            let _ = emit_app.emit("codex-disconnected", ());
        });

        *current = Some(Running { child, stdin, pending });
    }

    let initialized = state.request("initialize", json!({
        "clientInfo": {
            "name": "canvas_assistant",
            "title": "Canvas Assistant",
            "version": "0.1.0"
        }
    }));
    match initialized {
        Ok(_) => {
            // Protocol handshake must complete before any other method.
            let running = state.current.lock().map_err(|_| "Codex state unavailable")?;
            if let Some(running) = running.as_ref() {
                running.stdin.lock().map_err(|_| "Codex input unavailable")?
                    .write_all(b"{\"method\":\"initialized\",\"params\":{}}\n")
                    .map_err(|_| "Failed to acknowledge Codex initialization")?;
            }
            Ok(())
        }
        Err(error) => {
            state.stop();
            Err(error)
        }
    }
}

fn safe_identifier(v: &Value, key: &str) -> Result<String, String> {
    let id = v.get(key).and_then(Value::as_str).ok_or(format!("{key} is required"))?;
    if id.is_empty() || id.len() > 256 || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
        return Err(format!("Invalid {key}"));
    }
    Ok(id.to_string())
}

fn safe_model(v: &Value) -> Result<Option<String>, String> {
    let Some(model) = v.get("model").and_then(Value::as_str) else { return Ok(None) };
    if model.len() > 100 || model.is_empty() ||
        !model.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_.:/".contains(&c)) {
        return Err("Invalid model identifier".into());
    }
    Ok(Some(model.to_owned()))
}

fn validated_request(method: &str, input: &Value) -> Result<Value, String> {
    match method {
        "account/read" | "account/logout" => Ok(json!({})),
        "account/login/start" => Ok(json!({"type": "chatgptDeviceCode"})),
        "model/list" => Ok(json!({"limit": 100, "includeHidden": false})),
        "thread/list" => Ok(json!({
            "limit": 50, "sortKey": "updated_at", "sourceKinds": ["appServer"]
        })),
        "thread/start" => {
            let mut params = json!({"approvalPolicy": "never", "sandbox": "readOnly"});
            if let Some(model) = safe_model(input)? {
                params["model"] = Value::String(model);
            }
            Ok(params)
        }
        "thread/resume" => Ok(json!({
            "threadId": safe_identifier(input, "threadId")?,
            "excludeTurns": true
        })),
        "thread/turns/list" => Ok(json!({
            "threadId": safe_identifier(input, "threadId")?,
            "limit": 60, "sortDirection": "desc", "itemsView": "full"
        })),
        "turn/start" => {
            let text = input.get("text").and_then(Value::as_str)
                .ok_or("Message text is required")?.trim();
            if text.is_empty() || text.len() > 20_000 {
                return Err("Message must be 1 to 20,000 bytes".into());
            }
            let effort = input.get("effort").and_then(Value::as_str).unwrap_or("medium");
            if !matches!(effort, "minimal" | "low" | "medium" | "high" | "xhigh") {
                return Err("Unsupported reasoning effort".into());
            }
            let mut params = json!({
                "threadId": safe_identifier(input, "threadId")?,
                "input": [{"type": "text", "text": text}],
                "approvalPolicy": "never",
                "sandboxPolicy": {"type": "readOnly"},
                "effort": effort
            });
            if let Some(model) = safe_model(input)? {
                params["model"] = Value::String(model);
            }
            Ok(params)
        }
        "turn/interrupt" => Ok(json!({
            "threadId": safe_identifier(input, "threadId")?,
            "turnId": safe_identifier(input, "turnId")?
        })),
        _ => Err("This Codex operation is not exposed by Canvas Assistant".into()),
    }
}

#[tauri::command]
fn codex_call(state: State<'_, CodexState>, method: String, params: Value) -> Result<Value, String> {
    let safe = validated_request(&method, &params)?;
    state.request(&method, safe)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(CodexState::default())
        .invoke_handler(tauri::generate_handler![codex_start, codex_call])
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                window.state::<CodexState>().stop();
            }
        })
        .run(tauri::generate_context!())
        .expect("failed to run Canvas Assistant");
}
