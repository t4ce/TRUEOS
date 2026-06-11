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

function decodeBase64(payload) {
  if (typeof globalThis.atob === 'function') return globalThis.atob(payload);
  const bufferCtor = globalThis.Buffer;
  if (bufferCtor && typeof bufferCtor.from === 'function') {
    return bufferCtor.from(payload, 'base64').toString('utf8');
  }
  return null;
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

export function decodeSvgDataUri(src = '') {
  const source = String(src ?? '');
  if (!source.startsWith('data:image/svg+xml')) return null;

  const commaIdx = source.indexOf(',');
  if (commaIdx === -1) return null;
  const meta = source.slice(0, commaIdx).toLowerCase();
  const payload = source.slice(commaIdx + 1);

  try {
    return meta.includes(';base64') ? decodeBase64(payload) : decodeURIComponent(payload);
  } catch {
    return null;
  }
}

export function stripTagBlock(source = '', tag = '') {
  let out = '';
  let i = 0;
  const text = String(source ?? '');
  const lower = text.toLowerCase();
  const normalizedTag = String(tag ?? '').toLowerCase();
  const openNeedle = '<' + normalizedTag;
  const closeNeedle = '</' + normalizedTag;

  while (i < text.length) {
    const open = lower.indexOf(openNeedle, i);
    if (open < 0) {
      out += text.slice(i);
      break;
    }
    out += text.slice(i, open);
    const close = lower.indexOf(closeNeedle, open + openNeedle.length);
    if (close < 0) break;
    const end = text.indexOf('>', close + closeNeedle.length);
    i = end < 0 ? text.length : end + 1;
  }

  return out;
}

export function stripUnsupportedSvgText(svg = '') {
  return stripTagBlock(stripTagBlock(String(svg ?? ''), 'tspan'), 'text');
}

export function normalizeImageProps(attrs = {}) {
  const source = normalizeAttrs(attrs);
  const src = String(source.src ?? '');
  const alt = String(source.alt ?? '');
  const trimmedSrc = src.trim();
  const trimmedAlt = alt.trim();
  const svgMarkup = decodeSvgDataUri(src);
  const hasSrc = trimmedSrc.length > 0;

  return {
    src,
    alt,
    hasSrc,
    label: trimmedAlt.length > 0 ? alt : hasSrc ? src : 'img',
    svgMarkup,
    isSvgDataUri: svgMarkup != null,
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
