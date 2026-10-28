//! Canvas LMS REST client.
//!
//! The access token is only ever sent to the configured Canvas origin. Pagination
//! links pointing anywhere else are refused, and file downloads that redirect to
//! storage hosts are fetched without the Authorization header.
use serde_json::{json, Map, Value};
use std::{io::Read, time::Duration};

const MAX_PAGES: usize = 20;
const MAX_BODY: u64 = 20 * 1024 * 1024;
const TEXT_LIMIT: usize = 4000;

#[derive(Debug)]
pub enum CanvasError {
    NotConfigured,
    InvalidUrl(String),
    Unauthorized,
    Forbidden(String),
    NotFound,
    RateLimited,
    Http(u16, String),
    Network(String),
    Invalid(String),
}

impl std::fmt::Display for CanvasError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotConfigured => write!(f, "Canvas가 연결되지 않았습니다. 설정에서 Canvas 주소와 액세스 토큰을 입력하세요."),
            Self::InvalidUrl(m) => write!(f, "Canvas 주소가 올바르지 않습니다: {m}"),
            Self::Unauthorized => write!(f, "Canvas 토큰이 유효하지 않거나 만료되었습니다. 설정에서 새 토큰을 입력하세요."),
            Self::Forbidden(m) => write!(f, "Canvas에서 접근을 거부했습니다: {m}"),
            Self::NotFound => write!(f, "Canvas에서 항목을 찾을 수 없습니다."),
            Self::RateLimited => write!(f, "Canvas 요청 한도에 걸렸습니다. 잠시 후 다시 시도하세요."),
            Self::Http(code, m) => write!(f, "Canvas 요청 실패 (HTTP {code}): {m}"),
            Self::Network(m) => write!(f, "Canvas에 연결할 수 없습니다: {m}"),
            Self::Invalid(m) => write!(f, "{m}"),
        }
    }
}

impl From<CanvasError> for String {
    fn from(e: CanvasError) -> Self {
        e.to_string()
    }
}

pub type Result<T> = std::result::Result<T, CanvasError>;

/// Normalizes user input such as `canvas.school.ac.kr/` into `https://canvas.school.ac.kr`.
pub fn normalize_base_url(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(CanvasError::InvalidUrl("주소를 입력하세요".into()));
    }
    let with_scheme = if trimmed.contains("://") { trimmed.to_string() } else { format!("https://{trimmed}") };
    let parsed = url::Url::parse(&with_scheme).map_err(|e| CanvasError::InvalidUrl(e.to_string()))?;
    if parsed.scheme() != "https" {
        return Err(CanvasError::InvalidUrl("https 주소만 사용할 수 있습니다".into()));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(CanvasError::InvalidUrl("주소에 계정 정보를 넣을 수 없습니다".into()));
    }
    let host = parsed.host_str().ok_or_else(|| CanvasError::InvalidUrl("호스트가 없습니다".into()))?;
    if !host.contains('.') {
        return Err(CanvasError::InvalidUrl("도메인 형식이 아닙니다".into()));
    }
    Ok(match parsed.port() {
        Some(port) => format!("https://{host}:{port}"),
        None => format!("https://{host}"),
    })
}

/// Extracts the `rel="next"` URL from a Canvas `Link` header.
pub fn next_link(header: &str) -> Option<String> {
    header.split(',').find_map(|part| {
        let mut pieces = part.split(';');
        let url = pieces.next()?.trim().trim_start_matches('<').trim_end_matches('>');
        pieces.any(|p| p.trim() == "rel=\"next\"").then(|| url.to_string())
    })
}

fn same_origin(base: &str, other: &str) -> bool {
    match (url::Url::parse(base), url::Url::parse(other)) {
        (Ok(a), Ok(b)) => a.scheme() == b.scheme() && a.host_str() == b.host_str() && a.port_or_known_default() == b.port_or_known_default(),
        _ => false,
    }
}

/// Converts Canvas HTML (descriptions, announcements, pages) to readable plain text.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut chars = html.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            let mut tag = String::new();
            for t in chars.by_ref() {
                if t == '>' {
                    break;
                }
                tag.push(t);
            }
            let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
            if matches!(name.as_str(), "br" | "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
                out.push('\n');
                if name == "li" && !tag.starts_with('/') {
                    out.push_str("- ");
                }
            }
            if (name == "script" || name == "style") && !tag.starts_with('/') {
                let closing = format!("</{name}");
                let mut rest = String::new();
                for t in chars.by_ref() {
                    rest.push(t);
                    if rest.to_ascii_lowercase().ends_with(&closing) {
                        for t2 in chars.by_ref() {
                            if t2 == '>' {
                                break;
                            }
                        }
                        break;
                    }
                }
            }
        } else if c == '&' {
            let mut entity = String::new();
            while let Some(&n) = chars.peek() {
                if n == ';' || entity.len() > 8 || n.is_whitespace() {
                    break;
                }
                entity.push(n);
                chars.next();
            }
            if chars.peek() == Some(&';') {
                chars.next();
                let decoded = match entity.as_str() {
                    "nbsp" => Some(' '),
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    _ => entity.strip_prefix("#x").or_else(|| entity.strip_prefix("#X")).and_then(|h| u32::from_str_radix(h, 16).ok())
                        .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                        .and_then(char::from_u32),
                };
                if let Some(ch) = decoded {
                    out.push(ch);
                }
            } else {
                out.push('&');
                out.push_str(&entity);
            }
        } else {
            out.push(c);
        }
    }
    let mut cleaned = String::new();
    for line in out.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if !line.is_empty() {
            cleaned.push_str(&line);
            cleaned.push('\n');
        }
    }
    cleaned.trim().to_string()
}

fn truncate(text: String, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text;
    }
    let mut cut: String = text.chars().take(limit).collect();
    cut.push_str("\n…(이하 생략)");
    cut
}

fn pick(v: &Value, keys: &[&str]) -> Value {
    let mut out = Map::new();
    for k in keys {
        if let Some(x) = v.get(*k) {
            if !x.is_null() {
                out.insert((*k).to_string(), x.clone());
            }
        }
    }
    Value::Object(out)
}

fn html_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(|h| truncate(html_to_text(h), TEXT_LIMIT)).filter(|s| !s.is_empty())
}

pub fn require_id(v: &Value, key: &str) -> Result<u64> {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_u64().ok_or_else(|| CanvasError::Invalid(format!("{key} 값이 올바르지 않습니다"))),
        Some(Value::String(s)) => s.trim().parse().map_err(|_| CanvasError::Invalid(format!("{key} 값이 올바르지 않습니다"))),
        _ => Err(CanvasError::Invalid(format!("{key} 값이 필요합니다"))),
    }
}

fn opt_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty())
}

fn days_window(days_back: i64, days_ahead: i64) -> (String, String) {
    let now = chrono::Utc::now();
    let start = now - chrono::Duration::days(days_back);
    let end = now + chrono::Duration::days(days_ahead);
    (start.format("%Y-%m-%dT%H:%M:%SZ").to_string(), end.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

pub struct Client {
    base: String,
    token: String,
    agent: ureq::Agent,
    download_agent: ureq::Agent,
}

impl Client {
    pub fn new(base_url: &str, token: &str) -> Result<Self> {
        let base = normalize_base_url(base_url)?;
        let token = token.trim().to_string();
        if token.is_empty() {
            return Err(CanvasError::NotConfigured);
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(40)))
            .user_agent(concat!("CanvasAssistant/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        let download_agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(5)
            .timeout_global(Some(Duration::from_secs(300)))
            .build()
            .into();
        Ok(Self { base, token, agent, download_agent })
    }

    /// Test-only constructor that accepts a plain-http loopback server.
    #[cfg(test)]
    fn for_test(base: &str, token: &str) -> Self {
        let mut c = Self::new("https://placeholder.example", token).unwrap();
        c.base = base.to_string();
        c
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1/{}", self.base, path.trim_start_matches('/'))
    }

    fn check(status: u16, body: &str, headers_remaining: Option<&str>) -> Result<()> {
        if (200..300).contains(&status) {
            return Ok(());
        }
        let message = serde_json::from_str::<Value>(body).ok()
            .and_then(|v| v.get("errors").cloned().or_else(|| v.get("message").cloned()))
            .map(|e| match e {
                Value::Array(items) => items.iter().filter_map(|i| i.get("message").and_then(Value::as_str).map(str::to_string)).collect::<Vec<_>>().join(", "),
                Value::String(s) => s,
                other => other.to_string(),
            })
            .unwrap_or_else(|| body.chars().take(200).collect());
        Err(match status {
            401 => CanvasError::Unauthorized,
            403 if body.contains("Rate Limit") || headers_remaining == Some("0") => CanvasError::RateLimited,
            403 => CanvasError::Forbidden(message),
            404 => CanvasError::NotFound,
            429 => CanvasError::RateLimited,
            _ => CanvasError::Http(status, message),
        })
    }

    /// Single request returning (status, link header, body). Retries once on rate limiting.
    fn send(&self, method: &str, url: &str, form: Option<&[(String, String)]>) -> Result<(String, Option<String>)> {
        if !same_origin(&self.base, url) {
            return Err(CanvasError::Invalid("Canvas 주소 밖으로 요청할 수 없습니다".into()));
        }
        for attempt in 0..2 {
            let auth = format!("Bearer {}", self.token);
            let response = match (method, form) {
                ("GET", _) => self.agent.get(url).header("Authorization", &auth).header("Accept", "application/json+canvas-string-ids, application/json").call(),
                ("POST", Some(fields)) => self.agent.post(url).header("Authorization", &auth).header("Accept", "application/json")
                    .send_form(fields.iter().map(|(k, v)| (k.as_str(), v.as_str()))),
                ("PUT", Some(fields)) => self.agent.put(url).header("Authorization", &auth).header("Accept", "application/json")
                    .send_form(fields.iter().map(|(k, v)| (k.as_str(), v.as_str()))),
                _ => return Err(CanvasError::Invalid("unsupported request".into())),
            };
            let mut response = response.map_err(|e| CanvasError::Network(e.to_string()))?;
            let status = response.status().as_u16();
            let link = response.headers().get("link").and_then(|h| h.to_str().ok()).map(str::to_string);
            let remaining = response.headers().get("x-rate-limit-remaining").and_then(|h| h.to_str().ok()).map(str::to_string);
            let body = response.body_mut().with_config().limit(MAX_BODY).read_to_string()
                .map_err(|e| CanvasError::Network(e.to_string()))?;
            if (300..400).contains(&status) {
                return Err(CanvasError::Http(status, "예상하지 못한 리다이렉트입니다. Canvas 주소를 확인하세요.".into()));
            }
            match Self::check(status, &body, remaining.as_deref()) {
                Err(CanvasError::RateLimited) if attempt == 0 => std::thread::sleep(Duration::from_secs(3)),
                Err(e) => return Err(e),
                Ok(()) => return Ok((body, link)),
            }
        }
        Err(CanvasError::RateLimited)
    }

    pub fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let mut url = url::Url::parse(&self.url(path)).map_err(|e| CanvasError::Invalid(e.to_string()))?;
        for (k, v) in query {
            url.query_pairs_mut().append_pair(k, v);
        }
        let (body, _) = self.send("GET", url.as_str(), None)?;
        serde_json::from_str(&body).map_err(|e| CanvasError::Invalid(format!("Canvas 응답을 해석할 수 없습니다: {e}")))
    }

    /// GET a paginated list, following `Link: rel="next"` within the Canvas origin.
    pub fn get_all(&self, path: &str, query: &[(&str, String)], max_items: usize) -> Result<Vec<Value>> {
        let mut url = url::Url::parse(&self.url(path)).map_err(|e| CanvasError::Invalid(e.to_string()))?;
        url.query_pairs_mut().append_pair("per_page", "100");
        for (k, v) in query {
            url.query_pairs_mut().append_pair(k, v);
        }
        let mut next = Some(url.to_string());
        let mut items = Vec::new();
        let mut pages = 0;
        while let Some(page_url) = next.take() {
            pages += 1;
            let (body, link) = self.send("GET", &page_url, None)?;
            let value: Value = serde_json::from_str(&body).map_err(|e| CanvasError::Invalid(format!("Canvas 응답을 해석할 수 없습니다: {e}")))?;
            match value {
                Value::Array(list) => items.extend(list),
                other => items.push(other),
            }
            if items.len() >= max_items || pages >= MAX_PAGES {
                items.truncate(max_items);
                break;
            }
            next = link.as_deref().and_then(next_link).filter(|n| same_origin(&self.base, n));
        }
        Ok(items)
    }

    pub fn post_form(&self, path: &str, fields: &[(String, String)]) -> Result<Value> {
        let (body, _) = self.send("POST", &self.url(path), Some(fields))?;
        Ok(serde_json::from_str(&body).unwrap_or(Value::Null))
    }

    // ----- read operations (compact projections for UI and AI) -----

    pub fn profile(&self) -> Result<Value> {
        let p = self.get("users/self/profile", &[])?;
        Ok(pick(&p, &["id", "name", "short_name", "primary_email", "login_id", "time_zone", "locale"]))
    }

    pub fn courses(&self, include_past: bool) -> Result<Vec<Value>> {
        let mut q = vec![("include[]", "term".to_string()), ("include[]", "total_scores".to_string())];
        if !include_past {
            q.push(("enrollment_state", "active".to_string()));
        }
        let list = self.get_all("courses", &q, 200)?;
        Ok(list.iter().filter(|c| c.get("name").is_some()).map(|c| {
            let mut out = pick(c, &["id", "name", "course_code", "workflow_state", "start_at", "end_at"]);
            if let Some(term) = c.get("term").and_then(|t| t.get("name")) {
                out["term"] = term.clone();
            }
            if let Some(e) = c.get("enrollments").and_then(Value::as_array).and_then(|a| a.first()) {
                for k in ["computed_current_score", "computed_final_score", "computed_current_grade", "computed_final_grade"] {
                    if let Some(x) = e.get(k).filter(|x| !x.is_null()) {
                        out[k] = x.clone();
                    }
                }
            }
            out["html_url"] = json!(format!("{}/courses/{}", self.base, c.get("id").map(|v| v.to_string().trim_matches('"').to_string()).unwrap_or_default()));
            out
        }).collect())
    }

    fn assignment_view(a: &Value, with_description: bool) -> Value {
        let mut out = pick(a, &["id", "course_id", "name", "due_at", "lock_at", "unlock_at", "points_possible", "submission_types", "html_url", "has_submitted_submissions", "locked_for_user", "allowed_extensions"]);
        if let Some(s) = a.get("submission").filter(|s| s.is_object()) {
            out["submission"] = pick(s, &["workflow_state", "submitted_at", "score", "grade", "late", "missing", "excused", "attempt"]);
        }
        if with_description {
            if let Some(d) = html_field(a, "description") {
                out["description"] = json!(d);
            }
        }
        out
    }

    /// bucket: past | overdue | undated | ungraded | unsubmitted | upcoming | future
    pub fn assignments(&self, course_id: u64, bucket: Option<&str>) -> Result<Vec<Value>> {
        let mut q = vec![("include[]", "submission".to_string()), ("order_by", "due_at".to_string())];
        if let Some(b) = bucket {
            if !matches!(b, "past" | "overdue" | "undated" | "ungraded" | "unsubmitted" | "upcoming" | "future") {
                return Err(CanvasError::Invalid("bucket 값이 올바르지 않습니다".into()));
            }
            q.push(("bucket", b.to_string()));
        }
        let list = self.get_all(&format!("courses/{course_id}/assignments"), &q, 300)?;
        Ok(list.iter().map(|a| Self::assignment_view(a, false)).collect())
    }

    pub fn assignment(&self, course_id: u64, assignment_id: u64) -> Result<Value> {
        let a = self.get(&format!("courses/{course_id}/assignments/{assignment_id}"), &[("include[]", "submission".to_string())])?;
        Ok(Self::assignment_view(&a, true))
    }

    /// Upcoming work across every active course (Canvas to-do + upcoming events).
    pub fn upcoming(&self) -> Result<Value> {
        let todo = self.get_all("users/self/todo", &[], 100)?;
        let events = self.get_all("users/self/upcoming_events", &[], 100)?;
        let todo: Vec<Value> = todo.iter().map(|t| {
            let mut out = pick(t, &["type", "context_name", "course_id", "html_url"]);
            if let Some(a) = t.get("assignment") {
                out["assignment"] = Self::assignment_view(a, false);
            }
            out
        }).collect();
        let events: Vec<Value> = events.iter().map(|e| {
            let mut out = pick(e, &["id", "title", "type", "start_at", "end_at", "context_name", "context_code", "html_url", "location_name"]);
            if let Some(a) = e.get("assignment") {
                out["assignment"] = Self::assignment_view(a, false);
            }
            out
        }).collect();
        Ok(json!({"todo": todo, "upcoming_events": events}))
    }

    pub fn announcements(&self, course_ids: &[u64], days_back: i64) -> Result<Vec<Value>> {
        if course_ids.is_empty() {
            return Ok(vec![]);
        }
        let (start, end) = days_window(days_back.clamp(1, 365), 1);
        let mut q: Vec<(&str, String)> = course_ids.iter().map(|id| ("context_codes[]", format!("course_{id}"))).collect();
        q.push(("start_date", start));
        q.push(("end_date", end));
        let list = self.get_all("announcements", &q, 200)?;
        Ok(list.iter().map(|a| {
            let mut out = pick(a, &["id", "title", "posted_at", "author", "context_code", "html_url", "read_state"]);
            if let Some(author) = a.get("author").and_then(|x| x.get("display_name")) {
                out["author"] = author.clone();
            }
            if let Some(m) = html_field(a, "message") {
                out["message"] = json!(m);
            }
            out
        }).collect())
    }

    pub fn grades(&self) -> Result<Vec<Value>> {
        let enrollments = self.get_all("users/self/enrollments", &[("type[]", "StudentEnrollment".to_string()), ("state[]", "active".to_string())], 200)?;
        Ok(enrollments.iter().map(|e| {
            let mut out = pick(e, &["course_id", "enrollment_state"]);
            if let Some(g) = e.get("grades") {
                out["grades"] = pick(g, &["current_score", "final_score", "current_grade", "final_grade", "html_url"]);
            }
            out
        }).collect())
    }

    pub fn submissions(&self, course_id: u64) -> Result<Vec<Value>> {
        let list = self.get_all(&format!("courses/{course_id}/students/submissions"), &[
            ("student_ids[]", "self".to_string()), ("include[]", "assignment".to_string()),
        ], 300)?;
        Ok(list.iter().map(|s| {
            let mut out = pick(s, &["assignment_id", "workflow_state", "submitted_at", "graded_at", "score", "grade", "late", "missing", "excused", "attempt", "submission_type"]);
            if let Some(a) = s.get("assignment") {
                out["assignment_name"] = a.get("name").cloned().unwrap_or(Value::Null);
                out["due_at"] = a.get("due_at").cloned().unwrap_or(Value::Null);
                out["points_possible"] = a.get("points_possible").cloned().unwrap_or(Value::Null);
            }
            if let Some(c) = s.get("submission_comments").and_then(Value::as_array).filter(|c| !c.is_empty()) {
                out["comments"] = json!(c.iter().map(|x| pick(x, &["author_name", "comment", "created_at"])).collect::<Vec<_>>());
            }
            out
        }).collect())
    }

    pub fn files(&self, course_id: u64, search: Option<&str>) -> Result<Vec<Value>> {
        let mut q = vec![("sort", "updated_at".to_string()), ("order", "desc".to_string())];
        if let Some(term) = search.filter(|s| s.chars().count() >= 2) {
            q.push(("search_term", term.to_string()));
        }
        let list = self.get_all(&format!("courses/{course_id}/files"), &q, 300)?;
        Ok(list.iter().map(|f| pick(f, &["id", "display_name", "filename", "size", "content-type", "updated_at", "locked_for_user"])).collect())
    }

    pub fn modules(&self, course_id: u64) -> Result<Vec<Value>> {
        let list = self.get_all(&format!("courses/{course_id}/modules"), &[("include[]", "items".to_string())], 200)?;
        Ok(list.iter().map(|m| {
            let mut out = pick(m, &["id", "name", "position", "state", "unlock_at"]);
            if let Some(items) = m.get("items").and_then(Value::as_array) {
                out["items"] = json!(items.iter().map(|i| pick(i, &["id", "title", "type", "content_id", "page_url", "html_url", "completion_requirement"])).collect::<Vec<_>>());
            }
            out
        }).collect())
    }

    pub fn page(&self, course_id: u64, page_url: &str) -> Result<Value> {
        let slug: String = page_url.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')).collect();
        if slug.is_empty() {
            return Err(CanvasError::Invalid("page_url 값이 필요합니다".into()));
        }
        let p = self.get(&format!("courses/{course_id}/pages/{slug}"), &[])?;
        let mut out = pick(&p, &["page_id", "url", "title", "updated_at", "html_url"]);
        if let Some(b) = html_field(&p, "body") {
            out["body"] = json!(b);
        }
        Ok(out)
    }

    pub fn discussions(&self, course_id: u64) -> Result<Vec<Value>> {
        let list = self.get_all(&format!("courses/{course_id}/discussion_topics"), &[], 200)?;
        Ok(list.iter().map(|d| {
            let mut out = pick(d, &["id", "title", "posted_at", "last_reply_at", "discussion_subentry_count", "read_state", "unread_count", "html_url", "locked"]);
            if let Some(m) = html_field(d, "message") {
                out["message"] = json!(truncate(m, 1500));
            }
            out
        }).collect())
    }

    pub fn calendar(&self, course_ids: &[u64], days_ahead: i64) -> Result<Vec<Value>> {
        let (start, end) = days_window(0, days_ahead.clamp(1, 120));
        let mut contexts: Vec<(&str, String)> = course_ids.iter().map(|id| ("context_codes[]", format!("course_{id}"))).collect();
        if let Ok(me) = self.get("users/self", &[]) {
            if let Some(id) = me.get("id") {
                contexts.push(("context_codes[]", format!("user_{}", id.to_string().trim_matches('"'))));
            }
        }
        let mut out = Vec::new();
        // Canvas accepts at most 10 context codes per request.
        for chunk in contexts.chunks(10) {
            for kind in ["event", "assignment"] {
                let mut q = chunk.to_vec();
                q.push(("type", kind.to_string()));
                q.push(("start_date", start.clone()));
                q.push(("end_date", end.clone()));
                for e in self.get_all("calendar_events", &q, 300)? {
                    out.push(pick(&e, &["id", "title", "start_at", "end_at", "all_day", "context_code", "location_name", "html_url", "type"]));
                }
            }
        }
        out.sort_by(|a, b| a.get("start_at").and_then(Value::as_str).cmp(&b.get("start_at").and_then(Value::as_str)));
        Ok(out)
    }

    /// Downloads a course file to `dir`, returning the saved path.
    pub fn download_file(&self, file_id: u64, dir: &std::path::Path) -> Result<std::path::PathBuf> {
        let meta = self.get(&format!("files/{file_id}"), &[])?;
        let url = meta.get("url").and_then(Value::as_str).filter(|u| u.starts_with("https://"))
            .ok_or_else(|| CanvasError::Invalid("이 파일은 다운로드할 수 없습니다 (잠김 또는 권한 없음).".into()))?;
        let name = meta.get("display_name").and_then(Value::as_str).unwrap_or("download");
        let safe: String = name.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) || c.is_control() { '_' } else { c }).collect();
        let safe = if safe.trim().is_empty() { format!("file-{file_id}") } else { safe.trim().to_string() };
        std::fs::create_dir_all(dir).map_err(|e| CanvasError::Invalid(format!("폴더를 만들 수 없습니다: {e}")))?;
        let mut target = dir.join(&safe);
        let mut n = 1;
        while target.exists() {
            let stem = std::path::Path::new(&safe).file_stem().and_then(|s| s.to_str()).unwrap_or("file");
            let ext = std::path::Path::new(&safe).extension().and_then(|s| s.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
            target = dir.join(format!("{stem} ({n}){ext}"));
            n += 1;
        }
        // Canvas file URLs carry their own verifier; only send the token to the Canvas origin.
        let request = self.download_agent.get(url);
        let request = if same_origin(&self.base, url) { request.header("Authorization", &format!("Bearer {}", self.token)) } else { request };
        let mut response = request.call().map_err(|e| CanvasError::Network(e.to_string()))?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(CanvasError::Http(status, "파일 다운로드 실패".into()));
        }
        let mut reader = response.body_mut().with_config().limit(2 * 1024 * 1024 * 1024).reader();
        let mut file = std::fs::File::create(&target).map_err(|e| CanvasError::Invalid(format!("파일을 저장할 수 없습니다: {e}")))?;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf).map_err(|e| CanvasError::Network(e.to_string()))?;
            if n == 0 {
                break;
            }
            std::io::Write::write_all(&mut file, &buf[..n]).map_err(|e| CanvasError::Invalid(e.to_string()))?;
        }
        Ok(target)
    }

    // ----- write operations (only executed after explicit user approval) -----

    pub fn execute_write(&self, kind: &str, p: &Value) -> Result<Value> {
        match kind {
            "submit_text" => {
                let course = require_id(p, "course_id")?;
                let assignment = require_id(p, "assignment_id")?;
                let body = opt_str(p, "body").ok_or_else(|| CanvasError::Invalid("제출 내용이 비어 있습니다".into()))?;
                let r = self.post_form(&format!("courses/{course}/assignments/{assignment}/submissions"), &[
                    ("submission[submission_type]".into(), "online_text_entry".into()),
                    ("submission[body]".into(), body.into()),
                ])?;
                Ok(pick(&r, &["id", "workflow_state", "submitted_at", "attempt", "preview_url"]))
            }
            "submit_url" => {
                let course = require_id(p, "course_id")?;
                let assignment = require_id(p, "assignment_id")?;
                let link = opt_str(p, "url").filter(|u| u.starts_with("https://") || u.starts_with("http://"))
                    .ok_or_else(|| CanvasError::Invalid("제출할 URL이 올바르지 않습니다".into()))?;
                let r = self.post_form(&format!("courses/{course}/assignments/{assignment}/submissions"), &[
                    ("submission[submission_type]".into(), "online_url".into()),
                    ("submission[url]".into(), link.into()),
                ])?;
                Ok(pick(&r, &["id", "workflow_state", "submitted_at", "attempt"]))
            }
            "discussion_reply" => {
                let course = require_id(p, "course_id")?;
                let topic = require_id(p, "topic_id")?;
                let message = opt_str(p, "message").ok_or_else(|| CanvasError::Invalid("댓글 내용이 비어 있습니다".into()))?;
                let r = self.post_form(&format!("courses/{course}/discussion_topics/{topic}/entries"), &[("message".into(), message.into())])?;
                Ok(pick(&r, &["id", "created_at"]))
            }
            "planner_note" => {
                let title = opt_str(p, "title").ok_or_else(|| CanvasError::Invalid("할 일 제목이 필요합니다".into()))?;
                let date = opt_str(p, "todo_date").ok_or_else(|| CanvasError::Invalid("날짜(todo_date)가 필요합니다".into()))?;
                let mut fields = vec![("title".to_string(), title.to_string()), ("todo_date".to_string(), date.to_string())];
                if let Some(d) = opt_str(p, "details") {
                    fields.push(("details".into(), d.into()));
                }
                if let Ok(course) = require_id(p, "course_id") {
                    fields.push(("course_id".into(), course.to_string()));
                }
                let r = self.post_form("planner_notes", &fields)?;
                Ok(pick(&r, &["id", "title", "todo_date"]))
            }
            "calendar_event" => {
                let title = opt_str(p, "title").ok_or_else(|| CanvasError::Invalid("일정 제목이 필요합니다".into()))?;
                let start = opt_str(p, "start_at").ok_or_else(|| CanvasError::Invalid("시작 시각(start_at)이 필요합니다".into()))?;
                let me = self.get("users/self", &[])?;
                let user_id = me.get("id").map(|v| v.to_string().trim_matches('"').to_string()).ok_or(CanvasError::Unauthorized)?;
                let mut fields = vec![
                    ("calendar_event[context_code]".to_string(), format!("user_{user_id}")),
                    ("calendar_event[title]".to_string(), title.to_string()),
                    ("calendar_event[start_at]".to_string(), start.to_string()),
                ];
                if let Some(end) = opt_str(p, "end_at") {
                    fields.push(("calendar_event[end_at]".into(), end.into()));
                }
                if let Some(d) = opt_str(p, "description") {
                    fields.push(("calendar_event[description]".into(), d.into()));
                }
                let r = self.post_form("calendar_events", &fields)?;
                Ok(pick(&r, &["id", "title", "start_at", "end_at", "html_url"]))
            }
            _ => Err(CanvasError::Invalid("지원하지 않는 작업입니다".into())),
        }
    }
}

/// Human-readable Korean summary of a proposed write, shown in the approval queue.
pub fn describe_write(kind: &str, p: &Value) -> String {
    let s = |k: &str| p.get(k).map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).unwrap_or_default();
    let named = |name_key: &str, id_key: &str, prefix: &str| {
        let name = s(name_key);
        if name.is_empty() { format!("{prefix} {}", s(id_key)) } else { name }
    };
    let course = named("_course_name", "course_id", "강의");
    match kind {
        "submit_text" => format!("과제 텍스트 제출 — {course} / {} ({}자)", named("_assignment_name", "assignment_id", "과제"), s("body").chars().count()),
        "submit_url" => format!("과제 URL 제출 — {course} / {}: {}", named("_assignment_name", "assignment_id", "과제"), s("url")),
        "discussion_reply" => format!("토론 댓글 작성 — {course} / {}", named("_topic_title", "topic_id", "토론")),
        "planner_note" => format!("Canvas 할 일 추가 — {}", s("title")),
        "calendar_event" => format!("개인 캘린더 일정 추가 — {}", s("title")),
        _ => format!("알 수 없는 작업: {kind}"),
    }
}

pub const WRITE_KINDS: &[&str] = &["submit_text", "submit_url", "discussion_reply", "planner_note", "calendar_event"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_base_urls() {
        assert_eq!(normalize_base_url("canvas.example.ac.kr/").unwrap(), "https://canvas.example.ac.kr");
        assert_eq!(normalize_base_url(" https://canvas.example.ac.kr/courses/1 ").unwrap(), "https://canvas.example.ac.kr");
        assert_eq!(normalize_base_url("https://lms.example.com:8443").unwrap(), "https://lms.example.com:8443");
        assert!(normalize_base_url("http://canvas.example.ac.kr").is_err());
        assert!(normalize_base_url("https://user:pw@canvas.example.ac.kr").is_err());
        assert!(normalize_base_url("localhost").is_err());
        assert!(normalize_base_url("").is_err());
    }

    #[test]
    fn parses_link_header() {
        let h = r#"<https://c.example.com/api/v1/courses?page=1>; rel="current",<https://c.example.com/api/v1/courses?page=2>; rel="next", <https://c.example.com/api/v1/courses?page=9>; rel="last""#;
        assert_eq!(next_link(h).as_deref(), Some("https://c.example.com/api/v1/courses?page=2"));
        assert_eq!(next_link(r#"<https://c.example.com/x>; rel="last""#), None);
    }

    #[test]
    fn origin_check_blocks_token_leaks() {
        assert!(same_origin("https://c.example.com", "https://c.example.com/api/v1/x?page=2"));
        assert!(!same_origin("https://c.example.com", "https://evil.example.net/api/v1/x"));
        assert!(!same_origin("https://c.example.com", "http://c.example.com/api"));
    }

    #[test]
    fn converts_html() {
        let t = html_to_text("<p>안녕&nbsp;하세요</p><ul><li>하나</li><li>둘 &amp; 셋</li></ul><script>alert(1)</script><p>끝&#33;</p>");
        assert_eq!(t, "안녕 하세요\n- 하나\n- 둘 & 셋\n끝!");
    }

    #[test]
    fn requires_numeric_ids() {
        assert_eq!(require_id(&json!({"course_id": "12"}), "course_id").unwrap(), 12);
        assert_eq!(require_id(&json!({"course_id": 7}), "course_id").unwrap(), 7);
        assert!(require_id(&json!({"course_id": "1/../x"}), "course_id").is_err());
        assert!(require_id(&json!({}), "course_id").is_err());
    }

    /// Minimal HTTP/1.1 server answering scripted responses on loopback.
    fn mock_server(responses: Vec<(u16, Vec<(&'static str, String)>, String)>) -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for (status, headers, body) in responses {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                        break;
                    }
                    request.push_str(&line);
                }
                tx.send(request).unwrap();
                let mut out = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
                for (k, v) in headers {
                    out.push_str(&format!("{k}: {v}\r\n"));
                }
                out.push_str("\r\n");
                out.push_str(&body);
                let mut stream = stream;
                stream.write_all(out.as_bytes()).unwrap();
            }
        });
        (base, rx)
    }

    #[test]
    fn sends_token_and_stops_at_foreign_next_links() {
        let first = r#"[{"id":"1","name":"열전달","course_code":"HT","enrollments":[{"computed_current_score":91.5}]},{"id":"9"}]"#.to_string();
        let (base, rx) = mock_server(vec![
            (200, vec![("Link", "<https://evil.example.net/api/v1/courses?page=2>; rel=\"next\"".into())], first),
        ]);
        let client = Client::for_test(&base, "secret-token-1234567890");
        let courses = client.courses(false).unwrap();
        assert_eq!(courses.len(), 1, "entries without a name are skipped");
        assert_eq!(courses[0]["computed_current_score"], 91.5);
        let req = rx.recv().unwrap().to_ascii_lowercase();
        assert!(req.contains("authorization: bearer secret-token-1234567890"));
        assert!(req.contains("enrollment_state=active"));
        assert!(rx.recv_timeout(std::time::Duration::from_millis(300)).is_err(), "foreign next link must not be fetched");
    }

    #[test]
    fn retries_rate_limit_then_follows_same_origin_pages_only() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let base = format!("http://{addr}");
        let next = format!("<{base}/api/v1/users/self/todo?page=2>; rel=\"next\"");
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            use std::io::{BufRead, BufReader, Write};
            let script: Vec<(u16, Option<String>, &str)> = vec![
                (200, Some(next), r#"[{"type":"submitting","context_name":"A","assignment":{"id":"5","name":"보고서","due_at":"2030-01-01T00:00:00Z"}}]"#),
                (403, None, "Rate Limit Exceeded"),
                (200, Some("<https://evil.example.net/x>; rel=\"next\"".into()), r#"[{"type":"grading","context_name":"B"}]"#),
                (200, None, r#"[]"#),
            ];
            for (status, link, body) in script {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                loop {
                    let mut l = String::new();
                    if reader.read_line(&mut l).unwrap() == 0 || l == "\r\n" { break; }
                }
                tx.send(first).unwrap();
                let mut out = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
                if let Some(l) = link { out.push_str(&format!("Link: {l}\r\n")); }
                out.push_str("\r\n");
                out.push_str(body);
                let mut stream = stream;
                stream.write_all(out.as_bytes()).unwrap();
            }
        });
        let client = Client::for_test(&base, "secret-token-1234567890");
        let up = client.upcoming().unwrap();
        let todo = up["todo"].as_array().unwrap();
        assert_eq!(todo.len(), 2, "page 1 + page 2 after rate-limit retry; evil next link ignored");
        assert_eq!(todo[0]["assignment"]["name"], "보고서");
        let requests: Vec<String> = rx.try_iter().collect();
        assert!(requests[0].starts_with("GET /api/v1/users/self/todo?per_page=100"));
        assert!(requests[1].contains("page=2"));
        assert!(requests[2].contains("page=2"), "retried after rate limit");
        assert!(requests[3].starts_with("GET /api/v1/users/self/upcoming_events"));
        assert!(requests.iter().all(|r| !r.contains("evil")));
    }

    #[test]
    fn maps_auth_errors() {
        let (base, _rx) = mock_server(vec![(401, vec![], r#"{"errors":[{"message":"Invalid access token."}]}"#.into())]);
        let client = Client::for_test(&base, "bad-token-1234567890123");
        assert!(matches!(client.profile(), Err(CanvasError::Unauthorized)));
    }

    #[test]
    fn describes_writes() {
        assert!(describe_write("submit_url", &json!({"course_id": 1, "assignment_id": 2, "url": "https://x.y"})).contains("https://x.y"));
        assert!(describe_write("weird", &json!({})).contains("알 수 없는"));
        let named = describe_write("submit_text", &json!({"course_id": 1, "assignment_id": 2, "body": "abc", "_course_name": "열전달", "_assignment_name": "HW6"}));
        assert_eq!(named, "과제 텍스트 제출 — 열전달 / HW6 (3자)");
    }
}
