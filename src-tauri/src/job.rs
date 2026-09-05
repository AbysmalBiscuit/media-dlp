use crate::args::{self, BinaryPaths};
use crate::binaries;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{Emitter, State};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::oneshot;

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

/// yt-dlp's byte-count fields are normally plain integers, but
/// `total_bytes_estimate` is computed by true division in Python and can
/// render as e.g. `10485760.0`; falling back to the integer part before the
/// first `.` keeps that value usable instead of silently becoming `None`.
fn numeric_field<T: std::str::FromStr>(raw: &str) -> Option<T> {
    if raw == "NA" || raw.is_empty() {
        return None;
    }
    raw.parse()
        .ok()
        .or_else(|| raw.split('.').next().and_then(|whole| whole.parse().ok()))
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
        downloaded_bytes: numeric_field(f[1]),
        total_bytes: numeric_field(f[2]).or_else(|| numeric_field(f[3])),
        eta: numeric_field(f[4]),
        speed: numeric_field(f[5]),
        fragment_index: numeric_field(f[6]),
        fragment_count: numeric_field(f[7]),
        playlist_index: numeric_field(f[8]),
        playlist_count: numeric_field(f[9]),
        title: (!f[10].is_empty() && f[10] != "NA").then(|| f[10].to_string()),
    })
}

/// Without this flag, spawning yt-dlp flashes a console window on Windows.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn ytdlp_command(ytdlp: &Path, argv: &[String]) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(ytdlp);
    command.args(argv);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

async fn spawn_ytdlp(
    app: &tauri::AppHandle,
    settings: &crate::settings::Settings,
    save_folder: &str,
    url: &str,
) -> Result<tokio::process::Child, String> {
    let bins = binaries::resolve(app)?;
    let argv = args::download_args(settings, &bins, Path::new(save_folder), url);
    let mut command = ytdlp_command(&bins.ytdlp, &argv);
    command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    command.spawn().map_err(|e| e.to_string())
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

/// A pending cancel hands over a reply channel; `download` signals it back
/// once the child is actually dead and its partial files are gone, so
/// `cancel` never returns while a half-written file could still exist.
type CancelRequest = oneshot::Sender<oneshot::Sender<()>>;

/// Each reservation gets a generation strictly greater than the last, so a
/// job that is done can tell whether the slot still holds its own entry or
/// one a later job has since claimed.
#[derive(Default)]
struct Slot {
    next_generation: u64,
    occupant: Option<(u64, CancelRequest)>,
}

impl Slot {
    fn reserve(&mut self, cancel_tx: CancelRequest) -> Option<u64> {
        if self.occupant.is_some() {
            return None;
        }
        self.next_generation += 1;
        self.occupant = Some((self.next_generation, cancel_tx));
        Some(self.next_generation)
    }

    /// Clears the slot only when it still holds the given generation's
    /// entry, so a job that has already been superseded by a newer
    /// reservation cannot evict that newer job's registration.
    fn release(&mut self, generation: u64) {
        if self
            .occupant
            .as_ref()
            .is_some_and(|(g, _)| *g == generation)
        {
            self.occupant = None;
        }
    }

    /// Takes whichever request currently occupies the slot, regardless of
    /// generation: a cancel always targets whatever job is running now.
    fn take(&mut self) -> Option<CancelRequest> {
        self.occupant.take().map(|(_, tx)| tx)
    }

    #[cfg(test)]
    fn is_occupied(&self) -> bool {
        self.occupant.is_some()
    }
}

#[derive(Default)]
pub struct RunningJob(Mutex<Slot>);

impl RunningJob {
    fn reserve(&self, cancel_tx: CancelRequest) -> Option<u64> {
        self.0.lock().unwrap().reserve(cancel_tx)
    }

    fn release(&self, generation: u64) {
        self.0.lock().unwrap().release(generation);
    }

    fn take(&self) -> Option<CancelRequest> {
        self.0.lock().unwrap().take()
    }
}

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

/// Reads a byte stream line by line without ever failing on invalid UTF-8:
/// a lossy decode keeps every line reachable instead of ending the stream
/// early and silently truncating whatever text (like a sign-in error) was
/// still to come.
async fn read_lines_lossy<R>(reader: R, mut on_line: impl FnMut(&str))
where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                while matches!(buf.last(), Some(b'\n') | Some(b'\r')) {
                    buf.pop();
                }
                on_line(&String::from_utf8_lossy(&buf));
            }
        }
    }
}

/// yt-dlp's own downloader destination, used to target this job's temp
/// artifacts for cleanup on cancel.
fn download_destination(line: &str) -> Option<String> {
    line.strip_prefix("[download] Destination: ")
        .map(str::to_string)
}

/// The path this job most recently reported as its output, whether from the
/// downloader or from a post-processor. A merge or an audio extraction
/// prints its own `Destination: ` line (or `Merging formats into "..."`)
/// after the downloader's, so the last one seen is the real result.
fn reported_destination(line: &str) -> Option<String> {
    const DESTINATION: &str = "Destination: ";
    if let Some(idx) = line.rfind(DESTINATION) {
        return Some(line[idx + DESTINATION.len()..].to_string());
    }
    const MERGE: &str = "Merging formats into \"";
    if let Some(start) = line.find(MERGE) {
        let rest = &line[start + MERGE.len()..];
        if let Some(end) = rest.rfind('"') {
            return Some(rest[..end].to_string());
        }
    }
    None
}

/// Kills yt-dlp (and any ffmpeg it spawned) through the `Child` this task
/// still owns: the pid is read at the moment of killing, while the handle is
/// still open, so the OS cannot have recycled it for an unrelated process.
async fn kill_tree(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        #[cfg(windows)]
        {
            let mut killer = tokio::process::Command::new("taskkill");
            killer.args(["/PID", &pid.to_string(), "/T", "/F"]);
            killer.creation_flags(CREATE_NO_WINDOW);
            let _ = killer.output().await;
        }
        #[cfg(not(windows))]
        {
            let _ = child.kill().await;
        }
    }
    let _ = child.wait().await;
}

/// Generous enough for a working `taskkill /T /F` to finish, short enough
/// that an unresponsive process cannot hang the caller indefinitely.
const KILL_AND_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Kills the child and waits for both output pumps to finish, bounded so a
/// `taskkill` that never terminates the process, or a lingering handle on
/// its pipes, cannot hang the caller forever. Returns `None` once the bound
/// expires, in which case the child or its pipes may still be alive.
async fn kill_and_drain(
    child: &mut tokio::process::Child,
    stdout_pump: tokio::task::JoinHandle<(Vec<String>, String)>,
    stderr_pump: tokio::task::JoinHandle<String>,
) -> Option<Vec<String>> {
    let drain = async {
        kill_tree(child).await;
        let (destinations, _) = stdout_pump.await.unwrap_or_default();
        let _ = stderr_pump.await;
        destinations
    };
    tokio::time::timeout(KILL_AND_DRAIN_TIMEOUT, drain)
        .await
        .ok()
}

fn artifact_path(save_folder: &Path, destination: &str) -> PathBuf {
    let path = PathBuf::from(destination);
    if path.is_absolute() {
        path
    } else {
        save_folder.join(path)
    }
}

fn is_download_artifact(entry_name: &str, base_name: &str) -> bool {
    entry_name == format!("{base_name}.part")
        || entry_name == format!("{base_name}.ytdl")
        || entry_name.starts_with(&format!("{base_name}.part-Frag"))
}

fn remove_with_retry(path: &Path) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match std::fs::remove_file(path) {
            Ok(()) => return,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(_) => return,
        }
    }
}

/// yt-dlp names a fragment temporary `<destination>.part-Frag<N>` and its
/// resume state `<destination>.ytdl`; only files matching a destination this
/// job actually reported are touched, never a sweep of the whole folder.
fn remove_download_artifacts(save_folder: &Path, destinations: &[String]) {
    for destination in destinations {
        let target = artifact_path(save_folder, destination);
        let dir = match target.parent() {
            Some(dir) => dir,
            None => continue,
        };
        let base_name = match target.file_name().and_then(|n| n.to_str()) {
            Some(name) => name,
            None => continue,
        };
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && is_download_artifact(name, base_name)
            {
                remove_with_retry(&path);
            }
        }
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
    enum Outcome {
        Exited(std::io::Result<std::process::ExitStatus>),
        Cancelled(Option<oneshot::Sender<()>>),
    }

    let (cancel_tx, cancel_rx) = oneshot::channel();
    let generation = match running.reserve(cancel_tx) {
        Some(generation) => generation,
        None => return Err("A download is already running.".to_string()),
    };

    let mut child = match spawn_ytdlp(&app, &settings, &save_folder, &url).await {
        Ok(child) => child,
        Err(e) => {
            running.release(generation);
            return Err(e);
        }
    };

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            running.release(generation);
            let _ = child.kill().await;
            return Err("yt-dlp started with no stdout pipe".to_string());
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            running.release(generation);
            let _ = child.kill().await;
            return Err("yt-dlp started with no stderr pipe".to_string());
        }
    };

    let cancelled = Arc::new(AtomicBool::new(false));
    let progress_cancelled = cancelled.clone();
    let emitter = app.clone();
    let stdout_pump = tokio::spawn(async move {
        let mut destinations = Vec::new();
        let mut last_file = String::new();
        read_lines_lossy(stdout, |line| {
            if let Some(dest) = download_destination(line) {
                destinations.push(dest);
            }
            if let Some(dest) = reported_destination(line) {
                last_file = dest;
            }
            if !progress_cancelled.load(Ordering::Relaxed)
                && let Some(progress) = parse_progress_line(line)
            {
                let _ = emitter.emit("download-progress", &progress);
            }
        })
        .await;
        (destinations, last_file)
    });

    let stderr_pump = tokio::spawn(async move {
        let mut collected = String::new();
        read_lines_lossy(stderr, |line| {
            collected.push_str(line);
            collected.push('\n');
        })
        .await;
        collected
    });

    let outcome = tokio::select! {
        status = child.wait() => Outcome::Exited(status),
        ack = cancel_rx => Outcome::Cancelled(ack.ok()),
    };

    running.release(generation);

    match outcome {
        Outcome::Exited(Ok(status)) => {
            let (_, last_file) = stdout_pump.await.unwrap_or_default();
            let collected = stderr_pump.await.unwrap_or_default();
            if status.success() {
                let _ = app.emit("download-finished", Finished { file: last_file });
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
        Outcome::Exited(Err(e)) => {
            cancelled.store(true, Ordering::Relaxed);
            let _ = kill_and_drain(&mut child, stdout_pump, stderr_pump).await;
            Err(e.to_string())
        }
        Outcome::Cancelled(ack) => {
            cancelled.store(true, Ordering::Relaxed);
            // A timed-out drain means the child or its pipes may still be
            // alive, so cleanup is skipped rather than raced against
            // whatever still holds those files.
            if let Some(destinations) = kill_and_drain(&mut child, stdout_pump, stderr_pump).await {
                let folder = PathBuf::from(&save_folder);
                let _ = tokio::task::spawn_blocking(move || {
                    remove_download_artifacts(&folder, &destinations);
                })
                .await;
            }
            if let Some(ack) = ack {
                let _ = ack.send(());
            }
            Ok(())
        }
    }
}

#[tauri::command]
pub async fn cancel(running: State<'_, RunningJob>, save_folder: String) -> Result<(), String> {
    // Cleanup is driven by the destinations `download` itself observed, so
    // this parameter is accepted but unused.
    let _save_folder = save_folder;
    let request = running.take();
    if let Some(request) = request {
        let (ack_tx, ack_rx) = oneshot::channel();
        if request.send(ack_tx).is_ok() {
            let _ = ack_rx.await;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

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
            "9999",
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
    fn a_fractional_estimate_still_parses() {
        let raw = line(&[
            "downloading",
            "1024",
            "NA",
            "8192.0",
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

    #[test]
    fn the_progress_template_carries_the_same_sentinel() {
        assert!(args::PROGRESS_TEMPLATE.contains(PROGRESS_SENTINEL));
    }

    #[test]
    fn a_sign_in_requirement_gets_a_specific_message() {
        let msg = friendly_error("ERROR: [youtube] abc123: Sign in to confirm you're not a bot");
        assert!(msg.contains("signed in"));
        assert!(!msg.contains("--"));
    }

    #[test]
    fn a_login_required_message_gets_the_same_treatment() {
        let msg = friendly_error(
            "ERROR: This video is only available for registered users. Login required.",
        );
        assert!(msg.contains("signed in"));
        assert!(!msg.contains("--"));
    }

    #[test]
    fn any_other_failure_gets_the_generic_message() {
        let msg = friendly_error("ERROR: unable to download video data: HTTP Error 403: Forbidden");
        assert_eq!(
            msg,
            "That download did not finish. Open the details for what yt-dlp reported."
        );
        assert!(!msg.contains("--"));
    }

    #[test]
    fn download_destination_matches_only_the_download_prefix() {
        assert_eq!(
            download_destination("[download] Destination: video.mp4"),
            Some("video.mp4".to_string())
        );
        assert_eq!(
            download_destination("[Merger] Merging formats into \"video.mkv\""),
            None
        );
    }

    #[test]
    fn reported_destination_reads_a_postprocessor_destination_line() {
        assert_eq!(
            reported_destination("[ffmpeg] Destination: final.mp3"),
            Some("final.mp3".to_string())
        );
    }

    #[test]
    fn reported_destination_reads_the_merge_target() {
        assert_eq!(
            reported_destination("[Merger] Merging formats into \"video.mkv\""),
            Some("video.mkv".to_string())
        );
    }

    #[test]
    fn reported_destination_ignores_unrelated_lines() {
        assert_eq!(reported_destination("[download]  42.0% of ~10.00MiB"), None);
    }

    #[test]
    fn artifact_path_joins_a_relative_destination_to_the_save_folder() {
        let folder = PathBuf::from(r"C:\Downloads");
        assert_eq!(
            artifact_path(&folder, "video.mp4"),
            folder.join("video.mp4")
        );
    }

    #[test]
    fn artifact_path_keeps_an_already_absolute_destination() {
        let folder = PathBuf::from(r"C:\Downloads");
        assert_eq!(
            artifact_path(&folder, r"C:\Elsewhere\video.mp4"),
            PathBuf::from(r"C:\Elsewhere\video.mp4")
        );
    }

    fn artifact_temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "media-dlp-artifacts-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn removes_exactly_the_fragment_and_resume_artifacts_it_named() {
        let dir = artifact_temp_dir("named");
        for name in ["a.mp4.part", "b.mp4", "c.mp4.part-Frag0", "d.mp4.ytdl"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        remove_download_artifacts(
            &dir,
            &[
                "a.mp4".to_string(),
                "c.mp4".to_string(),
                "d.mp4".to_string(),
            ],
        );
        let remaining: HashSet<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(remaining, HashSet::from(["b.mp4".to_string()]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn leaves_files_belonging_to_an_unrelated_destination_alone() {
        let dir = artifact_temp_dir("unrelated");
        std::fs::write(dir.join("someone_elses_download.part"), b"x").unwrap();
        std::fs::write(dir.join("a.mp4.parted"), b"x").unwrap();
        remove_download_artifacts(&dir, &["a.mp4".to_string()]);
        assert!(dir.join("someone_elses_download.part").exists());
        assert!(dir.join("a.mp4.parted").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reserve_refuses_a_second_job_while_one_is_occupied() {
        let mut slot = Slot::default();
        let (tx1, _rx1) = oneshot::channel();
        assert!(slot.reserve(tx1).is_some());
        let (tx2, _rx2) = oneshot::channel();
        assert!(slot.reserve(tx2).is_none());
    }

    #[test]
    fn releasing_the_reserving_generation_frees_the_slot() {
        let mut slot = Slot::default();
        let (tx, _rx) = oneshot::channel();
        let generation = slot.reserve(tx).unwrap();
        slot.release(generation);
        assert!(!slot.is_occupied());
    }

    #[test]
    fn releasing_a_stale_generation_does_not_evict_a_newer_job() {
        let mut slot = Slot::default();
        let (tx_a, _rx_a) = oneshot::channel();
        let generation_a = slot.reserve(tx_a).unwrap();
        // A cancel (or any other taker) frees the slot before job A gets
        // around to releasing its own generation.
        assert!(slot.take().is_some());
        let (tx_b, _rx_b) = oneshot::channel();
        let generation_b = slot.reserve(tx_b).unwrap();
        assert_ne!(generation_a, generation_b);
        slot.release(generation_a);
        assert!(slot.is_occupied());
    }
}
