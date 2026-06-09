import { Graphics } from 'pixi.js';
import type { Container } from 'pixi.js';
import { clearGraphics, getOrCreateGraphics } from '../pixiReuse';

declare const __TRUEOS_CAPTURE_BUILD__: boolean;

type TrueosSvgTexture = {
  state: 'loading' | 'ready' | 'error';
  texId: number;
  width: number;
  height: number;
};

const trueosSvgTextureCache = new Map<string, TrueosSvgTexture>();

function isTrueosCaptureOnly(): boolean {
  const host = globalThis as any;
  return Boolean(__TRUEOS_CAPTURE_BUILD__ || host.__TRUEOS_CAPTURE_ONLY__ || host.window?.__TRUEOS_CAPTURE_ONLY__);
}

function trueosSharedReadyImageTexture(src: string): any {
  const host = globalThis as any;
  const key = String(src ?? '').trim();
  if (!key) return null;
  let cache = host.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__;
  if (!cache || typeof cache.get !== 'function' || typeof cache.set !== 'function') {
    cache = new Map<string, any>();
    host.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = cache;
  }
  const cached = cache.get(key);
  if (cached) return cached;
  const resolve = host.__trueosResolveReadyImageTexture;
  if (typeof resolve !== 'function') return null;
  const result = resolve(key);
  if (result && typeof result.then === 'function') {
    const task = result.then((ready: any) => {
      cache.set(key, ready);
      return ready;
    }).catch((err: any) => {
      cache.delete(key);
      throw err;
    });
    cache.set(key, task);
    return task;
  }
  cache.set(key, result);
  return result;
}

function trueosPeekReadyImageTexture(src: string): any {
  const host = globalThis as any;
  const key = String(src ?? '').trim();
  if (!key) return null;
  const peek = host.__trueosPeekReadyImageTexture;
  if (typeof peek !== 'function') return null;
  const result = peek(key);
  if (!result || typeof result.then === 'function') return null;
  const texId = Math.max(0, Number(result?.texId ?? 0) | 0);
  return texId > 0 ? result : null;
}

function trueosRememberSharedReadyImageTexture(src: string, result: any): void {
  const host = globalThis as any;
  const key = String(src ?? '').trim();
  if (!key || !result) return;
  let cache = host.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__;
  if (!cache || typeof cache.get !== 'function' || typeof cache.set !== 'function') {
    cache = new Map<string, any>();
    host.__TRUEOS_READY_IMAGE_TEXTURE_CACHE__ = cache;
  }
  cache.set(key, result);
}

function trueosApplyReadySvgTexture(entry: TrueosSvgTexture, result: any): void {
  const texId = Math.max(0, Number(result?.texId ?? 0) | 0);
  if (texId <= 0) throw new Error('svg texture missing');
  entry.state = 'ready';
  entry.texId = texId;
  entry.width = Math.max(0, Number(result?.pixelWidth ?? result?.width ?? 0) | 0);
  entry.height = Math.max(0, Number(result?.pixelHeight ?? result?.height ?? 0) | 0);
}

function trueosAdoptReadySvgFromShared(url: string, entry: TrueosSvgTexture): boolean {
  const result = trueosPeekReadyImageTexture(url) || trueosSharedReadyImageTexture(url);
  if (!result || typeof result.then === 'function') return false;
  trueosApplyReadySvgTexture(entry, result);
  trueosRememberSharedReadyImageTexture(url, result);
  return true;
}

function stripUnsupportedSvgText(svg: string): string {
  // Pixi's SVG parser warns that <text> is unsupported.
  return stripTagBlock(stripTagBlock(String(svg), 'tspan'), 'text');
}

function stripTagBlock(source: string, tag: string): string {
  let out = '';
  let i = 0;
  const lower = source.toLowerCase();
  const openNeedle = '<' + tag;
  const closeNeedle = '</' + tag;
  while (i < source.length) {
    const open = lower.indexOf(openNeedle, i);
    if (open < 0) {
      out += source.slice(i);
      break;
    }
    out += source.slice(i, open);
    const close = lower.indexOf(closeNeedle, open + openNeedle.length);
    if (close < 0) break;
    const end = source.indexOf('>', close + closeNeedle.length);
    i = end < 0 ? source.length : end + 1;
  }
  return out;
}

function parseViewBox(svg: string): { minX: number; minY: number; w: number; h: number } | null {
  const s = String(svg);
  const lower = s.toLowerCase();
  const at = lower.indexOf('viewbox');
  if (at < 0) return null;
  const eq = s.indexOf('=', at + 7);
  if (eq < 0) return null;
  let i = eq + 1;
  while (i < s.length) {
    const c = s.charCodeAt(i);
    if (c !== 32 && c !== 9 && c !== 10 && c !== 13 && c !== 12) break;
    i += 1;
  }
  const quote = s.charAt(i);
  if (quote !== '"' && quote !== "'") return null;
  const end = s.indexOf(quote, i + 1);
  if (end < 0) return null;
  const parts = splitWs(s.slice(i + 1, end));
  if (parts.length < 4) return null;
  const minX = Number(parts[0]);
  const minY = Number(parts[1]);
  const w = Number(parts[2]);
  const h = Number(parts[3]);
  if (![minX, minY, w, h].every((n) => Number.isFinite(n)) || w <= 0 || h <= 0) return null;
  return { minX, minY, w, h };
}

function splitWs(source: string): string[] {
  const out: string[] = [];
  let part = '';
  for (let i = 0; i < source.length; i += 1) {
    const c = source.charCodeAt(i);
    const ws = c === 32 || c === 9 || c === 10 || c === 13 || c === 12;
    if (ws) {
      if (part.length > 0) {
        out.push(part);
        part = '';
      }
    } else {
      part += source.charAt(i);
    }
  }
  if (part.length > 0) out.push(part);
  return out;
}

function trueosReadySvgTexture(svg: string, requestRerender: (() => void) | null): TrueosSvgTexture | null {
  const key = String(svg ?? '');
  if (!key.trim()) return null;
  const cached = trueosSvgTextureCache.get(key);
  const url = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(key)}`;
  if (cached) {
    if (isTrueosCaptureOnly() && cached.state === 'loading') {
      try { trueosAdoptReadySvgFromShared(url, cached); } catch { cached.state = 'error'; }
    }
    return cached;
  }

  if (isTrueosCaptureOnly()) {
    return null;
  }

  const entry: TrueosSvgTexture = { state: 'loading', texId: 0, width: 0, height: 0 };
  trueosSvgTextureCache.set(key, entry);
  const applyReady = (result: any) => {
      trueosApplyReadySvgTexture(entry, result);
      if (!isTrueosCaptureOnly()) requestRerender?.();
  };
  const applyError = () => {
      entry.state = 'error';
      if (!isTrueosCaptureOnly()) requestRerender?.();
  };
  try {
    const result = trueosSharedReadyImageTexture(url);
    if (!result) return entry;
    if (result && typeof result.then === 'function') {
      if (isTrueosCaptureOnly()) return entry;
      result.then(applyReady).catch(applyError);
    } else {
      applyReady(result);
    }
  } catch {
    applyError();
  }
  return entry;
}

function fittedViewBoxRect(svg: string, w: number, h: number): { x: number; y: number; w: number; h: number } {
  const boxW = Math.max(0, w);
  const boxH = Math.max(0, h);
  const vb = parseViewBox(svg);
  if (!vb || boxW <= 0 || boxH <= 0) return { x: 0, y: 0, w: boxW, h: boxH };

  const sx = boxW / vb.w;
  const sy = boxH / vb.h;
  const s = Math.min(sx, sy);
  const drawW = Math.max(0, vb.w * s);
  const drawH = Math.max(0, vb.h * s);
  return {
    x: Math.max(0, (boxW - drawW) / 2),
    y: Math.max(0, (boxH - drawH) / 2),
    w: drawW,
    h: drawH,
  };
}

export function applyYogaDefaultsSvg(yogaNode: any, node: { attrs?: Record<string, string> }, Yoga: any): void {
  yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
  yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
  yogaNode.setPadding(Yoga.EDGE_TOP, 0);
  yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);

  // SVG behaves like a replaced element; default size similar to <canvas>.
  const wAttr = Number(node.attrs?.width ?? '0');
  const hAttr = Number(node.attrs?.height ?? '0');
  const hasW = Number.isFinite(wAttr) && wAttr > 0;
  const hasH = Number.isFinite(hAttr) && hAttr > 0;

  const w = hasW ? wAttr : 300;
  const h = hasH ? hAttr : 150;

  if (hasW || hasH) {
    yogaNode.setAlignSelf(Yoga.ALIGN_FLEX_START);
    yogaNode.setFlexGrow(0);
    yogaNode.setFlexShrink(0);
  }

  yogaNode.setWidth(w);
  yogaNode.setHeight(h);
  yogaNode.setMinWidth(Math.min(120, w));
  yogaNode.setMinHeight(Math.min(80, h));
}

export function renderSvgElement(opts: {
  svgMarkup: string;
  container: Container;
  w: number;
  h: number;
  requestRerender: (() => void) | null;
}): void {
  const { svgMarkup, container, w, h, requestRerender } = opts;

  const svgString = stripUnsupportedSvgText(svgMarkup);

  const svgG = getOrCreateGraphics(container, '__svg');
  // Re-render only when the markup changes.
  const prev = (svgG as any).__svgString as string | undefined;
  const prevW = (svgG as any).__w as number | undefined;
  const prevH = (svgG as any).__h as number | undefined;
  const needsRebuild = prev !== svgString;
  const readyTexture = trueosReadySvgTexture(svgString, requestRerender);

  svgG.scale.set(1);
  svgG.position.set(0, 0);
  if (readyTexture?.state === 'ready' && readyTexture.texId > 0 && typeof (svgG as any).image === 'function') {
    if (needsRebuild || prevW !== w || prevH !== h || (svgG as any).__texId !== readyTexture.texId) {
      const fit = fittedViewBoxRect(svgString, w, h);
      clearGraphics(svgG);
      (svgG as any).image(readyTexture.texId, fit.x, fit.y, fit.w, fit.h);
      (svgG as any).__svgString = svgString;
      (svgG as any).__w = w;
      (svgG as any).__h = h;
      (svgG as any).__texId = readyTexture.texId;
    }
    return;
  }

  if (__TRUEOS_CAPTURE_BUILD__) {
    clearGraphics(svgG);
    return;
  }

  const svgFn = (svgG as any).svg;
  if (typeof svgFn === 'function') {
    if (needsRebuild || prevW !== w || prevH !== h) {
      clearGraphics(svgG);
      let res: any;
      try {
        res = svgFn.call(svgG, svgString);
      } catch {
        res = null;
      }

      if (res && typeof res.then === 'function') {
        res.then(() => requestRerender?.()).catch(() => void 0);
      }
      (svgG as any).__svgString = svgString;
      (svgG as any).__w = w;
      (svgG as any).__h = h;
    }

    // Fit viewBox into the Yoga box (default preserveAspectRatio: xMidYMid meet).
    const vb = parseViewBox(svgString);
    if (vb) {
      const sx = w / vb.w;
      const sy = h / vb.h;
      const s = Math.min(sx, sy);
      const drawW = vb.w * s;
      const drawH = vb.h * s;
      svgG.scale.set(s);
      svgG.position.set(-vb.minX * s + (w - drawW) / 2, -vb.minY * s + (h - drawH) / 2);
    }

    return;
  }

  // Fallback: nothing.
}
