use crate::parsers::gobble::Gobble;
use anyhow::{Context, Result, anyhow};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, SystemTime};

/// Extracts Google Docs content using the "Download Hijack" strategy:
/// 1. Try public/unauthenticated fetch (covers "anyone with the link" docs)
/// 2. If private, trigger an OS-level open of the export link.
/// 3. The user's default browser (already authenticated) downloads the file.
/// 4. filegoblin watches `~/Downloads`, reads the file, and deletes it.
pub struct GoogleDocsGobbler {
    #[allow(dead_code)]
    url: String,
    doc_id: String,
}

impl GoogleDocsGobbler {
    /// Parses the document ID from a Google Docs URL.
    pub fn new(url: &str) -> Result<Self> {
        let parts: Vec<&str> = url.split("/d/").collect();
        if parts.len() < 2 {
            return Err(anyhow!("Invalid Google Docs URL format"));
        }
        let doc_id = parts[1].split('/').next().unwrap_or("").to_string();
        if doc_id.is_empty() {
            return Err(anyhow!("Could not extract Document ID from URL"));
        }

        Ok(Self {
            url: url.to_string(),
            doc_id,
        })
    }

    /// Attempts a public (unauthenticated) fetch of the Google Docs export endpoint.
    fn try_public_fetch(doc_id: &str) -> Result<String> {
        let export_url = format!(
            "https://docs.google.com/document/d/{}/export?format=txt",
            doc_id
        );

        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()?;

        let res = client.get(&export_url).send()?;

        if !res.status().is_success() {
            return Err(anyhow!("Public fetch returned HTTP {}", res.status()));
        }

        let text = res.text()?;

        // Google returns a 200 but serves a login page for private docs
        if text.contains("<title>Sign in - Google Accounts</title>")
            || text.contains("<form action=\"https://accounts.google.com")
            || text.contains("accounts.google.com/ServiceLogin")
            || text.contains("<title>Google Drive - Access Denied</title>")
            || text.len() < 50
        {
            return Err(anyhow!("Document is private, requires authentication."));
        }

        Ok(text)
    }

    /// Triggers the user's default browser to download the file and watches for it.
    fn hijack_browser_download(doc_id: &str) -> Result<String> {
        let export_url = format!(
            "https://docs.google.com/document/d/{}/export?format=txt",
            doc_id
        );

        let mut downloads_path = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map(PathBuf::from)
            .context("Could not find home directory to locate Downloads folder")?;
        downloads_path.push("Downloads");

        if !downloads_path.exists() {
            return Err(anyhow!(
                "Downloads folder not found at {:?}",
                downloads_path
            ));
        }

        eprintln!("   → Triggering OS browser download...");

        let start_time = SystemTime::now();

        #[cfg(target_os = "macos")]
        let mut cmd = Command::new("open");

        #[cfg(target_os = "windows")]
        let mut cmd = {
            let mut c = Command::new("cmd");
            c.arg("/C").arg("start");
            c
        };

        #[cfg(target_os = "linux")]
        let mut cmd = Command::new("xdg-open");

        let status = cmd
            .arg(&export_url)
            .status()
            .context("Failed to execute browser launch command")?;

        if !status.success() {
            return Err(anyhow!("Failed to open browser URL"));
        }

        eprintln!("   → Watching ~/Downloads for incoming file...");

        // Poll the Downloads directory for up to 15 seconds
        let mut attempts = 0;
        while attempts < 15 {
            sleep(Duration::from_secs(1));
            attempts += 1;

            let entries = match fs::read_dir(&downloads_path) {
                Ok(e) => e,
                Err(_) => continue,
            };

            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "txt")
                    && let Ok(metadata) = entry.metadata()
                {
                    // Check if the file was created OR modified after we ran the command
                    let is_new = metadata
                        .created()
                        .ok()
                        .or_else(|| metadata.modified().ok())
                        .is_some_and(|time| time >= start_time);

                    if is_new {
                        // Google Docs downloads sometimes have a .crdownload or .part extension
                        // momentarily, but we only check for .txt, so it should be finished.
                        // Let's just wait an extra 500ms to ensure the write is flushed.
                        sleep(Duration::from_millis(500));

                        let content = fs::read_to_string(&path)
                            .context("Failed to read the downloaded .txt file")?;

                        // Cleanup the file so we don't litter the user's Downloads folder
                        let _ = fs::remove_file(&path);

                        eprintln!("   ✅ Intercepted download successfully!");
                        return Ok(content);
                    }
                }
            }
        }

        Err(anyhow!(
            "❌ Timed out waiting for browser download in ~/Downloads.\n\
             Check if your browser blocked the download or if it saved it somewhere else."
        ))
    }
}

impl Gobble for GoogleDocsGobbler {
    fn gobble(&self, _path: &std::path::Path, _flags: &crate::cli::Cli) -> Result<String> {
        anyhow::bail!("GoogleDocsGobbler requires a URL, not a file path.")
    }

    fn gobble_str(&self, _content: &str, _flags: &crate::cli::Cli) -> Result<String> {
        eprintln!("📄 Extracting Google Doc: {}", self.doc_id);

        // Step 1: Try public/unauthenticated fetch first.
        eprintln!("   → Trying public access...");
        match Self::try_public_fetch(&self.doc_id) {
            Ok(text) => {
                eprintln!("   ✅ Document is publicly accessible!");
                return Ok(text);
            }
            Err(_) => {
                eprintln!("   🔒 Document is private.");
            }
        }

        // Step 2: Use the Download Hijack trick
        Self::hijack_browser_download(&self.doc_id)
    }
}
