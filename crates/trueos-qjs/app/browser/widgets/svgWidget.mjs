const svgPixelsCache = new Map();

function getSvgPixels(assetId) {
  if (!assetId) return null;
  if (svgPixelsCache.has(assetId)) return svgPixelsCache.get(assetId);
  const fn = globalThis.__trueosReadSvgPixels;
  if (typeof fn !== 'function') {
    svgPixelsCache.set(assetId, null);
    return null;
  }
  let raw = null;
  try {
    raw = fn(assetId);
  } catch (_) {
    raw = null;
  }
  if (!Array.isArray(raw) || raw.length < 3) {
    svgPixelsCache.set(assetId, null);
    return null;
  }
  const w = Math.max(1, Number(raw[0] || 1) | 0);
  const h = Math.max(1, Number(raw[1] || 1) | 0);
  const len = Math.max(0, Number(raw[2] || 0) | 0);
  const px = new Uint32Array(len);
  for (let i = 0; i < len && (i + 3) < raw.length; i++) {
    px[i] = Number(raw[i + 3] || 0) >>> 0;
  }
  const out = { w, h, px };
  svgPixelsCache.set(assetId, out);
  return out;
}

export function renderSvgWidget(rect, ctx) {
  if (!rect || String(rect.tag || '') !== 'svg') return [];
  if (!ctx || ctx.mode !== 'collect') return [];
  if (typeof ctx.getSvgAssetIdByBlockId !== 'function') return [];

  const assetId = Number(ctx.getSvgAssetIdByBlockId(String(rect.id || '')) || 0) >>> 0;
  if (!assetId) return [];
  const img = getSvgPixels(assetId);
  if (!img || !img.px || img.px.length <= 0) return [];

  const x = Math.round(Number(rect.x || 0));
  const y = Math.round(Number(rect.y || 0));
  const w = Math.max(1, Math.round(Number(rect.w || 0)));
  const h = Math.max(1, Math.round(Number(rect.h || 0)));
  const depth = Math.max(0, Number(rect.depth || 0));

  const sx = w / img.w;
  const sy = h / img.h;
  const out = [];

  for (let py = 0; py < img.h; py++) {
    let runColor = 0;
    let runStart = -1;
    for (let px = 0; px < img.w; px++) {
      const idx = py * img.w + px;
      const color = idx < img.px.length ? (img.px[idx] >>> 0) : 0;
      const visible = ((color >>> 24) & 0xFF) !== 0;
      if (visible && runStart < 0) {
        runStart = px;
        runColor = color;
        continue;
      }
      const sameRun = visible && runStart >= 0 && color === runColor;
      if (sameRun) continue;

      if (runStart >= 0) {
        const spanPx = px - runStart;
        const rx = Math.round(x + runStart * sx);
        const ry = Math.round(y + py * sy);
        const rw = Math.max(1, Math.round(spanPx * sx));
        const rh = Math.max(1, Math.round(sy));
        out.push(rx, ry, rw, rh, depth + 1, -1, runColor >>> 0);
      }

      if (visible) {
        runStart = px;
        runColor = color;
      } else {
        runStart = -1;
        runColor = 0;
      }
    }

    if (runStart >= 0) {
      const spanPx = img.w - runStart;
      const rx = Math.round(x + runStart * sx);
      const ry = Math.round(y + py * sy);
      const rw = Math.max(1, Math.round(spanPx * sx));
      const rh = Math.max(1, Math.round(sy));
      out.push(rx, ry, rw, rh, depth + 1, -1, runColor >>> 0);
    }
  }

  return out;
}
