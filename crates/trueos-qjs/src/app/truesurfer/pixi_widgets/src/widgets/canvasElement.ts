import type { Container, Graphics } from 'pixi.js';
import { makeThemedText, TEXT_BASELINE_NUDGE_Y } from '../text';

export function applyYogaDefaultsCanvas(yogaNode: any, node: { attrs?: Record<string, string> }, Yoga: any): void {
  yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
  yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
  yogaNode.setPadding(Yoga.EDGE_TOP, 0);
  yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);

  // HTML default canvas size.
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

export function renderCanvasElement(opts: {
  node: { attrs?: Record<string, string> };
  container: Container;
  graphics: Graphics;
  w: number;
  h: number;
  theme: any;
}): void {
  const { graphics: g, container, w, h, theme } = opts;

  // Placeholder for now (we don't implement the imperative CanvasRenderingContext2D API yet).
  const sw = 1;
  const inset = sw / 2;
  g.rect(inset, inset, Math.max(0, w - sw), Math.max(0, h - sw));
  g.fill(0xffffff);
  g.stroke({ width: sw, color: theme.control.border, alignment: 0 });

  // Subtle diagonal to hint it's a canvas.
  g.moveTo(6, h - 6);
  g.lineTo(w - 6, 6);
  g.stroke({ width: 1, color: 0x000000, alpha: 0.1 });

  const t = makeThemedText({
    text: 'canvas',
    fontFamily: theme.fontFamily,
    fontSize: Math.max(10, Math.floor(theme.fontSize * 0.85)),
    fill: theme.mutedText,
    wordWrap: false,
  });
  t.position.set(8, 8 + TEXT_BASELINE_NUDGE_Y);
  container.addChild(t);
}
