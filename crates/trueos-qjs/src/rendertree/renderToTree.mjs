// QJS-facing render tree artifact builder.
// This is the render-tree bridge: widlib widgets in, expanded render-tree out.

import { parse as parseHtml } from 'parse5';
import { domToWidgets } from '../widlib/index.mjs';
import { defaultLayoutMetrics, defaultTheme } from './renderTheme.mjs';
import {
  RENDER_TRACE_VERSION,
  blockNode,
  normalizeViewport,
  stableObject,
  textNode,
} from './renderTypes.mjs';

const DEFAULT_MAX_IFRAME_DEPTH = 4;
const TEMPORAL_INPUT_TAGS = Object.freeze({
  time: 'timeinput',
  date: 'dateinput',
  month: 'monthinput',
  week: 'weekinput',
  'datetime-local': 'datetimelocalinput',
});
function cleanChildren(children, options) {
  const out = [];
  for (const child of children ?? []) {
    const next = widgetNodeToRenderNode(child, options);
    if (Array.isArray(next)) out.push(...next);
    else if (next) out.push(next);
  }
  return out;
}

function widgetKeyPath(key) {
  const value = String(key ?? '');
  const colon = value.lastIndexOf(':');
  return colon >= 0 ? value.slice(0, colon) : value;
}

function textRowsFromWidgetTree(widgetTree) {
  const rows = [];
  const seen = new Set();
  const push = (text) => {
    const row = String(text ?? '').replace(/\s+/g, ' ').trim();
    if (!row || seen.has(row)) return;
    seen.add(row);
    rows.push(row);
  };
  const walk = (node) => {
    if (!node || typeof node !== 'object') return;
    if (node.kind === 'text') push(node.text);
    for (const child of node.children ?? []) walk(child);
  };
  walk(widgetTree);
  return rows;
}

function temporalTagName(node, attrs) {
  if (String(node.tag ?? '').toLowerCase() !== 'input') return '';
  const type = String(attrs.type ?? '').toLowerCase();
  return TEMPORAL_INPUT_TAGS[type] ?? '';
}

function attrsWithWidgetProps(node, tagName) {
  const attrs = stableObject(node.attrs);
  const props = node.props && typeof node.props === 'object' ? node.props : {};

  if (tagName === 'summary' && props.detailsKey && attrs['data-details-key'] == null) {
    attrs['data-details-key'] = String(props.detailsKey);
  }
  if (tagName === 'summary' && props.detailsKey && props.open && attrs['data-details-open'] == null) {
    attrs['data-details-open'] = '1';
  }
  if (tagName === 'textarea' && props.value != null && attrs.value == null) {
    attrs.value = String(props.value);
  }
  if (tagName === 'svg' && props.markup && attrs['data-svg'] == null) {
    attrs['data-svg'] = String(props.markup);
  }
  if ((tagName === 'svg' || tagName === 'canvas' || tagName === 'iframe' || tagName === 'img') && props.dimensions) {
    if (props.dimensions.width != null && attrs.width == null) attrs.width = String(props.dimensions.width);
    if (props.dimensions.height != null && attrs.height == null) attrs.height = String(props.dimensions.height);
  }
  if (tagName === 'iframe' && props.srcdocText && attrs['data-trueos-srcdoc-text'] == null) {
    attrs['data-trueos-srcdoc-text'] = String(props.srcdocText);
  }
  if (tagName === 'select') {
    if (props.options && attrs['data-options'] == null) {
      attrs['data-options'] = props.options.map((option) => String(option.label ?? option.value ?? '')).join('\n');
    }
    if (props.selectedIndex != null && attrs['data-selected-index'] == null) {
      attrs['data-selected-index'] = String(props.selectedIndex);
    }
  }

  return stableObject(attrs);
}

function searchRenderNode(node) {
  const attrs = stableObject(node.attrs);
  const key = String(node.key ?? 'search');
  const inputKey = `${key}:search-input`;
  const inputAttrs = {
    type: 'text',
    value: String(attrs.value ?? ''),
  };
  if (attrs.width != null) inputAttrs.width = String(attrs.width);
  if (attrs.placeholder) inputAttrs.placeholder = attrs.placeholder;
  if (attrs.disabled) inputAttrs.disabled = attrs.disabled;

  return blockNode({
    key: `${key}:search-row`,
    tagName: 'searchrow',
    attrs: {},
    children: [
      blockNode({
        key: `${key}:search-btn`,
        tagName: 'searchbutton',
        attrs: { 'data-focus-key': inputKey },
        children: [],
      }),
      blockNode({
        key: inputKey,
        tagName: 'input',
        attrs: stableObject(inputAttrs),
        children: [],
      }),
    ],
  });
}

function barRowRenderNode(node, tagName) {
  const attrs = attrsWithWidgetProps(node, tagName);
  const key = String(node.key ?? `${tagName}`);
  const fallbackText =
    node.props && node.props.fallbackText != null
      ? String(node.props.fallbackText).replace(/\s+/g, ' ').trim()
      : '';
  const rowChildren = [];
  if (fallbackText.length > 0) rowChildren.push(textNode(fallbackText));
  rowChildren.push(blockNode({
    key,
    tagName,
    attrs,
    children: [],
  }));
  return blockNode({
    key: `${key}-row`,
    tagName: 'barrow',
    attrs: { 'data-kind': tagName },
    children: rowChildren,
  });
}

function sliderRenderNode(node) {
  const attrs = attrsWithWidgetProps(node, 'slider');
  const key = String(node.key ?? 'slider');
  return blockNode({
    key: `${key}-row`,
    tagName: 'barrow',
    attrs: { 'data-kind': 'slider' },
    children: [
      blockNode({
        key: `${key}-label`,
        tagName: 'sliderlabel',
        attrs: {
          'data-slider-init': String(attrs.value ?? ''),
          'data-slider-key': key,
        },
        children: [],
      }),
      blockNode({
        key,
        tagName: 'slider',
        attrs,
        children: [],
      }),
    ],
  });
}

function colorChannelValue(node, channel) {
  const props = node.props && typeof node.props === 'object' ? node.props : {};
  const value = props[channel];
  if (value != null && Number.isFinite(Number(value))) return String(Math.max(0, Math.min(255, Math.round(Number(value)))));
  if (channel === 'r' || channel === 'a') return '255';
  return '0';
}

function colorRenderNodes(node) {
  const attrs = attrsWithWidgetProps(node, 'color');
  const key = String(node.key ?? 'color');
  const mkSpin = (channel) =>
    blockNode({
      key: `${key}-${channel}`,
      tagName: 'number',
      attrs: {
        channel,
        max: '255',
        min: '0',
        step: '1',
        value: colorChannelValue(node, channel),
      },
      children: [],
    });
  return [
    blockNode({
      key,
      tagName: 'color',
      attrs,
      children: [],
    }),
    blockNode({
      key: `${key}-controls`,
      tagName: 'p',
      attrs: {},
      children: [mkSpin('r'), mkSpin('g'), mkSpin('b'), mkSpin('a')],
    }),
  ];
}

function detailsRenderNode(node, options) {
  const attrs = attrsWithWidgetProps(node, 'details');
  if (node.props && node.props.open && attrs.open == null) attrs.open = '';
  const key = String(node.key ?? 'details');
  const rawChildren = cleanChildren(node.children, options);
  const hasSummary = rawChildren.some((child) => child && child.kind === 'block' && child.tagName === 'summary');
  const children = hasSummary
    ? rawChildren
    : [
        blockNode({
          key: `${widgetKeyPath(key)}:summary`,
          tagName: 'summary',
          attrs: { 'data-details-key': key },
          children: [textNode(String(attrs.summary ?? attrs.title ?? 'Details'))],
        }),
        ...rawChildren,
      ];

  if (attrs.open != null) {
    for (const child of children) {
      if (child && child.kind === 'block' && child.tagName === 'summary') {
        const childAttrs = child.attrs && typeof child.attrs === 'object' ? { ...child.attrs } : {};
        childAttrs['data-details-key'] = String(childAttrs['data-details-key'] ?? key);
        childAttrs['data-details-open'] = '1';
        child.attrs = stableObject(childAttrs);
      }
    }
  }

  return blockNode({
    key,
    tagName: 'details',
    attrs,
    children,
  });
}

function iframeRenderNode(node, options) {
  const attrs = attrsWithWidgetProps(node, 'iframe');
  const props = node.props && typeof node.props === 'object' ? node.props : {};
  const srcdoc = String(props.srcdoc ?? attrs.srcdoc ?? '');
  const key = String(node.key ?? 'iframe');
  const depth = Math.max(0, Number(options.iframeDepth ?? 0) || 0);
  const maxDepth = Math.max(0, Number(options.maxIframeDepth ?? DEFAULT_MAX_IFRAME_DEPTH) || 0);
  const children = [];

  if (srcdoc.trim().length > 0 && depth < maxDepth) {
    try {
      const doc = parseHtml(srcdoc);
      const nestedTree = domToWidgets(doc, { rootKey: `${widgetKeyPath(key)}:iframe-doc` });
      const rows = textRowsFromWidgetTree(nestedTree);
      if (rows.length > 0 && attrs['data-trueos-srcdoc-text'] == null) {
        attrs['data-trueos-srcdoc-text'] = rows.join('\n');
      }
      children.push(...cleanChildren(nestedTree.children, { ...options, iframeDepth: depth + 1 }));
    } catch (_) {
      children.push(textNode('(iframe srcdoc parse error)'));
    }
  }

  return blockNode({
    key,
    tagName: 'iframe',
    attrs: stableObject(attrs),
    children,
  });
}

export function widgetNodeToRenderNode(node, options = {}) {
  if (!node || typeof node !== 'object') return null;
  if (node.kind === 'text') {
    return textNode(node.text);
  }
  if (node.kind !== 'widget') return null;

  const sourceTag = String(node.tag ?? node.widget ?? 'div').toLowerCase();
  if (sourceTag === 'search' && options.expandSearch !== false) return searchRenderNode(node);
  if ((sourceTag === 'progress' || sourceTag === 'meter') && options.expandBars !== false) {
    return barRowRenderNode(node, sourceTag);
  }
  if (sourceTag === 'slider' && options.expandSlider !== false) return sliderRenderNode(node);
  if (sourceTag === 'color' && options.expandColor !== false) return colorRenderNodes(node);
  if (sourceTag === 'details' && options.expandDetails !== false) return detailsRenderNode(node, options);
  if (sourceTag === 'iframe' && options.expandIframe !== false) return iframeRenderNode(node, options);

  const baseAttrs = stableObject(node.attrs);
  const tagName = temporalTagName(node, baseAttrs) || sourceTag;
  const renderNode = blockNode({
    key: String(node.key ?? ''),
    tagName,
    children: cleanChildren(node.children, options),
  });
  const attrs = attrsWithWidgetProps(node, tagName);
  if (tagName === 'details' && node.props && node.props.open && attrs.open == null) attrs.open = '';
  if (Object.keys(attrs).length > 0) renderNode.attrs = stableObject(attrs);
  if (tagName === 'details' && attrs.open != null) {
    for (const child of renderNode.children) {
      if (child && child.kind === 'block' && child.tagName === 'summary') {
        const childAttrs = child.attrs && typeof child.attrs === 'object' ? { ...child.attrs } : {};
        childAttrs['data-details-open'] = '1';
        child.attrs = stableObject(childAttrs);
      }
    }
  }
  return renderNode;
}

export function widgetTreeToRenderNodes(widgetTree, options = {}) {
  const children = cleanChildren(widgetTree && widgetTree.children, options);
  if (options.wrapRoot === false) return children;
  return [
    blockNode({
      key: String(options.rootKey ?? 'root:internal-iframe'),
      tagName: String(options.rootTagName ?? 'iframe'),
      attrs: { 'data-root': '1' },
      children,
    }),
  ];
}

function numberFrom(value, fallback) {
  const n = Number(value);
  return Number.isFinite(n) ? n : fallback;
}

function clampSize(value, fallback) {
  return Math.max(0, Math.round(numberFrom(value, fallback)));
}

function textWidth(text, maxWidth) {
  const width = String(text ?? '').length * Math.max(1, defaultTheme.fontSize * 0.5) + 4;
  return Math.max(1, Math.min(Math.max(1, maxWidth), width));
}

function layoutDefaultsFor(node) {
  const meta = node && node.meta && typeof node.meta === 'object' ? node.meta : {};
  const defaults = meta.layoutDefaults && typeof meta.layoutDefaults === 'object' ? meta.layoutDefaults : {};
  return defaults;
}

function tagLayoutDefaults(tagName) {
  return defaultLayoutMetrics.tagDefaults[tagName] ?? {};
}

function explicitSizeFromAttrs(node, axis) {
  const attrs = node && node.attrs && typeof node.attrs === 'object' ? node.attrs : {};
  return attrs[axis] ?? attrs[axis === 'width' ? 'w' : 'h'];
}

function isRowTag(tagName) {
  return tagName === 'tr' || tagName === 'barrow' || tagName === 'searchrow';
}

function isLeafTag(tagName) {
  return tagName === 'input'
    || tagName === 'button'
    || tagName === 'textarea'
    || tagName === 'select'
    || tagName === 'img'
    || tagName === 'svg'
    || tagName === 'canvas'
    || tagName === 'iframe'
    || tagName === 'hr'
    || tagName === 'progress'
    || tagName === 'meter'
    || tagName === 'slider'
    || tagName === 'sliderlabel'
    || tagName === 'searchbutton'
    || tagName === 'number'
    || tagName === 'color'
    || tagName === 'timeinput'
    || tagName === 'dateinput'
    || tagName === 'monthinput'
    || tagName === 'weekinput'
    || tagName === 'datetimelocalinput';
}

function boxHeightForTag(tagName, defaults, childrenHeight) {
  const explicit = numberFrom(defaults.height, NaN);
  if (Number.isFinite(explicit)) return Math.round(explicit);
  if (tagName === 'hr') return 1;
  if (tagName === 'h1' || tagName === 'h2' || tagName === 'h3') return Math.max(36, childrenHeight);
  if (tagName === 'textarea') return 108;
  if (tagName === 'color') return 200;
  if (tagName === 'img' || tagName === 'svg' || tagName === 'canvas' || tagName === 'iframe') return Math.max(36, childrenHeight);
  if (isLeafTag(tagName)) return Math.max(clampSize(defaults.minHeight, 36), childrenHeight || 0);
  return Math.max(clampSize(defaults.minHeight, 0), childrenHeight);
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

function layoutNode(renderNode, sourceMap, x, y, width, options) {
  if (!renderNode || typeof renderNode !== 'object') return null;
  if (renderNode.kind === 'text') {
    return {
      kind: 'text',
      text: String(renderNode.text ?? ''),
      x,
      y,
      width: textWidth(renderNode.text, width),
      height: Math.ceil(defaultTheme.fontSize * 1.25),
      children: [],
    };
  }

  const tagName = String(renderNode.tagName ?? 'div');
  const sourceNode = sourceMap.get(String(renderNode.key ?? ''));
  const defaults = { ...tagLayoutDefaults(tagName), ...layoutDefaultsFor(sourceNode) };
  const attrs = renderNode.attrs && typeof renderNode.attrs === 'object' ? renderNode.attrs : {};
  const explicitWidth = explicitSizeFromAttrs(renderNode, 'width') ?? attrs.width ?? defaults.width ?? defaults.minWidth;
  const ownWidth = Math.min(width, clampSize(explicitWidth, width));
  const paddingLeft = clampSize(defaults.paddingLeft ?? defaults.paddingX, tagName === 'summary' ? 26 : 0);
  const paddingRight = clampSize(defaults.paddingRight ?? defaults.paddingX, 0);
  const paddingTop = clampSize(defaults.paddingTop ?? defaults.paddingY, isLeafTag(tagName) ? 0 : 0);
  const paddingBottom = clampSize(defaults.paddingBottom ?? defaults.paddingY, 0);
  const innerX = isLeafTag(tagName) ? 0 : paddingLeft;
  const innerY = isLeafTag(tagName) ? 0 : paddingTop;
  const innerWidth = Math.max(1, ownWidth - paddingLeft - paddingRight);
  const children = [];
  const rawChildren = Array.isArray(renderNode.children) ? renderNode.children : [];
  const layoutChildren =
    tagName === 'details' && !(attrs.open != null || attrs['data-details-open'] === '1')
      ? rawChildren.filter((child) => child && child.kind === 'block' && child.tagName === 'summary')
      : rawChildren;

  let cursorX = innerX;
  let cursorY = innerY;
  let maxBottom = 0;
  if (isRowTag(tagName)) {
    const gap = tagName === 'searchrow' ? 6 : 0;
    for (const child of layoutChildren) {
      const remaining = Math.max(1, innerWidth - cursorX);
      const childLayout = layoutNode(child, sourceMap, cursorX, innerY, remaining, options);
      if (!childLayout) continue;
      children.push(childLayout);
      cursorX += childLayout.width + gap;
      maxBottom = Math.max(maxBottom, childLayout.y + childLayout.height);
    }
  } else {
    for (const child of layoutChildren) {
      const childLayout = layoutNode(child, sourceMap, innerX, cursorY, innerWidth, options);
      if (!childLayout) continue;
      children.push(childLayout);
      cursorY += childLayout.height + 8;
      maxBottom = Math.max(maxBottom, childLayout.y + childLayout.height);
    }
  }

  const explicitHeight = explicitSizeFromAttrs(renderNode, 'height') ?? attrs.height;
  const childrenHeight = maxBottom + paddingBottom;
  const computedHeight = boxHeightForTag(tagName, defaults, childrenHeight);
  const ownHeight = clampSize(explicitHeight, computedHeight);
  const out = {
    kind: 'block',
    key: String(renderNode.key ?? ''),
    tagName,
    x,
    y,
    width: ownWidth,
    height: Math.max(ownHeight, computedHeight),
    children,
  };
  if (renderNode.attrs && Object.keys(renderNode.attrs).length > 0) out.attrs = renderNode.attrs;
  return out;
}

export function renderNodesToLayout(renderNodes, options = {}) {
  const viewport = normalizeViewport(options.viewport);
  const sourceMap = options.sourceMap instanceof Map ? options.sourceMap : new Map();
  const children = [];
  let cursorY = 16;
  const contentWidth = Math.max(1, viewport.width - 38);
  for (const node of renderNodes ?? []) {
    const child = layoutNode(node, sourceMap, 16, cursorY, contentWidth, options);
    if (!child) continue;
    children.push(child);
    cursorY += child.height + 8;
  }
  return {
    kind: 'block',
    key: '',
    tagName: 'root',
    x: 0,
    y: 0,
    width: viewport.width,
    height: Math.max(viewport.height, cursorY + 16),
    children,
  };
}

export function widgetTreeToLayout(widgetTree, renderNodes, options = {}) {
  return renderNodesToLayout(renderNodes ?? widgetTreeToRenderNodes(widgetTree, options), {
    ...options,
    sourceMap: sourceNodeByKey(widgetTree),
  });
}

export function hashText(text) {
  let hash = 0x811c9dc5;
  const value = String(text ?? '');
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return `0x${hash.toString(16).padStart(8, '0')}`;
}

export function countRenderNodes(nodes) {
  let count = 0;
  const walk = (node) => {
    if (!node || typeof node !== 'object') return;
    count += 1;
    for (const child of node.children ?? []) walk(child);
  };
  for (const node of nodes ?? []) walk(node);
  return count;
}

export function createRenderTreeTrace(widgetTree, options = {}) {
  const source = String(options.source ?? 'parse5');
  const viewport = normalizeViewport(options.viewport);
  const bytes = Math.max(0, Math.trunc(numberFrom(options.bytes, 0)));
  const renderNodes = widgetTreeToRenderNodes(widgetTree, options);
  const renderHash = hashText(JSON.stringify(renderNodes));
  const artifact = {
    renderTree: {
      op: 'render-tree',
      source,
      hash: renderHash,
      bytes,
      renderNodes,
    },
  };

  if (options.includeLayout === true) {
    const layout = widgetTreeToLayout(widgetTree, renderNodes, { ...options, viewport });
    const layoutHash = hashText(JSON.stringify(layout));
    const traceBody = {
      version: RENDER_TRACE_VERSION,
      source,
      viewport,
      renderHash,
      layoutHash,
      renderNodes,
      layout,
    };
    artifact.layoutTrace = {
      op: 'layout-trace',
      trace: {
        ...traceBody,
        hash: hashText(JSON.stringify(traceBody)),
      },
    };
  }

  return artifact;
}

export function renderTreeNdjson(widgetTree, options = {}) {
  const artifact = createRenderTreeTrace(widgetTree, options);
  const lines = [JSON.stringify(artifact.renderTree)];
  if (artifact.layoutTrace) lines.push(JSON.stringify(artifact.layoutTrace));
  return lines.join('\n');
}

export function summarizeRenderTreeTrace(artifact) {
  const renderTree = artifact && artifact.renderTree ? artifact.renderTree : {};
  const layoutTrace = artifact && artifact.layoutTrace ? artifact.layoutTrace : {};
  const trace = layoutTrace.trace && typeof layoutTrace.trace === 'object' ? layoutTrace.trace : {};
  return {
    renderNodes: countRenderNodes(renderTree.renderNodes),
    renderHash: renderTree.hash ?? trace.renderHash ?? '',
    layoutHash: trace.layoutHash ?? '',
    hash: trace.hash ?? '',
  };
}
