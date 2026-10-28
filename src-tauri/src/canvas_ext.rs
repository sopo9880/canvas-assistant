//! Additional Canvas endpoints for v0.2: course navigation, syllabus, planner,
//! rubrics, submission history, discussions, inbox, quizzes, groups, folders,
//! a guarded generic GET, file submissions and the remaining student writes.
use crate::canvas::{html_field, opt_str, pick, push_comment, require_id, same_origin, truncate, CanvasError, Client, Result};
use serde_json::{json, Map, Value};
use std::{io::Read, path::Path, time::Duration};

const MAX_UPLOAD: u64 = 500 * 1024 * 1024;

fn s(v: &Value, key: &str) -> String {
    v.get(key).map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string())).unwrap_or_default()
}

/// Validates a relative Canvas API path for the generic read tool.
pub fn safe_api_path(path: &str) -> Result<String> {
    let p = path.trim().trim_start_matches("/api/v1/").trim_start_matches("api/v1/").trim_start_matches('/');
    let p = p.split('?').next().unwrap_or("");
    if p.is_empty() || p.len() > 300 || p.contains("..") || p.contains("//")
        || !p.bytes().all(|c| c.is_ascii_alphanumeric() || b"/_-.".contains(&c))
    {
        return Err(CanvasError::Invalid("API 경로가 올바르지 않습니다 (예: courses/123/assignments)".into()));
    }
    Ok(p.to_string())
}

fn guess_mime(name: &str) -> &'static str {
    match Path::new(name).extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("pdf") => "application/pdf",
        Some("doc") => "application/msword",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("ppt") => "application/vnd.ms-powerpoint",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        Some("xls") => "application/vnd.ms-excel",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("hwp") => "application/x-hwp",
        Some("hwpx") => "application/hwp+zip",
        Some("zip") => "application/zip",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("txt") => "text/plain",
        Some("csv") => "text/csv",
        Some("py") => "text/x-python",
        Some("m") => "text/plain",
        Some("mp4") => "video/mp4",
        Some("mp3") => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

/// multipart/form-data body: every upload param as given, the file last.
pub fn multipart_body(params: &Map<String, Value>, file_name: &str, content_type: &str, data: &[u8], boundary: &str) -> Vec<u8> {
    let mut body = Vec::with_capacity(data.len() + 4096);
    for (k, v) in params {
        let value = v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
        body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    let safe_name = file_name.replace(['"', '\r', '\n'], "_");
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{safe_name}\"\r\nContent-Type: {content_type}\r\n\r\n").as_bytes());
    body.extend_from_slice(data);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

impl Client {
    fn put_form(&self, path: &str, fields: &[(String, String)]) -> Result<Value> {
        let (body, _) = self.send("PUT", &self.url(path), Some(fields))?;
        Ok(serde_json::from_str(&body).unwrap_or(Value::Null))
    }

    fn delete(&self, path: &str) -> Result<Value> {
        let (body, _) = self.send("DELETE", &self.url(path), Some(&[]))?;
        Ok(serde_json::from_str(&body).unwrap_or(Value::Null))
    }

    // ---------------- reads ----------------

    /// The course navigation exactly as the school configured it (names, order, hidden).
    pub fn tabs(&self, course: u64) -> Result<Vec<Value>> {
        let list = self.get_all(&format!("courses/{course}/tabs"), &[], 100)?;
        Ok(list.iter().filter(|t| t.get("hidden") != Some(&json!(true))).map(|t| pick(t, &["id", "label", "type", "position", "html_url", "full_url", "visibility"])).collect())
    }

    pub fn course_home(&self, course: u64) -> Result<Value> {
        let c = self.get(&format!("courses/{course}"), &[("include[]", "syllabus_body".to_string())])?;
        let mut out = pick(&c, &["id", "name", "course_code", "default_view", "start_at", "end_at"]);
        if let Ok(front) = self.get(&format!("courses/{course}/front_page"), &[]) {
            out["front_page"] = json!({"title": front.get("title"), "url": front.get("url"), "body": html_field(&front, "body")});
        }
        if let Ok(activity) = self.get_all(&format!("courses/{course}/activity_stream"), &[], 20) {
            out["recent_activity"] = json!(activity.iter().map(|a| {
                let mut x = pick(a, &["type", "title", "created_at", "updated_at", "html_url", "read_state"]);
                if let Some(m) = html_field(a, "message") {
                    x["message"] = json!(truncate(m, 400));
                }
                x
            }).collect::<Vec<_>>());
        }
        Ok(out)
    }

    pub fn syllabus(&self, course: u64) -> Result<Value> {
        let c = self.get(&format!("courses/{course}"), &[("include[]", "syllabus_body".to_string())])?;
        Ok(json!({"course": c.get("name"), "syllabus": html_field(&c, "syllabus_body").map(|t| truncate(t, 12000))}))
    }

    pub fn planner(&self, days_back: i64, days_ahead: i64) -> Result<Vec<Value>> {
        let (start, end) = crate::canvas::days_window(days_back.clamp(0, 60), days_ahead.clamp(1, 120));
        let list = self.get_all("planner/items", &[("start_date", start), ("end_date", end)], 300)?;
        Ok(list.iter().map(|i| {
            let mut x = pick(i, &["plannable_type", "plannable_id", "plannable_date", "course_id", "context_name", "html_url", "new_activity"]);
            if let Some(p) = i.get("plannable") {
                x["title"] = p.get("title").or_else(|| p.get("name")).cloned().unwrap_or(Value::Null);
                x["due_at"] = p.get("due_at").or_else(|| p.get("todo_date")).cloned().unwrap_or(Value::Null);
                x["points_possible"] = p.get("points_possible").cloned().unwrap_or(Value::Null);
            }
            if let Some(o) = i.get("planner_override").filter(|o| o.is_object()) {
                x["marked_complete"] = o.get("marked_complete").cloned().unwrap_or(json!(false));
                x["dismissed"] = o.get("dismissed").cloned().unwrap_or(json!(false));
            }
            if let Some(sub) = i.get("submissions").filter(|s| s.is_object()) {
                x["submission"] = pick(sub, &["submitted", "graded", "late", "missing", "needs_grading", "has_feedback", "excused"]);
            }
            x
        }).collect())
    }

    pub fn assignment_groups(&self, course: u64) -> Result<Vec<Value>> {
        let list = self.get_all(&format!("courses/{course}/assignment_groups"), &[
            ("include[]", "assignments".to_string()), ("include[]", "submission".to_string()),
        ], 100)?;
        Ok(list.iter().map(|g| {
            let mut x = pick(g, &["id", "name", "position", "group_weight"]);
            if let Some(a) = g.get("assignments").and_then(Value::as_array) {
                x["assignments"] = json!(a.iter().map(|a| {
                    let mut y = pick(a, &["id", "name", "points_possible", "due_at", "omit_from_final_grade"]);
                    if let Some(sub) = a.get("submission").filter(|s| s.is_object()) {
                        y["submission"] = pick(sub, &["id", "workflow_state", "score", "grade", "excused", "missing", "late", "student_entered_score"]);
                    }
                    y
                }).collect::<Vec<_>>());
            }
            x
        }).collect())
    }

    /// The user's own submission with comments, attempt history and rubric assessment.
    pub fn my_submission(&self, course: u64, assignment: u64) -> Result<Value> {
        let sub = self.get(&format!("courses/{course}/assignments/{assignment}/submissions/self"), &[
            ("include[]", "submission_comments".to_string()),
            ("include[]", "submission_history".to_string()),
            ("include[]", "rubric_assessment".to_string()),
        ])?;
        let mut out = pick(&sub, &["id", "workflow_state", "attempt", "score", "grade", "submitted_at", "graded_at", "late", "missing", "excused", "submission_type", "url", "preview_url", "seconds_late", "student_entered_score"]);
        if let Some(b) = html_field(&sub, "body") {
            out["body"] = json!(truncate(b, 4000));
        }
        if let Some(a) = sub.get("attachments").and_then(Value::as_array) {
            out["attachments"] = json!(a.iter().map(|f| pick(f, &["id", "display_name", "size", "content-type"])).collect::<Vec<_>>());
        }
        if let Some(c) = sub.get("submission_comments").and_then(Value::as_array) {
            out["comments"] = json!(c.iter().map(|x| pick(x, &["author_name", "comment", "created_at", "attempt"])).collect::<Vec<_>>());
        }
        if let Some(r) = sub.get("rubric_assessment") {
            out["rubric_assessment"] = r.clone();
        }
        if let Some(h) = sub.get("submission_history").and_then(Value::as_array) {
            out["history"] = json!(h.iter().map(|x| pick(x, &["attempt", "submitted_at", "submission_type", "score", "grade", "late"])).collect::<Vec<_>>());
        }
        Ok(out)
    }

    /// Full discussion thread with author names and nested replies.
    pub fn discussion_thread(&self, course: u64, topic: u64) -> Result<Value> {
        let t = self.get(&format!("courses/{course}/discussion_topics/{topic}"), &[])?;
        let mut out = pick(&t, &["id", "title", "posted_at", "html_url", "locked", "require_initial_post", "discussion_subentry_count"]);
        if let Some(m) = html_field(&t, "message") {
            out["message"] = json!(truncate(m, 6000));
        }
        out["author"] = t.pointer("/author/display_name").cloned().unwrap_or(Value::Null);
        match self.get(&format!("courses/{course}/discussion_topics/{topic}/view"), &[]) {
            Ok(view) => {
                let names: std::collections::HashMap<String, String> = view.get("participants").and_then(Value::as_array).into_iter().flatten()
                    .filter_map(|p| Some((s(p, "id"), p.get("display_name")?.as_str()?.to_string()))).collect();
                let mut count = 0usize;
                fn walk(entries: &[Value], names: &std::collections::HashMap<String, String>, count: &mut usize) -> Vec<Value> {
                    let mut out = Vec::new();
                    for e in entries {
                        if *count >= 200 {
                            break;
                        }
                        *count += 1;
                        let mut x = pick(e, &["id", "created_at", "rating_sum"]);
                        x["author"] = json!(names.get(&s(e, "user_id")).cloned().unwrap_or_default());
                        x["message"] = json!(e.get("message").and_then(Value::as_str).map(|m| truncate(crate::canvas::html_to_text(m), 1500)).unwrap_or_default());
                        if e.get("deleted") == Some(&json!(true)) {
                            x["deleted"] = json!(true);
                        }
                        if let Some(r) = e.get("replies").and_then(Value::as_array) {
                            x["replies"] = json!(walk(r, names, count));
                        }
                        out.push(x);
                    }
                    out
                }
                out["entries"] = json!(walk(view.get("view").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]), &names, &mut count));
            }
            Err(CanvasError::Forbidden(m)) => out["entries_note"] = json!(format!("답글을 볼 수 없습니다: {m} (먼저 글을 써야 보이는 토론일 수 있습니다)")),
            Err(e) => return Err(e),
        }
        Ok(out)
    }

    pub fn conversations(&self, scope: &str) -> Result<Vec<Value>> {
        let scope = if matches!(scope, "unread" | "starred" | "sent" | "archived") { scope } else { "inbox" };
        let mut q = vec![("include[]", "participant_avatars".to_string())];
        if scope != "inbox" {
            q.push(("scope", scope.to_string()));
        }
        let list = self.get_all("conversations", &q, 100)?;
        Ok(list.iter().map(|c| {
            let mut x = pick(c, &["id", "subject", "workflow_state", "last_message", "last_message_at", "message_count", "context_name", "starred"]);
            x["participants"] = json!(c.get("participants").and_then(Value::as_array).into_iter().flatten().filter_map(|p| p.get("name").cloned()).collect::<Vec<_>>());
            x
        }).collect())
    }

    pub fn conversation(&self, id: u64) -> Result<Value> {
        let c = self.get(&format!("conversations/{id}"), &[])?;
        let names: std::collections::HashMap<String, String> = c.get("participants").and_then(Value::as_array).into_iter().flatten()
            .filter_map(|p| Some((s(p, "id"), p.get("name")?.as_str()?.to_string()))).collect();
        let mut out = pick(&c, &["id", "subject", "context_name", "workflow_state"]);
        out["participants"] = json!(c.get("participants").and_then(Value::as_array).into_iter().flatten().map(|p| pick(p, &["id", "name"])).collect::<Vec<_>>());
        out["messages"] = json!(c.get("messages").and_then(Value::as_array).into_iter().flatten().take(100).map(|m| json!({
            "id": m.get("id"),
            "author": names.get(&s(m, "author_id")).cloned().unwrap_or_default(),
            "created_at": m.get("created_at"),
            "body": m.get("body").and_then(Value::as_str).map(|b| truncate(b.to_string(), 4000)),
            "attachments": m.get("attachments").and_then(Value::as_array).map(|a| a.iter().map(|f| pick(f, &["id", "display_name"])).collect::<Vec<_>>()),
        })).collect::<Vec<_>>());
        Ok(out)
    }

    pub fn quizzes(&self, course: u64) -> Result<Vec<Value>> {
        let list = self.get_all(&format!("courses/{course}/quizzes"), &[], 200)?;
        Ok(list.iter().map(|q| pick(q, &["id", "title", "quiz_type", "due_at", "lock_at", "unlock_at", "time_limit", "allowed_attempts", "points_possible", "question_count", "html_url", "locked_for_user", "lock_explanation"])).collect())
    }

    pub fn peer_reviews(&self, course: u64, assignment: u64) -> Result<Vec<Value>> {
        let list = self.get_all(&format!("courses/{course}/assignments/{assignment}/peer_reviews"), &[("include[]", "user".to_string())], 100)?;
        Ok(list.iter().map(|r| {
            let mut x = pick(r, &["id", "asset_id", "workflow_state", "assessor_id", "user_id"]);
            x["reviewee"] = r.pointer("/user/display_name").cloned().unwrap_or(Value::Null);
            x
        }).collect())
    }

    pub fn groups(&self) -> Result<Vec<Value>> {
        let list = self.get_all("users/self/groups", &[], 100)?;
        Ok(list.iter().map(|g| pick(g, &["id", "name", "course_id", "members_count", "group_category_id"])).collect())
    }

    pub fn group_members(&self, group: u64) -> Result<Vec<Value>> {
        let list = self.get_all(&format!("groups/{group}/users"), &[], 200)?;
        Ok(list.iter().map(|u| pick(u, &["id", "name", "short_name"])).collect())
    }

    pub fn course_people(&self, course: u64, search: Option<&str>) -> Result<Vec<Value>> {
        let mut q = vec![("include[]", "enrollments".to_string())];
        if let Some(t) = search.filter(|t| t.chars().count() >= 2) {
            q.push(("search_term", t.to_string()));
        }
        let list = self.get_all(&format!("courses/{course}/users"), &q, 500)?;
        Ok(list.iter().map(|u| {
            let mut x = pick(u, &["id", "name", "short_name"]);
            x["roles"] = json!(u.get("enrollments").and_then(Value::as_array).into_iter().flatten().filter_map(|e| e.get("type").cloned()).collect::<Vec<_>>());
            x
        }).collect())
    }

    pub fn folders(&self, course: u64) -> Result<Vec<Value>> {
        let list = self.get_all(&format!("courses/{course}/folders"), &[], 500)?;
        Ok(list.iter().filter(|f| f.get("hidden_for_user") != Some(&json!(true))).map(|f| pick(f, &["id", "name", "full_name", "parent_folder_id", "files_count", "folders_count", "locked_for_user", "position"])).collect())
    }

    pub fn folder_files(&self, folder: u64) -> Result<Vec<Value>> {
        let list = self.get_all(&format!("folders/{folder}/files"), &[("sort", "name".to_string())], 500)?;
        Ok(list.iter().map(|f| pick(f, &["id", "display_name", "size", "content-type", "updated_at", "locked_for_user"])).collect())
    }

    /// Read-only access to any Canvas API path the student can see.
    pub fn api_get(&self, path: &str, query: &Value) -> Result<Value> {
        let path = safe_api_path(path)?;
        let mut q: Vec<(String, String)> = Vec::new();
        if let Some(obj) = query.as_object() {
            for (k, v) in obj.iter().take(30) {
                match v {
                    Value::Array(items) => items.iter().take(20).for_each(|i| q.push((k.clone(), i.as_str().map(str::to_string).unwrap_or_else(|| i.to_string())))),
                    Value::Null => {}
                    other => q.push((k.clone(), other.as_str().map(str::to_string).unwrap_or_else(|| other.to_string()))),
                }
            }
        }
        let q: Vec<(&str, String)> = q.iter().filter(|(k, _)| k != "per_page" && k != "access_token").map(|(k, v)| (k.as_str(), v.clone())).collect();
        let list = self.get_all(&path, &q, 300)?;
        Ok(if list.len() == 1 && !path.ends_with('s') { list.into_iter().next().unwrap_or(Value::Null) } else { json!(list) })
    }

    // ---------------- file upload ----------------

    /// Canvas 3-step upload. Returns the new file id.
    pub fn upload_file(&self, init_path: &str, file: &Path) -> Result<String> {
        let name = file.file_name().and_then(|n| n.to_str()).ok_or_else(|| CanvasError::Invalid("파일 이름을 읽을 수 없습니다".into()))?.to_string();
        let meta = std::fs::metadata(file).map_err(|e| CanvasError::Invalid(format!("{name}: 파일을 열 수 없습니다 ({e})")))?;
        if !meta.is_file() {
            return Err(CanvasError::Invalid(format!("{name}: 파일이 아닙니다")));
        }
        if meta.len() > MAX_UPLOAD {
            return Err(CanvasError::Invalid(format!("{name}: 500MB보다 큰 파일은 앱에서 올릴 수 없습니다. Canvas 웹에서 제출하세요.")));
        }
        let content_type = guess_mime(&name);
        // Step 1: tell Canvas about the file.
        let init = self.post_form(init_path, &[
            ("name".into(), name.clone()),
            ("size".into(), meta.len().to_string()),
            ("content_type".into(), content_type.into()),
            ("on_duplicate".into(), "rename".into()),
        ])?;
        let upload_url = init.get("upload_url").and_then(Value::as_str).filter(|u| u.starts_with("https://") || u.starts_with("http://"))
            .ok_or_else(|| CanvasError::Invalid("Canvas가 업로드 주소를 주지 않았습니다".into()))?.to_string();
        let params = init.get("upload_params").and_then(Value::as_object).cloned().unwrap_or_default();
        // Step 2: send the bytes to the storage service — never with the access token.
        let mut data = Vec::with_capacity(meta.len() as usize);
        std::fs::File::open(file).and_then(|mut f| f.read_to_end(&mut data)).map_err(|e| CanvasError::Invalid(format!("{name}: 읽기 실패 ({e})")))?;
        let boundary = format!("----CanvasAssistant{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        let body = multipart_body(&params, &name, content_type, &data, &boundary);
        drop(data);
        let storage: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(600)))
            .build()
            .into();
        let mut resp = storage.post(&upload_url)
            .header("Content-Type", &format!("multipart/form-data; boundary={boundary}"))
            .send(&body[..])
            .map_err(|e| CanvasError::Network(e.to_string()))?;
        let status = resp.status().as_u16();
        let location = resp.headers().get("location").and_then(|h| h.to_str().ok()).map(str::to_string);
        let text = resp.body_mut().with_config().limit(2 * 1024 * 1024).read_to_string().unwrap_or_default();
        if !(200..400).contains(&status) {
            return Err(CanvasError::Http(status, format!("{name}: 파일 업로드 실패 {}", text.chars().take(200).collect::<String>())));
        }
        // Step 3: confirm. A JSON body with an id means we're done.
        if let Some(id) = serde_json::from_str::<Value>(&text).ok().and_then(|v| v.get("id").cloned()) {
            if (200..300).contains(&status) {
                return Ok(id.as_str().map(str::to_string).unwrap_or_else(|| id.to_string()));
            }
        }
        let location = location.ok_or_else(|| CanvasError::Invalid(format!("{name}: 업로드 확인 주소가 없습니다")))?;
        let confirm = if same_origin(&self.base, &location) {
            let (b, _) = self.send(if (300..400).contains(&status) { "GET" } else { "GET" }, &location, None)?;
            b
        } else {
            let mut r = storage.get(&location).call().map_err(|e| CanvasError::Network(e.to_string()))?;
            r.body_mut().read_to_string().map_err(|e| CanvasError::Network(e.to_string()))?
        };
        let v: Value = serde_json::from_str(&confirm).map_err(|_| CanvasError::Invalid(format!("{name}: 업로드 확인 응답을 해석할 수 없습니다")))?;
        let id = v.get("id").ok_or_else(|| CanvasError::Invalid(format!("{name}: 업로드된 파일 ID가 없습니다")))?;
        Ok(id.as_str().map(str::to_string).unwrap_or_else(|| id.to_string()))
    }

    // ---------------- writes ----------------

    pub fn execute_write_ext(&self, kind: &str, p: &Value) -> Result<Value> {
        match kind {
            "submit_files" => {
                let course = require_id(p, "course_id")?;
                let assignment = require_id(p, "assignment_id")?;
                let files: Vec<String> = p.get("files").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
                if files.is_empty() {
                    return Err(CanvasError::Invalid("제출할 파일을 선택하세요".into()));
                }
                if files.len() > 20 {
                    return Err(CanvasError::Invalid("한 번에 20개까지 제출할 수 있습니다".into()));
                }
                let init = format!("courses/{course}/assignments/{assignment}/submissions/self/files");
                let mut ids = Vec::new();
                for f in &files {
                    ids.push(self.upload_file(&init, Path::new(f))?);
                }
                let mut fields = vec![("submission[submission_type]".to_string(), "online_upload".to_string())];
                fields.extend(ids.iter().map(|id| ("submission[file_ids][]".to_string(), id.clone())));
                push_comment(&mut fields, p);
                let r = self.post_form(&format!("courses/{course}/assignments/{assignment}/submissions"), &fields)?;
                let mut out = pick(&r, &["id", "workflow_state", "submitted_at", "attempt"]);
                out["files"] = json!(files.iter().map(|f| Path::new(f).file_name().and_then(|n| n.to_str()).unwrap_or("").to_string()).collect::<Vec<_>>());
                Ok(out)
            }
            "submission_comment" => {
                let course = require_id(p, "course_id")?;
                let assignment = require_id(p, "assignment_id")?;
                let text = opt_str(p, "text").ok_or_else(|| CanvasError::Invalid("코멘트 내용이 비어 있습니다".into()))?;
                let r = self.put_form(&format!("courses/{course}/assignments/{assignment}/submissions/self"), &[("comment[text_comment]".into(), text.into())])?;
                Ok(json!({"ok": true, "comments": r.get("submission_comments").and_then(Value::as_array).map(|c| c.len())}))
            }
            "planner_complete" => {
                let ptype = opt_str(p, "plannable_type").filter(|t| matches!(*t, "assignment" | "quiz" | "discussion_topic" | "wiki_page" | "announcement" | "planner_note" | "calendar_event"))
                    .ok_or_else(|| CanvasError::Invalid("plannable_type 값이 올바르지 않습니다".into()))?;
                let pid = require_id(p, "plannable_id")?;
                let complete = p.get("complete").and_then(Value::as_bool).unwrap_or(true);
                let created = self.post_form("planner/overrides", &[
                    ("plannable_type".into(), ptype.into()),
                    ("plannable_id".into(), pid.to_string()),
                    ("marked_complete".into(), complete.to_string()),
                ]);
                match created {
                    Ok(v) => Ok(pick(&v, &["id", "marked_complete"])),
                    Err(CanvasError::Http(400, _)) | Err(CanvasError::Http(422, _)) => {
                        // An override already exists: update it instead.
                        let list = self.get_all("planner/overrides", &[], 500)?;
                        let existing = list.iter().find(|o| s(o, "plannable_id") == pid.to_string() && s(o, "plannable_type").eq_ignore_ascii_case(ptype))
                            .and_then(|o| o.get("id")).map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()))
                            .ok_or_else(|| CanvasError::Invalid("플래너 항목을 찾지 못했습니다".into()))?;
                        let v = self.put_form(&format!("planner/overrides/{existing}"), &[("marked_complete".into(), complete.to_string())])?;
                        Ok(pick(&v, &["id", "marked_complete"]))
                    }
                    Err(e) => Err(e),
                }
            }
            "module_item_done" => {
                let course = require_id(p, "course_id")?;
                let module = require_id(p, "module_id")?;
                let item = require_id(p, "item_id")?;
                let path = format!("courses/{course}/modules/{module}/items/{item}/done");
                if p.get("done").and_then(Value::as_bool).unwrap_or(true) { self.put_form(&path, &[])?; } else { self.delete(&path)?; }
                Ok(json!({"ok": true}))
            }
            "module_item_read" => {
                let course = require_id(p, "course_id")?;
                let module = require_id(p, "module_id")?;
                let item = require_id(p, "item_id")?;
                self.post_form(&format!("courses/{course}/modules/{module}/items/{item}/mark_read"), &[])?;
                Ok(json!({"ok": true}))
            }
            "discussion_entry_reply" => {
                let course = require_id(p, "course_id")?;
                let topic = require_id(p, "topic_id")?;
                let entry = require_id(p, "entry_id")?;
                let message = opt_str(p, "message").ok_or_else(|| CanvasError::Invalid("답글 내용이 비어 있습니다".into()))?;
                let r = self.post_form(&format!("courses/{course}/discussion_topics/{topic}/entries/{entry}/replies"), &[("message".into(), message.into())])?;
                Ok(pick(&r, &["id", "created_at"]))
            }
            "discussion_mark_read" => {
                let course = require_id(p, "course_id")?;
                let topic = require_id(p, "topic_id")?;
                self.put_form(&format!("courses/{course}/discussion_topics/{topic}/read_all"), &[])?;
                Ok(json!({"ok": true}))
            }
            "send_message" => {
                let recipients: Vec<String> = p.get("recipients").and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|r| require_id(&json!({"id": r}), "id").ok().map(|n| n.to_string())).collect()).unwrap_or_default();
                if recipients.is_empty() || recipients.len() > 20 {
                    return Err(CanvasError::Invalid("받는 사람을 1~20명 지정하세요".into()));
                }
                let body = opt_str(p, "body").ok_or_else(|| CanvasError::Invalid("메시지 내용이 비어 있습니다".into()))?;
                let mut fields: Vec<(String, String)> = recipients.iter().map(|r| ("recipients[]".to_string(), r.clone())).collect();
                fields.push(("body".into(), body.into()));
                fields.push(("subject".into(), opt_str(p, "subject").unwrap_or("(제목 없음)").into()));
                fields.push(("group_conversation".into(), "true".into()));
                if let Ok(course) = require_id(p, "course_id") {
                    fields.push(("context_code".into(), format!("course_{course}")));
                }
                let r = self.post_form("conversations", &fields)?;
                Ok(json!({"ok": true, "conversations": r.as_array().map(|a| a.len()).unwrap_or(1)}))
            }
            "what_if" => {
                let sub = require_id(p, "submission_id")?;
                let score = p.get("score").and_then(|v| v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())));
                let fields = match score {
                    Some(sc) => vec![("student_entered_score".to_string(), sc.to_string())],
                    None => vec![("student_entered_score".to_string(), String::new())],
                };
                let r = self.put_form(&format!("submissions/{sub}/what_if_grades"), &fields)?;
                Ok(r)
            }
            "what_if_reset" => {
                let course = require_id(p, "course_id")?;
                Ok(self.put_form(&format!("courses/{course}/what_if_grades/reset"), &[])?)
            }
            "conversation_read" => {
                let id = require_id(p, "conversation_id")?;
                self.put_form(&format!("conversations/{id}"), &[("conversation[workflow_state]".into(), "read".into())])?;
                Ok(json!({"ok": true}))
            }
            _ => Err(CanvasError::Invalid("지원하지 않는 작업입니다".into())),
        }
    }
}

/// Korean summary for new write kinds (see canvas::describe_write).
pub fn describe_write_ext(kind: &str, p: &Value) -> Option<String> {
    let name = |nk: &str, ik: &str, prefix: &str| {
        let n = s(p, nk);
        if n.is_empty() { format!("{prefix} {}", s(p, ik)) } else { n }
    };
    let course = name("_course_name", "course_id", "강의");
    Some(match kind {
        "submit_files" => {
            let files = p.get("files").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0);
            format!("과제 파일 제출 — {course} / {} ({})", name("_assignment_name", "assignment_id", "과제"), if files == 0 { "파일 첨부 필요".to_string() } else { format!("파일 {files}개") })
        }
        "submission_comment" => format!("제출물에 코멘트 — {course} / {}", name("_assignment_name", "assignment_id", "과제")),
        "planner_complete" => format!("플래너 {} — {} #{}", if p.get("complete") == Some(&json!(false)) { "완료 취소" } else { "완료 표시" }, s(p, "plannable_type"), s(p, "plannable_id")),
        "module_item_done" => format!("모듈 항목 {} — {course}", if p.get("done") == Some(&json!(false)) { "완료 취소" } else { "완료 표시" }),
        "module_item_read" => format!("모듈 항목 읽음 처리 — {course}"),
        "discussion_entry_reply" => format!("토론 답글 — {course} / {}", name("_topic_title", "topic_id", "토론")),
        "discussion_mark_read" => format!("토론 모두 읽음 — {course} / {}", name("_topic_title", "topic_id", "토론")),
        "send_message" => format!("Canvas 메시지 보내기 — {} (받는 사람 {}명)", s(p, "subject"), p.get("recipients").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0)),
        "what_if" => format!("성적 예측(What-if) 점수 입력 — 제출 #{}: {}", s(p, "submission_id"), s(p, "score")),
        "what_if_reset" => format!("성적 예측 초기화 — {course}"),
        "conversation_read" => format!("메시지 읽음 처리 — #{}", s(p, "conversation_id")),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scripted HTTP server that records full requests (line, headers, body).
    fn recording_server(script: Vec<Box<dyn Fn(&str) -> (u16, Vec<(String, String)>, String) + Send>>) -> (String, std::sync::mpsc::Receiver<(String, String)>) {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let base2 = base.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for respond in script {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut head = String::new();
                let mut len = 0usize;
                loop {
                    let mut l = String::new();
                    if reader.read_line(&mut l).unwrap() == 0 || l == "\r\n" { break; }
                    if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") { len = v.trim().parse().unwrap_or(0); }
                    head.push_str(&l);
                }
                let mut body = vec![0u8; len];
                std::io::Read::read_exact(&mut reader, &mut body).unwrap();
                let body = String::from_utf8_lossy(&body).into_owned();
                let (status, headers, out) = respond(&base2);
                tx.send((head, body)).unwrap();
                let mut resp = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n", out.len());
                for (k, v) in headers { resp.push_str(&format!("{k}: {v}\r\n")); }
                resp.push_str("\r\n");
                resp.push_str(&out);
                let mut stream = stream;
                stream.write_all(resp.as_bytes()).unwrap();
            }
        });
        (base, rx)
    }

    #[test]
    fn submits_files_with_three_step_upload() {
        let file = std::env::temp_dir().join(format!("ca-upload-{}.pdf", std::process::id()));
        std::fs::write(&file, b"%PDF-1.4 hello").unwrap();
        let (base, rx) = recording_server(vec![
            // 1. Canvas: upload slot
            Box::new(|b: &str| (200, vec![], format!(r#"{{"upload_url":"{b}/storage","upload_params":{{"key":"k1","policy":"p1"}}}}"#))),
            // 2. Storage: redirect to Canvas confirmation
            Box::new(|b: &str| (301, vec![("Location".into(), format!("{b}/api/v1/files/55/create_success?uuid=x"))], String::new())),
            // 3. Canvas: confirmation returns the file
            Box::new(|_: &str| (200, vec![], r#"{"id":"55","display_name":"report.pdf"}"#.into())),
            // 4. Canvas: submission
            Box::new(|_: &str| (201, vec![], r#"{"id":"900","workflow_state":"submitted","attempt":2}"#.into())),
        ]);
        let client = Client::for_test(&base, "secret-token-1234567890");
        let r = client.execute_write("submit_files", &json!({"course_id": 7, "assignment_id": 8, "files": [file.to_string_lossy()], "comment": "확인 부탁드립니다"})).unwrap();
        assert_eq!(r["workflow_state"], "submitted");
        let reqs: Vec<(String, String)> = rx.try_iter().collect();
        assert_eq!(reqs.len(), 4);
        assert!(reqs[0].0.starts_with("POST /api/v1/courses/7/assignments/8/submissions/self/files"));
        assert!(reqs[0].1.contains("size=14") && reqs[0].1.contains("content_type=application%2Fpdf"));
        let storage = reqs[1].0.to_ascii_lowercase();
        assert!(storage.starts_with("post /storage"));
        assert!(!storage.contains("authorization"), "token must not go to the storage service");
        assert!(reqs[1].1.contains("name=\"key\"\r\n\r\nk1") && reqs[1].1.contains("%PDF-1.4 hello"));
        assert!(reqs[2].0.starts_with("GET /api/v1/files/55/create_success") && reqs[2].0.to_ascii_lowercase().contains("authorization: bearer"));
        assert!(reqs[3].1.contains("submission%5Bsubmission_type%5D=online_upload"));
        assert!(reqs[3].1.contains("submission%5Bfile_ids%5D%5B%5D=55"));
        assert!(reqs[3].1.contains("comment%5Btext_comment%5D="));
        let _ = std::fs::remove_file(file);
    }

    #[test]
    fn api_paths_are_guarded() {
        assert_eq!(safe_api_path("/api/v1/courses/1/assignments").unwrap(), "courses/1/assignments");
        assert_eq!(safe_api_path("courses/1/pages?x=1").unwrap(), "courses/1/pages");
        assert!(safe_api_path("../../login").is_err());
        assert!(safe_api_path("https://evil.example/x").is_err());
        assert!(safe_api_path("courses/1/files;rm").is_err());
        assert!(safe_api_path("").is_err());
    }

    #[test]
    fn multipart_puts_file_last() {
        let mut params = Map::new();
        params.insert("key".into(), json!("abc"));
        params.insert("policy".into(), json!("p"));
        let body = String::from_utf8(multipart_body(&params, "보고서.pdf", "application/pdf", b"DATA", "B")).unwrap();
        let file_at = body.find("name=\"file\"").unwrap();
        assert!(body.find("name=\"key\"").unwrap() < file_at);
        assert!(body.find("name=\"policy\"").unwrap() < file_at);
        assert!(body.contains("filename=\"보고서.pdf\""));
        assert!(body.ends_with("\r\n--B--\r\n"));
    }

    #[test]
    fn guesses_common_types() {
        assert_eq!(guess_mime("a.PDF"), "application/pdf");
        assert_eq!(guess_mime("report.hwp"), "application/x-hwp");
        assert_eq!(guess_mime("noext"), "application/octet-stream");
    }

    #[test]
    fn describes_new_writes() {
        assert!(describe_write_ext("submit_files", &json!({"course_id": 1, "assignment_id": 2})).unwrap().contains("파일 첨부 필요"));
        assert!(describe_write_ext("send_message", &json!({"subject": "질문", "recipients": [1, 2]})).unwrap().contains("2명"));
        assert!(describe_write_ext("nope", &json!({})).is_none());
    }
}
