const TABLE_TAGS = Object.freeze(['table', 'caption', 'tbody', 'thead', 'tfoot', 'tr', 'td', 'th']);
const SECTION_TAGS = new Set(['tbody', 'thead', 'tfoot']);
const CELL_TAGS = new Set(['td', 'th']);

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

export function normalizeTableTag(tagName = '') {
  const tag = String(tagName ?? '').trim().toLowerCase();
  return TABLE_TAGS.includes(tag) ? tag : '';
}

export function isTableTag(tagName = '') {
  return normalizeTableTag(tagName).length > 0;
}

export function isTableSectionTag(tagName = '') {
  return SECTION_TAGS.has(String(tagName ?? '').trim().toLowerCase());
}

export function isTableCellTag(tagName = '') {
  return CELL_TAGS.has(String(tagName ?? '').trim().toLowerCase());
}

export function positiveIntegerAttr(value, fallback = 1) {
  const number = Number(value);
  if (!Number.isFinite(number)) return fallback;
  return Math.max(1, Math.floor(number));
}

export function parseCellSpan(attrs = {}) {
  const source = normalizeAttrs(attrs);
  return {
    colSpan: Math.min(1000, positiveIntegerAttr(source.colspan ?? source.colSpan, 1)),
    rowSpan: Math.min(65534, positiveIntegerAttr(source.rowspan ?? source.rowSpan, 1)),
  };
}

export function normalizeCellProps(tagName = 'td', attrs = {}) {
  const tag = normalizeTableTag(tagName) || 'td';
  const header = tag === 'th';

  return {
    tag,
    header,
    role: header ? 'columnheader' : 'cell',
    scope: String(normalizeAttrs(attrs).scope ?? ''),
    ...parseCellSpan(attrs),
  };
}

export function normalizeTableProps(tagName = 'table', attrs = {}) {
  const tag = normalizeTableTag(tagName) || 'table';
  const source = normalizeAttrs(attrs);

  return {
    tag,
    border: source.border == null ? undefined : Number(source.border),
    cellPadding: source.cellpadding == null ? undefined : Number(source.cellpadding),
    cellSpacing: source.cellspacing == null ? undefined : Number(source.cellspacing),
  };
}

export const TABLE_WIDGET_DEFINITIONS = [
  widgetDefinition('table', { category: 'table' }),
  widgetDefinition('caption', { category: 'table' }),
  widgetDefinition('tbody', { category: 'table' }),
  widgetDefinition('thead', { category: 'table' }),
  widgetDefinition('tfoot', { category: 'table' }),
  widgetDefinition('tr', { category: 'table-row', role: 'row' }),
  widgetDefinition('td', {
    category: 'table-cell',
    role: 'cell',
    attrs: ['colspan', 'rowspan'],
    layoutDefaults: { minWidth: 80, paddingX: 8, paddingY: 6 },
  }),
  widgetDefinition('th', {
    category: 'table-cell',
    role: 'cell',
    attrs: ['colspan', 'rowspan', 'scope'],
    layoutDefaults: { minWidth: 80, paddingX: 8, paddingY: 6 },
  }),
];

export const WIDGET_DEFINITIONS = TABLE_WIDGET_DEFINITIONS;
