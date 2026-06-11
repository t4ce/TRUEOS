import { clampNumber, flagAttr, normalizeAttrs, nodeChildren, textFromNode, widgetDefinition } from './textField.mjs';

export const SELECT_LAYOUT_DEFAULTS = Object.freeze({
  height: 36,
  minHeight: 36,
  minWidth: 220,
  paddingTop: 0,
  paddingRight: 0,
  paddingBottom: 0,
  paddingLeft: 0,
  arrowWidth: 22,
  popupItemHeight: 30,
  popupMaxVisible: 7,
});

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
  const max = Math.max(0, options.length - 1);

  if (Number.isFinite(raw)) return Math.max(0, Math.min(max, raw | 0));

  const selectedIndex = options.findIndex((option) => option.selected);
  return selectedIndex >= 0 ? selectedIndex : 0;
}

export function normalizeSelectState(input = {}, state = {}) {
  const source = normalizeAttrs(input);
  const attrs = normalizeAttrs(source.attrs ?? source);
  const options = parseSelectOptions(input);
  const selectedIndex = state.selectedIndex == null ? parseSelectedIndex(input, options) : clampNumber(state.selectedIndex, 0, Math.max(0, options.length - 1)) | 0;

  return {
    options,
    selectedIndex,
    selectedOption: options[selectedIndex] ?? null,
    multiple: state.multiple ?? flagAttr(attrs, 'multiple'),
    disabled: state.disabled ?? flagAttr(attrs, 'disabled'),
    open: Boolean(state.open ?? source.open),
  };
}

export function toggleSelectOpen(state = {}) {
  return { ...state, open: !Boolean(state.open) };
}

export function chooseSelectOption(state = {}, index = 0) {
  const options = Array.isArray(state.options) ? state.options : [];
  const selectedIndex = clampNumber(index, 0, Math.max(0, options.length - 1)) | 0;
  return {
    ...state,
    selectedIndex,
    selectedOption: options[selectedIndex] ?? null,
    open: false,
  };
}

export function visibleSelectOptions(options = [], { maxVisible = SELECT_LAYOUT_DEFAULTS.popupMaxVisible } = {}) {
  const limit = Math.max(0, Number(maxVisible) | 0);
  return Array.isArray(options) ? options.slice(0, limit) : [];
}

export function selectPopupPlacement({
  absX = 0,
  absY = 0,
  width = 0,
  height = SELECT_LAYOUT_DEFAULTS.height,
  optionCount = 0,
  viewportW = 0,
  viewportH = 0,
  itemHeight = SELECT_LAYOUT_DEFAULTS.popupItemHeight,
  maxVisible = SELECT_LAYOUT_DEFAULTS.popupMaxVisible,
  margin = 4,
} = {}) {
  const visibleCount = Math.min(Math.max(0, optionCount | 0), Math.max(0, maxVisible | 0));
  const panelH = visibleCount * Math.max(0, itemHeight);
  let px = Number(absX) || 0;
  let py = (Number(absY) || 0) + (Number(height) || 0);

  px = Math.max(0, Math.min(px, Math.max(0, (Number(viewportW) || 0) - (Number(width) || 0))));

  if (py + panelH > (Number(viewportH) || 0) - margin) {
    py = (Number(absY) || 0) - panelH;
  }
  py = Math.max(0, Math.min(py, Math.max(0, (Number(viewportH) || 0) - panelH)));

  return {
    x: px,
    y: py,
    width: Math.max(0, Number(width) || 0),
    height: panelH,
    itemHeight: Math.max(0, itemHeight),
    visibleCount,
    opensAbove: py < (Number(absY) || 0),
  };
}

export function downChevronPath({ x = 0, y = 0, width = 22, height = 36, pad = 4 } = {}) {
  const x0 = x + pad;
  const x1 = x + width - pad;
  const y0 = y + pad;
  const y1 = y + height - pad;
  const midY = (y0 + y1) / 2;

  return [
    { x: x0, y: midY - 2 },
    { x: (x0 + x1) / 2, y: midY + 2 },
    { x: x1, y: midY - 2 },
  ];
}

export const SELECT_WIDGET_DEFINITION = widgetDefinition('select', {
  category: 'choice-control',
  leaf: true,
  interactive: true,
  complexity: 'complex',
  layoutDefaults: SELECT_LAYOUT_DEFAULTS,
  attrs: ['disabled', 'multiple', 'data-options', 'data-selected-index'],
  state: ['selectedIndex', 'open'],
  interactions: ['open-popup', 'choose-option'],
  overlays: ['select-popup'],
});
