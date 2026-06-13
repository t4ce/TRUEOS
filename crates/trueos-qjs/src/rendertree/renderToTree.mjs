// QJS-facing render tree artifact builder.
// This is the render-tree bridge: widlib widgets in, expanded render-tree out.

import { parse as parseHtml } from 'parse5';
import { buildCssStyleRefIndex } from '../truesurfer/css.mjs';
import { domToWidgets } from '../widlib/index.mjs';
import { widgetTreeToLayout as buildWidgetTreeLayout } from './layout.mjs';
import {
  RENDER_TRACE_VERSION,
  blockNode,
  normalizeViewport,
  stableObject,
  textNode,
} from './renderTypes.mjs';

export { renderNodesToLayout, widgetTreeToLayout } from './layout.mjs';

const DEFAULT_MAX_IFRAME_DEPTH = 4;
const TEMPORAL_INPUT_TAGS = Object.freeze({
  time: 'timeinput',
  date: 'dateinput',
  month: 'monthinput',
  week: 'weekinput',
  'datetime-local': 'datetimelocalinput',
});

function renderContextFor(options = {}) {
  if (options.renderContext && typeof options.renderContext === 'object') return options.renderContext;
  return { embeddedScenes: [], nextEmbeddedSceneId: 1 };
}

function embeddedSceneIdFor(context, key) {
  const seq = Math.max(1, Number(context.nextEmbeddedSceneId ?? 1) || 1);
  context.nextEmbeddedSceneId = seq + 1;
  return `${widgetKeyPath(key)}:embedded-scene:${seq}`;
}

function publicEmbeddedScene(scene) {
  const out = {
    id: String(scene.id ?? ''),
    parentKey: String(scene.parentKey ?? ''),
    parentTagName: String(scene.parentTagName ?? 'iframe'),
    source: String(scene.source ?? 'iframe-srcdoc'),
    depth: Math.max(0, Number(scene.depth ?? 0) || 0),
    renderHash: String(scene.renderHash ?? ''),
    renderNodes: Array.isArray(scene.renderNodes) ? scene.renderNodes : [],
  };
  if (scene.layoutHash) out.layoutHash = String(scene.layoutHash);
  if (scene.parentBox) out.parentBox = scene.parentBox;
  if (scene.viewport) out.viewport = scene.viewport;
  if (scene.layout) out.layout = scene.layout;
  if (scene.ui3PaintPlan) out.ui3PaintPlan = scene.ui3PaintPlan;
  if (scene.error) out.error = String(scene.error);
  return out;
}

function findLayoutBoxByKey(node, key) {
  if (!node || typeof node !== 'object') return null;
  if (String(node.key ?? '') === String(key ?? '')) return node;
  for (const child of node.children ?? []) {
    const found = findLayoutBoxByKey(child, key);
    if (found) return found;
  }
  return null;
}

function layoutBoxSummary(box) {
  if (!box || typeof box !== 'object') return null;
  return {
    key: String(box.key ?? ''),
    tagName: String(box.tagName ?? ''),
    x: Math.round(Number(box.x ?? 0) || 0),
    y: Math.round(Number(box.y ?? 0) || 0),
    width: Math.max(0, Math.round(Number(box.width ?? 0) || 0)),
    height: Math.max(0, Math.round(Number(box.height ?? 0) || 0)),
  };
}

function buildEmbeddedSceneLayouts(scenes, rootLayout, options = {}) {
  const out = [];
  for (const scene of scenes ?? []) {
    const parentBox = findLayoutBoxByKey(rootLayout, scene.parentKey)
      ?? out.map((prior) => findLayoutBoxByKey(prior.layout, scene.parentKey)).find(Boolean)
      ?? null;
    const parentSummary = layoutBoxSummary(parentBox);
    const viewport = normalizeViewport({
      width: parentSummary && parentSummary.width > 0 ? parentSummary.width : options.viewport?.width,
      height: parentSummary && parentSummary.height > 0 ? parentSummary.height : options.viewport?.height,
    });
    const layout = scene.widgetTree
      ? buildWidgetTreeLayout(scene.widgetTree, scene.renderNodes, {
        ...options,
        viewport,
        rootPad: 0,
        scrollbarPad: 0,
      })
      : null;
    const layoutHash = layout ? hashText(JSON.stringify(layout)) : '';
    out.push(publicEmbeddedScene({
      ...scene,
      parentBox: parentSummary,
      viewport,
      layout,
      ui3PaintPlan: layout ? createUi3PaintPlan(layout) : null,
      layoutHash,
    }));
  }
  return out;
}

function layoutAttrs(node) {
  return node && node.attrs && typeof node.attrs === 'object' ? node.attrs : {};
}

function layoutPaint(node) {
  return node && node.paint && typeof node.paint === 'object' && !Array.isArray(node.paint) ? node.paint : null;
}

function layoutPaintRole(node) {
  return String(layoutPaint(node)?.role ?? '');
}

function layoutTextColor(node, inheritedTextColor) {
  const paint = layoutPaint(node);
  const value = paint && paint.textColor != null ? Number(paint.textColor) : inheritedTextColor;
  return Number.isFinite(value) ? Math.max(0, Math.trunc(value) & 0xffffff) : inheritedTextColor;
}

function isSummaryOpen(node) {
  const attrs = layoutAttrs(node);
  return attrs.open != null || attrs['data-details-open'] === '1';
}

function isHitBoxNode(tagName, role) {
  return tagName === 'button'
    || tagName === 'a'
    || tagName === 'summary'
    || tagName === 'input'
    || tagName === 'select'
    || tagName === 'textarea'
    || tagName === 'searchbutton'
    || role === 'button'
    || role === 'link';
}

function getUrlOrigin(url) {
  const value = String(url ?? '').trim();
  const match = value.match(/^[a-z][a-z0-9+.-]*:\/\/[^/?#]+/i);
  return match ? match[0] : '';
}

function getUrlDirectory(url) {
  const value = String(url ?? '').trim();
  const origin = getUrlOrigin(value);
  if (!origin) return '';
  const rest = value.slice(origin.length);
  const queryIndex = rest.search(/[?#]/);
  const pathOnly = queryIndex >= 0 ? rest.slice(0, queryIndex) : rest;
  const slash = pathOnly.lastIndexOf('/');
  if (slash < 0) return `${origin}/`;
  return `${origin}${pathOnly.slice(0, slash + 1)}`;
}

function resolveHref(baseUrl, href) {
  const value = String(href ?? '').trim();
  if (!value) return '';
  if (value.startsWith('#')) return '';
  if (/^[a-z][a-z0-9+.-]*:/i.test(value)) return value;
  const base = String(baseUrl ?? '').trim();
  const origin = getUrlOrigin(base);
  if (value.startsWith('//')) {
    const schemeMatch = base.match(/^([a-z][a-z0-9+.-]*:)/i);
    return schemeMatch ? `${schemeMatch[1]}${value}` : `https:${value}`;
  }
  if (value.startsWith('/')) return origin ? `${origin}${value}` : value;
  const dir = getUrlDirectory(base);
  return dir ? `${dir}${value}` : value;
}

function hitBoxActivation(node, tagName, role, options = {}) {
  const attrs = layoutAttrs(node);
  if ((tagName === 'a' || role === 'link') && attrs.href != null) {
    const href = String(attrs.href ?? '').trim();
    const resolvedHref = resolveHref(options.baseUrl, href);
    if (!/^(https?:|file:)/i.test(resolvedHref)) return null;
    const activation = {
      kind: 'navigate',
      href,
      resolvedHref,
    };
    if (attrs.target != null) activation.target = String(attrs.target);
    return activation;
  }
  return null;
}

function createUi3PaintPlan(layout, options = {}) {
  const paintedBoxes = [];
  const textRuns = [];
  const summaryIcons = [];
  const hitBoxes = [];

  const walk = (node, parentX = 0, parentY = 0, inheritedTextColor = defaultTextColor()) => {
    if (!node || typeof node !== 'object') return;
    const x = parentX + Number(node.x ?? 0);
    const y = parentY + Number(node.y ?? 0);
    const kind = String(node.kind ?? '');
    const textColor = layoutTextColor(node, inheritedTextColor);

    if (kind === 'text') {
      textRuns.push({
        key: String(node.key ?? ''),
        x,
        y,
        width: Math.max(0, Math.round(Number(node.width ?? 0) || 0)),
        height: Math.max(0, Math.round(Number(node.height ?? 0) || 0)),
        text: String(node.text ?? ''),
        lines: Array.isArray(node.lines) ? node.lines.map((line) => String(line ?? '')) : undefined,
        preserveWhitespace: node.preserveWhitespace === true,
        textColor,
      });
      return;
    }

    const tagName = String(node.tagName ?? '').toLowerCase();
    const role = layoutPaintRole(node);
    if (kind === 'block') {
      if (role) {
        paintedBoxes.push({
          key: String(node.key ?? ''),
          tagName,
          role,
          x,
          y,
          width: Math.max(0, Math.round(Number(node.width ?? 0) || 0)),
          height: Math.max(0, Math.round(Number(node.height ?? 0) || 0)),
          paint: layoutPaint(node),
        });
      }
      if (tagName === 'summary') {
        summaryIcons.push({
          key: String(node.key ?? ''),
          x,
          y,
          height: Math.max(0, Math.round(Number(node.height ?? 0) || 0)),
          open: isSummaryOpen(node),
        });
      }
      if (isHitBoxNode(tagName, role)) {
        const hitBox = {
          key: String(node.key ?? ''),
          tagName,
          role,
          x,
          y,
          width: Math.max(0, Math.round(Number(node.width ?? 0) || 0)),
          height: Math.max(0, Math.round(Number(node.height ?? 0) || 0)),
        };
        const activation = hitBoxActivation(node, tagName, role, options);
        if (activation) hitBox.activation = activation;
        hitBoxes.push(hitBox);
      }
    }

    for (const child of node.children ?? []) walk(child, x, y, textColor);
  };

  walk(layout);
  return {
    version: 1,
    contentHeight: Math.max(0, Math.round(Number(layout?.height ?? 0) || 0)),
    paintedBoxes,
    textRuns,
    summaryIcons,
    hitBoxes,
  };
}

function defaultTextColor() {
  return 0x000000;
}

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

function widgetPaint(node) {
  const paint = node && node.meta && node.meta.paint;
  return paint && typeof paint === 'object' && !Array.isArray(paint) ? { ...paint } : null;
}

function withWidgetPaint(renderNode, widgetNode) {
  const paint = widgetPaint(widgetNode);
  if (paint && renderNode && typeof renderNode === 'object') renderNode.paint = paint;
  return renderNode;
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
  if (tagName === 'img' && props.imageAsset && typeof props.imageAsset === 'object') {
    const asset = props.imageAsset;
    const assetSrc = String(asset.src ?? '');
    if (asset.tag && attrs['data-trueos-img-tag'] == null) attrs['data-trueos-img-tag'] = String(asset.tag);
    if (assetSrc.startsWith('data:')) {
      if (attrs['data-trueos-img-inline'] == null) attrs['data-trueos-img-inline'] = '1';
    } else if (assetSrc && attrs['data-trueos-img-src'] == null) {
      attrs['data-trueos-img-src'] = assetSrc;
    }
    if (asset.kind && attrs['data-trueos-img-kind'] == null) attrs['data-trueos-img-kind'] = String(asset.kind);
    if (asset.state && attrs['data-trueos-img-state'] == null) attrs['data-trueos-img-state'] = String(asset.state);
    if (Number(asset.texId || 0) > 0 && attrs['data-trueos-img-tex-id'] == null) attrs['data-trueos-img-tex-id'] = String(Number(asset.texId || 0) | 0);
    if (Number(asset.pixelWidth || 0) > 0 && attrs['data-trueos-img-width'] == null) attrs['data-trueos-img-width'] = String(Number(asset.pixelWidth || 0) | 0);
    if (Number(asset.pixelHeight || 0) > 0 && attrs['data-trueos-img-height'] == null) attrs['data-trueos-img-height'] = String(Number(asset.pixelHeight || 0) | 0);
    if (asset.mime && attrs['data-trueos-img-mime'] == null) attrs['data-trueos-img-mime'] = String(asset.mime);
    if (asset.error && attrs['data-trueos-img-error'] == null) attrs['data-trueos-img-error'] = String(asset.error);
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

  return withWidgetPaint(blockNode({
    key,
    tagName: 'details',
    attrs,
    children,
  }), node);
}

function iframeRenderNode(node, options) {
  const context = renderContextFor(options);
  const attrs = attrsWithWidgetProps(node, 'iframe');
  const props = node.props && typeof node.props === 'object' ? node.props : {};
  const srcdoc = String(props.srcdoc ?? attrs.srcdoc ?? '');
  const key = String(node.key ?? 'iframe');
  const depth = Math.max(0, Number(options.iframeDepth ?? 0) || 0);
  const maxDepth = Math.max(0, Number(options.maxIframeDepth ?? DEFAULT_MAX_IFRAME_DEPTH) || 0);

  if (srcdoc.trim().length > 0 && depth < maxDepth) {
    const sceneId = embeddedSceneIdFor(context, key);
    const sceneInsertIndex = context.embeddedScenes.length;
    attrs['data-trueos-embedded-scene-id'] = sceneId;
    attrs['data-trueos-embedded-scene-source'] = 'iframe-srcdoc';
    try {
      const doc = parseHtml(srcdoc);
      buildCssStyleRefIndex(doc);
      const nestedTree = domToWidgets(doc, { rootKey: `${widgetKeyPath(key)}:iframe-doc` });
      const rows = textRowsFromWidgetTree(nestedTree);
      if (rows.length > 0 && attrs['data-trueos-srcdoc-text'] == null) {
        attrs['data-trueos-srcdoc-text'] = rows.join('\n');
      }
      const nestedRenderNodes = widgetTreeToRenderNodes(nestedTree, {
        ...options,
        iframeDepth: depth + 1,
        renderContext: context,
        wrapRoot: false,
      });
      context.embeddedScenes.splice(sceneInsertIndex, 0, {
        id: sceneId,
        parentKey: key,
        parentTagName: 'iframe',
        source: 'iframe-srcdoc',
        depth: depth + 1,
        widgetTree: nestedTree,
        renderNodes: nestedRenderNodes,
        renderHash: hashText(JSON.stringify(nestedRenderNodes)),
      });
    } catch (error) {
      attrs['data-trueos-embedded-scene-error'] = 'parse';
      attrs['data-trueos-srcdoc-text'] = attrs['data-trueos-srcdoc-text'] ?? '(iframe srcdoc parse error)';
      context.embeddedScenes.splice(sceneInsertIndex, 0, {
        id: sceneId,
        parentKey: key,
        parentTagName: 'iframe',
        source: 'iframe-srcdoc',
        depth: depth + 1,
        renderNodes: [textNode('(iframe srcdoc parse error)')],
        renderHash: hashText(JSON.stringify([textNode('(iframe srcdoc parse error)')])),
        error: error && error.message ? String(error.message) : 'iframe srcdoc parse error',
      });
    }
  }

  return withWidgetPaint(blockNode({
    key,
    tagName: 'iframe',
    attrs: stableObject(attrs),
    children: [],
  }), node);
}

export function widgetNodeToRenderNode(node, options = {}) {
  if (!node || typeof node !== 'object') return null;
  if (node.kind === 'text') {
    return textNode(node.text, { preserveWhitespace: node.preserveWhitespace === true });
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
  return withWidgetPaint(renderNode, node);
}

export function widgetTreeToRenderNodes(widgetTree, options = {}) {
  const renderContext = renderContextFor(options);
  const children = cleanChildren(widgetTree && widgetTree.children, { ...options, renderContext });
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
  const renderContext = renderContextFor(options);
  const renderNodes = widgetTreeToRenderNodes(widgetTree, { ...options, renderContext });
  const embeddedScenes = renderContext.embeddedScenes.map(publicEmbeddedScene);
  const renderHash = hashText(JSON.stringify({ rootId: 'root', renderNodes, embeddedScenes }));
  const artifact = {
    renderTree: {
      op: 'render-tree',
      version: RENDER_TRACE_VERSION,
      rootId: 'root',
      source,
      hash: renderHash,
      bytes,
      renderNodes,
    },
  };
  if (embeddedScenes.length > 0) {
    artifact.renderTree.embeddedScenes = embeddedScenes;
  }

  if (options.includeLayout === true) {
    const layout = buildWidgetTreeLayout(widgetTree, renderNodes, { ...options, viewport });
    const ui3PaintPlan = createUi3PaintPlan(layout, options);
    const embeddedLayoutScenes = buildEmbeddedSceneLayouts(
      renderContext.embeddedScenes,
      layout,
      { ...options, viewport },
    );
    const layoutHash = hashText(JSON.stringify({
      rootId: 'root',
      layout,
      embeddedScenes: embeddedLayoutScenes,
    }));
    const traceBody = {
      version: RENDER_TRACE_VERSION,
      rootId: 'root',
      source,
      viewport,
      renderHash,
      layoutHash,
      renderNodes,
      layout,
      ui3PaintPlan,
    };
    if (embeddedLayoutScenes.length > 0) traceBody.embeddedScenes = embeddedLayoutScenes;
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
