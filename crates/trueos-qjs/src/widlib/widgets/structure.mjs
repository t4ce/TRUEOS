const REPLACED_DIMENSION_ATTRS = new Set(['width', 'height']);

function structureWidgetDefinition(tag, overrides = {}) {
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

function numberAttr(value) {
  if (value == null || value === '') return undefined;
  const number = Number(value);
  return Number.isFinite(number) && number >= 0 ? number : undefined;
}

export function replacedDimensionsFromAttrs(attrs = {}) {
  const dimensions = {};

  for (const attr of REPLACED_DIMENSION_ATTRS) {
    const value = numberAttr(attrs[attr]);
    if (value !== undefined) dimensions[attr] = value;
  }

  return dimensions;
}

export function iframeSrcdocProps(attrs = {}) {
  return { srcdoc: String(attrs.srcdoc ?? '') };
}

const BASIC_STRUCTURE_TAGS = [
  'p',
  'div',
  'form',
  'label',
  'fieldset',
  'legend',
  'section',
  'article',
  'header',
  'footer',
  'main',
  'nav',
  'aside',
];

const HEADING_WIDGET_DEFINITIONS = ['h1', 'h2', 'h3', 'h4', 'h5', 'h6'].map((tag) =>
  structureWidgetDefinition(tag, {
    category: 'heading',
    layoutDefaults: { minHeight: 36, paddingY: 6 },
  })
);

const TABLE_WIDGET_DEFINITIONS = [
  structureWidgetDefinition('table', { category: 'table' }),
  structureWidgetDefinition('caption', { category: 'table' }),
  structureWidgetDefinition('tbody', { category: 'table' }),
  structureWidgetDefinition('thead', { category: 'table' }),
  structureWidgetDefinition('tfoot', { category: 'table' }),
  structureWidgetDefinition('tr', { category: 'table-row', role: 'row' }),
  structureWidgetDefinition('td', {
    category: 'table-cell',
    role: 'cell',
    layoutDefaults: { minWidth: 80, paddingX: 8, paddingY: 6 },
  }),
  structureWidgetDefinition('th', {
    category: 'table-cell',
    role: 'cell',
    layoutDefaults: { minWidth: 80, paddingX: 8, paddingY: 6 },
  }),
];

const MEDIA_WIDGET_DEFINITIONS = [
  structureWidgetDefinition('img', {
    category: 'replaced',
    leaf: true,
    complexity: 'complex',
    layoutDefaults: { width: 240, height: 140, minWidth: 120, minHeight: 80 },
    attrs: ['src', 'alt', 'width', 'height'],
  }),
  structureWidgetDefinition('canvas', {
    category: 'replaced',
    leaf: true,
    layoutDefaults: { width: 300, height: 150, minWidth: 120, minHeight: 80 },
    attrs: ['width', 'height'],
  }),
  structureWidgetDefinition('iframe', {
    category: 'replaced',
    leaf: true,
    complex: true,
    layoutDefaults: { width: 420, height: 240, minWidth: 200, minHeight: 160 },
    attrs: ['src', 'srcdoc', 'width', 'height'],
    currentStatus: 'represent-only',
  }),
];

export const STRUCTURE_WIDGET_DEFINITIONS = [
  structureWidgetDefinition('root', { role: 'root' }),
  structureWidgetDefinition('text', { role: 'text', leaf: true }),
  ...BASIC_STRUCTURE_TAGS.map((tag) =>
    structureWidgetDefinition(tag, tag === 'label' ? { role: 'inline-container' } : {})
  ),
  ...HEADING_WIDGET_DEFINITIONS,
  structureWidgetDefinition('hr', {
    category: 'rule',
    leaf: true,
    layoutDefaults: { height: 1, marginY: 2 },
  }),
  structureWidgetDefinition('details', {
    category: 'disclosure',
    interactive: true,
    complexity: 'complex',
    attrs: ['open'],
    state: ['open'],
  }),
  structureWidgetDefinition('summary', {
    category: 'disclosure',
    interactive: true,
    complexity: 'complex',
    interactions: ['toggle'],
  }),
  structureWidgetDefinition('dialog', {
    category: 'popup',
    interactive: true,
    complex: true,
    layoutDefaults: { width: 540, minWidth: 360, minHeight: 148 },
    state: ['open'],
    overlays: ['dialog'],
    currentStatus: 'represent-only',
  }),
  ...TABLE_WIDGET_DEFINITIONS,
  ...MEDIA_WIDGET_DEFINITIONS,
];
