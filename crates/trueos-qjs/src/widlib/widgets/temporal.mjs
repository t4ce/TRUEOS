export const TEMPORAL_INPUT_KINDS = Object.freeze(['time', 'date', 'month', 'week', 'datetime-local']);

export const TEMPORAL_LEGACY_TAGS = Object.freeze({
  time: 'timeinput',
  date: 'dateinput',
  month: 'monthinput',
  week: 'weekinput',
  'datetime-local': 'datetimelocalinput',
});

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

export function normalizeTemporalKind(kind) {
  const normalized = String(kind ?? '').toLowerCase();
  return TEMPORAL_INPUT_KINDS.includes(normalized) ? normalized : 'date';
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
      layoutDefaults: { height: 36, minHeight: 36, minWidth: normalized === 'datetime-local' ? 340 : 220, paddingLeft: 0, paddingRight: 0, paddingTop: 0, paddingBottom: 0 },
      attrs: ['type', 'value', 'min', 'max', 'step', 'disabled'],
      state: ['value'],
      interactions: ['edit-value', 'open-picker'],
      overlays: ['temporal-picker'],
    }),
    subtype: normalized,
  };
}

export function legacyTemporalDefinition(kind) {
  const normalized = normalizeTemporalKind(kind);
  return {
    ...widgetDefinition(TEMPORAL_LEGACY_TAGS[normalized], {
      source: 'synthetic',
      category: 'text-control',
      leaf: true,
      interactive: true,
      complexity: 'complex',
      currentStatus: 'legacy-synthetic',
      layoutDefaults: { height: 36, minHeight: 36, minWidth: normalized === 'datetime-local' ? 340 : 220, paddingLeft: 0, paddingRight: 0, paddingTop: 0, paddingBottom: 0 },
      attrs: ['value', 'min', 'max', 'step', 'disabled'],
      state: ['value'],
      interactions: ['edit-value', 'open-picker'],
      overlays: ['temporal-picker'],
    }),
    subtype: normalized,
  };
}

export const TEMPORAL_INPUT_WIDGET_DEFINITIONS = TEMPORAL_INPUT_KINDS.map((kind) => temporalInputDefinition(kind));
export const TEMPORAL_LEGACY_WIDGET_DEFINITIONS = TEMPORAL_INPUT_KINDS.map((kind) => legacyTemporalDefinition(kind));
export const TEMPORAL_WIDGET_DEFINITIONS = [...TEMPORAL_INPUT_WIDGET_DEFINITIONS, ...TEMPORAL_LEGACY_WIDGET_DEFINITIONS];

function toFiniteNumber(value, fallback = 0) {
  const n = Number(value);
  return Number.isFinite(n) ? n : fallback;
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

export function temporalKindFromTagName(tagName, attrs = {}) {
  const tag = String(tagName ?? '').toLowerCase();
  if (tag === 'input') return normalizeTemporalKind(normalizeAttrs(attrs).type);

  for (const [kind, legacyTag] of Object.entries(TEMPORAL_LEGACY_TAGS)) {
    if (tag === legacyTag) return kind;
  }

  return normalizeTemporalKind(tag.replace(/input$/, '').replace('datetimelocal', 'datetime-local'));
}

export function defaultTemporalState(kind = 'date', now = new Date(), inputKey = '') {
  const normalized = normalizeTemporalKind(kind);
  const year2 = clampInt(now.getFullYear() - 2000, 0, 99);
  const month = clampInt(now.getMonth() + 1, 1, 12);
  const day = clampInt(now.getDate(), 1, 31);
  const weekIndex = clampInt(Math.floor((day - 1) / 7) + 1, 1, 4);
  const yearSliderKey = inputKey ? `${inputKey}:year-slider` : '';

  if (normalized === 'time') {
    return {
      kind: normalized,
      hour: clampInt(now.getHours(), 0, 23),
      minute: clampInt(now.getMinutes(), 0, 59),
      second: clampInt(now.getSeconds(), 0, 59),
      openPanel: null,
    };
  }

  if (normalized === 'month') {
    return {
      kind: normalized,
      year2,
      month,
      openYear: false,
      openMonthGrid: false,
      yearSliderKey,
    };
  }

  if (normalized === 'week') {
    const week = pseudoWeekNumber({ month, weekIndex });
    return {
      kind: normalized,
      year2,
      week,
      month,
      weekIndex,
      openPanel: null,
      openYear: false,
      yearSliderKey,
    };
  }

  const dateState = {
    kind: normalized,
    year2,
    month,
    day,
    weekIndex,
    openYear: false,
    openMonthGrid: false,
    yearSliderKey,
  };

  if (normalized !== 'datetime-local') return dateState;

  return {
    ...dateState,
    hour: clampInt(now.getHours(), 0, 23),
    minute: clampInt(now.getMinutes(), 0, 59),
    second: clampInt(now.getSeconds(), 0, 59),
    openPanel: null,
  };
}

export function parseTemporalValue(kind, value, now = new Date(), inputKey = '') {
  const normalized = normalizeTemporalKind(kind);
  const fallback = defaultTemporalState(normalized, now, inputKey);
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
    const week = parseIntInRange(raw.slice(sep + 2), 1, 53);
    if (year2 == null || week == null) return fallback;
    return {
      ...fallback,
      year2,
      week,
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
    day,
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

export function normalizeTemporalState(kindOrInput = 'date', attrs = {}, now = new Date(), inputKey = '') {
  const source =
    typeof kindOrInput === 'string' ? normalizeAttrs(attrs) : normalizeAttrs(kindOrInput?.attrs ?? kindOrInput ?? attrs);
  const kind =
    typeof kindOrInput === 'string'
      ? normalizeTemporalKind(kindOrInput)
      : temporalKindFromTagName(kindOrInput?.tagName ?? kindOrInput?.tag ?? 'input', source);
  const state = parseTemporalValue(kind, source.value, now, inputKey);
  return { ...state, kind, yearSliderKey: inputKey ? `${inputKey}:year-slider` : state.yearSliderKey };
}

export function pseudoWeekNumber(state) {
  const month = clampInt(state?.month ?? 1, 1, 12);
  const weekIndex = clampInt(state?.weekIndex ?? 1, 1, 4);
  return (month - 1) * 4 + weekIndex;
}

export function formatTemporalValue(kind, state) {
  const normalized = normalizeTemporalKind(kind ?? state?.kind);
  const st = { ...defaultTemporalState(normalized), ...(state ?? {}), kind: normalized };
  const day = st.day == null ? (clampInt(st.weekIndex, 1, 4) - 1) * 7 + 1 : st.day;
  const date = `20${pad2(st.year2)}-${pad2(st.month)}-${pad2(day)}`;
  const time = `${pad2(st.hour)}:${pad2(st.minute)}:${pad2(st.second)}`;

  if (normalized === 'time') return time;
  if (normalized === 'month') return `20${pad2(st.year2)}-${pad2(st.month)}`;
  if (normalized === 'week') return `20${pad2(st.year2)}-W${pad2(st.week == null ? pseudoWeekNumber(st) : st.week)}`;
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

export function nextTemporalPanel(kind, currentPanel = null, localX = 0, width = 0) {
  const normalized = normalizeTemporalKind(kind);
  if (normalized !== 'datetime-local') {
    const panel = normalized === 'time' ? 'time' : normalized === 'month' ? 'month' : 'week';
    return currentPanel === panel ? null : panel;
  }

  const leftHalf = toFiniteNumber(localX, 0) <= toFiniteNumber(width, 0) * 0.52;
  const panel = leftHalf ? 'week' : 'time';
  return currentPanel === panel ? null : panel;
}
