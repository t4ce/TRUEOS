import { Application, Container, Graphics, Rectangle } from 'pixi.js';
import type { Text } from 'pixi.js';
import { defaultTheme } from './theme';
// SVG generation/parsing helpers live in widget modules.
import { makeThemedText, TEXT_BASELINE_NUDGE_Y, WRAP_EPSILON_PX } from './text';
import { clearGraphics, getOrCreateContainer, getOrCreateGraphics, getOrCreateText } from './pixiReuse';
import { clampWrappedLines, getCaretIndexFromPoint, wrapFieldTextWithIndices } from './widgets/textField';
import { renderProgressOrMeter } from './widgets/progressMeter';
import { applyYogaDefaultsProgressOrMeter } from './widgets/progressMeter';
import type { SliderBounds, SliderState } from './widgets/slider';
import {
  applyYogaDefaultsSlider,
  createYogaNodeForSliderLabel,
  renderSlider,
  renderSliderLabel,
  getOrInitSliderState as widgetGetOrInitSliderState,
} from './widgets/slider';
import { getEffectiveDetailsChildren, renderSummary } from './widgets/detailsSummary';
import { applyYogaDefaultsDetails, applyYogaDefaultsSummary } from './widgets/detailsSummary';
import { renderHr } from './widgets/hr';
import { applyYogaDefaultsHr } from './widgets/hr';
import { renderButton } from './widgets/button';
import { applyYogaDefaultsButton } from './widgets/button';
import { renderCell, renderTable } from './widgets/table';
import { applyYogaDefaultsCell, applyYogaDefaultsTable, applyYogaDefaultsTr } from './widgets/table';
import { isHeadingTag } from './widgets/headings';
import { applyYogaDefaultsHeading } from './widgets/headings';
import { renderImg } from './widgets/img';
import { applyYogaDefaultsImg } from './widgets/img';
import { applyYogaDefaultsSvg, renderSvgElement } from './widgets/svgElement';
import { applyYogaDefaultsCanvas, renderCanvasElement } from './widgets/canvasElement';
import { applyYogaDefaultsIframe, renderIframePlaceholder } from './widgets/iframe';
import type { FieldBounds as WidgetFieldBounds, InputState as WidgetInputState } from './widgets/input';
import { applyYogaDefaultsInput, renderInput } from './widgets/input';
import { renderTextarea } from './widgets/textarea';
import { applyYogaDefaultsTextarea } from './widgets/textarea';
import { applyYogaDefaultsBarrow } from './widgets/barrow';
import { applyYogaDefaultsSearchButton, applyYogaDefaultsSearchRow, renderSearchButton } from './widgets/search';
import type { DialogDrag, DialogState } from './widgets/dialog';
import { applyYogaDefaultsDialog, getOrInitDialogState, renderDialog } from './widgets/dialog';
import type { NumberState } from './widgets/number';
import { applyYogaDefaultsNumber, getOrInitNumberState, renderNumberSpinner } from './widgets/number';
import type { Rgb } from './widgets/color';
import { applyYogaDefaultsColor, renderColorPicker, sampleColorPickerAtLocal } from './widgets/color';
import type { SelectPopup, SelectState } from './widgets/select';
import { applyYogaDefaultsSelect, getOrInitSelectState, renderSelect, renderSelectPopup } from './widgets/select';
import type { TemporalPopup, TemporalState } from './widgets/temporal';
import {
  applyYogaDefaultsTemporalInput,
  closeAllTemporalPopups,
  renderTemporalInput,
  renderTemporalPopups,
} from './widgets/temporal';
import { attachPixiRenderCapture, installPixiCommandCapture } from './pixiCommandCapture';

installPixiCommandCapture();

let Yoga: any = null;

declare const __TRUEOS_CAPTURE_BUILD__: boolean;

declare global {
  interface Window {
    __TRUEOS_CAPTURE_ONLY__?: boolean;
    __TRUEOS_INPUT_HTML__?: string;
    __TRUEOS_PIXI_APP?: Application;
    __TRUEOS_PIXI_APP_READY__?: boolean;
    __TRUEOS_PIXI_APP_ERROR__?: string;
    __TRUEOS_PIXI_APP_PHASE__?: string;
    __TRUEOS_PIXI_LAYOUT_STEP__?: string;
    __TRUEOS_PIXI_DIRTY__?: boolean;
    __TRUEOS_REPAINT_NOW__?: () => void;
    __TRUEOS_PIXI_BRIDGE_STATS__?: TrueosBridgeStats;
    __TRUEOS_PIXI_LAST_LAYOUT__?: LayoutBox;
    __TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__?: Array<{ x: number; y: number; text: string }>;
    __TRUEOS_WIDGET_RENDER_TREE__?: unknown;
  }
}

type BlockNode = {
  kind: 'block';
  key: string;
  tagName: string;
  attrs?: Record<string, string>;
  children: RenderNode[];
};

type TextNode = {
  kind: 'text';
  text: string;
};

type RenderNode = BlockNode | TextNode;

type LayoutBox = {
  kind: 'block' | 'text';
  key?: string;
  tagName?: string;
  attrs?: Record<string, string>;
  text?: string;
  x: number;
  y: number;
  width: number;
  height: number;
  children: LayoutBox[];
};

type TextStyleCtx = {
  bold: boolean;
};

type TrueosBridgeStats = {
  renderNodes: number;
  renderBlocks: number;
  renderText: number;
  renderTags: string;
  renderTextSamples: string;
  layoutBoxes: number;
  layoutBlocks: number;
  layoutText: number;
  layoutMaxDepth: number;
  layoutTextSamples: string;
  measureTextCalls: number;
  scrollbarVisible: number;
  scrollbarTrack: string;
  scrollbarThumb: string;
  pixiCommands: number;
  pixiOps: string;
  pixiUnsupported: string;
};

type TrueosTreeStats = {
  nodes: number;
  blocks: number;
  text: number;
  maxDepth: number;
  tags: Record<string, number>;
};

const SCROLLBAR_PAD = 6;
const SCROLLBAR_W = 10;

const USER_POINTER_ID = 1;
const USER_POINTER_ID_3 = 3;
const USER_POINTER_ID_4 = 4;
const TRUEOS_LAYOUT_TEXT_OVERLAY_LIMIT = 512;
const trueosIframeSrcdocRowsByKey = new Map<string, string[]>();
let trueosIframeSrcdocLogCount = 0;

const uiState = {
  // Per-pointer focus (so multiple cursors can have focused fields at once).
  // Keyboard input is routed to keyboardOwnerPointerId (last cursor to click a field).
  focusedKeyByPointer: new Map<number, string | null>(),
  keyboardOwnerPointerId: 1,
  inputs: new Map<string, WidgetInputState>(),

  sliders: new Map<string, SliderState>(),
  sliderDrags: new Map<number, { key: string }>(),
  sliderBounds: new Map<string, SliderBounds>(),

  dialogs: new Map<string, DialogState>(),
  dialogDrags: new Map<number, DialogDrag>(),
  dialogSelectedBy: new Map<string, number>(),
  dialogZ: new Map<string, number>(),
  dialogZCounter: 1,

  numbers: new Map<string, NumberState>(),
  // Pointer-hold repeat for <number> spinners.
  numberHolds: new Map<
    number,
    { key: string; timeoutId: number | null; intervalId: number | null }
  >(),

  selects: new Map<string, SelectState>(),

  // Temporal inputs: <input type=time|date|month|week|datetime-local>
  temporals: new Map<string, TemporalState>(),
  // yearSliderKey -> temporal input key (so we can close year widget on slider release).
  temporalYearOwners: new Map<string, string>(),

  // Single shared color (for now): the <color> picker updates these, and <number channel=r|g|b>
  // edits them.
  color: {
    rgb: { r: 255, g: 0, b: 0 } as Rgb,
    a: 255,
    pick: null as { x: number; y: number } | null,
    draggingPointerId: null as number | null,
    // Absolute bounds (in stage coordinates) of the last rendered <color> widget.
    bounds: null as { x: number; y: number; w: number; h: number } | null,
  },

  // Cursor colors (per pointerId). Used for cursor cross and selection border color.
  cursorColors: new Map<number, number>(),

  primaryMousePointerId: 1,

  // Multi-cursor harness: lets you drive pointerId 1 or 3 using the real mouse,
  // cycling control every few seconds to stress-test the "last cursor wins" logic.
  harness: {
    enabled: true,
    activeUserPointerId: USER_POINTER_ID as number,
    periodMs: 3000,
  },

  // Stored positions for user cursors (so cursor 1 and cursor 3 can diverge).
  userCursorPos: new Map<number, { x: number; y: number }>(),

  lastMouse: { x: 0, y: 0, has: false },

  scroll: {
    y: 0,
    contentHeight: 0,
    viewportHeight: 0,

    draggingPointerId: null as number | null,
    dragOffsetY: 0,

    // Updated each paint.
    track: { x: 0, y: 0, w: SCROLLBAR_W, h: 0 },
    thumb: { x: 0, y: 0, w: SCROLLBAR_W, h: 0 },
  },

  // Per-iframe scroll state (keyed by iframe LayoutBox key).
  iframeScroll: new Map<
    string,
    {
      y: number;
      contentHeight: number;
      viewportHeight: number;
      draggingPointerId: number | null;
      dragOffsetY: number;
      // Updated each paint (absolute coords).
      track: { x: number; y: number; w: number; h: number };
      thumb: { x: number; y: number; w: number; h: number };
      // Absolute rect for hit testing wheel routing.
      rect: { x: number; y: number; w: number; h: number };
    }
  >(),
  // Frame-ordered iframe rects for event routing (deepest wins by iterating from end).
  iframeRects: [] as Array<{ key: string; x: number; y: number; w: number; h: number }>,

  // Hover simulation for non-mouse cursors (virtual/AI pointers).
  hoverRects: [] as Array<{
    key: string;
    kind: string;
    cursor: 'text' | 'pointer' | 'move';
    x: number;
    y: number;
    w: number;
    h: number;
  }>,
  hoverHandlers: new Map<string, { over: () => void; out: () => void }>(),
  hoveredKeyByPointer: new Map<number, string | null>(),
  hoveredCursorByPointer: new Map<number, 'text' | 'pointer' | 'move' | null>(),

  virtualCursor: {
    enabled: false,
    x: 0,
    y: 0,
    t: 0,
    radius: 120,
    speed: 0.9,
  },

  // Drag-selection for text-like <input> and <textarea>.
  textDrags: new Map<number, { key: string; anchor: number }>(),

  // Per-frame bounds for text-like fields, used for drag selection.
  fieldBounds: new Map<string, WidgetFieldBounds>(),

  // Per-frame clamp bounds for dragging dialogs (keyed by dialog key).
  // Bounds are expressed in the coordinate space the dialog is drawn into.
  dialogDragBounds: new Map<string, { minX: number; minY: number; maxX: number; maxY: number }>(),

  detailsOpen: new Map<string, boolean>(),

  // One context menu per pointerId.
  contextMenus: new Map<number, { open: boolean; x: number; y: number }>(),

  // Per-pointer clipboard (used by context menu Copy/Paste).
  clipboards: new Map<number, string>(),

};

// Singleton canvas/context for text measurement during rendering (used by inputs/textarea).
let renderMeasureCtx: CanvasRenderingContext2D | null = null;
let trueosMeasureTextCalls = 0;
function getRenderMeasure(theme: { fontSize: number; fontFamily: string }): (s: string) => number {
  if (!renderMeasureCtx) {
    const c = document.createElement('canvas');
    const ctx = c.getContext('2d');
    if (!ctx) throw new Error('2D canvas not available');
    renderMeasureCtx = ctx;
  }
  renderMeasureCtx.font = `${theme.fontSize}px ${theme.fontFamily}`;
  return (s: string) => {
    trueosMeasureTextCalls += 1;
    return renderMeasureCtx!.measureText(s).width;
  };
}

function compactCounts(counts: Record<string, number>, limit = 16): string {
  return Object.entries(counts)
    .sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0))
    .slice(0, limit)
    .map(([name, count]) => `${name}:${count}`)
    .join(',');
}

function stripTrueosCapturePrefix(value: unknown): string {
  let s = String(value ?? '');
  if (s.indexOf('<truesurfer-') >= 0) {
    s = s.replace(/<truesurfer-[A-Za-z0-9._-]+>/g, '');
  }
  return s;
}

function trueosResidueMayPrecedeText(value: string, index: number): boolean {
  if (index >= value.length) return true;
  const code = value.charCodeAt(index);
  return (
    code === 95 || // _
    code === 40 || // (
    code === 91 || // [
    code === 123 || // {
    code === 34 || // "
    code === 39 || // '
    (code >= 48 && code <= 57) ||
    (code >= 65 && code <= 90)
  );
}

function stripTrueosLeadingResidue(value: string): string {
  let s = value;
  let changed = true;
  while (changed) {
    changed = false;
    let residueLen = 0;
    if (s.charCodeAt(0) === 78) {
      residueLen = 1;
      while (residueLen < s.length) {
        const code = s.charCodeAt(residueLen);
        if (code !== 117 && code !== 109) break;
        residueLen += 1;
      }
      if (residueLen === 1) residueLen = 0;
    } else {
      while (residueLen < s.length) {
        const code = s.charCodeAt(residueLen);
        if (code !== 117 && code !== 109) break;
        residueLen += 1;
      }
      if (residueLen < 2) residueLen = 0;
    }
    if (residueLen >= 2 && trueosResidueMayPrecedeText(s, residueLen)) {
      s = s.slice(residueLen);
      changed = true;
    }
  }
  return s;
}

function stripTrueosSyntheticSymbols(value: unknown): string {
  let s = stripTrueosCapturePrefix(value);
  const hadSyntheticMarker =
    s.indexOf('__trueos') >= 0 ||
    s.indexOf('tsNu') >= 0 ||
    s.indexOf('tsNum') >= 0;
  if (s.indexOf('__TRUEOS_HOST_READY__') >= 0) {
    s = s.replace(/__TRUEOS_HOST_READY__/g, '');
  }
  if (s.indexOf('__trueos') >= 0) {
    s = stripTrueosSyntheticNRuns(s);
    s = s
      .replace(/__trueosNumberValue/g, '')
      .replace(/__trueosHostNum/g, '')
      .replace(/__trueosNum/g, '')
      .replace(/__trueosNu/g, '')
      .replace(/__trueos/g, '');
  }
  if (s.indexOf('tsNu') >= 0 || s.indexOf('tsNum') >= 0) {
    s = s
      .replace(/tsNum/g, '')
      .replace(/tsNutsNutsNutsNu/g, '')
      .replace(/tsNutsNutsNu/g, '')
      .replace(/tsNutsNu/g, '')
      .replace(/tsNu/g, '');
  }
  if (hadSyntheticMarker) {
    s = stripTrueosLeadingResidue(s.trimStart());
  }
  return s;
}

function stripTrueosSyntheticNRuns(value: string): string {
  const prefix = '__trueosN';
  let s = value;
  let searchFrom = 0;
  while (searchFrom < s.length) {
    const idx = s.indexOf(prefix, searchFrom);
    if (idx < 0) break;
    let end = idx + prefix.length;
    while (end < s.length) {
      const code = s.charCodeAt(end);
      if (code !== 117 && code !== 109) break;
      end += 1;
    }
    if (end === idx + prefix.length) {
      searchFrom = end;
      continue;
    }
    s = s.slice(0, idx) + s.slice(end);
    searchFrom = idx;
  }
  return s;
}

function cleanTrueosStatsText(value: unknown): string {
  return stripTrueosSyntheticSymbols(value);
}

function cleanTrueosOverlayText(value: unknown): string {
  return stripTrueosLeadingResidue(cleanTrueosStatsText(value).trimStart());
}

function isRenderableTrueosOverlayText(value: string): boolean {
  const text = normalizeWhitespace(cleanTrueosOverlayText(value));
  if (text.length === 0) return false;
  if (text === 'true' || text === 'false') return false;
  if (text === 'N' || text === 'Nu' || text === 'Num') return false;
  if (text.startsWith('<truesurfer-')) return false;
  if (text.startsWith('__trueo')) return false;
  return true;
}

function addTagCount(counts: Record<string, number>, tagName: unknown): void {
  const tag = stripTrueosCapturePrefix(tagName) || 'block';
  counts[tag] = (counts[tag] ?? 0) + 1;
}

function summarizeRenderNodes(nodes: RenderNode[]): TrueosTreeStats {
  const stats: TrueosTreeStats = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} };
  const walk = (node: RenderNode, depth: number) => {
    stats.nodes += 1;
    stats.maxDepth = Math.max(stats.maxDepth, depth);
    if (node.kind === 'text') {
      stats.text += 1;
      return;
    }
    stats.blocks += 1;
    addTagCount(stats.tags, node.tagName);
    for (const child of node.children) walk(child, depth + 1);
  };
  for (const node of nodes) walk(node, 1);
  return stats;
}

function summarizeLayoutBoxes(root: LayoutBox): TrueosTreeStats {
  const stats: TrueosTreeStats = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} };
  const walk = (box: LayoutBox, depth: number) => {
    stats.nodes += 1;
    stats.maxDepth = Math.max(stats.maxDepth, depth);
    if (box.kind === 'text') {
      stats.text += 1;
    } else {
      stats.blocks += 1;
      addTagCount(stats.tags, box.tagName ?? 'block');
    }
    for (const child of box.children) walk(child, depth + 1);
  };
  walk(root, 1);
  return stats;
}

function sampleTextForLog(text: string, limit = 64): string {
  const s = normalizeWhitespace(cleanTrueosStatsText(text));
  let out = '';
  for (let i = 0; i < s.length && out.length < limit; i += 1) {
    const ch = s.charAt(i);
    out += ch === '|' || ch === '"' || ch === '\\' ? '_' : ch;
  }
  return out;
}

function sampleRawTextForLog(text: string, limit = 120): string {
  let out = '';
  for (let i = 0; i < text.length && out.length < limit; i += 1) {
    const ch = text.charAt(i);
    out += ch === '\r' || ch === '\n' || ch === '\t' || ch === '|' || ch === '"' || ch === '\\' ? '_' : ch;
  }
  return out;
}

function summarizeRenderTextSamples(nodes: RenderNode[], limit = 12): string {
  const samples: string[] = [];
  const walk = (node: RenderNode, parentTag: string, parentKey: string) => {
    if (samples.length >= limit) return;
    if (node.kind === 'text') {
      samples.push(
        `#${samples.length}@${parentTag}:${parentKey} chars=${node.text.length} sample="${sampleTextForLog(node.text)}"`
      );
      return;
    }
    const nextTag = stripTrueosCapturePrefix(node.tagName || 'block') || 'block';
    const nextKey = node.key || '';
    for (const child of node.children) walk(child, nextTag, nextKey);
  };
  for (const node of nodes) walk(node, 'root', '');
  return samples.join('|');
}

function summarizeLayoutTextSamples(root: LayoutBox, limit = 12): string {
  const samples: string[] = [];
  const walk = (box: LayoutBox, parentTag: string, parentKey: string) => {
    if (samples.length >= limit) return;
    if (box.kind === 'text') {
      const text = box.text ?? '';
      samples.push(
        `#${samples.length}@${parentTag}:${parentKey} chars=${text.length} box=${Math.round(box.x)},${Math.round(box.y)},${Math.round(box.width)},${Math.round(box.height)} sample="${sampleTextForLog(text)}"`
      );
      return;
    }
    const nextTag = stripTrueosCapturePrefix(box.tagName || 'block') || 'block';
    const nextKey = box.key || '';
    for (const child of box.children) walk(child, nextTag, nextKey);
  };
  walk(root, 'root', '');
  return samples.join('|');
}

function trueosIframeLeafText(box: LayoutBox): string {
  if (box.kind !== 'block') return '';
  const tag = box.tagName ?? '';
  const attrs = box.attrs ?? {};
  if (
    tag === 'input' ||
    tag === 'timeinput' ||
    tag === 'dateinput' ||
    tag === 'monthinput' ||
    tag === 'weekinput' ||
    tag === 'datetimelocalinput'
  ) {
    const stateValue = box.key ? uiState.inputs.get(box.key)?.value : undefined;
    return stateValue ?? attrs.value ?? attrs.placeholder ?? '';
  }
  if (tag === 'textarea') {
    const stateValue = box.key ? uiState.inputs.get(box.key)?.value : undefined;
    return stateValue ?? attrs.value ?? attrs.placeholder ?? '';
  }
  if (tag === 'select') {
    const options = String(attrs['data-options'] ?? '')
      .split('\n')
      .map((s) => s.trim())
      .filter((s) => s.length > 0);
    const selectedIndex = Math.max(0, Number(attrs['data-selected-index'] ?? '0') | 0);
    return options[selectedIndex] ?? '';
  }
  return '';
}

function decodeBasicHtmlEntities(value: string): string {
  return String(value ?? '')
    .replace(/&quot;/g, '"')
    .replace(/&#34;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&apos;/g, "'")
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&amp;/g, '&');
}

function stripTagsToText(value: string): string {
  return normalizeWhitespace(decodeBasicHtmlEntities(String(value ?? '').replace(/<[^>]*>/g, ' ')));
}

function attrValueFromTag(tag: string, name: string): string {
  const re = new RegExp(`${name}[ \\t\\r\\n\\f]*=[ \\t\\r\\n\\f]*("([^"]*)"|'([^']*)'|([^ \\t\\r\\n\\f>]+))`, 'i');
  const match = re.exec(tag);
  return decodeBasicHtmlEntities(match?.[2] ?? match?.[3] ?? match?.[4] ?? '');
}

function sanitizeTextRows(rows: unknown[]): string[] {
  const out: string[] = [];
  for (const row of rows) {
    const text = normalizeWhitespace(String(row ?? ''));
    if (text.length === 0) continue;
    if (out.includes(text)) continue;
    out.push(text);
  }
  return out;
}

function collectIframeSrcdocTextRowsFromMarkup(srcdoc: string): string[] {
  const rows: string[] = [];
  const cleaned = String(srcdoc ?? '')
    .replace(/<script[^]*?<\/script>/gi, ' ')
    .replace(/<style[^]*?<\/style>/gi, ' ');
  const tokenRe = /<(h[1-6]|p|label|button)\b[^>]*>([^]*?)<\/\1>|<input\b[^>]*>/gi;
  let match: RegExpExecArray | null;
  while ((match = tokenRe.exec(cleaned)) && rows.length < TRUEOS_LAYOUT_TEXT_OVERLAY_LIMIT) {
    const full = match[0] ?? '';
    const tag = String(match[1] ?? '').toLowerCase();
    if (full.toLowerCase().startsWith('<input')) {
      // Form-control values are rendered by the captured widget text path.
      continue;
    }
    const text = tag === 'p' || tag === 'label' ? stripTagsToText(match[2] ?? '') : stripTagsToText(match[2] ?? '');
    if (text.length > 0) rows.push(text);
  }
  return rows;
}

function collectIframeSrcdocTextRows(srcdoc: string): string[] {
  const markupRows = collectIframeSrcdocTextRowsFromMarkup(srcdoc);
  const rows: string[] = sanitizeTextRows(markupRows);
  return sanitizeTextRows(rows);
}

function pushIframeSrcdocTextOverlays(
  box: LayoutBox,
  x: number,
  y: number,
  out: Array<{ x: number; y: number; text: string }>
): void {
  const rowsFromKey = sanitizeTextRows(trueosIframeSrcdocRowsByKey.get(String(box.key ?? '')) ?? []);
  const rowsFromAttr = sanitizeTextRows(String(box.attrs?.['data-trueos-srcdoc-text'] ?? '')
    .split('\n')
    .map((s) => normalizeWhitespace(s)));
  const rows =
    rowsFromKey.length > 0
      ? rowsFromKey
      : rowsFromAttr.length > 0
        ? rowsFromAttr
        : collectIframeSrcdocTextRows(String(box.attrs?.srcdoc ?? ''));
  let yy = y + 48;
  for (const row of rows) {
    if (out.length >= TRUEOS_LAYOUT_TEXT_OVERLAY_LIMIT) return;
    out.push({ x: x + 16, y: yy, text: row });
    yy += 32;
  }
}

function collectLayoutBoxText(box: LayoutBox): string {
  if (box.kind === 'text') return box.text ?? '';
  return box.children.map(collectLayoutBoxText).join(' ');
}

function collectLayoutTextOverlays(root: LayoutBox): Array<{ x: number; y: number; text: string }> {
  const out: Array<{ x: number; y: number; text: string }> = [];
  const walk = (box: LayoutBox, ox: number, oy: number, iframeDepth: number) => {
    if (out.length >= TRUEOS_LAYOUT_TEXT_OVERLAY_LIMIT) return;
    const x = ox + box.x;
    const y = oy + box.y;
    const isNestedIframe =
      box.kind === 'block' && box.tagName === 'iframe' && String(box.attrs?.['data-root'] ?? '') !== '1';
    const nextIframeDepth = iframeDepth + (isNestedIframe ? 1 : 0);
    const isButton = box.kind === 'block' && box.tagName === 'button';
    const rawText = box.kind === 'text' ? box.text ?? '' : isButton ? collectLayoutBoxText(box) : '';
    const cleanedText = normalizeWhitespace(cleanTrueosOverlayText(rawText));
    const before = out.length;
    if (isRenderableTrueosOverlayText(cleanedText)) {
      const labelX = isButton ? x + 8 : x;
      const labelY = isButton ? y + Math.max(0, Math.floor((box.height - defaultTheme.fontSize * 1.25) / 2)) : y;
      out.push({ x: labelX, y: labelY, text: cleanedText });
    }
    if (isButton) return;
    for (const child of box.children) walk(child, x, y, nextIframeDepth);
    if (isNestedIframe && out.length === before) {
      pushIframeSrcdocTextOverlays(box, x, y, out);
    }
  };
  walk(root, 0, 0, 0);
  return out;
}

function summarizeTrueosLayoutTextOverlays(overlays: Array<{ x: number; y: number; text: string }>, limit = 8): string {
  const samples: string[] = [];
  for (let i = 0; i < overlays.length && samples.length < limit; i += 1) {
    const item = overlays[i];
    samples.push(`#${samples.length} x=${Math.round(item.x)} y=${Math.round(item.y)} text="${sampleTextForLog(item.text)}"`);
  }
  return samples.join('|');
}

function summarizePixiCommands(): { total: number; ops: string; unsupported: string } {
  const commands = ((window as any).__pixiCapture?.commands ?? []) as Array<{ op?: unknown }>;
  const opCounts: Record<string, number> = {};
  const unsupportedCounts: Record<string, number> = {};
  const supported = new Set([
    'addChild',
    'addChildAt',
    'setChildIndex',
    'removeChild',
    'removeChildren',
    'removeAllListeners',
    'on',
    'clear',
    'rect',
    'roundRect',
    'circle',
    'ellipse',
    'moveTo',
    'lineTo',
    'closePath',
    'poly',
    'fill',
    'stroke',
    'image',
    'visible',
    'alpha',
    'scale',
    'mask',
    'text.text.set',
    'text.style.set',
    'text.resolution.set',
    'text.setSize',
    'render',
    'snapshot',
  ]);
  for (const cmd of commands) {
    const op = stripTrueosCapturePrefix(cmd?.op);
    if (!op) continue;
    opCounts[op] = (opCounts[op] ?? 0) + 1;
    if (!supported.has(op)) unsupportedCounts[op] = (unsupportedCounts[op] ?? 0) + 1;
  }
  return {
    total: commands.length,
    ops: compactCounts(opCounts, 24),
    unsupported: compactCounts(unsupportedCounts, 24),
  };
}

function publishTrueosBridgeStats(
  renderStats: TrueosTreeStats,
  layoutStats: TrueosTreeStats,
  renderTextSamples: string,
  layoutTextSamples: string
): void {
  if (!isTrueosCaptureOnly()) return;
  const pixi = summarizePixiCommands();
  window.__TRUEOS_PIXI_BRIDGE_STATS__ = {
    renderNodes: renderStats.nodes,
    renderBlocks: renderStats.blocks,
    renderText: renderStats.text,
    renderTags: compactCounts(renderStats.tags, 24),
    renderTextSamples,
    layoutBoxes: layoutStats.nodes,
    layoutBlocks: layoutStats.blocks,
    layoutText: layoutStats.text,
    layoutMaxDepth: layoutStats.maxDepth,
    layoutTextSamples,
    measureTextCalls: trueosMeasureTextCalls,
    scrollbarVisible: uiState.scroll.track.h > 0 ? 1 : 0,
    scrollbarTrack: `${Math.round(uiState.scroll.track.x)},${Math.round(uiState.scroll.track.y)},${Math.round(uiState.scroll.track.w)},${Math.round(uiState.scroll.track.h)}`,
    scrollbarThumb: `${Math.round(uiState.scroll.thumb.x)},${Math.round(uiState.scroll.thumb.y)},${Math.round(uiState.scroll.thumb.w)},${Math.round(uiState.scroll.thumb.h)}`,
    pixiCommands: pixi.total,
    pixiOps: pixi.ops,
    pixiUnsupported: pixi.unsupported,
  };
}

// Retained-mode: cache LayoutBox containers per scene root so we can update in place.
const retainedNodeCache = new WeakMap<Container, Map<string, Container>>();

function wouldCreateCycle(parent: Container, child: any): boolean {
  // Adding `child` under `parent` would create a cycle if `parent` is already in
  // the ancestry chain of `child`.
  let p: any = parent;
  while (p) {
    if (p === child) return true;
    p = p.parent;
  }
  return false;
}

function ensureChildrenArray(parent: any): any[] {
  if (!Array.isArray(parent.children)) parent.children = [];
  return parent.children;
}

function setDisplayPosition(target: any, x: number, y: number): void {
  const px = Number(x) || 0;
  const py = Number(y) || 0;
  if (!target.position || typeof target.position !== 'object') {
    target.position = { x: 0, y: 0 };
  }
  target.position.x = px;
  target.position.y = py;
}

function ensureChildAt(parent: Container, child: Container, index: number): void {
  // Guard against accidental cycles; adding a container to itself blows the stack.
  if (child === parent) return;
  if (wouldCreateCycle(parent, child)) return;
  const children = ensureChildrenArray(parent);
  if (child.parent !== parent) {
    // addChildAt allows inserting at the end (index == children.length).
    const insertAt = Math.max(0, Math.min(index, children.length));
    parent.addChildAt(child, insertAt);
    return;
  }

  // setChildIndex requires 0..children.length-1 (end is length-1, not length).
  const max = Math.max(0, children.length - 1);
  const target = Math.max(0, Math.min(index, max));
  const cur = parent.getChildIndex(child);
  if (cur !== target) parent.setChildIndex(child, target);
}

function ensureChildAtAny(parent: Container, child: any, index: number): void {
  // Same as ensureChildAt but for Graphics/Text/Mesh.
  if (child === parent) return;
  if (wouldCreateCycle(parent, child)) return;
  const children = ensureChildrenArray(parent);
  if (child.parent !== parent) {
    const insertAt = Math.max(0, Math.min(index, children.length));
    parent.addChildAt(child, insertAt);
    return;
  }

  const max = Math.max(0, children.length - 1);
  const target = Math.max(0, Math.min(index, max));
  const cur = parent.getChildIndex(child);
  if (cur !== target) parent.setChildIndex(child, target);
}

let requestRerender: (() => void) | null = null;
let requestPaint: (() => void) | null = null;

function getCursorColor(pointerId: number): number {
  const existing = uiState.cursorColors.get(pointerId);
  if (existing != null) return existing;

  // Simple palette; stable assignment per pointerId.
  const palette = [0x111111, 0x2563eb, 0x16a34a, 0xdc2626, 0x7c3aed, 0x0ea5e9, 0xf59e0b];
  const idx = Math.abs(Number(pointerId) || 0) % palette.length;
  const col = palette[idx];
  uiState.cursorColors.set(pointerId, col);
  return col;
}

function getEffectivePointerId(ev: any): number {
  const actual = Number(ev?.pointerId ?? ev?.data?.pointerId ?? 0);
  const pt = String(ev?.pointerType ?? ev?.data?.pointerType ?? '').toLowerCase();
  const isMouse = pt === 'mouse' || actual === 1 || actual === uiState.primaryMousePointerId;

  if (uiState.harness.enabled && isMouse) {
    return uiState.harness.activeUserPointerId;
  }
  return actual;
}

function isTrueosCaptureOnly(): boolean {
  return !!(globalThis as any).__TRUEOS_CAPTURE_ONLY__;
}

function setTrueosPhase(phase: string): void {
  if (isTrueosCaptureOnly()) window.__TRUEOS_PIXI_APP_PHASE__ = phase;
}

function setTrueosLayoutStep(step: string): void {
  if (isTrueosCaptureOnly()) window.__TRUEOS_PIXI_LAYOUT_STEP__ = step;
}

function describeStartupError(err: unknown): string {
  const phase = window.__TRUEOS_PIXI_APP_PHASE__ ?? 'unknown';
  const layout = window.__TRUEOS_PIXI_LAYOUT_STEP__ ?? '';
  const anyErr = err as any;
  const name = String(anyErr?.name ?? 'Error');
  const message = String(anyErr?.message ?? err);
  const stack = String(anyErr?.stack ?? '');
  return `phase=${phase} layout=${layout} name=${name} message=${message} stack=${stack}`;
}

function createCaptureOnlyApplication(): Application {
  const w = Math.max(1, Number(window.innerWidth || 1920) | 0);
  const h = Math.max(1, Number(window.innerHeight || 1080) | 0);
  const screen = new Rectangle(0, 0, w, h);
  const canvas = document.createElement('canvas') as HTMLCanvasElement;
  const renderer = {
    width: w,
    height: h,
    screen,
    render(root?: unknown) {
      return root;
    },
    resize(nextW: number, nextH: number) {
      const rw = Math.max(1, Number(nextW || w) | 0);
      const rh = Math.max(1, Number(nextH || h) | 0);
      this.width = rw;
      this.height = rh;
      screen.width = rw;
      screen.height = rh;
    },
  };

  return {
    stage: new Container(),
    screen,
    canvas,
    renderer,
    ticker: {
      stop() {},
      add() {},
      remove() {},
    },
  } as unknown as Application;
}

function createCaptureOnlyYoga() {
  const EDGE_LEFT = 0;
  const EDGE_TOP = 1;
  const EDGE_RIGHT = 2;
  const EDGE_BOTTOM = 3;
  const FLEX_DIRECTION_COLUMN = 0;
  const FLEX_DIRECTION_ROW = 1;
  const MEASURE_MODE_UNDEFINED = 0;

  class Node {
    children: Node[];
    measureFunc: ((width: number, widthMode: number) => { width: number; height: number }) | null;
    paddingLeft: number;
    paddingTop: number;
    paddingRight: number;
    paddingBottom: number;
    marginLeft: number;
    marginTop: number;
    marginRight: number;
    marginBottom: number;
    width: number;
    height: number;
    minWidth: number;
    minHeight: number;
    flexDirection: number;
    computed: { left: number; top: number; width: number; height: number };

    constructor() {
      this.children = [];
      this.measureFunc = null;
      this.paddingLeft = 0;
      this.paddingTop = 0;
      this.paddingRight = 0;
      this.paddingBottom = 0;
      this.marginLeft = 0;
      this.marginTop = 0;
      this.marginRight = 0;
      this.marginBottom = 0;
      this.width = 0;
      this.height = 0;
      this.minWidth = 0;
      this.minHeight = 0;
      this.flexDirection = FLEX_DIRECTION_COLUMN;
      this.computed = { left: 0, top: 0, width: 0, height: 0 };
    }

    static create() {
      return new Node();
    }

    setMeasureFunc(fn: Node['measureFunc']) { this.measureFunc = fn; }
    setMargin(edge: number, value: number) {
      const v = Number(value) || 0;
      if (edge === EDGE_LEFT) this.marginLeft = v;
      else if (edge === EDGE_TOP) this.marginTop = v;
      else if (edge === EDGE_RIGHT) this.marginRight = v;
      else if (edge === EDGE_BOTTOM) this.marginBottom = v;
    }
    setPadding(edge: number, value: number) {
      const v = Number(value) || 0;
      if (edge === EDGE_LEFT) this.paddingLeft = v;
      else if (edge === EDGE_TOP) this.paddingTop = v;
      else if (edge === EDGE_RIGHT) this.paddingRight = v;
      else if (edge === EDGE_BOTTOM) this.paddingBottom = v;
    }
    setFlexDirection(value: number) { this.flexDirection = value; }
    setAlignItems(_value: number) {}
    setJustifyContent(_value: number) {}
    setFlexWrap(_value: number) {}
    setFlexGrow(_value: number) {}
    setFlexShrink(_value: number) {}
    setAlignSelf(_value: number) {}
    setPositionType(_value: number) {}
    setPosition(_edge: number, _value: number) {}
    setWidth(value: number) { this.width = Math.max(0, Number(value) || 0); }
    setHeight(value: number) { this.height = Math.max(0, Number(value) || 0); }
    setMinWidth(value: number) { this.minWidth = Math.max(0, Number(value) || 0); }
    setMinHeight(value: number) { this.minHeight = Math.max(0, Number(value) || 0); }
    insertChild(child: Node, index: number) {
      this.children.splice(Math.max(0, Math.min(index, this.children.length)), 0, child);
    }
    getChildCount() { return this.children.length; }
    getComputedLeft() { return this.computed.left; }
    getComputedTop() { return this.computed.top; }
    getComputedWidth() { return this.computed.width; }
    getComputedHeight() { return this.computed.height; }
    freeRecursive() {}

    calculateLayout(width = this.width, height = this.height) {
      this.layout(0, 0, Math.max(1, Number(width) || this.width || 1), Math.max(1, Number(height) || this.height || 1));
    }

    private layout(x: number, y: number, availableW: number, availableH: number) {
      const padX = this.paddingLeft + this.paddingRight;
      const padY = this.paddingTop + this.paddingBottom;
      const ownW = Math.max(this.minWidth, this.width || availableW);
      let ownH = Math.max(this.minHeight, this.height || 0);

      this.computed.left = x;
      this.computed.top = y;
      this.computed.width = ownW;

      if (this.measureFunc) {
        const measured = this.measureFunc(Math.max(0, ownW - padX), MEASURE_MODE_UNDEFINED);
        ownH = Math.max(ownH, Math.ceil(Number(measured.height) || 0) + padY);
        this.computed.height = ownH;
        return;
      }

      if (this.flexDirection === FLEX_DIRECTION_ROW) {
        let cx = this.paddingLeft;
        let rowH = 0;
        for (const child of this.children) {
          const childW = child.width || child.minWidth || Math.max(24, (ownW - padX) / Math.max(1, this.children.length));
          child.layout(cx + child.marginLeft, this.paddingTop + child.marginTop, childW, availableH);
          cx += child.computed.width + child.marginLeft + child.marginRight;
          rowH = Math.max(rowH, child.computed.height + child.marginTop + child.marginBottom);
        }
        ownH = Math.max(ownH, rowH + padY);
      } else {
        let cy = this.paddingTop;
        for (const child of this.children) {
          const childW = Math.max(0, ownW - padX - child.marginLeft - child.marginRight);
          child.layout(this.paddingLeft + child.marginLeft, cy + child.marginTop, childW, availableH);
          cy += child.computed.height + child.marginTop + child.marginBottom;
        }
        ownH = Math.max(ownH, cy + this.paddingBottom);
      }
      this.computed.height = Math.max(this.minHeight, ownH);
    }
  }

  return {
    Node,
    EDGE_LEFT,
    EDGE_TOP,
    EDGE_RIGHT,
    EDGE_BOTTOM,
    FLEX_DIRECTION_COLUMN,
    FLEX_DIRECTION_ROW,
    FLEX_DIRECTION_ROW_REVERSE: FLEX_DIRECTION_ROW,
    ALIGN_STRETCH: 0,
    ALIGN_CENTER: 1,
    ALIGN_FLEX_START: 2,
    JUSTIFY_CENTER: 0,
    JUSTIFY_FLEX_START: 1,
    JUSTIFY_SPACE_BETWEEN: 2,
    WRAP_WRAP: 1,
    WRAP_NO_WRAP: 0,
    POSITION_TYPE_ABSOLUTE: 1,
    DIRECTION_LTR: 0,
    MEASURE_MODE_UNDEFINED,
  };
}

function computeScrollableContentHeight(root: LayoutBox): number {
  let max = 0;

  const walk = (n: LayoutBox, ax: number, ay: number) => {
    const nx = ax + n.x;
    const ny = ay + n.y;

    // Ignore floating dialogs; they are viewport overlay widgets.
    if (n.kind === 'block' && n.tagName === 'dialog') return;

    max = Math.max(max, ny + n.height);
    for (const c of n.children ?? []) walk(c, nx, ny);
  };

  for (const c of root.children ?? []) walk(c, 0, 0);
  return max;
}

function getOrInitInputState(key: string, attrs?: Record<string, string>): WidgetInputState {
  const existing = uiState.inputs.get(key);
  if (existing) return existing;

  const state: WidgetInputState = {};
  const type = (attrs?.type ?? 'text').toLowerCase();
  if (type === 'checkbox' || type === 'radio') {
    state.checked = attrs ? Object.prototype.hasOwnProperty.call(attrs, 'checked') : false;
    if (type === 'checkbox') {
      const aria = (attrs?.['aria-checked'] ?? '').toLowerCase();
      const data = (attrs?.['data-indeterminate'] ?? '').toLowerCase();
      state.indeterminate =
        (attrs ? Object.prototype.hasOwnProperty.call(attrs, 'indeterminate') : false) ||
        aria === 'mixed' ||
        data === 'true' ||
        data === '1' ||
        data === 'yes';
    }
  } else {
    state.value = attrs?.value ?? '';
  }

  uiState.inputs.set(key, state);
  return state;
}
function collectRadioGroups(root: LayoutBox): Map<string, string[]> {
  const groups = new Map<string, string[]>();

  function walk(node: LayoutBox) {
    if (node.kind === 'block' && node.tagName === 'input') {
      const type = (node.attrs?.type ?? 'text').toLowerCase();
      if (type === 'radio') {
        const name = node.attrs?.name ?? '__default__';
        const groupKey = `radio:${name}`;
        const key = node.key;
        if (key) {
          const arr = groups.get(groupKey) ?? [];
          arr.push(key);
          groups.set(groupKey, arr);
        }
      }
    }

    for (const c of node.children) walk(c);
  }

  walk(root);
  return groups;
}

function normalizeWhitespace(text: string): string {
  let out = '';
  let inWs = false;
  const s = String(text ?? '');
  for (let i = 0; i < s.length; i += 1) {
    const c = s.charCodeAt(i);
    const ws = c === 32 || c === 9 || c === 10 || c === 13 || c === 12;
    if (ws) {
      inWs = true;
      continue;
    }
    if (inWs && out.length > 0) out += ' ';
    out += s.charAt(i);
    inWs = false;
  }
  return out;
}

function cloneRenderAttrs(value: unknown): Record<string, string> | undefined {
  if (!value || typeof value !== 'object') return undefined;
  const out: Record<string, string> = {};
  for (const [key, raw] of Object.entries(value as Record<string, unknown>)) {
    if (typeof key !== 'string' || key.length === 0) continue;
    out[key] = String(raw ?? '');
  }
  return Object.keys(out).length > 0 ? out : undefined;
}

function normalizePublishedRenderNode(value: unknown, path: string): RenderNode | null {
  if (!value || typeof value !== 'object') return null;
  const node = value as Record<string, unknown>;
  const kind = String(node.kind ?? '');
  if (kind === 'text') {
    const text = normalizeWhitespace(String(node.text ?? ''));
    return text.length > 0 ? { kind: 'text', text } : null;
  }
  if (kind !== 'block') return null;

  const tagName = String(node.tagName ?? '').toLowerCase();
  if (tagName.length === 0) return null;
  const key = String(node.key ?? `${path}:${tagName}`);
  const children: RenderNode[] = [];
  const rawChildren = Array.isArray(node.children) ? node.children : [];
  for (let index = 0; index < rawChildren.length; index += 1) {
    const child = normalizePublishedRenderNode(rawChildren[index], `${path}.${index}`);
    if (child) children.push(child);
  }
  return {
    kind: 'block',
    key,
    tagName,
    attrs: cloneRenderAttrs(node.attrs),
    children,
  };
}

function normalizePublishedRenderTree(value: unknown): RenderNode[] {
  const raw = Array.isArray(value)
    ? value
    : value && typeof value === 'object' && Array.isArray((value as any).widgetRenderTree)
      ? (value as any).widgetRenderTree
      : [];
  const out: RenderNode[] = [];
  for (let index = 0; index < raw.length; index += 1) {
    const node = normalizePublishedRenderNode(raw[index], `0.${index}`);
    if (node) out.push(node);
  }
  return out;
}

function createTextMeasurer(font: string) {
  const canvas = document.createElement('canvas');
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('2D canvas not available');
  ctx.font = font;

  const pxAt = font.indexOf('px');
  let digitStart = pxAt;
  while (digitStart > 0) {
    const c = font.charCodeAt(digitStart - 1);
    if (c < 48 || c > 57) break;
    digitStart -= 1;
  }
  const fontSize = pxAt > digitStart ? Number(font.slice(digitStart, pxAt)) : 16;
  const lineHeight = Math.ceil(fontSize * 1.25);

  return {
    measure(text: string, maxWidth?: number) {
      trueosMeasureTextCalls += 1;
      const words = normalizeWhitespace(text).split(' ').filter(Boolean);
      if (words.length === 0) return { width: 0, height: lineHeight, lines: [''] };

      const lines: string[] = [];
      let current = '';
      for (const word of words) {
        const next = current ? `${current} ${word}` : word;
        const nextWidth = ctx.measureText(next).width;
        const limit = maxWidth ?? Number.POSITIVE_INFINITY;
        if (nextWidth <= limit || !current) {
          current = next;
        } else {
          lines.push(current);
          current = word;
        }
      }
      if (current) lines.push(current);

      const width = Math.min(
        Math.max(...lines.map((l) => ctx.measureText(l).width)),
        maxWidth ?? Number.POSITIVE_INFINITY
      );
      const height = lines.length * lineHeight;

      return { width: Math.ceil(width), height: Math.ceil(height), lines };
    },
    lineHeight,
    font,
  };
}

function buildLayoutTree(renderNodes: RenderNode[], viewportWidth: number, viewportHeight: number): LayoutBox {
  setTrueosLayoutStep(`build:start nodes=${renderNodes.length} viewport=${viewportWidth}x${viewportHeight}`);
  const padding = 12;
  const gap = 8;

  const theme = defaultTheme;
  setTrueosLayoutStep('build:measurer');
  const measurer = createTextMeasurer(`${theme.fontSize}px ${theme.fontFamily}`);

  function gapAfter(child: RenderNode): number {
    if (child.kind !== 'block') return 0;
    // Some nodes manage their own spacing.
    if (child.tagName === 'hr') return 0;
    // Table internals are tightly packed.
    if (child.tagName === 'tr' || child.tagName === 'td' || child.tagName === 'th') return 0;
    return gap;
  }

  function yogaForNode(node: RenderNode): { yogaNode: any; buildBox: () => LayoutBox } {
    const nodeLabel = node.kind === 'text' ? `text:${node.text.slice(0, 24)}` : `${node.tagName}:${node.key}`;
    setTrueosLayoutStep(`node:${nodeLabel}:start`);
    if (node.kind === 'text') {
      const yogaNode = Yoga.Node.create();
      setTrueosLayoutStep(`node:${nodeLabel}:measure-func`);
      yogaNode.setMeasureFunc((width: number, widthMode: number) => {
        setTrueosLayoutStep(`node:${nodeLabel}:measure-call`);
        const maxWidth = widthMode === Yoga.MEASURE_MODE_UNDEFINED ? undefined : Math.max(0, width);
        const m = measurer.measure(node.text, maxWidth);
        return { width: m.width, height: m.height };
      });
      // Small spacing so row-wrap paragraphs don't glue words together.
      yogaNode.setMargin(Yoga.EDGE_RIGHT, 6);
      yogaNode.setMargin(Yoga.EDGE_BOTTOM, 0);

      return {
        yogaNode,
        buildBox: () => ({
          kind: 'text',
          text: node.text,
          x: yogaNode.getComputedLeft(),
          y: yogaNode.getComputedTop(),
          width: yogaNode.getComputedWidth(),
          height: yogaNode.getComputedHeight(),
          children: [],
        }),
      };
    }

    // Some widgets are measured leaf nodes.
    if (node.tagName === 'sliderlabel') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:sliderlabel`);
      return createYogaNodeForSliderLabel({ node, Yoga, measurer });
    }

    setTrueosLayoutStep(`node:${node.tagName}:${node.key}:create`);
    const yogaNode = Yoga.Node.create();

    setTrueosLayoutStep(`node:${node.tagName}:${node.key}:base-defaults`);
    yogaNode.setFlexDirection(Yoga.FLEX_DIRECTION_COLUMN);
    yogaNode.setAlignItems(Yoga.ALIGN_STRETCH);
    yogaNode.setPadding(Yoga.EDGE_LEFT, padding);
    yogaNode.setPadding(Yoga.EDGE_RIGHT, padding);
    yogaNode.setPadding(Yoga.EDGE_TOP, padding);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, padding);
    // Margins are applied between siblings (not on every node), to avoid
    // "extra bottom padding" inside containers like <form>.
    yogaNode.setMargin(Yoga.EDGE_BOTTOM, 0);

    if (isHeadingTag(node.tagName)) {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:heading-defaults`);
      applyYogaDefaultsHeading(yogaNode, Yoga);
    }

    if (node.tagName === 'hr') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:hr-defaults`);
      applyYogaDefaultsHr(yogaNode, Yoga);
    }

    // Inline-ish containers: only use row+wrap when mixing text with controls.
    // For plain text paragraphs, a column layout with a single measured text node is more stable.
    if (node.tagName === 'p' || node.tagName === 'label') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:inline-scan`);
      const hasControls = node.children.some(
        (c) =>
          c.kind === 'block' &&
          (c.tagName === 'input' ||
            c.tagName === 'button' ||
            c.tagName === 'select' ||
            c.tagName === 'textarea' ||
            c.tagName === 'timeinput' ||
            c.tagName === 'dateinput' ||
            c.tagName === 'monthinput' ||
            c.tagName === 'weekinput' ||
            c.tagName === 'datetimelocalinput' ||
            c.tagName === 'progress' ||
            c.tagName === 'meter' ||
            c.tagName === 'slider' ||
            c.tagName === 'number' ||
            c.tagName === 'color')
      );

      if (hasControls) {
        yogaNode.setFlexDirection(Yoga.FLEX_DIRECTION_ROW);
        yogaNode.setFlexWrap(Yoga.WRAP_WRAP);
        yogaNode.setAlignItems(Yoga.ALIGN_CENTER);
      }

      yogaNode.setPadding(Yoga.EDGE_TOP, 4);
      yogaNode.setPadding(Yoga.EDGE_BOTTOM, 4);
      yogaNode.setPadding(Yoga.EDGE_LEFT, 4);
      yogaNode.setPadding(Yoga.EDGE_RIGHT, 4);
    }

    // Table-ish: table is a column of rows; rows are horizontal.
    if (node.tagName === 'table') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:table-defaults`);
      applyYogaDefaultsTable(yogaNode, Yoga);
    }
    if (node.tagName === 'tr') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:tr-defaults`);
      applyYogaDefaultsTr(yogaNode, Yoga);
    }
    if (node.tagName === 'td' || node.tagName === 'th') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:cell-defaults`);
      applyYogaDefaultsCell(yogaNode, Yoga);
    }

    if (node.tagName === 'input') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:input-defaults`);
      applyYogaDefaultsInput(yogaNode, node, Yoga);
    }
    if (node.tagName === 'textarea') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:textarea-defaults`);
      applyYogaDefaultsTextarea(yogaNode, Yoga);
    }
    if (node.tagName === 'select') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:select-defaults`);
      applyYogaDefaultsSelect(yogaNode, Yoga);
    }

    if (
      node.tagName === 'timeinput' ||
      node.tagName === 'dateinput' ||
      node.tagName === 'monthinput' ||
      node.tagName === 'weekinput' ||
      node.tagName === 'datetimelocalinput'
    ) {
      const kind =
        node.tagName === 'timeinput'
          ? 'time'
          : node.tagName === 'monthinput'
            ? 'month'
            : node.tagName === 'weekinput'
              ? 'week'
              : node.tagName === 'dateinput'
                ? 'date'
                : 'datetime-local';
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:temporal-defaults`);
      applyYogaDefaultsTemporalInput(yogaNode, Yoga, kind);
    }

    if (node.tagName === 'img') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:img-defaults`);
      applyYogaDefaultsImg(yogaNode, node, Yoga);
    }
    if (node.tagName === 'svg') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:svg-defaults`);
      applyYogaDefaultsSvg(yogaNode, node, Yoga);
    }
    if (node.tagName === 'canvas') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:canvas-defaults`);
      applyYogaDefaultsCanvas(yogaNode, node, Yoga);
    }
    if (node.tagName === 'iframe') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:iframe-defaults`);
      applyYogaDefaultsIframe(yogaNode, node, Yoga);
    }
    if (node.tagName === 'button') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:button-defaults`);
      applyYogaDefaultsButton(yogaNode, Yoga);
    }
    if (node.tagName === 'dialog') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:dialog-defaults`);
      applyYogaDefaultsDialog(yogaNode, Yoga);
    }
    if (node.tagName === 'number') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:number-defaults`);
      applyYogaDefaultsNumber(yogaNode, Yoga);
    }
    if (node.tagName === 'color') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:color-defaults`);
      applyYogaDefaultsColor(yogaNode, node, Yoga);
    }
    if (node.tagName === 'searchrow') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:searchrow-defaults`);
      applyYogaDefaultsSearchRow(yogaNode, Yoga);
    }
    if (node.tagName === 'searchbutton') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:searchbutton-defaults`);
      applyYogaDefaultsSearchButton(yogaNode, Yoga);
    }

    if (node.tagName === 'summary') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:summary-defaults`);
      applyYogaDefaultsSummary(yogaNode, Yoga);
    }
    if (node.tagName === 'details') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:details-defaults`);
      applyYogaDefaultsDetails(yogaNode, Yoga);
    }

    if (node.tagName === 'barrow') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:barrow-defaults`);
      applyYogaDefaultsBarrow(yogaNode, Yoga);
    }
    if (node.tagName === 'progress' || node.tagName === 'meter') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:progress-defaults`);
      applyYogaDefaultsProgressOrMeter(yogaNode, Yoga);
    }
    if (node.tagName === 'slider') {
      setTrueosLayoutStep(`node:${node.tagName}:${node.key}:slider-defaults`);
      applyYogaDefaultsSlider(yogaNode, Yoga);
    }

    setTrueosLayoutStep(`node:${node.tagName}:${node.key}:children-effective`);
    const effectiveChildren = getEffectiveDetailsChildren(node as any, uiState.detailsOpen) as RenderNode[];

    setTrueosLayoutStep(`node:${node.tagName}:${node.key}:children-map count=${effectiveChildren.length}`);
    const childPairs = effectiveChildren.map(yogaForNode);
    setTrueosLayoutStep(`node:${node.tagName}:${node.key}:children-insert`);
    for (let i = 0; i < childPairs.length; i++) {
      const childRender = effectiveChildren[i];
      const childPair = childPairs[i];
      if (childRender && childRender.kind === 'block') {
        const m = i === childPairs.length - 1 ? 0 : gapAfter(childRender);
        childPair.yogaNode.setMargin(Yoga.EDGE_BOTTOM, m);
      }
      yogaNode.insertChild(childPair.yogaNode, yogaNode.getChildCount());
    }

    return {
      yogaNode,
      buildBox: () => ({
        kind: 'block',
        key: node.key,
        tagName: node.tagName,
        attrs: node.attrs,
        x: yogaNode.getComputedLeft(),
        y: yogaNode.getComputedTop(),
        width: yogaNode.getComputedWidth(),
        height: yogaNode.getComputedHeight(),
        children: childPairs.map((c) => c.buildBox()),
      }),
    };
  }

  const rootYoga = Yoga.Node.create();
  setTrueosLayoutStep('root:flex-direction');
  rootYoga.setFlexDirection(Yoga.FLEX_DIRECTION_COLUMN);
  setTrueosLayoutStep('root:align-items');
  rootYoga.setAlignItems(Yoga.ALIGN_STRETCH);
  setTrueosLayoutStep('root:width');
  rootYoga.setWidth(viewportWidth);
  setTrueosLayoutStep('root:height');
  rootYoga.setHeight(viewportHeight);
  setTrueosLayoutStep('root:padding-left');
  rootYoga.setPadding(Yoga.EDGE_LEFT, 16);
  setTrueosLayoutStep('root:padding-top');
  rootYoga.setPadding(Yoga.EDGE_TOP, 16);
  // Reserve an extra gutter so content doesn't touch the global scrollbar.
  setTrueosLayoutStep('root:padding-right');
  rootYoga.setPadding(Yoga.EDGE_RIGHT, 16 + SCROLLBAR_PAD);
  setTrueosLayoutStep('root:padding-bottom');
  rootYoga.setPadding(Yoga.EDGE_BOTTOM, 16);

  setTrueosLayoutStep(`root:children-map count=${renderNodes.length}`);
  const pairs = renderNodes.map(yogaForNode);
  setTrueosLayoutStep('root:children-insert');
  for (let i = 0; i < pairs.length; i++) {
    const renderNode = renderNodes[i];
    const pair = pairs[i];
    if (renderNode && renderNode.kind === 'block') {
      const m = i === pairs.length - 1 ? 0 : gapAfter(renderNode);
      pair.yogaNode.setMargin(Yoga.EDGE_BOTTOM, m);
    }
    rootYoga.insertChild(pair.yogaNode, rootYoga.getChildCount());
  }

  setTrueosLayoutStep('root:calculate');
  rootYoga.calculateLayout(viewportWidth, viewportHeight, Yoga.DIRECTION_LTR);

  setTrueosLayoutStep('root:build-box');
  const box: LayoutBox = {
    kind: 'block',
    tagName: 'root',
    x: 0,
    y: 0,
    width: rootYoga.getComputedWidth(),
    height: rootYoga.getComputedHeight(),
    children: pairs.map((p) => p.buildBox()),
  };

  // Cleanup yoga nodes to avoid leaks.
  // IMPORTANT: don't manually free children; Yoga's freeRecursive handles the whole subtree.
  setTrueosLayoutStep('root:free');
  rootYoga.freeRecursive?.();

  setTrueosLayoutStep('build:done');
  return box;
}

function renderToPixi(app: Application, box: LayoutBox, sceneRoot?: Container) {
  setTrueosLayoutStep('render:start');
  const theme = defaultTheme;
  const stage = sceneRoot ?? app.stage;

  // Stable scene structure:
  // [background][contentRoot][dialogRoot][overlayRoot]
  setTrueosLayoutStep('render:get-background');
  const background = getOrCreateGraphics(stage, '__background');
  setTrueosLayoutStep('render:get-content-root');
  const contentRoot = getOrCreateContainer(stage, '__contentRoot');
  setTrueosLayoutStep('render:get-dialog-root');
  const dialogRoot = getOrCreateContainer(stage, '__dialogRoot');
  setTrueosLayoutStep('render:get-overlay-root');
  const overlayRoot = getOrCreateContainer(stage, '__overlayRoot');

  setTrueosLayoutStep('render:ensure-background');
  ensureChildAtAny(stage, background, 0);
  setTrueosLayoutStep('render:ensure-content-root');
  ensureChildAt(stage, contentRoot, 1);
  setTrueosLayoutStep('render:ensure-dialog-root');
  ensureChildAt(stage, dialogRoot, 2);
  setTrueosLayoutStep('render:ensure-overlay-root');
  ensureChildAt(stage, overlayRoot, 3);

  // Overlays are built immediate-mode (only when open); clear them each paint.
  setTrueosLayoutStep('render:overlay-remove-children');
  overlayRoot.removeChildren();
  setTrueosLayoutStep('render:overlay-removed');

  const selectPopups: SelectPopup[] = [];
  const temporalPopups: TemporalPopup[] = [];

  const radioGroups = collectRadioGroups(box);

  setTrueosLayoutStep('render:clear-ui-state');
  uiState.fieldBounds.clear();
  uiState.sliderBounds.clear();
  uiState.dialogDragBounds.clear();
  uiState.hoverRects.length = 0;
  uiState.hoverHandlers.clear();
  uiState.iframeRects.length = 0;

  setTrueosLayoutStep('render:node-cache');
  const nodeCache = retainedNodeCache.get(stage) ?? new Map<string, Container>();
  retainedNodeCache.set(stage, nodeCache);
  const usedNodeKeys = new Set<string>();

  const computeContentHeightForBox = (root: LayoutBox): number => {
    let max = 0;
    const walk = (n: LayoutBox, ax: number, ay: number) => {
      // Ignore floating dialogs; they are overlays.
      if (n.kind === 'block' && n.tagName === 'dialog') return;
      const nx = ax + n.x;
      const ny = ay + n.y;
      max = Math.max(max, ny + n.height);
      for (const c of n.children ?? []) walk(c, nx, ny);
    };
    for (const c of root.children ?? []) walk(c, 0, 0);
    return max;
  };

  const activeDragKeys = new Set<string>();
  for (const d of uiState.textDrags.values()) activeDragKeys.add(d.key);

  setTrueosLayoutStep('render:measure');
  const measure = getRenderMeasure(theme);

  function clamp(n: number, lo: number, hi: number) {
    return Math.max(lo, Math.min(hi, n));
  }

  const firstDraggingPointerForKey = (key: string): number | null => {
    for (const [pid, d] of uiState.textDrags.entries()) {
      if (d.key === key) return pid;
    }
    return null;
  };

  const focusedPidForKey = (key: string): number | null => {
    // If multiple pointers focus the same key, prefer the keyboard owner.
    const kb = uiState.keyboardOwnerPointerId;
    if (uiState.focusedKeyByPointer.get(kb) === key) return kb;
    for (const [pid, k] of uiState.focusedKeyByPointer.entries()) {
      if (k === key) return pid;
    }
    return null;
  };

  // SVG strings are centralized in src/svgs.ts

  // Background fill.
  setTrueosLayoutStep('render:background-clear');
  clearGraphics(background);
  setTrueosLayoutStep('render:background-rect');
  background.rect(0, 0, app.renderer.width, app.renderer.height);
  setTrueosLayoutStep('render:background-fill');
  background.fill(theme.background);

  // All normal document content lives in this container, which is translated for global scrolling.
  setTrueosLayoutStep('render:content-position');
  {
    const scrollState = (uiState as any).scroll;
    const scrollY = scrollState ? Number(scrollState.y || 0) || 0 : 0;
    if (scrollY !== 0) {
      const pos = (contentRoot as any).position;
      if (pos) {
        pos.x = 0;
        pos.y = -scrollY;
      }
    }
  }
  setTrueosLayoutStep('render:content-position-done');

  function drawNode(
    node: LayoutBox,
    parent: Container,
    textCtx: TextStyleCtx,
    absX = 0,
    absY = 0,
    dialogSink: LayoutBox[],
    dialogClampRect: { x: number; y: number; w: number; h: number },
    path: string,
    orderIndex: number
  ) {
    setTrueosLayoutStep(`render:draw:${path}:${node.kind}:${node.kind === 'block' ? node.tagName : 'text'}:start`);
    // IMPORTANT: LayoutBox.key can be an empty string in some helper nodes.
    // Treat empty keys as missing to avoid collisions that can create container cycles.
    const stableBlockKey =
      node.kind === 'block'
        ? node.key && node.key.length > 0
          ? node.key
          : `${path}:${node.tagName ?? 'block'}`
        : '';
    const cacheKey = node.kind === 'block' ? `b:${stableBlockKey}` : `t:${path}`;
    setTrueosLayoutStep(`render:draw:${path}:cache`);
    let container = nodeCache.get(cacheKey);
    if (!container || wouldCreateCycle(parent, container)) {
      // If the cached container would create a cycle under this parent, it means
      // the key was reused incorrectly (or the node moved) in a way that would
      // reparent an ancestor into its own subtree. Create a fresh container.
      setTrueosLayoutStep(`render:draw:${path}:new-container`);
      container = new Container();
      (container as any).label = cacheKey;
      nodeCache.set(cacheKey, container);
    }
    setTrueosLayoutStep(`render:draw:${path}:ensure-child`);
    usedNodeKeys.add(cacheKey);
    ensureChildAt(parent, container, orderIndex);

    // Use a dedicated child-root so widget internals don't interleave with layout children.
    setTrueosLayoutStep(`render:draw:${path}:children-root`);
    const childrenRoot = getOrCreateContainer(container, '__children');
    // Put layout children above the base graphics, but allow widgets to add overlays above.
    setTrueosLayoutStep(`render:draw:${path}:ensure-children-root`);
    ensureChildAt(container, childrenRoot, 1);

    setTrueosLayoutStep(`render:draw:${path}:position`);
    setDisplayPosition(container, node.x, node.y);

    // Pixel-align 1px rules so symmetric margins look symmetric.
    if (node.kind === 'block' && node.tagName === 'hr') {
      setDisplayPosition(container, Math.round(node.x), Math.round(node.y));
    }

    // Floating dialogs override their position from UI state, but are clamped to the
    // visible area of their containing stacking context (viewport, iframe content, or parent dialog).
    if (node.kind === 'block' && node.tagName === 'dialog' && node.key) {
      const st = getOrInitDialogState(uiState.dialogs, node.key);
      const dw = Math.max(0, node.width);
      const dh = Math.max(0, node.height);
      const minX = dialogClampRect.x;
      const minY = dialogClampRect.y;
      const maxX = Math.max(minX, dialogClampRect.x + dialogClampRect.w - dw);
      const maxY = Math.max(minY, dialogClampRect.y + dialogClampRect.h - dh);

      // Persist the clamp bounds so pointermove can use them during drags.
      uiState.dialogDragBounds.set(node.key, { minX, minY, maxX, maxY });

      if (isTrueosCaptureOnly() && !(st as any).__trueosInitialPositionSeeded) {
        const nestedFloatingScope = dialogClampRect.w <= 760 && dialogClampRect.h <= 800;
        const centeredX = minX + Math.max(12, Math.floor((dialogClampRect.w - dw) / 2));
        const centeredY = minY + Math.max(nestedFloatingScope ? 190 : 40, Math.floor((dialogClampRect.h - dh) / 2));
        st.x = Math.max(minX, Math.min(maxX, centeredX));
        st.y = Math.max(minY, Math.min(maxY, centeredY));
        (st as any).__trueosInitialPositionSeeded = true;
      }

      st.x = Math.max(minX, Math.min(maxX, st.x));
      st.y = Math.max(minY, Math.min(maxY, st.y));
      setDisplayPosition(container, st.x, st.y);
    }

    const nodeAbsX = absX + container.position.x;
    const nodeAbsY = absY + container.position.y;

    if (node.kind === 'block') {
      setTrueosLayoutStep(`render:draw:${path}:block:${node.tagName}:begin`);
      let nextTextCtx = textCtx;
      if (
        node.tagName === 'h1' ||
        node.tagName === 'h2' ||
        node.tagName === 'h3' ||
        node.tagName === 'summary' ||
        node.tagName === 'th'
      ) {
        nextTextCtx = { bold: true };
      }

      setTrueosLayoutStep(`render:draw:${path}:graphics`);
      const g = getOrCreateGraphics(container, '__g');
      setTrueosLayoutStep(`render:draw:${path}:graphics-clear`);
      clearGraphics(g);
      // Make sure the base graphics stays behind everything else.
      setTrueosLayoutStep(`render:draw:${path}:graphics-ensure`);
      ensureChildAtAny(container, g, 0);
      (g as any).zIndex = -10;
      let w = Math.max(0, node.width);
      let h = Math.max(0, node.height);
      let overlayLabel: Text | null = null;

      // Headings: snap to whole pixels so the 1px border doesn't land on half pixels
      // (which can look like a faint extra 1px row outside the top edge).
      if (node.tagName === 'h1' || node.tagName === 'h2' || node.tagName === 'h3') {
        setDisplayPosition(container, Math.round(node.x), Math.round(node.y));
        w = Math.round(w);
        h = Math.round(h);
      }

      setTrueosLayoutStep(`render:draw:${path}:widget:${node.tagName}`);
      if (node.tagName === 'hr') {
        renderHr({ graphics: g, w, theme });
      } else if (node.tagName === 'barrow') {
        // Layout-only wrapper for [label][bar]. No visuals.
      } else if (node.tagName === 'searchrow') {
        // Layout-only wrapper for [search icon button][input]. No visuals.
      } else if (node.tagName === 'searchbutton') {
        renderSearchButton({
          node,
          container,
          graphics: g,
          w,
          h,
          theme,
          uiState,
          getPointerId: getEffectivePointerId,
          focusInputKey: node.attrs?.['data-focus-key'],
          requestPaint,
        });
      } else if (node.tagName === 'progress' || node.tagName === 'meter') {
        renderProgressOrMeter({ node, graphics: g, w, h, theme });

      } else if (node.tagName === 'sliderlabel') {
        renderSliderLabel({
          node,
          container,
          theme,
          sliderStates: uiState.sliders,
        });

      } else if (node.tagName === 'slider') {
        renderSlider({
          node,
          container,
          graphics: g,
          w,
          h,
          absX: nodeAbsX,
          absY: nodeAbsY,
          theme,
          sliderStates: uiState.sliders,
          sliderBounds: uiState.sliderBounds,
          sliderDrags: uiState.sliderDrags,
          requestPaint,
          getPointerId: getEffectivePointerId,
        });

      } else if (
        node.tagName === 'timeinput' ||
        node.tagName === 'dateinput' ||
        node.tagName === 'monthinput' ||
        node.tagName === 'weekinput' ||
        node.tagName === 'datetimelocalinput'
      ) {
        renderTemporalInput({
          node,
          container,
          graphics: g,
          w,
          h,
          absX: nodeAbsX,
          absY: nodeAbsY,
          theme,
          uiState,
          getPointerId: getEffectivePointerId,
          getCursorColor,
          temporalStates: uiState.temporals,
          yearSliderOwners: uiState.temporalYearOwners,
          getOrInitInputValue: (k, attrs) => getOrInitInputState(k, attrs) as any,
          requestPaint,
          popupSink: temporalPopups,
        });

      } else if (node.tagName === 'input') {
        const key = node.key;
        const focusPid = key != null ? focusedPidForKey(key) : null;
        const isKeyboardFocused = key != null && uiState.focusedKeyByPointer.get(uiState.keyboardOwnerPointerId) === key;
        const caretPointerId =
          key == null
            ? null
            : isKeyboardFocused
              ? uiState.keyboardOwnerPointerId
              : activeDragKeys.has(key)
                ? firstDraggingPointerForKey(key)
                : null;
        const showCaret = caretPointerId != null;
        const focusColor = focusPid != null ? getCursorColor(focusPid) : null;
        renderInput({
          node,
          container,
          graphics: g,
          w,
          h,
          absX: nodeAbsX,
          absY: nodeAbsY,
          theme,
          textMeasure: measure,
          uiState,
          getOrInitInputState,
          clamp,
          radioGroups,
          textDrags: uiState.textDrags,
          requestPaint,
          showCaret,
          caretPointerId,
          focusColor: focusColor ?? undefined,
          getCursorColor,
          getPointerId: getEffectivePointerId,
        });
      } else if (node.tagName === 'textarea') {
        const key = node.key;
        const focusPid = key != null ? focusedPidForKey(key) : null;
        const isKeyboardFocused = key != null && uiState.focusedKeyByPointer.get(uiState.keyboardOwnerPointerId) === key;
        const caretPointerId =
          key == null
            ? null
            : isKeyboardFocused
              ? uiState.keyboardOwnerPointerId
              : activeDragKeys.has(key)
                ? firstDraggingPointerForKey(key)
                : null;
        const showCaret = caretPointerId != null;
        const focusColor = focusPid != null ? getCursorColor(focusPid) : null;
        renderTextarea({
          node,
          container,
          graphics: g,
          w,
          h,
          absX: nodeAbsX,
          absY: nodeAbsY,
          theme,
          textMeasure: measure,
          uiState,
          getOrInitInputState,
          clamp,
          textDrags: uiState.textDrags,
          requestPaint,
          showCaret,
          caretPointerId,
          focusColor: focusColor ?? undefined,
          getCursorColor,
          getPointerId: getEffectivePointerId,
        });
      } else if (node.tagName === 'select') {
        // Ensure state exists so it persists across rerenders.
        if (node.key) {
          const initIdx = Number(node.attrs?.['data-selected-index'] ?? '0');
          getOrInitSelectState(uiState.selects, node.key, Number.isFinite(initIdx) ? initIdx : 0);
        }
        renderSelect({
          node,
          container,
          graphics: g,
          w,
          h,
          absX: nodeAbsX,
          absY: nodeAbsY,
          theme,
          selectStates: uiState.selects,
          uiState,
          getPointerId: getEffectivePointerId,
          getCursorColor,
          requestPaint,
          popupSink: selectPopups,
        });
      } else if (node.tagName === 'summary') {
        if (node.key) {
          uiState.hoverRects.push({ key: node.key, kind: 'summary', cursor: 'pointer', x: nodeAbsX, y: nodeAbsY, w, h });
        }
        renderSummary({
          node,
          container,
          w,
          h,
          theme,
          detailsOpen: uiState.detailsOpen,
          requestRerender,
        });
      } else if (node.tagName === 'dialog') {
        renderDialog({
          node,
          container,
          w,
          h,
          theme,
          selectedBy: uiState.dialogSelectedBy,
          getCursorColor,
          dialogStates: uiState.dialogs,
          dialogDrags: uiState.dialogDrags,
          bringToFront: (k) => {
            uiState.dialogZ.set(k, uiState.dialogZCounter++);
          },
          requestPaint,
          getPointerId: getEffectivePointerId,
        });
      } else if (node.tagName === 'img') {
        renderImg({ node, container, graphics: g, w, h, theme, requestRerender });
      } else if (node.tagName === 'svg') {
        const svgMarkup = node.attrs?.['data-svg'] ?? '';
        // Reuse the same Graphics container; svg rendering adds its own Graphics.
        renderSvgElement({ svgMarkup, container, w, h, requestRerender });
      } else if (node.tagName === 'canvas') {
        renderCanvasElement({ node, container, graphics: g, w, h, theme });
      } else if (node.tagName === 'iframe') {
        renderIframePlaceholder({ node, container, graphics: g, w, h, theme });
      } else if (node.tagName === 'color') {
        uiState.color.bounds = { x: nodeAbsX, y: nodeAbsY, w: Math.max(0, w), h: Math.max(0, h) };
        renderColorPicker({
          node,
          container,
          graphics: g,
          w,
          h,
          theme,
          rgb: uiState.color.rgb,
          setRgb: (rgb) => {
            uiState.color.rgb = rgb;
          },
          alpha: uiState.color.a,
          setAlpha: (a) => {
            uiState.color.a = Math.max(0, Math.min(255, Math.round(a)));
          },
          pick: uiState.color.pick,
          setPick: (p) => {
            uiState.color.pick = p;
          },
          requestPaint,
          getPointerId: getEffectivePointerId,
          setDraggingPointerId: (pid) => {
            uiState.color.draggingPointerId = pid;
          },
        });
      } else if (node.tagName === 'number') {
        const key = node.key;
        const ch = String(node.attrs?.channel ?? '').toLowerCase();
        const isCh = ch === 'r' || ch === 'g' || ch === 'b' || ch === 'a';
        if (key) {
          renderNumberSpinner({
            node,
            container,
            graphics: g,
            w,
            h,
            theme,
            getValue: () => {
              if (isCh) {
                if (ch === 'a') return uiState.color.a ?? 255;
                return (uiState.color.rgb as any)[ch] ?? 0;
              }
              return getOrInitNumberState(uiState.numbers, key, node.attrs).value;
            },
            setValue: (n) => {
              if (isCh) {
                if (ch === 'a') uiState.color.a = Math.max(0, Math.min(255, Math.round(n)));
                else (uiState.color.rgb as any)[ch] = Math.max(0, Math.min(255, Math.round(n)));
              } else {
                getOrInitNumberState(uiState.numbers, key, node.attrs).value = n;
              }
            },
            requestPaint,
            numberHolds: uiState.numberHolds,
            getPointerId: getEffectivePointerId,
          });
        }
      } else if (node.tagName === 'button') {
        if (node.key) {
          uiState.hoverRects.push({ key: node.key, kind: 'button', cursor: 'pointer', x: nodeAbsX, y: nodeAbsY, w, h });
        }
        renderButton({
          container,
          graphics: g,
          w,
          h,
          label: normalizeWhitespace(collectLayoutBoxText(node)),
          theme,
          registerHoverHandlers: node.key
            ? (handlers) => {
                uiState.hoverHandlers.set(node.key!, handlers);
              }
            : undefined,
        });
      } else if (isHeadingTag(node.tagName)) {
        // Headings should not get the generic 1px element border.
      } else if (node.tagName === 'table') {
        renderTable({ graphics: g, w, h, boxBorder: theme.boxBorder });
      } else if (node.tagName === 'td' || node.tagName === 'th') {
        renderCell({ nodeTag: node.tagName, graphics: g, w, h, theme });
      } else {
        // Default block border: draw fully inside the box so it doesn't "bleed"
        // into the outside margin area (which looks like an extra 1px row above).
        const bw = Math.max(0, Math.round(w));
        const bh = Math.max(0, Math.round(h));
        g.rect(0, 0, bw, bh);
        g.stroke({ width: 1, color: theme.boxBorder, alignment: 0 });
      }
      setTrueosLayoutStep(`render:draw:${path}:overlay-label`);
      if (overlayLabel) container.addChild(overlayLabel);

      // Iframe: clip all nested content to the frame rect.
      // (This is the first step toward a true nested scene.)
      let iframeContentRoot: Container | null = null;
      let iframeScrollRoot: Container | null = null;
      const isRootIframe = node.tagName === 'iframe' && String(node.attrs?.['data-root'] ?? '') === '1';
      if (node.tagName === 'iframe' && !isRootIframe) {
        // Chrome is drawn into `g` by renderIframePlaceholder; scroll applies only to content.
        const IFRAME_CHROME_TOP = 34;
        const IFRAME_PAD_X = 8;
        const IFRAME_PAD_BOTTOM = 8;

        // Track iframe rect for wheel routing.
        if (node.key) {
          uiState.iframeRects.push({ key: node.key, x: nodeAbsX, y: nodeAbsY, w: Math.max(0, w), h: Math.max(0, h) });
        }

        iframeContentRoot = getOrCreateContainer(container, '__iframeContentRoot');
        setDisplayPosition(iframeContentRoot, 0, 0);

        // Mask only the content area (keep header fixed).
        const contentMask = getOrCreateGraphics(container, '__iframeContentMask');
        clearGraphics(contentMask);
        const maskX = 0;
        const maskY = IFRAME_CHROME_TOP;
        const maskW = Math.max(0, w);
        const maskH = Math.max(0, h - IFRAME_CHROME_TOP);
        contentMask.rect(maskX, maskY, maskW, maskH);
        contentMask.fill(0xffffff);
        contentMask.alpha = 0;
        iframeContentRoot.mask = contentMask;

        // Per-iframe scroll state.
        const iframeKey = node.key ?? '';
        const st =
          uiState.iframeScroll.get(iframeKey) ??
          {
            y: 0,
            contentHeight: 0,
            viewportHeight: 0,
            draggingPointerId: null as number | null,
            dragOffsetY: 0,
            track: { x: 0, y: 0, w: SCROLLBAR_W, h: 0 },
            thumb: { x: 0, y: 0, w: SCROLLBAR_W, h: 0 },
            rect: { x: nodeAbsX, y: nodeAbsY, w: Math.max(0, w), h: Math.max(0, h) },
          };
        st.rect = { x: nodeAbsX, y: nodeAbsY, w: Math.max(0, w), h: Math.max(0, h) };

        // Compute content height from layout subtree (relative), then clamp scroll.
        st.contentHeight = computeContentHeightForBox(node);
        st.viewportHeight = Math.max(0, h - IFRAME_CHROME_TOP - IFRAME_PAD_BOTTOM);
        const maxScroll = Math.max(0, st.contentHeight - st.viewportHeight);
        st.y = Math.max(0, Math.min(st.y, maxScroll));

        // Scroll root (translated).
        iframeScrollRoot = getOrCreateContainer(iframeContentRoot, '__iframeScrollRoot');
        setDisplayPosition(iframeScrollRoot, 0, -st.y);

        // Draw iframe-local scrollbar.
        const scrollbar = getOrCreateGraphics(container, '__iframeScrollbar');
        clearGraphics(scrollbar);
        scrollbar.eventMode = 'static';
        // Place it inside the iframe content area.
        const pad = SCROLLBAR_PAD;
        const trackW = SCROLLBAR_W;
        const trackX = Math.max(0, w - trackW - pad);
        const trackY = IFRAME_CHROME_TOP + pad;
        const trackH = Math.max(0, (h - IFRAME_CHROME_TOP) - pad * 2);
        const show = maxScroll > 0.5 && trackH > 1;
        scrollbar.visible = show;
        if (show) {
          const minThumbH = 24;
          const thumbH = Math.max(minThumbH, ((st.viewportHeight || 1) / Math.max(1, st.contentHeight)) * trackH);
          const travel = Math.max(1, trackH - thumbH);
          const ratio = maxScroll <= 0 ? 0 : st.y / maxScroll;
          const thumbY = trackY + travel * ratio;

          st.track = { x: nodeAbsX + trackX, y: nodeAbsY + trackY, w: trackW, h: trackH };
          st.thumb = { x: nodeAbsX + trackX, y: nodeAbsY + thumbY, w: trackW, h: thumbH };

          scrollbar.rect(trackX, trackY, trackW, trackH);
          scrollbar.fill({ color: 0x000000, alpha: 0.06 });
          scrollbar.rect(trackX, thumbY, trackW, thumbH);
          scrollbar.fill({ color: 0x000000, alpha: 0.25 });

          scrollbar.on('pointerdown', (ev: any) => {
            if (ev?.button === 2) return;
            const pid = getEffectivePointerId(ev);
            if (pid <= 0) return;

            const gx = ev.global?.x ?? 0;
            const gy = ev.global?.y ?? 0;

            const hitTrack = gx >= st.track.x && gx <= st.track.x + st.track.w && gy >= st.track.y && gy <= st.track.y + st.track.h;
            if (!hitTrack) return;

            const hitThumb = gx >= st.thumb.x && gx <= st.thumb.x + st.thumb.w && gy >= st.thumb.y && gy <= st.thumb.y + st.thumb.h;
            if (hitThumb) {
              st.draggingPointerId = pid;
              st.dragOffsetY = gy - st.thumb.y;
              uiState.iframeScroll.set(iframeKey, st);
              ev.stopPropagation?.();
              return;
            }

            // Track click: jump thumb and start dragging.
            const travel2 = Math.max(1, st.track.h - st.thumb.h);
            const targetTop = Math.max(st.track.y, Math.min(st.track.y + travel2, gy - st.thumb.h / 2));
            const ratio2 = (targetTop - st.track.y) / travel2;
            st.y = Math.max(0, Math.min(maxScroll, ratio2 * maxScroll));
            st.draggingPointerId = pid;
            st.dragOffsetY = gy - targetTop;
            uiState.iframeScroll.set(iframeKey, st);
            requestPaint?.();
            ev.stopPropagation?.();
          });
        } else {
          st.track = { x: 0, y: 0, w: trackW, h: 0 };
          st.thumb = { x: 0, y: 0, w: trackW, h: 0 };
        }

        uiState.iframeScroll.set(iframeKey, st);
      }

      // Dialog stacking model:
      // - Dialogs are hoisted out of normal flow drawing into an overlay list.
      // - The overlay list is per stacking context: root or the nearest ancestor dialog.
      // This makes dialogs float above later siblings even when nested inside other elements,
      // while still letting dialogs nest inside dialogs.
      const localDialogs: LayoutBox[] = [];
      const childSink = node.tagName === 'dialog' || (node.tagName === 'iframe' && !isRootIframe) ? localDialogs : dialogSink;

      // Dialog clamp rect for this stacking context.
      let childDialogClampRect = dialogClampRect;
      if (node.tagName === 'dialog') {
        // Nested dialogs are constrained to the parent dialog box.
        childDialogClampRect = { x: 0, y: 0, w: Math.max(0, w), h: Math.max(0, h) };
      } else if (node.tagName === 'iframe' && !isRootIframe) {
        // Dialogs inside iframes are constrained to the iframe content viewport.
        const iframeKey = node.key ?? '';
        const st = uiState.iframeScroll.get(iframeKey);
        const scrollY = st ? st.y : 0;
        const IFRAME_CHROME_TOP = 34;
        childDialogClampRect = {
          x: 0,
          y: IFRAME_CHROME_TOP + scrollY,
          w: Math.max(0, w),
          h: Math.max(0, h - IFRAME_CHROME_TOP),
        };
      }

      const childParent = iframeScrollRoot ?? iframeContentRoot ?? childrenRoot;
      const childAbsX = nodeAbsX + (iframeContentRoot?.position.x ?? 0);
      const childAbsY = nodeAbsY + (iframeContentRoot?.position.y ?? 0) + (iframeScrollRoot?.position.y ?? 0);

      setTrueosLayoutStep(`render:draw:${path}:children`);
      let childOrder = 0;
      for (let ci = 0; ci < (node.children ?? []).length; ci++) {
        const child = (node.children ?? [])[ci];
        if (child.kind === 'block' && child.tagName === 'dialog') {
          childSink.push(child);
        } else if (node.tagName === 'button' && child.kind === 'text') {
          continue;
        } else {
          drawNode(child, childParent, nextTextCtx, childAbsX, childAbsY, childSink, childDialogClampRect, `${path}.${ci}`, childOrder++);
        }
      }

      if ((node.tagName === 'dialog' || (node.tagName === 'iframe' && !isRootIframe)) && localDialogs.length > 0) {
        localDialogs.sort((a, b) => {
          const az = a.key ? uiState.dialogZ.get(a.key) ?? 0 : 0;
          const bz = b.key ? uiState.dialogZ.get(b.key) ?? 0 : 0;
          return az - bz;
        });
        for (const dlg of localDialogs) {
          const dlgKey = dlg.key && dlg.key.length > 0 ? dlg.key : `${path}.dlg.${childOrder}`;
          drawNode(dlg, childParent, nextTextCtx, childAbsX, childAbsY, localDialogs, childDialogClampRect, `${path}.dlg.${dlgKey}`, childOrder++);
        }
      }
    } else {
      setTrueosLayoutStep(`render:draw:${path}:text:begin`);
      const t = getOrCreateText(container, '__text', (tt) => {
        (tt as any).style = {
          fontFamily: theme.fontFamily,
          fontSize: theme.fontSize,
          fill: theme.text,
          fontWeight: textCtx.bold ? '700' : '400',
          wordWrap: true,
          wordWrapWidth: 0,
        };
      });
      t.text = node.text ?? '';
      (t.style as any).fontFamily = theme.fontFamily;
      (t.style as any).fontSize = theme.fontSize;
      (t.style as any).fill = theme.text;
      (t.style as any).fontWeight = textCtx.bold ? '700' : '400';
      (t.style as any).wordWrap = true;
      (t.style as any).wordWrapWidth = Math.max(0, Math.ceil(node.width) + WRAP_EPSILON_PX);
      setDisplayPosition(t, 0, TEXT_BASELINE_NUDGE_Y);
      setTrueosLayoutStep(`render:draw:${path}:text:done`);
    }
  }

  setTrueosLayoutStep('render:root-loop');
  const baseTextCtx: TextStyleCtx = { bold: false };
  const stageClampRect = { x: 0, y: 0, w: app.renderer.width, h: app.renderer.height };
  const rootDialogs: LayoutBox[] = [];
  const contentRootPos = (contentRoot as any).position;
  const contentRootY = contentRootPos ? Number(contentRootPos.y || 0) || 0 : 0;
  let rootOrder = 0;
  for (let i = 0; i < box.children.length; i++) {
    setTrueosLayoutStep(`render:root-loop:${i}`);
    const child = box.children[i];
    if (!child) continue;
    if (child.kind === 'block' && child.tagName === 'dialog') rootDialogs.push(child);
    else {
      setTrueosLayoutStep(`render:root-loop:${i}:dispatch`);
      drawNode(child, contentRoot, baseTextCtx, 0, contentRootY, rootDialogs, stageClampRect, `root.${i}`, rootOrder++);
    }
  }

  setTrueosLayoutStep('render:root-dialogs');
  if (rootDialogs.length > 0) {
    rootDialogs.sort((a, b) => {
      const az = a.key ? uiState.dialogZ.get(a.key) ?? 0 : 0;
      const bz = b.key ? uiState.dialogZ.get(b.key) ?? 0 : 0;
      return az - bz;
    });
    let dlgOrder = 0;
    for (const dlg of rootDialogs) {
      const dlgKey = dlg.key && dlg.key.length > 0 ? dlg.key : `rootdlg.${dlgOrder}`;
      drawNode(dlg, dialogRoot, baseTextCtx, 0, 0, rootDialogs, stageClampRect, `dlg.${dlgKey}`, dlgOrder++);
    }
  }

  // Draw temporal picker popups before <select> popups so time pickers can contribute
  // nested <select> dropdowns and have those rendered above.
  setTrueosLayoutStep('render:temporal-popups');
  if (temporalPopups.length > 0) {
    renderTemporalPopups({
      popups: temporalPopups,
      stage: overlayRoot,
      theme,
      viewportW: app.renderer.width,
      viewportH: app.renderer.height,
      temporalStates: uiState.temporals,
      getOrInitInputValue: (k, attrs) => getOrInitInputState(k, attrs) as any,
      sliders: uiState.sliders,
      sliderBounds: uiState.sliderBounds,
      sliderDrags: uiState.sliderDrags,
      selects: uiState.selects,
      selectPopups,
      uiFocus: uiState,
      getPointerId: getEffectivePointerId,
      getCursorColor,
      requestPaint,
    });
  }

  // Draw <select> popups last so they appear above later siblings.
  setTrueosLayoutStep('render:select-popups');
  if (selectPopups.length > 0) {
    for (const p of selectPopups) {
      renderSelectPopup({
        popup: p,
        stage: overlayRoot,
        theme,
        selectStates: uiState.selects,
        uiState,
        getPointerId: getEffectivePointerId,
        requestPaint,
        viewportW: app.renderer.width,
        viewportH: app.renderer.height,
      });
    }
  }

  // Context menu overlay.
  setTrueosLayoutStep('render:context-menus');
  for (const [ownerPid, menuState] of uiState.contextMenus.entries()) {
    if (!menuState?.open) continue;

    const menu = new Container();
    menu.eventMode = 'static';
    menu.cursor = 'default';
    setDisplayPosition(menu, menuState.x, menuState.y);

    const itemW = 140;
    const itemH = 28;
    const pad = 6;
    const labels = ['Copy', 'Paste', 'Close'];

    const bg = new Graphics();
    bg.rect(0, 0, itemW + pad * 2, labels.length * itemH + pad * 2);
    bg.fill(0xffffff);
    // Owner-colored 2px border.
    const borderInset = 1;
    bg.rect(borderInset, borderInset, itemW + pad * 2 - borderInset * 2, labels.length * itemH + pad * 2 - borderInset * 2);
    bg.stroke({ width: 2, color: getCursorColor(ownerPid), alignment: 0 });
    menu.addChild(bg);

    labels.forEach((label, i) => {
      const y = pad + i * itemH;
      const hit = new Container();
      hit.eventMode = 'static';
      hit.cursor = 'pointer';
      setDisplayPosition(hit, pad, y);

      const gg = new Graphics();
      gg.rect(0, 0, itemW, itemH);
      gg.fill(0xffffff);
      hit.addChild(gg);

      const tt = makeThemedText({
        text: label,
        fontFamily: theme.fontFamily,
        fontSize: theme.fontSize,
        fill: theme.text,
      });
      setDisplayPosition(tt, 8, Math.max(0, (itemH - tt.height) / 2) + TEXT_BASELINE_NUDGE_Y);
      hit.addChild(tt);

      const isOwnerEvent = (ev: any) => getEffectivePointerId(ev) === ownerPid;

      hit.on('pointerover', (ev: any) => {
        if (!isOwnerEvent(ev)) return;
        gg.clear();
        gg.rect(0, 0, itemW, itemH);
        gg.fill(0xf2f2f2);
      });
      hit.on('pointerout', (ev: any) => {
        if (!isOwnerEvent(ev)) return;
        gg.clear();
        gg.rect(0, 0, itemW, itemH);
        gg.fill(0xffffff);
      });
      hit.on('pointerdown', (ev: any) => {
        if (!isOwnerEvent(ev)) return;
        ev.stopPropagation?.();

        const focusedKey = uiState.focusedKeyByPointer.get(ownerPid) ?? null;
        const focusedState = focusedKey ? uiState.inputs.get(focusedKey) : null;

        // Only allow Copy/Paste for text-like fields (<input>/<textarea>) that registered bounds this paint.
        const isTextField =
          focusedKey != null &&
          uiState.fieldBounds.has(focusedKey) &&
          focusedState != null &&
          typeof (focusedState as any).value === 'string';

        if (label === 'Copy' && isTextField) {
          const st = focusedState as any as { value: string; selections?: Map<number, { start: number; end: number }> };
          const full = st.value ?? '';
          const sel = st.selections?.get(ownerPid) ?? null;
          const a = sel ? Math.max(0, Math.min(full.length, sel.start ?? 0)) : 0;
          const b = sel ? Math.max(0, Math.min(full.length, sel.end ?? a)) : a;
          const start = Math.min(a, b);
          const end = Math.max(a, b);
          const picked = start !== end ? full.slice(start, end) : full;
          uiState.clipboards.set(ownerPid, picked);
        } else if (label === 'Paste' && isTextField) {
          const clip = uiState.clipboards.get(ownerPid) ?? '';
          if (clip.length > 0) {
            const st = focusedState as any as { value: string; selections?: Map<number, { start: number; end: number }> };
            const full = st.value ?? '';

            if (!st.selections) st.selections = new Map();
            if (!st.selections.has(ownerPid)) {
              const p = full.length;
              st.selections.set(ownerPid, { start: p, end: p });
            }
            const sel = st.selections.get(ownerPid)!;
            const a = Math.max(0, Math.min(full.length, sel.start ?? full.length));
            const b = Math.max(0, Math.min(full.length, sel.end ?? a));
            const start = Math.min(a, b);
            const end = Math.max(a, b);

            st.value = full.slice(0, start) + clip + full.slice(end);
            const caret = start + clip.length;
            sel.start = caret;
            sel.end = caret;
          }
        }

        // Close on selection (including Close).
        const st = uiState.contextMenus.get(ownerPid);
        if (st) {
          st.open = false;
          uiState.contextMenus.set(ownerPid, st);
        }
        requestPaint?.();
      });

      menu.addChild(hit);
    });

    overlayRoot.addChild(menu);
  }

  // Prune cached layout node containers that weren't visited this paint.
  setTrueosLayoutStep('render:prune-cache');
  for (const [k, c] of nodeCache.entries()) {
    if (usedNodeKeys.has(k)) continue;
    try {
      c.removeFromParent();
      (c as any).destroy?.({ children: true });
    } catch {
      // Best-effort.
    }
    nodeCache.delete(k);
  }

  setTrueosLayoutStep('render:done');

  // Retained-mode renderer: we keep a stable scene graph rooted at `stage`.
  // Do not clear or re-add `stage` (it may be `sceneRoot` itself).
}

async function main() {
  try {
  setTrueosPhase('main:start');
  const rootEl = document.getElementById('app') ?? document.body;
  const captureOnly = __TRUEOS_CAPTURE_BUILD__ || isTrueosCaptureOnly();
  setTrueosPhase('main:yoga');
  Yoga = captureOnly ? createCaptureOnlyYoga() : (await import('yoga-layout')).default;

  setTrueosPhase('main:create-app');
  const app = captureOnly ? createCaptureOnlyApplication() : new Application();
  if (!captureOnly) {
    await app.init({
      background: '#ffffff',
      resizeTo: window,
      antialias: false,
      preference: 'webgl',
    });
  }
  setTrueosPhase('main:attach-capture');
  attachPixiRenderCapture(app);
  window.__TRUEOS_PIXI_APP = app;
  setTrueosPhase('main:append-canvas');
  rootEl.appendChild(app.canvas);

  // We render on-demand (after state/layout changes) rather than continuously.
  // This saves substantial GPU time when the UI is static.
  app.ticker.stop();
  setTrueosPhase('main:capture-flags');
  if (captureOnly) {
    uiState.harness.enabled = false;
    uiState.virtualCursor.enabled = false;
    if (window.__pixiCapture) window.__pixiCapture.persist = false;
  }

  // Disable the browser context menu over the canvas.
  setTrueosPhase('main:canvas-listeners');
  app.canvas.addEventListener('contextmenu', (e) => e.preventDefault());

  // Wheel scroll: scene-level global scrollbar.
  app.canvas.addEventListener(
    'wheel',
    (e) => {
      const x = (e as any).offsetX ?? 0;
      const y = (e as any).offsetY ?? 0;

      // Deepest iframe under pointer wins.
      let iframeKey: string | null = null;
      for (let i = uiState.iframeRects.length - 1; i >= 0; i--) {
        const r = uiState.iframeRects[i];
        if (x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h) {
          iframeKey = r.key;
          break;
        }
      }

      if (iframeKey) {
        const st = uiState.iframeScroll.get(iframeKey);
        if (st) {
          const maxScroll = Math.max(0, st.contentHeight - st.viewportHeight);
          if (maxScroll > 0) {
            st.y = Math.max(0, Math.min(maxScroll, st.y + e.deltaY));
            uiState.iframeScroll.set(iframeKey, st);
            requestPaint?.();
            e.preventDefault();
          }
          return;
        }
      }

      const maxScroll = Math.max(0, uiState.scroll.contentHeight - uiState.scroll.viewportHeight);
      if (maxScroll <= 0) return;

      uiState.scroll.y = Math.max(0, Math.min(maxScroll, uiState.scroll.y + e.deltaY));
      requestPaint?.();
      e.preventDefault();
    },
    { passive: false }
  );

  // Make sure the stage participates in hit testing.
  setTrueosPhase('main:stage:eventMode');
  app.stage.eventMode = 'static';
  setTrueosPhase('main:stage:hitArea');
  app.stage.hitArea = app.screen;

  // Global context menu + "outside click" behavior.
  // This must be registered once (retained scene); widget handlers can stopPropagation.
  setTrueosPhase('main:stage:on:pointerdown');
  app.stage.on('pointerdown', (ev: any) => {
    if (ev?.button === 2) {
      const pid = getEffectivePointerId(ev);
      if (pid > 0) {
        const m = uiState.contextMenus.get(pid) ?? { open: false, x: 0, y: 0 };
        m.open = true;
        m.x = ev.global?.x ?? 0;
        m.y = ev.global?.y ?? 0;
        uiState.contextMenus.set(pid, m);
      }
      requestPaint?.();
      ev.preventDefault?.();
      return;
    }

    // Left click closes only THIS pointer's menu (clicks from other pointers don't dismiss it).
    if (ev?.button !== 2) {
      const pid = getEffectivePointerId(ev);
      const m = pid > 0 ? uiState.contextMenus.get(pid) : null;
      if (m && m.open) {
        m.open = false;
        uiState.contextMenus.set(pid, m);
        requestPaint?.();
      }
    }

    // Left click outside closes any open <select> popups.
    if (ev?.button !== 2) {
      let didClose = false;
      for (const st of uiState.selects.values()) {
        if (st.open) {
          st.open = false;
          didClose = true;
        }
      }
      if (didClose) requestPaint?.();
    }

    // Left click outside closes any open temporal pickers.
    if (ev?.button !== 2) {
      const didCloseTemporal = closeAllTemporalPopups(uiState.temporals);
      if (didCloseTemporal) requestPaint?.();
    }

    // Some interactions (e.g. hover/active fills) mutate Graphics directly; ensure we present.
    requestPresent();
  });
  setTrueosPhase('main:stage:done');

  setTrueosPhase('main:roots');
  const sceneRoot = new Container();
  const overlayUiRoot = new Container();
  overlayUiRoot.eventMode = 'static';
  // Overlay sits above the scene, but must not steal input.
  const overlayRoot = new Container();
  overlayRoot.eventMode = 'none';
  app.stage.addChild(sceneRoot);
  app.stage.addChild(overlayUiRoot);
  app.stage.addChild(overlayRoot);

  const scrollbarG = new Graphics();
  scrollbarG.label = '__trueosGlobalScrollbar';
  scrollbarG.eventMode = 'static';
  overlayUiRoot.addChild(scrollbarG);

  const buildCrossShape = (g: Graphics, opts: { half: number; strokeWidth: number; color: number }) => {
    g.clear();
    const { half, strokeWidth, color } = opts;
    g.moveTo(-half, 0);
    g.lineTo(half, 0);
    g.stroke({ width: strokeWidth, color });
    g.moveTo(0, -half);
    g.lineTo(0, half);
    g.stroke({ width: strokeWidth, color });
  };

  // Primary (real) cursor overlay.
  const mouseCursorG = new Graphics();
  mouseCursorG.eventMode = 'none';
  mouseCursorG.visible = false;
  overlayRoot.addChild(mouseCursorG);

  const cursor3G = new Graphics();
  cursor3G.eventMode = 'none';
  cursor3G.visible = false;
  overlayRoot.addChild(cursor3G);

  const cursor4G = new Graphics();
  cursor4G.eventMode = 'none';
  cursor4G.visible = false;
  overlayRoot.addChild(cursor4G);

  const virtualCursorG = new Graphics();
  virtualCursorG.eventMode = 'none';
  overlayRoot.addChild(virtualCursorG);

  setTrueosPhase('main:text-measure');
  const dragMeasureCanvas = document.createElement('canvas');
  const dragMeasureCtx = dragMeasureCanvas.getContext('2d');
  if (!dragMeasureCtx) throw new Error('2D canvas not available');
  dragMeasureCtx.font = `${defaultTheme.fontSize}px ${defaultTheme.fontFamily}`;
  const dragMeasure = (s: string) => dragMeasureCtx.measureText(s).width;
  const dragLineHeight = defaultTheme.fontSize * 1.25;

  setTrueosPhase('main:html');
  const html =
    typeof window.__TRUEOS_INPUT_HTML__ === 'string'
      ? window.__TRUEOS_INPUT_HTML__
      : await fetch('/input.html').then((r) => r.text());
  if (isTrueosCaptureOnly()) {
    console.log(
      `[trueos pixi widgets] input-html chars=${html.length} sample="${sampleRawTextForLog(html)}"`
    );
  }

  setTrueosPhase('main:render-tree');
  trueosIframeSrcdocRowsByKey.clear();
  const renderNodes = normalizePublishedRenderTree(window.__TRUEOS_WIDGET_RENDER_TREE__);
  if (isTrueosCaptureOnly()) {
    console.log(
      `[trueos pixi widgets] render-tree source=truesurfer nodes=${renderNodes.length}`
    );
  }
  if (renderNodes.length === 0) {
    throw new Error('TrueSurfer widget render tree is missing');
  }
  const renderTreeStats = summarizeRenderNodes(renderNodes);

  let lastLayout: LayoutBox | null = null;
  let lastLayoutStats: TrueosTreeStats = { nodes: 0, blocks: 0, text: 0, maxDepth: 0, tags: {} };
  let trueosLayoutOverlayLogCount = 0;

  const clampScroll = () => {
    const maxScroll = Math.max(0, uiState.scroll.contentHeight - uiState.scroll.viewportHeight);
    uiState.scroll.y = Math.max(0, Math.min(uiState.scroll.y, maxScroll));
  };

  const updateScrollbarVisuals = () => {
    const vw = app.renderer.width;
    const vh = app.renderer.height;
    uiState.scroll.viewportHeight = vh;

    const contentH = uiState.scroll.contentHeight;
    const maxScroll = Math.max(0, contentH - vh);
    const show = maxScroll > 0.5;

    scrollbarG.clear();
    scrollbarG.visible = show;
    if (!show) {
      uiState.scroll.track = { x: 0, y: 0, w: uiState.scroll.track.w, h: 0 };
      uiState.scroll.thumb = { x: 0, y: 0, w: uiState.scroll.thumb.w, h: 0 };
      return;
    }

    const pad = SCROLLBAR_PAD;
    const trackW = SCROLLBAR_W;
    const trackX = Math.max(0, vw - trackW - pad);
    const trackY = pad;
    const trackH = Math.max(0, vh - pad * 2);

    const minThumbH = 24;
    const thumbH = Math.max(minThumbH, (vh / Math.max(vh, contentH)) * trackH);
    const travel = Math.max(1, trackH - thumbH);
    const ratio = maxScroll <= 0 ? 0 : uiState.scroll.y / maxScroll;
    const thumbY = trackY + travel * ratio;

    uiState.scroll.track = { x: trackX, y: trackY, w: trackW, h: trackH };
    uiState.scroll.thumb = { x: trackX, y: thumbY, w: trackW, h: thumbH };

    // Track
    scrollbarG.rect(trackX, trackY, trackW, trackH);
    scrollbarG.fill({ color: 0x000000, alpha: 0.06 });

    // Thumb
    scrollbarG.rect(trackX, thumbY, trackW, thumbH);
    scrollbarG.fill({ color: 0x000000, alpha: 0.25 });
  };

  const paint = () => {
    if (!lastLayout) return;
    setTrueosPhase('main:paint:clamp');
    clampScroll();
    setTrueosPhase('main:paint:render-to-pixi');
    renderToPixi(app, lastLayout, sceneRoot);
    setTrueosPhase('main:paint:scrollbar');
    updateScrollbarVisuals();
    // Manual render (ticker is stopped).
    setTrueosPhase('main:paint:renderer-render');
    app.renderer.render(app.stage);
    publishTrueosBridgeStats(
      renderTreeStats,
      lastLayoutStats,
      summarizeRenderTextSamples(renderNodes),
      summarizeLayoutTextSamples(lastLayout)
    );
    if (isTrueosCaptureOnly()) {
      const overlays = collectLayoutTextOverlays(lastLayout);
      window.__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__ = overlays;
      if (trueosLayoutOverlayLogCount < 4) {
        trueosLayoutOverlayLogCount += 1;
        console.log(
          `[trueos pixi widgets] layout-text-overlays count=${overlays.length} samples=${summarizeTrueosLayoutTextOverlays(overlays)}`
        );
      }
    }
    setTrueosPhase('main:paint:done');
  };

  if (isTrueosCaptureOnly()) {
    window.__TRUEOS_REPAINT_NOW__ = () => {
      window.__TRUEOS_PIXI_DIRTY__ = false;
      paint();
    };
  }

  const rerender = () => {
    setTrueosPhase('main:layout-build');
    const layout = buildLayoutTree(renderNodes, window.innerWidth, window.innerHeight);
    setTrueosPhase('main:layout-commit');
    lastLayout = layout;
    if (isTrueosCaptureOnly()) {
      window.__TRUEOS_PIXI_LAST_LAYOUT__ = layout;
      window.__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__ = [];
    }
    lastLayoutStats = summarizeLayoutBoxes(layout);
    uiState.scroll.contentHeight = computeScrollableContentHeight(layout);
    uiState.scroll.viewportHeight = window.innerHeight;
    paint();
  };

  requestRerender = () => {
    void rerender();
  };

  // Coalesce paints to at most once per frame.
  let paintScheduled = false;
  let presentScheduled = false;
  const requestPresent = () => {
    if (isTrueosCaptureOnly()) {
      window.__TRUEOS_PIXI_DIRTY__ = true;
      return;
    }
    if (presentScheduled || paintScheduled) return;
    presentScheduled = true;
    requestAnimationFrame(() => {
      presentScheduled = false;
      app.renderer.render(app.stage);
    });
  };

  requestPaint = () => {
    if (paintScheduled) return;
    if (isTrueosCaptureOnly()) {
      window.__TRUEOS_PIXI_DIRTY__ = true;
      return;
    }
    paintScheduled = true;
    requestAnimationFrame(() => {
      paintScheduled = false;
      paint();
    });
  };

  setTrueosPhase('main:first-rerender');
  rerender();
  setTrueosPhase('main:cursor-setup');

  // Cursor style shared between real + virtual cursor.
  // <details> chevron uses a 2px stroke; match that.
  const CURSOR_STROKE = 2;
  // Double the previous arm length.
  const CURSOR_HALF = 10;
  const trueosKernelCursorMode = isTrueosCaptureOnly();
  buildCrossShape(mouseCursorG, { half: CURSOR_HALF, strokeWidth: CURSOR_STROKE, color: getCursorColor(USER_POINTER_ID) });
  buildCrossShape(cursor3G, { half: CURSOR_HALF, strokeWidth: CURSOR_STROKE, color: getCursorColor(USER_POINTER_ID_3) });
  buildCrossShape(cursor4G, { half: CURSOR_HALF, strokeWidth: CURSOR_STROKE, color: getCursorColor(USER_POINTER_ID_4) });
  // Virtual cursor uses a distinct id so it can have a distinct color.
  const VIRTUAL_POINTER_ID = 2;
  buildCrossShape(virtualCursorG, { half: CURSOR_HALF, strokeWidth: CURSOR_STROKE, color: getCursorColor(VIRTUAL_POINTER_ID) });

  // Seed positions so both cursors can be visible immediately.
  uiState.userCursorPos.set(USER_POINTER_ID, { x: app.renderer.width * 0.25, y: app.renderer.height * 0.5 });
  uiState.userCursorPos.set(USER_POINTER_ID_3, { x: app.renderer.width * 0.25 + 40, y: app.renderer.height * 0.5 + 20 });
  uiState.userCursorPos.set(USER_POINTER_ID_4, { x: app.renderer.width * 0.25 + 80, y: app.renderer.height * 0.5 + 40 });

  mouseCursorG.visible = !trueosKernelCursorMode;
  cursor3G.visible = !trueosKernelCursorMode;
  cursor4G.visible = !trueosKernelCursorMode;
  if (!trueosKernelCursorMode) {
    const p1 = uiState.userCursorPos.get(USER_POINTER_ID)!;
    const p3 = uiState.userCursorPos.get(USER_POINTER_ID_3)!;
    const p4 = uiState.userCursorPos.get(USER_POINTER_ID_4)!;
    mouseCursorG.position.set(p1.x, p1.y);
    cursor3G.position.set(p3.x, p3.y);
    cursor4G.position.set(p4.x, p4.y);
  }

  virtualCursorG.visible = !trueosKernelCursorMode && uiState.virtualCursor.enabled;

  const updateUserCursorOverlays = () => {
    if (trueosKernelCursorMode) {
      mouseCursorG.visible = false;
      cursor3G.visible = false;
      cursor4G.visible = false;
      virtualCursorG.visible = false;
      return;
    }

    // Keep overlays and hover state in sync with uiState.userCursorPos.
    const p1 = uiState.userCursorPos.get(USER_POINTER_ID);
    const p3 = uiState.userCursorPos.get(USER_POINTER_ID_3);
    const p4 = uiState.userCursorPos.get(USER_POINTER_ID_4);

    if (p1) {
      mouseCursorG.visible = true;
      mouseCursorG.position.set(p1.x, p1.y);
    }
    if (p3) {
      cursor3G.visible = true;
      cursor3G.position.set(p3.x, p3.y);
    }
    if (p4) {
      cursor4G.visible = true;
      cursor4G.position.set(p4.x, p4.y);
    }

    const findHover = (x: number, y: number) => {
      let hitKey: string | null = null;
      let hitCursor: 'text' | 'pointer' | 'move' | null = null;
      for (let i = uiState.hoverRects.length - 1; i >= 0; i--) {
        const rct = uiState.hoverRects[i];
        if (x >= rct.x && x <= rct.x + rct.w && y >= rct.y && y <= rct.y + rct.h) {
          hitKey = rct.key;
          hitCursor = rct.cursor;
          break;
        }
      }
      return { hitKey, hitCursor };
    };

    if (p1) {
      const { hitKey, hitCursor } = findHover(p1.x, p1.y);
      uiState.hoveredKeyByPointer.set(USER_POINTER_ID, hitKey);
      uiState.hoveredCursorByPointer.set(USER_POINTER_ID, hitCursor);
      const isActive =
        uiState.textDrags.has(USER_POINTER_ID) || uiState.sliderDrags.has(USER_POINTER_ID) || uiState.dialogDrags.has(USER_POINTER_ID);
      mouseCursorG.rotation = hitCursor != null || isActive ? Math.PI / 4 : 0;
    }

    if (p3) {
      const { hitKey, hitCursor } = findHover(p3.x, p3.y);
      uiState.hoveredKeyByPointer.set(USER_POINTER_ID_3, hitKey);
      uiState.hoveredCursorByPointer.set(USER_POINTER_ID_3, hitCursor);
      const isActive =
        uiState.textDrags.has(USER_POINTER_ID_3) || uiState.sliderDrags.has(USER_POINTER_ID_3) || uiState.dialogDrags.has(USER_POINTER_ID_3);
      cursor3G.rotation = hitCursor != null || isActive ? Math.PI / 4 : 0;
    }

    if (p4) {
      const { hitKey, hitCursor } = findHover(p4.x, p4.y);
      uiState.hoveredKeyByPointer.set(USER_POINTER_ID_4, hitKey);
      uiState.hoveredCursorByPointer.set(USER_POINTER_ID_4, hitCursor);
      const isActive =
        uiState.textDrags.has(USER_POINTER_ID_4) || uiState.sliderDrags.has(USER_POINTER_ID_4) || uiState.dialogDrags.has(USER_POINTER_ID_4);
      cursor4G.rotation = hitCursor != null || isActive ? Math.PI / 4 : 0;
    }

    // Cursor overlays and hover-driven visuals update without needing a full paint traversal.
    requestPresent();
  };

  // Multi-cursor harness: cycle mouse control between cursor 1, cursor 3, cursor 4.
  // When enabled, this runs a periodic timer.
  if (uiState.harness.enabled) {
    setInterval(() => {
      const prev = uiState.harness.activeUserPointerId;
      const next = prev === USER_POINTER_ID ? USER_POINTER_ID_3 : prev === USER_POINTER_ID_3 ? USER_POINTER_ID_4 : USER_POINTER_ID;
      uiState.harness.activeUserPointerId = next;

    // Make the toggle visible immediately even if the real mouse hasn't moved:
    // snap the newly-controlled cursor to the last known mouse position and
    // move the previously-controlled cursor back to where the new cursor was.
    if (uiState.lastMouse.has) {
      const prevPos = uiState.userCursorPos.get(prev);
      const nextPos = uiState.userCursorPos.get(next);

      uiState.userCursorPos.set(next, { x: uiState.lastMouse.x, y: uiState.lastMouse.y });
      if (nextPos) uiState.userCursorPos.set(prev, { x: nextPos.x, y: nextPos.y });
      else if (prevPos) uiState.userCursorPos.set(prev, { x: prevPos.x, y: prevPos.y });
    }

      // If we cancel an active drag/scroll, the main scene visuals can change, so repaint.
      const hadTextDrag = uiState.textDrags.size > 0;
      const hadSliderDrag = uiState.sliderDrags.size > 0;
      const hadDialogDrag = uiState.dialogDrags.size > 0;
      const hadScrollDrag = uiState.scroll.draggingPointerId != null;
      const hadColorDrag = uiState.color.draggingPointerId != null;
      let hadIframeDrag = false;
      for (const st of uiState.iframeScroll.values()) {
        if (st.draggingPointerId != null) {
          hadIframeDrag = true;
          break;
        }
      }
      const needsRepaint = hadTextDrag || hadSliderDrag || hadDialogDrag || hadScrollDrag || hadColorDrag || hadIframeDrag;

      // Avoid stuck drags when control flips mid-gesture.
      uiState.textDrags.delete(USER_POINTER_ID);
      uiState.textDrags.delete(USER_POINTER_ID_3);
      uiState.textDrags.delete(USER_POINTER_ID_4);
      uiState.sliderDrags.delete(USER_POINTER_ID);
      uiState.sliderDrags.delete(USER_POINTER_ID_3);
      uiState.sliderDrags.delete(USER_POINTER_ID_4);
      uiState.dialogDrags.delete(USER_POINTER_ID);
      uiState.dialogDrags.delete(USER_POINTER_ID_3);
      uiState.dialogDrags.delete(USER_POINTER_ID_4);

    // Avoid stuck number holds.
    for (const pid of [USER_POINTER_ID, USER_POINTER_ID_3, USER_POINTER_ID_4]) {
      const h = uiState.numberHolds.get(pid);
      if (h) {
        if (h.timeoutId != null) window.clearTimeout(h.timeoutId);
        if (h.intervalId != null) window.clearInterval(h.intervalId);
        uiState.numberHolds.delete(pid);
      }
    }
    if (
      uiState.scroll.draggingPointerId === USER_POINTER_ID ||
      uiState.scroll.draggingPointerId === USER_POINTER_ID_3 ||
      uiState.scroll.draggingPointerId === USER_POINTER_ID_4
    ) {
      uiState.scroll.draggingPointerId = null;
    }

    if (
      uiState.color.draggingPointerId === USER_POINTER_ID ||
      uiState.color.draggingPointerId === USER_POINTER_ID_3 ||
      uiState.color.draggingPointerId === USER_POINTER_ID_4
    ) {
      uiState.color.draggingPointerId = null;
    }

      updateUserCursorOverlays();
      if (needsRepaint) requestPaint?.();
    }, uiState.harness.periodMs);
  }

  // Virtual input device cursor: simple patrol (circle).
  // Disabled by default; when disabled we avoid per-frame ticker work.
  if (!trueosKernelCursorMode && uiState.virtualCursor.enabled) {
    app.ticker.add(() => {
      const dt = Math.max(0, app.ticker.deltaMS) / 1000;

      virtualCursorG.visible = true;

      uiState.virtualCursor.t += dt;

    const cx = app.renderer.width * 0.75;
    const cy = app.renderer.height * 0.25;
    const a = uiState.virtualCursor.t * uiState.virtualCursor.speed;
    const r = uiState.virtualCursor.radius;
    uiState.virtualCursor.x = cx + Math.cos(a) * r;
    uiState.virtualCursor.y = cy + Math.sin(a) * r;

    virtualCursorG.position.set(uiState.virtualCursor.x, uiState.virtualCursor.y);

    // Virtual hover simulation.
    {
      const pid = VIRTUAL_POINTER_ID;
      const x = uiState.virtualCursor.x;
      const y = uiState.virtualCursor.y;
      let hitKey: string | null = null;
      let hitCursor: 'text' | 'pointer' | 'move' | null = null;

      // Iterate from end so later-drawn widgets win.
      for (let i = uiState.hoverRects.length - 1; i >= 0; i--) {
        const rct = uiState.hoverRects[i];
        if (x >= rct.x && x <= rct.x + rct.w && y >= rct.y && y <= rct.y + rct.h) {
          hitKey = rct.key;
          hitCursor = rct.cursor;
          break;
        }
      }

      const prev = uiState.hoveredKeyByPointer.get(pid) ?? null;
      if (prev !== hitKey) {
        if (prev) uiState.hoverHandlers.get(prev)?.out?.();
        if (hitKey) uiState.hoverHandlers.get(hitKey)?.over?.();
        uiState.hoveredKeyByPointer.set(pid, hitKey);
      }

      uiState.hoveredCursorByPointer.set(pid, hitCursor);

      const isActive = uiState.textDrags.has(pid) || uiState.sliderDrags.has(pid) || uiState.dialogDrags.has(pid);
      virtualCursorG.rotation = hitCursor != null || isActive ? Math.PI / 4 : 0;
    }

    });
  }

  // Initial cursor draw.
  uiState.virtualCursor.x = app.renderer.width * 0.75 + uiState.virtualCursor.radius;
  uiState.virtualCursor.y = app.renderer.height * 0.25;
  virtualCursorG.position.set(uiState.virtualCursor.x, uiState.virtualCursor.y);
  if (isTrueosCaptureOnly()) {
    paint();
  }

  // Mouse drag selection for <input>/<textarea>.
  // Also used for slider drag, dialog drag, and scrollbar thumb drag.
  app.stage.on('pointerup', (ev: any) => {
    const pid = getEffectivePointerId(ev);
    const releasedSliderKey = uiState.sliderDrags.get(pid)?.key ?? null;
    uiState.textDrags.delete(pid);
    uiState.sliderDrags.delete(pid);
    uiState.dialogDrags.delete(pid);
    if (uiState.scroll.draggingPointerId === pid) uiState.scroll.draggingPointerId = null;
    if (uiState.color.draggingPointerId === pid) uiState.color.draggingPointerId = null;

    for (const st of uiState.iframeScroll.values()) {
      if (st.draggingPointerId === pid) st.draggingPointerId = null;
    }

    // End number spinner hold-repeat.
    {
      const h = uiState.numberHolds.get(pid);
      if (h) {
        if (h.timeoutId != null) window.clearTimeout(h.timeoutId);
        if (h.intervalId != null) window.clearInterval(h.intervalId);
        uiState.numberHolds.delete(pid);
      }
    }

    // Year widget: close on slider release.
    if (releasedSliderKey) {
      const ownerTemporalKey = uiState.temporalYearOwners.get(releasedSliderKey) ?? null;
      if (ownerTemporalKey) {
        const t = uiState.temporals.get(ownerTemporalKey);
        if (t && t.openYear) {
          t.openYear = false;
          uiState.temporals.set(ownerTemporalKey, t);
          requestPaint?.();
        }
      }
    }

    // Ensure pointerup-driven visuals (e.g. button state) are presented.
    requestPresent();
  });
  app.stage.on('pointerupoutside', (ev: any) => {
    const pid = getEffectivePointerId(ev);
    const releasedSliderKey = uiState.sliderDrags.get(pid)?.key ?? null;
    uiState.textDrags.delete(pid);
    uiState.sliderDrags.delete(pid);
    uiState.dialogDrags.delete(pid);
    if (uiState.scroll.draggingPointerId === pid) uiState.scroll.draggingPointerId = null;
    if (uiState.color.draggingPointerId === pid) uiState.color.draggingPointerId = null;

    for (const st of uiState.iframeScroll.values()) {
      if (st.draggingPointerId === pid) st.draggingPointerId = null;
    }

    // End number spinner hold-repeat.
    {
      const h = uiState.numberHolds.get(pid);
      if (h) {
        if (h.timeoutId != null) window.clearTimeout(h.timeoutId);
        if (h.intervalId != null) window.clearInterval(h.intervalId);
        uiState.numberHolds.delete(pid);
      }
    }

    // Year widget: close on slider release.
    if (releasedSliderKey) {
      const ownerTemporalKey = uiState.temporalYearOwners.get(releasedSliderKey) ?? null;
      if (ownerTemporalKey) {
        const t = uiState.temporals.get(ownerTemporalKey);
        if (t && t.openYear) {
          t.openYear = false;
          uiState.temporals.set(ownerTemporalKey, t);
          requestPaint?.();
        }
      }
    }

    requestPresent();
  });

  // Thumb drag start (last cursor wins).
  scrollbarG.on('pointerdown', (ev: any) => {
    if (ev?.button === 2) return;
    const pid = getEffectivePointerId(ev);
    if (pid <= 0) return;

    const gx = ev.global?.x ?? 0;
    const gy = ev.global?.y ?? 0;

    const track = uiState.scroll.track;
    const th = uiState.scroll.thumb;
    const hitTrack = gx >= track.x && gx <= track.x + track.w && gy >= track.y && gy <= track.y + track.h;
    if (!hitTrack) return;

    const maxScroll = Math.max(0, uiState.scroll.contentHeight - uiState.scroll.viewportHeight);
    if (maxScroll <= 0.5) return;

    const hitThumb = gx >= th.x && gx <= th.x + th.w && gy >= th.y && gy <= th.y + th.h;
    if (hitThumb) {
      uiState.scroll.draggingPointerId = pid;
      uiState.scroll.dragOffsetY = gy - th.y;
      ev.stopPropagation?.();
      return;
    }

    // Track click: jump the thumb (centered on the pointer) and start dragging.
    const travel = Math.max(1, track.h - th.h);
    const targetTop = Math.max(track.y, Math.min(track.y + travel, gy - th.h / 2));
    const ratio = (targetTop - track.y) / travel;
    uiState.scroll.y = Math.max(0, Math.min(maxScroll, ratio * maxScroll));

    uiState.scroll.draggingPointerId = pid;
    uiState.scroll.dragOffsetY = gy - targetTop;
    requestPaint?.();
    ev.stopPropagation?.();
  });
  app.stage.on('pointermove', (ev: any) => {
    // Track the primary (real) cursor separately from drag-selection pointers.
    // Prefer pointerType when available; fallback to pointerId==1 (typical mouse).
    const pidAny = Number(ev?.pointerId ?? ev?.data?.pointerId ?? 1);
    const pt = String(ev?.pointerType ?? ev?.data?.pointerType ?? '').toLowerCase();
    const isMouse = pt === 'mouse' || pidAny === 1;

    if (isMouse) {
      const gx = ev.global?.x ?? 0;
      const gy = ev.global?.y ?? 0;

      uiState.lastMouse.x = gx;
      uiState.lastMouse.y = gy;
      uiState.lastMouse.has = true;
      uiState.primaryMousePointerId = pidAny;

      // Update the stored position for whichever user cursor the harness says is under mouse control.
      const controlPid = uiState.harness.enabled ? uiState.harness.activeUserPointerId : pidAny;
      uiState.userCursorPos.set(controlPid, { x: gx, y: gy });

      // Keep overlays/hover in sync as the real mouse moves.
      updateUserCursorOverlays();
    }

    const pid = getEffectivePointerId(ev);
    if (pid <= 0) return;
    let didUpdate = false;

    // Text selection drag.
    {
      const drag = uiState.textDrags.get(pid);
      if (drag) {
        const key = drag.key;
        const bounds = uiState.fieldBounds.get(key);
        const state = uiState.inputs.get(key);
        if (bounds && state && typeof state.value === 'string') {
          const shown = bounds.isPassword ? '•'.repeat(state.value.length) : state.value;
          const lines = clampWrappedLines(
            wrapFieldTextWithIndices(shown, Math.max(0, bounds.innerWidth), dragMeasure),
            bounds.maxLines
          );

          const localX = (ev.global?.x ?? 0) - bounds.x - bounds.innerLeft;
          const localY = (ev.global?.y ?? 0) - bounds.y - bounds.innerTop;
          const idx = getCaretIndexFromPoint({
            fullText: shown,
            lines,
            localX,
            localY,
            lineHeight: dragLineHeight,
            measure: dragMeasure,
          });

          if (!state.selections) state.selections = new Map();
          state.selections.set(pid, { start: drag.anchor, end: idx });
          didUpdate = true;
        }
      }
    }

    // Slider drag.
    {
      const drag = uiState.sliderDrags.get(pid);
      if (drag) {
        const key = drag.key;
        const b = uiState.sliderBounds.get(key);
        if (b) {
          const gx = ev.global?.x ?? 0;
          const localX = gx - b.x;
          const innerW2 = Math.max(1, b.w - b.innerPad * 2);
          const r = (localX - b.innerPad) / innerW2;
          const s = widgetGetOrInitSliderState(uiState.sliders, key, undefined);
          s.value = Math.max(0, Math.min(1, r));
          didUpdate = true;
        }
      }
    }

    // Color picker drag.
    {
      const dragPid = uiState.color.draggingPointerId;
      if (dragPid != null && dragPid === pid) {
        const b = uiState.color.bounds;
        if (b) {
          const gx = ev.global?.x ?? 0;
          const gy = ev.global?.y ?? 0;
          const lx = gx - b.x;
          const ly = gy - b.y;
          const s = sampleColorPickerAtLocal({ lx, ly, w: b.w, h: b.h });
          if (s) {
            uiState.color.rgb = s;
            uiState.color.pick = { x: lx, y: ly };
            didUpdate = true;
          }
        }
      }
    }

    // Dialog drag.
        // Scrollbar thumb drag.
        {
          const dragPid = uiState.scroll.draggingPointerId;
          if (dragPid != null && dragPid === pid) {
            const track = uiState.scroll.track;
            const thumb = uiState.scroll.thumb;
            const maxScroll = Math.max(0, uiState.scroll.contentHeight - uiState.scroll.viewportHeight);

            if (maxScroll > 0.5 && track.h > 0 && thumb.h > 0) {
              const gy = ev.global?.y ?? 0;
              const travel = Math.max(1, track.h - thumb.h);
              const top = Math.max(track.y, Math.min(track.y + travel, gy - uiState.scroll.dragOffsetY));
              const ratio = (top - track.y) / travel;
              uiState.scroll.y = Math.max(0, Math.min(maxScroll, ratio * maxScroll));
              didUpdate = true;
            }
          }
        }

        // Iframe scrollbar thumb drag.
        {
          for (const st of uiState.iframeScroll.values()) {
            if (st.draggingPointerId == null || st.draggingPointerId !== pid) continue;

            const maxScroll = Math.max(0, st.contentHeight - st.viewportHeight);
            if (maxScroll <= 0.5 || st.track.h <= 0 || st.thumb.h <= 0) continue;

            const gy = ev.global?.y ?? 0;
            const travel = Math.max(1, st.track.h - st.thumb.h);
            const top = Math.max(st.track.y, Math.min(st.track.y + travel, gy - st.dragOffsetY));
            const ratio = (top - st.track.y) / travel;
            st.y = Math.max(0, Math.min(maxScroll, ratio * maxScroll));
            didUpdate = true;
          }
        }
    {
      const drag = uiState.dialogDrags.get(pid);
      if (drag) {
        const st = getOrInitDialogState(uiState.dialogs, drag.key);
        const gx = ev.global?.x ?? 0;
        const gy = ev.global?.y ?? 0;
        st.x = drag.originX + (gx - drag.startGX);
        st.y = drag.originY + (gy - drag.startGY);

        // Clamp to the most recently computed bounds for this dialog's stacking context.
        const b = uiState.dialogDragBounds.get(drag.key);
        if (b) {
          st.x = Math.max(b.minX, Math.min(b.maxX, st.x));
          st.y = Math.max(b.minY, Math.min(b.maxY, st.y));
        }
        didUpdate = true;
      }
    }

    if (didUpdate) requestPaint?.();
  });

  setTrueosPhase('main:input-listeners');
  // Keyboard input: very lightweight text editing for focused <input type=text|password>.
  window.addEventListener('keydown', (ev) => {
    const pid = uiState.keyboardOwnerPointerId;
    const key = uiState.focusedKeyByPointer.get(pid) ?? null;
    if (!key) return;

    const state = uiState.inputs.get(key);
    if (!state) return;

    // Only text-like inputs have a value.
    if (typeof state.value !== 'string') return;

    // Selection helpers (keyboard focus pointer only)
    if (!state.selections) state.selections = new Map();
    if (!state.selections.has(pid)) {
      const p = state.value.length;
      state.selections.set(pid, { start: p, end: p });
    }

    const sel = state.selections.get(pid)!;
    const len = state.value.length;
    const clampPos = (n: number) => Math.max(0, Math.min(len, n));
    const a0 = clampPos(sel.start ?? len);
    const b0 = clampPos(sel.end ?? a0);
    sel.start = a0;
    sel.end = b0;

    const start0 = Math.min(a0, b0);
    const end0 = Math.max(a0, b0);
    const hasSel = start0 !== end0;
    const setCaret = (pos: number) => {
      const p = Math.max(0, Math.min(state.value!.length, pos));
      sel.start = p;
      sel.end = p;
    };
    const setSelection = (start: number, end: number) => {
      sel.start = Math.max(0, Math.min(state.value!.length, start));
      sel.end = Math.max(0, Math.min(state.value!.length, end));
    };

    if (ev.key.toLowerCase() === 'a' && (ev.ctrlKey || ev.metaKey)) {
      setSelection(0, state.value.length);
      ev.preventDefault();
      paint();
      return;
    }

    if (ev.key === 'ArrowLeft' || ev.key === 'ArrowRight') {
      const dir = ev.key === 'ArrowLeft' ? -1 : 1;
      if (ev.shiftKey) {
        const anchor = sel.start ?? len;
        const focus = (sel.end ?? anchor) + dir;
        setSelection(anchor, focus);
      } else {
        const caret = hasSel ? start0 : end0;
        setCaret(caret + dir);
      }
      ev.preventDefault();
      void rerender();
      return;
    }

    if (ev.key === 'Home') {
      if (ev.shiftKey) setSelection(sel.start ?? len, 0);
      else setCaret(0);
      ev.preventDefault();
      void rerender();
      return;
    }
    if (ev.key === 'End') {
      if (ev.shiftKey) setSelection(sel.start ?? 0, state.value.length);
      else setCaret(state.value.length);
      ev.preventDefault();
      void rerender();
      return;
    }

    if (ev.key === 'Backspace') {
      if (hasSel) {
        state.value = state.value.slice(0, start0) + state.value.slice(end0);
        setCaret(start0);
      } else {
        const caret = end0;
        if (caret > 0) {
          state.value = state.value.slice(0, caret - 1) + state.value.slice(caret);
          setCaret(caret - 1);
        }
      }
      ev.preventDefault();
      void rerender();
      return;
    }
    if (ev.key === 'Enter') {
      // Multiline editing for demo: Enter inserts a newline.
      const insert = '\n';
      if (hasSel) {
        state.value = state.value.slice(0, start0) + insert + state.value.slice(end0);
        setCaret(start0 + insert.length);
      } else {
        const caret = end0;
        state.value = state.value.slice(0, caret) + insert + state.value.slice(caret);
        setCaret(caret + insert.length);
      }
      ev.preventDefault();
      void rerender();
      return;
    }
    if (ev.key === 'Delete') {
      if (hasSel) {
        state.value = state.value.slice(0, start0) + state.value.slice(end0);
        setCaret(start0);
      } else {
        const caret = end0;
        if (caret < state.value.length) {
          state.value = state.value.slice(0, caret) + state.value.slice(caret + 1);
          setCaret(caret);
        }
      }
      ev.preventDefault();
      void rerender();
      return;
    }
    if (ev.key === 'Escape') {
      uiState.focusedKeyByPointer.set(pid, null);
      void rerender();
      return;
    }
    if (ev.key.length === 1 && !ev.ctrlKey && !ev.metaKey && !ev.altKey) {
      if (hasSel) {
        state.value = state.value.slice(0, start0) + ev.key + state.value.slice(end0);
        setCaret(start0 + 1);
      } else {
        const caret = end0;
        state.value = state.value.slice(0, caret) + ev.key + state.value.slice(caret);
        setCaret(caret + 1);
      }
      ev.preventDefault();
      void rerender();
    }
  });

  window.addEventListener('resize', () => {
    void rerender();
    // Cursor is animated by ticker; ensure it stays visible immediately after resize.
    virtualCursorG.visible = uiState.virtualCursor.enabled;
  });
  setTrueosPhase('main:done');
  if (captureOnly) {
    window.__TRUEOS_PIXI_APP_READY__ = true;
    window.__TRUEOS_PIXI_APP_ERROR__ = '';
    window.__TRUEOS_PIXI_APP_PHASE__ = 'ready';
  }
  } catch (err) {
    window.__TRUEOS_PIXI_APP_READY__ = false;
    window.__TRUEOS_PIXI_APP_ERROR__ = describeStartupError(err);
    try {
      console.error(err);
    } catch {
      // Best-effort diagnostics only.
    }
    try {
      const pre = document.createElement('pre');
      pre.textContent = String((err as any)?.stack ?? err);
      document.body.appendChild(pre);
    } catch {
      // Capture hosts may have a tiny DOM shim.
    }
  }
}

main()
  .then(() => {
    if (window.__TRUEOS_PIXI_APP_ERROR__) return;
    window.__TRUEOS_PIXI_APP_READY__ = true;
    window.__TRUEOS_PIXI_APP_ERROR__ = '';
    window.__TRUEOS_PIXI_APP_PHASE__ = 'ready';
  })
  .catch((err) => {
    window.__TRUEOS_PIXI_APP_READY__ = false;
    window.__TRUEOS_PIXI_APP_ERROR__ = describeStartupError(err);
    console.error(err);
    const pre = document.createElement('pre');
    pre.textContent = String(err?.stack ?? err);
    document.body.appendChild(pre);
  });
