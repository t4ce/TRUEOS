function appWindowMetrics(appWindowId, rectEntries, scrollOffsetFor, selfScrollbarId = '') {
  let appWindowRect = null;
  for (let i = 0; i < rectEntries.length; i++) {
    const r = rectEntries[i];
    if (!r) continue;
    if (String(r.id || '') === appWindowId && String(r.tag || '') === 'html_app_window') {
      appWindowRect = r;
      break;
    }
  }
  if (!appWindowRect) return null;

  const appWindowScrollY = Math.max(0, Number(scrollOffsetFor(appWindowId) || 0));
  const viewportH = Math.max(1, Number(appWindowRect.h || 0));
  let contentBottom = Number(appWindowRect.y || 0) + viewportH;
  for (let i = 0; i < rectEntries.length; i++) {
    const c = rectEntries[i];
    if (!c) continue;
    const cid = String(c.id || '');
    if (!cid || cid === selfScrollbarId || cid === appWindowId) continue;
    if (!cid.startsWith(`${appWindowId}/`)) continue;
    if (cid.includes('/dialog[')) continue;
    if (String(c.tag || '') === 'scrollbar') continue;
    const b = Number(c.y || 0) + Number(c.h || 0) + appWindowScrollY;
    if (b > contentBottom) contentBottom = b;
  }

  const contentH = Math.max(viewportH, contentBottom - Number(appWindowRect.y || 0));
  const maxScroll = Math.max(0, contentH - viewportH);
  return { appWindowRect, viewportH, contentH, maxScroll, scrollY: Math.min(maxScroll, appWindowScrollY) };
}

export function renderScrollbarWidget(rect, ctx) {
  if (!rect || String(rect.tag || '') !== 'scrollbar') return [];
  if (!ctx || ctx.mode !== 'collect' || !Array.isArray(ctx.rectEntries) || typeof ctx.scrollOffsetFor !== 'function') return [];

  const selfId = String(rect.id || '');
  if (!selfId) return [];
  const slash = selfId.lastIndexOf('/');
  if (slash <= 0) return [];
  const parentId = selfId.slice(0, slash);
  const appWindowId = `${parentId}/html_app_window[0]`;
  const metrics = appWindowMetrics(appWindowId, ctx.rectEntries, ctx.scrollOffsetFor, selfId);
  if (!metrics) return [];
  const { appWindowRect, viewportH, contentH, maxScroll, scrollY: clampedScroll } = metrics;

  const barW = Math.max(4, Number(globalThis.__trueosThemeScrollbarW || 8));
  const x = Math.round(Number(appWindowRect.x || 0));
  const y = Math.round(Number(appWindowRect.y || 0));
  const w = Math.max(4, Math.round(Math.min(barW, Number(appWindowRect.w || 0))));
  const h = Math.max(4, Math.round(Number(appWindowRect.h || 0)));

  // 1px border + 1px inner gap.
  const innerX = x + 2;
  const innerY = y + 2;
  const innerW = Math.max(1, w - 4);
  const innerH = Math.max(1, h - 4);

  // Thumb height scales linearly with viewport/content, clamped to 20% minimum.
  const ratio = maxScroll <= 0 ? 1 : (viewportH / Math.max(viewportH, contentH));
  const thumbRatio = Math.max(0.2, Math.min(1, ratio));
  const thumbH = maxScroll <= 0 ? innerH : Math.max(1, Math.round(innerH * thumbRatio));
  const thumbTravel = Math.max(0, innerH - thumbH);
  const thumbOff = maxScroll <= 0 ? 0 : Math.round((clampedScroll / maxScroll) * thumbTravel);
  const thumbY = innerY + thumbOff;

  const frameDepth = Math.max(0, Number(rect.depth || 0) + 1);
  return [
    x, y, w, h, frameDepth, 0, 1,
    innerX, thumbY, innerW, thumbH, frameDepth + 1, 0, 2,
  ];
}
