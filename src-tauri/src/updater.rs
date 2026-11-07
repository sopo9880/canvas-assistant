//! In-app updates. Modeled on how HYP Viewer updates its installer:
//! look the newest stable GitHub release up, take the installer for this OS by its exact name,
//! download it with resume (Range + ETag-free .part file, retries with back-off), verify its
//! SHA-256 against the release (asset digest or SHA256SUMS.txt), and only start it when the
//! user clicks install. The installer is never run from a path the frontend supplies.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const LATEST_API: &str = "https://api.github.com/repos/sopo9880/canvas-assistant/releases/latest";
const TRUSTED_PATH: &str = "/sopo9880/canvas-assistant/releases/download/";
pub const MAX_INSTALLER_BYTES: u64 = 350 * 1024 * 1024;
const MAX_SUMS_BYTES: u64 = 128 * 1024;
const ATTEMPTS: u32 = 7;

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub url: String,
    pub assets: Vec<Asset>,
}

/// Only files attached to this repository's own releases are ever downloaded.
pub fn trusted_asset_url(raw: &str) -> bool {
    url::Url::parse(raw).is_ok_and(|u| u.scheme() == "https" && u.host_str() == Some("github.com") && u.path().starts_with(TRUSTED_PATH))
}

fn safe_name(name: &str) -> bool {
    !name.is_empty() && name.len() < 120 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

pub fn parse_release(v: &Value) -> Option<Release> {
    if v.get("draft").and_then(Value::as_bool) == Some(true) || v.get("prerelease").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let version = v.get("tag_name")?.as_str()?.trim_start_matches('v').to_string();
    if version.is_empty() {
        return None;
    }
    let assets = v.get("assets").and_then(Value::as_array).into_iter().flatten().filter_map(|a| {
        let name = a.get("name")?.as_str()?.to_string();
        let url = a.get("browser_download_url")?.as_str()?.to_string();
        let size = a.get("size")?.as_u64()?;
        if !safe_name(&name) || !trusted_asset_url(&url) {
            return None;
        }
        let sha256 = a.get("digest").and_then(Value::as_str).and_then(|d| d.strip_prefix("sha256:"))
            .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())).map(str::to_lowercase);
        Some(Asset { name, url, size, sha256 })
    }).collect();
    Some(Release {
        version,
        notes: v.get("body").and_then(Value::as_str).unwrap_or("").to_string(),
        url: v.get("html_url").and_then(Value::as_str).unwrap_or(crate::RELEASES_URL).to_string(),
        assets,
    })
}

/// File name of the installer for an OS/architecture, as the Release workflow names it.
pub fn installer_name(version: &str, os: &str, arch: &str) -> Option<String> {
    match (os, arch) {
        ("windows", _) => Some(format!("CanvasAssistant-{version}-windows-x64-setup.exe")),
        ("macos", "aarch64") => Some(format!("CanvasAssistant-{version}-macos-arm64.dmg")),
        ("macos", "x86_64") => Some(format!("CanvasAssistant-{version}-macos-x64.dmg")),
        _ => None,
    }
}

pub fn pick_installer<'a>(release: &'a Release, os: &str, arch: &str) -> Option<&'a Asset> {
    let name = installer_name(&release.version, os, arch)?;
    let mut found = release.assets.iter().filter(|a| a.name == name);
    let first = found.next()?;
    if found.next().is_some() { None } else { Some(first) }
}

/// Finds `name` in a `sha256sum` style listing.
pub fn parse_sums(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let file = parts.next()?.trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hash.to_lowercase())
    })
}

fn agent(long: bool) -> ureq::Agent {
    let mut b = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .user_agent(concat!("CanvasAssistant/", env!("CARGO_PKG_VERSION")));
    b = if long {
        b.timeout_global(Some(Duration::from_secs(45 * 60))).timeout_recv_body(Some(Duration::from_secs(45)))
    } else {
        b.timeout_global(Some(Duration::from_secs(15)))
    };
    b.build().into()
}

/// The newest stable release. `Ok(None)` when it cannot be read anonymously (private repository,
/// rate limit) or there is none; the caller then points to the release page.
pub fn latest() -> Result<Option<Release>, String> {
    let mut resp = agent(false).get(LATEST_API).header("Accept", "application/vnd.github+json").call().map_err(|e| e.to_string())?;
    if resp.status().as_u16() != 200 {
        return Ok(None);
    }
    let v: Value = resp.body_mut().read_json().map_err(|e| e.to_string())?;
    Ok(parse_release(&v))
}

fn expected_hash(release: &Release, asset: &Asset, trust: &dyn Fn(&str) -> bool) -> Result<String, String> {
    if let Some(h) = &asset.sha256 {
        return Ok(h.clone());
    }
    let sums = release.assets.iter().find(|a| a.name == "SHA256SUMS.txt")
        .filter(|a| a.size <= MAX_SUMS_BYTES && trust(&a.url))
        .ok_or("이 릴리스에는 SHA-256 검증 정보가 없어 자동 설치할 수 없습니다.")?;
    let mut resp = agent(false).get(&sums.url).call().map_err(|e| format!("체크섬 파일을 가져오지 못했습니다: {e}"))?;
    if resp.status().as_u16() != 200 {
        return Err("체크섬 파일을 가져오지 못했습니다.".into());
    }
    let text = resp.body_mut().with_config().limit(MAX_SUMS_BYTES).read_to_string().map_err(|e| e.to_string())?;
    parse_sums(&text, &asset.name).ok_or_else(|| "설치 파일의 체크섬을 찾지 못했습니다.".to_string())
}

fn hash_file(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn valid(path: &Path, size: u64, expected: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.len() == size) && hash_file(path).is_ok_and(|h| h == expected)
}

/// The verified installer from the last successful download. Install only ever runs this file.
static VERIFIED: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn verified_installer() -> Option<PathBuf> {
    VERIFIED.lock().ok().and_then(|g| g.clone()).filter(|p| p.is_file())
}

enum Step {
    Done,
    Retry(String),
    Fail(String),
}

/// Downloads and verifies `asset` into `dir`, resuming an earlier partial download.
/// `progress(received, total)` is called at most a few times per second.
pub fn download(
    release: &Release,
    asset: &Asset,
    dir: &Path,
    trust: &dyn Fn(&str) -> bool,
    retry_base: Duration,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, String> {
    if asset.size < 1 || asset.size > MAX_INSTALLER_BYTES || !trust(&asset.url) || !safe_name(&asset.name) {
        return Err("허용되지 않은 릴리스 설치 파일입니다.".into());
    }
    let expected = expected_hash(release, asset, trust)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("업데이트 폴더를 만들 수 없습니다: {e}"))?;
    let target = dir.join(&asset.name);
    let part = dir.join(format!("{}.part", asset.name));
    let meta = dir.join(format!("{}.meta", asset.name));
    let identity = format!("{}|{}|{}|{expected}", release.version, asset.name, asset.size);

    if target.exists() {
        if valid(&target, asset.size, &expected) {
            progress(asset.size, asset.size);
            *VERIFIED.lock().map_err(|_| "busy")? = Some(target.clone());
            return Ok(target);
        }
        let _ = std::fs::remove_file(&target);
    }
    // A partial file only counts when it belongs to this exact release asset.
    if std::fs::read_to_string(&meta).ok().as_deref() != Some(identity.as_str()) {
        let _ = std::fs::remove_file(&part);
    }
    std::fs::write(&meta, &identity).map_err(|e| e.to_string())?;

    let client = agent(true);
    let mut last_error = String::new();
    for attempt in 0..ATTEMPTS {
        let received = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if received > asset.size {
            let _ = std::fs::remove_file(&part);
            continue;
        }
        let step = if received == asset.size { Step::Done } else { fetch(&client, asset, &part, received, progress) };
        match step {
            Step::Done => {
                if !valid(&part, asset.size, &expected) {
                    let _ = std::fs::remove_file(&part);
                    return Err("SHA-256 검증에 실패해 받은 파일을 지웠습니다. 다시 시도하세요.".into());
                }
                let _ = std::fs::remove_file(&target);
                std::fs::rename(&part, &target).map_err(|e| format!("설치 파일을 저장할 수 없습니다: {e}"))?;
                let _ = std::fs::remove_file(&meta);
                progress(asset.size, asset.size);
                *VERIFIED.lock().map_err(|_| "busy")? = Some(target.clone());
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

fn fetch(client: &ureq::Agent, asset: &Asset, part: &Path, received: u64, progress: &mut dyn FnMut(u64, u64)) -> Step {
    let mut req = client.get(&asset.url);
    if received > 0 {
        req = req.header("Range", format!("bytes={received}-"));
    }
    let mut resp = match req.call() {
        Ok(r) => r,
        Err(e) => return Step::Retry(e.to_string()),
    };
    let status = resp.status().as_u16();
    let mut start = received;
    match status {
        416 => {
            let _ = std::fs::remove_file(part);
            return Step::Retry("이어받기 범위가 맞지 않아 처음부터 다시 받습니다".into());
        }
        200 => {
            // The server ignored Range: start over.
            start = 0;
        }
        206 => {
            let range = resp.headers().get("content-range").and_then(|h| h.to_str().ok()).unwrap_or("").to_string();
            let ok = range.strip_prefix("bytes ").and_then(|r| r.split_once('/')).is_some_and(|(span, total)| {
                total.parse::<u64>().ok() == Some(asset.size) && span.split_once('-').and_then(|(a, _)| a.parse::<u64>().ok()) == Some(received)
            });
            if !ok {
                let _ = std::fs::remove_file(part);
                return Step::Retry("잘못된 이어받기 범위입니다".into());
            }
        }
        s if (400..500).contains(&s) && s != 408 && s != 429 => return Step::Fail(format!("업데이트 파일 요청이 거절되었습니다 (HTTP {s})")),
        s => return Step::Retry(format!("HTTP {s}")),
    }
    let file = if start == 0 { std::fs::File::create(part) } else { std::fs::OpenOptions::new().append(true).open(part) };
    let mut file = match file {
        Ok(f) => f,
        Err(e) => return Step::Fail(format!("설치 파일을 저장할 수 없습니다: {e}")),
    };
    let mut done = start;
    let mut buf = vec![0u8; 64 * 1024];
    let mut reader = resp.body_mut().as_reader();
    let mut last_emit = Instant::now();
    progress(done, asset.size);
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                done += n as u64;
                if done > asset.size {
                    let _ = std::fs::remove_file(part);
                    return Step::Fail("설치 파일이 예상보다 큽니다.".into());
                }
                if let Err(e) = file.write_all(&buf[..n]) {
                    return Step::Fail(format!("설치 파일을 저장할 수 없습니다: {e}"));
                }
                if last_emit.elapsed() >= Duration::from_millis(250) {
                    last_emit = Instant::now();
                    progress(done, asset.size);
                }
            }
            Err(e) => {
                let _ = file.flush();
                return Step::Retry(e.to_string());
            }
        }
    }
    let _ = file.flush();
    if done == asset.size { Step::Done } else { Step::Retry("다운로드가 중단되었습니다".into()) }
}

/// Starts the verified installer. Windows runs the NSIS setup in passive mode (progress only,
/// closes the running app, installs over it, relaunches); macOS opens the disk image.
pub fn launch(path: &Path) -> Result<Value, String> {
    let verified = verified_installer().ok_or("먼저 업데이트를 내려받으세요.")?;
    if verified != path {
        return Err("검증된 설치 파일이 아닙니다.".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP so the installer outlives this app.
        std::process::Command::new(path).args(["/P", "/UPDATE", "/R"]).creation_flags(0x0000_0008 | 0x0000_0200)
            .spawn().map_err(|e| format!("설치 프로그램을 시작할 수 없습니다: {e}"))?;
        return Ok(json!({"restart": true}));
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn().map_err(|e| format!("설치 파일을 열 수 없습니다: {e}"))?;
        return Ok(json!({"restart": false, "note": "열린 창에서 Canvas Assistant를 Applications로 끌어다 놓으세요."}));
    }
    #[allow(unreachable_code)]
    Err("이 운영체제에서는 자동 설치를 지원하지 않습니다.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ca-upd-{tag}-{}-{:x}", std::process::id(), Instant::now().elapsed().as_nanos() ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sha(data: &[u8]) -> String {
        Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Serves `data`; the first `cut_first` connections send only half the body and hang up.
    fn server(data: Vec<u8>, cut_first: u32, ranges: Arc<Mutex<Vec<String>>>, hits: Arc<AtomicU32>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut reader = BufReader::new(stream.try_clone().unwrap());
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
                let n = hits.fetch_add(1, Ordering::SeqCst);
                ranges.lock().unwrap().push(range.clone());
                let total = data.len();
                let from = range.strip_prefix("bytes=").and_then(|r| r.trim_end_matches('-').parse::<usize>().ok()).unwrap_or(0);
                let body = &data[from..];
                let head = if from > 0 {
                    format!("HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {from}-{}/{total}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", total - 1, body.len())
                } else {
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len())
                };
                let _ = stream.write_all(head.as_bytes());
                if n < cut_first {
                    let _ = stream.write_all(&body[..body.len() / 2]);
                } else {
                    let _ = stream.write_all(body);
                }
            }
        });
        format!("http://127.0.0.1:{port}/installer.exe")
    }

    fn release_for(url: &str, data: &[u8], with_digest: bool) -> (Release, Asset) {
        let asset = Asset { name: "CanvasAssistant-9.9.9-windows-x64-setup.exe".into(), url: url.into(), size: data.len() as u64, sha256: with_digest.then(|| sha(data)) };
        (Release { version: "9.9.9".into(), notes: String::new(), url: String::new(), assets: vec![asset.clone()] }, asset)
    }

    #[test]
    fn picks_the_installer_for_each_platform() {
        assert_eq!(installer_name("0.3.2", "windows", "x86_64").unwrap(), "CanvasAssistant-0.3.2-windows-x64-setup.exe");
        assert_eq!(installer_name("0.3.2", "macos", "aarch64").unwrap(), "CanvasAssistant-0.3.2-macos-arm64.dmg");
        assert_eq!(installer_name("0.3.2", "macos", "x86_64").unwrap(), "CanvasAssistant-0.3.2-macos-x64.dmg");
        assert!(installer_name("0.3.2", "linux", "x86_64").is_none());
    }

    #[test]
    fn parses_only_trusted_stable_releases() {
        let good = "https://github.com/sopo9880/canvas-assistant/releases/download/v0.3.2/CanvasAssistant-0.3.2-windows-x64-setup.exe";
        let v = json!({"tag_name": "v0.3.2", "body": "notes", "html_url": "https://github.com/sopo9880/canvas-assistant/releases/tag/v0.3.2", "assets": [
            {"name": "CanvasAssistant-0.3.2-windows-x64-setup.exe", "browser_download_url": good, "size": 100, "digest": format!("sha256:{}", "a".repeat(64))},
            {"name": "evil.exe", "browser_download_url": "https://evil.example/sopo9880/canvas-assistant/releases/download/x/evil.exe", "size": 1},
            {"name": "../bad.exe", "browser_download_url": good, "size": 1},
        ]});
        let r = parse_release(&v).unwrap();
        assert_eq!(r.version, "0.3.2");
        assert_eq!(r.assets.len(), 1);
        assert_eq!(r.assets[0].sha256.as_deref(), Some("a".repeat(64).as_str()));
        assert!(pick_installer(&r, "windows", "x86_64").is_some());
        assert!(pick_installer(&r, "macos", "aarch64").is_none());
        assert!(parse_release(&json!({"tag_name": "v1", "prerelease": true})).is_none());
        assert!(parse_release(&json!({"tag_name": "v1", "draft": true})).is_none());
        assert!(!trusted_asset_url("http://github.com/sopo9880/canvas-assistant/releases/download/a"));
        assert!(!trusted_asset_url("https://github.com/other/canvas-assistant/releases/download/a"));
    }

    #[test]
    fn reads_sha256sums() {
        let text = format!("{}  CanvasAssistant-0.3.2-windows-x64-setup.exe\n{} *other.dmg\n", "a".repeat(64), "b".repeat(64));
        assert_eq!(parse_sums(&text, "other.dmg").unwrap(), "b".repeat(64));
        assert_eq!(parse_sums(&text, "CanvasAssistant-0.3.2-windows-x64-setup.exe").unwrap(), "a".repeat(64));
        assert!(parse_sums(&text, "missing.exe").is_none());
    }

    #[test]
    fn download_resumes_after_a_dropped_connection_and_verifies() {
        let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let hits = Arc::new(AtomicU32::new(0));
        let url = server(data.clone(), 1, ranges.clone(), hits.clone());
        let (release, asset) = release_for(&url, &data, true);
        let dir = tmp("resume");
        let mut last = (0, 0);
        let path = download(&release, &asset, &dir, &|_| true, Duration::from_millis(5), &mut |a, b| last = (a, b)).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), data);
        assert_eq!(last, (data.len() as u64, data.len() as u64));
        let seen = ranges.lock().unwrap().clone();
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert_eq!(seen[0], "");
        assert!(seen[1].starts_with("bytes=") && seen[1] != "bytes=0-", "{seen:?}");
        assert_eq!(verified_installer().unwrap(), path);
        // A verified file is reused without asking the server again.
        let before = hits.load(Ordering::SeqCst);
        download(&release, &asset, &dir, &|_| true, Duration::from_millis(5), &mut |_, _| {}).unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), before);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn download_rejects_a_corrupt_file_and_untrusted_urls() {
        let data: Vec<u8> = vec![7; 50_000];
        let url = server(data.clone(), 0, Arc::new(Mutex::new(Vec::new())), Arc::new(AtomicU32::new(0)));
        let (release, mut asset) = release_for(&url, &data, false);
        asset.sha256 = Some("0".repeat(64));
        let dir = tmp("corrupt");
        let err = download(&release, &asset, &dir, &|_| true, Duration::from_millis(5), &mut |_, _| {}).unwrap_err();
        assert!(err.contains("SHA-256"), "{err}");
        assert!(!dir.join(&asset.name).exists() && !dir.join(format!("{}.part", asset.name)).exists());
        assert!(download(&release, &asset, &dir, &trusted_asset_url, Duration::from_millis(5), &mut |_, _| {}).unwrap_err().contains("허용되지"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn install_only_runs_the_verified_file() {
        assert!(launch(Path::new("/tmp/not-downloaded.exe")).is_err());
    }
}
