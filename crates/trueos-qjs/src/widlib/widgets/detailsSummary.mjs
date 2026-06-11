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

export function normalizeDetailsKey(input = {}) {
  const source = normalizeAttrs(input);
  const attrs = attrsFromInput(input);
  const rawKey = source.key ?? attrs['data-details-key'] ?? '';
  const key = String(rawKey ?? '').trim();
  return key.length > 0 ? key : undefined;
}

export function defaultDetailsOpen(input = {}) {
  const attrs = attrsFromInput(input);
  return flagAttr(attrs, 'open') || flagAttr(attrs, 'data-details-open');
}

export function resolveDetailsOpen(input = {}, detailsOpen = null) {
  const key = normalizeDetailsKey(input);
  if (key && detailsOpen && typeof detailsOpen.has === 'function' && detailsOpen.has(key)) {
    return detailsOpen.get(key) === true;
  }

  return defaultDetailsOpen(input);
}

export function normalizeDetailsState(input = {}, detailsOpen = null) {
  const key = normalizeDetailsKey(input);
  const open = resolveDetailsOpen(input, detailsOpen);

  return {
    key,
    open,
    defaultOpen: defaultDetailsOpen(input),
  };
}

export function nextDetailsOpen(input = {}, detailsOpen = null) {
  return !resolveDetailsOpen(input, detailsOpen);
}

export function isSummaryNode(node = {}) {
  const tag = String(node?.tag ?? node?.tagName ?? node?.nodeName ?? '').toLowerCase();
  return tag === 'summary';
}

export function getEffectiveDetailsChildren(node = {}, detailsOpen = null) {
  const tag = String(node?.tag ?? node?.tagName ?? node?.nodeName ?? '').toLowerCase();
  const children = Array.isArray(node?.children) ? node.children : Array.isArray(node?.childNodes) ? node.childNodes : [];
  if (tag !== 'details') return children;
  if (resolveDetailsOpen(node, detailsOpen)) return children;
  return children.filter((child) => isSummaryNode(child));
}

export const DETAILS_WIDGET_DEFINITION = widgetDefinition('details', {
  category: 'disclosure',
  interactive: true,
  complexity: 'complex',
  attrs: ['open'],
  state: ['open'],
});

export const SUMMARY_WIDGET_DEFINITION = widgetDefinition('summary', {
  category: 'disclosure',
  interactive: true,
  complexity: 'complex',
  interactions: ['toggle'],
  layoutDefaults: {
    minHeight: 36,
    paddingTop: 6,
    paddingRight: 12,
    paddingBottom: 6,
    paddingLeft: 26,
  },
});

export const WIDGET_DEFINITIONS = [DETAILS_WIDGET_DEFINITION, SUMMARY_WIDGET_DEFINITION];
