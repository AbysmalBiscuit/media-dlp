use crate::settings::{
    AudioFormat, AudioQuality, CookieBrowser, FilenameSettings, Settings, VideoFormat, VideoQuality,
};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryPaths {
    pub ytdlp: PathBuf,
    pub ffmpeg: PathBuf,
}

pub const PROGRESS_TEMPLATE: &str = concat!(
    "download:@P@",
    "%(progress.status)s\t",
    "%(progress.downloaded_bytes)s\t",
    "%(progress.total_bytes)s\t",
    "%(progress.total_bytes_estimate)s\t",
    "%(progress.eta)s\t",
    "%(progress.speed)s\t",
    "%(progress.fragment_index)s\t",
    "%(progress.fragment_count)s\t",
    "%(info.playlist_index)s\t",
    "%(info.playlist_count)s\t",
    "%(info.title)s"
);

impl VideoQuality {
    fn height(self) -> Option<u32> {
        match self {
            VideoQuality::Best => None,
            VideoQuality::P1080 => Some(1080),
            VideoQuality::P720 => Some(720),
            VideoQuality::P480 => Some(480),
        }
    }
}

impl VideoFormat {
    fn as_str(self) -> &'static str {
        match self {
            VideoFormat::Mp4 => "mp4",
            VideoFormat::Mkv => "mkv",
            VideoFormat::Webm => "webm",
        }
    }

    /// The `ext` sort term steers selection toward codecs the container holds
    /// natively, so the merge is a remux. mkv accepts every codec and needs none.
    fn ext_preference(self) -> Option<&'static str> {
        match self {
            VideoFormat::Mp4 => Some("ext:mp4:m4a"),
            VideoFormat::Webm => Some("ext:webm:webm"),
            VideoFormat::Mkv => None,
        }
    }
}

impl AudioFormat {
    fn as_str(self) -> &'static str {
        match self {
            AudioFormat::Mp3 => "mp3",
            AudioFormat::M4a => "m4a",
            AudioFormat::Opus => "opus",
            AudioFormat::Wav => "wav",
        }
    }
}

impl AudioQuality {
    fn as_str(self) -> &'static str {
        match self {
            AudioQuality::Best => "0",
            AudioQuality::Good => "5",
            AudioQuality::Smaller => "9",
        }
    }
}

impl CookieBrowser {
    pub fn as_str(self) -> &'static str {
        match self {
            CookieBrowser::Chrome => "chrome",
            CookieBrowser::Firefox => "firefox",
        }
    }
}

pub fn filename_template(f: &FilenameSettings) -> String {
    let sep = f.separator.as_str();
    let mut t = String::new();
    for (enabled, field) in [
        (f.playlist_number, "playlist_index"),
        (f.upload_date, "upload_date>%Y-%m-%d"),
        (f.channel, "uploader"),
        (f.title, "title"),
    ] {
        if enabled {
            t.push_str(&format!("%({field}&{{}}{sep}|)s"));
        }
    }
    // The id is not a chip: Instagram gives every post by an account the same
    // title, so without the id the second download overwrites the first.
    t.push_str("%(id)s.%(ext)s");
    t
}

pub fn probe_args(url: &str) -> Vec<String> {
    ["-J", "--no-warnings", "-I", "1", url]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

fn format_sort(settings: &Settings) -> Option<String> {
    let res = settings.video_quality.height().map(|h| format!("res:{h}"));
    let ext = settings.video_format.ext_preference().map(str::to_string);
    match (res, ext) {
        (Some(r), Some(e)) => Some(format!("{r},{e}")),
        (Some(r), None) => Some(r),
        (None, Some(e)) => Some(e),
        (None, None) => None,
    }
}

pub fn download_args(
    settings: &Settings,
    bins: &BinaryPaths,
    save_folder: &Path,
    url: &str,
) -> Vec<String> {
    let mut argv: Vec<String> = Vec::new();
    let mut push = |a: &str| argv.push(a.to_string());

    if settings.audio_only {
        push("-f");
        push("ba/b");
        push("-x");
        push("--audio-format");
        push(settings.audio_format.as_str());
        push("--audio-quality");
        push(settings.audio_quality.as_str());
    } else {
        push("-f");
        push("bv*+ba/b");
        if let Some(sort) = format_sort(settings) {
            push("-S");
            push(&sort);
        }
        push("--merge-output-format");
        push(settings.video_format.as_str());
    }

    push("-P");
    push(&save_folder.display().to_string());
    push("-o");
    push(&filename_template(&settings.filename));
    push("--trim-filenames");
    push("120");
    push("--ffmpeg-location");
    push(&bins.ffmpeg.display().to_string());
    push("--newline");
    push("--progress-delta");
    push("0.5");
    push("--progress-template");
    push(PROGRESS_TEMPLATE);

    if let Some(browser) = settings.cookie_browser {
        push("--cookies-from-browser");
        push(browser.as_str());
    }

    push(url);
    argv
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::*;
    use std::path::PathBuf;

    fn bins() -> BinaryPaths {
        BinaryPaths {
            ytdlp: PathBuf::from(r"C:\bin\yt-dlp.exe"),
            ffmpeg: PathBuf::from(r"C:\bin\ffmpeg.exe"),
        }
    }

    fn folder() -> PathBuf {
        PathBuf::from(r"C:\Downloads")
    }

    fn arg_after(argv: &[String], flag: &str) -> Option<String> {
        argv.iter()
            .position(|a| a == flag)
            .and_then(|i| argv.get(i + 1))
            .cloned()
    }

    #[test]
    fn probe_asks_for_json_and_only_the_first_item() {
        assert_eq!(
            probe_args("https://example.com/v"),
            vec!["-J", "--no-warnings", "-I", "1", "https://example.com/v"]
        );
    }

    #[test]
    fn title_and_id_template_when_every_other_chip_is_off() {
        let f = FilenameSettings {
            channel: false,
            upload_date: false,
            playlist_number: false,
            title: true,
            separator: Separator::Dash,
        };
        assert_eq!(filename_template(&f), "%(title&{} - |)s%(id)s.%(ext)s");
    }

    #[test]
    fn the_id_alone_when_every_chip_is_off() {
        let f = FilenameSettings {
            channel: false,
            upload_date: false,
            playlist_number: false,
            title: false,
            separator: Separator::Dash,
        };
        assert_eq!(filename_template(&f), "%(id)s.%(ext)s");
    }

    #[test]
    fn every_chip_composes_in_a_fixed_order() {
        let f = FilenameSettings {
            channel: true,
            upload_date: true,
            playlist_number: true,
            title: true,
            separator: Separator::Dash,
        };
        assert_eq!(
            filename_template(&f),
            "%(playlist_index&{} - |)s%(upload_date>%Y-%m-%d&{} - |)s%(uploader&{} - |)s%(title&{} - |)s%(id)s.%(ext)s"
        );
    }

    #[test]
    fn the_separator_choice_reaches_every_chip() {
        let f = FilenameSettings {
            channel: true,
            upload_date: false,
            playlist_number: true,
            title: true,
            separator: Separator::Underscore,
        };
        assert_eq!(
            filename_template(&f),
            "%(playlist_index&{}_|)s%(uploader&{}_|)s%(title&{}_|)s%(id)s.%(ext)s"
        );
    }

    #[test]
    fn the_upload_date_chip_formats_as_strftime() {
        let f = FilenameSettings {
            upload_date: true,
            playlist_number: false,
            ..FilenameSettings::default()
        };
        assert_eq!(
            filename_template(&f),
            "%(upload_date>%Y-%m-%d&{} - |)s%(title&{} - |)s%(id)s.%(ext)s"
        );
    }

    #[test]
    fn the_id_trails_the_title_whatever_the_chips_say() {
        let f = FilenameSettings::default();
        assert_eq!(
            filename_template(&f),
            "%(playlist_index&{} - |)s%(title&{} - |)s%(id)s.%(ext)s"
        );
    }

    #[test]
    fn mp4_at_best_quality_sorts_by_extension_only() {
        let s = Settings::default();
        let argv = download_args(&s, &bins(), &folder(), "URL");
        assert_eq!(arg_after(&argv, "-f").unwrap(), "bv*+ba/b");
        assert_eq!(arg_after(&argv, "-S").unwrap(), "ext:mp4:m4a");
        assert_eq!(arg_after(&argv, "--merge-output-format").unwrap(), "mp4");
    }

    #[test]
    fn a_quality_cap_prefixes_the_sort_with_a_resolution() {
        let s = Settings {
            video_quality: VideoQuality::P720,
            ..Settings::default()
        };
        let argv = download_args(&s, &bins(), &folder(), "URL");
        assert_eq!(arg_after(&argv, "-S").unwrap(), "res:720,ext:mp4:m4a");
    }

    #[test]
    fn mkv_at_best_quality_needs_no_sort_at_all() {
        let s = Settings {
            video_format: VideoFormat::Mkv,
            ..Settings::default()
        };
        let argv = download_args(&s, &bins(), &folder(), "URL");
        assert!(!argv.iter().any(|a| a == "-S"));
        assert_eq!(arg_after(&argv, "--merge-output-format").unwrap(), "mkv");
    }

    #[test]
    fn mkv_with_a_cap_sorts_by_resolution_only() {
        let s = Settings {
            video_format: VideoFormat::Mkv,
            video_quality: VideoQuality::P1080,
            ..Settings::default()
        };
        let argv = download_args(&s, &bins(), &folder(), "URL");
        assert_eq!(arg_after(&argv, "-S").unwrap(), "res:1080");
    }

    #[test]
    fn webm_prefers_webm_on_both_streams() {
        let s = Settings {
            video_format: VideoFormat::Webm,
            video_quality: VideoQuality::P480,
            ..Settings::default()
        };
        let argv = download_args(&s, &bins(), &folder(), "URL");
        assert_eq!(arg_after(&argv, "-S").unwrap(), "res:480,ext:webm:webm");
        assert_eq!(arg_after(&argv, "--merge-output-format").unwrap(), "webm");
    }

    #[test]
    fn audio_only_extracts_and_drops_every_video_flag() {
        let s = Settings {
            audio_only: true,
            audio_format: AudioFormat::Opus,
            audio_quality: AudioQuality::Best,
            ..Settings::default()
        };
        let argv = download_args(&s, &bins(), &folder(), "URL");
        assert_eq!(arg_after(&argv, "-f").unwrap(), "ba/b");
        assert!(argv.iter().any(|a| a == "-x"));
        assert_eq!(arg_after(&argv, "--audio-format").unwrap(), "opus");
        assert_eq!(arg_after(&argv, "--audio-quality").unwrap(), "0");
        assert!(!argv.iter().any(|a| a == "--merge-output-format"));
        assert!(!argv.iter().any(|a| a == "-S"));
    }

    #[test]
    fn audio_quality_tiers_map_to_the_vbr_scale() {
        for (tier, expected) in [
            (AudioQuality::Best, "0"),
            (AudioQuality::Good, "5"),
            (AudioQuality::Smaller, "9"),
        ] {
            let s = Settings {
                audio_only: true,
                audio_quality: tier,
                ..Settings::default()
            };
            let argv = download_args(&s, &bins(), &folder(), "URL");
            assert_eq!(arg_after(&argv, "--audio-quality").unwrap(), expected);
        }
    }

    #[test]
    fn no_cookie_flag_when_no_browser_is_chosen() {
        let s = Settings {
            cookie_browser: None,
            ..Settings::default()
        };
        let argv = download_args(&s, &bins(), &folder(), "URL");
        assert!(!argv.iter().any(|a| a == "--cookies-from-browser"));
    }

    #[test]
    fn a_chosen_browser_becomes_the_cookie_flag() {
        let s = Settings {
            cookie_browser: Some(CookieBrowser::Firefox),
            ..Settings::default()
        };
        let argv = download_args(&s, &bins(), &folder(), "URL");
        assert_eq!(
            arg_after(&argv, "--cookies-from-browser").unwrap(),
            "firefox"
        );
    }

    #[test]
    fn every_download_carries_the_common_flags_and_ends_with_the_url() {
        let argv = download_args(&Settings::default(), &bins(), &folder(), "URL");
        assert_eq!(arg_after(&argv, "-P").unwrap(), r"C:\Downloads");
        assert_eq!(
            arg_after(&argv, "-o").unwrap(),
            "%(playlist_index&{} - |)s%(title&{} - |)s%(id)s.%(ext)s"
        );
        assert_eq!(arg_after(&argv, "--trim-filenames").unwrap(), "120");
        assert_eq!(
            arg_after(&argv, "--ffmpeg-location").unwrap(),
            r"C:\bin\ffmpeg.exe"
        );
        assert_eq!(arg_after(&argv, "--progress-delta").unwrap(), "0.5");
        assert_eq!(
            arg_after(&argv, "--progress-template").unwrap(),
            PROGRESS_TEMPLATE
        );
        assert!(argv.iter().any(|a| a == "--newline"));
        assert_eq!(argv.last().unwrap(), "URL");
    }

    #[test]
    fn the_progress_template_names_eleven_tab_separated_fields() {
        let body = PROGRESS_TEMPLATE.strip_prefix("download:@P@").unwrap();
        assert_eq!(body.split('\t').count(), 11);
        assert!(body.ends_with("%(info.title)s"));
    }
}
