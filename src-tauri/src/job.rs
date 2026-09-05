use crate::args::{self, BinaryPaths};
use crate::binaries;
use serde::Serialize;
use std::sync::Mutex;
use tauri::{Emitter, State};
use tokio::io::{AsyncBufReadExt, BufReader};

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

pub const PROGRESS_SENTINEL: &str = "@P@";
const PROGRESS_FIELD_COUNT: usize = 11;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProgressStatus {
    Downloading,
    Finished,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub status: ProgressStatus,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub eta: Option<u64>,
    pub speed: Option<f64>,
    pub fragment_index: Option<u32>,
    pub fragment_count: Option<u32>,
    pub playlist_index: Option<u32>,
    pub playlist_count: Option<u32>,
    pub title: Option<String>,
}

fn field<T: std::str::FromStr>(raw: &str) -> Option<T> {
    if raw == "NA" || raw.is_empty() {
        None
    } else {
        raw.parse().ok()
    }
}

/// Fields are tab-separated with the title last: `splitn` bounds the split so a
/// tab inside the title cannot shift any earlier field.
pub fn parse_progress_line(line: &str) -> Option<Progress> {
    let body = line
        .trim_end_matches(['\r', '\n'])
        .strip_prefix(PROGRESS_SENTINEL)?;
    let f: Vec<&str> = body.splitn(PROGRESS_FIELD_COUNT, '\t').collect();
    if f.len() < PROGRESS_FIELD_COUNT {
        return None;
    }
    let status = match f[0] {
        "downloading" => ProgressStatus::Downloading,
        "finished" => ProgressStatus::Finished,
        "error" => ProgressStatus::Error,
        _ => return None,
    };
    Some(Progress {
        status,
        downloaded_bytes: field(f[1]),
        total_bytes: field(f[2]).or_else(|| field(f[3])),
        eta: field(f[4]),
        speed: field(f[5]),
        fragment_index: field(f[6]),
        fragment_count: field(f[7]),
        playlist_index: field(f[8]),
        playlist_count: field(f[9]),
        title: (!f[10].is_empty() && f[10] != "NA").then(|| f[10].to_string()),
    })
}

/// Without this flag, spawning yt-dlp flashes a console window on Windows.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn ytdlp_command(ytdlp: &std::path::Path, argv: &[String]) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(ytdlp);
    command.args(argv);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

async fn run(bins: &BinaryPaths, argv: Vec<String>) -> Result<(String, String), String> {
    let out = ytdlp_command(&bins.ytdlp, &argv)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    Ok((
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    ))
}

#[tauri::command]
pub async fn probe(app: tauri::AppHandle, url: String) -> Result<ProbeInfo, String> {
    let bins = binaries::resolve(&app)?;
    let (stdout, stderr) = run(&bins, args::probe_args(&url)).await?;
    parse_probe(&stdout).map_err(|e| {
        if stderr.trim().is_empty() {
            e
        } else {
            stderr.trim().to_string()
        }
    })
}

#[derive(Default)]
pub struct RunningJob(pub Mutex<Option<u32>>);

#[derive(Clone, Serialize)]
struct Finished {
    file: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Failed {
    message: String,
    details: String,
}

/// yt-dlp reports a sign-in requirement in prose; the wording differs per site,
/// so the check stays on the two phrases every extractor shares.
fn friendly_error(stderr: &str) -> String {
    let lower = stderr.to_lowercase();
    if lower.contains("sign in") || lower.contains("login required") {
        "This video needs you to be signed in. Pick a browser under Cookies and try again."
            .to_string()
    } else {
        "That download did not finish. Open the details for what yt-dlp reported.".to_string()
    }
}

#[tauri::command]
pub async fn download(
    app: tauri::AppHandle,
    running: State<'_, RunningJob>,
    settings: crate::settings::Settings,
    save_folder: String,
    url: String,
) -> Result<(), String> {
    let bins = binaries::resolve(&app)?;
    let argv = args::download_args(&settings, &bins, std::path::Path::new(&save_folder), &url);

    let mut command = ytdlp_command(&bins.ytdlp, &argv);
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = command.spawn().map_err(|e| e.to_string())?;
    *running.0.lock().unwrap() = child.id();

    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;

    let emitter = app.clone();
    let pump = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut last_file = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(rest) = line.strip_prefix("[download] Destination: ") {
                last_file = rest.to_string();
            }
            if let Some(progress) = parse_progress_line(&line) {
                let _ = emitter.emit("download-progress", &progress);
            }
        }
        last_file
    });

    let mut collected = String::new();
    let mut err_lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = err_lines.next_line().await {
        collected.push_str(&line);
        collected.push('\n');
    }

    let status = child.wait().await.map_err(|e| e.to_string())?;
    let file = pump.await.unwrap_or_default();
    *running.0.lock().unwrap() = None;

    if status.success() {
        let _ = app.emit("download-finished", Finished { file });
    } else {
        let _ = app.emit(
            "download-failed",
            Failed {
                message: friendly_error(&collected),
                details: collected,
            },
        );
    }
    Ok(())
}

/// yt-dlp writes each stream to `<name>.<id>.part` and cleans them up only on a
/// graceful exit, so a killed process leaves them behind.
fn remove_partials(folder: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "part") {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[tauri::command]
pub fn cancel(running: State<'_, RunningJob>, save_folder: String) -> Result<(), String> {
    let pid = running.0.lock().unwrap().take();
    if let Some(pid) = pid {
        #[cfg(windows)]
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output();
        #[cfg(not(windows))]
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output();
        std::thread::sleep(std::time::Duration::from_millis(300));
        remove_partials(std::path::Path::new(&save_folder));
    }
    Ok(())
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

    fn line(fields: &[&str]) -> String {
        format!("download:@P@{}", fields.join("\t")).replace("download:@P@", "@P@")
    }

    #[test]
    fn a_full_progress_line_parses_every_field() {
        let raw = line(&[
            "downloading",
            "1024",
            "4096",
            "4096",
            "12",
            "512.5",
            "3",
            "10",
            "2",
            "12",
            "A Video",
        ]);
        let p = parse_progress_line(&raw).unwrap();
        assert_eq!(p.status, ProgressStatus::Downloading);
        assert_eq!(p.downloaded_bytes, Some(1024));
        assert_eq!(p.total_bytes, Some(4096));
        assert_eq!(p.eta, Some(12));
        assert_eq!(p.speed, Some(512.5));
        assert_eq!(p.fragment_index, Some(3));
        assert_eq!(p.fragment_count, Some(10));
        assert_eq!(p.playlist_index, Some(2));
        assert_eq!(p.playlist_count, Some(12));
        assert_eq!(p.title.as_deref(), Some("A Video"));
    }

    #[test]
    fn na_fields_become_none() {
        let raw = line(&[
            "downloading",
            "1024",
            "NA",
            "NA",
            "NA",
            "NA",
            "NA",
            "NA",
            "NA",
            "NA",
            "A Video",
        ]);
        let p = parse_progress_line(&raw).unwrap();
        assert_eq!(p.total_bytes, None);
        assert_eq!(p.eta, None);
        assert_eq!(p.speed, None);
        assert_eq!(p.playlist_index, None);
    }

    #[test]
    fn a_total_falls_back_to_the_estimate() {
        let raw = line(&[
            "downloading",
            "1024",
            "NA",
            "8192",
            "NA",
            "NA",
            "NA",
            "NA",
            "NA",
            "NA",
            "T",
        ]);
        assert_eq!(parse_progress_line(&raw).unwrap().total_bytes, Some(8192));
    }

    #[test]
    fn a_tab_inside_the_title_does_not_corrupt_earlier_fields() {
        let raw = line(&[
            "finished",
            "4096",
            "4096",
            "4096",
            "0",
            "0",
            "1",
            "1",
            "1",
            "1",
            "Odd\tTitle",
        ]);
        let p = parse_progress_line(&raw).unwrap();
        assert_eq!(p.status, ProgressStatus::Finished);
        assert_eq!(p.downloaded_bytes, Some(4096));
        assert_eq!(p.title.as_deref(), Some("Odd\tTitle"));
    }

    #[test]
    fn lines_without_the_sentinel_are_ignored() {
        assert!(parse_progress_line("[download] Destination: video.mp4").is_none());
        assert!(parse_progress_line("").is_none());
    }

    #[test]
    fn a_truncated_line_is_ignored_rather_than_parsed_wrongly() {
        assert!(parse_progress_line("@P@downloading\t1024").is_none());
    }

    #[test]
    fn an_unknown_status_is_ignored() {
        let raw = line(&["sideways", "1", "1", "1", "1", "1", "1", "1", "1", "1", "T"]);
        assert!(parse_progress_line(&raw).is_none());
    }
}
