use crate::args::BinaryPaths;
use crate::settings::{CookieBrowser, UpdateChannel};
use serde::Serialize;
use std::path::Path;
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

/// Copies to a temp file beside `dest` and renames it into place. Rename is
/// atomic on the same volume, so a crash mid-copy never leaves a truncated
/// file at `dest` for a later launch to mistake for a complete staged binary.
fn stage_atomically(src: &Path, dest: &Path) -> Result<(), String> {
    let tmp = dest.with_extension("part");
    std::fs::copy(src, &tmp).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dest).map_err(|e| e.to_string())
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
        stage_atomically(&resources.join(&ytdlp_name), &staged)
            .map_err(|e| format!("could not stage yt-dlp from {}: {e}", resources.display()))?;
    }

    let ffmpeg = resources.join(exe_name("ffmpeg", platform));
    if !ffmpeg.exists() {
        return Err(format!("ffmpeg not found at {}", ffmpeg.display()));
    }

    Ok(BinaryPaths {
        ytdlp: staged,
        ffmpeg,
    })
}

/// Suppresses the console window that would otherwise flash on screen when
/// this windowed application spawns a console subprocess.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn run_ytdlp(bins: &BinaryPaths, args: &[&str]) -> Result<String, String> {
    let mut command = std::process::Command::new(&bins.ytdlp);
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let out = command.output().map_err(|e| e.to_string())?;
    ytdlp_result(out)
}

fn ytdlp_result(out: std::process::Output) -> Result<String, String> {
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
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

    #[test]
    fn stage_atomically_copies_the_source_content_to_dest() {
        let dir = std::env::temp_dir().join(format!("media-dlp-stage-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("source.bin");
        std::fs::write(&src, b"binary content").unwrap();
        let dest = dir.join("staged.bin");

        stage_atomically(&src, &dest).unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"binary content");
        assert!(!dest.with_extension("part").exists());
    }

    fn fake_output(success: bool, stdout: &str, stderr: &str) -> std::process::Output {
        #[cfg(windows)]
        let status = {
            use std::os::windows::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(u32::from(!success))
        };
        #[cfg(unix)]
        let status = {
            use std::os::unix::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(if success { 0 } else { 256 })
        };
        std::process::Output {
            status,
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[test]
    fn a_successful_run_returns_trimmed_stdout() {
        let out = fake_output(true, "2026.08.19\n", "");
        assert_eq!(ytdlp_result(out).unwrap(), "2026.08.19");
    }

    #[test]
    fn a_nonzero_exit_is_reported_as_an_error_with_stderr() {
        let out = fake_output(false, "", "network error\n");
        assert_eq!(ytdlp_result(out).unwrap_err(), "network error");
    }
}
