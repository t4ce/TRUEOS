import { defaultLayoutMetrics, defaultTheme } from './renderTheme.mjs';
import { normalizeViewport } from './renderTypes.mjs';

const ROOT_PAD = 16;
const SCROLLBAR_PAD = 6;
const BLOCK_GAP = 8;
const INLINE_GAP = 6;

const CONTROL_TAGS = new Set([
  'input',
  'button',
  'select',
  'textarea',
  'timeinput',
  'dateinput',
  'monthinput',
  'weekinput',
  'datetimelocalinput',
  'progress',
  'meter',
  'slider',
  'number',
  'color',
]);

const LEAF_TAGS = new Set([
  ...CONTROL_TAGS,
  'img',
  'svg',
  'canvas',
  'iframe',
  'hr',
  'sliderlabel',
  'searchbutton',
]);

const ROW_TAGS = new Set(['tr', 'barrow', 'searchrow']);

function numberFrom(value, fallback) {
  const n = Number(value);
  return Number.isFinite(n) ? n : fallback;
}

function sizeFrom(value, fallback) {
  return Math.max(0, Math.round(numberFrom(value, fallback)));
}

function normalizeWhitespace(text) {
  return String(text ?? '').replace(/\s+/g, ' ').trim();
}

export function createTextMeasurer(options = {}) {
  const fontSize = Math.max(1, numberFrom(options.fontSize, defaultTheme.fontSize));
  const lineHeight = Math.ceil(fontSize * 1.25);
  const charWidth = fontSize * 0.58;
  let ctx = null;

  try {
    const canvas = globalThis.document?.createElement?.('canvas');
    ctx = canvas?.getContext?.('2d') ?? null;
    if (ctx) ctx.font = `${fontSize}px ${defaultTheme.fontFamily}`;
  } catch (_) {
    ctx = null;
  }

  const widthOf = (text) => {
    const value = String(text ?? '');
    if (ctx) return ctx.measureText(value).width;
    return value.length * charWidth;
  };

  return {
    lineHeight,
    measure(text, maxWidth = Number.POSITIVE_INFINITY) {
      const limit = Math.max(1, numberFrom(maxWidth, Number.POSITIVE_INFINITY));
      const words = normalizeWhitespace(text).split(' ').filter(Boolean);
      if (words.length === 0) return { width: 0, height: lineHeight, lines: [''] };

      const lines = [];
      let current = '';
      for (const word of words) {
        const next = current ? `${current} ${word}` : word;
        if (widthOf(next) <= limit || !current) {
          current = next;
        } else {
          lines.push(current);
          current = word;
        }
      }
      if (current) lines.push(current);

      const width = Math.min(
        limit,
        Math.max(1, ...lines.map((line) => Math.ceil(widthOf(line)))),
      );
      return {
        width,
        height: Math.max(lineHeight, lines.length * lineHeight),
        lines,
      };
    },
  };
}

function tagDefaults(tagName) {
  return defaultLayoutMetrics.tagDefaults[tagName] ?? {};
}

function sourceNodeByKey(widgetTree) {
  const map = new Map();
  const walk = (node) => {
    if (!node || typeof node !== 'object') return;
    if (node.key != null) map.set(String(node.key), node);
    for (const child of node.children ?? []) walk(child);
  };
  walk(widgetTree);
  return map;
}

function layoutDefaultsFor(sourceNode) {
  const meta = sourceNode && sourceNode.meta && typeof sourceNode.meta === 'object' ? sourceNode.meta : {};
  const defaults = meta.layoutDefaults && typeof meta.layoutDefaults === 'object' ? meta.layoutDefaults : {};
  const layout = sourceNode && sourceNode.layout && typeof sourceNode.layout === 'object' ? sourceNode.layout : {};
  return { ...defaults, ...layout };
}

function attrsOf(node) {
  return node && node.attrs && typeof node.attrs === 'object' ? node.attrs : {};
}

function inputTypeOf(node) {
  return String(attrsOf(node).type ?? '').toLowerCase();
}

function isCheckableInput(node) {
  return String(node?.tagName ?? '').toLowerCase() === 'input'
    && (inputTypeOf(node) === 'checkbox' || inputTypeOf(node) === 'radio');
}

function attrSize(node, axis) {
  const attrs = attrsOf(node);
  return attrs[axis] ?? attrs[axis === 'width' ? 'w' : 'h'];
}

function isOpenDetails(node) {
  const attrs = attrsOf(node);
  return attrs.open != null || attrs['data-details-open'] === '1';
}

function isHeading(tagName) {
  return tagName === 'h1' || tagName === 'h2' || tagName === 'h3' || tagName === 'h4' || tagName === 'h5' || tagName === 'h6';
}

function hasInlineChild(node) {
  return (node.children ?? []).some((child) => {
    if (!child || child.kind !== 'block') return false;
    const tagName = String(child.tagName ?? '').toLowerCase();
    return CONTROL_TAGS.has(tagName)
      || tagName === 'img'
      || tagName === 'svg'
      || tagName === 'canvas'
      || tagName === 'iframe';
  });
}

function isRowNode(node, tagName) {
  return ROW_TAGS.has(tagName)
    || tagName === 'summary'
    || ((tagName === 'p' || tagName === 'label') && hasInlineChild(node));
}

function gapAfter(child) {
  if (!child || child.kind !== 'block') return 0;
  const tagName = String(child.tagName ?? '');
  if (tagName === 'hr' || tagName === 'tr' || tagName === 'td' || tagName === 'th') return 0;
  return BLOCK_GAP;
}

function nodePadding(tagName, defaults) {
  if (LEAF_TAGS.has(tagName)) {
    return {
      left: sizeFrom(defaults.paddingLeft ?? defaults.paddingX, 0),
      top: sizeFrom(defaults.paddingTop ?? defaults.paddingY, 0),
      right: sizeFrom(defaults.paddingRight ?? defaults.paddingX, 0),
      bottom: sizeFrom(defaults.paddingBottom ?? defaults.paddingY, 0),
    };
  }
  if (tagName === 'p' || tagName === 'label') return { left: 4, top: 4, right: 4, bottom: 4 };
  if (tagName === 'summary') return { left: 26, top: 6, right: 8, bottom: 6 };
  return {
    left: sizeFrom(defaults.paddingLeft ?? defaults.paddingX, 12),
    top: sizeFrom(defaults.paddingTop ?? defaults.paddingY, 12),
    right: sizeFrom(defaults.paddingRight ?? defaults.paddingX, 12),
    bottom: sizeFrom(defaults.paddingBottom ?? defaults.paddingY, 12),
  };
}

function widthForNode(node, tagName, defaults, availableWidth) {
  if (isCheckableInput(node)) return Math.min(18, Math.max(1, availableWidth));
  const attrWidth = attrSize(node, 'width');
  const explicit = attrWidth ?? defaults.width;
  const minWidth = sizeFrom(defaults.minWidth, 0);
  if (explicit != null && explicit !== '') {
    return Math.min(Math.max(minWidth, sizeFrom(explicit, availableWidth)), Math.max(1, availableWidth));
  }
  if (LEAF_TAGS.has(tagName) && minWidth > 0) return Math.min(minWidth, Math.max(1, availableWidth));
  return Math.max(1, availableWidth);
}

function heightForNode(node, tagName, defaults, contentHeight, padding) {
  if (isCheckableInput(node)) return 18;
  const attrHeight = attrSize(node, 'height');
  const explicit = attrHeight ?? defaults.height;
  const minHeight = sizeFrom(defaults.minHeight, 0);
  if (explicit != null && explicit !== '') return Math.max(minHeight, sizeFrom(explicit, contentHeight));
  if (tagName === 'hr') return Math.max(1, minHeight || 1);
  if (isHeading(tagName)) return Math.max(36, minHeight, contentHeight);
  if (tagName === 'textarea') return Math.max(108, minHeight, contentHeight);
  if (LEAF_TAGS.has(tagName)) return Math.max(minHeight, contentHeight, 36);
  return Math.max(minHeight, contentHeight + padding.bottom);
}

function childRenderList(node) {
  const children = Array.isArray(node.children) ? node.children : [];
  if (String(node.tagName ?? '') !== 'details' || isOpenDetails(node)) return children;
  return children.filter((child) => child && child.kind === 'block' && String(child.tagName ?? '') === 'summary');
}

function layoutTextNode(renderNode, x, y, width, measurer) {
  const text = String(renderNode.text ?? '');
  const measured = measurer.measure(text, width);
  return {
    kind: 'text',
    text,
    x,
    y,
    width: measured.width,
    height: measured.height,
    children: [],
  };
}

function inlineWidthForChild(child, parentTagName, sourceMap, remainingWidth, remainingChildren, measurer) {
  const remaining = Math.max(1, remainingWidth);
  const divisor = Math.max(1, remainingChildren);
  if (!child || typeof child !== 'object') return Math.max(1, Math.floor(remaining / divisor));

  if (child.kind === 'text') {
    return Math.min(remaining, Math.max(1, measurer.measure(child.text, remaining).width));
  }

  if (child.kind !== 'block') return Math.max(1, Math.floor(remaining / divisor));
  if (parentTagName === 'tr') return Math.max(1, Math.floor(remaining / divisor));
  if (isCheckableInput(child)) return Math.min(18, remaining);

  const tagName = String(child.tagName ?? 'div').toLowerCase();
  const sourceNode = sourceMap.get(String(child.key ?? ''));
  const defaults = { ...tagDefaults(tagName), ...layoutDefaultsFor(sourceNode) };
  const explicit = attrSize(child, 'width') ?? defaults.width ?? defaults.minWidth;
  if (explicit != null && explicit !== '') {
    return Math.min(remaining, Math.max(1, sizeFrom(explicit, remaining)));
  }

  if (LEAF_TAGS.has(tagName)) {
    const minWidth = sizeFrom(defaults.minWidth, 0);
    if (minWidth > 0) return Math.min(remaining, minWidth);
  }

  return Math.max(1, Math.floor(remaining / divisor));
}

function rowGapForTag(tagName) {
  return tagName === 'tr' ? 0 : INLINE_GAP;
}

function layoutBlockNode(renderNode, sourceMap, x, y, availableWidth, options, measurer) {
  const tagName = String(renderNode.tagName ?? 'div').toLowerCase();
  const sourceNode = sourceMap.get(String(renderNode.key ?? ''));
  const defaults = { ...tagDefaults(tagName), ...layoutDefaultsFor(sourceNode) };
  const width = widthForNode(renderNode, tagName, defaults, availableWidth);
  const padding = nodePadding(tagName, defaults);
  const innerX = LEAF_TAGS.has(tagName) ? 0 : padding.left;
  const innerY = LEAF_TAGS.has(tagName) ? 0 : padding.top;
  const innerWidth = Math.max(1, width - padding.left - padding.right);
  const children = [];
  const renderChildren = childRenderList(renderNode);

  let contentBottom = innerY;
  if (isRowNode(renderNode, tagName)) {
    let cursorX = innerX;
    let rowBottom = innerY;
    const gap = rowGapForTag(tagName);
    for (let i = 0; i < renderChildren.length; i += 1) {
      const child = renderChildren[i];
      const remaining = Math.max(1, innerWidth - (cursorX - innerX));
      const childWidth = inlineWidthForChild(
        child,
        tagName,
        sourceMap,
        remaining,
        renderChildren.length - i,
        measurer,
      );
      const box = layoutNode(child, sourceMap, cursorX, innerY, childWidth, options, measurer);
      if (!box) continue;
      children.push(box);
      cursorX += box.width + gap;
      rowBottom = Math.max(rowBottom, box.y + box.height);
    }
    contentBottom = rowBottom;
  } else {
    let cursorY = innerY;
    for (const child of renderChildren) {
      const box = layoutNode(child, sourceMap, innerX, cursorY, innerWidth, options, measurer);
      if (!box) continue;
      children.push(box);
      cursorY += box.height + gapAfter(child);
      contentBottom = Math.max(contentBottom, box.y + box.height);
    }
  }

  const height = heightForNode(renderNode, tagName, defaults, contentBottom, padding);
  const out = {
    kind: 'block',
    key: String(renderNode.key ?? ''),
    tagName,
    x,
    y,
    width,
    height,
    children,
  };
  if (renderNode.attrs && Object.keys(renderNode.attrs).length > 0) out.attrs = renderNode.attrs;
  return out;
}

export function layoutNode(renderNode, sourceMap, x, y, width, options = {}, measurer = createTextMeasurer()) {
  if (!renderNode || typeof renderNode !== 'object') return null;
  if (renderNode.kind === 'text') return layoutTextNode(renderNode, x, y, width, measurer);
  if (renderNode.kind !== 'block') return null;
  return layoutBlockNode(renderNode, sourceMap, x, y, Math.max(1, width), options, measurer);
}

export function renderNodesToLayout(renderNodes, options = {}) {
  const viewport = normalizeViewport(options.viewport);
  const sourceMap = options.sourceMap instanceof Map ? options.sourceMap : new Map();
  const measurer = options.measurer ?? createTextMeasurer(options);
  const children = [];
  const contentWidth = Math.max(1, viewport.width - ROOT_PAD * 2 - SCROLLBAR_PAD);
  let cursorY = ROOT_PAD;

  for (const node of renderNodes ?? []) {
    const box = layoutNode(node, sourceMap, ROOT_PAD, cursorY, contentWidth, options, measurer);
    if (!box) continue;
    children.push(box);
    cursorY += box.height + gapAfter(node);
  }

  return {
    kind: 'block',
    key: '',
    tagName: 'root',
    x: 0,
    y: 0,
    width: viewport.width,
    height: Math.max(viewport.height, cursorY + ROOT_PAD),
    children,
  };
}

export function widgetTreeToLayout(widgetTree, renderNodes, options = {}) {
  return renderNodesToLayout(renderNodes ?? [], {
    ...options,
    sourceMap: sourceNodeByKey(widgetTree),
  });
}
