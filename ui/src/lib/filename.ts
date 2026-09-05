import type { FilenameSettings, ProbeInfo, Separator } from './types';

const SEPARATORS: Record<Separator, string> = {
  dash: ' - ',
  underscore: '_',
  space: ' ',
};

// Mirrors the --trim-filenames value the argument builder passes to yt-dlp.
// yt-dlp's cap excludes the extension, so this bounds the stem before the
// extension is appended.
const TRIM_FILENAME_LENGTH = 120;

const SUBSTITUTES: Record<string, string> = {
  '"': '＂',
  '*': '＊',
  ':': '：',
  '<': '＜',
  '>': '＞',
  '?': '？',
  '|': '｜',
  '/': '⧸',
  '\\': '⧹',
};

const sanitize = (raw: string) => raw.replace(/["*:<>?|/\\]/g, (char) => SUBSTITUTES[char]);

// Only an eight-digit upload date has a known YYYY-MM-DD shape; anything else
// is passed through so the preview never disagrees with what yt-dlp writes.
function formatUploadDate(raw: string): string {
  if (!/^\d{8}$/.test(raw)) return raw;
  return `${raw.slice(0, 4)}-${raw.slice(4, 6)}-${raw.slice(6, 8)}`;
}

export function previewFilename(
  f: FilenameSettings,
  info: ProbeInfo | null,
  extension: string,
): string {
  const separator = SEPARATORS[f.separator];
  const parts: string[] = [];
  if (f['playlist-number'] && info?.playlistCount) parts.push('1');
  if (f['upload-date'] && info?.uploadDate) parts.push(formatUploadDate(info.uploadDate));
  if (f.channel && info?.uploader) parts.push(sanitize(info.uploader));
  parts.push(sanitize(info?.title ?? 'Video title'));
  if (f['video-id'] && info?.id) parts.push(info.id);
  const stem = parts.join(separator).slice(0, TRIM_FILENAME_LENGTH);
  return `${stem}.${extension}`;
}
