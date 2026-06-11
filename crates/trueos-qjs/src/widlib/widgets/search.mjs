import { flagAttr, normalizeAttrs, toFiniteNumber, widgetDefinition } from './textField.mjs';

export const SEARCH_BUTTON_LAYOUT_DEFAULTS = Object.freeze({
  width: 36,
  height: 36,
  minWidth: 36,
  minHeight: 36,
  marginRight: 6,
  flexGrow: 0,
  flexShrink: 0,
});

export const SEARCH_ROW_LAYOUT_DEFAULTS = Object.freeze({
  flexDirection: 'row',
  flexWrap: 'nowrap',
  alignItems: 'center',
  justifyContent: 'flex-start',
  paddingTop: 0,
  paddingRight: 0,
  paddingBottom: 0,
  paddingLeft: 0,
});

export function normalizeSearchAttrs(attrs = {}) {
  const source = normalizeAttrs(attrs);
  const width = source.width == null ? undefined : toFiniteNumber(source.width, undefined);

  return {
    value: String(source.value ?? ''),
    placeholder: String(source.placeholder ?? ''),
    width,
    disabled: flagAttr(source, 'disabled'),
  };
}

export function normalizeSearchState(attrs = {}, state = {}) {
  const source = normalizeSearchAttrs(attrs);
  const value = state.value ?? source.value;

  return {
    value: String(value ?? ''),
    placeholder: String(state.placeholder ?? source.placeholder ?? ''),
    width: state.width ?? source.width,
    disabled: state.disabled ?? source.disabled,
    focused: Boolean(state.focused),
  };
}

export function normalizeSearchButtonState(attrs = {}, state = {}) {
  const source = normalizeAttrs(attrs);

  return {
    focusInputKey: String(state.focusInputKey ?? source['data-focus-key'] ?? ''),
    disabled: state.disabled ?? flagAttr(source, 'disabled'),
    pressed: Boolean(state.pressed),
    hovered: Boolean(state.hovered),
  };
}

export function searchExpansion({ key = '', attrs = {} } = {}) {
  const state = normalizeSearchState(attrs);
  const inputKey = key ? `${key}:input` : '';
  const buttonKey = key ? `${key}:button` : '';

  return {
    tag: 'searchrow',
    key,
    children: [
      {
        tag: 'searchbutton',
        key: buttonKey,
        attrs: inputKey ? { 'data-focus-key': inputKey } : {},
      },
      {
        tag: 'input',
        key: inputKey,
        attrs: {
          type: 'search',
          value: state.value,
          placeholder: state.placeholder,
          disabled: state.disabled ? 'true' : undefined,
        },
      },
    ],
  };
}

export function magnifierGeometry({ width = 36, height = 36 } = {}) {
  const w = Math.max(0, toFiniteNumber(width, 36));
  const h = Math.max(0, toFiniteNumber(height, 36));
  const cx = w / 2 - 2;
  const cy = h / 2 - 2;
  const r = Math.max(5, Math.min(7, Math.min(w, h) * 0.22));

  return {
    circle: { cx, cy, r },
    handle: {
      x0: cx + r * 0.65,
      y0: cy + r * 0.65,
      x1: cx + r * 1.55,
      y1: cy + r * 1.55,
    },
  };
}

export const SEARCH_WIDGET_DEFINITION = widgetDefinition('search', {
  category: 'composite-control',
  kind: 'composite',
  leaf: true,
  interactive: true,
  complex: true,
  attrs: ['value', 'placeholder', 'width', 'disabled'],
  state: ['value', 'focused'],
  interactions: ['search', 'edit-text'],
  expandsTo: ['searchbutton', 'input.text'],
  currentStatus: 'represent-only',
});

export const SEARCH_ROW_WIDGET_DEFINITION = widgetDefinition('searchrow', {
  source: 'synthetic',
  category: 'layout',
  role: 'row',
  kind: 'container',
  layoutDefaults: SEARCH_ROW_LAYOUT_DEFAULTS,
  expandsTo: ['searchbutton', 'input.text'],
});

export const SEARCH_BUTTON_WIDGET_DEFINITION = widgetDefinition('searchbutton', {
  source: 'synthetic',
  category: 'form-control',
  leaf: true,
  interactive: true,
  layoutDefaults: SEARCH_BUTTON_LAYOUT_DEFAULTS,
  attrs: ['data-focus-key', 'disabled'],
  state: ['pressed', 'hovered'],
  interactions: ['focus-linked-input', 'press'],
});
