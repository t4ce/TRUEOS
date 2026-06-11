export const DEFAULT_TEXT_FIELD_LAYOUT = Object.freeze({
  leftPad: 8,
  topPad: 6,
  height: 36,
  minWidth: 220,
  maxLines: 5,
  lineHeightMultiplier: 1.25,
});

export function hasOwn(object, key) {
  return Object.prototype.hasOwnProperty.call(object, key);
}

export function normalizeAttrs(attrs = {}) {
  if (Array.isArray(attrs)) {
    const out = {};
    for (const attr of attrs) {
      if (!attr || attr.name == null) continue;
      out[String(attr.name)] = attr.value ?? '';
    }
    return out;
  }

  return attrs && typeof attrs === 'object' ? attrs : {};
}

export function normalizeToken(value, fallback = '') {
  const token = String(value ?? fallback)
    .trim()
    .toLowerCase();
  return token.length > 0 ? token : String(fallback).toLowerCase();
}

export function flagAttr(attrs = {}, name) {
  const source = normalizeAttrs(attrs);
  return hasOwn(source, name) && source[name] !== false && source[name] !== 'false';
}

export function toFiniteNumber(value, fallback = 0) {
  const n = Number(value);
  return Number.isFinite(n) ? n : fallback;
}

export function clampNumber(value, min = 0, max = 1) {
  const n = toFiniteNumber(value, min);
  const lo = toFiniteNumber(min, 0);
  const hi = toFiniteNumber(max, lo);
  if (hi < lo) return lo;
  return Math.max(lo, Math.min(hi, n));
}

export function nodeChildren(node) {
  if (!node || typeof node !== 'object') return [];
  if (Array.isArray(node.children)) return node.children;
  if (Array.isArray(node.childNodes)) return node.childNodes;
  return [];
}

export function textFromNode(node) {
  if (typeof node === 'string') return node;
  if (!node || typeof node !== 'object') return '';
  if (node.text != null) return String(node.text);
  if (node.textContent != null) return String(node.textContent);
  if (node.value != null && String(node.nodeName ?? '').toLowerCase() === '#text') return String(node.value);

  const children = nodeChildren(node);
  if (children.length === 0) return '';
  return children.map((child) => textFromNode(child)).join('');
}

export function widgetDefinition(tag, overrides = {}) {
  const leaf = Boolean(overrides.leaf);
  const complex = Boolean(overrides.complex) || overrides.complexity === 'complex';

  return {
    id: overrides.id ?? tag,
    tag,
    tags: overrides.tags ?? [tag],
    source: overrides.source ?? 'author',
    role: overrides.role ?? 'block',
    category: overrides.category ?? 'text-control',
    kind: overrides.kind ?? (leaf ? 'leaf' : 'container'),
    complexity: overrides.complexity ?? (complex ? 'complex' : 'basic'),
    leaf,
    interactive: Boolean(overrides.interactive),
    complex,
    currentStatus: overrides.currentStatus ?? 'basic',
    notes: overrides.notes ?? '',
    layoutDefaults: overrides.layoutDefaults ?? {},
    attrs: overrides.attrs ?? [],
    state: overrides.state ?? [],
    interactions: overrides.interactions ?? [],
    overlays: overrides.overlays ?? [],
    expandsTo: overrides.expandsTo ?? [],
    classify: overrides.classify,
  };
}

export function normalizeSelectionRange(selection = {}, textLength = 0) {
  const max = Math.max(0, textLength | 0);
  const start = clampNumber(selection.start ?? selection.anchor ?? 0, 0, max) | 0;
  const end = clampNumber(selection.end ?? selection.focus ?? start, 0, max) | 0;

  return {
    start,
    end,
    anchor: start,
    focus: end,
    min: Math.min(start, end),
    max: Math.max(start, end),
    collapsed: start === end,
    direction: end < start ? 'backward' : 'forward',
  };
}

export function normalizeSelections(selections = null, textLength = 0) {
  if (!selections) return [];

  const entries =
    selections instanceof Map
      ? Array.from(selections.entries())
      : Array.isArray(selections)
        ? selections.map((selection, index) => [selection.pointerId ?? index, selection])
        : Object.entries(selections);

  return entries
    .map(([pointerId, selection]) => ({
      pointerId: Number(pointerId),
      ...normalizeSelectionRange(selection, textLength),
    }))
    .filter((selection) => Number.isFinite(selection.pointerId));
}

export function normalizeTextFieldState(attrs = {}, state = {}) {
  const source = normalizeAttrs(attrs);
  const value = state.value ?? source.value ?? '';
  const text = String(value ?? '');

  return {
    value: text,
    placeholder: String(state.placeholder ?? source.placeholder ?? ''),
    disabled: state.disabled ?? flagAttr(source, 'disabled'),
    readOnly: state.readOnly ?? flagAttr(source, 'readonly'),
    required: state.required ?? flagAttr(source, 'required'),
    selections: normalizeSelections(state.selections, text.length),
  };
}

export function textFieldLayout({
  width = 0,
  height = DEFAULT_TEXT_FIELD_LAYOUT.height,
  fontSize = 16,
  leftPad = DEFAULT_TEXT_FIELD_LAYOUT.leftPad,
  topPad = DEFAULT_TEXT_FIELD_LAYOUT.topPad,
  maxLines = DEFAULT_TEXT_FIELD_LAYOUT.maxLines,
  lineHeightMultiplier = DEFAULT_TEXT_FIELD_LAYOUT.lineHeightMultiplier,
  baselineNudgeY = 0,
} = {}) {
  const w = Math.max(0, toFiniteNumber(width, 0));
  const h = Math.max(0, toFiniteNumber(height, DEFAULT_TEXT_FIELD_LAYOUT.height));
  const innerLeft = Math.max(0, toFiniteNumber(leftPad, DEFAULT_TEXT_FIELD_LAYOUT.leftPad));
  const innerTop = Math.max(0, toFiniteNumber(topPad, DEFAULT_TEXT_FIELD_LAYOUT.topPad) + toFiniteNumber(baselineNudgeY, 0));
  const lineHeight = Math.max(1, toFiniteNumber(fontSize, 16) * toFiniteNumber(lineHeightMultiplier, 1.25));

  return {
    w,
    h,
    innerLeft,
    innerTop,
    innerWidth: Math.max(0, w - innerLeft * 2),
    maxLines: Math.max(0, toFiniteNumber(maxLines, DEFAULT_TEXT_FIELD_LAYOUT.maxLines) | 0),
    lineHeight,
  };
}

// Shared helpers for text-like widgets (<input> text/password and <textarea>).
export function wrapFieldTextWithIndices(text, maxWidth, measure) {
  const s = String(text ?? '');
  const out = [];
  const max = Math.max(0, toFiniteNumber(maxWidth, 0));
  const widthOf = typeof measure === 'function' ? measure : (value) => String(value ?? '').length;

  // Wrap each hard line independently, preserving source indices.
  let lineStart = 0;
  for (let i = 0; i <= s.length; i++) {
    const isBreak = i === s.length || s[i] === '\n';
    if (!isBreak) continue;

    const paraStart = lineStart;
    const paraEnd = i;
    if (paraStart === paraEnd) {
      out.push({ start: paraStart, end: paraEnd, text: '' });
    } else {
      let segStart = paraStart;
      let lastSpace = -1;

      for (let pos = segStart; pos < paraEnd; pos++) {
        const ch = s[pos];
        if (ch === ' ') lastSpace = pos;

        const next = s.slice(segStart, pos + 1);
        if (widthOf(next) <= max || pos === segStart) continue;

        let breakPos = lastSpace >= segStart ? lastSpace + 1 : pos;
        if (breakPos <= segStart) breakPos = Math.min(paraEnd, segStart + 1);

        out.push({ start: segStart, end: breakPos, text: s.slice(segStart, breakPos) });
        segStart = breakPos;
        pos = segStart - 1;
        lastSpace = -1;
      }

      if (segStart <= paraEnd) {
        out.push({ start: segStart, end: paraEnd, text: s.slice(segStart, paraEnd) });
      }
    }

    lineStart = i + 1;
  }

  return out;
}

export function clampWrappedLines(lines, maxLines) {
  const limit = Math.max(0, toFiniteNumber(maxLines, 0) | 0);
  if (limit <= 0) return [];
  if (!Array.isArray(lines) || lines.length <= limit) return Array.isArray(lines) ? lines : [];
  return lines.slice(0, limit);
}

export function getCaretIndexFromPoint({ fullText = '', lines = [], localX = 0, localY = 0, lineHeight = 1, measure } = {}) {
  const text = String(fullText ?? '');
  const sourceLines = Array.isArray(lines) ? lines : [];
  if (sourceLines.length === 0) return 0;

  const widthOf = typeof measure === 'function' ? measure : (value) => String(value ?? '').length;
  const x = Math.max(0, toFiniteNumber(localX, 0));
  const y = Math.max(0, toFiniteNumber(localY, 0));
  const lh = Math.max(1, toFiniteNumber(lineHeight, 1));
  const lineIdx = Math.max(0, Math.min(sourceLines.length - 1, Math.floor(y / lh)));
  const line = sourceLines[lineIdx];

  let best = line.start;
  let bestDist = Number.POSITIVE_INFINITY;
  for (let i = line.start; i <= line.end; i++) {
    const w = widthOf(text.slice(line.start, i));
    const d = Math.abs(w - x);
    if (d < bestDist) {
      bestDist = d;
      best = i;
    }
  }
  return best;
}

export function textFieldPresentation({
  value = '',
  width = 0,
  height = DEFAULT_TEXT_FIELD_LAYOUT.height,
  fontSize = 16,
  measure,
  maxLines = DEFAULT_TEXT_FIELD_LAYOUT.maxLines,
  leftPad = DEFAULT_TEXT_FIELD_LAYOUT.leftPad,
  topPad = DEFAULT_TEXT_FIELD_LAYOUT.topPad,
  baselineNudgeY = 0,
} = {}) {
  const layout = textFieldLayout({ width, height, fontSize, leftPad, topPad, maxLines, baselineNudgeY });
  const text = String(value ?? '');
  const lines = wrapFieldTextWithIndices(text, layout.innerWidth, measure);
  const visibleLines = clampWrappedLines(lines, layout.maxLines);
  const visibleEnd = visibleLines.length > 0 ? visibleLines[visibleLines.length - 1].end : 0;

  return {
    text,
    layout,
    lines,
    visibleLines,
    visibleEnd,
    displayText: visibleLines.map((line) => line.text).join('\n'),
  };
}

export const TEXT_FIELD_WIDGET_DEFINITION = widgetDefinition('textfield', {
  source: 'synthetic',
  category: 'text-control',
  leaf: true,
  interactive: true,
  complexity: 'complex',
  currentStatus: 'helper',
  layoutDefaults: DEFAULT_TEXT_FIELD_LAYOUT,
  attrs: ['value', 'placeholder', 'disabled', 'readonly', 'required'],
  state: ['value', 'selectionStart', 'selectionEnd'],
  interactions: ['edit-text', 'select-text', 'move-caret'],
  notes: 'Renderer-neutral helper descriptor shared by text-like controls.',
});
