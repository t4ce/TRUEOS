const DEFAULT_RGBA = Object.freeze({ r: 255, g: 0, b: 0, a: 255 });

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
    category: overrides.category ?? 'value-control',
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

export const COLOR_WIDGET_DEFINITION = widgetDefinition('color', {
  kind: 'composite',
  leaf: true,
  interactive: true,
  complex: true,
  layoutDefaults: { width: 240, height: 200, minWidth: 240, minHeight: 200, paddingLeft: 0, paddingRight: 0, paddingTop: 0, paddingBottom: 0 },
  attrs: ['value', 'width', 'height'],
  state: ['r', 'g', 'b', 'a', 'rgb', 'alpha'],
  interactions: ['pick-color'],
  currentStatus: 'defer-composite-ui',
});

export const COLOR_PICKER_VERTEX_COLORS = Object.freeze([
  255, 0, 0, 255,
  128, 128, 0, 255,
  0, 255, 0, 255,
  0, 128, 128, 255,
  0, 0, 255, 255,
  128, 0, 128, 255,
]);

export const COLOR_PICKER_TRI_INDICES = Object.freeze([
  0, 1, 2,
  0, 2, 3,
  0, 3, 4,
  0, 4, 5,
]);

export function toFiniteNumber(value, fallback = 0) {
  const n = Number(value);
  return Number.isFinite(n) ? n : fallback;
}

export function clampByte(value, fallback = 0) {
  return Math.max(0, Math.min(255, Math.round(toFiniteNumber(value, fallback))));
}

function hexByte(hex) {
  const n = Number.parseInt(hex, 16);
  return Number.isFinite(n) ? n : 0;
}

function parseHexColor(value) {
  const raw = String(value ?? '').trim();
  if (!raw.startsWith('#')) return null;
  const hex = raw.slice(1);

  if (hex.length === 3 || hex.length === 4) {
    return {
      r: hexByte(hex[0] + hex[0]),
      g: hexByte(hex[1] + hex[1]),
      b: hexByte(hex[2] + hex[2]),
      a: hex.length === 4 ? hexByte(hex[3] + hex[3]) : 255,
    };
  }

  if (hex.length === 6 || hex.length === 8) {
    return {
      r: hexByte(hex.slice(0, 2)),
      g: hexByte(hex.slice(2, 4)),
      b: hexByte(hex.slice(4, 6)),
      a: hex.length === 8 ? hexByte(hex.slice(6, 8)) : 255,
    };
  }

  return null;
}

function parseRgbColor(value) {
  const raw = String(value ?? '').trim();
  const match = raw.match(/^rgba?\(([^)]+)\)$/i);
  if (!match) return null;

  const parts = match[1].split(',').map((part) => part.trim());
  if (parts.length !== 3 && parts.length !== 4) return null;

  const alphaRaw = parts.length === 4 ? toFiniteNumber(parts[3], 1) : 1;
  return {
    r: clampByte(parts[0]),
    g: clampByte(parts[1]),
    b: clampByte(parts[2]),
    a: clampByte(alphaRaw <= 1 ? alphaRaw * 255 : alphaRaw, 255),
  };
}

export function parseColorRgba(value) {
  return parseHexColor(value) ?? parseRgbColor(value);
}

export function normalizeColorRgba(value = null, fallback = DEFAULT_RGBA) {
  const attrs = normalizeAttrs(value);
  const parsed =
    typeof value === 'string' ? parseColorRgba(value) : typeof attrs.value === 'string' ? parseColorRgba(attrs.value) : null;
  const source = parsed ?? attrs ?? {};
  const fb = fallback ?? DEFAULT_RGBA;

  return {
    r: clampByte(source.r ?? source.red ?? fb.r, fb.r),
    g: clampByte(source.g ?? source.green ?? fb.g, fb.g),
    b: clampByte(source.b ?? source.blue ?? fb.b, fb.b),
    a: clampByte(source.a ?? source.alpha ?? fb.a, fb.a),
  };
}

export function colorRgbaToHex(rgba, includeAlpha = true) {
  const c = normalizeColorRgba(rgba);
  const hex = [c.r, c.g, c.b, ...(includeAlpha ? [c.a] : [])]
    .map((part) => clampByte(part).toString(16).padStart(2, '0'))
    .join('');
  return `#${hex}`.toUpperCase();
}

export function normalizeColorLayout(attrs = {}) {
  const source = normalizeAttrs(attrs);
  const wAttr = Number(source.width ?? 0);
  const hAttr = Number(source.height ?? 0);
  const hasWidth = Number.isFinite(wAttr) && wAttr > 0;
  const hasHeight = Number.isFinite(hAttr) && hAttr > 0;
  const width = hasWidth ? wAttr : 240;
  const height = hasHeight ? hAttr : 200;

  return {
    width,
    height,
    minWidth: Math.min(240, width),
    minHeight: Math.min(200, height),
    fixedSize: hasWidth || hasHeight,
  };
}

export function makeHexPositions(cx, cy, radius) {
  const out = [];
  const angles = [-90, -30, 30, 90, 150, 210];
  for (const angle of angles) {
    const a = (angle * Math.PI) / 180;
    out.push(cx + Math.cos(a) * radius, cy + Math.sin(a) * radius);
  }
  return out;
}

export function colorPickerGeometry(width, height, pad = 10) {
  const w = Math.max(0, toFiniteNumber(width, 0));
  const h = Math.max(0, toFiniteNumber(height, 0));
  const p = Math.max(0, toFiniteNumber(pad, 10));
  const meshW = Math.max(0, w - p * 2);
  const meshH = Math.max(0, h - p * 2);
  const cx = p + meshW / 2;
  const cy = p + meshH / 2;
  const radius = Math.max(0, Math.min(meshW, meshH) / 2 - 2);
  const positions = makeHexPositions(cx, cy, radius);
  const swatchWidth = 44;
  const swatchHeight = 18;

  return {
    pad: p,
    mesh: { x: p, y: p, width: meshW, height: meshH },
    center: { x: cx, y: cy },
    radius,
    positions,
    swatch: { x: Math.max(p, w - p - swatchWidth), y: p, width: swatchWidth, height: swatchHeight },
  };
}

function pointInTri(px, py, ax, ay, bx, by, cx, cy) {
  const v0x = cx - ax;
  const v0y = cy - ay;
  const v1x = bx - ax;
  const v1y = by - ay;
  const v2x = px - ax;
  const v2y = py - ay;

  const dot00 = v0x * v0x + v0y * v0y;
  const dot01 = v0x * v1x + v0y * v1y;
  const dot02 = v0x * v2x + v0y * v2y;
  const dot11 = v1x * v1x + v1y * v1y;
  const dot12 = v1x * v2x + v1y * v2y;
  const denominator = dot00 * dot11 - dot01 * dot01;
  if (Math.abs(denominator) < 1e-9) return false;

  const invDen = 1 / denominator;
  const u = (dot11 * dot02 - dot01 * dot12) * invDen;
  const v = (dot00 * dot12 - dot01 * dot02) * invDen;
  return u >= 0 && v >= 0 && u + v <= 1;
}

function barycentric(px, py, ax, ay, bx, by, cx, cy) {
  const v0x = bx - ax;
  const v0y = by - ay;
  const v1x = cx - ax;
  const v1y = cy - ay;
  const v2x = px - ax;
  const v2y = py - ay;
  const den = v0x * v1y - v1x * v0y;
  if (Math.abs(den) < 1e-9) return { w0: 1, w1: 0, w2: 0 };

  const w1 = (v2x * v1y - v1x * v2y) / den;
  const w2 = (v0x * v2y - v2x * v0y) / den;
  const w0 = 1 - w1 - w2;
  return { w0, w1, w2 };
}

export function sampleColorPickerAtLocal(opts = {}) {
  const { lx = 0, ly = 0, w = 0, h = 0 } = opts;
  const geometry = colorPickerGeometry(w, h);
  const { positions } = geometry;

  for (let ti = 0; ti < COLOR_PICKER_TRI_INDICES.length; ti += 3) {
    const i0 = COLOR_PICKER_TRI_INDICES[ti + 0];
    const i1 = COLOR_PICKER_TRI_INDICES[ti + 1];
    const i2 = COLOR_PICKER_TRI_INDICES[ti + 2];
    const ax = positions[i0 * 2 + 0];
    const ay = positions[i0 * 2 + 1];
    const bx = positions[i1 * 2 + 0];
    const by = positions[i1 * 2 + 1];
    const cx = positions[i2 * 2 + 0];
    const cy = positions[i2 * 2 + 1];

    if (!pointInTri(lx, ly, ax, ay, bx, by, cx, cy)) continue;

    const bc = barycentric(lx, ly, ax, ay, bx, by, cx, cy);
    const c0o = i0 * 4;
    const c1o = i1 * 4;
    const c2o = i2 * 4;
    const r =
      bc.w0 * COLOR_PICKER_VERTEX_COLORS[c0o + 0] +
      bc.w1 * COLOR_PICKER_VERTEX_COLORS[c1o + 0] +
      bc.w2 * COLOR_PICKER_VERTEX_COLORS[c2o + 0];
    const g =
      bc.w0 * COLOR_PICKER_VERTEX_COLORS[c0o + 1] +
      bc.w1 * COLOR_PICKER_VERTEX_COLORS[c1o + 1] +
      bc.w2 * COLOR_PICKER_VERTEX_COLORS[c2o + 1];
    const b =
      bc.w0 * COLOR_PICKER_VERTEX_COLORS[c0o + 2] +
      bc.w1 * COLOR_PICKER_VERTEX_COLORS[c1o + 2] +
      bc.w2 * COLOR_PICKER_VERTEX_COLORS[c2o + 2];

    return { r: clampByte(r), g: clampByte(g), b: clampByte(b) };
  }

  return null;
}
