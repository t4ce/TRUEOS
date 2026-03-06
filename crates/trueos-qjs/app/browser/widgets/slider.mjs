const RANGE_WIDGET_TAGS = new Set(['slider', 'progress', 'meter']);

function getAttr(node, name) {
  if (!node || !Array.isArray(node.attrs)) return '';
  const n = String(name || '').toLowerCase();
  for (let i = 0; i < node.attrs.length; i++) {
    const a = node.attrs[i];
    if (!a || String(a.name || '').toLowerCase() !== n) continue;
    return String(a.value || '');
  }
  return '';
}

function clamp01(v) {
  const x = Number(v);
  if (!Number.isFinite(x) || x <= 0) return 0;
  if (x >= 1) return 1;
  return x;
}

function readNumAttr(srcNode, name, fallback) {
  const raw = Number(getAttr(srcNode, name));
  return Number.isFinite(raw) ? raw : fallback;
}

function ratioForNode(srcNode) {
  const min = readNumAttr(srcNode, 'min', 0);
  const maxRaw = readNumAttr(srcNode, 'max', 1);
  const max = maxRaw <= min ? (min + 1) : maxRaw;
  const value = readNumAttr(srcNode, 'value', min);
  return clamp01((value - min) / (max - min));
}

function defaultWidthForTag(tag) {
  if (tag === 'slider') return 240;
  if (tag === 'progress') return 180;
  return 160;
}

export function rangeWidgetTagForInputType(typeAttr) {
  const t = String(typeAttr || '').toLowerCase();
  if (t === 'range') return 'slider';
  return '';
}

export function isRangeWidgetTag(tag) {
  return RANGE_WIDGET_TAGS.has(String(tag || '').toLowerCase());
}

// Bonus: use Yoga's measure callback so range-like controls naturally size to defaults/attrs.
export function applyYogaDefaultsRangeWidget(yogaNode, Yoga, tag, srcNode) {
  if (!yogaNode || !Yoga || !isRangeWidgetTag(tag)) return;
  yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
  yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
  yogaNode.setPadding(Yoga.EDGE_TOP, 0);
  yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);

  const rawW = Number(getAttr(srcNode, 'width'));
  const targetW = Number.isFinite(rawW) && rawW > 8
    ? Math.round(rawW)
    : defaultWidthForTag(tag);
  const targetH = 14;

  if (typeof yogaNode.setAlignSelf === 'function') yogaNode.setAlignSelf(Yoga.ALIGN_FLEX_START);
  if (typeof yogaNode.setMeasureFunc === 'function') {
    yogaNode.setMeasureFunc(() => ({ width: targetW, height: targetH }));
  }
  if (typeof yogaNode.setWidth === 'function') yogaNode.setWidth(targetW);
  if (typeof yogaNode.setMinWidth === 'function') yogaNode.setMinWidth(targetW);
  if (typeof yogaNode.setHeight === 'function') yogaNode.setHeight(targetH);
  if (typeof yogaNode.setMinHeight === 'function') yogaNode.setMinHeight(targetH);
}

export function renderRangeWidget(rect, ctx) {
  if (!rect || !isRangeWidgetTag(rect.tag)) return [];
  if (!ctx || ctx.mode !== 'collect') return [];

  const tag = String(rect.tag || '').toLowerCase();
  const srcNode = typeof ctx.getSourceNodeById === 'function'
    ? ctx.getSourceNodeById(String(rect.id || ''))
    : null;
  const ratio = ratioForNode(srcNode);

  const x = Math.round(Number(rect.x || 0));
  const y = Math.round(Number(rect.y || 0));
  const w = Math.max(12, Math.round(Number(rect.w || 0)));
  const h = Math.max(8, Math.round(Number(rect.h || 0)));
  const depth = Math.max(0, Number(rect.depth || 0));

  const pad = 2;
  const innerW = Math.max(1, w - pad * 2);
  const innerH = Math.max(1, h - pad * 2);
  const fillW = Math.max(1, Math.round(innerW * ratio));

  const out = [
    // Base control tint.
    x + 1, y + 1, Math.max(1, w - 2), Math.max(1, h - 2), depth + 1, 0, 5,
    // Value fill lane.
    x + pad, y + pad, fillW, innerH, depth + 2, 0, 2,
  ];

  if (tag === 'slider') {
    const knobW = 3;
    const knobX = x + pad + Math.max(0, Math.min(innerW - knobW, fillW - Math.floor(knobW / 2)));
    out.push(knobX, y + 1, knobW, Math.max(1, h - 2), depth + 3, 0, 1);
  }

  if (tag === 'meter') {
    const markX = x + pad + Math.round(innerW * 0.7);
    out.push(markX, y + 1, 1, Math.max(1, h - 2), depth + 3, 0, 1);
  }

  return out;
}
