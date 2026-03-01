import { Graphics } from 'pixi.js';
import type { Container } from 'pixi.js';
import { TEXT_BASELINE_NUDGE_Y, WRAP_EPSILON_PX } from '../text';
import { makeImgPlaceholderSvg, makeNeonOrbSvg } from '../svgs';
import { clearGraphics, getOrCreateGraphics, getOrCreateText } from '../pixiReuse';

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
  // Pixi's built-in SVG parser warns that <text> is unsupported.
  // Remove <text> and <tspan> nodes to avoid noisy console warnings.
  return String(svg)
    .replace(/<\s*tspan\b[^>]*>[\s\S]*?<\s*\/\s*tspan\s*>/gi, '')
    .replace(/<\s*text\b[^>]*>[\s\S]*?<\s*\/\s*text\s*>/gi, '');
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

  const svgFromSrc = hasSrc ? decodeSvgDataUri(src) : null;
  const svgString = stripUnsupportedSvgText(
    svgFromSrc ?? (hasSrc ? makeImgPlaceholderSvg(w, h) : makeNeonOrbSvg({ ring: 34, core: 14 }))
  );

  const svgG = getOrCreateGraphics(container, '__svg');
  const svgFn = (svgG as any).svg;
  if (typeof svgFn === 'function') {
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
