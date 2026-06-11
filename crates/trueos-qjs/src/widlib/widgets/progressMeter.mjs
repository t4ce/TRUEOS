function normalizeAttrs(attrs = {}) {
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

function widgetDefinition(tag, overrides = {}) {
  const leaf = Boolean(overrides.leaf);
  const complex = Boolean(overrides.complex) || overrides.complexity === 'complex';

  return {
    id: overrides.id ?? tag,
    tag,
    tags: overrides.tags ?? [tag],
    source: overrides.source ?? 'author',
    role: overrides.role ?? 'block',
    category: overrides.category ?? 'value-control',
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

export const PROGRESS_WIDGET_DEFINITION = widgetDefinition('progress', {
  leaf: true,
  layoutDefaults: { height: 14, minWidth: 240, paddingLeft: 0, paddingRight: 0, paddingTop: 0, paddingBottom: 0 },
  attrs: ['value', 'max'],
  state: ['value'],
});

export const METER_WIDGET_DEFINITION = widgetDefinition('meter', {
  leaf: true,
  layoutDefaults: { height: 14, minWidth: 240, paddingLeft: 0, paddingRight: 0, paddingTop: 0, paddingBottom: 0 },
  attrs: ['value', 'max', 'min', 'low', 'high', 'optimum'],
  state: ['value'],
});

export const PROGRESS_METER_WIDGET_DEFINITIONS = [PROGRESS_WIDGET_DEFINITION, METER_WIDGET_DEFINITION];

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

export function normalizeRatio(value, max = 1, min = 0) {
  const lo = toFiniteNumber(min, 0);
  const hi = toFiniteNumber(max, 1);
  if (!(hi > lo)) return 0;
  return clampNumber((toFiniteNumber(value, lo) - lo) / (hi - lo), 0, 1);
}

export function normalizeProgressState(attrs = {}) {
  const source = normalizeAttrs(attrs);
  const max = Math.max(0, toFiniteNumber(source.max ?? 1, 1));
  const value = clampNumber(toFiniteNumber(source.value ?? 0, 0), 0, max || 0);
  const ratio = max > 0 ? normalizeRatio(value, max, 0) : 0;

  return { value, max, ratio, percent: Math.round(ratio * 100) };
}

export function normalizeMeterState(attrs = {}) {
  const source = normalizeAttrs(attrs);
  const min = toFiniteNumber(source.min ?? 0, 0);
  const rawMax = toFiniteNumber(source.max ?? 1, 1);
  const max = rawMax > min ? rawMax : min;
  const value = clampNumber(toFiniteNumber(source.value ?? min, min), min, max);
  const low = clampNumber(toFiniteNumber(source.low ?? min, min), min, max);
  const high = clampNumber(toFiniteNumber(source.high ?? max, max), min, max);
  const optimum = clampNumber(toFiniteNumber(source.optimum ?? (min + max) / 2, (min + max) / 2), min, max);
  const ratio = normalizeRatio(value, max, min);

  return { value, min, max, low, high, optimum, ratio, percent: Math.round(ratio * 100) };
}

export function normalizeProgressMeterState(tagOrAttrs = 'progress', maybeAttrs = undefined) {
  const tag = typeof tagOrAttrs === 'string' ? tagOrAttrs.toLowerCase() : String(tagOrAttrs?.tag ?? 'progress').toLowerCase();
  const attrs = maybeAttrs ?? (typeof tagOrAttrs === 'string' ? {} : tagOrAttrs?.attrs ?? tagOrAttrs);
  return tag === 'meter' ? normalizeMeterState(attrs) : normalizeProgressState(attrs);
}

export function progressMeterGeometry(width, height, ratio, innerPad = 3) {
  const w = Math.max(0, Math.round(toFiniteNumber(width, 0)));
  const h = Math.max(0, Math.round(toFiniteNumber(height, 0)));
  const pad = Math.max(0, toFiniteNumber(innerPad, 3));
  const innerW = Math.max(0, w - pad * 2);
  const innerH = Math.max(0, h - pad * 2);
  const fillRatio = clampNumber(ratio, 0, 1);

  return {
    outer: { x: 0.5, y: 0.5, width: Math.max(0, w - 1), height: Math.max(0, h - 1), strokeWidth: 1 },
    inner: { x: pad, y: pad, width: innerW, height: innerH },
    fill: { x: pad, y: pad, width: innerW * fillRatio, height: innerH },
    ratio: fillRatio,
  };
}
