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

export const SLIDER_WIDGET_DEFINITION = widgetDefinition('slider', {
  leaf: true,
  interactive: true,
  complexity: 'complex',
  layoutDefaults: { height: 14, minWidth: 240, paddingLeft: 0, paddingRight: 0, paddingTop: 0, paddingBottom: 0 },
  attrs: ['value', 'min', 'max', 'step'],
  state: ['value'],
  interactions: ['drag'],
  notes: 'Custom demo tag; should become input[type=range] or host-defined widget later.',
});

export const SLIDER_LABEL_WIDGET_DEFINITION = widgetDefinition('sliderlabel', {
  source: 'synthetic',
  leaf: true,
  layoutDefaults: { marginRight: 6, paddingLeft: 0, paddingRight: 0, paddingTop: 0, paddingBottom: 0 },
  attrs: ['data-slider-key', 'data-slider-init'],
});

export const SLIDER_WIDGET_DEFINITIONS = [SLIDER_WIDGET_DEFINITION, SLIDER_LABEL_WIDGET_DEFINITION];

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

export function normalizeSliderState(attrs = {}, fallbackValue = undefined) {
  const source = normalizeAttrs(attrs);
  const hasMinMax = source.min != null || source.max != null;
  const min = hasMinMax ? toFiniteNumber(source.min ?? 0, 0) : 0;
  const max = hasMinMax ? toFiniteNumber(source.max ?? 1, 1) : 1;
  const safeMax = max > min ? max : min;
  const step = Math.max(1e-9, toFiniteNumber(source.step ?? (hasMinMax ? 1 : 0.01), hasMinMax ? 1 : 0.01));
  const value = clampNumber(toFiniteNumber(fallbackValue ?? source.value ?? min, min), min, safeMax);
  const ratio = normalizeRatio(value, safeMax, min);

  return {
    value,
    min,
    max: safeMax,
    step,
    ratio,
    percent: Math.round(ratio * 100),
  };
}

export function valueFromSliderRatio(ratio, attrs = {}) {
  const normalized = normalizeSliderState(attrs);
  const raw = normalized.min + clampNumber(ratio, 0, 1) * (normalized.max - normalized.min);
  const snapped = normalized.min + Math.round((raw - normalized.min) / normalized.step) * normalized.step;
  return clampNumber(snapped, normalized.min, normalized.max);
}

export function sliderRatioFromLocalX(localX, width, innerPad = 3) {
  const w = Math.max(0, toFiniteNumber(width, 0));
  const pad = Math.max(0, toFiniteNumber(innerPad, 3));
  const innerW = Math.max(1, w - pad * 2);
  return clampNumber((toFiniteNumber(localX, 0) - pad) / innerW, 0, 1);
}

export function sliderGeometry(width, height, ratio, innerPad = 3) {
  const w = Math.max(0, Math.round(toFiniteNumber(width, 0)));
  const h = Math.max(0, Math.round(toFiniteNumber(height, 0)));
  const pad = Math.max(0, toFiniteNumber(innerPad, 3));
  const innerW = Math.max(0, w - pad * 2);
  const innerH = Math.max(0, h - pad * 2);
  const r = clampNumber(ratio, 0, 1);
  const indicatorX = pad + innerW * r;
  const overhang = innerH / 2;

  return {
    outer: { x: 0.5, y: 0.5, width: Math.max(0, w - 1), height: Math.max(0, h - 1), strokeWidth: 1 },
    inner: { x: pad, y: pad, width: innerW, height: innerH },
    fill: { x: pad, y: pad, width: innerW * r, height: innerH },
    indicator: { x1: indicatorX, y1: pad - overhang, x2: indicatorX, y2: pad + innerH + overhang, strokeWidth: 2 },
    ratio: r,
  };
}

export function normalizeSliderLabel(attrs = {}, state = null) {
  const source = normalizeAttrs(attrs);
  const value = state && Number.isFinite(Number(state.value)) ? Number(state.value) : source['data-slider-init'];
  return String(Math.round(normalizeRatio(value ?? 0, 1, 0) * 100));
}
