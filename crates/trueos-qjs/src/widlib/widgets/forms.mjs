const CHECKABLE_INPUT_TYPES = new Set(['checkbox', 'radio']);
const TEMPORAL_INPUT_TYPES = new Set(['time', 'date', 'month', 'week', 'datetime-local']);
const TEXT_INPUT_TYPES = new Set(['text', 'search', 'password', 'email', 'url', 'tel']);

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

function normalizeToken(value, fallback = '') {
  const token = String(value ?? fallback)
    .trim()
    .toLowerCase();
  return token.length > 0 ? token : String(fallback).toLowerCase();
}

function flagAttr(attrs, name) {
  const source = normalizeAttrs(attrs);
  return hasOwn(source, name) && source[name] !== false && source[name] !== 'false';
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
    category: overrides.category ?? 'form-control',
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

export function normalizeInputType(attrs = {}) {
  return normalizeToken(normalizeAttrs(attrs).type, 'text');
}

export function isCheckableInputType(type) {
  return CHECKABLE_INPUT_TYPES.has(normalizeToken(type, 'text'));
}

export function isTemporalInputType(type) {
  return TEMPORAL_INPUT_TYPES.has(normalizeToken(type, 'text'));
}

export function isTextInputType(type) {
  const normalized = normalizeToken(type, 'text');
  return TEXT_INPUT_TYPES.has(normalized) || TEMPORAL_INPUT_TYPES.has(normalized);
}

export function classifyInput(attrs = {}) {
  const type = normalizeInputType(attrs);
  const isCheckable = isCheckableInputType(type);
  const isTemporal = isTemporalInputType(type);
  const isText = isTextInputType(type);

  return {
    id: isCheckable ? `input.${type}` : isTemporal ? `input.${type}` : 'input.text',
    subtype: type,
    category: isCheckable ? 'choice-control' : isText ? 'text-control' : 'form-control',
    kind: 'leaf',
    complexity: isTemporal ? 'complex' : 'basic',
    currentStatus: isTemporal ? 'defer-special-ui' : 'basic',
    leaf: true,
    interactive: true,
    state: isCheckable ? ['checked', 'indeterminate'] : ['value'],
  };
}

export function normalizeButtonType(attrs = {}) {
  const type = normalizeToken(normalizeAttrs(attrs).type, 'submit');
  return type === 'button' || type === 'reset' || type === 'submit' ? type : 'submit';
}

export function normalizeTextControlState(attrs = {}, fallbackValue = '') {
  const source = normalizeAttrs(attrs);
  const value = source.value ?? fallbackValue;

  return {
    value: String(value ?? ''),
    placeholder: String(source.placeholder ?? ''),
    disabled: flagAttr(source, 'disabled'),
  };
}

function nodeChildren(node) {
  if (!node || typeof node !== 'object') return [];
  if (Array.isArray(node.children)) return node.children;
  if (Array.isArray(node.childNodes)) return node.childNodes;
  return [];
}

function textFromNode(node) {
  if (typeof node === 'string') return node;
  if (!node || typeof node !== 'object') return '';
  if (node.text != null) return node.text;
  if (node.textContent != null) return node.textContent;
  if (node.value != null && String(node.nodeName ?? '').toLowerCase() === '#text') return node.value;

  const children = nodeChildren(node);
  if (children.length === 0) return '';
  return children.map((child) => textFromNode(child)).join('');
}

function textFromOption(option, attrs = {}) {
  if (typeof option === 'string') return option;
  if (!option || typeof option !== 'object') return '';
  if (option.label != null) return option.label;
  if (attrs.label != null) return attrs.label;
  const childText = textFromNode(option);
  if (childText.length > 0) return childText;
  if (option.text != null) return option.text;
  if (option.textContent != null) return option.textContent;
  if (option.value != null) return option.value;
  if (attrs.value != null) return attrs.value;
  return '';
}

export function normalizeSelectOption(option, index = 0) {
  const source = option && typeof option === 'object' ? option : {};
  const attrs = normalizeAttrs(source.attrs);
  const rawLabel = textFromOption(option, attrs);
  const label = String(rawLabel ?? '').trim();
  const value = source.value ?? attrs.value ?? label;

  return {
    label,
    value: String(value ?? ''),
    disabled: Boolean(source.disabled) || flagAttr(attrs, 'disabled'),
    selected: Boolean(source.selected) || flagAttr(attrs, 'selected'),
    index,
  };
}

function collectSelectOptionNodes(children, out = []) {
  for (const child of children) {
    const tag = String(child?.tag ?? child?.tagName ?? child?.nodeName ?? '').toLowerCase();
    if (tag === 'option') out.push(child);
    else if (tag === 'optgroup') collectSelectOptionNodes(nodeChildren(child), out);
  }
  return out;
}

export function parseSelectOptions(input = {}) {
  const source = normalizeAttrs(input);
  const attrs = normalizeAttrs(source.attrs ?? source);
  const rawOptions = Array.isArray(input) ? input : Array.isArray(source.options) ? source.options : null;

  if (rawOptions) {
    return rawOptions
      .map((option, index) => normalizeSelectOption(option, index))
      .filter((option) => option.label.length > 0 || option.value.length > 0);
  }

  const childOptions = collectSelectOptionNodes(nodeChildren(source));

  if (childOptions.length > 0) {
    return childOptions
      .map((option, index) => normalizeSelectOption(option, index))
      .filter((option) => option.label.length > 0 || option.value.length > 0);
  }

  const dataOptions = String(attrs['data-options'] ?? '');
  if (dataOptions.length === 0) return [];

  return dataOptions
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((label, index) => normalizeSelectOption({ label, value: label }, index));
}

export function parseSelectedIndex(input = {}, options = parseSelectOptions(input)) {
  const source = normalizeAttrs(input);
  const attrs = normalizeAttrs(source.attrs ?? source);
  const raw = Number(attrs['data-selected-index'] ?? source.selectedIndex);

  if (Number.isFinite(raw)) {
    return Math.max(0, Math.min(Math.max(0, options.length - 1), raw | 0));
  }

  const selectedIndex = options.findIndex((option) => option.selected);
  return selectedIndex >= 0 ? selectedIndex : 0;
}

export function normalizeSelectState(input = {}) {
  const source = normalizeAttrs(input);
  const attrs = normalizeAttrs(source.attrs ?? source);
  const options = parseSelectOptions(input);

  return {
    options,
    selectedIndex: parseSelectedIndex(input, options),
    multiple: flagAttr(attrs, 'multiple'),
    disabled: flagAttr(attrs, 'disabled'),
    open: Boolean(source.open),
  };
}

export function normalizeSearchAttrs(attrs = {}) {
  const source = normalizeAttrs(attrs);
  return {
    value: String(source.value ?? ''),
    placeholder: String(source.placeholder ?? ''),
    width: source.width == null ? undefined : Number(source.width),
    disabled: flagAttr(source, 'disabled'),
  };
}

export const FORM_WIDGET_DEFINITIONS = [
  widgetDefinition('button', {
    category: 'form-control',
    interactive: true,
    layoutDefaults: { minWidth: 100, minHeight: 36, paddingY: 6 },
    attrs: ['type', 'disabled'],
    state: ['pressed'],
    interactions: ['press'],
    classify: (attrs) => ({ subtype: normalizeButtonType(attrs) }),
  }),
  widgetDefinition('input', {
    category: 'form-control',
    leaf: true,
    interactive: true,
    layoutDefaults: { height: 36, minWidth: 220, checkboxSize: 16, radioSize: 16 },
    attrs: ['type', 'value', 'checked', 'name', 'placeholder', 'disabled'],
    state: ['value'],
    classify: classifyInput,
  }),
  widgetDefinition('textarea', {
    category: 'text-control',
    leaf: true,
    interactive: true,
    complexity: 'complex',
    layoutDefaults: { height: 108, minWidth: 220 },
    attrs: ['value', 'placeholder', 'disabled'],
    state: ['value'],
    interactions: ['edit-text', 'select-text'],
  }),
  widgetDefinition('select', {
    category: 'choice-control',
    leaf: true,
    interactive: true,
    complexity: 'complex',
    layoutDefaults: { height: 36, minWidth: 220 },
    attrs: ['disabled', 'multiple'],
    state: ['selectedIndex', 'open'],
    interactions: ['open-popup', 'choose-option'],
    overlays: ['select-popup'],
  }),
  widgetDefinition('search', {
    category: 'composite-control',
    kind: 'composite',
    leaf: true,
    interactive: true,
    complex: true,
    attrs: ['value', 'placeholder', 'width'],
    state: ['value'],
    interactions: ['search', 'edit-text'],
    expandsTo: ['searchbutton', 'input.text'],
    currentStatus: 'represent-only',
  }),
  widgetDefinition('searchrow', {
    source: 'synthetic',
    category: 'layout',
    role: 'row',
    kind: 'container',
    layoutDefaults: { flexDirection: 'row', alignItems: 'center' },
    expandsTo: ['searchbutton', 'input.text'],
    currentStatus: 'basic',
  }),
  widgetDefinition('searchbutton', {
    source: 'synthetic',
    category: 'form-control',
    leaf: true,
    interactive: true,
    layoutDefaults: { width: 36, height: 36, minWidth: 36, minHeight: 36, marginRight: 6 },
    attrs: ['data-focus-key', 'disabled'],
    state: ['pressed'],
    interactions: ['focus-linked-input', 'press'],
  }),
];
