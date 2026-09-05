export type VideoQuality = 'best' | '1080p' | '720p' | '480p';
export type VideoFormat = 'mp4' | 'mkv' | 'webm';
export type AudioFormat = 'mp3' | 'm4a' | 'opus' | 'wav';
export type AudioQuality = 'best' | 'good' | 'smaller';
export type CookieBrowser = 'chrome' | 'firefox';
export type Theme = 'system' | 'light' | 'dark';
export type UpdateChannel = 'stable' | 'nightly' | 'master';
export type Separator = 'dash' | 'underscore' | 'space';

export interface FilenameSettings {
  channel: boolean;
  'upload-date': boolean;
  'playlist-number': boolean;
  'video-id': boolean;
  separator: Separator;
}

export interface Settings {
  'save-folder': string | null;
  'video-quality': VideoQuality;
  'video-format': VideoFormat;
  'audio-format': AudioFormat;
  'audio-quality': AudioQuality;
  'audio-only': boolean;
  'cookie-browser': CookieBrowser | null;
  filename: FilenameSettings;
  theme: Theme;
  'update-channel': UpdateChannel;
}

export interface BrowserSupport {
  browser: CookieBrowser;
  supported: boolean;
  reason: string | null;
}

export type CookieStatus = 'present' | 'absent' | 'unknown';

export interface CookieCheck {
  domain: string;
  status: CookieStatus;
}

export interface ProbeInfo {
  title: string;
  uploader: string | null;
  uploadDate: string | null;
  thumbnail: string | null;
  playlistCount: number | null;
  webpageUrl: string;
  id: string;
}

export interface Progress {
  status: 'downloading' | 'finished' | 'error';
  downloadedBytes: number | null;
  totalBytes: number | null;
  eta: number | null;
  speed: number | null;
  fragmentIndex: number | null;
  fragmentCount: number | null;
  playlistIndex: number | null;
  playlistCount: number | null;
  title: string | null;
}

export interface DownloadFinished {
  file: string;
}

export interface DownloadFailed {
  message: string;
  details: string;
}
