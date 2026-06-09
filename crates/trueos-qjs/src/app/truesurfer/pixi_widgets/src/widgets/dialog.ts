import type { Container } from 'pixi.js';
import { Graphics, Rectangle } from 'pixi.js';
import { clearGraphics, getOrCreateGraphics } from '../pixiReuse';

export type DialogState = {
  x: number;
  y: number;
};

export type DialogDrag = {
  key: string;
  startGX: number;
  startGY: number;
  originX: number;
  originY: number;
};

export function getOrInitDialogState(map: Map<string, DialogState>, key: string): DialogState {
  const existing = map.get(key);
  if (existing) return existing;

  // Default position is top-left of the parent scene.
  const state: DialogState = { x: 0, y: 0 };
  map.set(key, state);
  return state;
}

export function applyYogaDefaultsDialog(yogaNode: any, Yoga: any): void {
  // Floating: don't participate in normal document flow.
  yogaNode.setPositionType(Yoga.POSITION_TYPE_ABSOLUTE);
  yogaNode.setPosition(Yoga.EDGE_LEFT, 0);
  yogaNode.setPosition(Yoga.EDGE_TOP, 0);
  yogaNode.setAlignSelf(Yoga.ALIGN_FLEX_START);
  yogaNode.setFlexGrow(0);
  yogaNode.setFlexShrink(0);

  yogaNode.setPadding(Yoga.EDGE_LEFT, 12);
  yogaNode.setPadding(Yoga.EDGE_RIGHT, 12);
  yogaNode.setPadding(Yoga.EDGE_TOP, 12);
  yogaNode.setPadding(Yoga.EDGE_BOTTOM, 12);

  yogaNode.setWidth(540);
  yogaNode.setMinWidth(360);
  yogaNode.setMinHeight(148);
}

export function renderDialog(opts: {
  node: { key?: string };
  container: Container;
  w: number;
  h: number;
  theme: { boxBorder: number };

  // Used to paint selection in cursor color.
  selectedBy: Map<string, number>;
  getCursorColor: (pointerId: number) => number;

  // Drag state.
  dialogStates: Map<string, DialogState>;
  dialogDrags: Map<number, DialogDrag>;

  bringToFront?: (key: string) => void;

  // Optional pointerId override for test harnesses.
  getPointerId?: (ev: any) => number;

  requestPaint: (() => void) | null;
}): void {
  const { node, container, w, h, theme, selectedBy, getCursorColor, dialogStates, dialogDrags, bringToFront, requestPaint } = opts;
  const key = node.key;
  if (!key) return;

  // Border: 2px when selected (consistent with other widgets), 1px otherwise.
  const selectedPid = selectedBy.get(key);
  const borderColor = selectedPid == null ? theme.boxBorder : getCursorColor(selectedPid);

  const bw = Math.max(0, Math.round(w));
  const bh = Math.max(0, Math.round(h));

  const border = getOrCreateGraphics(container, '__dialogBorder');
  clearGraphics(border);
  // Solid background so content doesn't show through.
  border.rect(0, 0, bw, bh);
  border.fill({ color: 0xffffff, alpha: 0.8 });

  const sw = selectedPid == null ? 1 : 2;
  const inset = sw / 2;
  border.rect(inset, inset, Math.max(0, bw - sw), Math.max(0, bh - sw));

  border.stroke({ width: sw, color: borderColor, alignment: 0 });

  // Capture drag only on the background, not on children.
  border.eventMode = 'static';
  border.cursor = 'move';
  border.hitArea = new Rectangle(0, 0, bw, bh);

  border.on('pointerdown', (ev: any) => {
    const diag = (step: string) => {
      try {
        if (typeof console !== 'undefined' && typeof console.log === 'function') {
          console.log(`[dialog pointerdown] ${step}`);
        }
      } catch {
        // Diagnostics must never alter widget behavior.
      }
    };
    diag('start');
    if (ev?.button === 2) return;

    diag('pointer-id');
    const pid = opts.getPointerId ? opts.getPointerId(ev) : Number(ev?.pointerId ?? ev?.data?.pointerId ?? 0);
    if (pid <= 0) return;
    if (pid <= 0) return;

    // "Last cursor wins": if someone else was dragging this dialog, cancel their drag.
    diag('clear-other-drags');
    for (const [otherPid, d] of dialogDrags.entries()) {
      if (d.key === key && otherPid !== pid) dialogDrags.delete(otherPid);
    }

    // Mark selected by this cursor.
    diag('select');
    selectedBy.set(key, pid);
    diag('bring-to-front');
    bringToFront?.(key);

    diag('state');
    const st = getOrInitDialogState(dialogStates, key);
    diag('set-drag');
    dialogDrags.set(pid, {
      key,
      startGX: ev.global?.x ?? 0,
      startGY: ev.global?.y ?? 0,
      originX: st.x,
      originY: st.y,
    });

    diag('request-paint');
    requestPaint?.();
    diag('stop-propagation');
    ev.stopPropagation?.();
    diag('done');
  });

  // Retained-mode: ensure the dialog chrome stays behind the layout children.
  // In the retained renderer, `__children` holds nested content; if the border
  // sits above it, it will both gray out content (alpha fill) and steal input.
  {
    const byLabel = (container as any).getChildByLabel as ((l: string) => any) | undefined;
    const childrenRoot =
      byLabel?.call(container, '__children') ?? container.children.find((c: any) => c && (c as any).label === '__children') ?? null;
    if (childrenRoot && border.parent === container) {
      const idxChildren = container.getChildIndex(childrenRoot);
      const max = Math.max(0, container.children.length - 1);
      const target = Math.max(0, Math.min(idxChildren - 1, max));
      const cur = container.getChildIndex(border);
      if (cur > target) container.setChildIndex(border, target);
    }
  }
}
