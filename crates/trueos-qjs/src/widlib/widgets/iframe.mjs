const DEFAULT_IFRAME_WIDTH = 420;
const DEFAULT_IFRAME_HEIGHT = 240;
const DEFAULT_IFRAME_MIN_WIDTH = 200;
const DEFAULT_IFRAME_MIN_HEIGHT = 160;

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

export function positiveNumberAttr(value) {
  const number = Number(value);
  return Number.isFinite(number) && number > 0 ? number : undefined;
}

export function parseIframeDimensions(attrs = {}) {
  const source = normalizeAttrs(attrs);
  const attrWidth = positiveNumberAttr(source.width);
  const attrHeight = positiveNumberAttr(source.height);
  const width = attrWidth ?? DEFAULT_IFRAME_WIDTH;
  const height = attrHeight ?? DEFAULT_IFRAME_HEIGHT;

  return {
    width,
    height,
    minWidth: Math.min(DEFAULT_IFRAME_MIN_WIDTH, width),
    minHeight: Math.min(DEFAULT_IFRAME_MIN_HEIGHT, height),
    hasWidth: attrWidth !== undefined,
    hasHeight: attrHeight !== undefined,
    fixedSize: attrWidth !== undefined || attrHeight !== undefined,
  };
}

export function iframeSrcdocProps(attrs = {}) {
  return { srcdoc: String(normalizeAttrs(attrs).srcdoc ?? '') };
}

export function normalizeIframeSandbox(attrs = {}) {
  return String(normalizeAttrs(attrs).sandbox ?? '')
    .split(/\s+/)
    .map((token) => token.trim())
    .filter((token) => token.length > 0);
}

export function isIframeRoot(attrs = {}) {
  return String(normalizeAttrs(attrs)['data-root'] ?? '') === '1';
}

export function normalizeIframeProps(attrs = {}) {
  const source = normalizeAttrs(attrs);
  const root = isIframeRoot(source);

  return {
    src: String(source.src ?? ''),
    srcdoc: String(source.srcdoc ?? ''),
    title: String(source.title ?? ''),
    name: String(source.name ?? ''),
    allow: String(source.allow ?? ''),
    sandbox: normalizeIframeSandbox(source),
    credentialless: flagAttr(source, 'credentialless'),
    loading: String(source.loading ?? '').toLowerCase(),
    root,
    hint: String(source.srcdoc ?? '').trim().length > 0 ? 'srcdoc' : source.src ? 'src' : 'empty',
    ...(root
      ? { width: undefined, height: undefined, minWidth: 0, minHeight: 0, fixedSize: false }
      : parseIframeDimensions(source)),
  };
}

export const WIDGET_DEFINITION = widgetDefinition('iframe', {
  category: 'replaced',
  leaf: true,
  complex: true,
  layoutDefaults: {
    width: DEFAULT_IFRAME_WIDTH,
    height: DEFAULT_IFRAME_HEIGHT,
    minWidth: DEFAULT_IFRAME_MIN_WIDTH,
    minHeight: DEFAULT_IFRAME_MIN_HEIGHT,
    paddingTop: 34,
    paddingRight: 8,
    paddingBottom: 8,
    paddingLeft: 8,
  },
  attrs: ['src', 'srcdoc', 'width', 'height'],
  currentStatus: 'represent-only',
});

export const IFRAME_WIDGET_DEFINITION = WIDGET_DEFINITION;
