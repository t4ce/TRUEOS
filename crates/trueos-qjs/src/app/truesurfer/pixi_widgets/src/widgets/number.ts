import type { Container } from 'pixi.js';
import { Graphics, Rectangle } from 'pixi.js';
import { makeThemedText, TEXT_BASELINE_NUDGE_Y } from '../text';
import { clearGraphics, getOrCreateGraphics, getOrCreateText } from '../pixiReuse';

export type NumberState = {
  value: number;
};

export function getOrInitNumberState(map: Map<string, NumberState>, key: string, attrs?: Record<string, string>): NumberState {
  const existing = map.get(key);
  if (existing) return existing;

  const vAttr = Number(attrs?.value ?? '0');
  const v = Number.isFinite(vAttr) ? vAttr : 0;
  const st: NumberState = { value: v };
  map.set(key, st);
  return st;
}

export function applyYogaDefaultsNumber(yogaNode: any, Yoga: any): void {
  yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
  yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
  yogaNode.setPadding(Yoga.EDGE_TOP, 0);
  yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);
  yogaNode.setHeight(36);
  yogaNode.setMinHeight(36);
  yogaNode.setMinWidth(140);
  yogaNode.setFlexGrow(0);
  yogaNode.setFlexShrink(0);
}

function clamp(n: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, n));
}

function parseNumberAttr(attrs: Record<string, string> | undefined, name: string, fallback: number): number {
  const v = Number(attrs?.[name] ?? '');
  return Number.isFinite(v) ? v : fallback;
}

function drawChevronUp(g: Graphics, x: number, y: number, w: number, h: number, color: number): void {
  const pad = 3;
  const x0 = x + pad;
  const x1 = x + w - pad;
  const y0 = y + pad;
  const y1 = y + h - pad;
  g.moveTo(x0, y1);
  g.lineTo((x0 + x1) / 2, y0);
  g.lineTo(x1, y1);
  g.stroke({ width: 2, color });
}

function drawChevronDown(g: Graphics, x: number, y: number, w: number, h: number, color: number): void {
  const pad = 3;
  const x0 = x + pad;
  const x1 = x + w - pad;
  const y0 = y + pad;
  const y1 = y + h - pad;
  g.moveTo(x0, y0);
  g.lineTo((x0 + x1) / 2, y1);
  g.lineTo(x1, y0);
  g.stroke({ width: 2, color });
}

export function renderNumberSpinner(opts: {
  node: { key?: string; attrs?: Record<string, string> };
  container: Container;
  graphics: Graphics;
  w: number;
  h: number;
  theme: any;

  getValue: () => number;
  setValue: (n: number) => void;

  requestPaint: (() => void) | null;

  // Press-and-hold repeat (per pointer).
  numberHolds?: Map<number, { key: string; timeoutId: number | null; intervalId: number | null }>;
  getPointerId?: (ev: any) => number;
}): void {
  const { node, container, graphics: g, w, h, theme, getValue, setValue, requestPaint } = opts;

  const key = node.key;
  const attrs = node.attrs;

  const min = parseNumberAttr(attrs, 'min', 0);
  const max = parseNumberAttr(attrs, 'max', 255);
  const step = Math.max(1e-9, parseNumberAttr(attrs, 'step', 1));

  const value = getValue();

  const sw = 1;
  const inset = sw / 2;
  g.rect(inset, inset, Math.max(0, w - sw), Math.max(0, h - sw));
  g.fill(theme.control.background);
  g.stroke({ width: sw, color: theme.control.border });

  const arrowW = 22;
  const sepX = Math.max(0, w - arrowW);

  // Separator
  g.moveTo(sepX + 0.5, 0);
  g.lineTo(sepX + 0.5, h);
  g.stroke({ width: 1, color: theme.control.border, alignment: 0 });

  // Arrows
  const arrows = getOrCreateGraphics(container, '__arrows');
  clearGraphics(arrows);
  drawChevronUp(arrows, sepX, 0, arrowW, h / 2, theme.text);
  drawChevronDown(arrows, sepX, h / 2, arrowW, h / 2, theme.text);

  const channel = (attrs?.channel ?? '').toLowerCase();
  const label = channel === 'r' ? 'R' : channel === 'g' ? 'G' : channel === 'b' ? 'B' : channel === 'a' ? 'A' : '';

  const valueText = getOrCreateText(container, '__valueText', (t) => {
    (t as any).style = {
      fontFamily: theme.fontFamily,
      fontSize: theme.fontSize,
      fill: theme.text,
      fontWeight: '400',
      wordWrap: false,
    };
  });
  valueText.text = label ? `${label}: ${Math.round(value)}` : String(Math.round(value));
  valueText.position.set(8, 9 + TEXT_BASELINE_NUDGE_Y);

  // Interactivity: click top/bottom half for +/-
  if (!key) return;

  const hitUp = new Rectangle(sepX, 0, arrowW, h / 2);
  const hitDown = new Rectangle(sepX, h / 2, arrowW, h / 2);

  const inc = (dir: -1 | 1) => {
    const cur = getValue();
    const next = clamp(cur + dir * step, min, max);
    setValue(next);
    requestPaint?.();
  };

  const hit = getOrCreateGraphics(container, '__hit');
  clearGraphics(hit);
  hit.eventMode = 'static';
  hit.cursor = 'default';
  hit.hitArea = new Rectangle(0, 0, Math.max(0, w), Math.max(0, h));
  hit.on('pointerdown', (ev: any) => {
    if (ev?.button === 2) return;

    const pid = opts.getPointerId ? opts.getPointerId(ev) : Number(ev?.pointerId ?? ev?.data?.pointerId ?? 0);
    if (pid <= 0) return;

    const p = container.toLocal(ev.global);
    const lx = p?.x ?? 0;
    const ly = p?.y ?? 0;

    const dir: -1 | 1 | null = hitUp.contains(lx, ly) ? 1 : hitDown.contains(lx, ly) ? -1 : null;
    if (!dir) return;

    // Immediate step.
    inc(dir);

    // Press-and-hold: after 500ms, repeat every 250ms.
    // IMPORTANT: only one cursor can "own" spinner interaction at a time.
    // If another cursor clicks any spinner, the previous cursor's hold is cleared.
    const holds = opts.numberHolds;
    if (holds && key) {
      // Global "last cursor wins": cancel all other pointers' active holds.
      for (const [otherPid, h] of holds.entries()) {
        if (otherPid === pid) continue;
        if (h.timeoutId != null) window.clearTimeout(h.timeoutId);
        if (h.intervalId != null) window.clearInterval(h.intervalId);
        holds.delete(otherPid);
      }

      // Replace existing hold for this pointer.
      const prev = holds.get(pid);
      if (prev) {
        if (prev.timeoutId != null) window.clearTimeout(prev.timeoutId);
        if (prev.intervalId != null) window.clearInterval(prev.intervalId);
      }

      const hold = { key, timeoutId: null as number | null, intervalId: null as number | null };
      hold.timeoutId = window.setTimeout(() => {
        hold.timeoutId = null;
        hold.intervalId = window.setInterval(() => {
          inc(dir);
        }, 250);
      }, 500);

      holds.set(pid, hold);
    }

    ev.stopPropagation?.();
  });
}
