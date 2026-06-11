import type { Container } from 'pixi.js';
import { Rectangle } from 'pixi.js';
import { TEXT_BASELINE_NUDGE_Y } from '../text';
import { clearContainerEvents, clearGraphics, getOrCreateText } from '../pixiReuse';

const armedSummaryByPointer = new Map<number, string>();

function defaultOpenFromSummaryAttr(attrs?: Record<string, string>): boolean {
  if (!attrs || !Object.prototype.hasOwnProperty.call(attrs, 'data-details-open')) return false;
  const raw = String(attrs['data-details-open'] ?? '').trim().toLowerCase();
  return raw !== '0' && raw !== 'false' && raw !== 'no';
}

function owningDetailsKey(node: { key?: string; attrs?: Record<string, string> }): string | undefined {
  const summaryKey = node.key;
  if (summaryKey && summaryKey.endsWith(':summary')) return `${summaryKey.slice(0, -':summary'.length)}:details`;
  return node.attrs?.['data-details-key'];
}

export function renderSummary(opts: {
  node: { key?: string; attrs?: Record<string, string> };
  container: Container;
  w: number;
  h: number;
  theme: { text: number; fontFamily: string; fontSize: number };
  detailsOpen: Map<string, boolean>;
  requestRerender: (() => void) | null;
}): void {
  const { node, container, w, h, theme, detailsOpen, requestRerender } = opts;

  const detailsKey = owningDetailsKey(node);
  const defaultOpen = defaultOpenFromSummaryAttr(node.attrs);
  const isOpen = detailsKey && detailsOpen.has(detailsKey) ? detailsOpen.get(detailsKey) === true : defaultOpen;

  const pointerIdForEvent = (ev: any): number => Number(ev?.pointerId ?? ev?.data?.pointerId ?? 1) || 1;

  const toggle = (ev?: any) => {
    if (!detailsKey) return;
    if (ev?.button === 2) return;
    const current = detailsOpen.has(detailsKey) ? detailsOpen.get(detailsKey) === true : defaultOpen;
    const next = !current;
    detailsOpen.set(detailsKey, next);
    requestRerender?.();
    ev?.stopPropagation?.();
  };

  const arrowSize = 16;
  const oldArrowG = (container as any).children?.find((child: any) => child?.label === '__arrow');
  if (oldArrowG) {
    clearGraphics(oldArrowG);
    oldArrowG.visible = false;
  }

  const arrowT = getOrCreateText(container, '__arrowText', (tt) => {
    (tt as any).style = {
      fontFamily: theme.fontFamily,
      fontSize: theme.fontSize,
      fill: theme.text,
      fontWeight: '700',
    };
  });
  arrowT.visible = true;
  arrowT.text = isOpen ? 'v' : '>';
  (arrowT.style as any).fontFamily = theme.fontFamily;
  (arrowT.style as any).fontSize = theme.fontSize;
  (arrowT.style as any).fill = theme.text;
  (arrowT.style as any).fontWeight = '700';
  arrowT.position.set(5, Math.max(0, (h - theme.fontSize) / 2) + TEXT_BASELINE_NUDGE_Y);

  // Toggle the owning <details>.
  if (detailsKey) {
    clearContainerEvents(container);
    container.eventMode = 'static';
    container.cursor = 'pointer';
    container.hitArea = new Rectangle(0, 0, Math.max(0, w), Math.max(0, h));
    container.on('pointerdown', (ev: any) => {
      if (ev?.button === 2) return;
      armedSummaryByPointer.set(pointerIdForEvent(ev), detailsKey);
      ev.stopPropagation?.();
    });
    container.on('pointerup', (ev: any) => {
      if (ev?.button === 2) return;
      const pid = pointerIdForEvent(ev);
      const armed = armedSummaryByPointer.get(pid);
      armedSummaryByPointer.delete(pid);
      if (armed !== detailsKey) return;
      toggle(ev);
    });
    container.on('pointerupoutside', (ev: any) => {
      const pid = pointerIdForEvent(ev);
      if (armedSummaryByPointer.get(pid) === detailsKey) armedSummaryByPointer.delete(pid);
    });
  }
}

export function applyYogaDefaultsSummary(yogaNode: any, Yoga: any): void {
  // A summary is a single row; reserve space for the arrow (16px) + gap.
  yogaNode.setFlexDirection(Yoga.FLEX_DIRECTION_ROW);
  yogaNode.setAlignItems(Yoga.ALIGN_CENTER);
  // Put trailing controls (like checkboxes) all the way on the right.
  yogaNode.setJustifyContent(Yoga.JUSTIFY_SPACE_BETWEEN);
  yogaNode.setPadding(Yoga.EDGE_TOP, 6);
  yogaNode.setPadding(Yoga.EDGE_BOTTOM, 6);
  yogaNode.setPadding(Yoga.EDGE_LEFT, 26);
  yogaNode.setPadding(Yoga.EDGE_RIGHT, 12);
  yogaNode.setMinHeight(36);
}

export function applyYogaDefaultsDetails(yogaNode: any, Yoga: any): void {
  yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
  yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
  yogaNode.setPadding(Yoga.EDGE_TOP, 0);
  yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);
}

export function getEffectiveDetailsChildren(node: any, detailsOpen: Map<string, boolean>): any[] {
  // Collapse <details> content unless open.
  if (!node || node.tagName !== 'details' || !node.key) return node?.children ?? [];
  const attrOpen = node.attrs ? Object.prototype.hasOwnProperty.call(node.attrs, 'open') : false;
  const open = detailsOpen.has(node.key) ? detailsOpen.get(node.key) === true : attrOpen;
  if (open) return node.children ?? [];
  return (node.children ?? []).filter((c: any) => c && c.kind === 'block' && c.tagName === 'summary');
}
