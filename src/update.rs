//! Update check and silent installer hand-off.
//!
//! The installers publish a small manifest next to the setup executable:
//!
//! ```json
//! { "version": "1.3.0", "url": "https://…/MSFSFlightTracker-Setup-1.3.0.exe", "notes": "…" }
//! ```
//!
//! Every client checks it at startup (and once a day while open), downloads the
//! setup in the background and runs it silently; the installer replaces the
//! files and starts the new version.

use crate::net;
use std::path::PathBuf;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Official release channel: every published release uploads latest.json,
/// and GitHub's "latest" alias keeps this address stable.
pub const DEFAULT_MANIFEST: &str =
    "https://github.com/Federicofnn/msfs-flight-tracker/releases/latest/download/latest.json";

/// What the Settings screen shows about updates.
#[derive(Default, Clone)]
pub struct State {
    pub checking: bool,
    pub downloading: bool,
    pub available: Option<Release>,
    pub installer: Option<PathBuf>,
    pub status: String,
    pub notes: String,
    pub error: String,
}

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub notes: String,
}

/// Reads the manifest and returns a release only when it is newer than us.
pub fn check(manifest: &str) -> Result<Option<Release>, String> {
    let manifest = manifest.trim();
    if manifest.is_empty() {
        return Err("No update address configured".to_string());
    }
    let json = net::get_json(manifest)?;
    let version = json["version"]
        .as_str()
        .ok_or_else(|| "Update manifest has no version".to_string())?
        .trim()
        .to_string();
    let url = json["url"]
        .as_str()
        .ok_or_else(|| "Update manifest has no download link".to_string())?
        .trim()
        .to_string();
    if !version.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(format!("Invalid version in update manifest: {version}"));
    }
    if url.is_empty() {
        return Err("Update manifest has an empty download link".to_string());
    }
    let notes = json["notes"].as_str().unwrap_or_default().trim().to_string();
    Ok(is_newer(&version, VERSION).then_some(Release {
        version,
        url,
        notes,
    }))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    let parse = |text: &str| -> Vec<u64> {
        text.trim_start_matches('v')
            .split(|c: char| !c.is_ascii_digit())
            .filter(|part| !part.is_empty())
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    };
    let candidate = parse(candidate);
    let current = parse(current);
    for index in 0..candidate.len().max(current.len()) {
        let a = candidate.get(index).copied().unwrap_or(0);
        let b = current.get(index).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    false
}

/// Downloads the setup executable and returns its path.
pub fn download(release: &Release) -> Result<PathBuf, String> {
    let bytes = net::http_get_max(&release.url, 128 * 1024 * 1024)?;
    if bytes.len() < 64 * 1024 {
        return Err("The downloaded installer looks incomplete".to_string());
    }
    let path = std::env::temp_dir().join(format!(
        "MSFSFlightTracker-Setup-{}.exe",
        release.version
    ));
    std::fs::write(&path, bytes).map_err(|e| format!("Cannot save the installer: {e}"))?;
    Ok(path)
}

/// Starts the setup. `/S` installs silently, replacing the running app.
pub fn run_installer(path: &std::path::Path) -> Result<(), String> {
    std::process::Command::new(path)
        .arg("/S")
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Cannot start the installer: {e}"))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn reads_the_published_manifest() {
        let release = check(DEFAULT_MANIFEST).expect("official manifest reachable");
        match &release {
            Some(release) => {
                assert!(release.url.starts_with("https://"));
                println!("update available: {}", release.version);
            }
            None => println!("up to date at {}", VERSION),
        }
    }

    #[test]
    fn compares_versions_by_number() {
        assert!(is_newer("1.10.0", "1.9.9"));
        assert!(is_newer("1.2.1", "1.2.0"));
        assert!(is_newer("2.0", "1.99.99"));
        assert!(!is_newer("1.2.0", "1.2.0"));
        assert!(!is_newer("1.1.9", "1.2.0"));
        assert!(!is_newer("v1.2.0", "1.2.0"));
    }
}
