import { buttonLayoutDefaults, buttonSceneStyle } from './widgets/button.mjs';
import { headingTextContext, isHeadingTag } from './widgets/headings.mjs';
import { iframeContentOffset, iframeLayoutDefaults, iframeSceneOffset, isRootIframe } from './widgets/iframe.mjs';
import * as parse5 from 'parse5';

const ROOT_ID = 1;
const ROOT_IFRAME_ID = 2;

const KIND_CONTAINER = 0;
const KIND_GRAPHICS = 1;
const KIND_TEXT = 2;

const THEME = {
  text: 0x111111,
  mutedText: 0x666666,
  border: 0xdddddd,
  controlBorder: 0x666666,
  controlBg: 0xffffff,
  accent: 0x3b82f6,
  hr: 0xcccccc,
  tableHeader: 0xf7f7f7,
  buttonFill: 0xf2f2f2,
  buttonText: 0x111111,
  iframeChrome: 0xf3f4f6,
};

const BLOCK_TAGS = new Set([
  'address', 'article', 'aside', 'blockquote', 'body', 'button', 'canvas', 'color', 'details',
  'dialog', 'div', 'dl', 'fieldset', 'figcaption', 'figure', 'footer', 'form', 'h1', 'h2', 'h3',
  'h4', 'h5', 'h6', 'header', 'hr', 'iframe', 'img', 'input', 'label', 'main', 'meter', 'nav',
  'number', 'ol', 'p', 'progress', 'search', 'section', 'select', 'slider', 'stub', 'summary',
  'svg', 'table', 'tbody', 'td', 'textarea', 'tfoot', 'th', 'thead', 'tr', 'ul',
]);

const INTERACTIVE_TAGS = new Set([
  'button', 'input', 'textarea', 'select', 'slider', 'number', 'color', 'searchbutton', 'summary',
]);

const TRUEOS_SYNTHETIC_MARKERS = [
  '<truesurfer-parse5-trueos-host-core>',
  '<truesurfer-parse5-trueos-host-core',
  '<truesurfer-parse5-trueos-host-cor',
  '<truesurfer-parse5-trueos-host-event>',
  '<truesurfer-parse5-trueos-host-canvas>',
  '<truesurfer-parse5-trueos-host-dom>',
  '<truesurfer-parse5-trueos-host-fetch>',
  '<truesurfer-parse5-trueos-host-capture>',
  '<truesurfer-parse5-trueos-app.js>',
  '<truesurfer-parse5-trueos-app',
  '<truesurfer-init>',
  '<truesurfer-pixi-host-prelude>',
  '<truesurfer-pixi-capture-adapter>',
];

function stripTrueosSyntheticMarkers(value) {
  let s = String(value ?? '');
  if (s.indexOf('__TRUEOS_HOST_READY__') >= 0) {
    s = s.replace(/__TRUEOS_HOST_READY__/g, '');
  }
  if (s.indexOf('__trueos') >= 0) {
    s = s
      .replace(/__trueosNumberValue/g, '')
      .replace(/__trueosHostNum/g, '')
      .replace(/__trueosNum/g, '')
      .replace(/__trueosNu/g, '')
      .replace(/__trueosN/g, '');
  }
  if (s.indexOf('tsNu') >= 0 || s.indexOf('tsNum') >= 0) {
    s = s
      .replace(/tsNum/g, '')
      .replace(/tsNutsNutsNutsNu/g, '')
      .replace(/tsNutsNutsNu/g, '')
      .replace(/tsNutsNu/g, '');
  }
  if (s.indexOf('<truesurfer-') < 0) return s;
  for (const marker of TRUEOS_SYNTHETIC_MARKERS) {
    while (s.indexOf(marker) >= 0) s = s.replace(marker, '');
  }
  return s.replace(/<truesurfer-[A-Za-z0-9._-]+>/g, '');
}

function normalizeWhitespace(value) {
  let out = '';
  let inWs = false;
  const s = stripTrueosSyntheticMarkers(value);
  for (let i = 0; i < s.length; i += 1) {
    const c = s.charCodeAt(i);
    const ws = c === 32 || c === 9 || c === 10 || c === 13 || c === 12;
    if (ws) {
      inWs = true;
      continue;
    }
    if (inWs && out.length > 0) out += ' ';
    out += s.charAt(i);
    inWs = false;
  }
  return out;
}

function isElement(node) {
  return node && typeof node === 'object' && typeof node.nodeName === 'string' && Array.isArray(node.childNodes);
}

function isText(node) {
  return node && typeof node === 'object' && node.nodeName === '#text' && typeof node.value === 'string';
}

function getBody(doc) {
  const html = findFirstElement(doc, 'html');
  if (!html) return undefined;
  return findFirstElement(html, 'body');
}

function findFirstElement(node, tagName) {
  const children = Array.isArray(node?.childNodes) ? node.childNodes : [];
  for (const child of children) {
    if (isElement(child) && child.tagName === tagName) return child;
    const nested = findFirstElement(child, tagName);
    if (nested) return nested;
  }
  return undefined;
}

function attrsToMap(node) {
  const attrs = node?.attrs;
  if (!Array.isArray(attrs) || attrs.length === 0) return undefined;
  const out = {};
  for (const a of attrs) {
    if (a && typeof a.name === 'string') out[a.name] = String(a.value ?? '');
  }
  return Object.keys(out).length > 0 ? out : undefined;
}

function hasAttr(attrs, name) {
  return !!attrs && Object.prototype.hasOwnProperty.call(attrs, name);
}

function numberAttr(attrs, name, fallback) {
  const n = Number(attrs?.[name] ?? '');
  return Number.isFinite(n) && n > 0 ? n : fallback;
}

function clamp(n, lo, hi) {
  const v = Number(n);
  if (!Number.isFinite(v)) return lo;
  return Math.max(lo, Math.min(hi, v));
}

function extractText(node) {
  if (isText(node)) return node.value;
  if (!isElement(node)) return '';
  const children = Array.isArray(node.childNodes) ? node.childNodes : [];
  let out = '';
  for (let i = 0; i < children.length; i += 1) out += extractText(children[i]) + ' ';
  return out;
}

function toWidgetRenderTree(node, path = '0') {
  if (!isElement(node)) return [];

  const tagName = node.tagName ?? node.nodeName;
  const attrs = attrsToMap(node);
  if (tagName === 'textarea') {
    return [{
      kind: 'block',
      key: `${path}:${tagName}`,
      tagName,
      attrs: { ...(attrs ?? {}), value: normalizeWhitespace(extractText(node)) },
      children: [],
    }];
  }

  if (tagName === 'progress' || tagName === 'meter') {
    const fallbackText = normalizeWhitespace(extractText(node));
    const children = [];
    if (fallbackText.length > 0) children.push({ kind: 'text', text: fallbackText });
    children.push({ kind: 'block', key: `${path}:${tagName}`, tagName, attrs, children: [] });
    return [{ kind: 'block', key: `${path}:${tagName}-row`, tagName: 'barrow', attrs: { 'data-kind': tagName }, children }];
  }

  if (tagName === 'slider') {
    const sliderKey = `${path}:${tagName}`;
    return [{
      kind: 'block',
      key: `${path}:${tagName}-row`,
      tagName: 'barrow',
      attrs: { 'data-kind': tagName },
      children: [
        {
          kind: 'block',
          key: `${path}:${tagName}-label`,
          tagName: 'sliderlabel',
          attrs: { 'data-slider-key': sliderKey, 'data-slider-init': String(attrs?.value ?? '') },
          children: [],
        },
        { kind: 'block', key: sliderKey, tagName, attrs, children: [] },
      ],
    }];
  }

  if (tagName === 'search') {
    const inputAttrs = { ...(attrs ?? {}), type: 'text' };
    const inputKey = `${path}:search-input`;
    return [{
      kind: 'block',
      key: `${path}:search-row`,
      tagName: 'searchrow',
      attrs: {},
      children: [
        { kind: 'block', key: `${path}:search-btn`, tagName: 'searchbutton', attrs: { 'data-focus-key': inputKey }, children: [] },
        { kind: 'block', key: inputKey, tagName: 'input', attrs: inputAttrs, children: [] },
      ],
    }];
  }

  if (tagName === 'select') {
    const options = [];
    let selectedIndex = 0;
    const childNodes = Array.isArray(node.childNodes) ? node.childNodes : [];
    for (let i = 0; i < childNodes.length; i += 1) {
      const child = childNodes[i];
      if (!isElement(child) || child.tagName !== 'option') continue;
      const label = normalizeWhitespace(extractText(child));
      if (label.length > 0) options.push(label);
      if (hasAttr(attrsToMap(child), 'selected')) selectedIndex = Math.max(0, options.length - 1);
    }
    return [{
      kind: 'block',
      key: `${path}:${tagName}`,
      tagName,
      attrs: { ...(attrs ?? {}), 'data-options': options.join('\n'), 'data-selected-index': String(selectedIndex) },
      children: [],
    }];
  }

  if (tagName === 'color') {
    const mkSpin = (ch, value) => ({
      kind: 'block',
      key: `${path}:color-${ch}`,
      tagName: 'number',
      attrs: { channel: ch, min: '0', max: '255', step: '1', value: String(value) },
      children: [],
    });
    return [
      { kind: 'block', key: `${path}:color`, tagName: 'color', attrs, children: [] },
      {
        kind: 'block',
        key: `${path}:color-controls`,
        tagName: 'p',
        attrs: {},
        children: [mkSpin('r', 255), mkSpin('g', 0), mkSpin('b', 0), mkSpin('a', 255)],
      },
    ];
  }

  if (tagName === 'img' || tagName === 'canvas' || tagName === 'input' || tagName === 'number') {
    return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs, children: [] }];
  }

  if (tagName === 'svg') {
    let svg = '';
    try {
      svg = parse5.serialize(node);
    } catch {
      svg = '';
    }
    return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs: { ...(attrs ?? {}), 'data-svg': svg }, children: [] }];
  }

  if (tagName === 'iframe') {
    const srcdoc = String(attrs?.srcdoc ?? '');
    let children = [];
    if (srcdoc.trim().length > 0) {
      try {
        const doc = parse5.parse(stripTrueosSyntheticMarkers(srcdoc));
        const body = getBody(doc) ?? doc;
        children = toWidgetRenderTree(body, `${path}:iframe-doc`);
      } catch {
        children = [{ kind: 'text', text: '(iframe srcdoc parse error)' }];
      }
    }
    return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs, children }];
  }

  if (tagName === 'details' || tagName === 'stub') {
    const detailsKey = `${path}:${tagName}`;
    const childNodes = Array.isArray(node.childNodes) ? node.childNodes : [];
    const summaryEl = childNodes.find((child) => isElement(child) && child.tagName === 'summary');
    const summaryFallback = summaryEl
      ? normalizeWhitespace(extractText(summaryEl))
      : normalizeWhitespace(String(attrs?.summary ?? attrs?.title ?? '')) || 'Details';
    const children = [{
      kind: 'block',
      key: `${path}:summary`,
      tagName: 'summary',
      attrs: { ...(attrsToMap(summaryEl) ?? {}), 'data-details-key': detailsKey },
      children: toSummaryChildren(summaryEl, summaryFallback, `${path}:summary`),
    }];
    if (hasAttr(attrs, 'open')) {
      let elementIndex = 0;
      for (let i = 0; i < childNodes.length; i += 1) {
        const child = childNodes[i];
        if (!isElement(child) || child.tagName === 'summary') continue;
        children.push(...toWidgetRenderTree(child, `${path}.${elementIndex}`));
        elementIndex += 1;
      }
    }
    return [{ kind: 'block', key: detailsKey, tagName: 'details', attrs, children }];
  }

  const children = [];
  let inlineText = '';
  const childNodes = Array.isArray(node.childNodes) ? node.childNodes : [];

  for (let i = 0; i < childNodes.length; i += 1) {
    const child = childNodes[i];
    if (isText(child)) {
      inlineText += child.value;
      continue;
    }
    if (!isElement(child)) continue;

    const text = normalizeWhitespace(inlineText);
    if (text.length > 0) children.push({ kind: 'text', text });
    inlineText = '';

    const childTag = child.tagName ?? child.nodeName;
    if (BLOCK_TAGS.has(childTag)) {
      children.push(...toWidgetRenderTree(child, `${path}.${i}`));
    } else {
      inlineText += extractText(child) + ' ';
    }
  }

  const tail = normalizeWhitespace(inlineText);
  if (tail.length > 0) children.push({ kind: 'text', text: tail });

  if (tagName === 'html' || tagName === 'body') return children;
  return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs, children }];
}

function toSummaryChildren(summaryEl, fallback, path) {
  if (!summaryEl) return fallback.length > 0 ? [{ kind: 'text', text: fallback }] : [];
  const children = [];
  let inlineText = '';
  const childNodes = Array.isArray(summaryEl.childNodes) ? summaryEl.childNodes : [];
  for (let i = 0; i < childNodes.length; i += 1) {
    const child = childNodes[i];
    if (isText(child)) {
      inlineText += child.value;
      continue;
    }
    if (!isElement(child)) continue;
    const tag = child.tagName ?? child.nodeName;
    if (tag === 'input' || tag === 'button' || tag === 'select' || tag === 'textarea') {
      const text = normalizeWhitespace(inlineText);
      if (text.length > 0) children.push({ kind: 'text', text });
      inlineText = '';
      children.push(...toWidgetRenderTree(child, `${path}.${i}`));
    } else {
      inlineText += extractText(child) + ' ';
    }
  }
  const tail = normalizeWhitespace(inlineText);
  if (tail.length > 0) children.push({ kind: 'text', text: tail });
  return children.length > 0 ? children : fallback.length > 0 ? [{ kind: 'text', text: fallback }] : [];
}

function buildRootRenderTree(html) {
  const doc = parse5.parse(stripTrueosSyntheticMarkers(html));
  const body = getBody(doc) ?? doc;
  return [
    {
      kind: 'block',
      key: 'root:internal-iframe',
      tagName: 'iframe',
      attrs: { 'data-root': '1' },
      children: toWidgetRenderTree(body, '0'),
    },
  ];
}

function node(nodeId, kind) {
  return { code: 1, node: nodeId, a: kind };
}

function addChild(parent, child) {
  return { code: 2, node: parent, a: child };
}

function position(nodeId, x, y) {
  return { code: 3, node: nodeId, a: x, b: y };
}

function graphicsClear(nodeId) {
  return { code: 4, node: nodeId };
}

function graphicsRect(nodeId, x, y, w, h) {
  return { code: 5, node: nodeId, a: x, b: y, c: w, d: h };
}

function graphicsRoundRect(nodeId, x, y, w, h, radius) {
  return { code: 24, node: nodeId, a: x, b: y, c: w, d: h, text: String(radius ?? 0) };
}

function graphicsFill(nodeId, rgb, alpha = 1) {
  return { code: 6, node: nodeId, a: rgb >>> 0, b: alpha };
}

function graphicsStroke(nodeId, rgb, alpha = 1, width = 1) {
  return { code: 7, node: nodeId, a: rgb >>> 0, b: alpha, c: width };
}

function graphicsCircle(nodeId, x, y, radius) {
  return { code: 18, node: nodeId, a: x, b: y, c: radius };
}

function graphicsMoveTo(nodeId, x, y) {
  return { code: 19, node: nodeId, a: x, b: y };
}

function graphicsLineTo(nodeId, x, y) {
  return { code: 20, node: nodeId, a: x, b: y };
}

function text(nodeId, value) {
  return { code: 8, node: nodeId, text: String(value ?? '') };
}

function textFill(nodeId, rgb, alpha = 1) {
  return { code: 9, node: nodeId, a: rgb >>> 0, b: alpha };
}

function textFontSizeTier(nodeId, tier) {
  return { code: 29, node: nodeId, a: tier };
}

function listen(nodeId, event) {
  return { code: 16, node: nodeId, text: String(event ?? '') };
}

function textFillForContext(ctx) {
  if (ctx && typeof ctx.textFill === 'number') return ctx.textFill >>> 0;
  return ctx && ctx.bold ? 0x111111 : 0x333333;
}

function textTierForContext(ctx) {
  return ctx && ctx.bold ? 2 : 1;
}

function estimateTextHeight(value) {
  const len = normalizeWhitespace(value).length;
  return Math.max(22, Math.ceil(len / 64) * 22);
}

function widgetSize(renderNode) {
  if (!renderNode || renderNode.kind === 'text') return { w: 620, h: estimateTextHeight(renderNode?.text ?? '') };
  const tag = String(renderNode.tagName ?? '').toLowerCase();
  const attrs = renderNode.attrs ?? {};
  if (tag === 'hr') return { w: 620, h: 5 };
  if (tag === 'button') return { w: Math.max(132, estimateInlineText(renderNode) * 8 + 36), h: 42 };
  if (tag === 'input') {
    const type = String(attrs.type ?? 'text').toLowerCase();
    if (type === 'checkbox' || type === 'radio') return { w: 18, h: 18 };
    return { w: numberAttr(attrs, 'width', 260), h: 36 };
  }
  if (tag === 'textarea') return { w: numberAttr(attrs, 'width', 320), h: numberAttr(attrs, 'height', 108) };
  if (tag === 'select') return { w: numberAttr(attrs, 'width', 220), h: 36 };
  if (tag === 'progress' || tag === 'meter' || tag === 'slider') return { w: numberAttr(attrs, 'width', 240), h: 24 };
  if (tag === 'sliderlabel') return { w: 56, h: 24 };
  if (tag === 'number') return { w: 84, h: 32 };
  if (tag === 'color') return { w: 260, h: 172 };
  if (tag === 'searchbutton') return { w: 36, h: 36 };
  if (tag === 'searchrow' || tag === 'barrow') return { w: 620, h: estimateChildrenHeight(renderNode.children, 8, 36) + 8 };
  if (tag === 'img') return { w: numberAttr(attrs, 'width', 160), h: numberAttr(attrs, 'height', 120) };
  if (tag === 'svg' || tag === 'canvas') return { w: numberAttr(attrs, 'width', 300), h: numberAttr(attrs, 'height', 150) };
  if (tag === 'iframe') {
    if (isRootIframe(renderNode)) return { w: 0, h: 0 };
    return { w: numberAttr(attrs, 'width', 420), h: numberAttr(attrs, 'height', 240) };
  }
  if (tag === 'td' || tag === 'th') return { w: 220, h: estimateChildrenHeight(renderNode.children, 4, 34) + 12 };
  if (tag === 'tr') return { w: 620, h: estimateChildrenHeight(renderNode.children, 0, 38) };
  if (tag === 'h1') return { w: 620, h: 48 };
  if (tag === 'h2') return { w: 620, h: 42 };
  if (isHeadingTag(tag)) return { w: 620, h: 38 };
  if (tag === 'dialog') return { w: 520, h: estimateChildrenHeight(renderNode.children, 8, 80) + 28 };
  return { w: 620, h: estimateChildrenHeight(renderNode.children, 8, 36) + 12 };
}

function estimateInlineText(renderNode) {
  let out = 0;
  const children = Array.isArray(renderNode?.children) ? renderNode.children : [];
  for (let i = 0; i < children.length; i += 1) {
    const child = children[i];
    if (child.kind === 'text') out += normalizeWhitespace(child.text).length;
  }
  return out;
}

function estimateChildrenHeight(children, gap, minHeight) {
  const list = Array.isArray(children) ? children : [];
  let h = 0;
  for (let i = 0; i < list.length; i += 1) {
    h += list[i].kind === 'text' ? estimateTextHeight(list[i].text) : widgetSize(list[i]).h;
    if (i + 1 < list.length) h += gap;
  }
  return Math.max(minHeight, h);
}

function appendTextNode(value, parentId, state, x, y, ctx = state.context) {
  const textValue = normalizeWhitespace(value);
  if (textValue.length === 0) return 0;
  const id = state.nextId;
  state.nextId += 1;
  state.ops.push(node(id, KIND_TEXT), addChild(parentId, id), position(id, x, y));
  state.ops.push(textFill(id, textFillForContext(ctx), 1), textFontSizeTier(id, textTierForContext(ctx)), text(id, textValue));
  state.textCount += 1;
  state.textBytes += textValue.length;
  return id;
}

function drawBox(state, id, w, h, fill, stroke = THEME.border, radius = 0) {
  state.ops.push(graphicsClear(id));
  if (radius > 0) state.ops.push(graphicsRoundRect(id, 0.5, 0.5, Math.max(0, w - 1), Math.max(0, h - 1), radius));
  else state.ops.push(graphicsRect(id, 0.5, 0.5, Math.max(0, w - 1), Math.max(0, h - 1)));
  state.ops.push(graphicsFill(id, fill, 1), graphicsStroke(id, stroke, 1, 1));
}

function progressValue(attrs, tagName) {
  const max = Number(attrs?.max ?? (tagName === 'meter' ? 1 : 100));
  const value = Number(attrs?.value ?? 0);
  if (!Number.isFinite(max) || max <= 0) return 0;
  return clamp(value / max, 0, 1);
}

function drawWidgetChrome(tagName, renderNode, id, state, size) {
  const attrs = renderNode.attrs ?? {};
  if (tagName === 'hr') {
    state.ops.push(graphicsClear(id), graphicsRect(id, 0, 2, size.w, 1), graphicsFill(id, THEME.hr, 1));
    return;
  }
  if (tagName === 'button' || tagName === 'searchbutton') {
    drawBox(state, id, size.w, size.h, tagName === 'searchbutton' ? THEME.controlBg : THEME.buttonFill, THEME.controlBorder, tagName === 'searchbutton' ? 0 : 4);
    if (tagName === 'searchbutton') {
      state.ops.push(graphicsCircle(id, 14, 14, 6), graphicsStroke(id, THEME.mutedText, 1, 2));
      state.ops.push(graphicsMoveTo(id, 19, 19), graphicsLineTo(id, 26, 26), graphicsStroke(id, THEME.mutedText, 1, 2));
    }
    return;
  }
  if (tagName === 'input') {
    const type = String(attrs.type ?? 'text').toLowerCase();
    if (type === 'checkbox') {
      drawBox(state, id, size.w, size.h, THEME.controlBg, THEME.controlBorder, 0);
      if (hasAttr(attrs, 'checked')) {
        state.ops.push(graphicsRect(id, 4, 4, Math.max(0, size.w - 8), Math.max(0, size.h - 8)), graphicsFill(id, THEME.accent, 1));
      }
    } else if (type === 'radio') {
      state.ops.push(graphicsClear(id), graphicsCircle(id, size.w / 2, size.h / 2, Math.min(size.w, size.h) / 2 - 1));
      state.ops.push(graphicsFill(id, THEME.controlBg, 1), graphicsStroke(id, THEME.controlBorder, 1, 1));
      if (hasAttr(attrs, 'checked')) {
        state.ops.push(graphicsCircle(id, size.w / 2, size.h / 2, Math.min(size.w, size.h) / 2 - 5), graphicsFill(id, THEME.accent, 1));
      }
    } else {
      drawBox(state, id, size.w, size.h, THEME.controlBg, THEME.controlBorder, 0);
    }
    return;
  }
  if (tagName === 'textarea' || tagName === 'select' || tagName === 'number') {
    drawBox(state, id, size.w, size.h, THEME.controlBg, THEME.controlBorder, 0);
    if (tagName === 'select') {
      state.ops.push(graphicsMoveTo(id, size.w - 24, 14), graphicsLineTo(id, size.w - 17, 22), graphicsLineTo(id, size.w - 10, 14), graphicsStroke(id, THEME.mutedText, 1, 1));
    }
    if (tagName === 'number') {
      state.ops.push(graphicsRect(id, size.w - 24, 0, 24, size.h), graphicsFill(id, 0xf7f7f7, 1), graphicsStroke(id, THEME.border, 1, 1));
    }
    return;
  }
  if (tagName === 'progress' || tagName === 'meter' || tagName === 'slider') {
    drawBox(state, id, size.w, size.h, THEME.controlBg, THEME.controlBorder, 0);
    const pct = progressValue(attrs, tagName === 'slider' ? 'meter' : tagName);
    const fillW = Math.max(0, Math.floor((size.w - 4) * pct));
    state.ops.push(graphicsRect(id, 2, 2, fillW, Math.max(0, size.h - 4)), graphicsFill(id, THEME.accent, 1));
    if (tagName === 'slider') {
      const knobX = 2 + fillW;
      state.ops.push(graphicsCircle(id, knobX, size.h / 2, 8), graphicsFill(id, 0xffffff, 1), graphicsStroke(id, THEME.accent, 1, 2));
    }
    return;
  }
  if (tagName === 'color') {
    state.ops.push(graphicsClear(id));
    state.ops.push(graphicsRect(id, 0, 0, size.w, size.h), graphicsFill(id, 0xffffff, 1), graphicsStroke(id, THEME.controlBorder, 1, 1));
    state.ops.push(graphicsRect(id, 12, 12, size.w - 24, size.h - 48), graphicsFill(id, 0xff0000, 1));
    state.ops.push(graphicsRect(id, 12, size.h - 28, size.w - 24, 12), graphicsFill(id, 0x111111, 1));
    return;
  }
  if (tagName === 'img' || tagName === 'svg' || tagName === 'canvas') {
    drawBox(state, id, size.w, size.h, 0xffffff, THEME.controlBorder, 0);
    if (tagName === 'canvas') {
      state.ops.push(graphicsMoveTo(id, 8, size.h - 8), graphicsLineTo(id, size.w - 8, 8), graphicsStroke(id, THEME.border, 1, 1));
    }
    return;
  }
  if (tagName === 'iframe' && !isRootIframe(renderNode)) {
    drawBox(state, id, size.w, size.h, 0xffffff, THEME.controlBorder, 0);
    state.ops.push(graphicsRect(id, 1, 1, Math.max(0, size.w - 2), 26), graphicsFill(id, THEME.iframeChrome, 1));
    return;
  }
  if (tagName === 'dialog') {
    drawBox(state, id, size.w, size.h, 0xffffff, 0x999999, 4);
    state.ops.push(graphicsRect(id, 1, 1, Math.max(0, size.w - 2), 26), graphicsFill(id, 0xf7f7f7, 1));
    return;
  }
  if (tagName === 'table') {
    drawBox(state, id, size.w, size.h, 0xffffff, 0x999999, 0);
    return;
  }
  if (tagName === 'td' || tagName === 'th') {
    drawBox(state, id, size.w, size.h, tagName === 'th' ? THEME.tableHeader : 0xffffff, 0xb0b0b0, 0);
  }
}

function appendWidgetOps(renderNode, parentId, state, layout, depth = 0) {
  if (!renderNode || typeof renderNode !== 'object') return;
  const id = state.nextId;
  state.nextId += 1;

  if (renderNode.kind === 'text') {
    const value = normalizeWhitespace(renderNode.text);
    if (value.length === 0) return;
    state.ops.push(node(id, KIND_TEXT), addChild(parentId, id), position(id, layout.childX + 12, layout.nextY + 6));
    state.ops.push(textFill(id, textFillForContext(state.context), 1), textFontSizeTier(id, textTierForContext(state.context)), text(id, value));
    state.textCount += 1;
    state.textBytes += value.length;
    layout.nextY += 24;
    return;
  }

  if (renderNode.kind !== 'block') return;
  const tagName = String(renderNode.tagName ?? 'block').toLowerCase();
  const iframeRoot = tagName === 'iframe' && isRootIframe(renderNode);
  const iframeOffset = tagName === 'iframe' ? iframeSceneOffset(renderNode) : null;
  const y = iframeRoot ? iframeOffset.y : layout.nextY;
  const x = iframeRoot ? iframeOffset.x : layout.childX;
  const size = widgetSize(renderNode);
  let blockAdvance = Math.max(0, size.h + 8);
  let childLayout = { childX: 0, nextY: 0 };
  if (tagName === 'iframe') {
    const iframeDefaults = iframeLayoutDefaults(renderNode);
    const contentOffset = iframeContentOffset(renderNode);
    childLayout = { childX: contentOffset.x, nextY: contentOffset.y };
    blockAdvance = iframeRoot ? 0 : Math.max(56, Number(iframeDefaults.height ?? size.h));
    state.iframeCount += 1;
    if (String(renderNode.attrs?.srcdoc ?? '').trim().length > 0) state.iframeSrcdocCount += 1;
  }
  const draws = tagName === 'hr' || tagName === 'button' || tagName === 'searchbutton' || tagName === 'input'
    || tagName === 'textarea' || tagName === 'select' || tagName === 'number' || tagName === 'progress'
    || tagName === 'meter' || tagName === 'slider' || tagName === 'color' || tagName === 'img'
    || tagName === 'svg' || tagName === 'canvas' || tagName === 'dialog' || tagName === 'table'
    || tagName === 'td' || tagName === 'th' || (tagName === 'iframe' && !iframeRoot);
  state.ops.push(node(id, draws ? KIND_GRAPHICS : KIND_CONTAINER), addChild(parentId, id), position(id, x, y));
  if (draws) drawWidgetChrome(tagName, renderNode, id, state, size);
  if (INTERACTIVE_TAGS.has(tagName)) {
    state.ops.push(listen(id, 'pointerdown'), listen(id, 'pointermove'), listen(id, 'pointerup'));
  }
  if (tagName === 'button') {
    buttonLayoutDefaults();
    const style = buttonSceneStyle();
    blockAdvance = Math.max(blockAdvance, Number(style.height || 42) + 14);
    state.buttonCount += 1;
  }
  if (!iframeRoot) layout.nextY += blockAdvance;

  const previousContext = state.context;
  if (tagName === 'button') {
    state.context = { ...previousContext, textFill: buttonSceneStyle().textFill };
  } else {
    state.context = isHeadingTag(tagName) ? headingTextContext(tagName, previousContext) : previousContext;
  }
  if (tagName !== 'iframe') childLayout = { childX: tagName === 'p' || tagName === 'label' ? 4 : 12, nextY: tagName === 'dialog' ? 32 : 8 };
  if (tagName === 'button') childLayout = { childX: 14, nextY: 9 };
  if (tagName === 'summary') childLayout = { childX: 18, nextY: 6 };
  if (tagName === 'td' || tagName === 'th') childLayout = { childX: 8, nextY: 6 };
  if (tagName === 'iframe' && !iframeRoot) childLayout = { childX: 8, nextY: 34 };
  const children = Array.isArray(renderNode.children) ? renderNode.children : [];
  if ((tagName === 'input' || tagName === 'textarea') && String(renderNode.attrs?.type ?? 'text').toLowerCase() !== 'checkbox' && String(renderNode.attrs?.type ?? 'text').toLowerCase() !== 'radio') {
    appendTextNode(renderNode.attrs?.value ?? renderNode.attrs?.placeholder ?? '', id, state, 8, 8, { textFill: THEME.text });
  } else if (tagName === 'select') {
    const opts = String(renderNode.attrs?.['data-options'] ?? '').split('\n').filter(Boolean);
    const idx = clamp(Number(renderNode.attrs?.['data-selected-index'] ?? 0), 0, Math.max(0, opts.length - 1));
    appendTextNode(opts[idx] ?? '', id, state, 8, 8, { textFill: THEME.text });
  } else if (tagName === 'number') {
    appendTextNode(renderNode.attrs?.value ?? '0', id, state, 8, 7, { textFill: THEME.text });
  } else if (tagName === 'sliderlabel') {
    const raw = Number(renderNode.attrs?.['data-slider-init'] ?? 0);
    appendTextNode(`${Math.round(clamp(raw, 0, 1) * 100)}%`, id, state, 0, 2, { textFill: THEME.mutedText });
  } else if (tagName === 'img' || tagName === 'svg' || tagName === 'canvas') {
    appendTextNode(renderNode.attrs?.alt ?? tagName, id, state, 8, 8, { textFill: THEME.mutedText });
  } else if (tagName === 'iframe' && !iframeRoot) {
    appendTextNode(String(renderNode.attrs?.srcdoc ?? '').trim().length > 0 ? 'iframe (srcdoc)' : 'iframe (empty)', id, state, 8, 6, { textFill: THEME.mutedText });
  }
  const rowLike = tagName === 'p' || tagName === 'label' || tagName === 'barrow' || tagName === 'searchrow' || tagName === 'tr';
  if (rowLike) {
    let rowX = childLayout.childX;
    let rowY = childLayout.nextY;
    let rowH = 0;
    for (let i = 0; i < children.length; i += 1) {
      const child = children[i];
      const childSize = widgetSize(child);
      const itemLayout = { childX: rowX, nextY: rowY };
      appendWidgetOps(child, id, state, itemLayout, depth + 1);
      rowX += Math.max(24, childSize.w) + 8;
      rowH = Math.max(rowH, Math.max(24, childSize.h));
      if (rowX > 560 && tagName !== 'tr') {
        rowX = childLayout.childX;
        rowY += rowH + 8;
        rowH = 0;
      }
    }
  } else {
    for (let i = 0; i < children.length; i += 1) {
      appendWidgetOps(children[i], id, state, childLayout, depth + 1);
    }
  }
  state.context = previousContext;
}

export function buildTextWidgetScene(html) {
  const renderTree = buildRootRenderTree(html);
  const state = {
    nextId: ROOT_IFRAME_ID,
    ops: [],
    context: {},
    iframeCount: 0,
    iframeSrcdocCount: 0,
    buttonCount: 0,
    textCount: 0,
    textBytes: 0,
  };
  const rootLayout = { childX: 0, nextY: 0 };
  for (let i = 0; i < renderTree.length; i += 1) appendWidgetOps(renderTree[i], ROOT_ID, state, rootLayout);

  return {
    ok: 1,
    ui3Scene: {
      version: 1,
      commandSource: 'qjs-widget-text',
      rootId: ROOT_ID,
      opCount: state.ops.length,
      ops: state.ops,
    },
    widget: {
      module: '/qjs/truesurfer/text_widget_scene.mjs',
      buttonHelper: '/qjs/truesurfer/widgets/button.mjs',
      headingHelper: '/qjs/truesurfer/widgets/headings.mjs',
      iframeHelper: '/qjs/truesurfer/widgets/iframe.mjs',
      renderer: 'parse5-widget-subset',
      tags: 'iframe,h1-h6,p,label,button,input,textarea,select,search,details,summary,dialog,hr,progress,meter,slider,number,color,table,tr,td,th,img,svg,canvas',
      iframeCount: state.iframeCount,
      iframeSrcdocCount: state.iframeSrcdocCount,
      buttonCount: state.buttonCount,
      textCount: state.textCount,
      textBytes: state.textBytes,
    },
  };
}

export const buildDemoTextWidgetScene = buildTextWidgetScene;
globalThis.__trueosBuildTextWidgetScene = buildTextWidgetScene;
globalThis.__trueosBuildDemoTextWidgetScene = buildDemoTextWidgetScene;
