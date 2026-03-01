import type { Container, Graphics } from 'pixi.js';
import { Rectangle } from 'pixi.js';
import { TEXT_BASELINE_NUDGE_Y } from '../text';
import { getOrCreateText } from '../pixiReuse';

export function applyYogaDefaultsIframe(yogaNode: any, node: { attrs?: Record<string, string> }, Yoga: any): void {
  const isRoot = String(node.attrs?.['data-root'] ?? '') === '1';

  // Treat iframe like a replaced element that also forms a nested layout root.
  yogaNode.setFlexDirection(Yoga.FLEX_DIRECTION_COLUMN);
  yogaNode.setAlignItems(Yoga.ALIGN_STRETCH);

  if (isRoot) {
    // Structural only: fill available space and don't add chrome padding.
    yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
    yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
    yogaNode.setPadding(Yoga.EDGE_TOP, 0);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);
    yogaNode.setAlignSelf(Yoga.ALIGN_STRETCH);
    yogaNode.setFlexGrow(1);
    yogaNode.setFlexShrink(1);
    yogaNode.setMinWidth(0);
    yogaNode.setMinHeight(0);
    return;
  }

  // Reserve a small header strip and inset the iframe content.
  yogaNode.setPadding(Yoga.EDGE_LEFT, 8);
  yogaNode.setPadding(Yoga.EDGE_RIGHT, 8);
  yogaNode.setPadding(Yoga.EDGE_BOTTOM, 8);
  yogaNode.setPadding(Yoga.EDGE_TOP, 34);

  // Default HTML-ish size.
  const wAttr = Number(node.attrs?.width ?? '0');
  const hAttr = Number(node.attrs?.height ?? '0');
  const hasW = Number.isFinite(wAttr) && wAttr > 0;
  const hasH = Number.isFinite(hAttr) && hAttr > 0;

  const w = hasW ? wAttr : 420;
  const h = hasH ? hAttr : 240;

  if (hasW || hasH) {
    yogaNode.setAlignSelf(Yoga.ALIGN_FLEX_START);
    yogaNode.setFlexGrow(0);
    yogaNode.setFlexShrink(0);
  }

  yogaNode.setWidth(w);
  yogaNode.setHeight(h);
  yogaNode.setMinWidth(Math.min(200, w));
  yogaNode.setMinHeight(Math.min(160, h));
}

export function renderIframePlaceholder(opts: {
  node: { attrs?: Record<string, string> };
  container: Container;
  graphics: Graphics;
  w: number;
  h: number;
  theme: any;
}): void {
  const { node, container, graphics: g, w, h, theme } = opts;

  const isRoot = String(node.attrs?.['data-root'] ?? '') === '1';
  if (isRoot) return;

  const sw = 1;
  const inset = sw / 2;
  g.rect(inset, inset, Math.max(0, w - sw), Math.max(0, h - sw));
  g.fill(0xffffff);
  g.stroke({ width: sw, color: theme.control.border, alignment: 0 });

  // Light header strip.
  g.rect(inset, inset, Math.max(0, w - sw), 26);
  g.fill({ color: 0x000000, alpha: 0.04 });

  const srcdoc = String(node.attrs?.srcdoc ?? '');
  const hint = srcdoc.trim().length > 0 ? 'srcdoc' : 'empty';
  const title = getOrCreateText(container, '__title', (t) => {
    (t as any).style = {
      fontFamily: theme.fontFamily,
      fontSize: Math.max(10, Math.floor(theme.fontSize * 0.85)),
      fill: theme.mutedText,
      fontWeight: '400',
      wordWrap: false,
    };
  });
  title.text = `iframe (${hint})`;
  title.position.set(8, 6 + TEXT_BASELINE_NUDGE_Y);

  // Keep hit area so nested event routing can hook here later.
  container.eventMode = 'static';
  container.cursor = 'default';
  container.hitArea = new Rectangle(0, 0, Math.max(0, w), Math.max(0, h));
}
