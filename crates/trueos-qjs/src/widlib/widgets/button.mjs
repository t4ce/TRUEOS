import { flagAttr, normalizeAttrs, normalizeToken, textFromNode, widgetDefinition } from './textField.mjs';

export const BUTTON_LAYOUT_DEFAULTS = Object.freeze({
  minWidth: 100,
  minHeight: 36,
  paddingTop: 6,
  paddingBottom: 6,
  alignItems: 'center',
  justifyContent: 'center',
  flexDirection: 'row',
});

export function normalizeButtonType(attrs = {}) {
  const type = normalizeToken(normalizeAttrs(attrs).type, 'submit');
  return type === 'button' || type === 'reset' || type === 'submit' ? type : 'submit';
}

export function normalizeButtonLabel(input = {}) {
  if (typeof input === 'string') return input.trim();

  const source = input && typeof input === 'object' ? input : {};
  const attrs = normalizeAttrs(source.attrs ?? source);
  const raw = source.label ?? attrs.label ?? attrs.value ?? textFromNode(source);
  return String(raw ?? '').trim();
}

export function normalizeButtonState(attrs = {}, state = {}) {
  const source = normalizeAttrs(attrs);

  return {
    type: normalizeButtonType(source),
    label: normalizeButtonLabel({ attrs: source, children: state.children ?? [] }),
    disabled: state.disabled ?? flagAttr(source, 'disabled'),
    pressed: Boolean(state.pressed),
    hovered: Boolean(state.hovered),
    active: Boolean(state.active ?? state.pressed),
  };
}

export function buttonVisualState(state = {}) {
  const normalized = normalizeButtonState({}, state);
  if (normalized.disabled) return 'disabled';
  if (normalized.active) return 'active';
  if (normalized.hovered) return 'hover';
  return 'default';
}

export const BUTTON_WIDGET_DEFINITION = widgetDefinition('button', {
  category: 'form-control',
  interactive: true,
  layoutDefaults: BUTTON_LAYOUT_DEFAULTS,
  attrs: ['type', 'value', 'disabled'],
  state: ['pressed', 'hovered', 'active'],
  interactions: ['press'],
  classify: (attrs) => ({ subtype: normalizeButtonType(attrs) }),
});
