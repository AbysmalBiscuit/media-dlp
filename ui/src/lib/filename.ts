import type { FilenameSettings, ProbeInfo, Separator } from './types';

const SEPARATORS: Record<Separator, string> = {
  dash: ' - ',
  underscore: '_',
  space: ' ',
};

const humanDate = (raw: string) => `${raw.slice(0, 4)}-${raw.slice(4, 6)}-${raw.slice(6, 8)}`;

export function previewFilename(
  f: FilenameSettings,
  info: ProbeInfo | null,
  extension: string,
): string {
  const separator = SEPARATORS[f.separator];
  const parts: string[] = [];
  if (f['playlist-number'] && info?.playlistCount) parts.push('1');
  if (f['upload-date'] && info?.uploadDate) parts.push(humanDate(info.uploadDate));
  if (f.channel && info?.uploader) parts.push(info.uploader);
  parts.push(info?.title ?? 'Video title');
  return `${parts.join(separator)}.${extension}`;
}
