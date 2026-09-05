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

export function previewFilename(
  f: FilenameSettings,
  info: ProbeInfo | null,
  extension: string,
): string {
  const separator = SEPARATORS[f.separator];
  const parts: string[] = [];
  if (f['playlist-number'] && info?.playlistCount) parts.push('1');
  if (f['upload-date'] && info?.uploadDate) parts.push(info.uploadDate);
  if (f.channel && info?.uploader) parts.push(sanitize(info.uploader));
  parts.push(sanitize(info?.title ?? 'Video title'));
  const stem = parts.join(separator).slice(0, TRIM_FILENAME_LENGTH);
  return `${stem}.${extension}`;
}
