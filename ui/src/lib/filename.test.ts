import { expect, test } from 'bun:test';
import { previewFilename } from './filename';
import type { FilenameSettings, ProbeInfo } from './types';

const info: ProbeInfo = {
  title: 'A Video',
  uploader: 'A Channel',
  uploadDate: '20260214',
  thumbnail: null,
  playlistCount: 12,
  webpageUrl: 'u',
};

const chips = (over: Partial<FilenameSettings> = {}): FilenameSettings => ({
  channel: false,
  'upload-date': false,
  'playlist-number': false,
  separator: 'dash',
  ...over,
});

test('title only when every chip is off', () => {
  expect(previewFilename(chips(), info, 'mp4')).toBe('A Video.mp4');
});

test('chips compose in a fixed order', () => {
  const f = chips({ channel: true, 'upload-date': true, 'playlist-number': true });
  expect(previewFilename(f, info, 'mp4')).toBe('1 - 2026-02-14 - A Channel - A Video.mp4');
});

test('the separator reaches every chip', () => {
  const f = chips({ channel: true, 'playlist-number': true, separator: 'underscore' });
  expect(previewFilename(f, info, 'mp3')).toBe('1_A Channel_A Video.mp3');
});

test('a chip whose field is missing contributes nothing', () => {
  const bare = { ...info, uploader: null, uploadDate: null, playlistCount: null };
  const f = chips({ channel: true, 'upload-date': true, 'playlist-number': true });
  expect(previewFilename(f, bare, 'mp4')).toBe('A Video.mp4');
});

test('without a probe the preview uses a neutral stand-in', () => {
  expect(previewFilename(chips(), null, 'mp4')).toBe('Video title.mp4');
});

test('the space separator joins chips with a plain space', () => {
  const f = chips({ channel: true, separator: 'space' });
  expect(previewFilename(f, info, 'mp4')).toBe('A Channel A Video.mp4');
});

test('characters unsafe in a filename are replaced', () => {
  const unsafe: ProbeInfo = { ...info, title: 'Part 1: A/B', uploader: null };
  expect(previewFilename(chips(), unsafe, 'mp4')).toBe('Part 1： A⧸B.mp4');
});
