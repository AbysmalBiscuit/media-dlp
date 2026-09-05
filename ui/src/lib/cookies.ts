import type { CookieBrowser, CookieCheck } from './types';

export const BROWSER_LABELS: Record<CookieBrowser, string> = {
  chrome: 'Chrome',
  firefox: 'Firefox',
};

/** The link and browser a check was asked about. */
export interface CookieAsk {
  url: string;
  browser: CookieBrowser;
}

/**
 * The warning to show under the cookie options, or null when the fieldset
 * should stay silent.
 *
 * `present` and `unknown` say nothing: a message on the common path is noise,
 * and a check that could not run tells the reader nothing it can act on.
 *
 * A result whose link or browser no longer matches what the UI holds yields
 * null as well. The check is debounced and async, so a slow answer about a
 * replaced link can land after a fast answer about the current one, and a
 * warning naming the wrong site is worse than no warning.
 */
export function cookieWarning(
  result: CookieCheck,
  asked: CookieAsk,
  current: { url: string; browser: CookieBrowser | null },
): string | null {
  if (asked.url !== current.url || asked.browser !== current.browser) return null;
  if (result.status !== 'absent') return null;
  const browser = BROWSER_LABELS[asked.browser];
  return `${browser} has no cookies for ${result.site}. If this link needs you to be signed in, sign in to ${result.site} in ${browser} and try again.`;
}
