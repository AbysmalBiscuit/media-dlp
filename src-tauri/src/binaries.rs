use crate::args::BinaryPaths;
use crate::settings::{CookieBrowser, UpdateChannel};
use serde::Serialize;
use tauri::Manager;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Linux,
    MacOs,
}

impl Platform {
    fn current() -> Self {
        if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSupport {
    pub browser: CookieBrowser,
    pub supported: bool,
    pub reason: Option<String>,
}

/// yt-dlp's Windows key derivation unwraps only the DPAPI-protected key and has
/// no branch for App-Bound Encryption, so cookies written by Chrome 127 and
/// later cannot be decrypted there.
fn browser_support_for(platform: Platform) -> Vec<BrowserSupport> {
    let chrome_blocked = platform == Platform::Windows;
    vec![
        BrowserSupport {
            browser: CookieBrowser::Chrome,
            supported: !chrome_blocked,
            reason: chrome_blocked
                .then(|| "Chrome locks its cookies on Windows. Use Firefox instead.".to_string()),
        },
        BrowserSupport {
            browser: CookieBrowser::Firefox,
            supported: true,
            reason: None,
        },
    ]
}

fn exe_name(stem: &str, platform: Platform) -> String {
    match platform {
        Platform::Windows => format!("{stem}.exe"),
        _ => stem.to_string(),
    }
}

/// yt-dlp overwrites itself when it self-updates, so it is staged into the
/// writable data directory; the install directory is read-only for a normal user.
pub fn resolve(app: &tauri::AppHandle) -> Result<BinaryPaths, String> {
    let platform = Platform::current();
    let resources = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("binaries");
    let data = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("bin");
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;

    let ytdlp_name = exe_name("yt-dlp", platform);
    let staged = data.join(&ytdlp_name);
    if !staged.exists() {
        std::fs::copy(resources.join(&ytdlp_name), &staged)
            .map_err(|e| format!("could not stage yt-dlp from {}: {e}", resources.display()))?;
    }

    Ok(BinaryPaths {
        ytdlp: staged,
        ffmpeg: resources.join(exe_name("ffmpeg", platform)),
    })
}

fn run_ytdlp(bins: &BinaryPaths, args: &[&str]) -> Result<String, String> {
    let mut command = std::process::Command::new(&bins.ytdlp);
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let out = command.output().map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[tauri::command]
pub fn browser_support() -> Vec<BrowserSupport> {
    browser_support_for(Platform::current())
}

#[tauri::command]
pub fn ytdlp_version(app: tauri::AppHandle) -> Result<String, String> {
    run_ytdlp(&resolve(&app)?, &["--version"])
}

#[tauri::command]
pub fn check_for_updates(app: tauri::AppHandle, channel: UpdateChannel) -> Result<String, String> {
    let target = match channel {
        UpdateChannel::Stable => "stable",
        UpdateChannel::Nightly => "nightly",
        UpdateChannel::Master => "master",
    };
    run_ytdlp(&resolve(&app)?, &["--update-to", target])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::CookieBrowser;

    #[test]
    fn every_browser_is_reported_exactly_once() {
        let support = browser_support_for(Platform::Windows);
        assert_eq!(support.len(), 2);
        assert!(support.iter().any(|s| s.browser == CookieBrowser::Chrome));
        assert!(support.iter().any(|s| s.browser == CookieBrowser::Firefox));
    }

    #[test]
    fn chrome_is_unsupported_on_windows_with_a_reason() {
        let support = browser_support_for(Platform::Windows);
        let chrome = support
            .iter()
            .find(|s| s.browser == CookieBrowser::Chrome)
            .unwrap();
        assert!(!chrome.supported);
        assert!(chrome.reason.is_some());
    }

    #[test]
    fn chrome_is_supported_elsewhere() {
        for platform in [Platform::Linux, Platform::MacOs] {
            let support = browser_support_for(platform);
            let chrome = support
                .iter()
                .find(|s| s.browser == CookieBrowser::Chrome)
                .unwrap();
            assert!(chrome.supported);
            assert!(chrome.reason.is_none());
        }
    }

    #[test]
    fn firefox_is_supported_everywhere() {
        for platform in [Platform::Windows, Platform::Linux, Platform::MacOs] {
            let support = browser_support_for(platform);
            let firefox = support
                .iter()
                .find(|s| s.browser == CookieBrowser::Firefox)
                .unwrap();
            assert!(firefox.supported);
        }
    }

    #[test]
    fn the_executable_name_carries_an_extension_only_on_windows() {
        assert_eq!(exe_name("yt-dlp", Platform::Windows), "yt-dlp.exe");
        assert_eq!(exe_name("yt-dlp", Platform::Linux), "yt-dlp");
        assert_eq!(exe_name("ffmpeg", Platform::MacOs), "ffmpeg");
    }
}
