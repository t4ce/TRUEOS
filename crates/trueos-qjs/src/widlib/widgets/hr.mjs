const DEFAULT_RULE_HEIGHT = 2;
const DEFAULT_RULE_MARGIN_Y = 2;

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

export function nonNegativeNumberAttr(value) {
  if (value == null || value === '') return undefined;
  const number = Number(value);
  return Number.isFinite(number) && number >= 0 ? number : undefined;
}

export function normalizeHrMetrics(attrs = {}) {
  const source = normalizeAttrs(attrs);
  const height = nonNegativeNumberAttr(source.size) ?? nonNegativeNumberAttr(source.height) ?? DEFAULT_RULE_HEIGHT;
  const marginY = nonNegativeNumberAttr(source['data-margin-y']) ?? DEFAULT_RULE_MARGIN_Y;

  return {
    height,
    marginY,
  };
}

export function normalizeHrProps(attrs = {}) {
  return {
    role: 'separator',
    ...normalizeHrMetrics(attrs),
  };
}

export const WIDGET_DEFINITION = widgetDefinition('hr', {
  category: 'rule',
  leaf: true,
  layoutDefaults: {
    height: DEFAULT_RULE_HEIGHT,
    marginY: DEFAULT_RULE_MARGIN_Y,
  },
});

export const HR_WIDGET_DEFINITION = WIDGET_DEFINITION;
