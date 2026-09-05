use crate::args::{self, BinaryPaths};
use crate::binaries;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeInfo {
    pub title: String,
    pub uploader: Option<String>,
    pub upload_date: Option<String>,
    pub thumbnail: Option<String>,
    pub playlist_count: Option<u32>,
    pub webpage_url: String,
}

pub fn parse_probe(stdout: &str) -> Result<ProbeInfo, String> {
    let root: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|_| "yt-dlp returned no usable information".to_string())?;

    let playlist_count = root
        .get("playlist_count")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32);

    let item = if root.get("_type").and_then(|v| v.as_str()) == Some("playlist") {
        root.get("entries")
            .and_then(|e| e.as_array())
            .and_then(|e| e.first())
            .ok_or_else(|| "that playlist has no videos in it".to_string())?
    } else {
        &root
    };

    let text = |key: &str| item.get(key).and_then(|v| v.as_str()).map(str::to_string);

    Ok(ProbeInfo {
        title: text("title").ok_or_else(|| "that link has no title".to_string())?,
        uploader: text("uploader"),
        upload_date: text("upload_date"),
        thumbnail: text("thumbnail"),
        playlist_count,
        webpage_url: text("webpage_url").unwrap_or_default(),
    })
}

async fn run(bins: &BinaryPaths, argv: Vec<String>) -> Result<(String, String), String> {
    let mut command = tokio::process::Command::new(&bins.ytdlp);
    command.args(&argv);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let out = command.output().await.map_err(|e| e.to_string())?;
    Ok((
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    ))
}

#[tauri::command]
pub async fn probe(app: tauri::AppHandle, url: String) -> Result<ProbeInfo, String> {
    let bins = binaries::resolve(&app)?;
    let (stdout, stderr) = run(&bins, args::probe_args(&url)).await?;
    parse_probe(&stdout).map_err(|e| if stderr.trim().is_empty() { e } else { stderr })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SINGLE: &str = r#"{
        "title": "A Video",
        "uploader": "A Channel",
        "upload_date": "20260214",
        "thumbnail": "https://example.com/t.jpg",
        "webpage_url": "https://example.com/watch?v=1"
    }"#;

    const PLAYLIST: &str = r#"{
        "_type": "playlist",
        "title": "A Playlist",
        "playlist_count": 12,
        "webpage_url": "https://example.com/list",
        "entries": [
            {
                "title": "First Video",
                "uploader": "A Channel",
                "upload_date": "20260214",
                "thumbnail": "https://example.com/t.jpg",
                "webpage_url": "https://example.com/watch?v=1"
            }
        ]
    }"#;

    #[test]
    fn a_single_video_carries_no_playlist_count() {
        let info = parse_probe(SINGLE).unwrap();
        assert_eq!(info.title, "A Video");
        assert_eq!(info.uploader.as_deref(), Some("A Channel"));
        assert_eq!(info.upload_date.as_deref(), Some("20260214"));
        assert_eq!(info.playlist_count, None);
    }

    #[test]
    fn a_playlist_reports_its_count_and_the_first_entry() {
        let info = parse_probe(PLAYLIST).unwrap();
        assert_eq!(info.title, "First Video");
        assert_eq!(info.playlist_count, Some(12));
    }

    #[test]
    fn missing_optional_fields_are_none_rather_than_an_error() {
        let info = parse_probe(r#"{"title": "Bare", "webpage_url": "u"}"#).unwrap();
        assert_eq!(info.title, "Bare");
        assert!(info.uploader.is_none());
        assert!(info.thumbnail.is_none());
    }

    #[test]
    fn a_non_json_response_is_an_error_not_a_panic() {
        assert!(parse_probe("ERROR: unable to extract").is_err());
    }

    #[test]
    fn an_empty_playlist_is_an_error() {
        assert!(parse_probe(r#"{"_type":"playlist","entries":[],"title":"Empty"}"#).is_err());
    }
}
