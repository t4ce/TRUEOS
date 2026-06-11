const DEFAULT_CANVAS_WIDTH = 300;
const DEFAULT_CANVAS_HEIGHT = 150;
const DEFAULT_CANVAS_MIN_WIDTH = 120;
const DEFAULT_CANVAS_MIN_HEIGHT = 80;

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

export function positiveNumberAttr(value) {
  const number = Number(value);
  return Number.isFinite(number) && number > 0 ? number : undefined;
}

export function parseCanvasDimensions(attrs = {}) {
  const source = normalizeAttrs(attrs);
  const attrWidth = positiveNumberAttr(source.width);
  const attrHeight = positiveNumberAttr(source.height);
  const width = attrWidth ?? DEFAULT_CANVAS_WIDTH;
  const height = attrHeight ?? DEFAULT_CANVAS_HEIGHT;

  return {
    width,
    height,
    minWidth: Math.min(DEFAULT_CANVAS_MIN_WIDTH, width),
    minHeight: Math.min(DEFAULT_CANVAS_MIN_HEIGHT, height),
    hasWidth: attrWidth !== undefined,
    hasHeight: attrHeight !== undefined,
    fixedSize: attrWidth !== undefined || attrHeight !== undefined,
  };
}

export function normalizeCanvasProps(attrs = {}) {
  return {
    label: 'canvas',
    ...parseCanvasDimensions(attrs),
  };
}

export const WIDGET_DEFINITION = widgetDefinition('canvas', {
  category: 'replaced',
  leaf: true,
  layoutDefaults: {
    width: DEFAULT_CANVAS_WIDTH,
    height: DEFAULT_CANVAS_HEIGHT,
    minWidth: DEFAULT_CANVAS_MIN_WIDTH,
    minHeight: DEFAULT_CANVAS_MIN_HEIGHT,
  },
  attrs: ['width', 'height'],
});

export const CANVAS_WIDGET_DEFINITION = WIDGET_DEFINITION;
