export const RENDER_TRACE_VERSION = 2;
export const DEFAULT_VIEWPORT = Object.freeze({ width: 2560, height: 1224 });

function numberFrom(value, fallback) {
  const n = Number(value);
  return Number.isFinite(n) ? n : fallback;
}

function clampSize(value, fallback) {
  return Math.max(0, Math.round(numberFrom(value, fallback)));
}

export function stableObject(source) {
  if (!source || typeof source !== 'object' || Array.isArray(source)) return {};
  const out = {};
  const keys = Object.keys(source).sort();
  for (const key of keys) {
    const value = source[key];
    if (value === undefined || typeof value === 'function') continue;
    out[key] = value === null ? '' : String(value);
  }
  return out;
}

export function attrsOrUndefined(attrs) {
  const stable = stableObject(attrs);
  return Object.keys(stable).length > 0 ? stable : undefined;
}

export function textNode(text, opts = {}) {
  const node = { kind: 'text', text: String(text ?? '') };
  if (opts.preserveWhitespace === true) node.preserveWhitespace = true;
  return node;
}

export function blockNode(opts) {
  const node = {
    kind: 'block',
    key: String(opts?.key ?? ''),
    tagName: String(opts?.tagName ?? 'div'),
    children: Array.isArray(opts?.children) ? opts.children : [],
  };
  const attrs = attrsOrUndefined(opts?.attrs);
  if (attrs) node.attrs = attrs;
  return node;
}

export function normalizeViewport(viewport) {
  const source = viewport && typeof viewport === 'object' ? viewport : DEFAULT_VIEWPORT;
  return {
    width: clampSize(source.width, DEFAULT_VIEWPORT.width),
    height: clampSize(source.height, DEFAULT_VIEWPORT.height),
  };
}
