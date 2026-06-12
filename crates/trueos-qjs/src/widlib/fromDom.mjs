import * as parse5 from 'parse5';
import { BLOCK_TAGS, INLINE_TAGS, REPLACED_TAGS } from './tags.mjs';
import {
  attrsToObject,
  directChildElements,
  extractText,
  getBody,
  isElement,
  isText,
  normalizeWhitespace,
} from './dom.mjs';
import { defaultRegistry } from './registry.mjs';
import {
  iframeSrcdocProps,
  normalizeImageProps,
  normalizeColorRgba,
  normalizeMeterRatio,
  normalizeNumberValue,
  normalizeProgressRatio,
  normalizeSearchAttrs,
  normalizeSelectState,
  normalizeSliderValue,
  normalizeTemporalKind,
  parseTemporalValue,
  replacedDimensionsFromAttrs,
} from './widgets/index.mjs';

function makeText(text) {
  return { kind: 'text', text };
}

function makeWidget({ tag, key, attrs = {}, props = {}, children = [], registry, styleRef = null, paint = null }) {
  const meta = registry.get(tag, attrs);
  const metaPaint = paint && typeof paint === 'object' && !Array.isArray(paint) ? { ...paint } : undefined;
  return {
    kind: 'widget',
    key,
    tag,
    widget: meta.id,
    role: meta.role,
    category: meta.category,
    attrs,
    props,
    children,
    meta: {
      source: meta.source,
      kind: meta.kind,
      complexity: meta.complexity,
      leaf: meta.leaf,
      interactive: meta.interactive,
      complex: meta.complex,
      currentStatus: meta.currentStatus,
      notes: meta.notes,
      layoutDefaults: meta.layoutDefaults,
      attrs: meta.attrs,
      state: meta.state,
      interactions: meta.interactions,
      overlays: meta.overlays,
      expandsTo: meta.expandsTo,
      styleRef,
      paint: metaPaint,
    },
  };
}

function svgMarkup(node) {
  try {
    return parse5.serialize(node);
  } catch {
    return '';
  }
}

function selectProps(node) {
  return normalizeSelectState({
    attrs: attrsToObject(node),
    childNodes: directChildElements(node),
  });
}

function detailsChildren(node, path, opts) {
  const children = [];
  const summary = directChildElements(node, 'summary')[0];
  const detailsKey = `${path}:details`;

  if (summary) {
    children.push(
      makeWidget({
        tag: 'summary',
        key: `${path}:summary`,
        attrs: attrsToObject(summary),
        props: { detailsKey },
        children: childNodesToWidgets(summary, `${path}:summary`, opts),
        registry: opts.registry,
      })
    );
  }

  let index = 0;
  for (const child of node.childNodes ?? []) {
    if (!isElement(child)) continue;
    const tag = String(child.tagName ?? child.nodeName).toLowerCase();
    if (tag === 'summary') continue;
    children.push(...nodeToWidgets(child, `${path}.${index}`, opts));
    index += 1;
  }

  return children;
}

function childNodesToWidgets(node, path, opts) {
  const out = [];
  let inlineText = '';
  let elementIndex = 0;

  const flushText = () => {
    const text = normalizeWhitespace(inlineText);
    inlineText = '';
    if (text.length > 0) out.push(makeText(text));
  };

  for (const child of node.childNodes ?? []) {
    if (isText(child)) {
      inlineText += child.value ?? '';
      continue;
    }

    if (!isElement(child)) continue;

    const tag = String(child.tagName ?? child.nodeName).toLowerCase();
    const childPath = `${path}.${elementIndex}`;
    elementIndex += 1;

    if (BLOCK_TAGS.has(tag) || opts.keepUnknownElements) {
      flushText();
      out.push(...nodeToWidgets(child, childPath, opts));
    } else if (INLINE_TAGS.has(tag)) {
      if (tag === 'br') inlineText += '\n';
      else inlineText += `${extractText(child)} `;
    }
  }

  flushText();
  return out;
}

export function nodeToWidgets(node, path = '0', options = {}) {
  const opts = {
    registry: options.registry ?? defaultRegistry,
    keepUnknownElements: Boolean(options.keepUnknownElements),
  };

  if (isText(node)) {
    const text = normalizeWhitespace(node.value ?? '');
    return text.length > 0 ? [makeText(text)] : [];
  }

  if (!isElement(node)) return [];

  const tag = String(node.tagName ?? node.nodeName).toLowerCase();
  if (tag === 'html' || tag === 'body') return childNodesToWidgets(node, path, opts);

  const attrs = attrsToObject(node);
  const key = `${path}:${tag}`;
  let children = [];
  let props = {};

  if (tag === 'textarea') {
    props = { value: extractText(node) };
  } else if (tag === 'select') {
    props = selectProps(node);
  } else if (tag === 'svg') {
    props = { markup: svgMarkup(node), dimensions: replacedDimensionsFromAttrs(attrs) };
  } else if (tag === 'progress' || tag === 'meter') {
    props = {
      fallbackText: normalizeWhitespace(extractText(node)),
      ratio: tag === 'progress' ? normalizeProgressRatio(attrs) : normalizeMeterRatio(attrs),
    };
  } else if (tag === 'slider') {
    props = normalizeSliderValue(attrs);
  } else if (tag === 'number') {
    props = normalizeNumberValue(attrs);
  } else if (tag === 'color') {
    props = normalizeColorRgba(attrs);
  } else if (tag === 'search') {
    props = normalizeSearchAttrs(attrs);
  } else if (tag === 'details') {
    props = { open: 'open' in attrs };
    children = detailsChildren(node, path, opts);
  } else if (tag === 'img') {
    props = normalizeImageProps(attrs);
  } else if (tag === 'canvas') {
    props = { dimensions: replacedDimensionsFromAttrs(attrs) };
  } else if (tag === 'iframe') {
    props = { ...iframeSrcdocProps(attrs), dimensions: replacedDimensionsFromAttrs(attrs) };
  } else if (tag === 'input') {
    const inputType = String(attrs.type ?? 'text').toLowerCase();
    if (inputType === 'date' || inputType === 'time' || inputType === 'month' || inputType === 'week' || inputType === 'datetime-local') {
      const kind = normalizeTemporalKind(inputType);
      props = parseTemporalValue(kind, attrs.value ?? '');
    }
  } else if (!REPLACED_TAGS.has(tag) && !opts.registry.get(tag, attrs).leaf) {
    children = childNodesToWidgets(node, path, opts);
  }

  return [
    makeWidget({
      tag,
      key,
      attrs,
      props,
      children,
      registry: opts.registry,
      styleRef: node.__trueosStyleRef ?? null,
      paint: node.__trueosComputedStyle && typeof node.__trueosComputedStyle.paint === 'object'
        ? node.__trueosComputedStyle.paint
        : null,
    }),
  ];
}

export function domToWidgets(dom, options = {}) {
  const registry = options.registry ?? defaultRegistry;
  const rootKey = options.rootKey ?? 'root';
  const body = getBody(dom) ?? dom;
  const children = nodeToWidgets(body, rootKey, { ...options, registry });

  return {
    kind: 'widget-root',
    key: rootKey,
    children,
    registry: registry.entries(),
  };
}
