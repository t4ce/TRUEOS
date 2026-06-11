const HEADING_TAGS = Object.freeze(['h1', 'h2', 'h3', 'h4', 'h5', 'h6']);

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

export function normalizeHeadingTag(tagName = '') {
  const tag = String(tagName ?? '').trim().toLowerCase();
  return HEADING_TAGS.includes(tag) ? tag : '';
}

export function isHeadingTag(tagName = '') {
  return normalizeHeadingTag(tagName).length > 0;
}

export function normalizeHeadingLevel(value = 1) {
  const level = Number(value);
  if (Number.isFinite(level) && level >= 1 && level <= 6) return Math.floor(level);

  const tag = normalizeHeadingTag(value);
  return tag ? Number(tag.charAt(1)) : 1;
}

export function headingTagFromLevel(value = 1) {
  return `h${normalizeHeadingLevel(value)}`;
}

export function normalizeHeadingProps(input = {}) {
  const tag = normalizeHeadingTag(input?.tag ?? input?.tagName ?? input?.nodeName ?? input) || 'h1';
  const level = normalizeHeadingLevel(tag);
  return {
    tag,
    level,
    role: 'heading',
    ariaLevel: level,
  };
}

export const HEADING_WIDGET_DEFINITIONS = HEADING_TAGS.map((tag) =>
  widgetDefinition(tag, {
    category: 'heading',
    layoutDefaults: {
      minHeight: 36,
      paddingTop: 6,
      paddingBottom: 6,
    },
  })
);

export const WIDGET_DEFINITIONS = HEADING_WIDGET_DEFINITIONS;
