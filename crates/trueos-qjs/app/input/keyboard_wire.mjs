export const KEYBOARD_MODIFIER_ALIASES = Object.freeze({
  alt: 'Alt',
  cmd: 'Meta',
  command: 'Meta',
  control: 'Ctrl',
  ctrl: 'Ctrl',
  meta: 'Meta',
  option: 'Alt',
  shift: 'Shift',
  super: 'Meta',
});

export const KEYBOARD_KEY_ALIASES = Object.freeze({
  backspace: 'Backspace',
  del: 'Delete',
  delete: 'Delete',
  down: 'ArrowDown',
  end: 'End',
  enter: 'Enter',
  esc: 'Escape',
  escape: 'Escape',
  home: 'Home',
  ins: 'Insert',
  insert: 'Insert',
  left: 'ArrowLeft',
  pagedown: 'PageDown',
  pageup: 'PageUp',
  pgdn: 'PageDown',
  pgdown: 'PageDown',
  pgup: 'PageUp',
  return: 'Enter',
  right: 'ArrowRight',
  space: 'Space',
  spacebar: 'Space',
  tab: 'Tab',
  up: 'ArrowUp',
});

export const KEYBOARD_NAMED_KEY_CODES = Object.freeze({
  Backspace: 1,
  Tab: 2,
  Enter: 3,
  Escape: 4,
  Space: 5,
  Delete: 6,
  Insert: 7,
  Home: 8,
  End: 9,
  PageUp: 10,
  PageDown: 11,
  ArrowUp: 12,
  ArrowDown: 13,
  ArrowLeft: 14,
  ArrowRight: 15,
  F1: 101,
  F2: 102,
  F3: 103,
  F4: 104,
  F5: 105,
  F6: 106,
  F7: 107,
  F8: 108,
  F9: 109,
  F10: 110,
  F11: 111,
  F12: 112,
});

const KEYBOARD_MODIFIER_BITS = Object.freeze({
  Shift: 1 << 0,
  Ctrl: 1 << 1,
  Alt: 1 << 2,
  Meta: 1 << 3,
});

function defaultRaise(message) {
  throw new Error(message);
}

export function normalizeKeyboardModifier(value) {
  const raw = String(value || '').trim();
  if (!raw) return '';
  const lowered = raw.toLowerCase();
  return KEYBOARD_MODIFIER_ALIASES[lowered] || '';
}

export function normalizeKeyboardModifiers(value) {
  const items = Array.isArray(value)
    ? value
    : (value == null ? [] : [value]);
  const out = [];
  const seen = new Set();
  for (let i = 0; i < items.length; i += 1) {
    const normalized = normalizeKeyboardModifier(items[i]);
    if (!normalized || seen.has(normalized)) continue;
    seen.add(normalized);
    out.push(normalized);
  }
  return out;
}

export function keyboardModifiersToMask(modifiers) {
  const list = normalizeKeyboardModifiers(modifiers);
  let mask = 0;
  for (let i = 0; i < list.length; i += 1) {
    mask |= KEYBOARD_MODIFIER_BITS[list[i]] || 0;
  }
  return mask >>> 0;
}

export function normalizeKeyboardKey(value) {
  const raw = String(value || '').trim();
  if (!raw) return '';
  if (raw.length === 1) return raw;
  const lowered = raw.toLowerCase().replace(/[\s_-]+/g, '');
  if (KEYBOARD_KEY_ALIASES[lowered]) {
    return KEYBOARD_KEY_ALIASES[lowered];
  }
  if (/^f\d{1,2}$/i.test(raw)) {
    return `F${raw.slice(1)}`;
  }
  return raw;
}

export function clampKeyboardRepeat(value) {
  const count = Number(value);
  if (!Number.isFinite(count)) return 1;
  return Math.max(1, Math.min(64, Math.floor(count)));
}

export function normalizeKeyboardEntry(entry, raise = defaultRaise) {
  if (typeof entry === 'string') {
    if (!entry) {
      raise('keyboard text entry is empty');
    }
    return { type: 'text', text: entry };
  }
  if (!entry || typeof entry !== 'object' || Array.isArray(entry)) {
    raise('keyboard entry must be a string or object');
  }

  const type = typeof entry.type === 'string' ? entry.type.trim().toLowerCase() : '';
  if (type === 'text' || (!type && typeof entry.text === 'string' && entry.key == null)) {
    const text = typeof entry.text === 'string' ? entry.text : String(entry.text || '');
    if (!text) {
      raise('keyboard text entry is empty');
    }
    return { type: 'text', text };
  }

  if (type === 'key' || entry.key != null) {
    const key = normalizeKeyboardKey(entry.key);
    if (!key) {
      raise('keyboard key entry is missing a key');
    }
    const modifiers = normalizeKeyboardModifiers(entry.modifiers || entry.mods);
    const repeat = clampKeyboardRepeat(entry.repeat);
    return { type: 'key', key, modifiers, repeat };
  }

  raise('keyboard entry must declare type=text or type=key');
}

export function parseKeyboardInput(input = null, options = null, raise = defaultRaise) {
  const source = input && typeof input === 'object' && !Array.isArray(input) ? input : null;
  const opts = options && typeof options === 'object' ? options : {};
  const entriesRaw = Array.isArray(input)
    ? input
    : (source && Array.isArray(source.events)
      ? source.events
      : [input]);
  const events = [];
  for (let i = 0; i < entriesRaw.length; i += 1) {
    const entry = normalizeKeyboardEntry(entriesRaw[i], raise);
    if (entry) events.push(entry);
  }
  if (events.length === 0) {
    raise('keyboard input did not contain any events');
  }

  const logOnly = source && Object.prototype.hasOwnProperty.call(source, 'logOnly')
    ? source.logOnly !== false
    : (opts.logOnly !== false);
  return { events, logOnly };
}

export function keyboardKeyToKernelSpec(value) {
  const key = normalizeKeyboardKey(value);
  if (!key) return null;
  if ([...key].length === 1) {
    return {
      key,
      codepoint: key.codePointAt(0) >>> 0,
      keyCode: 0,
    };
  }
  const keyCode = KEYBOARD_NAMED_KEY_CODES[key] || 0;
  if (!keyCode) {
    return null;
  }
  return {
    key,
    codepoint: 0,
    keyCode,
  };
}
