use anyhow::{Context, Result};
use git2::build::RepoBuilder;
use git2::{FetchOptions, RemoteCallbacks};
use std::path::Path;

pub fn clone_github_repo(url: &str, out_path: &Path) -> Result<()> {
    let mut cb = RemoteCallbacks::new();

    let cred_path = home::home_dir()
        .map(|h| h.join(".config/filegoblin/credentials.json"))
        .unwrap_or_default();

    let mut token = None;
    if cred_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&cred_path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(gh_token) = json.get("github_token").and_then(|v| v.as_str()) {
                    token = Some(gh_token.to_string());
                }
            }
        }
    }

    cb.credentials(move |_url, username_from_url, _allowed_types| {
        if let Some(t) = &token {
            // GitHub accepts token as password, usually with any username (e.g., "git" or the token itself as username)
            // But standard is userpass_plaintext where password is the PAT
            git2::Cred::userpass_plaintext(username_from_url.unwrap_or("git"), t)
        } else {
            git2::Cred::default()
        }
    });

    let mut fo = FetchOptions::new();
    fo.remote_callbacks(cb);
    fo.depth(1);

    let mut builder = RepoBuilder::new();
    builder.fetch_options(fo);

    builder
        .clone(url, out_path)
        .context("Failed to clone GitHub repository")?;

    Ok(())
}
