import('/qjs/browser/browser.mjs').catch((e) => {
  try { console.log('[browser.mjs] import failed', String(e && e.stack ? e.stack : e)); } catch (_) {}
});
