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

export const BARROW_WIDGET_DEFINITION = widgetDefinition('barrow', {
  source: 'synthetic',
  category: 'layout',
  role: 'row',
  layoutDefaults: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'flex-start',
    paddingLeft: 8,
    paddingRight: 0,
    paddingTop: 0,
    paddingBottom: 0,
  },
});

export function normalizeBarrowLayout(attrs = {}) {
  const source = normalizeAttrs(attrs);
  const left = Number(source.paddingLeft ?? source['padding-left'] ?? 8);
  const right = Number(source.paddingRight ?? source['padding-right'] ?? 0);
  const top = Number(source.paddingTop ?? source['padding-top'] ?? 0);
  const bottom = Number(source.paddingBottom ?? source['padding-bottom'] ?? 0);

  return {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'flex-start',
    paddingLeft: Number.isFinite(left) ? Math.max(0, left) : 8,
    paddingRight: Number.isFinite(right) ? Math.max(0, right) : 0,
    paddingTop: Number.isFinite(top) ? Math.max(0, top) : 0,
    paddingBottom: Number.isFinite(bottom) ? Math.max(0, bottom) : 0,
  };
}

export function barrowProps(attrs = {}) {
  return {
    attrs: normalizeAttrs(attrs),
    layout: normalizeBarrowLayout(attrs),
  };
}
