//! Reading the contents of course files and the user's own submitted files.
use crate::canvas::{html_field, pick, truncate, CanvasError, Client, Result};
use crate::extract;
use serde_json::{json, Value};

fn offset(args: &Value) -> usize {
    args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize
}

/// Text of one extracted document, paged.
pub fn paged(name: &str, text: &str, offset: usize, limit: usize) -> Value {
    let (chunk, next, total) = extract::page(text, offset, limit);
    let mut out = json!({"name": name, "total_chars": total, "offset": offset, "text": chunk});
    if let Some(n) = next {
        out["next_offset"] = json!(n);
        out["note"] = json!(format!("내용이 길어 일부만 보냈습니다. 이어서 읽으려면 offset={n}으로 다시 호출하세요."));
    }
    out
}

impl Client {
    /// Reads a Canvas file (course file or any file the user can open) as text.
    pub fn read_file(&self, file_id: u64, args: &Value) -> Result<Value> {
        let meta = self.get(&format!("files/{file_id}"), &[]).map_err(|e| match e {
            CanvasError::Unauthorized | CanvasError::Forbidden(_) => CanvasError::Forbidden(
                "이 파일 정보를 볼 권한이 없습니다. 강의자료실이 LearningX라면 canvas_learningx_resources / canvas_learningx_read를 쓰세요".into(),
            ),
            other => other,
        })?;
        let name = meta.get("display_name").and_then(Value::as_str).unwrap_or("file").to_string();
        let size = meta.get("size").and_then(Value::as_u64).unwrap_or(0);
        if size > extract::MAX_READ_BYTES {
            return Err(CanvasError::Invalid(format!("'{name}'은(는) {}MB라 너무 커서 내용을 읽을 수 없습니다.", size >> 20)));
        }
        let url = meta.get("url").and_then(Value::as_str).filter(|u| !u.is_empty())
            .ok_or_else(|| CanvasError::Invalid(format!("'{name}'은(는) 잠겨 있거나 아직 공개되지 않아 내려받을 수 없습니다.")))?;
        let bytes = self.fetch_bytes(url, extract::MAX_READ_BYTES)?;
        let text = extract::extract_text(&name, &bytes).map_err(CanvasError::Invalid)?;
        let mut out = paged(&name, &text, offset(args), extract::PAGE_CHARS);
        out["file_id"] = json!(file_id);
        out["content_type"] = meta.get("content-type").cloned().unwrap_or(Value::Null);
        Ok(out)
    }

    /// The user's own submission for an assignment with the text of every submitted file
    /// (code, reports, archives). `attempt` picks an earlier attempt from the history.
    pub fn read_submission(&self, course: u64, assignment: u64, args: &Value) -> Result<Value> {
        let sub = self.get(&format!("courses/{course}/assignments/{assignment}/submissions/self"), &[
            ("include[]", "submission_history".to_string()),
            ("include[]", "submission_comments".to_string()),
        ])?;
        let history: Vec<Value> = sub.get("submission_history").and_then(Value::as_array).cloned().unwrap_or_default();
        let attempts: Vec<i64> = history.iter().filter_map(|h| h.get("attempt").and_then(Value::as_i64)).collect();
        let wanted = args.get("attempt").and_then(Value::as_i64);
        let chosen = match wanted {
            Some(a) => history.iter().find(|h| h.get("attempt").and_then(Value::as_i64) == Some(a)).cloned()
                .ok_or_else(|| CanvasError::Invalid(format!("{a}번째 제출을 찾지 못했습니다. 제출 기록: {attempts:?}")))?,
            None => sub.clone(),
        };
        if chosen.get("workflow_state").and_then(Value::as_str) == Some("unsubmitted") && chosen.get("attachments").is_none() && chosen.get("body").map_or(true, Value::is_null) {
            return Ok(json!({"submitted": false, "note": "이 과제에는 아직 제출한 내용이 없습니다."}));
        }
        let mut out = pick(&chosen, &["attempt", "submitted_at", "submission_type", "workflow_state", "score", "grade", "late", "url"]);
        out["attempts"] = json!(attempts);
        if let Some(b) = html_field(&chosen, "body") {
            out["body"] = json!(truncate(b, 20000));
        }
        if let Some(c) = sub.get("submission_comments").and_then(Value::as_array) {
            out["comments"] = json!(c.iter().map(|x| pick(x, &["author_name", "comment", "created_at", "attempt"])).collect::<Vec<_>>());
        }
        let files: Vec<Value> = chosen.get("attachments").and_then(Value::as_array).cloned().unwrap_or_default();
        let only = args.get("file_id").and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())));
        let per_file = if files.len() > 1 && only.is_none() { extract::PAGE_CHARS / files.len().min(4) } else { extract::PAGE_CHARS };
        let mut read = Vec::new();
        for f in files.iter().filter(|f| only.is_none() || f.get("id").and_then(Value::as_u64) == only).take(10) {
            let name = f.get("display_name").or_else(|| f.get("filename")).and_then(Value::as_str).unwrap_or("file").to_string();
            let mut entry = pick(f, &["id", "display_name", "size", "content-type"]);
            let size = f.get("size").and_then(Value::as_u64).unwrap_or(0);
            let result = if size > extract::MAX_READ_BYTES {
                Err(format!("{}MB라 너무 커서 읽지 않았습니다", size >> 20))
            } else {
                match f.get("url").and_then(Value::as_str).filter(|u| !u.is_empty()) {
                    Some(url) => self.fetch_bytes(url, extract::MAX_READ_BYTES).map_err(String::from).and_then(|b| extract::extract_text(&name, &b)),
                    None => Err("다운로드 주소가 없습니다".into()),
                }
            };
            match result {
                Ok(text) => {
                    let p = paged(&name, &text, if only.is_some() { offset(args) } else { 0 }, per_file);
                    entry["text"] = p["text"].clone();
                    entry["total_chars"] = p["total_chars"].clone();
                    if let Some(n) = p.get("next_offset") {
                        entry["note"] = json!(format!("일부만 보냈습니다. 이 파일만 이어서 읽으려면 file_id={} offset={n}", f.get("id").cloned().unwrap_or(Value::Null)));
                    }
                }
                Err(e) => entry["error"] = json!(e),
            }
            read.push(entry);
        }
        out["files"] = json!(read);
        Ok(out)
    }
}
