//! LearningX (Coursemos) course materials, used by schools whose "강의자료실" menu is an LTI tool.
//!
//! Flow: course tabs → external tool id → Canvas `sessionless_launch` (with the Canvas token)
//! → the returned one-time launch page (no token) → its LTI form is POSTed to LearningX →
//! LearningX sets session cookies and an `xn_api_token` → resource list API → content downloads.
//!
//! The Canvas token is never sent to LearningX or the content host. LearningX cookies are kept
//! in memory only, per process, and expire after a short time.
use crate::canvas::{same_origin, CanvasError, Client, Result};
use crate::extract;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::io::Read;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const SESSION_TTL: Duration = Duration::from_secs(20 * 60);
const MAX_HTML: u64 = 5 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
struct Cookie {
    domain: String,
    host_only: bool,
    name: String,
    value: String,
}

/// Minimal in-memory cookie jar (domain matching only; enough for LTI session cookies).
#[derive(Clone, Default, Debug)]
pub struct Jar(Vec<Cookie>);

impl Jar {
    fn store(&mut self, host: &str, set_cookie: &str) {
        let mut parts = set_cookie.split(';');
        let Some((name, value)) = parts.next().and_then(|kv| kv.split_once('=')) else { return };
        let (name, value) = (name.trim().to_string(), value.trim().trim_matches('"').to_string());
        if name.is_empty() {
            return;
        }
        let mut domain = host.to_ascii_lowercase();
        let mut host_only = true;
        let mut expired = false;
        for attr in parts {
            let (k, v) = attr.split_once('=').map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim())).unwrap_or((attr.trim().to_ascii_lowercase(), ""));
            match k.as_str() {
                "domain" if !v.is_empty() => {
                    let d = v.trim_start_matches('.').to_ascii_lowercase();
                    // Only accept the response host or a parent domain of it.
                    if host.eq_ignore_ascii_case(&d) || host.to_ascii_lowercase().ends_with(&format!(".{d}")) {
                        if d.contains('.') {
                            domain = d;
                            host_only = false;
                        }
                    }
                }
                "max-age" if v.trim_start_matches('-').parse::<i64>().map(|n| n <= 0).unwrap_or(false) => expired = true,
                _ => {}
            }
        }
        self.0.retain(|c| !(c.name == name && c.domain == domain));
        if !expired && !value.is_empty() {
            self.0.push(Cookie { domain, host_only, name, value });
        }
    }

    fn header_for(&self, host: &str) -> Option<String> {
        let host = host.to_ascii_lowercase();
        let pairs: Vec<String> = self.0.iter()
            .filter(|c| c.domain == host || (!c.host_only && host.ends_with(&format!(".{}", c.domain))))
            .map(|c| format!("{}={}", c.name, c.value))
            .collect();
        (!pairs.is_empty()).then(|| pairs.join("; "))
    }

    fn get(&self, name: &str) -> Option<String> {
        self.0.iter().rev().find(|c| c.name == name).map(|c| c.value.clone())
    }
}

/// A browser-like HTTP session for LearningX (cookies, manual redirects). Never carries the Canvas token.
pub struct Session {
    agent: ureq::Agent,
    jar: Jar,
    /// Origin of the LearningX app (e.g. https://canvas.school.ac.kr).
    pub lx_origin: String,
    pub api_token: Option<String>,
    /// Host that serves downloadable content (e.g. https://cmaker.school.ac.kr), learned from resources.
    pub content_origin: Option<String>,
    pub tool_id: u64,
    created: Instant,
}

pub struct Response {
    pub status: u16,
    pub url: String,
    pub content_type: String,
    pub disposition: String,
    pub body: Vec<u8>,
}

fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_string))
}

fn origin_of(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    Some(format!("{}://{}{}", u.scheme(), u.host_str()?, u.port().map(|p| format!(":{p}")).unwrap_or_default()))
}

impl Session {
    fn new() -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(Duration::from_secs(120)))
            .user_agent(concat!("Mozilla/5.0 (Windows NT 10.0; Win64; x64) CanvasAssistant/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        Self { agent, jar: Jar::default(), lx_origin: String::new(), api_token: None, content_origin: None, tool_id: 0, created: Instant::now() }
    }

    /// GET or form-POST with cookies, following up to 10 redirects (POST becomes GET after 301/302/303).
    pub fn request(&mut self, method: &str, url: &str, form: Option<&[(String, String)]>, bearer: Option<&str>, limit: u64) -> Result<Response> {
        let mut url = url.to_string();
        let mut method = method.to_string();
        let mut form = form.map(|f| f.to_vec());
        for _ in 0..10 {
            let parsed = url::Url::parse(&url).map_err(|_| CanvasError::Invalid("LearningX 주소가 올바르지 않습니다".into()))?;
            let local = matches!(parsed.host_str(), Some("127.0.0.1") | Some("localhost"));
            if parsed.scheme() != "https" && !(cfg!(test) && local) {
                return Err(CanvasError::Invalid("LearningX는 https 주소만 사용할 수 있습니다".into()));
            }
            let host = parsed.host_str().unwrap_or("").to_string();
            let cookie = self.jar.header_for(&host);
            let result = if method == "POST" {
                let mut r = self.agent.post(&url);
                if let Some(c) = &cookie { r = r.header("Cookie", c); }
                if let Some(b) = bearer { r = r.header("Authorization", &format!("Bearer {b}")); }
                let body: Vec<(&str, &str)> = form.as_deref().unwrap_or(&[]).iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                r.send_form(body)
            } else {
                let mut r = self.agent.get(&url).header("Accept", "text/html,application/json,*/*");
                if let Some(c) = &cookie { r = r.header("Cookie", c); }
                if let Some(b) = bearer { r = r.header("Authorization", &format!("Bearer {b}")); }
                r.call()
            };
            let mut resp = result.map_err(|e| CanvasError::Network(e.to_string()))?;
            for v in resp.headers().get_all("set-cookie") {
                if let Ok(s) = v.to_str() {
                    self.jar.store(&host, s);
                }
            }
            let status = resp.status().as_u16();
            if (300..400).contains(&status) && status != 304 {
                let loc = resp.headers().get("location").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
                let next = parsed.join(&loc).map_err(|_| CanvasError::Invalid("LearningX 리다이렉트를 따라갈 수 없습니다".into()))?;
                // The bearer token belongs to one origin only.
                if bearer.is_some() && origin_of(next.as_str()) != origin_of(&url) {
                    return Err(CanvasError::Invalid("LearningX가 다른 주소로 보냈습니다".into()));
                }
                url = next.to_string();
                if status != 307 && status != 308 {
                    method = "GET".into();
                    form = None;
                }
                continue;
            }
            let header = |n: &str| resp.headers().get(n).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
            let content_type = header("content-type");
            let disposition = header("content-disposition");
            let mut body = Vec::new();
            resp.body_mut().with_config().limit(limit + 1).reader().take(limit + 1).read_to_end(&mut body).map_err(|e| CanvasError::Network(e.to_string()))?;
            if body.len() as u64 > limit {
                return Err(CanvasError::Invalid(format!("파일이 너무 커서 ({}MB 초과) 읽을 수 없습니다", limit >> 20)));
            }
            return Ok(Response { status, url, content_type, disposition, body });
        }
        Err(CanvasError::Invalid("LearningX 리다이렉트가 너무 많습니다".into()))
    }
}

/// An HTML form found on the LTI launch page.
#[derive(Debug, PartialEq)]
pub struct Form {
    pub action: String,
    pub method: String,
    pub fields: Vec<(String, String)>,
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        let before_ok = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = &tag[at + name.len()..];
        let rest_trim = rest.trim_start();
        if before_ok && rest_trim.starts_with('=') {
            let v = rest_trim[1..].trim_start();
            let value = if let Some(q) = v.chars().next().filter(|c| *c == '"' || *c == '\'') {
                v[1..].split(q).next().unwrap_or("").to_string()
            } else {
                v.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or("").to_string()
            };
            return Some(decode_html(&value));
        }
        from = at + name.len();
    }
    None
}

fn decode_html(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&#39;", "'").replace("&#x27;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&#x2F;", "/").replace("&#47;", "/").replace("&amp;", "&")
}

/// Finds the LTI launch form (Canvas renders `<form id="tool_form" ...>` with hidden inputs).
pub fn parse_launch_form(html: &str, page_url: &str) -> Option<Form> {
    let lower = html.to_ascii_lowercase();
    let mut forms = Vec::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<form") {
        let start = from + i;
        let Some(tag_end) = lower[start..].find('>').map(|e| start + e) else { break };
        let end = lower[tag_end..].find("</form").map(|e| tag_end + e).unwrap_or(lower.len());
        forms.push((start, tag_end, end));
        from = end.max(tag_end + 1);
    }
    let pick = forms.iter().find(|(s, t, _)| html[*s..*t].contains("tool_form")).or_else(|| forms.iter().find(|(s, t, e)| {
        attr(&html[*s..*t], "action").is_some() && (lower[*t..*e].contains("oauth_signature") || lower[*t..*e].contains("id_token") || lower[*t..*e].contains("lti_message_type"))
    })).or_else(|| forms.first())?;
    let (s, t, e) = *pick;
    let tag = &html[s..=t];
    let action = attr(tag, "action")?;
    let action = url::Url::parse(page_url).ok()?.join(&action).ok()?.to_string();
    let method = attr(tag, "method").unwrap_or_else(|| "get".into()).to_ascii_uppercase();
    let mut fields = Vec::new();
    let body = &html[t..e];
    let body_lower = &lower[t..e];
    let mut from = 0;
    while let Some(i) = body_lower[from..].find("<input") {
        let start = from + i;
        let Some(end) = body_lower[start..].find('>').map(|x| start + x) else { break };
        let input = &body[start..=end];
        if let Some(name) = attr(input, "name") {
            let ty = attr(input, "type").unwrap_or_default().to_ascii_lowercase();
            if ty != "submit" && ty != "button" {
                fields.push((name, attr(input, "value").unwrap_or_default()));
            }
        }
        from = end;
    }
    // <textarea name=...>value</textarea> is used by some LTI 1.3 launches.
    let mut from = 0;
    while let Some(i) = body_lower[from..].find("<textarea") {
        let start = from + i;
        let Some(end) = body_lower[start..].find('>').map(|x| start + x) else { break };
        let close = body_lower[end..].find("</textarea").map(|x| end + x).unwrap_or(end + 1);
        if let Some(name) = attr(&body[start..=end], "name") {
            fields.push((name, decode_html(&body[end + 1..close])));
        }
        from = close;
    }
    Some(Form { action, method, fields })
}

/// Finds an `xn_api_token` value in page HTML/JS when it is not set as a cookie.
pub fn find_api_token(html: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = html[from..].find("xn_api_token") {
        let at = from + i + "xn_api_token".len();
        let window: String = html[at..].chars().take(300).collect();
        let mut chars = window.char_indices().peekable();
        while let Some((idx, c)) = chars.next() {
            if c == '"' || c == '\'' {
                let rest = &window[idx + 1..];
                if let Some(end) = rest.find(c) {
                    let candidate = &rest[..end];
                    if candidate.len() >= 16 && candidate.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-+/=".contains(&b)) {
                        return Some(candidate.to_string());
                    }
                }
            }
            if c == ';' || c == '\n' {
                break;
            }
        }
        from = at;
    }
    None
}

static SESSIONS: Mutex<Option<HashMap<u64, Session>>> = Mutex::new(None);

fn take_session(course: u64) -> Option<Session> {
    let mut guard = SESSIONS.lock().ok()?;
    let map = guard.get_or_insert_with(HashMap::new);
    let s = map.remove(&course)?;
    (s.created.elapsed() < SESSION_TTL).then_some(s)
}

fn put_session(course: u64, s: Session) {
    if let Ok(mut guard) = SESSIONS.lock() {
        guard.get_or_insert_with(HashMap::new).insert(course, s);
    }
}

fn lx_error(m: impl Into<String>) -> CanvasError {
    CanvasError::Invalid(m.into())
}

/// External-tool tabs of a course, LearningX material tools first.
fn candidate_tools(client: &Client, course: u64) -> Result<Vec<(u64, String)>> {
    let tabs = client.tabs(course)?;
    let mut tools: Vec<(u64, String, i32)> = tabs.iter().filter_map(|t| {
        let id = t.get("id").and_then(Value::as_str)?;
        let tool = id.strip_prefix("context_external_tool_")?.parse::<u64>().ok()?;
        let label = t.get("label").and_then(Value::as_str).unwrap_or("").to_string();
        let rank = if label.contains("강의자료") || label.to_lowercase().contains("resource") { 0 } else if label.contains("자료") || label.contains("콘텐츠") { 1 } else { 2 };
        Some((tool, label, rank))
    }).collect();
    tools.sort_by_key(|t| t.2);
    Ok(tools.into_iter().map(|(id, label, _)| (id, label)).collect())
}

fn launch(client: &Client, course: u64, tool: u64) -> Result<Session> {
    let sl = client.get(&format!("courses/{course}/external_tools/sessionless_launch"), &[("id", tool.to_string()), ("launch_type", "course_navigation".to_string())])?;
    let launch_url = sl.get("url").and_then(Value::as_str).ok_or_else(|| lx_error("Canvas가 외부 도구 실행 주소를 주지 않았습니다"))?.to_string();
    if !same_origin(client.base_url(), &launch_url) {
        return Err(lx_error("외부 도구 실행 주소가 Canvas 밖을 가리킵니다"));
    }
    let mut s = Session::new();
    s.tool_id = tool;
    // The launch URL carries a one-time verifier; it is opened without the Canvas token.
    let page = s.request("GET", &launch_url, None, None, MAX_HTML)?;
    let html = String::from_utf8_lossy(&page.body).into_owned();
    let form = parse_launch_form(&html, &page.url).ok_or_else(|| lx_error("외부 도구 실행 페이지에서 LTI 양식을 찾지 못했습니다"))?;
    if !form.action.starts_with("https://") && !cfg!(test) {
        return Err(lx_error("LTI 양식 주소가 https가 아닙니다"));
    }
    let landed = if form.method == "POST" {
        s.request("POST", &form.action, Some(&form.fields), None, MAX_HTML)?
    } else {
        let mut u = url::Url::parse(&form.action).map_err(|_| lx_error("LTI 양식 주소가 올바르지 않습니다"))?;
        for (k, v) in &form.fields { u.query_pairs_mut().append_pair(k, v); }
        s.request("GET", u.as_str(), None, None, MAX_HTML)?
    };
    if landed.status >= 400 {
        return Err(lx_error(format!("LearningX 로그인이 거부되었습니다 (HTTP {})", landed.status)));
    }
    let landed_html = String::from_utf8_lossy(&landed.body);
    s.api_token = s.jar.get("xn_api_token").or_else(|| find_api_token(&landed_html)).or_else(|| find_api_token(&html));
    s.lx_origin = origin_of(&form.action).unwrap_or_default();
    Ok(s)
}

fn session(client: &Client, course: u64, fresh: bool) -> Result<Session> {
    if !fresh {
        if let Some(s) = take_session(course) {
            return Ok(s);
        }
    }
    let tools = candidate_tools(client, course)?;
    if tools.is_empty() {
        return Err(lx_error("이 강의에는 LearningX(외부 도구) 메뉴가 없습니다. Canvas 파일은 canvas_list_files로 확인하세요."));
    }
    let mut last = None;
    for (tool, _) in tools.iter().take(4) {
        match launch(client, course, *tool) {
            Ok(s) if s.lx_origin.is_empty() => last = Some(lx_error("LearningX 주소를 알 수 없습니다")),
            Ok(mut s) => {
                // Make sure this tool actually serves the resources API.
                match resources_raw(&mut s, course) {
                    Ok(_) => return Ok(s),
                    Err(e) => last = Some(e),
                }
            }
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| lx_error("LearningX에 연결하지 못했습니다")))
}

fn resources_raw(s: &mut Session, course: u64) -> Result<Value> {
    let url = format!("{}/learningx/api/v1/courses/{course}/resources", s.lx_origin);
    let token = s.api_token.clone();
    let r = s.request("GET", &url, None, token.as_deref(), 20 * 1024 * 1024)?;
    if r.status == 401 || r.status == 403 {
        return Err(lx_error(format!("LearningX가 자료 목록 요청을 거부했습니다 (HTTP {})", r.status)));
    }
    if !(200..300).contains(&r.status) {
        return Err(lx_error(format!("LearningX 자료 목록을 가져오지 못했습니다 (HTTP {})", r.status)));
    }
    serde_json::from_slice(&r.body).map_err(|_| lx_error("LearningX 응답이 JSON이 아닙니다 (자료실이 LearningX가 아닐 수 있음)"))
}

fn str_field<'a>(o: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|k| o.get(*k).filter(|v| !v.is_null() && v.as_str() != Some("")))
}

/// Flattens the LearningX resource JSON into simple items. The format is not documented,
/// so this looks for objects with a title and a content id at any depth.
pub fn simplify(v: &Value, content_origin: &mut Option<String>, canvas_host: &str) -> Vec<Value> {
    fn walk(v: &Value, section: Option<String>, out: &mut Vec<Value>, origin: &mut Option<String>, canvas_host: &str) {
        match v {
            Value::Array(a) => a.iter().for_each(|x| walk(x, section.clone(), out, origin, canvas_host)),
            Value::Object(o) => {
                let title = str_field(o, &["title", "name", "content_title", "file_name"]).and_then(Value::as_str).map(str::to_string);
                let commons = o.get("commons_content").and_then(Value::as_object);
                let content_id = str_field(o, &["content_id"]).or_else(|| commons.and_then(|c| str_field(c, &["content_id"]))).map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()));
                // Remember where content is served from (e.g. https://cmaker.school.ac.kr/...).
                for map in [Some(o), commons].into_iter().flatten() {
                    for (k, val) in map {
                        if let Some(u) = val.as_str().filter(|u| u.starts_with("https://")) {
                            if (k.contains("url") || u.contains("content_id=")) && origin.is_none() {
                                if let Some(h) = host_of(u).filter(|h| h != canvas_host) {
                                    *origin = Some(format!("https://{h}"));
                                }
                            }
                        }
                    }
                }
                if let (Some(t), Some(cid)) = (&title, &content_id) {
                    let mut item = json!({"title": t, "content_id": cid});
                    if let Some(sec) = &section { item["section"] = json!(sec); }
                    let src = commons.unwrap_or(o);
                    for (key, keys) in [
                        ("type", &["content_type", "type", "file_type", "progress_support_type"][..]),
                        ("file_name", &["file_name", "filename", "original_file_name"][..]),
                        ("size", &["file_size", "content_size", "size"][..]),
                        ("view_url", &["view_url", "content_url", "url"][..]),
                    ] {
                        if let Some(v) = str_field(src, keys).or_else(|| str_field(o, keys)) { item[key] = v.clone(); }
                    }
                    for key in ["created_at", "updated_at", "unlock_at", "lock_at", "due_at", "description"] {
                        if let Some(v) = str_field(o, &[key]) { item[key] = v.clone(); }
                    }
                    out.push(item);
                }
                let child_section = if content_id.is_none() { title.clone().or(section.clone()) } else { section.clone() };
                for (k, val) in o {
                    if matches!(val, Value::Array(_) | Value::Object(_)) && k != "commons_content" {
                        walk(val, child_section.clone(), out, origin, canvas_host);
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(v, None, &mut out, content_origin, canvas_host);
    let mut seen = std::collections::HashSet::new();
    out.retain(|i| seen.insert(i["content_id"].to_string()));
    out
}

fn with_session<T>(client: &Client, course: u64, f: impl Fn(&mut Session) -> Result<T>) -> Result<T> {
    let mut s = session(client, course, false)?;
    let result = match f(&mut s) {
        Ok(v) => Ok(v),
        Err(_) => {
            // Session may have expired: launch again once.
            s = session(client, course, true)?;
            f(&mut s)
        }
    };
    put_session(course, s);
    result
}

impl Client {
    /// LearningX "강의자료실" items of a course.
    pub fn learningx_resources(&self, course: u64) -> Result<Value> {
        let canvas_host = host_of(self.base_url()).unwrap_or_default();
        with_session(self, course, |s| {
            let raw = resources_raw(s, course)?;
            let mut origin = s.content_origin.clone();
            let items = simplify(&raw, &mut origin, &canvas_host);
            s.content_origin = origin;
            if items.is_empty() {
                let mut text = serde_json::to_string(&raw).unwrap_or_default();
                text.truncate(text.char_indices().nth(20000).map(|(i, _)| i).unwrap_or(text.len()));
                return Ok(json!({"items": [], "raw": text, "note": "자료 형식을 알아보지 못해 원본 일부를 그대로 보냅니다."}));
            }
            Ok(json!({"count": items.len(), "items": items, "note": "content_id로 canvas_learningx_read(내용 읽기)를 호출할 수 있습니다."}))
        })
    }

    /// Downloads one LearningX content item. Returns (file name, bytes).
    pub fn learningx_download(&self, course: u64, content_id: &str, limit: u64) -> Result<(String, Vec<u8>)> {
        if content_id.is_empty() || content_id.len() > 80 || !content_id.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)) {
            return Err(lx_error("content_id가 올바르지 않습니다"));
        }
        let canvas_host = host_of(self.base_url()).unwrap_or_default();
        with_session(self, course, |s| {
            let mut items = Vec::new();
            if let Ok(raw) = resources_raw(s, course) {
                let mut origin = s.content_origin.clone();
                items = simplify(&raw, &mut origin, &canvas_host);
                s.content_origin = origin;
            }
            let item = items.iter().find(|i| i["content_id"].as_str() == Some(content_id)).cloned().unwrap_or(Value::Null);
            let origin = s.content_origin.clone().or_else(|| {
                // Fallback for Coursemos schools: canvas.school.ac.kr → cmaker.school.ac.kr
                canvas_host.split_once('.').map(|(_, rest)| format!("https://cmaker.{rest}"))
            }).ok_or_else(|| lx_error("자료를 내려받을 주소를 알 수 없습니다"))?;
            let url = format!("{origin}/index.php?module=xn_media_content2013&act=dispXn_media_content2013DownloadContent&content_id={content_id}");
            let mut r = s.request("GET", &url, None, None, limit)?;
            let html_like = r.content_type.starts_with("text/html") && r.disposition.is_empty();
            if html_like {
                // Some content hosts need the viewer page opened first to set their own cookie.
                if let Some(view) = item.get("view_url").and_then(Value::as_str).filter(|u| u.starts_with("https://")) {
                    let _ = s.request("GET", view, None, None, MAX_HTML);
                    r = s.request("GET", &url, None, None, limit)?;
                }
            }
            if !(200..300).contains(&r.status) {
                return Err(lx_error(format!("자료를 내려받지 못했습니다 (HTTP {})", r.status)));
            }
            if r.content_type.starts_with("text/html") && r.disposition.is_empty() {
                return Err(lx_error("이 자료는 파일로 내려받을 수 없는 형식(동영상·웹 콘텐츠 등)입니다. 앱의 강의자료실에서 브라우저로 여세요."));
            }
            let name = filename_from_disposition(&r.disposition)
                .or_else(|| item.get("file_name").and_then(Value::as_str).map(str::to_string))
                .or_else(|| item.get("title").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_else(|| format!("learningx-{content_id}"));
            Ok((name, std::mem::take(&mut r.body)))
        })
    }

    pub fn learningx_read(&self, course: u64, content_id: &str, offset: usize) -> Result<Value> {
        let (name, bytes) = self.learningx_download(course, content_id, extract::MAX_READ_BYTES)?;
        let text = extract::extract_text(&name, &bytes).map_err(CanvasError::Invalid)?;
        let mut out = crate::materials::paged(&name, &text, offset, extract::PAGE_CHARS);
        out["content_id"] = json!(content_id);
        Ok(out)
    }
}

pub fn filename_from_disposition(d: &str) -> Option<String> {
    let lower = d.to_ascii_lowercase();
    if let Some(i) = lower.find("filename*=") {
        let v = d[i + 10..].split(';').next()?.trim().trim_matches('"');
        let v = v.split_once("''").map(|(_, v)| v).unwrap_or(v);
        let decoded = url::form_urlencoded::parse(format!("x={}", v.replace('+', "%2B")).as_bytes()).next().map(|(_, v)| v.into_owned())?;
        if !decoded.is_empty() {
            return Some(sanitize(&decoded));
        }
    }
    let i = lower.find("filename=")?;
    let raw = d[i + 9..].split(';').next()?.trim().trim_matches('"');
    // Percent-encoded or raw UTF-8/Latin-1 names.
    let decoded = if raw.contains('%') {
        url::form_urlencoded::parse(format!("x={}", raw.replace('+', "%2B")).as_bytes()).next().map(|(_, v)| v.into_owned()).unwrap_or_else(|| raw.to_string())
    } else {
        raw.to_string()
    };
    (!decoded.is_empty()).then(|| sanitize(&decoded))
}

pub fn sanitize(name: &str) -> String {
    let s: String = name.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() { "download".into() } else { s.chars().take(150).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canvas_tool_form() {
        let html = r#"<html><body><form action="https://canvas.x.ac.kr/learningx/lti/courseresource" method="POST" target="tool_content" id="tool_form" data-tool-launch-type="course_navigation">
            <input type="hidden" name="oauth_consumer_key" value="key" id="oauth_consumer_key" />
            <input type="hidden" name="context_title" value="자료구조 &amp; 알고리즘" />
            <input type="hidden" name='custom_x' value='a"b' />
            <button type="submit">Press to Launch</button></form></body></html>"#;
        let f = parse_launch_form(html, "https://canvas.x.ac.kr/courses/1/external_tools/sessionless_launch?verifier=1").unwrap();
        assert_eq!(f.action, "https://canvas.x.ac.kr/learningx/lti/courseresource");
        assert_eq!(f.method, "POST");
        assert_eq!(f.fields, vec![("oauth_consumer_key".into(), "key".into()), ("context_title".into(), "자료구조 & 알고리즘".into()), ("custom_x".into(), "a\"b".into())]);
    }

    #[test]
    fn cookie_jar_scopes_domains() {
        let mut j = Jar::default();
        j.store("canvas.x.ac.kr", "xn_api_token=abc.def; Path=/; Secure");
        j.store("canvas.x.ac.kr", "shared=1; Domain=.x.ac.kr; Path=/");
        j.store("canvas.x.ac.kr", "evil=1; Domain=other.com");
        assert_eq!(j.get("xn_api_token").as_deref(), Some("abc.def"));
        assert_eq!(j.header_for("canvas.x.ac.kr").unwrap(), "xn_api_token=abc.def; shared=1; evil=1");
        assert_eq!(j.header_for("cmaker.x.ac.kr").unwrap(), "shared=1");
        assert_eq!(j.header_for("other.com"), None);
        j.store("canvas.x.ac.kr", "xn_api_token=; Max-Age=0");
        assert_eq!(j.get("xn_api_token"), None);
    }

    #[test]
    fn finds_token_in_script() {
        let html = "<script>window.xn_api_token = 'eyJhbGciOiJIUzI1NiJ9.payload.sig';</script>";
        assert_eq!(find_api_token(html).as_deref(), Some("eyJhbGciOiJIUzI1NiJ9.payload.sig"));
        assert_eq!(find_api_token("xn_api_token=short"), None);
    }

    #[test]
    fn simplifies_resource_tree() {
        let raw = json!([{"title": "1주차", "children": [
            {"title": "강의 슬라이드", "content_id": "abc123", "commons_content": {"content_type": "pdf", "file_name": "week1.pdf", "view_url": "https://cmaker.x.ac.kr/em/abc123"}},
            {"title": "실습 코드", "commons_content": {"content_id": "def456", "content_type": "zip"}}
        ]}]);
        let mut origin = None;
        let items = simplify(&raw, &mut origin, "canvas.x.ac.kr");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["section"], "1주차");
        assert_eq!(items[0]["file_name"], "week1.pdf");
        assert_eq!(items[1]["content_id"], "def456");
        assert_eq!(origin.as_deref(), Some("https://cmaker.x.ac.kr"));
    }

    #[test]
    fn reads_download_file_names() {
        assert_eq!(filename_from_disposition("attachment; filename*=UTF-8''%EA%B0%95%EC%9D%98.pdf").as_deref(), Some("강의.pdf"));
        assert_eq!(filename_from_disposition("attachment; filename=\"lab 1.zip\"").as_deref(), Some("lab 1.zip"));
        assert_eq!(filename_from_disposition("attachment; filename=\"..\\..\\x.exe\"").as_deref(), Some("_.._x.exe"));
    }
}

#[cfg(test)]
mod flow_tests {
    use super::*;

    #[test]
    fn launches_learningx_without_leaking_the_canvas_token() {
        let resources = r#"[{"title":"1주차","items":[{"title":"강의 슬라이드","content_id":"c1","commons_content":{"content_type":"pdf"}}]}]"#;
        let (base, rx) = crate::canvas::tests::mock_server_with(|base| vec![
            (200, vec![], r#"[{"id":"home","label":"홈"},{"id":"context_external_tool_7","label":"강의자료실","type":"external"}]"#.to_string()),
            (200, vec![], format!(r#"{{"url":"{base}/courses/1/external_tools/sessionless_launch?verifier=v1"}}"#)),
            (200, vec![], format!(r#"<form action="{base}/learningx/lti/courseresource" method="POST" id="tool_form"><input type="hidden" name="oauth_signature" value="sig"></form>"#)),
            (302, vec![("Location", "/learningx/app".to_string()), ("Set-Cookie", "xn_api_token=TOKEN_abcdefghijklmnop; Path=/; HttpOnly".to_string())], String::new()),
            (200, vec![], "<html>ok</html>".to_string()),
            (200, vec![], resources.to_string()),
            (200, vec![], resources.to_string()),
        ]);
        let client = Client::for_test(&base, "secret-canvas-token-123");
        let out = client.learningx_resources(1).unwrap();
        assert_eq!(out["items"][0]["title"], "강의 슬라이드");
        assert_eq!(out["items"][0]["section"], "1주차");
        let reqs: Vec<String> = rx.try_iter().map(|r| r.to_lowercase()).collect();
        assert_eq!(reqs.len(), 7);
        assert!(reqs[0].contains("/courses/1/tabs") && reqs[0].contains("secret-canvas-token-123"));
        assert!(reqs[1].contains("sessionless_launch?id=7&launch_type=course_navigation"));
        for r in &reqs[2..] {
            assert!(!r.contains("secret-canvas-token-123"), "Canvas token leaked: {r}");
        }
        assert!(reqs[3].starts_with("post /learningx/lti/courseresource"));
        assert!(reqs[4].contains("cookie: xn_api_token=token_abcdefghijklmnop"));
        assert!(reqs[5].starts_with("get /learningx/api/v1/courses/1/resources") && reqs[5].contains("bearer token_abcdefghijklmnop"));
    }
}
