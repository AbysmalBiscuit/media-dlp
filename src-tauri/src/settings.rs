use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::Manager;

pub const FILE_NAME: &str = "config.toml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Settings {
    pub save_folder: Option<PathBuf>,
    pub video_quality: VideoQuality,
    pub video_format: VideoFormat,
    pub audio_format: AudioFormat,
    pub audio_quality: AudioQuality,
    pub audio_only: bool,
    pub cookie_browser: Option<CookieBrowser>,
    pub filename: FilenameSettings,
    pub theme: Theme,
    pub update_channel: UpdateChannel,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            save_folder: None,
            video_quality: VideoQuality::Best,
            video_format: VideoFormat::Mp4,
            audio_format: AudioFormat::Mp3,
            audio_quality: AudioQuality::Good,
            audio_only: false,
            cookie_browser: None,
            filename: FilenameSettings::default(),
            theme: Theme::System,
            update_channel: UpdateChannel::Nightly,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VideoQuality {
    Best,
    #[serde(rename = "1080p")]
    P1080,
    #[serde(rename = "720p")]
    P720,
    #[serde(rename = "480p")]
    P480,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VideoFormat {
    Mp4,
    Mkv,
    Webm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioFormat {
    Mp3,
    M4a,
    Opus,
    Wav,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioQuality {
    Best,
    Good,
    Smaller,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CookieBrowser {
    Chrome,
    Firefox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateChannel {
    Stable,
    Nightly,
    Master,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Separator {
    Dash,
    Underscore,
    Space,
}

impl Separator {
    pub fn as_str(self) -> &'static str {
        match self {
            Separator::Dash => " - ",
            Separator::Underscore => "_",
            Separator::Space => " ",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct FilenameSettings {
    pub channel: bool,
    pub upload_date: bool,
    pub playlist_number: bool,
    pub video_id: bool,
    pub separator: Separator,
}

impl Default for FilenameSettings {
    fn default() -> Self {
        Self {
            channel: false,
            upload_date: false,
            playlist_number: true,
            video_id: true,
            separator: Separator::Dash,
        }
    }
}

pub fn load(dir: &Path) -> Settings {
    std::fs::read_to_string(dir.join(FILE_NAME))
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(dir: &Path, settings: &Settings) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let text = toml::to_string_pretty(settings)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(dir.join(FILE_NAME), text)
}

fn config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_config_dir().map_err(|e| e.to_string())
}

/// Keeps a save folder the user already chose; otherwise adopts the fallback
/// (the platform Downloads directory) when one resolves.
pub fn resolve_save_folder(current: Option<PathBuf>, fallback: Option<PathBuf>) -> Option<PathBuf> {
    current.or(fallback)
}

#[tauri::command]
pub fn get_settings(app: tauri::AppHandle) -> Result<Settings, String> {
    let mut settings = load(&config_dir(&app)?);
    settings.save_folder =
        resolve_save_folder(settings.save_folder, app.path().download_dir().ok());
    Ok(settings)
}

#[tauri::command]
pub fn save_settings(app: tauri::AppHandle, settings: Settings) -> Result<(), String> {
    save(&config_dir(&app)?, &settings).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn config_path(app: tauri::AppHandle) -> Result<String, String> {
    Ok(config_dir(&app)?.join(FILE_NAME).display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("media-dlp-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn defaults_when_no_file_exists() {
        let dir = tmp();
        let s = load(&dir);
        assert_eq!(s, Settings::default());
        assert_eq!(s.video_format, VideoFormat::Mp4);
        assert_eq!(s.audio_format, AudioFormat::Mp3);
        assert_eq!(s.audio_quality, AudioQuality::Good);
        assert_eq!(s.video_quality, VideoQuality::Best);
        assert_eq!(s.update_channel, UpdateChannel::Nightly);
        assert!(!s.audio_only);
    }

    #[test]
    fn round_trips_through_the_file() {
        let dir = tmp();
        let s = Settings {
            audio_only: true,
            audio_format: AudioFormat::Opus,
            video_quality: VideoQuality::P720,
            filename: FilenameSettings {
                channel: true,
                separator: Separator::Underscore,
                ..FilenameSettings::default()
            },
            ..Settings::default()
        };
        save(&dir, &s).unwrap();
        assert_eq!(load(&dir), s);
    }

    #[test]
    fn a_partial_file_fills_the_rest_with_defaults() {
        let dir = tmp();
        std::fs::write(dir.join(FILE_NAME), "audio-only = true\n").unwrap();
        let s = load(&dir);
        assert!(s.audio_only);
        assert_eq!(s.video_format, VideoFormat::Mp4);
    }

    #[test]
    fn a_malformed_file_loads_defaults_instead_of_panicking() {
        let dir = tmp();
        std::fs::write(dir.join(FILE_NAME), "this is not toml {{{").unwrap();
        assert_eq!(load(&dir), Settings::default());
    }

    #[test]
    fn an_unknown_key_is_ignored() {
        let dir = tmp();
        std::fs::write(dir.join(FILE_NAME), "audio-only = true\nnonsense = 3\n").unwrap();
        assert!(load(&dir).audio_only);
    }

    #[test]
    fn separator_as_str_is_the_literal_joiner_text() {
        assert_eq!(Separator::Dash.as_str(), " - ");
        assert_eq!(Separator::Underscore.as_str(), "_");
        assert_eq!(Separator::Space.as_str(), " ");
    }

    #[test]
    fn resolve_save_folder_keeps_an_existing_value() {
        let current = Some(PathBuf::from(r"C:\Existing"));
        let fallback = Some(PathBuf::from(r"C:\Downloads"));
        assert_eq!(resolve_save_folder(current.clone(), fallback), current);
    }

    #[test]
    fn resolve_save_folder_adopts_the_fallback_when_empty() {
        let fallback = Some(PathBuf::from(r"C:\Downloads"));
        assert_eq!(resolve_save_folder(None, fallback.clone()), fallback);
    }

    #[test]
    fn resolve_save_folder_stays_none_when_both_are_absent() {
        assert_eq!(resolve_save_folder(None, None), None);
    }
}
