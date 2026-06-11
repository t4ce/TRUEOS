const DEFAULT_SVG_WIDTH = 300;
const DEFAULT_SVG_HEIGHT = 150;
const DEFAULT_SVG_MIN_WIDTH = 120;
const DEFAULT_SVG_MIN_HEIGHT = 80;

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

export function positiveNumberAttr(value) {
  const number = Number(value);
  return Number.isFinite(number) && number > 0 ? number : undefined;
}

export function parseSvgDimensions(attrs = {}) {
  const source = normalizeAttrs(attrs);
  const attrWidth = positiveNumberAttr(source.width);
  const attrHeight = positiveNumberAttr(source.height);
  const width = attrWidth ?? DEFAULT_SVG_WIDTH;
  const height = attrHeight ?? DEFAULT_SVG_HEIGHT;

  return {
    width,
    height,
    minWidth: Math.min(DEFAULT_SVG_MIN_WIDTH, width),
    minHeight: Math.min(DEFAULT_SVG_MIN_HEIGHT, height),
    hasWidth: attrWidth !== undefined,
    hasHeight: attrHeight !== undefined,
    fixedSize: attrWidth !== undefined || attrHeight !== undefined,
  };
}

export function stripTagBlock(source = '', tag = '') {
  let out = '';
  let i = 0;
  const text = String(source ?? '');
  const lower = text.toLowerCase();
  const normalizedTag = String(tag ?? '').toLowerCase();
  const openNeedle = '<' + normalizedTag;
  const closeNeedle = '</' + normalizedTag;

  while (i < text.length) {
    const open = lower.indexOf(openNeedle, i);
    if (open < 0) {
      out += text.slice(i);
      break;
    }
    out += text.slice(i, open);
    const close = lower.indexOf(closeNeedle, open + openNeedle.length);
    if (close < 0) break;
    const end = text.indexOf('>', close + closeNeedle.length);
    i = end < 0 ? text.length : end + 1;
  }

  return out;
}

export function stripUnsupportedSvgText(svg = '') {
  return stripTagBlock(stripTagBlock(String(svg ?? ''), 'tspan'), 'text');
}

export function splitSvgNumberList(source = '') {
  return String(source ?? '')
    .split(/[\s,]+/)
    .map((part) => part.trim())
    .filter((part) => part.length > 0);
}

export function parseViewBoxFromString(svg = '') {
  const source = String(svg ?? '');
  const lower = source.toLowerCase();
  const at = lower.indexOf('viewbox');
  if (at < 0) return null;
  const eq = source.indexOf('=', at + 7);
  if (eq < 0) return null;

  let i = eq + 1;
  while (i < source.length && /\s/.test(source.charAt(i))) i += 1;

  const quote = source.charAt(i);
  if (quote !== '"' && quote !== "'") return null;
  const end = source.indexOf(quote, i + 1);
  if (end < 0) return null;

  return parseViewBoxValue(source.slice(i + 1, end));
}

export function parseViewBoxValue(value = '') {
  const parts = splitSvgNumberList(value);
  if (parts.length < 4) return null;

  const minX = Number(parts[0]);
  const minY = Number(parts[1]);
  const width = Number(parts[2]);
  const height = Number(parts[3]);
  if (![minX, minY, width, height].every((number) => Number.isFinite(number)) || width <= 0 || height <= 0) {
    return null;
  }

  return { minX, minY, width, height };
}

export function fittedViewBoxRect(viewBoxOrSvg = null, width = 0, height = 0) {
  const boxWidth = Math.max(0, Number(width) || 0);
  const boxHeight = Math.max(0, Number(height) || 0);
  const viewBox =
    typeof viewBoxOrSvg === 'string' ? parseViewBoxFromString(viewBoxOrSvg) : viewBoxOrSvg && typeof viewBoxOrSvg === 'object' ? viewBoxOrSvg : null;

  if (!viewBox || boxWidth <= 0 || boxHeight <= 0) return { x: 0, y: 0, width: boxWidth, height: boxHeight };

  const sourceWidth = Number(viewBox.width ?? viewBox.w);
  const sourceHeight = Number(viewBox.height ?? viewBox.h);
  if (!(sourceWidth > 0) || !(sourceHeight > 0)) return { x: 0, y: 0, width: boxWidth, height: boxHeight };

  const scale = Math.min(boxWidth / sourceWidth, boxHeight / sourceHeight);
  const drawWidth = Math.max(0, sourceWidth * scale);
  const drawHeight = Math.max(0, sourceHeight * scale);

  return {
    x: Math.max(0, (boxWidth - drawWidth) / 2),
    y: Math.max(0, (boxHeight - drawHeight) / 2),
    width: drawWidth,
    height: drawHeight,
    scale,
  };
}

export function svgDataUrl(svg = '') {
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(String(svg ?? ''))}`;
}

export function normalizeSvgProps(attrs = {}, svgMarkup = '') {
  const source = normalizeAttrs(attrs);
  const markup = stripUnsupportedSvgText(svgMarkup);
  const viewBox = source.viewBox ? parseViewBoxValue(source.viewBox) : parseViewBoxFromString(markup);

  return {
    markup,
    viewBox,
    preserveAspectRatio: String(source.preserveAspectRatio ?? 'xMidYMid meet'),
    ...parseSvgDimensions(source),
  };
}

export const WIDGET_DEFINITION = widgetDefinition('svg', {
  category: 'replaced',
  leaf: true,
  complexity: 'complex',
  layoutDefaults: {
    width: DEFAULT_SVG_WIDTH,
    height: DEFAULT_SVG_HEIGHT,
    minWidth: DEFAULT_SVG_MIN_WIDTH,
    minHeight: DEFAULT_SVG_MIN_HEIGHT,
  },
  attrs: ['width', 'height', 'viewBox'],
});

export const SVG_WIDGET_DEFINITION = WIDGET_DEFINITION;
