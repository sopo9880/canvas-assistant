//! Files the user attaches to a chat message (📎, drag and drop, paste).
//!
//! Each attachment is copied into `<app data>/attachments/<id>/<name>`, so the AI only ever
//! reads copies the user handed over, never arbitrary paths on the PC. Images go to the model
//! directly as image input; other files are read through the `read_attachment` tool.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const MAX_BYTES: u64 = 100 * 1024 * 1024;
const KEEP_DAYS: u64 = 30;
/// Image types the model accepts as image input.
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];

pub fn root(data_dir: &Path) -> PathBuf {
    data_dir.join("attachments")
}

pub fn is_image(name: &str) -> bool {
    IMAGE_EXTS.contains(&crate::extract::extension(name).as_str())
}

fn valid_id(id: &str) -> bool {
    id.len() == 20 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

fn new_id() -> String {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed) ^ std::process::id().rotate_left(16);
    format!("{:012x}{:08x}", nanos & 0xffff_ffff_ffff, n)
}

fn describe(id: &str, path: &Path) -> Value {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    json!({"id": id, "name": name, "size": size, "image": is_image(&name), "path": path.to_string_lossy()})
}

/// Copies bytes in as a new attachment.
pub fn store(data_dir: &Path, name: &str, bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(format!("'{name}'은(는) 100MB보다 커서 첨부할 수 없습니다"));
    }
    let clean = crate::learningx::sanitize(name);
    let clean = if clean.contains('.') { clean } else { format!("{clean}.bin") };
    let mut id = new_id();
    while root(data_dir).join(&id).exists() {
        id = new_id();
    }
    let dir = root(data_dir).join(&id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("첨부 폴더를 만들 수 없습니다: {e}"))?;
    let path = dir.join(&clean);
    std::fs::write(&path, bytes).map_err(|e| format!("첨부 파일을 저장할 수 없습니다: {e}"))?;
    Ok(describe(&id, &path))
}

/// Copies a file the user picked or dropped.
pub fn store_path(data_dir: &Path, src: &Path) -> Result<Value, String> {
    let meta = std::fs::metadata(src).map_err(|_| format!("파일을 찾을 수 없습니다: {}", src.display()))?;
    if !meta.is_file() {
        return Err(format!("폴더는 첨부할 수 없습니다: {}", src.display()));
    }
    if meta.len() > MAX_BYTES {
        return Err(format!("'{}'은(는) 100MB보다 커서 첨부할 수 없습니다", src.display()));
    }
    let name = src.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let bytes = std::fs::read(src).map_err(|e| format!("파일을 읽을 수 없습니다: {e}"))?;
    store(data_dir, name, &bytes)
}

/// Path of a stored attachment by id.
pub fn path_for(data_dir: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_id(id) {
        return Err("첨부 ID가 올바르지 않습니다".into());
    }
    let dir = root(data_dir).join(id);
    let file = std::fs::read_dir(&dir).ok()
        .and_then(|mut entries| entries.find_map(|e| e.ok().map(|e| e.path()).filter(|p| p.is_file())))
        .ok_or("첨부 파일이 없습니다 (30일이 지나 정리되었을 수 있음)")?;
    Ok(file)
}

pub fn info(data_dir: &Path, id: &str) -> Result<Value, String> {
    Ok(describe(id, &path_for(data_dir, id)?))
}

/// read_attachment tool: text of an attached document, paged.
pub fn read(data_dir: &Path, args: &Value) -> Result<Value, String> {
    let id = args.get("id").and_then(Value::as_str).unwrap_or("").trim();
    let path = path_for(data_dir, id)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    if is_image(&name) {
        return Ok(json!({"name": name, "note": "이미지는 사용자의 메시지에 이미지로 함께 들어 있습니다. 메시지의 이미지를 보고 답하세요."}));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("첨부 파일을 읽을 수 없습니다: {e}"))?;
    let text = crate::extract::extract_text(&name, &bytes)?;
    let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let mut out = crate::materials::paged(&name, &text, offset, crate::extract::PAGE_CHARS);
    out["id"] = json!(id);
    Ok(out)
}

/// Deletes attachments older than 30 days.
pub fn cleanup(data_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(root(data_dir)) else { return };
    let limit = std::time::Duration::from_secs(KEEP_DAYS * 24 * 3600);
    for e in entries.flatten() {
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > limit);
        if old && e.file_name().to_str().is_some_and(valid_id) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("ca-att-{}-{}", std::process::id(), new_id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn stores_and_reads_attachments() {
        let d = dir();
        let a = store(&d, "../../과제 코드.py", b"print('hi')\n").unwrap();
        assert_eq!(a["name"], "_.._과제 코드.py");
        let id = a["id"].as_str().unwrap();
        let r = read(&d, &json!({"id": id})).unwrap();
        assert_eq!(r["text"], "print('hi')");
        let img = store(&d, "shot.PNG", b"\x89PNG").unwrap();
        assert_eq!(img["image"], true);
        assert!(read(&d, &json!({"id": img["id"]})).unwrap()["note"].as_str().unwrap().contains("이미지"));
        assert!(path_for(&d, "../etc").is_err());
        assert!(path_for(&d, "00000000000000000000").is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn ids_are_unique() {
        let a: std::collections::HashSet<String> = (0..200).map(|_| new_id()).collect();
        assert_eq!(a.len(), 200);
        assert!(a.iter().all(|i| valid_id(i)));
    }
}
