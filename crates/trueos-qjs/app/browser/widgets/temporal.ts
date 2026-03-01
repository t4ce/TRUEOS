import { Container, Graphics, Rectangle } from 'pixi.js';
import { makeThemedText, TEXT_BASELINE_NUDGE_Y } from '../text';
import { clearContainerEvents, getOrCreateText } from '../pixiReuse';
import type { SelectPopup, SelectState } from './select';
import { getOrInitSelectState, renderSelect } from './select';
import type { SliderBounds, SliderState } from './slider';
import { getOrInitSliderState, renderSlider } from './slider';

export type TemporalKind = 'time' | 'date' | 'month' | 'week' | 'datetime-local';

export type TemporalState = {
  kind: TemporalKind;

  // Always stored as 0..99 and displayed as "20" + yy.
  year2: number;
  month: number; // 1..12
  weekIndex: number; // 1..4

  hour: number; // 0..23
  minute: number; // 0..59
  second: number; // 0..59

  // Which main panel is open for this control.
  openPanel: null | 'time' | 'month' | 'week';

  // Nested panels.
  openYear: boolean;
  openMonthGrid: boolean;

  // Stable key for the shared year slider.
  yearSliderKey: string;
};

export type TemporalPopup = {
  kind: 'month-panel' | 'week-panel' | 'time-panel' | 'year-panel' | 'month-grid';
  inputKey: string;
  absX: number;
  absY: number;
  anchorW: number;
  anchorH: number;
};

function clampInt(n: number, lo: number, hi: number): number {
  const x = Number.isFinite(n) ? (n | 0) : 0;
  return Math.max(lo, Math.min(hi, x));
}

function pad2(n: number): string {
  const x = clampInt(n, 0, 99);
  return x < 10 ? `0${x}` : String(x);
}

function parseIntOrNull(s: string, lo: number, hi: number): number | null {
  const n = Number(s);
  if (!Number.isFinite(n)) return null;
  const i = Math.trunc(n);
  if (i < lo || i > hi) return null;
  return i;
}

function parseYear2FromYYYY(yyyy: string): number | null {
  if (!/^\d{4}$/.test(yyyy)) return null;
  const y = Number(yyyy);
  if (!Number.isFinite(y)) return null;
  const y2 = y - 2000;
  if (y2 < 0 || y2 > 99) return null;
  return y2;
}

function parseTimeValue(v: string): { hour: number; minute: number; second: number } | null {
  const m = /^\s*(\d{2}):(\d{2})(?::(\d{2}))?\s*$/.exec(v);
  if (!m) return null;
  const h = parseIntOrNull(m[1], 0, 23);
  const mm = parseIntOrNull(m[2], 0, 59);
  const s = m[3] != null ? parseIntOrNull(m[3], 0, 59) : 0;
  if (h == null || mm == null || s == null) return null;
  return { hour: h, minute: mm, second: s };
}

function parseMonthValue(v: string): { year2: number; month: number } | null {
  const m = /^\s*(\d{4})-(\d{2})\s*$/.exec(v);
  if (!m) return null;
  const y2 = parseYear2FromYYYY(m[1]);
  const mo = parseIntOrNull(m[2], 1, 12);
  if (y2 == null || mo == null) return null;
  return { year2: y2, month: mo };
}

function parseDateValue(v: string): { year2: number; month: number; weekIndex: number } | null {
  // We store date selection via (month + weekIndex) for simplicity.
  // When parsing an existing YYYY-MM-DD, convert the day into a 1..4 week bucket.
  const m = /^\s*(\d{4})-(\d{2})-(\d{2})\s*$/.exec(v);
  if (!m) return null;
  const y2 = parseYear2FromYYYY(m[1]);
  const mo = parseIntOrNull(m[2], 1, 12);
  const dd = parseIntOrNull(m[3], 1, 31);
  if (y2 == null || mo == null || dd == null) return null;
  const wi = clampInt(Math.floor((dd - 1) / 7) + 1, 1, 4);
  return { year2: y2, month: mo, weekIndex: wi };
}

function parseWeekValue(v: string): { year2: number; month: number; weekIndex: number } | null {
  // Non-spec, simplified: YYYY-Wnn where nn is (month-1)*4 + weekIndex.
  const m = /^\s*(\d{4})-W(\d{2})\s*$/.exec(v);
  if (!m) return null;
  const y2 = parseYear2FromYYYY(m[1]);
  const w = parseIntOrNull(m[2], 1, 48);
  if (y2 == null || w == null) return null;
  const month = clampInt(Math.floor((w - 1) / 4) + 1, 1, 12);
  const weekIndex = clampInt(((w - 1) % 4) + 1, 1, 4);
  return { year2: y2, month, weekIndex };
}

function parseDateTimeLocalValue(v: string): {
  year2: number;
  month: number;
  weekIndex: number;
  hour: number;
  minute: number;
  second: number;
} | null {
  // Accept both "YYYY-MM-DDTHH:MM" and "YYYY-MM-DDTHH:MM:SS".
  const m = /^\s*(\d{4})-(\d{2})-(\d{2})[T\s](\d{2}):(\d{2})(?::(\d{2}))?\s*$/.exec(v);
  if (!m) return null;
  const y2 = parseYear2FromYYYY(m[1]);
  const mo = parseIntOrNull(m[2], 1, 12);
  const dd = parseIntOrNull(m[3], 1, 31);
  const hh = parseIntOrNull(m[4], 0, 23);
  const mm = parseIntOrNull(m[5], 0, 59);
  const ss = m[6] != null ? parseIntOrNull(m[6], 0, 59) : 0;
  if (y2 == null || mo == null || dd == null || hh == null || mm == null || ss == null) return null;
  const wi = clampInt(Math.floor((dd - 1) / 7) + 1, 1, 4);
  return { year2: y2, month: mo, weekIndex: wi, hour: hh, minute: mm, second: ss };
}

function formatMonthValue(st: TemporalState): string {
  return `20${pad2(st.year2)}-${pad2(st.month)}`;
}

function pseudoWeekNo(st: TemporalState): number {
  // Simplified "4 weeks per month" model.
  return (clampInt(st.month, 1, 12) - 1) * 4 + clampInt(st.weekIndex, 1, 4);
}

function formatWeekValue(st: TemporalState): string {
  return `20${pad2(st.year2)}-W${pad2(pseudoWeekNo(st))}`;
}

function formatDateValue(st: TemporalState): string {
  // Simplified "date" = first day of the selected 7-day bucket within the month.
  const day = (clampInt(st.weekIndex, 1, 4) - 1) * 7 + 1;
  return `20${pad2(st.year2)}-${pad2(st.month)}-${pad2(day)}`;
}

function formatTimeValue(st: TemporalState): string {
  return `${pad2(st.hour)}:${pad2(st.minute)}:${pad2(st.second)}`;
}

function formatDateTimeLocalValue(st: TemporalState): string {
  return `${formatDateValue(st)}T${formatTimeValue(st)}`;
}

export function getOrInitTemporalState(opts: {
  map: Map<string, TemporalState>;
  yearSliderOwners: Map<string, string>; // yearSliderKey -> inputKey
  inputKey: string;
  kind: TemporalKind;
  attrs?: Record<string, string>;
}): TemporalState {
  const { map, yearSliderOwners, inputKey, kind, attrs } = opts;
  const existing = map.get(inputKey);
  if (existing) {
    existing.kind = kind;
    return existing;
  }

  const now = new Date();
  const fallback: TemporalState = {
    kind,
    year2: clampInt(now.getFullYear() - 2000, 0, 99),
    month: clampInt(now.getMonth() + 1, 1, 12),
    weekIndex: 1,
    hour: clampInt(now.getHours(), 0, 23),
    minute: clampInt(now.getMinutes(), 0, 59),
    second: clampInt(now.getSeconds(), 0, 59),
    openPanel: null,
    openYear: false,
    openMonthGrid: false,
    yearSliderKey: `${inputKey}:year-slider`,
  };

  const raw = String(attrs?.value ?? '');
  if (raw.trim().length > 0) {
    if (kind === 'time') {
      const t = parseTimeValue(raw);
      if (t) {
        fallback.hour = t.hour;
        fallback.minute = t.minute;
        fallback.second = t.second;
      }
    } else if (kind === 'month') {
      const t = parseMonthValue(raw);
      if (t) {
        fallback.year2 = t.year2;
        fallback.month = t.month;
      }
    } else if (kind === 'week') {
      const t = parseWeekValue(raw);
      if (t) {
        fallback.year2 = t.year2;
        fallback.month = t.month;
        fallback.weekIndex = t.weekIndex;
      }
    } else if (kind === 'date') {
      const t = parseDateValue(raw);
      if (t) {
        fallback.year2 = t.year2;
        fallback.month = t.month;
        fallback.weekIndex = t.weekIndex;
      }
    } else if (kind === 'datetime-local') {
      const t = parseDateTimeLocalValue(raw);
      if (t) {
        fallback.year2 = t.year2;
        fallback.month = t.month;
        fallback.weekIndex = t.weekIndex;
        fallback.hour = t.hour;
        fallback.minute = t.minute;
        fallback.second = t.second;
      }
    }
  }

  map.set(inputKey, fallback);
  yearSliderOwners.set(fallback.yearSliderKey, inputKey);
  return fallback;
}

export function applyYogaDefaultsTemporalInput(yogaNode: any, Yoga: any, kind: TemporalKind): void {
  yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
  yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
  yogaNode.setPadding(Yoga.EDGE_TOP, 0);
  yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);

  yogaNode.setHeight(36);
  yogaNode.setMinHeight(36);

  yogaNode.setMinWidth(kind === 'datetime-local' ? 340 : 220);
}

function drawControlBorder(g: Graphics, theme: any, w: number, h: number, focusColor: number | null): void {
  const sw = focusColor != null ? 2 : 1;
  const inset = sw / 2;
  if (theme.control.radius > 0) g.roundRect(inset, inset, Math.max(0, w - sw), Math.max(0, h - sw), theme.control.radius);
  else g.rect(inset, inset, Math.max(0, w - sw), Math.max(0, h - sw));
  g.fill(theme.control.background);
  g.stroke({ width: sw, color: focusColor != null ? focusColor : theme.control.border });
}

function drawChevronDown(g: Graphics, x: number, y: number, size: number, color: number): void {
  const midX = x + size / 2;
  const midY = y + size / 2;
  g.moveTo(x, midY - 2);
  g.lineTo(midX, midY + 2);
  g.lineTo(x + size, midY - 2);
  g.stroke({ width: 2, color });
}

export function renderTemporalInput(opts: {
  node: { key?: string; tagName?: string; attrs?: Record<string, string> };
  container: Container;
  graphics: Graphics;
  w: number;
  h: number;
  absX: number;
  absY: number;
  theme: any;

  uiState: { focusedKeyByPointer: Map<number, string | null>; keyboardOwnerPointerId: number };
  getPointerId: (ev: any) => number;
  getCursorColor: (pid: number) => number;

  // Persistent state for these widgets.
  temporalStates: Map<string, TemporalState>;
  yearSliderOwners: Map<string, string>;

  // Also update the shared <input>-style value store.
  getOrInitInputValue: (key: string, attrs?: Record<string, string>) => { value?: string };

  requestPaint: (() => void) | null;

  popupSink: TemporalPopup[];
}): void {
  const {
    node,
    container,
    graphics: g,
    w,
    h,
    absX,
    absY,
    theme,
    uiState,
    getPointerId,
    getCursorColor,
    temporalStates,
    yearSliderOwners,
    getOrInitInputValue,
    requestPaint,
    popupSink,
  } = opts;

  const key = node.key;
  if (!key) return;
  if (!node.tagName) return;

  const kind: TemporalKind =
    node.tagName === 'timeinput'
      ? 'time'
      : node.tagName === 'monthinput'
        ? 'month'
        : node.tagName === 'weekinput'
          ? 'week'
          : node.tagName === 'dateinput'
            ? 'date'
            : 'datetime-local';

  const st = getOrInitTemporalState({ map: temporalStates, yearSliderOwners, inputKey: key, kind, attrs: node.attrs });

  // Keep the public-ish "value" field in sync.
  const inputValue = getOrInitInputValue(key, { ...(node.attrs ?? {}), type: 'text' });
  if (kind === 'time') inputValue.value = formatTimeValue(st);
  else if (kind === 'month') inputValue.value = formatMonthValue(st);
  else if (kind === 'week') inputValue.value = formatWeekValue(st);
  else if (kind === 'date') inputValue.value = formatDateValue(st);
  else inputValue.value = formatDateTimeLocalValue(st);

  // Focus ring selection color.
  const focusPid = (() => {
    const kb = uiState.keyboardOwnerPointerId;
    if (uiState.focusedKeyByPointer.get(kb) === key) return kb;
    for (const [pid, k] of uiState.focusedKeyByPointer.entries()) {
      if (k === key) return pid;
    }
    return null;
  })();
  const focusColor = focusPid != null ? getCursorColor(focusPid) : null;

  drawControlBorder(g, theme, w, h, focusColor);

  const leftPad = 8;

  if (kind !== 'datetime-local') {
    const shown = inputValue.value ?? '';

    const t = getOrCreateText(container, '__shown', (tt) => {
      (tt as any).style = {
        fontFamily: theme.fontFamily,
        fontSize: theme.fontSize,
        fill: theme.text,
        fontWeight: '400',
        wordWrap: false,
      };
    });
    t.text = shown;
    t.visible = true;
    t.position.set(leftPad, 9 + TEXT_BASELINE_NUDGE_Y);

    const dt = (container as any).getChildByLabel
      ? (container as any).getChildByLabel('__date')
      : container.children.find((c: any) => c?.label === '__date');
    const tt = (container as any).getChildByLabel
      ? (container as any).getChildByLabel('__time')
      : container.children.find((c: any) => c?.label === '__time');
    if (dt) dt.visible = false;
    if (tt) tt.visible = false;

    drawChevronDown(g, Math.max(0, w - 18), 11, 10, theme.text);
  } else {
    // Datetime-local: show a left "date" button area and a right "time" area.
    const sepX = Math.max(0, Math.round(w * 0.52));
    g.moveTo(sepX + 0.5, 0);
    g.lineTo(sepX + 0.5, h);
    g.stroke({ width: 1, color: theme.control.border, alignment: 0 });

    const dateText = formatDateValue(st);
    const timeText = formatTimeValue(st);

    const dt = getOrCreateText(container, '__date', (tt2) => {
      (tt2 as any).style = {
        fontFamily: theme.fontFamily,
        fontSize: theme.fontSize,
        fill: theme.text,
        fontWeight: '400',
        wordWrap: false,
      };
    });
    dt.text = dateText;
    dt.visible = true;
    dt.position.set(leftPad, 9 + TEXT_BASELINE_NUDGE_Y);

    const tt = getOrCreateText(container, '__time', (tt2) => {
      (tt2 as any).style = {
        fontFamily: theme.fontFamily,
        fontSize: theme.fontSize,
        fill: theme.text,
        fontWeight: '400',
        wordWrap: false,
      };
    });
    tt.text = timeText;
    tt.visible = true;
    tt.position.set(sepX + leftPad, 9 + TEXT_BASELINE_NUDGE_Y);

    const shownT = (container as any).getChildByLabel
      ? (container as any).getChildByLabel('__shown')
      : container.children.find((c: any) => c?.label === '__shown');
    if (shownT) shownT.visible = false;

    drawChevronDown(g, Math.max(sepX + 0, sepX + (w - sepX) - 18), 11, 10, theme.text);
  }

  // Interactivity: click to open/close.
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

    if (kind !== 'datetime-local') {
      st.openPanel = st.openPanel ? null : kind === 'time' ? 'time' : kind === 'month' ? 'month' : 'week';
      st.openYear = false;
      st.openMonthGrid = false;
    } else {
      // Datetime-local: click left half opens week panel; right half opens time panel.
      const gx = ev.global?.x ?? 0;
      const localX = gx - absX;
      const leftHalf = localX <= w * 0.52;
      st.openPanel = leftHalf ? (st.openPanel === 'week' ? null : 'week') : (st.openPanel === 'time' ? null : 'time');
      st.openYear = false;
      st.openMonthGrid = false;
    }

    temporalStates.set(key, st);
    requestPaint?.();
    ev.stopPropagation?.();
  });

  // Emit popups for draw-last.
  if (st.openPanel === 'month') {
    popupSink.push({ kind: 'month-panel', inputKey: key, absX, absY, anchorW: w, anchorH: h });
  } else if (st.openPanel === 'week') {
    popupSink.push({ kind: 'week-panel', inputKey: key, absX, absY, anchorW: w, anchorH: h });
  } else if (st.openPanel === 'time') {
    popupSink.push({ kind: 'time-panel', inputKey: key, absX, absY, anchorW: w, anchorH: h });
  }
}

function drawPanelFrame(panelG: Graphics, theme: any, w: number, h: number): void {
  panelG.rect(0, 0, w, h);
  panelG.fill(theme.control.background);
  panelG.rect(0.5, 0.5, Math.max(0, w - 1), Math.max(0, h - 1));
  panelG.stroke({ width: 1, color: theme.control.border, alignment: 0 });
}

function renderMonthGrid(opts: {
  stage: Container;
  theme: any;
  popup: TemporalPopup;
  st: TemporalState;
  viewportW: number;
  viewportH: number;
  getPointerId: (ev: any) => number;
  requestPaint: (() => void) | null;
  onPick: (month: number) => void;
}): void {
  const { stage, theme, popup, st, viewportW, viewportH, getPointerId, requestPaint, onPick } = opts;

  const cols = 4;
  const rows = 3;
  const cellW = 44;
  const cellH = 34;
  const pad = 8;
  const panelW = pad * 2 + cols * cellW;
  const panelH = pad * 2 + rows * cellH;

  let px = popup.absX;
  let py = popup.absY + popup.anchorH;

  // Clamp.
  px = Math.max(0, Math.min(px, Math.max(0, viewportW - panelW)));
  if (py + panelH > viewportH - 4) py = popup.absY - panelH;
  py = Math.max(0, Math.min(py, Math.max(0, viewportH - panelH)));

  const panel = new Container();
  panel.position.set(px, py);
  stage.addChild(panel);

  const g = new Graphics();
  drawPanelFrame(g, theme, panelW, panelH);
  panel.addChild(g);

  // Cells.
  for (let i = 0; i < 12; i++) {
    const m = i + 1;
    const cx = pad + (i % cols) * cellW;
    const cy = pad + Math.floor(i / cols) * cellH;

    if (m === st.month) {
      const hi = new Graphics();
      hi.rect(cx + 1, cy + 1, cellW - 2, cellH - 2);
      hi.fill({ color: 0x000000, alpha: 0.06 });
      panel.addChild(hi);
    }

    const lbl = makeThemedText({
      text: String(m),
      fontFamily: theme.fontFamily,
      fontSize: theme.fontSize,
      fill: theme.text,
      wordWrap: false,
    });
    lbl.position.set(cx + 14, cy + 8 + TEXT_BASELINE_NUDGE_Y);
    panel.addChild(lbl);

    g.rect(cx, cy, cellW, cellH);
    g.stroke({ width: 1, color: theme.control.border, alignment: 0 });
  }

  panel.eventMode = 'static';
  panel.cursor = 'pointer';
  panel.hitArea = new Rectangle(0, 0, panelW, panelH);
  panel.on('pointerdown', (ev: any) => {
    if (ev?.button === 2) return;
    const pid = getPointerId(ev);
    if (pid <= 0) return;

    const local = panel.toLocal(ev.global);
    const lx = local?.x ?? -1;
    const ly = local?.y ?? -1;

    const gx = lx - pad;
    const gy = ly - pad;
    if (gx < 0 || gy < 0) return;

    const col = Math.floor(gx / cellW);
    const row = Math.floor(gy / cellH);
    if (col < 0 || col >= cols || row < 0 || row >= rows) return;

    const idx = row * cols + col;
    const month = idx + 1;
    if (month < 1 || month > 12) return;

    onPick(month);
    requestPaint?.();
    ev.stopPropagation?.();
  });
}

function renderYearSlider(opts: {
  stage: Container;
  theme: any;
  popup: TemporalPopup;
  st: TemporalState;
  viewportW: number;
  viewportH: number;
  sliders: Map<string, SliderState>;
  sliderBounds: Map<string, SliderBounds>;
  sliderDrags: Map<number, { key: string }>;
  getPointerId: (ev: any) => number;
  requestPaint: (() => void) | null;
  onChange: (year2: number) => void;
}): void {
  const {
    stage,
    theme,
    popup,
    st,
    viewportW,
    viewportH,
    sliders,
    sliderBounds,
    sliderDrags,
    getPointerId,
    requestPaint,
    onChange,
  } = opts;

  const pad = 10;
  const panelW = 250;
  const panelH = 78;

  let px = popup.absX;
  let py = popup.absY;

  // Default: to the right of the anchor.
  px = popup.absX + popup.anchorW + 6;
  py = popup.absY;

  // Clamp.
  px = Math.max(0, Math.min(px, Math.max(0, viewportW - panelW)));
  py = Math.max(0, Math.min(py, Math.max(0, viewportH - panelH)));

  const panel = new Container();
  panel.position.set(px, py);
  stage.addChild(panel);

  const g = new Graphics();
  drawPanelFrame(g, theme, panelW, panelH);
  panel.addChild(g);

  const caption = makeThemedText({
    text: `20${pad2(st.year2)}`,
    fontFamily: theme.fontFamily,
    fontSize: theme.fontSize,
    fill: theme.text,
    wordWrap: false,
  });
  caption.position.set(pad, 8 + TEXT_BASELINE_NUDGE_Y);
  panel.addChild(caption);

  // Slider: map 0..1 to 0..99.
  const sliderKey = st.yearSliderKey;
  const initRatio = Math.max(0, Math.min(1, clampInt(st.year2, 0, 99) / 99));
  const sliderState = getOrInitSliderState(sliders, sliderKey, { value: String(initRatio) });
  // Don't stomp on live drags; pointermove updates sliderState.value.
  let isDragging = false;
  for (const d of sliderDrags.values()) {
    if (d.key === sliderKey) {
      isDragging = true;
      break;
    }
  }
  if (!isDragging) sliderState.value = initRatio;

  const sliderBox = new Container();
  sliderBox.position.set(pad, 40);
  panel.addChild(sliderBox);

  const sliderG = new Graphics();
  sliderBox.addChild(sliderG);

  renderSlider({
    node: { key: sliderKey, attrs: { value: String(sliderState.value) } },
    container: sliderBox,
    graphics: sliderG,
    w: panelW - pad * 2,
    h: 14,
    absX: px + pad,
    absY: py + 40,
    theme: {
      text: theme.text,
      control: { progress: theme.control.progress },
    },
    sliderStates: sliders,
    sliderBounds,
    sliderDrags,
    requestPaint,
    getPointerId,
  });

  // Update the year from the slider state (during drag).
  const yy = clampInt(Math.round(sliderState.value * 99), 0, 99);
  if (yy !== st.year2) onChange(yy);

  // Stop outside-click closing.
  panel.eventMode = 'static';
  panel.hitArea = new Rectangle(0, 0, panelW, panelH);
  panel.on('pointerdown', (ev: any) => {
    ev.stopPropagation?.();
  });
}

function renderWeekButtons(opts: {
  panel: Container;
  theme: any;
  x: number;
  y: number;
  w: number;
  st: TemporalState;
  onPick: (weekIndex: number) => void;
}): { hitRects: Array<{ x: number; y: number; w: number; h: number; weekIndex: number }> } {
  const { panel, theme, x, y, w, st, onPick } = opts;
  const btnH = 30;
  const gap = 6;
  const hitRects: Array<{ x: number; y: number; w: number; h: number; weekIndex: number }> = [];

  for (let i = 0; i < 4; i++) {
    const wi = i + 1;
    const by = y + i * (btnH + gap);

    const g = new Graphics();
    g.rect(x, by, w, btnH);
    g.fill({ color: 0x000000, alpha: wi === st.weekIndex ? 0.06 : 0.03 });
    g.rect(x + 0.5, by + 0.5, Math.max(0, w - 1), Math.max(0, btnH - 1));
    g.stroke({ width: 1, color: theme.control.border, alignment: 0 });
    panel.addChild(g);

    const wkNo = (clampInt(st.month, 1, 12) - 1) * 4 + wi;
    const t = makeThemedText({
      text: `${wi} [${pad2(wkNo)}]`,
      fontFamily: theme.fontFamily,
      fontSize: theme.fontSize,
      fill: theme.text,
      wordWrap: false,
    });
    t.position.set(x + 10, by + 7 + TEXT_BASELINE_NUDGE_Y);
    panel.addChild(t);

    hitRects.push({ x, y: by, w, h: btnH, weekIndex: wi });
  }

  // (Hook is used by caller; this function only describes hit rects.)
  void onPick;
  return { hitRects };
}

export function renderTemporalPopups(opts: {
  popups: TemporalPopup[];
  stage: Container;
  theme: any;
  viewportW: number;
  viewportH: number;

  temporalStates: Map<string, TemporalState>;

  // Shared input values map.
  getOrInitInputValue: (key: string, attrs?: Record<string, string>) => { value?: string };

  // Slider wiring for year widget.
  sliders: Map<string, SliderState>;
  sliderBounds: Map<string, SliderBounds>;
  sliderDrags: Map<number, { key: string }>;

  // Select wiring for time dropdowns.
  selects: Map<string, SelectState>;
  selectPopups: SelectPopup[];
  getCursorColor: (pid: number) => number;

  uiFocus: { focusedKeyByPointer: Map<number, string | null>; keyboardOwnerPointerId: number };
  getPointerId: (ev: any) => number;

  requestPaint: (() => void) | null;
}): void {
  const {
    popups,
    stage,
    theme,
    viewportW,
    viewportH,
    temporalStates,
    getOrInitInputValue,
    sliders,
    sliderBounds,
    sliderDrags,
    selects,
    selectPopups,
    getCursorColor,
    uiFocus,
    getPointerId,
    requestPaint,
  } = opts;

  const subPopups: TemporalPopup[] = [];

  for (const popup of popups) {
    const st = temporalStates.get(popup.inputKey);
    if (!st) continue;

    if (popup.kind === 'month-panel') {
      // Month panel = year button + month grid (single panel).
      const pad = 10;
      const cols = 4;
      const rows = 3;
      const cellW = 44;
      const cellH = 34;
      const headerH = 24;
      const headerGap = 10;

      const panelW = pad * 2 + cols * cellW;
      const panelH = pad + headerH + headerGap + rows * cellH + pad;

      let px = popup.absX;
      let py = popup.absY + popup.anchorH;
      px = Math.max(0, Math.min(px, Math.max(0, viewportW - panelW)));
      if (py + panelH > viewportH - 4) py = popup.absY - panelH;
      py = Math.max(0, Math.min(py, Math.max(0, viewportH - panelH)));

      const panel = new Container();
      panel.position.set(px, py);
      stage.addChild(panel);

      const g = new Graphics();
      drawPanelFrame(g, theme, panelW, panelH);
      panel.addChild(g);

      const yearBtn = { x: pad, y: pad, w: 132, h: headerH };
      {
        const bg = new Graphics();
        bg.rect(yearBtn.x, yearBtn.y, yearBtn.w, yearBtn.h);
        bg.fill({ color: 0x000000, alpha: 0.03 });
        bg.rect(yearBtn.x + 0.5, yearBtn.y + 0.5, Math.max(0, yearBtn.w - 1), Math.max(0, yearBtn.h - 1));
        bg.stroke({ width: 1, color: theme.control.border, alignment: 0 });
        panel.addChild(bg);

        const tt = makeThemedText({
          text: `Year 20${pad2(st.year2)}`,
          fontFamily: theme.fontFamily,
          fontSize: theme.fontSize,
          fill: theme.text,
          wordWrap: false,
        });
        tt.position.set(yearBtn.x + 8, yearBtn.y + 4 + TEXT_BASELINE_NUDGE_Y);
        panel.addChild(tt);
      }

      // Grid.
      const gridX0 = pad;
      const gridY0 = pad + headerH + headerGap;
      for (let i = 0; i < 12; i++) {
        const m = i + 1;
        const cx = gridX0 + (i % cols) * cellW;
        const cy = gridY0 + Math.floor(i / cols) * cellH;

        if (m === st.month) {
          const hi = new Graphics();
          hi.rect(cx + 1, cy + 1, cellW - 2, cellH - 2);
          hi.fill({ color: 0x000000, alpha: 0.06 });
          panel.addChild(hi);
        }

        const lbl = makeThemedText({
          text: String(m),
          fontFamily: theme.fontFamily,
          fontSize: theme.fontSize,
          fill: theme.text,
          wordWrap: false,
        });
        lbl.position.set(cx + 14, cy + 8 + TEXT_BASELINE_NUDGE_Y);
        panel.addChild(lbl);

        g.rect(cx, cy, cellW, cellH);
        g.stroke({ width: 1, color: theme.control.border, alignment: 0 });
      }

      panel.eventMode = 'static';
      panel.cursor = 'pointer';
      panel.hitArea = new Rectangle(0, 0, panelW, panelH);
      panel.on('pointerdown', (ev: any) => {
        if (ev?.button === 2) return;
        const pid = getPointerId(ev);
        if (pid <= 0) return;
        uiFocus.focusedKeyByPointer.set(pid, popup.inputKey);
        uiFocus.keyboardOwnerPointerId = pid;

        const local = panel.toLocal(ev.global);
        const lx = local?.x ?? -1;
        const ly = local?.y ?? -1;

        const hitYear = lx >= yearBtn.x && lx <= yearBtn.x + yearBtn.w && ly >= yearBtn.y && ly <= yearBtn.y + yearBtn.h;
        if (hitYear) {
          st.openYear = true;
          temporalStates.set(popup.inputKey, st);
          requestPaint?.();
          ev.stopPropagation?.();
          return;
        }

        const gx = lx - gridX0;
        const gy = ly - gridY0;
        if (gx < 0 || gy < 0) return;
        const col = Math.floor(gx / cellW);
        const row = Math.floor(gy / cellH);
        if (col < 0 || col >= cols || row < 0 || row >= rows) return;

        const idx = row * cols + col;
        const month = idx + 1;
        if (month < 1 || month > 12) return;

        st.month = month;
        st.openPanel = null;
        st.openYear = false;
        st.openMonthGrid = false;
        temporalStates.set(popup.inputKey, st);

        const val = getOrInitInputValue(popup.inputKey, { type: 'text' });
        val.value = formatMonthValue(st);

        requestPaint?.();
        ev.stopPropagation?.();
      });

      // Stop stage-level outside click close.
      panel.on('pointerdown', (ev: any) => {
        ev.stopPropagation?.();
      });

      if (st.openYear) subPopups.push({ kind: 'year-panel', inputKey: popup.inputKey, absX: px, absY: py, anchorW: panelW, anchorH: 0 });
    }

    if (popup.kind === 'week-panel') {
      const pad = 10;
      const panelW = 280;
      const panelH = 10 + 24 + 10 + (4 * 30 + 3 * 6) + 10;

      let px = popup.absX;
      let py = popup.absY + popup.anchorH;
      px = Math.max(0, Math.min(px, Math.max(0, viewportW - panelW)));
      if (py + panelH > viewportH - 4) py = popup.absY - panelH;
      py = Math.max(0, Math.min(py, Math.max(0, viewportH - panelH)));

      const panel = new Container();
      panel.position.set(px, py);
      stage.addChild(panel);

      const g = new Graphics();
      drawPanelFrame(g, theme, panelW, panelH);
      panel.addChild(g);

      // Header: Month button + Year button (spawns year slider).
      const monthBtn = { x: pad, y: pad, w: 104, h: 24 };
      const yearBtn = { x: pad + monthBtn.w + 10, y: pad, w: 120, h: 24 };

      const drawHeaderBtn = (r: { x: number; y: number; w: number; h: number }, label: string) => {
        const bg = new Graphics();
        bg.rect(r.x, r.y, r.w, r.h);
        bg.fill({ color: 0x000000, alpha: 0.03 });
        bg.rect(r.x + 0.5, r.y + 0.5, Math.max(0, r.w - 1), Math.max(0, r.h - 1));
        bg.stroke({ width: 1, color: theme.control.border, alignment: 0 });
        panel.addChild(bg);

        const t = makeThemedText({
          text: label,
          fontFamily: theme.fontFamily,
          fontSize: theme.fontSize,
          fill: theme.text,
          wordWrap: false,
        });
        t.position.set(r.x + 8, r.y + 4 + TEXT_BASELINE_NUDGE_Y);
        panel.addChild(t);
      };

      drawHeaderBtn(monthBtn, `Month ${st.month}`);
      drawHeaderBtn(yearBtn, `Year 20${pad2(st.year2)}`);

      const weeksY = pad + 24 + 10;
      const { hitRects: weekHits } = renderWeekButtons({
        panel,
        theme,
        x: pad,
        y: weeksY,
        w: panelW - pad * 2,
        st,
        onPick: () => {},
      });

      panel.eventMode = 'static';
      panel.cursor = 'pointer';
      panel.hitArea = new Rectangle(0, 0, panelW, panelH);
      panel.on('pointerdown', (ev: any) => {
        if (ev?.button === 2) return;
        const pid = getPointerId(ev);
        if (pid <= 0) return;
        uiFocus.focusedKeyByPointer.set(pid, popup.inputKey);
        uiFocus.keyboardOwnerPointerId = pid;

        const local = panel.toLocal(ev.global);
        const lx = local?.x ?? -1;
        const ly = local?.y ?? -1;

        const hit = (r: { x: number; y: number; w: number; h: number }) => lx >= r.x && lx <= r.x + r.w && ly >= r.y && ly <= r.y + r.h;

        if (hit(monthBtn)) {
          st.openMonthGrid = !st.openMonthGrid;
          temporalStates.set(popup.inputKey, st);
          requestPaint?.();
          ev.stopPropagation?.();
          return;
        }

        if (hit(yearBtn)) {
          st.openYear = true;
          temporalStates.set(popup.inputKey, st);
          requestPaint?.();
          ev.stopPropagation?.();
          return;
        }

        for (const wh of weekHits) {
          if (hit(wh)) {
            st.weekIndex = wh.weekIndex;

            // Commit value.
            const iv = getOrInitInputValue(popup.inputKey, { type: 'text' });
            if (st.kind === 'week') iv.value = formatWeekValue(st);
            else if (st.kind === 'date') iv.value = formatDateValue(st);
            else iv.value = formatDateTimeLocalValue(st);

            // Close.
            st.openPanel = null;
            st.openYear = false;
            st.openMonthGrid = false;
            temporalStates.set(popup.inputKey, st);
            requestPaint?.();
            ev.stopPropagation?.();
            return;
          }
        }
      });

      if (st.openMonthGrid) {
        subPopups.push({ kind: 'month-grid', inputKey: popup.inputKey, absX: px, absY: py + monthBtn.y + monthBtn.h + 4, anchorW: 0, anchorH: 0 });
      }
      if (st.openYear) {
        subPopups.push({ kind: 'year-panel', inputKey: popup.inputKey, absX: px + yearBtn.x, absY: py + yearBtn.y, anchorW: yearBtn.w, anchorH: 0 });
      }
    }

    if (popup.kind === 'time-panel') {
      const pad = 10;
      const panelW = 330;
      const panelH = 80;

      let px = popup.absX;
      let py = popup.absY + popup.anchorH;
      px = Math.max(0, Math.min(px, Math.max(0, viewportW - panelW)));
      if (py + panelH > viewportH - 4) py = popup.absY - panelH;
      py = Math.max(0, Math.min(py, Math.max(0, viewportH - panelH)));

      const panel = new Container();
      panel.position.set(px, py);
      stage.addChild(panel);

      const g = new Graphics();
      drawPanelFrame(g, theme, panelW, panelH);
      panel.addChild(g);

      const label = makeThemedText({
        text: 'Time',
        fontFamily: theme.fontFamily,
        fontSize: theme.fontSize,
        fill: theme.text,
        wordWrap: false,
      });
      label.position.set(pad, 8 + TEXT_BASELINE_NUDGE_Y);
      panel.addChild(label);

      const mkOptions = (count: number) => Array.from({ length: count }, (_, i) => pad2(i)).join('\n');

      const base = popup.inputKey;
      const hKey = `${base}:time-h`;
      const mKey = `${base}:time-m`;
      const sKey = `${base}:time-s`;

      // Ensure states exist.
      const hSt = getOrInitSelectState(selects, hKey, clampInt(st.hour, 0, 23));
      const mSt = getOrInitSelectState(selects, mKey, clampInt(st.minute, 0, 59));
      const sSt = getOrInitSelectState(selects, sKey, clampInt(st.second, 0, 59));
      // Keep select state aligned to the temporal state (the user interaction updates
      // selectStates first; we then sync back into TemporalState below).
      hSt.selectedIndex = clampInt(st.hour, 0, 23);
      mSt.selectedIndex = clampInt(st.minute, 0, 59);
      sSt.selectedIndex = clampInt(st.second, 0, 59);

      const selW = 96;
      const selH = 36;
      const rowY = 32;
      const gap = 8;

      const renderSel = (key: string, x: number, optsCount: number) => {
        const selBox = new Container();
        selBox.position.set(x, rowY);
        panel.addChild(selBox);

        const gg = new Graphics();
        selBox.addChild(gg);

        renderSelect({
          node: { key, attrs: { 'data-options': mkOptions(optsCount), 'data-selected-index': String(getOrInitSelectState(selects, key, 0).selectedIndex) } },
          container: selBox,
          graphics: gg,
          w: selW,
          h: selH,
          absX: px + x,
          absY: py + rowY,
          theme,
          selectStates: selects,
          uiState: uiFocus,
          getPointerId,
          getCursorColor,
          requestPaint,
          popupSink: selectPopups,
        });
      };

      renderSel(hKey, pad, 24);
      renderSel(mKey, pad + selW + gap, 60);
      renderSel(sKey, pad + (selW + gap) * 2, 60);

      // Sync back from select states.
      const hh = clampInt(selects.get(hKey)?.selectedIndex ?? st.hour, 0, 23);
      const mm = clampInt(selects.get(mKey)?.selectedIndex ?? st.minute, 0, 59);
      const ss = clampInt(selects.get(sKey)?.selectedIndex ?? st.second, 0, 59);

      st.hour = hh;
      st.minute = mm;
      st.second = ss;
      temporalStates.set(popup.inputKey, st);

      const iv = getOrInitInputValue(popup.inputKey, { type: 'text' });
      if (st.kind === 'time') iv.value = formatTimeValue(st);
      else iv.value = formatDateTimeLocalValue(st);

      // Stop outside click close.
      panel.eventMode = 'static';
      panel.hitArea = new Rectangle(0, 0, panelW, panelH);
      panel.on('pointerdown', (ev: any) => {
        ev.stopPropagation?.();
      });
    }
  }

  // Render sub-popups last (month grid, year panel).
  for (const popup of subPopups) {
    const st = temporalStates.get(popup.inputKey);
    if (!st) continue;

    if (popup.kind === 'month-grid') {
      renderMonthGrid({
        stage,
        theme,
        popup,
        st,
        viewportW,
        viewportH,
        getPointerId,
        requestPaint,
        onPick: (month) => {
          st.month = month;
          st.openMonthGrid = false;
          temporalStates.set(popup.inputKey, st);

          const iv = getOrInitInputValue(popup.inputKey, { type: 'text' });
          if (st.kind === 'month') iv.value = formatMonthValue(st);
          else if (st.kind === 'week') iv.value = formatWeekValue(st);
          else if (st.kind === 'date') iv.value = formatDateValue(st);
          else iv.value = formatDateTimeLocalValue(st);
        },
      });
    }

    if (popup.kind === 'year-panel') {
      renderYearSlider({
        stage,
        theme,
        popup,
        st,
        viewportW,
        viewportH,
        sliders,
        sliderBounds,
        sliderDrags,
        getPointerId,
        requestPaint,
        onChange: (yy) => {
          st.year2 = yy;
          temporalStates.set(popup.inputKey, st);

          const iv = getOrInitInputValue(popup.inputKey, { type: 'text' });
          if (st.kind === 'month') iv.value = formatMonthValue(st);
          else if (st.kind === 'week') iv.value = formatWeekValue(st);
          else if (st.kind === 'date') iv.value = formatDateValue(st);
          else if (st.kind === 'time') iv.value = formatTimeValue(st);
          else iv.value = formatDateTimeLocalValue(st);
        },
      });
    }
  }

  // Note: <select> popups are rendered by main.ts after *all* popup sources contribute.
}

export function closeAllTemporalPopups(temporalStates: Map<string, TemporalState>): boolean {
  let didClose = false;
  for (const st of temporalStates.values()) {
    if (st.openPanel != null || st.openYear || st.openMonthGrid) {
      st.openPanel = null;
      st.openYear = false;
      st.openMonthGrid = false;
      didClose = true;
    }
  }
  return didClose;
}
