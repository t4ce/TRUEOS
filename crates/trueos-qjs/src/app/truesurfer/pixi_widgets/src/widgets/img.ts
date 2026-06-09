import { Graphics } from 'pixi.js';
import type { Container } from 'pixi.js';
import { TEXT_BASELINE_NUDGE_Y, WRAP_EPSILON_PX } from '../text';
import { makeImgPlaceholderSvg, makeNeonOrbSvg } from '../svgs';
import { clearGraphics, getOrCreateGraphics, getOrCreateText } from '../pixiReuse';

declare const __TRUEOS_CAPTURE_BUILD__: boolean;

type TrueosImageCacheEntry = {
  state: 'loading' | 'ready' | 'error';
  texId: number;
  width: number;
  height: number;
};

const trueosImageCache = new Map<string, TrueosImageCacheEntry>();

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

function trueosApplyReadyImageEntry(entry: TrueosImageCacheEntry, result: any): void {
  const texId = Math.max(0, Number(result?.texId ?? 0) | 0);
  if (texId <= 0) throw new Error('image texture missing');
  entry.state = 'ready';
  entry.texId = texId;
  entry.width = Math.max(0, Number(result?.pixelWidth ?? result?.width ?? 0) | 0);
  entry.height = Math.max(0, Number(result?.pixelHeight ?? result?.height ?? 0) | 0);
}

function trueosAdoptReadyImageFromShared(key: string, entry: TrueosImageCacheEntry): boolean {
  const result = trueosPeekReadyImageTexture(key) || trueosSharedReadyImageTexture(key);
  if (!result || typeof result.then === 'function') return false;
  trueosApplyReadyImageEntry(entry, result);
  trueosRememberSharedReadyImageTexture(key, result);
  return true;
}

function trueosReadyImage(src: string, requestRerender: (() => void) | null): TrueosImageCacheEntry | null {
  const key = String(src ?? '').trim();
  if (!key) return null;
  const cached = trueosImageCache.get(key);
  if (cached) {
    if (isTrueosCaptureOnly() && cached.state === 'loading') {
      try { trueosAdoptReadyImageFromShared(key, cached); } catch { cached.state = 'error'; }
    }
    return cached;
  }

  if (isTrueosCaptureOnly()) {
    return null;
  }

  const entry: TrueosImageCacheEntry = { state: 'loading', texId: 0, width: 0, height: 0 };
  trueosImageCache.set(key, entry);
  const applyReady = (result: any) => {
      trueosApplyReadyImageEntry(entry, result);
      if (!isTrueosCaptureOnly()) requestRerender?.();
  };
  const applyError = () => {
      entry.state = 'error';
      if (!isTrueosCaptureOnly()) requestRerender?.();
  };
  try {
    const result = trueosSharedReadyImageTexture(key);
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

function decodeSvgDataUri(src: string): string | null {
  const s = String(src ?? '');
  if (!s.startsWith('data:image/svg+xml')) return null;

  // data:image/svg+xml,<svg...>
  // data:image/svg+xml;charset=utf-8,<svg...>
  const commaIdx = s.indexOf(',');
  if (commaIdx === -1) return null;
  const meta = s.slice(0, commaIdx).toLowerCase();
  const payload = s.slice(commaIdx + 1);

  try {
    if (meta.includes(';base64')) {
      return atob(payload);
    }
    return decodeURIComponent(payload);
  } catch {
    return null;
  }
}

function stripUnsupportedSvgText(svg: string): string {
  return stripTagBlock(stripTagBlock(String(svg), 'tspan'), 'text');
}

function svgDataUrl(svg: string): string {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
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

export function renderImg(opts: {
  node: { attrs?: Record<string, string> };
  container: Container;
  graphics: Graphics;
  w: number;
  h: number;
  theme: any;
  requestRerender: (() => void) | null;
}): void {
  const { node, container, graphics: g, w, h, theme, requestRerender } = opts;

  const alt = node.attrs?.alt ?? '';
  const src = node.attrs?.src ?? '';
  const hasSrc = src.trim().length > 0;
  const label = alt.trim().length > 0 ? alt : src.trim().length > 0 ? src : 'img';
  const imageFn = (g as any).image;
  const readyImage = hasSrc ? trueosReadyImage(src, requestRerender) : null;

  if (readyImage?.state === 'ready' && readyImage.texId > 0 && typeof imageFn === 'function') {
    imageFn.call(g, readyImage.texId, 0, 0, Math.max(0, w), Math.max(0, h));
    const t = (container as any).getChildByLabel
      ? (container as any).getChildByLabel('__label')
      : container.children.find((c: any) => c?.label === '__label');
    if (t) t.visible = false;
    return;
  }

  const svgFromSrc = hasSrc ? decodeSvgDataUri(src) : null;
  const svgString = stripUnsupportedSvgText(
    svgFromSrc ?? (hasSrc ? makeImgPlaceholderSvg(w, h) : makeNeonOrbSvg({ ring: 34, core: 14 }))
  );

  const svgG = getOrCreateGraphics(container, '__svg');
  const readySvg = trueosReadyImage(svgDataUrl(svgString), requestRerender);
  if (readySvg?.state === 'ready' && readySvg.texId > 0 && typeof (svgG as any).image === 'function') {
    const key = `texture:${readySvg.texId}:${Math.round(w)}x${Math.round(h)}`;
    if ((svgG as any).__key !== key) {
      clearGraphics(svgG);
      (svgG as any).image(readySvg.texId, 0, 0, Math.max(0, w), Math.max(0, h));
      (svgG as any).__key = key;
    }
    svgG.scale.set(1);
    svgG.position.set(0, 0);
    if (!hasSrc) {
      const t = (container as any).getChildByLabel
        ? (container as any).getChildByLabel('__label')
        : container.children.find((c: any) => c?.label === '__label');
      if (t) t.visible = false;
      return;
    }
    if (label.trim().length > 0) {
      const t = getOrCreateText(container, '__label', (tt) => {
        (tt as any).style = {
          fontFamily: theme.fontFamily,
          fontSize: theme.fontSize,
          fill: theme.mutedText,
          fontWeight: '400',
          wordWrap: true,
          wordWrapWidth: 0,
        };
      });
      t.text = label;
      (t.style as any).wordWrapWidth = Math.max(0, Math.ceil(w - 16) + WRAP_EPSILON_PX);
      t.position.set(8, 8 + TEXT_BASELINE_NUDGE_Y);
      t.visible = true;
    }
    return;
  } else if (__TRUEOS_CAPTURE_BUILD__) {
    // TRUEOS consumes SVG as decoded texture assets. Avoid recording Pixi's
    // browser-only Graphics.svg fallback as an unsupported scene op while the
    // async texture request is still pending.
    clearGraphics(svgG);
  }

  const svgFn = (svgG as any).svg;
  if (!__TRUEOS_CAPTURE_BUILD__ && typeof svgFn === 'function') {
    const key = `${hasSrc ? 'src' : 'nosrc'}:${Math.round(w)}x${Math.round(h)}:${svgString.length}`;
    if ((svgG as any).__key !== key) {
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
      (svgG as any).__key = key;
    }

    if (!hasSrc) {
      svgG.scale.set(w / 100, h / 100);
    }

    if (hasSrc && label.trim().length > 0) {
      const t = getOrCreateText(container, '__label', (tt) => {
        (tt as any).style = {
          fontFamily: theme.fontFamily,
          fontSize: theme.fontSize,
          fill: theme.mutedText,
          fontWeight: '400',
          wordWrap: true,
          wordWrapWidth: 0,
        };
      });
      t.text = label;
      // Wrap width depends on the Yoga box.
      (t.style as any).wordWrapWidth = Math.max(0, Math.ceil(w - 16) + WRAP_EPSILON_PX);
      t.position.set(8, 8 + TEXT_BASELINE_NUDGE_Y);
      t.visible = true;
    } else {
      const t = (container as any).getChildByLabel
        ? (container as any).getChildByLabel('__label')
        : container.children.find((c: any) => c?.label === '__label');
      if (t) t.visible = false;
    }
    return;
  }

  // Fallback: placeholder rect + label.
  {
    const sw = 1;
    const inset = sw / 2;
    g.rect(inset, inset, Math.max(0, w - sw), Math.max(0, h - sw));
    g.fill(0xf6f6f6);
    g.stroke({ width: sw, color: theme.control.border });
  }

  const t = getOrCreateText(container, '__label', (tt) => {
    (tt as any).style = {
      fontFamily: theme.fontFamily,
      fontSize: theme.fontSize,
      fill: theme.mutedText,
      fontWeight: '400',
      wordWrap: true,
      wordWrapWidth: 0,
    };
  });
  t.text = label;
  (t.style as any).wordWrapWidth = Math.max(0, Math.ceil(w - 16) + WRAP_EPSILON_PX);
  t.position.set(8, 8 + TEXT_BASELINE_NUDGE_Y);
}

export function applyYogaDefaultsImg(yogaNode: any, node: { attrs?: Record<string, string> }, Yoga: any): void {
  yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
  yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
  yogaNode.setPadding(Yoga.EDGE_TOP, 0);
  yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);

  const wAttr = Number(node.attrs?.width ?? '0');
  const hAttr = Number(node.attrs?.height ?? '0');
  const hasW = Number.isFinite(wAttr) && wAttr > 0;
  const hasH = Number.isFinite(hAttr) && hAttr > 0;

  const w = hasW ? wAttr : 240;
  const h = hasH ? hAttr : 140;

  if (hasW || hasH) {
    yogaNode.setAlignSelf(Yoga.ALIGN_FLEX_START);
    yogaNode.setFlexGrow(0);
    yogaNode.setFlexShrink(0);
  }

  yogaNode.setWidth(w);
  yogaNode.setHeight(h);
  yogaNode.setMinWidth(120);
  yogaNode.setMinHeight(80);
}
