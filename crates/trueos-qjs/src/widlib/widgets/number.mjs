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

export const NUMBER_WIDGET_DEFINITION = widgetDefinition('number', {
  leaf: true,
  interactive: true,
  complexity: 'complex',
  layoutDefaults: {
    height: 36,
    minHeight: 36,
    minWidth: 140,
    paddingLeft: 0,
    paddingRight: 0,
    paddingTop: 0,
    paddingBottom: 0,
    flexGrow: 0,
    flexShrink: 0,
  },
  attrs: ['value', 'min', 'max', 'step', 'channel'],
  state: ['value'],
  interactions: ['increment', 'decrement'],
  notes: 'Custom demo spinner; should become input[type=number] or host-defined widget later.',
});

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

export function normalizeNumberValue(attrs = {}, value = undefined) {
  const source = normalizeAttrs(attrs);
  const min = toFiniteNumber(source.min ?? 0, 0);
  const max = toFiniteNumber(source.max ?? 255, 255);
  const safeMax = max >= min ? max : min;
  const step = Math.max(1e-9, toFiniteNumber(source.step ?? 1, 1));
  const normalizedValue = clampNumber(toFiniteNumber(value ?? source.value ?? 0, 0), min, safeMax);
  const channel = String(source.channel ?? '').toLowerCase();
  const label = channel === 'r' ? 'R' : channel === 'g' ? 'G' : channel === 'b' ? 'B' : channel === 'a' ? 'A' : '';
  const roundedValue = Math.round(normalizedValue);

  return {
    value: normalizedValue,
    roundedValue,
    min,
    max: safeMax,
    step,
    channel,
    label,
    displayText: label ? `${label}: ${roundedValue}` : String(roundedValue),
  };
}

export function createNumberState(attrs = {}) {
  const normalized = normalizeNumberValue(attrs);
  return { value: normalized.value };
}

export function stepNumberValue(attrs = {}, value = undefined, direction = 1) {
  const normalized = normalizeNumberValue(attrs, value);
  const dir = direction < 0 ? -1 : 1;
  return clampNumber(normalized.value + dir * normalized.step, normalized.min, normalized.max);
}

export function numberSpinnerGeometry(width, height, arrowWidth = 22) {
  const w = Math.max(0, toFiniteNumber(width, 0));
  const h = Math.max(0, toFiniteNumber(height, 0));
  const aw = Math.max(0, toFiniteNumber(arrowWidth, 22));
  const sepX = Math.max(0, w - aw);

  return {
    outer: { x: 0.5, y: 0.5, width: Math.max(0, w - 1), height: Math.max(0, h - 1), strokeWidth: 1 },
    separator: { x1: sepX + 0.5, y1: 0, x2: sepX + 0.5, y2: h, strokeWidth: 1 },
    valueText: { x: 8, y: 9 },
    arrowColumn: { x: sepX, y: 0, width: aw, height: h },
    upHit: { x: sepX, y: 0, width: aw, height: h / 2 },
    downHit: { x: sepX, y: h / 2, width: aw, height: h / 2 },
  };
}

export function numberStepDirectionAtLocalPoint(localX, localY, width, height, arrowWidth = 22) {
  const geometry = numberSpinnerGeometry(width, height, arrowWidth);
  const x = toFiniteNumber(localX, -1);
  const y = toFiniteNumber(localY, -1);
  const { upHit, downHit } = geometry;
  const inUp = x >= upHit.x && x <= upHit.x + upHit.width && y >= upHit.y && y <= upHit.y + upHit.height;
  const inDown = x >= downHit.x && x <= downHit.x + downHit.width && y >= downHit.y && y <= downHit.y + downHit.height;
  return inUp ? 1 : inDown ? -1 : 0;
}
