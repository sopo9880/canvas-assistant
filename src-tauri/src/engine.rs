//! Codex engine management. Canvas Assistant no longer ships Codex inside its installer: the
//! official `@openai/codex` platform package is downloaded on first run, verified against the
//! npm registry's sha512 integrity, unpacked into its own folder and only switched on after a
//! compatibility probe (`app-server` answers `model/list`). The previous version is kept so the
//! user can roll back, and updates happen only when the user presses the button.
//!
//! Layout under `<app data>/engine/`:
//!   state.json            {"current": "0.170.0", "previous": "0.165.0"}
//!   <version>/codex[.exe], codex-code-mode-host[.exe], codex-path/rg[.exe]
//!   download/             partial and verified package files (deleted after install)
use crate::updater::{self, Asset, Step};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha512};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const REGISTRY: &str = "https://registry.npmjs.org/@openai/codex";
/// Version installed on first run: the one this app release was tested against.
pub const PINNED: &str = "0.170.0";
const TRUSTED_TARBALL_PREFIX: &str = "/@openai/codex/-/";
const MAX_PACKAGE_BYTES: u64 = 600 * 1024 * 1024;
const ATTEMPTS: u32 = 7;

pub fn platform() -> Option<&'static str> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some("win32-x64")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("darwin-arm64")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("darwin-x64")
    } else {
        None
    }
}

fn triple(platform: &str) -> Option<&'static str> {
    match platform {
        "win32-x64" => Some("x86_64-pc-windows-msvc"),
        "darwin-arm64" => Some("aarch64-apple-darwin"),
        "darwin-x64" => Some("x86_64-apple-darwin"),
        "linux-x64" => Some("x86_64-unknown-linux-musl"),
        _ => None,
    }
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// Stable versions only: three numeric parts, no pre-release suffix.
pub fn valid_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.len() < 6 && p.bytes().all(|b| b.is_ascii_digit()))
}

pub fn engine_root(data_dir: &Path) -> PathBuf {
    data_dir.join("engine")
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub current: Option<String>,
    pub previous: Option<String>,
}

pub fn load_state(root: &Path) -> State {
    std::fs::read_to_string(root.join("state.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_state(root: &Path, state: &State) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let tmp = root.join("state.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, root.join("state.json")).map_err(|e| format!("엔진 상태를 저장할 수 없습니다: {e}"))
}

fn has_files(dir: &Path) -> bool {
    dir.join(exe("codex")).is_file() && dir.join(exe("codex-code-mode-host")).is_file()
}

/// Folder of an installed, intact version.
pub fn version_dir(root: &Path, version: &str) -> Option<PathBuf> {
    if !valid_version(version) {
        return None;
    }
    Some(root.join(version)).filter(|d| has_files(d))
}

/// The Codex executable of the active engine, if one is installed and intact.
pub fn current_path(root: &Path) -> Option<PathBuf> {
    let v = load_state(root).current?;
    version_dir(root, &v).map(|d| d.join(exe("codex")))
}

pub fn status(root: &Path) -> Value {
    let s = load_state(root);
    let ok = |v: &Option<String>| v.as_deref().filter(|v| version_dir(root, v).is_some()).map(str::to_string);
    json!({
        "platform": platform(),
        "supported": platform().is_some(),
        "installed": ok(&s.current),
        "previous": ok(&s.previous),
        "pinned": PINNED,
    })
}

// ---------------- registry ----------------

#[derive(Clone, Debug, PartialEq)]
pub struct Dist {
    pub version: String,
    pub tarball: String,
    pub integrity: String,
}

/// The tarball must come from the npm registry's own package path.
pub fn trusted_tarball(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|u| u.scheme() == "https" && u.host_str() == Some("registry.npmjs.org") && u.path().starts_with(TRUSTED_TARBALL_PREFIX) && u.path().ends_with(".tgz"))
}

fn get_json(url: &str) -> Result<Value, String> {
    let mut resp = updater::agent(false).get(url).header("Accept", "application/json").call().map_err(|e| format!("npm 레지스트리에 연결할 수 없습니다: {e}"))?;
    if resp.status().as_u16() != 200 {
        return Err(format!("npm 레지스트리가 오류를 돌려주었습니다 (HTTP {})", resp.status().as_u16()));
    }
    resp.body_mut().with_config().limit(2 * 1024 * 1024).read_json().map_err(|e| e.to_string())
}

/// Newest stable version (npm's `latest` dist-tag).
pub fn latest_version(base: &str) -> Result<String, String> {
    let v = get_json(&format!("{base}/latest"))?;
    let version = v.get("version").and_then(Value::as_str).ok_or("최신 버전 정보를 읽을 수 없습니다")?;
    if !valid_version(version) {
        return Err(format!("안정 버전이 아닙니다: {version}"));
    }
    Ok(version.to_string())
}

/// Download details of one platform package; the registry publishes them as `<version>-<platform>`.
pub fn fetch_dist(base: &str, version: &str, platform: &str) -> Result<Dist, String> {
    if !valid_version(version) {
        return Err("허용되지 않는 Codex 버전입니다".into());
    }
    let v = get_json(&format!("{base}/{version}-{platform}"))?;
    parse_dist(&v, version, platform)
}

pub fn parse_dist(v: &Value, version: &str, platform: &str) -> Result<Dist, String> {
    if v.get("version").and_then(Value::as_str) != Some(&format!("{version}-{platform}")) {
        return Err("레지스트리 응답의 버전이 요청과 다릅니다".into());
    }
    let tarball = v.pointer("/dist/tarball").and_then(Value::as_str).ok_or("다운로드 주소가 없습니다")?;
    let integrity = v.pointer("/dist/integrity").and_then(Value::as_str).filter(|i| i.starts_with("sha512-")).ok_or("sha512 무결성 정보가 없어 설치할 수 없습니다")?;
    Ok(Dist { version: version.to_string(), tarball: tarball.to_string(), integrity: integrity.to_string() })
}

fn content_length(url: &str) -> Result<u64, String> {
    let resp = updater::agent(false).head(url).call().map_err(|e| format!("Codex 파일 정보를 가져오지 못했습니다: {e}"))?;
    if resp.status().as_u16() != 200 {
        return Err(format!("Codex 파일 정보 요청이 거절되었습니다 (HTTP {})", resp.status().as_u16()));
    }
    resp.headers().get("content-length").and_then(|h| h.to_str().ok()).and_then(|s| s.parse().ok()).ok_or_else(|| "Codex 파일 크기를 알 수 없습니다".to_string())
}

fn sha512_integrity(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = Sha512::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode(h.finalize())))
}

// ---------------- download ----------------

/// Downloads the package into `dir` (resuming an earlier attempt) and checks its sha512.
pub fn download(dist: &Dist, dir: &Path, trust: &dyn Fn(&str) -> bool, retry_base: Duration, progress: &mut dyn FnMut(u64, u64)) -> Result<PathBuf, String> {
    if !trust(&dist.tarball) {
        return Err("허용되지 않는 다운로드 주소입니다".into());
    }
    let size = content_length(&dist.tarball)?;
    if size < 1 || size > MAX_PACKAGE_BYTES {
        return Err("Codex 파일 크기가 허용 범위를 벗어났습니다".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("다운로드 폴더를 만들 수 없습니다: {e}"))?;
    let name = format!("codex-{}.tgz", dist.version);
    let target = dir.join(&name);
    let part = dir.join(format!("{name}.part"));
    let meta = dir.join(format!("{name}.meta"));
    let identity = format!("{}|{size}|{}", dist.version, dist.integrity);
    if target.exists() {
        if sha512_integrity(&target).is_ok_and(|h| h == dist.integrity) {
            progress(size, size);
            return Ok(target);
        }
        let _ = std::fs::remove_file(&target);
    }
    if std::fs::read_to_string(&meta).ok().as_deref() != Some(identity.as_str()) {
        let _ = std::fs::remove_file(&part);
    }
    std::fs::write(&meta, &identity).map_err(|e| e.to_string())?;

    let asset = Asset { name: name.clone(), url: dist.tarball.clone(), size, sha256: None };
    let client = updater::agent(true);
    let mut last_error = String::new();
    for attempt in 0..ATTEMPTS {
        let received = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if received > size {
            let _ = std::fs::remove_file(&part);
            continue;
        }
        let step = if received == size { Step::Done } else { updater::fetch(&client, &asset, &part, received, progress) };
        match step {
            Step::Done => {
                if !sha512_integrity(&part).is_ok_and(|h| h == dist.integrity) {
                    let _ = std::fs::remove_file(&part);
                    return Err("무결성(sha512) 검증에 실패해 받은 파일을 지웠습니다. 다시 시도하세요.".into());
                }
                let _ = std::fs::remove_file(&target);
                std::fs::rename(&part, &target).map_err(|e| format!("파일을 저장할 수 없습니다: {e}"))?;
                let _ = std::fs::remove_file(&meta);
                progress(size, size);
                return Ok(target);
            }
            Step::Fail(e) => return Err(e),
            Step::Retry(e) => {
                last_error = e;
                std::thread::sleep(retry_base * (1u32 << attempt).min(12));
            }
        }
    }
    Err(format!("재시도 횟수를 초과했습니다. 다시 누르면 이어받습니다. ({last_error})"))
}

// ---------------- extract ----------------

/// Unpacks only the files Canvas Assistant needs from the platform package into `dest`:
/// codex, codex-code-mode-host and the bundled ripgrep. Nothing else is written, and no member
/// path is ever used as-is.
pub fn extract(tgz: &Path, dest: &Path, platform: &str) -> Result<(), String> {
    let triple = triple(platform).ok_or("지원하지 않는 운영체제입니다")?;
    let prefix = format!("package/vendor/{triple}/");
    let file = std::fs::File::open(tgz).map_err(|e| e.to_string())?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let wanted = [exe("codex"), exe("codex-code-mode-host")];
    for entry in archive.entries().map_err(|e| format!("패키지를 열 수 없습니다: {e}"))? {
        let mut entry = entry.map_err(|e| format!("패키지가 손상되었습니다: {e}"))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().map_err(|e| e.to_string())?.to_string_lossy().replace('\\', "/");
        let Some(rel) = path.strip_prefix(&prefix) else { continue };
        let out = if let Some(name) = rel.strip_prefix("bin/").filter(|n| wanted.iter().any(|w| w == n)) {
            dest.join(name)
        } else if let Some(name) = rel.strip_prefix("codex-path/").filter(|n| *n == exe("rg")) {
            let dir = dest.join("codex-path");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            dir.join(name)
        } else {
            continue;
        };
        entry.unpack(&out).map_err(|e| format!("파일을 풀 수 없습니다 ({}): {e}", out.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        }
    }
    if !has_files(dest) {
        return Err("패키지에 codex 실행 파일이 없습니다".into());
    }
    Ok(())
}

// ---------------- probe ----------------

/// Compatibility probe: the binary reports the expected version and its app-server completes the
/// handshake and lists models (works without logging in). The scratch Codex home keeps the probe
/// away from the user's real login and chats.
pub fn probe(codex: &Path, version: &str) -> Result<usize, String> {
    let mut v = Command::new(codex);
    v.arg("--version").stdin(Stdio::null()).stderr(Stdio::null());
    hide_window(&mut v);
    let out = v.output().map_err(|e| format!("Codex를 실행할 수 없습니다: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() || !text.contains(version) {
        return Err(format!("Codex 버전 확인 실패 (기대 {version}, 실제 {})", text.trim()));
    }
    let home = std::env::temp_dir().join(format!("canvas-assistant-probe-{}-{}", std::process::id(), chrono::Utc::now().timestamp_millis()));
    std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    let result = probe_server(codex, &home);
    let _ = std::fs::remove_dir_all(&home);
    result
}

fn probe_server(codex: &Path, home: &Path) -> Result<usize, String> {
    let mut command = Command::new(codex);
    command.args(crate::codex::app_server_args()).env("CODEX_HOME", home).current_dir(home).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    hide_window(&mut command);
    let mut child = command.spawn().map_err(|e| format!("app-server를 시작할 수 없습니다: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("stdin 없음")?;
    let stdout = child.stdout.take().ok_or("stdout 없음")?;
    let (tx, rx) = std::sync::mpsc::channel::<Value>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if tx.send(v).is_err() {
                    break;
                }
            }
        }
    });
    let send = |stdin: &mut std::process::ChildStdin, v: Value| stdin.write_all(format!("{v}\n").as_bytes()).map_err(|e| e.to_string());
    let outcome = (|| {
        send(&mut stdin, json!({"id": 1, "method": "initialize", "params": {"clientInfo": {"name": "canvas_assistant", "title": "Canvas Assistant", "version": env!("CARGO_PKG_VERSION")}}}))?;
        let deadline = Instant::now() + Duration::from_secs(45);
        let mut listed = false;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let msg = rx.recv_timeout(left).map_err(|_| "app-server가 시간 안에 응답하지 않았습니다".to_string())?;
            match msg.get("id").and_then(Value::as_i64) {
                Some(1) if !listed => {
                    if let Some(e) = msg.get("error") {
                        return Err(format!("초기화 거절: {e}"));
                    }
                    send(&mut stdin, json!({"method": "initialized", "params": {}}))?;
                    send(&mut stdin, json!({"id": 2, "method": "model/list", "params": {"limit": 100, "includeHidden": false}}))?;
                    listed = true;
                }
                Some(2) => {
                    if let Some(e) = msg.get("error") {
                        return Err(format!("model/list 거절: {e}"));
                    }
                    let n = msg.pointer("/result/data").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
                    return if n > 0 { Ok(n) } else { Err("모델 목록이 비어 있습니다".into()) };
                }
                _ => {}
            }
        }
    })();
    let _ = child.kill();
    let _ = child.wait();
    outcome
}

fn hide_window(_command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        _command.creation_flags(0x0800_0000);
    }
}

// ---------------- install / rollback ----------------

/// Installs `version`: download, verify, unpack into a temp folder, probe it, move it into place
/// and switch `state.json`. The running engine is never touched until the very end, so any
/// failure leaves the current version exactly as it was.
#[allow(clippy::too_many_arguments)]
pub fn install(
    root: &Path,
    base: &str,
    version: &str,
    platform: &str,
    trust: &dyn Fn(&str) -> bool,
    retry_base: Duration,
    check: &dyn Fn(&Path, &str) -> Result<(), String>,
    progress: &mut dyn FnMut(&str, u64, u64),
) -> Result<State, String> {
    if !valid_version(version) {
        return Err("허용되지 않는 Codex 버전입니다".into());
    }
    let mut state = load_state(root);
    if state.current.as_deref() == Some(version) && version_dir(root, version).is_some() {
        return Ok(state);
    }
    progress("resolve", 0, 0);
    let dist = fetch_dist(base, version, platform)?;
    let download_dir = root.join("download");
    let tgz = download(&dist, &download_dir, trust, retry_base, &mut |a, b| progress("download", a, b))?;
    progress("unpack", 0, 0);
    let tmp = root.join(format!("{version}.tmp"));
    let _ = std::fs::remove_dir_all(&tmp);
    if let Err(e) = extract(&tgz, &tmp, platform) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    progress("verify", 0, 0);
    if let Err(e) = check(&tmp.join(exe("codex")), version) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("호환성 검사에 실패해 설치를 취소했습니다 (기존 버전은 그대로입니다): {e}"));
    }
    let dest = root.join(version);
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::rename(&tmp, &dest).map_err(|e| format!("엔진 폴더를 만들 수 없습니다: {e}"))?;
    if state.current.as_deref() != Some(version) {
        state.previous = state.current.take().filter(|c| c != version);
    }
    state.current = Some(version.to_string());
    save_state(root, &state)?;
    let _ = std::fs::remove_file(&tgz);
    Ok(state)
}

/// Switches back to the previous version (and keeps the replaced one as the new "previous").
pub fn rollback(root: &Path) -> Result<State, String> {
    let mut state = load_state(root);
    let previous = state.previous.clone().filter(|p| version_dir(root, p).is_some()).ok_or("되돌릴 이전 버전이 없습니다")?;
    state.previous = state.current.take();
    state.current = Some(previous);
    save_state(root, &state)?;
    Ok(state)
}

/// Deletes every version folder except the current and previous one, plus stale temp files.
/// Call it after the old Codex process has been stopped (Windows cannot delete a running exe).
pub fn cleanup(root: &Path) {
    let state = load_state(root);
    let keep: Vec<&str> = [state.current.as_deref(), state.previous.as_deref()].into_iter().flatten().collect();
    for e in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        if name == "state.json" || name == "download" || keep.contains(&name.as_str()) {
            continue;
        }
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        } else if name.ends_with(".tmp") {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Dirs for `PATH` so Codex finds its bundled ripgrep.
pub fn tool_dir(codex: &Path) -> Option<PathBuf> {
    codex.parent().map(|d| d.join("codex-path")).filter(|d| d.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ca-eng-{tag}-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A platform package like npm's: everything under package/vendor/<triple>/.
    fn package(version: &str) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        let triple = "x86_64-unknown-linux-musl";
        let files: Vec<(String, Vec<u8>)> = vec![
            (format!("package/vendor/{triple}/bin/codex"), format!("codex {version}").into_bytes()),
            (format!("package/vendor/{triple}/bin/codex-code-mode-host"), b"host".to_vec()),
            (format!("package/vendor/{triple}/codex-path/rg"), b"rg".to_vec()),
            (format!("package/vendor/{triple}/codex-resources/voice/bin/big.so"), vec![0u8; 4096]),
            ("package/package.json".into(), b"{}".to_vec()),
        ];
        for (name, data) in files {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            builder.append_data(&mut h, &name, &data[..]).unwrap();
        }
        let raw = builder.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&raw).unwrap();
        gz.finish().unwrap()
    }

    fn integrity(data: &[u8]) -> String {
        format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode(Sha512::digest(data)))
    }

    /// Serves registry metadata and package files for `versions`; `corrupt` serves wrong bytes.
    fn registry(versions: &[&str], corrupt: bool, hits: Arc<Mutex<Vec<String>>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let pkgs: Vec<(String, Vec<u8>)> = versions.iter().map(|v| (v.to_string(), package(v))).collect();
        let latest = versions.last().unwrap().to_string();
        let base2 = base.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap_or(0);
                let mut range = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    if line.to_lowercase().starts_with("range:") {
                        range = line[6..].trim().to_string();
                    }
                }
                let mut it = first.split_whitespace();
                let method = it.next().unwrap_or("").to_string();
                let path = it.next().unwrap_or("").to_string();
                hits.lock().unwrap().push(format!("{method} {path}"));
                let json = |v: Value| format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{v}", v.to_string().len());
                if path == "/latest" {
                    let _ = stream.write_all(json(json!({"version": latest})).as_bytes());
                    continue;
                }
                if let Some(rest) = path.strip_prefix("/-/") {
                    let ver = rest.trim_start_matches("codex-").trim_end_matches("-linux-x64.tgz");
                    let Some((_, data)) = pkgs.iter().find(|(v, _)| v == ver) else {
                        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        continue;
                    };
                    let mut data = data.clone();
                    if corrupt {
                        let n = data.len();
                        data[n / 2] ^= 0xff;
                    }
                    let from = range.strip_prefix("bytes=").and_then(|r| r.trim_end_matches('-').parse::<usize>().ok()).unwrap_or(0);
                    let body = &data[from..];
                    let head = if from > 0 {
                        format!("HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {from}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len() - 1, data.len(), body.len())
                    } else {
                        format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len())
                    };
                    let _ = stream.write_all(head.as_bytes());
                    if method != "HEAD" {
                        let _ = stream.write_all(body);
                    }
                    continue;
                }
                if let Some(name) = path.strip_prefix('/') {
                    if let Some(ver) = name.strip_suffix("-linux-x64") {
                        if let Some((_, data)) = pkgs.iter().find(|(v, _)| v == ver) {
                            // integrity always describes the genuine package
                            let meta = json!({"version": name, "dist": {"tarball": format!("{base2}/-/codex-{ver}-linux-x64.tgz"), "integrity": integrity(data)}});
                            let _ = stream.write_all(json(meta).as_bytes());
                            continue;
                        }
                    }
                }
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            }
        });
        base
    }

    fn run_install(root: &Path, base: &str, version: &str, check: &dyn Fn(&Path, &str) -> Result<(), String>) -> Result<State, String> {
        install(root, base, version, "linux-x64", &|_| true, Duration::from_millis(1), check, &mut |_, _, _| {})
    }

    fn ok_check(p: &Path, v: &str) -> Result<(), String> {
        let text = std::fs::read_to_string(p).unwrap();
        if text.contains(v) { Ok(()) } else { Err("wrong binary".into()) }
    }

    #[test]
    fn versions_must_be_stable_three_part() {
        assert!(valid_version("0.170.0"));
        for bad in ["0.170", "0.170.0-alpha.1", "0.170.0.1", "", "a.b.c", "1..2", "../1.2.3"] {
            assert!(!valid_version(bad), "{bad}");
        }
    }

    #[test]
    fn only_registry_package_urls_are_trusted_and_dist_is_checked() {
        assert!(trusted_tarball("https://registry.npmjs.org/@openai/codex/-/codex-0.170.0-win32-x64.tgz"));
        for bad in ["http://registry.npmjs.org/@openai/codex/-/x.tgz", "https://evil.example/@openai/codex/-/x.tgz", "https://registry.npmjs.org/other/-/x.tgz", "https://registry.npmjs.org/@openai/codex/-/x.exe"] {
            assert!(!trusted_tarball(bad), "{bad}");
        }
        let good = json!({"version": "0.170.0-win32-x64", "dist": {"tarball": "https://registry.npmjs.org/@openai/codex/-/c.tgz", "integrity": "sha512-abc"}});
        assert!(parse_dist(&good, "0.170.0", "win32-x64").is_ok());
        assert!(parse_dist(&good, "0.171.0", "win32-x64").is_err());
        let weak = json!({"version": "0.170.0-win32-x64", "dist": {"tarball": "https://registry.npmjs.org/@openai/codex/-/c.tgz", "integrity": "sha1-abc"}});
        assert!(parse_dist(&weak, "0.170.0", "win32-x64").is_err());
    }

    #[test]
    fn pinned_version_matches_package_json() {
        let pkg: Value = serde_json::from_str(include_str!("../../package.json")).unwrap();
        assert_eq!(pkg["codexVersion"].as_str(), Some(PINNED));
    }

    #[test]
    fn extract_keeps_only_what_the_app_needs() {
        let dir = tmp("extract");
        let tgz = dir.join("p.tgz");
        std::fs::write(&tgz, package("1.2.3")).unwrap();
        let out = dir.join("out");
        extract(&tgz, &out, "linux-x64").unwrap();
        let mut names: Vec<String> = walk(&out);
        names.sort();
        assert_eq!(names, ["codex", "codex-code-mode-host", "codex-path/rg"]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(out.join("codex")).unwrap().permissions().mode() & 0o111, 0o111);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    fn walk(dir: &Path) -> Vec<String> {
        fn go(base: &Path, dir: &Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                if e.path().is_dir() { go(base, &e.path(), out) } else { out.push(e.path().strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/")) }
            }
        }
        let mut v = vec![];
        go(dir, dir, &mut v);
        v
    }

    #[test]
    fn installs_updates_and_rolls_back_keeping_two_versions() {
        let root = tmp("flow");
        let hits = Arc::new(Mutex::new(vec![]));
        let base = registry(&["1.0.0", "1.1.0", "1.2.0"], false, hits.clone());
        assert_eq!(latest_version(&base).unwrap(), "1.2.0");
        assert!(current_path(&root).is_none());

        let s = run_install(&root, &base, "1.0.0", &ok_check).unwrap();
        assert_eq!((s.current.as_deref(), s.previous.as_deref()), (Some("1.0.0"), None));
        assert!(current_path(&root).unwrap().ends_with("1.0.0/codex"));
        assert!(!root.join("download/codex-1.0.0.tgz").exists(), "package file is removed after install");

        let s = run_install(&root, &base, "1.1.0", &ok_check).unwrap();
        assert_eq!((s.current.as_deref(), s.previous.as_deref()), (Some("1.1.0"), Some("1.0.0")));
        let s = run_install(&root, &base, "1.2.0", &ok_check).unwrap();
        assert_eq!((s.current.as_deref(), s.previous.as_deref()), (Some("1.2.0"), Some("1.1.0")));
        cleanup(&root);
        assert!(root.join("1.2.0").is_dir() && root.join("1.1.0").is_dir());
        assert!(!root.join("1.0.0").exists(), "only current and previous are kept");

        // Installing the version that is already active does not hit the network.
        let before = hits.lock().unwrap().len();
        run_install(&root, &base, "1.2.0", &ok_check).unwrap();
        assert_eq!(hits.lock().unwrap().len(), before);

        let s = rollback(&root).unwrap();
        assert_eq!((s.current.as_deref(), s.previous.as_deref()), (Some("1.1.0"), Some("1.2.0")));
        assert!(current_path(&root).unwrap().ends_with("1.1.0/codex"));
        assert_eq!(status(&root)["installed"], "1.1.0");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_probe_or_bad_hash_leaves_the_current_version_alone() {
        let root = tmp("fail");
        let base = registry(&["1.0.0", "1.1.0"], false, Arc::new(Mutex::new(vec![])));
        run_install(&root, &base, "1.0.0", &ok_check).unwrap();

        let err = run_install(&root, &base, "1.1.0", &|_, _| Err("app-server died".into())).unwrap_err();
        assert!(err.contains("app-server died") && err.contains("기존 버전"), "{err}");
        assert_eq!(load_state(&root).current.as_deref(), Some("1.0.0"));
        assert!(!root.join("1.1.0").exists() && !root.join("1.1.0.tmp").exists());

        // A verified package file from the failed attempt is reused; start clean for the tamper case.
        assert!(root.join("download/codex-1.1.0.tgz").exists());
        std::fs::remove_dir_all(root.join("download")).unwrap();
        let bad = registry(&["1.0.0", "1.1.0"], true, Arc::new(Mutex::new(vec![])));
        let err = run_install(&root, &bad, "1.1.0", &ok_check).unwrap_err();
        assert!(err.contains("sha512"), "{err}");
        assert_eq!(load_state(&root).current.as_deref(), Some("1.0.0"));
        assert!(!root.join("download/codex-1.1.0.tgz").exists());
        assert!(rollback(&root).is_err(), "nothing to roll back to");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_broken_install_folder_is_not_used() {
        let root = tmp("broken");
        let base = registry(&["1.0.0"], false, Arc::new(Mutex::new(vec![])));
        run_install(&root, &base, "1.0.0", &ok_check).unwrap();
        std::fs::remove_file(root.join("1.0.0/codex")).unwrap();
        assert!(current_path(&root).is_none());
        assert_eq!(status(&root)["installed"], Value::Null);
        let _ = std::fs::remove_dir_all(root);
    }

    /// Run by hand against a real binary: CODEX_PROBE_BIN=/path/codex CODEX_PROBE_VERSION=0.170.0
    #[test]
    #[ignore]
    fn real_binary_passes_the_probe() {
        let bin = std::env::var("CODEX_PROBE_BIN").unwrap();
        let version = std::env::var("CODEX_PROBE_VERSION").unwrap();
        let n = probe(Path::new(&bin), &version).unwrap();
        assert!(n > 0);
        assert!(probe(Path::new(&bin), "9.9.9").is_err());
    }
}
