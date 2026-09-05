import { expect, test } from 'bun:test';
import { cookieWarning } from './cookies';
import type { CookieCheck } from './types';

const url = 'https://www.instagram.com/reel/DcxYeDDvkUG/';
const asked = { url, browser: 'firefox' } as const;
const now = { url, browser: 'firefox' } as const;

const check = (status: CookieCheck['status']): CookieCheck => ({
  site: 'instagram.com',
  status,
});

test('an absent store warns, naming the site and the browser', () => {
  expect(cookieWarning(check('absent'), asked, now)).toBe(
    'Firefox has no cookies for instagram.com. If this link needs you to be signed in, sign in to instagram.com in Firefox and try again.',
  );
});

test('a store holding the cookies says nothing', () => {
  expect(cookieWarning(check('present'), asked, now)).toBeNull();
});

test('a check that could not run says nothing', () => {
  expect(cookieWarning(check('unknown'), asked, now)).toBeNull();
});

test('a result about a link the user has replaced says nothing', () => {
  const current = { url: 'https://youtube.com/watch?v=abc', browser: 'firefox' } as const;
  expect(cookieWarning(check('absent'), asked, current)).toBeNull();
});

test('a result about a browser the user has changed says nothing', () => {
  expect(cookieWarning(check('absent'), asked, { url, browser: 'chrome' })).toBeNull();
});

test('a result arriving after the browser is set to none says nothing', () => {
  expect(cookieWarning(check('absent'), asked, { url, browser: null })).toBeNull();
});
