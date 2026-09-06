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
  id: 'dQw4w9WgXcQ',
};

const chips = (over: Partial<FilenameSettings> = {}): FilenameSettings => ({
  channel: false,
  'upload-date': false,
  'playlist-number': false,
  title: true,
  separator: 'dash',
  ...over,
});

test('title and id only when every other chip is off', () => {
  expect(previewFilename(chips(), info, 'mp4')).toBe('A Video - dQw4w9WgXcQ.mp4');
});

test('the id alone when every chip is off', () => {
  expect(previewFilename(chips({ title: false }), info, 'mp4')).toBe('dQw4w9WgXcQ.mp4');
});

test('chips compose in a fixed order', () => {
  const f = chips({ channel: true, 'upload-date': true, 'playlist-number': true });
  expect(previewFilename(f, info, 'mp4')).toBe(
    '1 - 2026-02-14 - A Channel - A Video - dQw4w9WgXcQ.mp4',
  );
});

test('the separator reaches every chip', () => {
  const f = chips({ channel: true, 'playlist-number': true, separator: 'underscore' });
  expect(previewFilename(f, info, 'mp3')).toBe('1_A Channel_A Video_dQw4w9WgXcQ.mp3');
});

test('a chip whose field is missing contributes nothing', () => {
  const bare = { ...info, uploader: null, uploadDate: null, playlistCount: null };
  const f = chips({ channel: true, 'upload-date': true, 'playlist-number': true });
  expect(previewFilename(f, bare, 'mp4')).toBe('A Video - dQw4w9WgXcQ.mp4');
});

test('an eight-digit upload date is rendered as YYYY-MM-DD', () => {
  const f = chips({ 'upload-date': true });
  expect(previewFilename(f, info, 'mp4')).toBe('2026-02-14 - A Video - dQw4w9WgXcQ.mp4');
});

test('an upload date that is not eight digits passes through untouched', () => {
  const odd: ProbeInfo = { ...info, uploadDate: 'NA' };
  const f = chips({ 'upload-date': true });
  expect(previewFilename(f, odd, 'mp4')).toBe('NA - A Video - dQw4w9WgXcQ.mp4');
});

test('without a probe the preview uses neutral stand-ins', () => {
  expect(previewFilename(chips(), null, 'mp4')).toBe('Video title - aBcD1234xyz.mp4');
});

test('without a probe a chip still changes the preview', () => {
  const before = previewFilename(chips(), null, 'mp4');
  const after = previewFilename(chips({ channel: true }), null, 'mp4');
  expect(after).not.toBe(before);
  expect(after).toBe('Channel name - Video title - aBcD1234xyz.mp4');
});

test('the space separator joins chips with a plain space', () => {
  const f = chips({ channel: true, separator: 'space' });
  expect(previewFilename(f, info, 'mp4')).toBe('A Channel A Video dQw4w9WgXcQ.mp4');
});

test('characters unsafe in a filename are replaced', () => {
  const unsafe: ProbeInfo = { ...info, title: 'Part 1: A/B', uploader: null };
  expect(previewFilename(chips(), unsafe, 'mp4')).toBe('Part 1： A⧸B - dQw4w9WgXcQ.mp4');
});

test('a stem longer than the cap is trimmed, and the extension is untouched', () => {
  const long: ProbeInfo = { ...info, title: 'A'.repeat(200) };
  const result = previewFilename(chips(), long, 'mp4');
  expect(result).toBe(`${'A'.repeat(120)}.mp4`);
});
