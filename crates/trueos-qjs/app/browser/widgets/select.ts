import { Container, Graphics, Rectangle } from 'pixi.js';
import { makeThemedText, TEXT_BASELINE_NUDGE_Y } from '../text';
import { clearContainerEvents, getOrCreateText } from '../pixiReuse';

export type SelectState = {
  selectedIndex: number;
  open: boolean;
};

export function getOrInitSelectState(map: Map<string, SelectState>, key: string, initSelectedIndex: number): SelectState {
  const existing = map.get(key);
  if (existing) return existing;
  const st: SelectState = { selectedIndex: Math.max(0, initSelectedIndex | 0), open: false };
  map.set(key, st);
  return st;
}

export function applyYogaDefaultsSelect(yogaNode: any, Yoga: any): void {
  yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
  yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
  yogaNode.setPadding(Yoga.EDGE_TOP, 0);
  yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);

  yogaNode.setHeight(36);
  yogaNode.setMinHeight(36);
  yogaNode.setMinWidth(220);
}

function drawDownChevron(g: Graphics, x: number, y: number, w: number, h: number, color: number): void {
  const pad = 4;
  const x0 = x + pad;
  const x1 = x + w - pad;
  const y0 = y + pad;
  const y1 = y + h - pad;

  g.moveTo(x0, (y0 + y1) / 2 - 2);
  g.lineTo((x0 + x1) / 2, (y0 + y1) / 2 + 2);
  g.lineTo(x1, (y0 + y1) / 2 - 2);
  g.stroke({ width: 2, color });
}

function parseOptions(attrs?: Record<string, string>): string[] {
  const raw = String(attrs?.['data-options'] ?? '');
  const lines = raw
    .split('\n')
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
  return lines.length > 0 ? lines : ['(empty)'];
}

function parseInitSelectedIndex(attrs?: Record<string, string>): number {
  const raw = Number(attrs?.['data-selected-index'] ?? '0');
  return Number.isFinite(raw) ? Math.max(0, raw | 0) : 0;
}

export type SelectPopup = {
  key: string;
  absX: number;
  absY: number;
  w: number;
  h: number;
  options: string[];
  selectedIndex: number;
};

export function renderSelect(opts: {
  node: { key?: string; attrs?: Record<string, string> };
  container: Container;
  graphics: Graphics;
  w: number;
  h: number;
  absX: number;
  absY: number;
  theme: any;

  selectStates: Map<string, SelectState>;

  // Focus plumbing.
  uiState: { focusedKeyByPointer: Map<number, string | null>; keyboardOwnerPointerId: number };
  getPointerId: (ev: any) => number;
  getCursorColor: (pointerId: number) => number;

  requestPaint: (() => void) | null;

  // Collect open popup requests so they can be drawn last (above siblings).
  popupSink: SelectPopup[];
}): void {
  const { node, container, graphics: g, w, h, absX, absY, theme, selectStates, uiState, getPointerId, getCursorColor, requestPaint, popupSink } =
    opts;

  const key = node.key;
  if (!key) return;

  const options = parseOptions(node.attrs);
  const initIdx = parseInitSelectedIndex(node.attrs);
  const st = getOrInitSelectState(selectStates, key, initIdx);
  st.selectedIndex = Math.max(0, Math.min(options.length - 1, st.selectedIndex | 0));

  // Border.
  const focusPid = (() => {
    const kb = uiState.keyboardOwnerPointerId;
    if (uiState.focusedKeyByPointer.get(kb) === key) return kb;
    for (const [pid, k] of uiState.focusedKeyByPointer.entries()) {
      if (k === key) return pid;
    }
    return null;
  })();
  const focusColor = focusPid != null ? getCursorColor(focusPid) : null;

  const sw = focusColor != null ? 2 : 1;
  const inset = sw / 2;
  if (theme.control.radius > 0) g.roundRect(inset, inset, Math.max(0, w - sw), Math.max(0, h - sw), theme.control.radius);
  else g.rect(inset, inset, Math.max(0, w - sw), Math.max(0, h - sw));
  g.fill(theme.control.background);

  g.stroke({ width: sw, color: focusColor != null ? focusColor : theme.control.border });

  // Right-side arrow area.
  const arrowW = 22;
  const sepX = Math.max(0, w - arrowW);
  g.moveTo(sepX + 0.5, 0);
  g.lineTo(sepX + 0.5, h);
  g.stroke({ width: 1, color: theme.control.border, alignment: 0 });
  drawDownChevron(g, sepX, 0, arrowW, h, theme.text);

  const label = options[st.selectedIndex] ?? '';
  const t = getOrCreateText(container, '__label', (tt) => {
    (tt as any).style = {
      fontFamily: theme.fontFamily,
      fontSize: theme.fontSize,
      fill: theme.text,
      fontWeight: '400',
      wordWrap: false,
    };
  });
  t.text = label;
  t.position.set(8, 9 + TEXT_BASELINE_NUDGE_Y);

  // Click to open/close.
  clearContainerEvents(container);
  container.eventMode = 'static';
  container.cursor = 'pointer';
  container.hitArea = new Rectangle(0, 0, Math.max(0, w), Math.max(0, h));
  container.on('pointerdown', (ev: any) => {
    if (ev?.button === 2) return;
    const pid = getPointerId(ev);
    if (pid <= 0) return;

    uiState.focusedKeyByPointer.set(pid, key);
    uiState.keyboardOwnerPointerId = pid;

    st.open = !st.open;
    requestPaint?.();
    ev.stopPropagation?.();
  });

  if (st.open) {
    popupSink.push({
      key,
      absX,
      absY,
      w,
      h,
      options,
      selectedIndex: st.selectedIndex,
    });
  }
}

export function renderSelectPopup(opts: {
  popup: SelectPopup;
  stage: Container;
  theme: any;

  selectStates: Map<string, SelectState>;

  // Close on outside click is handled by the stage-level handler;
  // inside click should stop propagation.
  uiState: { keyboardOwnerPointerId: number; focusedKeyByPointer: Map<number, string | null> };
  getPointerId: (ev: any) => number;

  requestPaint: (() => void) | null;

  viewportW: number;
  viewportH: number;
}): void {
  const { popup, stage, theme, selectStates, uiState, getPointerId, requestPaint, viewportW, viewportH } = opts;

  const itemH = 30;
  const maxVisible = 7;
  const visibleCount = Math.min(maxVisible, popup.options.length);
  const panelH = visibleCount * itemH;

  // Default: below the control.
  let px = popup.absX;
  let py = popup.absY + popup.h;

  // Clamp horizontally.
  px = Math.max(0, Math.min(px, Math.max(0, viewportW - popup.w)));

  // If it would overflow bottom, show above.
  if (py + panelH > viewportH - 4) {
    py = popup.absY - panelH;
  }
  py = Math.max(0, Math.min(py, Math.max(0, viewportH - panelH)));

  const panel = new Container();
  panel.position.set(px, py);
  stage.addChild(panel);

  const bg = new Graphics();
  bg.rect(0, 0, popup.w, panelH);
  bg.fill(0xffffff);
  bg.rect(0.5, 0.5, Math.max(0, popup.w - 1), Math.max(0, panelH - 1));
  bg.stroke({ width: 1, color: theme.control.border, alignment: 0 });
  panel.addChild(bg);

  // One hit area for the whole panel.
  panel.eventMode = 'static';
  panel.cursor = 'pointer';
  panel.hitArea = new Rectangle(0, 0, popup.w, panelH);

  panel.on('pointerdown', (ev: any) => {
    if (ev?.button === 2) return;

    const pid = getPointerId(ev);
    const local = panel.toLocal(ev.global);
    const lx = local?.x ?? -1;
    const ly = local?.y ?? -1;

    if (lx < 0 || lx > popup.w || ly < 0 || ly > panelH) return;
    const idx = Math.max(0, Math.min(popup.options.length - 1, Math.floor(ly / itemH)));

    const st = selectStates.get(popup.key);
    if (st) {
      st.selectedIndex = idx;
      st.open = false;
    }

    // Keep focus on this select for this pointer.
    if (pid > 0) {
      uiState.focusedKeyByPointer.set(pid, popup.key);
      uiState.keyboardOwnerPointerId = pid;
    }

    requestPaint?.();
    ev.stopPropagation?.();
  });

  // Draw labels.
  for (let i = 0; i < visibleCount; i++) {
    const y = i * itemH;

    if (i === popup.selectedIndex) {
      const hi = new Graphics();
      hi.rect(1, y + 1, Math.max(0, popup.w - 2), itemH - 2);
      hi.fill({ color: 0x000000, alpha: 0.06 });
      panel.addChild(hi);
    }

    const t = makeThemedText({
      text: popup.options[i] ?? '',
      fontFamily: theme.fontFamily,
      fontSize: theme.fontSize,
      fill: theme.text,
      wordWrap: false,
    });
    t.position.set(8, y + 7 + TEXT_BASELINE_NUDGE_Y);
    panel.addChild(t);
  }
}
