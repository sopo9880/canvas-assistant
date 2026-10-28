//! Non-secret settings (JSON file in the app data folder) and the Canvas access
//! token, which lives only in the Windows Credential Manager.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const SERVICE: &str = "CanvasAssistant";
const TOKEN_USER: &str = "canvas-access-token";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub canvas_base_url: Option<String>,
    pub close_to_tray: bool,
    pub notifications: bool,
    /// Optional absolute path to codex.exe when it is not on PATH.
    pub codex_path: Option<String>,
    pub default_model: Option<String>,
    pub default_effort: Option<String>,
    pub canvas_tools_in_chat: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            canvas_base_url: None,
            close_to_tray: true,
            notifications: true,
            codex_path: None,
            default_model: None,
            default_effort: None,
            canvas_tools_in_chat: true,
        }
    }
}

fn settings_path(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

pub fn load(dir: &Path) -> Settings {
    std::fs::read_to_string(settings_path(dir)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(dir: &Path, settings: &Settings) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = dir.join("settings.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, settings_path(dir)).map_err(|e| format!("설정을 저장할 수 없습니다: {e}"))
}

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, TOKEN_USER).map_err(|e| format!("자격 증명 저장소를 사용할 수 없습니다: {e}"))
}

pub fn get_token() -> Option<String> {
    entry().ok()?.get_password().ok().filter(|t| !t.trim().is_empty())
}

pub fn set_token(token: &str) -> Result<(), String> {
    let token = token.trim();
    if token.len() < 20 || token.len() > 512 || token.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("액세스 토큰 형식이 올바르지 않습니다. Canvas 계정 설정에서 발급한 토큰 전체를 붙여넣으세요.".into());
    }
    entry()?.set_password(token).map_err(|e| format!("토큰을 저장할 수 없습니다: {e}"))
}

pub fn delete_token() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("토큰을 삭제할 수 없습니다: {e}")),
    }
}

/// Builds a Canvas client from the saved settings and token.
pub fn canvas_client(dir: &Path) -> Result<crate::canvas::Client, String> {
    let settings = load(dir);
    let base = settings.canvas_base_url.ok_or_else(|| crate::canvas::CanvasError::NotConfigured.to_string())?;
    let token = get_token().ok_or_else(|| crate::canvas::CanvasError::NotConfigured.to_string())?;
    crate::canvas::Client::new(&base, &token).map_err(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip_with_defaults() {
        let dir = std::env::temp_dir().join(format!("ca-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load(&dir).close_to_tray);
        let mut s = Settings::default();
        s.canvas_base_url = Some("https://canvas.example.ac.kr".into());
        s.close_to_tray = false;
        save(&dir, &s).unwrap();
        let back = load(&dir);
        assert_eq!(back.canvas_base_url.as_deref(), Some("https://canvas.example.ac.kr"));
        assert!(!back.close_to_tray);
        // Unknown or missing fields fall back to defaults.
        std::fs::write(dir.join("settings.json"), r#"{"closeToTray": false, "bogus": 1}"#).unwrap();
        assert!(load(&dir).notifications);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_malformed_tokens() {
        assert!(set_token("short").is_err());
        assert!(set_token("has space inside the token value 1234567890").is_err());
    }
}
