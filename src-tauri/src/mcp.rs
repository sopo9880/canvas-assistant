//! Canvas MCP server spoken over stdio. Codex launches this same executable with
//! `--mcp-canvas --data-dir <dir>` and calls these tools during a chat.
//!
//! Read tools call the Canvas API directly. Tools that would change anything in
//! Canvas only *propose* the change: it is written to the local approval queue and
//! runs after the user approves it inside Canvas Assistant.
use crate::{canvas, config, db::Db};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

const RESULT_LIMIT: usize = 60_000;
const SUPPORTED_PROTOCOLS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

pub const INSTRUCTIONS: &str = "Canvas LMS tools for the signed-in student. Read tools return live data from the school's Canvas. \
Tools whose names start with propose_ never change Canvas directly: they add a request to the user's approval queue in Canvas Assistant, \
and the change happens only if the user approves it there. Always tell the user that approval is needed after proposing.";

fn schema(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
}

fn id_prop(desc: &str) -> Value {
    json!({"type": ["integer", "string"], "description": desc})
}

pub fn tool_list() -> Vec<Value> {
    let ro = json!({"readOnlyHint": true, "openWorldHint": true});
    let propose = json!({"readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false});
    let reason = json!({"type": "string", "description": "Why this change is being proposed, shown to the user (Korean)."});
    vec![
        json!({"name": "canvas_profile", "description": "The signed-in Canvas user's profile (name, time zone).", "inputSchema": schema(json!({}), &[]), "annotations": ro}),
        json!({"name": "canvas_list_courses", "description": "List the user's courses with current scores. Use the returned id as course_id for other tools.",
            "inputSchema": schema(json!({"include_past": {"type": "boolean", "description": "Include finished courses."}}), &[]), "annotations": ro}),
        json!({"name": "canvas_upcoming", "description": "Canvas to-do list and upcoming events/deadlines across all courses.", "inputSchema": schema(json!({}), &[]), "annotations": ro}),
        json!({"name": "canvas_list_assignments", "description": "Assignments of a course with due dates and the user's submission status.",
            "inputSchema": schema(json!({"course_id": id_prop("Course id"), "bucket": {"type": "string", "enum": ["past", "overdue", "undated", "ungraded", "unsubmitted", "upcoming", "future"]}}), &["course_id"]), "annotations": ro}),
        json!({"name": "canvas_get_assignment", "description": "Full details (description, rubric-free) of one assignment.",
            "inputSchema": schema(json!({"course_id": id_prop("Course id"), "assignment_id": id_prop("Assignment id")}), &["course_id", "assignment_id"]), "annotations": ro}),
        json!({"name": "canvas_list_announcements", "description": "Recent announcements. Omit course_ids to search every active course.",
            "inputSchema": schema(json!({"course_ids": {"type": "array", "items": {"type": ["integer", "string"]}}, "days_back": {"type": "integer", "minimum": 1, "maximum": 365}}), &[]), "annotations": ro}),
        json!({"name": "canvas_grades", "description": "Current and final scores for every active course.", "inputSchema": schema(json!({}), &[]), "annotations": ro}),
        json!({"name": "canvas_list_submissions", "description": "The user's submissions, scores and instructor comments in a course.",
            "inputSchema": schema(json!({"course_id": id_prop("Course id")}), &["course_id"]), "annotations": ro}),
        json!({"name": "canvas_list_files", "description": "Files in a course, newest first.",
            "inputSchema": schema(json!({"course_id": id_prop("Course id"), "search": {"type": "string"}}), &["course_id"]), "annotations": ro}),
        json!({"name": "canvas_list_modules", "description": "Modules and their items (pages, files, assignments) of a course.",
            "inputSchema": schema(json!({"course_id": id_prop("Course id")}), &["course_id"]), "annotations": ro}),
        json!({"name": "canvas_get_page", "description": "Read a course wiki page as text. page_url is the page slug from modules.",
            "inputSchema": schema(json!({"course_id": id_prop("Course id"), "page_url": {"type": "string"}}), &["course_id", "page_url"]), "annotations": ro}),
        json!({"name": "canvas_list_discussions", "description": "Discussion topics of a course.",
            "inputSchema": schema(json!({"course_id": id_prop("Course id")}), &["course_id"]), "annotations": ro}),
        json!({"name": "canvas_calendar", "description": "Calendar events and assignment due dates for the coming days across active courses.",
            "inputSchema": schema(json!({"days_ahead": {"type": "integer", "minimum": 1, "maximum": 120}}), &[]), "annotations": ro}),
        json!({"name": "propose_submit_text", "description": "Propose submitting a text answer to an assignment (online_text_entry). Requires user approval.",
            "inputSchema": schema(json!({"course_id": id_prop("Course id"), "assignment_id": id_prop("Assignment id"), "body": {"type": "string", "description": "HTML or plain text to submit"}, "reason": reason}), &["course_id", "assignment_id", "body", "reason"]), "annotations": propose}),
        json!({"name": "propose_submit_url", "description": "Propose submitting a website URL to an assignment (online_url). Requires user approval.",
            "inputSchema": schema(json!({"course_id": id_prop("Course id"), "assignment_id": id_prop("Assignment id"), "url": {"type": "string"}, "reason": reason}), &["course_id", "assignment_id", "url", "reason"]), "annotations": propose}),
        json!({"name": "propose_discussion_reply", "description": "Propose posting a reply to a discussion topic. Requires user approval.",
            "inputSchema": schema(json!({"course_id": id_prop("Course id"), "topic_id": id_prop("Discussion topic id"), "message": {"type": "string"}, "reason": reason}), &["course_id", "topic_id", "message", "reason"]), "annotations": propose}),
        json!({"name": "propose_planner_note", "description": "Propose adding a personal to-do note to the Canvas planner. Requires user approval.",
            "inputSchema": schema(json!({"title": {"type": "string"}, "todo_date": {"type": "string", "description": "ISO 8601 date-time"}, "details": {"type": "string"}, "course_id": id_prop("Optional course id"), "reason": reason}), &["title", "todo_date", "reason"]), "annotations": propose}),
        json!({"name": "propose_calendar_event", "description": "Propose adding an event to the user's personal Canvas calendar. Requires user approval.",
            "inputSchema": schema(json!({"title": {"type": "string"}, "start_at": {"type": "string", "description": "ISO 8601 date-time"}, "end_at": {"type": "string"}, "description": {"type": "string"}, "reason": reason}), &["title", "start_at", "reason"]), "annotations": propose}),
    ]
}

fn course_ids(args: &Value, client: &canvas::Client) -> Result<Vec<u64>, String> {
    match args.get("course_ids").and_then(Value::as_array) {
        Some(list) if !list.is_empty() => list.iter().map(|v| canvas::require_id(&json!({"id": v}), "id").map_err(String::from)).collect(),
        _ => Ok(client.courses(false)?.iter().filter_map(|c| canvas::require_id(c, "id").ok()).collect()),
    }
}

fn proposal_payload(kind: &str, args: &Value) -> Result<Value, String> {
    let mut payload = args.clone();
    if let Some(obj) = payload.as_object_mut() {
        obj.remove("reason");
    }
    // Validate the same way the executor will, so bad proposals fail early.
    match kind {
        "submit_text" | "submit_url" => {
            canvas::require_id(&payload, "course_id")?;
            canvas::require_id(&payload, "assignment_id")?;
        }
        "discussion_reply" => {
            canvas::require_id(&payload, "course_id")?;
            canvas::require_id(&payload, "topic_id")?;
        }
        _ => {}
    }
    if kind == "submit_url" {
        let ok = payload.get("url").and_then(Value::as_str).is_some_and(|u| u.starts_with("https://") || u.starts_with("http://"));
        if !ok {
            return Err("url은 http(s) 주소여야 합니다".into());
        }
    }
    Ok(payload)
}

/// Adds `_course_name` / `_assignment_name` / `_topic_title` display fields (best effort).
fn annotate_names(client: &canvas::Client, payload: &mut Value) {
    let Ok(course) = canvas::require_id(payload, "course_id") else { return };
    if let Ok(courses) = client.courses(true) {
        if let Some(name) = courses.iter().find(|c| canvas::require_id(c, "id").ok() == Some(course)).and_then(|c| c.get("name")).cloned() {
            payload["_course_name"] = name;
        }
    }
    if let Ok(assignment) = canvas::require_id(payload, "assignment_id") {
        if let Ok(a) = client.assignment(course, assignment) {
            payload["_assignment_name"] = a.get("name").cloned().unwrap_or(Value::Null);
            payload["_due_at"] = a.get("due_at").cloned().unwrap_or(Value::Null);
            payload["_submission_types"] = a.get("submission_types").cloned().unwrap_or(Value::Null);
        }
    }
    if let Ok(topic) = canvas::require_id(payload, "topic_id") {
        if let Ok(list) = client.discussions(course) {
            if let Some(t) = list.iter().find(|d| canvas::require_id(d, "id").ok() == Some(topic)).and_then(|d| d.get("title")).cloned() {
                payload["_topic_title"] = t;
            }
        }
    }
}

/// Read-only Canvas queries shared by the MCP tools and the app UI.
pub fn read_tool(client: &canvas::Client, name: &str, args: &Value) -> Result<Value, String> {
    let id = |k: &str| canvas::require_id(args, k).map_err(String::from);
    Ok(match name {
        "canvas_profile" => client.profile()?,
        "canvas_list_courses" => json!(client.courses(args.get("include_past").and_then(Value::as_bool).unwrap_or(false))?),
        "canvas_upcoming" => client.upcoming()?,
        "canvas_list_assignments" => json!(client.assignments(id("course_id")?, args.get("bucket").and_then(Value::as_str))?),
        "canvas_get_assignment" => client.assignment(id("course_id")?, id("assignment_id")?)?,
        "canvas_list_announcements" => {
            let ids = course_ids(args, client)?;
            json!(client.announcements(&ids, args.get("days_back").and_then(Value::as_i64).unwrap_or(14))?)
        }
        "canvas_grades" => json!(client.grades()?),
        "canvas_list_submissions" => json!(client.submissions(id("course_id")?)?),
        "canvas_list_files" => json!(client.files(id("course_id")?, args.get("search").and_then(Value::as_str))?),
        "canvas_list_modules" => json!(client.modules(id("course_id")?)?),
        "canvas_get_page" => client.page(id("course_id")?, args.get("page_url").and_then(Value::as_str).unwrap_or(""))?,
        "canvas_list_discussions" => json!(client.discussions(id("course_id")?)?),
        "canvas_calendar" => {
            let ids = course_ids(args, client)?;
            json!(client.calendar(&ids, args.get("days_ahead").and_then(Value::as_i64).unwrap_or(14))?)
        }
        _ => return Err(format!("unknown tool {name}")),
    })
}

/// Runs one tool. Returns (text, is_error).
pub fn call_tool(data_dir: &Path, name: &str, args: &Value) -> (String, bool) {
    let result: Result<Value, String> = (|| {
        if let Some(kind) = name.strip_prefix("propose_") {
            if !canvas::WRITE_KINDS.contains(&kind) {
                return Err(format!("unknown tool {name}"));
            }
            let mut payload = proposal_payload(kind, args)?;
            // Attach human-readable names so the user knows exactly what they approve.
            if let Ok(client) = config::canvas_client(data_dir) {
                annotate_names(&client, &mut payload);
            }
            let reason = args.get("reason").and_then(Value::as_str).unwrap_or("").chars().take(500).collect::<String>();
            let summary = canvas::describe_write(kind, &payload);
            let db = Db::open(data_dir)?;
            let id = db.add_pending(kind, &summary, &reason, &payload)?;
            db.audit("propose", &summary, true);
            return Ok(json!({
                "status": "pending_user_approval",
                "request_id": id,
                "summary": summary,
                "note": "Nothing has changed in Canvas yet. The user must approve this request in Canvas Assistant (승인 대기)."
            }));
        }
        let client = config::canvas_client(data_dir)?;
        read_tool(&client, name, args)
    })();
    match result {
        Ok(v) => {
            let mut text = serde_json::to_string(&v).unwrap_or_default();
            if text.len() > RESULT_LIMIT {
                let mut cut = RESULT_LIMIT;
                while !text.is_char_boundary(cut) {
                    cut -= 1;
                }
                text.truncate(cut);
                text.push_str("…(truncated: narrow the request, e.g. one course or a bucket)");
            }
            (text, false)
        }
        Err(e) => (e, true),
    }
}

pub fn handle(data_dir: &Path, msg: &Value) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("2025-06-18");
            let version = if SUPPORTED_PROTOCOLS.contains(&requested) { requested } else { SUPPORTED_PROTOCOLS[1] };
            Ok(json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "canvas", "title": "Canvas LMS", "version": env!("CARGO_PKG_VERSION")},
                "instructions": INSTRUCTIONS
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tool_list()})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let (text, is_error) = call_tool(data_dir, name, &args);
            Ok(json!({"content": [{"type": "text", "text": text}], "isError": is_error}))
        }
        "resources/list" => Ok(json!({"resources": []})),
        "prompts/list" => Ok(json!({"prompts": []})),
        _ => Err(json!({"code": -32601, "message": format!("Method not found: {method}")})),
    };
    Some(match result {
        Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
        Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
    })
}

pub fn data_dir_from_args(args: &[String]) -> Option<PathBuf> {
    args.iter().position(|a| a == "--data-dir").and_then(|i| args.get(i + 1)).map(PathBuf::from)
}

/// Entry point for `--mcp-canvas`.
pub fn run_stdio(data_dir: PathBuf) {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                let out: Vec<Value> = batch.iter().filter_map(|m| handle(&data_dir, m)).collect();
                (!out.is_empty()).then(|| Value::Array(out))
            }
            Ok(msg) => handle(&data_dir, &msg),
            Err(_) => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "Parse error"}})),
        };
        if let Some(r) = response {
            let mut out = stdout.lock();
            if writeln!(out, "{r}").is_err() || out.flush().is_err() {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("ca-mcp-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn initialize_and_list() {
        let d = dir();
        let r = handle(&d, &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}})).unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        let r = handle(&d, &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).unwrap();
        let tools = r["result"]["tools"].as_array().unwrap();
        assert!(tools.iter().any(|t| t["name"] == "canvas_upcoming"));
        for t in tools {
            let name = t["name"].as_str().unwrap();
            let read_only = t["annotations"]["readOnlyHint"].as_bool().unwrap();
            assert_eq!(read_only, !name.starts_with("propose_"), "{name}");
        }
        assert!(handle(&d, &json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn proposals_only_queue() {
        let d = dir();
        let (text, err) = call_tool(&d, "propose_planner_note", &json!({"title": "보고서", "todo_date": "2026-11-01T09:00:00+09:00", "reason": "테스트"}));
        assert!(!err, "{text}");
        assert!(text.contains("pending_user_approval"));
        let db = Db::open(&d).unwrap();
        let pending = db.list_pending(false).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0]["reason"], "테스트");
        assert!(pending[0]["payload"].get("reason").is_none());
        let (_, err) = call_tool(&d, "propose_submit_url", &json!({"course_id": 1, "assignment_id": 2, "url": "javascript:alert(1)", "reason": "x"}));
        assert!(err);
        let (_, err) = call_tool(&d, "propose_delete_everything", &json!({}));
        assert!(err);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn read_tools_require_configuration() {
        let d = dir();
        let (text, err) = call_tool(&d, "canvas_upcoming", &json!({}));
        assert!(err);
        assert!(text.contains("Canvas"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn parses_data_dir_argument() {
        let args: Vec<String> = ["app", "--mcp-canvas", "--data-dir", "C:\\x y"].iter().map(|s| s.to_string()).collect();
        assert_eq!(data_dir_from_args(&args), Some(PathBuf::from("C:\\x y")));
    }
}
