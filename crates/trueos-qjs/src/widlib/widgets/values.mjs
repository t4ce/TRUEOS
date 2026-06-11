export const TEMPORAL_INPUT_KINDS = Object.freeze(['time', 'date', 'month', 'week', 'datetime-local']);

export const TEMPORAL_LEGACY_TAGS = Object.freeze({
  time: 'timeinput',
  date: 'dateinput',
  month: 'monthinput',
  week: 'weekinput',
  'datetime-local': 'datetimelocalinput',
});

const DEFAULT_RGBA = Object.freeze({ r: 255, g: 0, b: 0, a: 255 });

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

export function temporalInputDefinition(kind) {
  const normalized = normalizeTemporalKind(kind);
  return {
    ...widgetDefinition('input', {
      id: `input.${normalized}`,
      tags: ['input'],
      category: 'text-control',
      leaf: true,
      interactive: true,
      complexity: 'complex',
      currentStatus: 'defer-special-ui',
      layoutDefaults: { height: 36, minWidth: normalized === 'datetime-local' ? 340 : 220 },
      attrs: ['type', 'value', 'min', 'max', 'step', 'disabled'],
      state: ['value'],
      interactions: ['edit-value', 'open-picker'],
      overlays: ['temporal-picker'],
    }),
    subtype: normalized,
  };
}

function legacyTemporalDefinition(kind) {
  const normalized = normalizeTemporalKind(kind);
  return widgetDefinition(TEMPORAL_LEGACY_TAGS[normalized], {
    source: 'synthetic',
    category: 'text-control',
    leaf: true,
    interactive: true,
    complexity: 'complex',
    currentStatus: 'legacy-synthetic',
    layoutDefaults: { height: 36, minWidth: normalized === 'datetime-local' ? 340 : 220 },
    attrs: ['value', 'min', 'max', 'step', 'disabled'],
    state: ['value'],
    interactions: ['edit-value', 'open-picker'],
    overlays: ['temporal-picker'],
  });
}

export const VALUE_WIDGET_DEFINITIONS = [
  widgetDefinition('progress', {
    leaf: true,
    layoutDefaults: { height: 14, minWidth: 240 },
    attrs: ['value', 'max'],
    state: ['value'],
  }),
  widgetDefinition('meter', {
    leaf: true,
    layoutDefaults: { height: 14, minWidth: 240 },
    attrs: ['value', 'max', 'min', 'low', 'high', 'optimum'],
    state: ['value'],
  }),
  widgetDefinition('slider', {
    leaf: true,
    interactive: true,
    complexity: 'complex',
    layoutDefaults: { height: 14, minWidth: 240 },
    attrs: ['value', 'min', 'max', 'step'],
    state: ['value'],
    interactions: ['drag'],
    notes: 'Custom demo tag; should become input[type=range] or host-defined widget later.',
  }),
  widgetDefinition('sliderlabel', {
    source: 'synthetic',
    leaf: true,
    layoutDefaults: { marginRight: 6 },
    attrs: ['data-slider-key', 'data-slider-init'],
  }),
  widgetDefinition('barrow', {
    source: 'synthetic',
    category: 'layout',
    role: 'row',
    layoutDefaults: { paddingLeft: 8 },
  }),
  widgetDefinition('number', {
    leaf: true,
    interactive: true,
    complexity: 'complex',
    layoutDefaults: { height: 36, minWidth: 140 },
    attrs: ['value', 'min', 'max', 'step', 'channel'],
    state: ['value'],
    interactions: ['increment', 'decrement'],
    notes: 'Custom demo spinner; should become input[type=number] or host-defined widget later.',
  }),
  widgetDefinition('color', {
    kind: 'composite',
    leaf: true,
    interactive: true,
    complex: true,
    layoutDefaults: { width: 240, height: 200 },
    attrs: ['value', 'width', 'height'],
    state: ['r', 'g', 'b', 'a', 'rgb', 'alpha'],
    interactions: ['pick-color'],
    currentStatus: 'defer-composite-ui',
  }),
  ...TEMPORAL_INPUT_KINDS.map((kind) => legacyTemporalDefinition(kind)),
];

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

export function normalizeProgressRatio(attrs = {}) {
  const source = normalizeAttrs(attrs);
  const max = toFiniteNumber(source.max ?? 1, 1);
  const value = toFiniteNumber(source.value ?? 0, 0);
  return max > 0 ? normalizeRatio(value, max, 0) : 0;
}

export function normalizeMeterRatio(attrs = {}) {
  const source = normalizeAttrs(attrs);
  const min = toFiniteNumber(source.min ?? 0, 0);
  const max = toFiniteNumber(source.max ?? 1, 1);
  const value = toFiniteNumber(source.value ?? min, min);
  return normalizeRatio(value, max, min);
}

export function normalizeSliderValue(attrs = {}) {
  const source = normalizeAttrs(attrs);
  const hasMinMax = source.min != null || source.max != null;
  const min = hasMinMax ? toFiniteNumber(source.min ?? 0, 0) : 0;
  const max = hasMinMax ? toFiniteNumber(source.max ?? 1, 1) : 1;
  const value = clampNumber(toFiniteNumber(source.value ?? min, min), min, max);
  const step = Math.max(1e-9, toFiniteNumber(source.step ?? (hasMinMax ? 1 : 0.01), hasMinMax ? 1 : 0.01));

  return {
    value,
    min,
    max,
    step,
    ratio: normalizeRatio(value, max, min),
    percent: Math.round(normalizeRatio(value, max, min) * 100),
  };
}

export function normalizeSliderLabel(attrs = {}, state = null) {
  const source = normalizeAttrs(attrs);
  const sliderValue = state && Number.isFinite(Number(state.value)) ? Number(state.value) : source['data-slider-init'];
  return String(Math.round(normalizeRatio(sliderValue ?? 0, 1, 0) * 100));
}

export function normalizeNumberValue(attrs = {}, value = attrs.value) {
  const source = normalizeAttrs(attrs);
  const min = toFiniteNumber(source.min ?? 0, 0);
  const max = toFiniteNumber(source.max ?? 255, 255);
  const step = Math.max(1e-9, toFiniteNumber(source.step ?? 1, 1));
  const normalizedValue = clampNumber(toFiniteNumber(value ?? source.value ?? 0, 0), min, max);
  const channel = String(source.channel ?? '').toLowerCase();
  const label = channel === 'r' ? 'R' : channel === 'g' ? 'G' : channel === 'b' ? 'B' : channel === 'a' ? 'A' : '';

  return {
    value: normalizedValue,
    roundedValue: Math.round(normalizedValue),
    min,
    max,
    step,
    channel,
    label,
    displayText: label ? `${label}: ${Math.round(normalizedValue)}` : String(Math.round(normalizedValue)),
  };
}

export function stepNumberValue(attrs = {}, value = attrs.value, direction = 1) {
  const source = normalizeAttrs(attrs);
  const normalized = normalizeNumberValue(source, value ?? source.value);
  const dir = direction < 0 ? -1 : 1;
  return clampNumber(normalized.value + dir * normalized.step, normalized.min, normalized.max);
}

export function clampByte(value, fallback = 0) {
  return Math.max(0, Math.min(255, Math.round(toFiniteNumber(value, fallback))));
}

function hexByte(hex) {
  const n = Number.parseInt(hex, 16);
  return Number.isFinite(n) ? n : 0;
}

function parseHexColor(value) {
  const raw = String(value ?? '').trim();
  if (!raw.startsWith('#')) return null;
  const hex = raw.slice(1);

  if (hex.length === 3 || hex.length === 4) {
    return {
      r: hexByte(hex[0] + hex[0]),
      g: hexByte(hex[1] + hex[1]),
      b: hexByte(hex[2] + hex[2]),
      a: hex.length === 4 ? hexByte(hex[3] + hex[3]) : 255,
    };
  }

  if (hex.length === 6 || hex.length === 8) {
    return {
      r: hexByte(hex.slice(0, 2)),
      g: hexByte(hex.slice(2, 4)),
      b: hexByte(hex.slice(4, 6)),
      a: hex.length === 8 ? hexByte(hex.slice(6, 8)) : 255,
    };
  }

  return null;
}

function parseRgbColor(value) {
  const raw = String(value ?? '').trim();
  const match = raw.match(/^rgba?\(([^)]+)\)$/i);
  if (!match) return null;

  const parts = match[1].split(',').map((part) => part.trim());
  if (parts.length !== 3 && parts.length !== 4) return null;

  const alphaRaw = parts.length === 4 ? toFiniteNumber(parts[3], 1) : 1;
  return {
    r: clampByte(parts[0]),
    g: clampByte(parts[1]),
    b: clampByte(parts[2]),
    a: clampByte(alphaRaw <= 1 ? alphaRaw * 255 : alphaRaw, 255),
  };
}

export function parseColorRgba(value) {
  return parseHexColor(value) ?? parseRgbColor(value);
}

export function normalizeColorRgba(value = null, fallback = DEFAULT_RGBA) {
  const attrs = normalizeAttrs(value);
  const parsed =
    typeof value === 'string' ? parseColorRgba(value) : typeof attrs.value === 'string' ? parseColorRgba(attrs.value) : null;
  const source = parsed ?? attrs ?? {};
  const fb = fallback ?? DEFAULT_RGBA;

  return {
    r: clampByte(source.r ?? source.red ?? fb.r, fb.r),
    g: clampByte(source.g ?? source.green ?? fb.g, fb.g),
    b: clampByte(source.b ?? source.blue ?? fb.b, fb.b),
    a: clampByte(source.a ?? source.alpha ?? fb.a, fb.a),
  };
}

export function colorRgbaToHex(rgba, includeAlpha = true) {
  const c = normalizeColorRgba(rgba);
  const hex = [c.r, c.g, c.b, ...(includeAlpha ? [c.a] : [])]
    .map((part) => clampByte(part).toString(16).padStart(2, '0'))
    .join('');
  return `#${hex}`.toUpperCase();
}

function clampInt(value, min, max) {
  const n = Math.trunc(toFiniteNumber(value, min));
  return Math.max(min, Math.min(max, n));
}

function pad2(value) {
  const n = clampInt(value, 0, 99);
  return n < 10 ? `0${n}` : String(n);
}

function parseIntInRange(value, min, max) {
  if (!/^\d+$/.test(String(value ?? ''))) return null;
  const n = Number(value);
  if (!Number.isFinite(n)) return null;
  const i = Math.trunc(n);
  return i >= min && i <= max ? i : null;
}

function parseYear2FromYYYY(value) {
  const raw = String(value ?? '');
  if (!/^\d{4}$/.test(raw)) return null;
  const year = Number(raw);
  const year2 = year - 2000;
  return year2 >= 0 && year2 <= 99 ? year2 : null;
}

export function normalizeTemporalKind(kind) {
  const normalized = String(kind ?? '').toLowerCase();
  return TEMPORAL_INPUT_KINDS.includes(normalized) ? normalized : 'date';
}

export function defaultTemporalState(kind = 'date', now = new Date()) {
  return {
    kind: normalizeTemporalKind(kind),
    year2: clampInt(now.getFullYear() - 2000, 0, 99),
    month: clampInt(now.getMonth() + 1, 1, 12),
    weekIndex: 1,
    hour: clampInt(now.getHours(), 0, 23),
    minute: clampInt(now.getMinutes(), 0, 59),
    second: clampInt(now.getSeconds(), 0, 59),
    openPanel: null,
    openYear: false,
    openMonthGrid: false,
  };
}

export function parseTemporalValue(kind, value, now = new Date()) {
  const normalized = normalizeTemporalKind(kind);
  const fallback = defaultTemporalState(normalized, now);
  const raw = String(value ?? '').trim();
  if (raw.length === 0) return fallback;

  if (normalized === 'time') {
    const parts = raw.split(':');
    if (parts.length !== 2 && parts.length !== 3) return fallback;
    const hour = parseIntInRange(parts[0], 0, 23);
    const minute = parseIntInRange(parts[1], 0, 59);
    const second = parts.length === 3 ? parseIntInRange(parts[2], 0, 59) : 0;
    if (hour == null || minute == null || second == null) return fallback;
    return { ...fallback, hour, minute, second };
  }

  if (normalized === 'month') {
    const parts = raw.split('-');
    if (parts.length !== 2) return fallback;
    const year2 = parseYear2FromYYYY(parts[0]);
    const month = parseIntInRange(parts[1], 1, 12);
    if (year2 == null || month == null) return fallback;
    return { ...fallback, year2, month };
  }

  if (normalized === 'week') {
    const sep = raw.indexOf('-W');
    if (sep < 0) return fallback;
    const year2 = parseYear2FromYYYY(raw.slice(0, sep));
    const week = parseIntInRange(raw.slice(sep + 2), 1, 48);
    if (year2 == null || week == null) return fallback;
    return {
      ...fallback,
      year2,
      month: clampInt(Math.floor((week - 1) / 4) + 1, 1, 12),
      weekIndex: clampInt(((week - 1) % 4) + 1, 1, 4),
    };
  }

  const dateTimeSeparator = normalized === 'datetime-local' ? raw.search(/[T ]/) : -1;
  const dateRaw = dateTimeSeparator >= 0 ? raw.slice(0, dateTimeSeparator) : raw;
  const dateParts = dateRaw.split('-');
  if (dateParts.length !== 3) return fallback;

  const year2 = parseYear2FromYYYY(dateParts[0]);
  const month = parseIntInRange(dateParts[1], 1, 12);
  const day = parseIntInRange(dateParts[2], 1, 31);
  if (year2 == null || month == null || day == null) return fallback;

  const dateState = {
    ...fallback,
    year2,
    month,
    weekIndex: clampInt(Math.floor((day - 1) / 7) + 1, 1, 4),
  };

  if (normalized !== 'datetime-local') return dateState;

  const timeParts = raw.slice(dateTimeSeparator + 1).split(':');
  if (timeParts.length !== 2 && timeParts.length !== 3) return dateState;
  const hour = parseIntInRange(timeParts[0], 0, 23);
  const minute = parseIntInRange(timeParts[1], 0, 59);
  const second = timeParts.length === 3 ? parseIntInRange(timeParts[2], 0, 59) : 0;
  if (hour == null || minute == null || second == null) return dateState;
  return { ...dateState, hour, minute, second };
}

export function pseudoWeekNumber(state) {
  const month = clampInt(state?.month ?? 1, 1, 12);
  const weekIndex = clampInt(state?.weekIndex ?? 1, 1, 4);
  return (month - 1) * 4 + weekIndex;
}

export function formatTemporalValue(kind, state) {
  const normalized = normalizeTemporalKind(kind ?? state?.kind);
  const st = { ...defaultTemporalState(normalized), ...(state ?? {}), kind: normalized };

  const date = `20${pad2(st.year2)}-${pad2(st.month)}-${pad2((clampInt(st.weekIndex, 1, 4) - 1) * 7 + 1)}`;
  const time = `${pad2(st.hour)}:${pad2(st.minute)}:${pad2(st.second)}`;

  if (normalized === 'time') return time;
  if (normalized === 'month') return `20${pad2(st.year2)}-${pad2(st.month)}`;
  if (normalized === 'week') return `20${pad2(st.year2)}-W${pad2(pseudoWeekNumber(st))}`;
  if (normalized === 'datetime-local') return `${date}T${time}`;
  return date;
}

export function temporalDisplayLabel(kind, valueOrState, now = new Date()) {
  const normalized = normalizeTemporalKind(kind);
  const state =
    valueOrState && typeof valueOrState === 'object'
      ? { ...defaultTemporalState(normalized, now), ...valueOrState, kind: normalized }
      : parseTemporalValue(normalized, valueOrState, now);
  return formatTemporalValue(normalized, state);
}
