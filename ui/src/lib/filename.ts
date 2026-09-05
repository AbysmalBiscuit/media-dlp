import type { FilenameSettings, ProbeInfo, Separator } from './types';

const SEPARATORS: Record<Separator, string> = {
  dash: ' - ',
  underscore: '_',
  space: ' ',
};

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
  return `${parts.join(separator)}.${extension}`;
}
