import { REPLACED_TAGS } from './tags.mjs';
import { DEFAULT_WIDGET_DEFINITIONS } from './widgets/index.mjs';

function baseDefinition(tag, overrides = {}) {
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

function normalizeDefinition(definition) {
  return baseDefinition(String(definition.tag ?? definition.id ?? 'unknown').toLowerCase(), definition);
}

export function createWidgetRegistry(definitions = DEFAULT_WIDGET_DEFINITIONS) {
  const map = new Map();

  for (const definition of definitions) {
    const normalized = normalizeDefinition(definition);
    map.set(normalized.tag, normalized);
  }

  return {
    get(tag, attrs = {}) {
      const key = String(tag ?? '').toLowerCase();
      const definition = map.get(key) ?? baseDefinition(key || 'unknown', {
        category: REPLACED_TAGS.has(key) ? 'replaced' : 'structure',
        leaf: REPLACED_TAGS.has(key),
        currentStatus: 'unknown',
      });
      const classified = typeof definition.classify === 'function' ? definition.classify(attrs) : {};
      return { ...definition, ...classified };
    },

    has(tag) {
      return map.has(String(tag ?? '').toLowerCase());
    },

    register(tag, definition) {
      const normalized = normalizeDefinition({ ...definition, tag: String(tag).toLowerCase() });
      map.set(normalized.tag, normalized);
      return this;
    },

    entries() {
      return Array.from(map.values()).map((definition) => ({ ...definition }));
    },
  };
}

export { DEFAULT_WIDGET_DEFINITIONS };

export const defaultRegistry = createWidgetRegistry();
