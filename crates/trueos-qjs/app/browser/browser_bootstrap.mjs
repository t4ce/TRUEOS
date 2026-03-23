import('/qjs/browser/browser.mjs').catch((e) => {
  try { console.log('[surfer bootstrap] import failed', String(e && e.stack ? e.stack : e)); } catch (_) {}
});
