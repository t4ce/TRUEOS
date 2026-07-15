const DEFAULT_IMG_WIDTH = 240;
const DEFAULT_IMG_HEIGHT = 140;
const DEFAULT_IMG_MIN_WIDTH = 120;
const DEFAULT_IMG_MIN_HEIGHT = 80;

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

export function parseImageDimensions(attrs = {}) {
  const source = normalizeAttrs(attrs);
  const attrWidth = positiveNumberAttr(source.width);
  const attrHeight = positiveNumberAttr(source.height);
  const width = attrWidth ?? DEFAULT_IMG_WIDTH;
  const height = attrHeight ?? DEFAULT_IMG_HEIGHT;

  return {
    width,
    height,
    minWidth: DEFAULT_IMG_MIN_WIDTH,
    minHeight: DEFAULT_IMG_MIN_HEIGHT,
    hasWidth: attrWidth !== undefined,
    hasHeight: attrHeight !== undefined,
    fixedSize: attrWidth !== undefined || attrHeight !== undefined,
  };
}

export function normalizeImageProps(attrs = {}) {
  const source = normalizeAttrs(attrs);
  const src = String(source.src ?? '');
  const alt = String(source.alt ?? '');
  const trimmedSrc = src.trim();
  const trimmedAlt = alt.trim();
  const hasSrc = trimmedSrc.length > 0;

  return {
    src,
    alt,
    hasSrc,
    label: trimmedAlt.length > 0 ? alt : hasSrc ? src : 'img',
    placeholder: hasSrc
      ? { kind: 'rect-x', fill: '#f6f6f6', stroke: '#999999', cross: '#c8c8c8' }
      : { kind: 'rect', fill: '#ff66c4' },
    crossOrigin: String(source.crossorigin ?? source.crossOrigin ?? ''),
    decoding: String(source.decoding ?? ''),
    loading: String(source.loading ?? ''),
    ...parseImageDimensions(source),
  };
}

export const WIDGET_DEFINITION = widgetDefinition('img', {
  category: 'replaced',
  leaf: true,
  complexity: 'complex',
  layoutDefaults: {
    width: DEFAULT_IMG_WIDTH,
    height: DEFAULT_IMG_HEIGHT,
    minWidth: DEFAULT_IMG_MIN_WIDTH,
    minHeight: DEFAULT_IMG_MIN_HEIGHT,
  },
  attrs: ['src', 'alt', 'width', 'height'],
});

export const IMG_WIDGET_DEFINITION = WIDGET_DEFINITION;
