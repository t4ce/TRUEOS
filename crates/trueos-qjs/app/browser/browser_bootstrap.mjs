const INITIAL_NAVIGATE_TO_W3C_PNG = true;
const INITIAL_W3C_PNG_URL = 'https://www.w3.org/Graphics/PNG/Inline-img.html';

const host = (typeof globalThis !== 'undefined') ? globalThis : this;

function resolveBootstrapUrl(url) {
  const value = typeof url === 'string' ? url.trim() : '';
  return value || '';
}

function startInitialNavigation() {
  if (!INITIAL_NAVIGATE_TO_W3C_PNG) return null;
  const url = resolveBootstrapUrl(INITIAL_W3C_PNG_URL);
  if (!url || typeof host.fetch !== 'function') return null;
  if (typeof host.__trueosPrewarmUrl === 'function') {
    try { host.__trueosPrewarmUrl(url); } catch (_) {}
  }
  host.__trueosBrowserBootstrapInitialUrl = url;
  host.__trueosBrowserCurrentUrl = url;
  host.__trueosBrowserUrl = url;
  const promise = Promise.resolve(host.fetch(url, { method: 'GET' }))
    .then((response) => Promise.resolve(response.text()).then((html) => ({
      ok: 1,
      url,
      html: String(html || ''),
    })))
    .catch((err) => ({
      ok: 0,
      url,
      error: String(err && err.stack ? err.stack : err || 'unknown error'),
    }));
  host.__trueosBrowserBootstrapInitialNav = promise;
  return promise;
}

startInitialNavigation();

import('/qjs/browser/browser.mjs').catch((e) => {
  try { console.log('[browser.mjs] import failed', String(e && e.stack ? e.stack : e)); } catch (_) {}
});
