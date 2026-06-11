import {
  flagAttr,
  normalizeAttrs,
  normalizeSelections,
  textFieldLayout,
  textFieldPresentation,
  widgetDefinition,
} from './textField.mjs';

export const TEXTAREA_LAYOUT_DEFAULTS = Object.freeze({
  height: 108,
  minHeight: 108,
  minWidth: 220,
  paddingTop: 6,
  paddingBottom: 6,
  maxLines: 5,
});

export function normalizeTextareaValue(attrs = {}, fallbackValue = '') {
  const source = normalizeAttrs(attrs);
  return String(source.value ?? fallbackValue ?? '');
}

export function normalizeTextareaState(attrs = {}, state = {}) {
  const source = normalizeAttrs(attrs);
  const value = String(state.value ?? source.value ?? '');

  return {
    value,
    placeholder: String(state.placeholder ?? source.placeholder ?? ''),
    disabled: state.disabled ?? flagAttr(source, 'disabled'),
    readOnly: state.readOnly ?? flagAttr(source, 'readonly'),
    required: state.required ?? flagAttr(source, 'required'),
    rows: source.rows == null ? undefined : Math.max(1, Number(source.rows) | 0),
    cols: source.cols == null ? undefined : Math.max(1, Number(source.cols) | 0),
    selections: normalizeSelections(state.selections, value.length),
  };
}

export function textareaFieldBounds({
  x = 0,
  y = 0,
  width = 0,
  height = TEXTAREA_LAYOUT_DEFAULTS.height,
  fontSize = 16,
  maxLines = TEXTAREA_LAYOUT_DEFAULTS.maxLines,
  baselineNudgeY = 0,
} = {}) {
  const layout = textFieldLayout({ width, height, fontSize, maxLines, baselineNudgeY });

  return {
    x,
    y,
    w: layout.w,
    h: layout.h,
    innerLeft: layout.innerLeft,
    innerTop: layout.innerTop,
    innerWidth: layout.innerWidth,
    maxLines: layout.maxLines,
    isPassword: false,
  };
}

export function textareaPresentation({
  attrs = {},
  state = {},
  width = 0,
  height = TEXTAREA_LAYOUT_DEFAULTS.height,
  fontSize = 16,
  measure,
  maxLines = TEXTAREA_LAYOUT_DEFAULTS.maxLines,
  baselineNudgeY = 0,
} = {}) {
  const normalized = normalizeTextareaState(attrs, state);
  const presentation = textFieldPresentation({
    value: normalized.value,
    width,
    height,
    fontSize,
    measure,
    maxLines,
    baselineNudgeY,
  });

  return {
    ...presentation,
    value: normalized.value,
    selections: normalized.selections,
  };
}

export const TEXTAREA_WIDGET_DEFINITION = widgetDefinition('textarea', {
  category: 'text-control',
  leaf: true,
  interactive: true,
  complexity: 'complex',
  layoutDefaults: TEXTAREA_LAYOUT_DEFAULTS,
  attrs: ['value', 'placeholder', 'disabled', 'readonly', 'required', 'rows', 'cols'],
  state: ['value', 'selectionStart', 'selectionEnd'],
  interactions: ['edit-text', 'select-text', 'move-caret'],
});
