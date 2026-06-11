import {
  clampNumber,
  flagAttr,
  normalizeAttrs,
  normalizeSelectionRange,
  normalizeSelections,
  normalizeToken,
  getCaretIndexFromPoint,
  textFieldLayout,
  textFieldPresentation,
  widgetDefinition,
} from './textField.mjs';

export const CHECKABLE_INPUT_TYPES = Object.freeze(['checkbox', 'radio']);
export const TEMPORAL_INPUT_TYPES = Object.freeze(['time', 'date', 'month', 'week', 'datetime-local']);
export const TEXT_INPUT_TYPES = Object.freeze(['text', 'search', 'password', 'email', 'url', 'tel']);

export const INPUT_LAYOUT_DEFAULTS = Object.freeze({
  height: 36,
  minHeight: 36,
  minWidth: 220,
  paddingTop: 6,
  paddingBottom: 6,
  checkboxSize: 16,
  radioSize: 16,
  checkableMarginRight: 6,
});

export function normalizeInputType(attrs = {}) {
  return normalizeToken(normalizeAttrs(attrs).type, 'text');
}

export function isCheckableInputType(type) {
  return CHECKABLE_INPUT_TYPES.includes(normalizeToken(type, 'text'));
}

export function isTemporalInputType(type) {
  return TEMPORAL_INPUT_TYPES.includes(normalizeToken(type, 'text'));
}

export function isTextInputType(type) {
  const normalized = normalizeToken(type, 'text');
  return TEXT_INPUT_TYPES.includes(normalized) || TEMPORAL_INPUT_TYPES.includes(normalized);
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

export function inputLayoutDefaults(type = 'text') {
  const normalized = normalizeToken(type, 'text');
  if (isCheckableInputType(normalized)) {
    const size = normalized === 'radio' ? INPUT_LAYOUT_DEFAULTS.radioSize : INPUT_LAYOUT_DEFAULTS.checkboxSize;
    return {
      width: size,
      height: size,
      minWidth: size,
      paddingTop: 0,
      paddingRight: 0,
      paddingBottom: 0,
      paddingLeft: 0,
      marginRight: INPUT_LAYOUT_DEFAULTS.checkableMarginRight,
    };
  }

  return {
    height: INPUT_LAYOUT_DEFAULTS.height,
    minHeight: INPUT_LAYOUT_DEFAULTS.minHeight,
    minWidth: isTemporalInputType(normalized) && normalized === 'datetime-local' ? 340 : INPUT_LAYOUT_DEFAULTS.minWidth,
    paddingTop: INPUT_LAYOUT_DEFAULTS.paddingTop,
    paddingBottom: INPUT_LAYOUT_DEFAULTS.paddingBottom,
  };
}

export function normalizeInputState(attrs = {}, state = {}) {
  const source = normalizeAttrs(attrs);
  const type = normalizeInputType(source);
  const value = String(state.value ?? source.value ?? '');
  const checked = state.checked ?? flagAttr(source, 'checked');
  const indeterminate = state.indeterminate ?? flagAttr(source, 'indeterminate');

  if (isCheckableInputType(type)) {
    return {
      type,
      name: String(state.name ?? source.name ?? ''),
      value,
      checked: Boolean(checked),
      indeterminate: type === 'checkbox' && Boolean(indeterminate),
      disabled: state.disabled ?? flagAttr(source, 'disabled'),
      required: state.required ?? flagAttr(source, 'required'),
    };
  }

  return {
    type,
    name: String(state.name ?? source.name ?? ''),
    value,
    placeholder: String(state.placeholder ?? source.placeholder ?? ''),
    disabled: state.disabled ?? flagAttr(source, 'disabled'),
    readOnly: state.readOnly ?? flagAttr(source, 'readonly'),
    required: state.required ?? flagAttr(source, 'required'),
    selections: normalizeSelections(state.selections, value.length),
  };
}

export function inputDisplayValue(state = {}, passwordMask = '\u2022') {
  const normalized = normalizeInputState({ type: state.type }, state);
  if (normalized.type === 'password') return String(passwordMask).repeat(normalized.value.length);
  return normalized.value ?? '';
}

export function toggleCheckboxState(state = {}, { triState = true } = {}) {
  const checked = state.checked === true;
  const indeterminate = state.indeterminate === true;

  if (!triState) {
    return { ...state, checked: !checked, indeterminate: false };
  }

  if (!checked && !indeterminate) return { ...state, checked: true, indeterminate: false };
  if (checked && !indeterminate) return { ...state, checked: false, indeterminate: true };
  return { ...state, checked: false, indeterminate: false };
}

export function radioGroupSelection(keys = [], selectedKey = null) {
  const selected = String(selectedKey ?? '');
  const out = {};
  for (const key of keys) out[String(key)] = String(key) === selected;
  return out;
}

export function selectRadioState(stateByKey = {}, selectedKey = null) {
  const selected = String(selectedKey ?? '');
  const out = {};
  for (const [key, state] of Object.entries(stateByKey)) {
    out[key] = { ...state, checked: key === selected };
  }
  return out;
}

export function inputFieldBounds({
  x = 0,
  y = 0,
  width = 0,
  height = INPUT_LAYOUT_DEFAULTS.height,
  fontSize = 16,
  type = 'text',
  maxLines = 5,
  baselineNudgeY = 0,
} = {}) {
  const layout = textFieldLayout({ width, height, fontSize, maxLines, baselineNudgeY });
  const absX = Number.isFinite(Number(x)) ? Number(x) : 0;
  const absY = Number.isFinite(Number(y)) ? Number(y) : 0;

  return {
    x: absX,
    y: absY,
    w: layout.w,
    h: layout.h,
    innerLeft: layout.innerLeft,
    innerTop: layout.innerTop,
    innerWidth: layout.innerWidth,
    maxLines: layout.maxLines,
    isPassword: normalizeToken(type, 'text') === 'password',
  };
}

export function inputTextPresentation({
  attrs = {},
  state = {},
  width = 0,
  height = INPUT_LAYOUT_DEFAULTS.height,
  fontSize = 16,
  measure,
  maxLines = 5,
  baselineNudgeY = 0,
  passwordMask = '\u2022',
} = {}) {
  const normalized = normalizeInputState(attrs, state);
  const shown = normalized.type === 'password' ? String(passwordMask).repeat(normalized.value.length) : normalized.value;
  const presentation = textFieldPresentation({ value: shown, width, height, fontSize, measure, maxLines, baselineNudgeY });

  return {
    ...presentation,
    type: normalized.type,
    value: normalized.value,
    shown,
    isPassword: normalized.type === 'password',
    selections: normalizeSelections(normalized.selections, shown.length),
  };
}

export function caretSelectionFromPoint({ presentation, localX = 0, localY = 0, measure, pointerId = 0 } = {}) {
  const lines = presentation?.visibleLines ?? [];
  const text = presentation?.shown ?? presentation?.text ?? '';
  const lineHeight = presentation?.layout?.lineHeight ?? 1;
  const idx = getCaretIndexFromPoint({ fullText: text, lines, localX, localY, lineHeight, measure });

  return {
    pointerId: Number(pointerId),
    ...normalizeSelectionRange({ start: idx, end: idx }, String(text).length),
  };
}

export const INPUT_WIDGET_DEFINITION = widgetDefinition('input', {
  category: 'form-control',
  leaf: true,
  interactive: true,
  layoutDefaults: INPUT_LAYOUT_DEFAULTS,
  attrs: ['type', 'value', 'checked', 'name', 'placeholder', 'disabled', 'readonly', 'required'],
  state: ['value', 'checked', 'indeterminate', 'selectionStart', 'selectionEnd'],
  interactions: ['edit-text', 'select-text', 'toggle', 'choose-radio'],
  classify: classifyInput,
});
