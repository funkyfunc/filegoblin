use serde::{Deserialize, Serialize};
use anyhow::Result;

/// Local storage format for ~/.config/filegoblin/credentials.json
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct LocalCredentials {
    pub github_token: Option<String>,
    pub twitter_access_token: Option<String>,
    pub twitter_refresh_token: Option<String>,
    pub twitter_token_expires_at: Option<u64>,
    pub google_access_token: Option<String>,
    pub google_refresh_token: Option<String>,
    pub google_token_expires_at: Option<u64>,
    pub google_cookie_1psid: Option<String>,
}

pub fn get_credentials_path() -> std::path::PathBuf {
    home::home_dir()
        .map(|h| h.join(".config/filegoblin/credentials.json"))
        .unwrap_or_else(|| std::path::PathBuf::from("credentials.json"))
}

pub fn load_credentials() -> Option<LocalCredentials> {
    let path = get_credentials_path();
    if !path.exists() {
        return None;
    }
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn save_credentials(creds: &LocalCredentials) -> Result<()> {
    let path = get_credentials_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_string_pretty(creds)?;
    std::fs::write(path, data)?;
    Ok(())
}
