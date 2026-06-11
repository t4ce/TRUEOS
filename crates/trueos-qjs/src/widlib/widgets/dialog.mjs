const DEFAULT_DIALOG_WIDTH = 540;
const DEFAULT_DIALOG_MIN_WIDTH = 360;
const DEFAULT_DIALOG_MIN_HEIGHT = 148;

function hasOwn(object, key) {
  return Object.prototype.hasOwnProperty.call(object, key);
}

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

function attrsFromInput(input = {}) {
  const source = normalizeAttrs(input);
  return normalizeAttrs(source.attrs ?? source);
}

function finiteNumber(value, fallback = 0) {
  const number = Number(value);
  return Number.isFinite(number) ? number : fallback;
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
    category: overrides.category ?? 'structure',
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

export function flagAttr(attrs = {}, name) {
  const source = normalizeAttrs(attrs);
  return hasOwn(source, name) && source[name] !== false && source[name] !== 'false';
}

export function normalizeDialogKey(input = {}) {
  const source = normalizeAttrs(input);
  const attrs = attrsFromInput(input);
  const rawKey = source.key ?? attrs.id ?? attrs['data-dialog-key'] ?? '';
  const key = String(rawKey ?? '').trim();
  return key.length > 0 ? key : undefined;
}

export function defaultDialogState() {
  return { x: 0, y: 0 };
}

export function normalizeDialogState(state = {}) {
  return {
    x: finiteNumber(state?.x, 0),
    y: finiteNumber(state?.y, 0),
  };
}

export function getOrInitDialogState(map, key) {
  const existing = map?.get?.(key);
  if (existing) return existing;

  const state = defaultDialogState();
  map?.set?.(key, state);
  return state;
}

export function normalizeDialogOpen(input = {}) {
  return flagAttr(attrsFromInput(input), 'open');
}

export function normalizeDialogProps(input = {}, storedState = null) {
  return {
    key: normalizeDialogKey(input),
    open: normalizeDialogOpen(input),
    state: normalizeDialogState(storedState ?? input?.state ?? defaultDialogState()),
    width: DEFAULT_DIALOG_WIDTH,
    minWidth: DEFAULT_DIALOG_MIN_WIDTH,
    minHeight: DEFAULT_DIALOG_MIN_HEIGHT,
  };
}

export function normalizeDialogDrag(drag = {}) {
  return {
    key: String(drag?.key ?? ''),
    startGX: finiteNumber(drag?.startGX, 0),
    startGY: finiteNumber(drag?.startGY, 0),
    originX: finiteNumber(drag?.originX, 0),
    originY: finiteNumber(drag?.originY, 0),
  };
}

export function nextDialogPosition(drag = {}, pointer = {}) {
  const normalized = normalizeDialogDrag(drag);
  return {
    x: normalized.originX + finiteNumber(pointer?.x, normalized.startGX) - normalized.startGX,
    y: normalized.originY + finiteNumber(pointer?.y, normalized.startGY) - normalized.startGY,
  };
}

export const WIDGET_DEFINITION = widgetDefinition('dialog', {
  category: 'popup',
  interactive: true,
  complex: true,
  layoutDefaults: {
    width: DEFAULT_DIALOG_WIDTH,
    minWidth: DEFAULT_DIALOG_MIN_WIDTH,
    minHeight: DEFAULT_DIALOG_MIN_HEIGHT,
    padding: 12,
  },
  attrs: ['open'],
  state: ['open', 'x', 'y'],
  interactions: ['drag'],
  overlays: ['dialog'],
  currentStatus: 'represent-only',
});

export const DIALOG_WIDGET_DEFINITION = WIDGET_DEFINITION;
