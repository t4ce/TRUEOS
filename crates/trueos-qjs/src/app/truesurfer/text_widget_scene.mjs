import { buttonLayoutDefaults, buttonSceneStyle } from './widgets/button.mjs';
import { detailsIsOpen, detailsSummarySceneStyle, summaryLayoutDefaults } from './widgets/detailsSummary.mjs';
import { colorLayoutDefaults, colorSceneStyle } from './widgets/color.mjs';
import {
  controlSceneStyle,
  inputLayoutDefaults,
  numberLayoutDefaults,
  searchButtonLayoutDefaults,
  selectLayoutDefaults,
  selectedOptionLabel,
  textareaLayoutDefaults,
} from './widgets/formControls.mjs';
import { headingTextContext, isHeadingTag } from './widgets/headings.mjs';
import { iframeContentOffset, iframeLayoutDefaults, iframeSceneOffset, isRootIframe } from './widgets/iframe.mjs';
import { progressMeterLayoutDefaults, progressMeterRatio, progressMeterSceneStyle } from './widgets/progressMeter.mjs';
import { replacedElementLayoutDefaults, replacedElementSceneStyle } from './widgets/replacedElements.mjs';
import { sliderLabelLayoutDefaults, sliderLayoutDefaults, sliderRatio, sliderSceneStyle } from './widgets/slider.mjs';
import { tableCellLayoutDefaults, tableSceneStyle } from './widgets/table.mjs';
import {
  temporalDatePart,
  temporalDisplayValue,
  temporalKindFromTag,
  temporalLayoutDefaults,
  temporalSceneStyle,
  temporalTagForInputType,
  temporalTimePart,
} from './widgets/temporal.mjs';
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
  'svg', 'table', 'tbody', 'td', 'textarea', 'tfoot', 'th', 'thead', 'timeinput', 'tr', 'ul',
  'dateinput', 'monthinput', 'weekinput', 'datetimelocalinput',
]);

const INTERACTIVE_TAGS = new Set([
  'button', 'input', 'textarea', 'select', 'slider', 'number', 'color', 'searchbutton', 'summary',
  'timeinput', 'dateinput', 'monthinput', 'weekinput', 'datetimelocalinput',
]);

const ROW_TAGS = new Set(['barrow', 'searchrow', 'tr']);

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

  if (tagName === 'input') {
    const temporalTag = temporalTagForInputType(attrs?.type ?? 'text');
    if (temporalTag) {
      return [{ kind: 'block', key: `${path}:input`, tagName: temporalTag, attrs, children: [] }];
    }
    return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs, children: [] }];
  }

  if (tagName === 'img' || tagName === 'canvas' || tagName === 'number') {
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
      attrs: { ...(attrsToMap(summaryEl) ?? {}), 'data-details-key': detailsKey, 'data-details-open': hasAttr(attrs, 'open') ? '1' : '0' },
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

function listen(nodeId, event) {
  return { code: 16, node: nodeId, a: pointerEventCode(event) };
}

function pointerEventCode(event) {
  const e = String(event ?? '');
  if (e === 'pointerdown') return 1;
  if (e === 'pointerup') return 2;
  if (e === 'pointermove') return 3;
  if (e === 'pointerover') return 4;
  if (e === 'pointerout') return 5;
  if (e === 'pointerupoutside') return 6;
  if (e === 'contextmenu') return 7;
  return 1;
}

function textFillForContext(ctx) {
  if (ctx && typeof ctx.textFill === 'number') return ctx.textFill >>> 0;
  return ctx && ctx.bold ? 0x111111 : 0x333333;
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
    const defaults = inputLayoutDefaults(type);
    if (type === 'checkbox' || type === 'radio') return { w: numberAttr(attrs, 'width', defaults.width), h: numberAttr(attrs, 'height', defaults.height) };
    return { w: numberAttr(attrs, 'width', defaults.width), h: numberAttr(attrs, 'height', defaults.height) };
  }
  const temporalKind = temporalKindFromTag(tag);
  if (temporalKind) {
    const defaults = temporalLayoutDefaults(temporalKind);
    return { w: numberAttr(attrs, 'width', defaults.width), h: numberAttr(attrs, 'height', defaults.height) };
  }
  if (tag === 'textarea') {
    const defaults = textareaLayoutDefaults();
    return { w: numberAttr(attrs, 'width', defaults.width), h: numberAttr(attrs, 'height', defaults.height) };
  }
  if (tag === 'select') {
    const defaults = selectLayoutDefaults();
    return { w: numberAttr(attrs, 'width', defaults.width), h: numberAttr(attrs, 'height', defaults.height) };
  }
  if (tag === 'progress' || tag === 'meter') {
    const defaults = progressMeterLayoutDefaults();
    return { w: numberAttr(attrs, 'width', defaults.width), h: numberAttr(attrs, 'height', defaults.height) };
  }
  if (tag === 'slider') {
    const defaults = sliderLayoutDefaults();
    return { w: numberAttr(attrs, 'width', defaults.width), h: numberAttr(attrs, 'height', defaults.height) };
  }
  if (tag === 'sliderlabel') {
    const defaults = sliderLabelLayoutDefaults();
    return { w: defaults.width, h: defaults.height };
  }
  if (tag === 'number') {
    const defaults = numberLayoutDefaults();
    return { w: numberAttr(attrs, 'width', defaults.width), h: numberAttr(attrs, 'height', defaults.height) };
  }
  if (tag === 'color') {
    const defaults = colorLayoutDefaults(attrs);
    return { w: numberAttr(attrs, 'width', defaults.width), h: numberAttr(attrs, 'height', defaults.height) };
  }
  if (tag === 'searchbutton') {
    const defaults = searchButtonLayoutDefaults();
    return { w: defaults.width, h: defaults.height };
  }
  if (tag === 'searchrow' || tag === 'barrow') return { w: 620, h: estimateRowHeight(renderNode.children, 36) + 8 };
  if (tag === 'img') {
    const defaults = replacedElementLayoutDefaults(attrs, tag);
    return { w: numberAttr(attrs, 'width', defaults.width), h: numberAttr(attrs, 'height', defaults.height) };
  }
  if (tag === 'svg' || tag === 'canvas') {
    const defaults = replacedElementLayoutDefaults(attrs, tag);
    return { w: numberAttr(attrs, 'width', defaults.width), h: numberAttr(attrs, 'height', defaults.height) };
  }
  if (tag === 'iframe') {
    if (isRootIframe(renderNode)) return { w: 0, h: 0 };
    return { w: numberAttr(attrs, 'width', 420), h: numberAttr(attrs, 'height', 240) };
  }
  if (tag === 'td' || tag === 'th') {
    const defaults = tableCellLayoutDefaults();
    return { w: 220, h: estimateChildrenHeight(renderNode.children, 4, 34) + defaults.paddingTop + defaults.paddingBottom };
  }
  if (tag === 'tr') return { w: estimateRowWidth(renderNode.children, 620), h: estimateRowHeight(renderNode.children, 38) };
  if (tag === 'summary') return { w: 620, h: Math.max(summaryLayoutDefaults().minHeight, estimateChildrenHeight(renderNode.children, 4, 24) + 12) };
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

function collectInlineText(renderNode) {
  const parts = [];
  const children = Array.isArray(renderNode?.children) ? renderNode.children : [];
  for (let i = 0; i < children.length; i += 1) {
    const child = children[i];
    if (child.kind === 'text') {
      const text = normalizeWhitespace(child.text);
      if (text.length > 0) parts.push(text);
    }
  }
  return parts.join(' ');
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

function estimateRowHeight(children, minHeight) {
  const list = Array.isArray(children) ? children : [];
  let h = 0;
  for (let i = 0; i < list.length; i += 1) {
    h = Math.max(h, list[i].kind === 'text' ? estimateTextHeight(list[i].text) : widgetSize(list[i]).h);
  }
  return Math.max(minHeight, h);
}

function estimateRowWidth(children, fallback) {
  const list = Array.isArray(children) ? children : [];
  let w = 0;
  for (let i = 0; i < list.length; i += 1) {
    w += list[i].kind === 'text' ? Math.max(32, normalizeWhitespace(list[i].text).length * 8) : widgetSize(list[i]).w;
    if (i + 1 < list.length) w += 8;
  }
  return Math.max(fallback, w);
}

function appendTextNode(value, parentId, state, x, y, ctx = state.context) {
  const textValue = normalizeWhitespace(value);
  if (textValue.length === 0) return 0;
  const id = state.nextId;
  state.nextId += 1;
  state.ops.push(node(id, KIND_TEXT), addChild(parentId, id), position(id, x, y));
  state.ops.push(textFill(id, textFillForContext(ctx), 1), text(id, textValue));
  state.textCount += 1;
  state.textBytes += textValue.length;
  return id;
}

function appendRowChildren(children, parentId, state, startX, startY, gap) {
  const list = Array.isArray(children) ? children : [];
  let x = startX;
  for (let i = 0; i < list.length; i += 1) {
    const child = list[i];
    if (!child) continue;
    if (child.kind === 'text') {
      const value = normalizeWhitespace(child.text);
      if (value.length === 0) continue;
      appendTextNode(value, parentId, state, x, startY + 6);
      x += Math.max(32, value.length * 8) + gap;
      continue;
    }
    const size = widgetSize(child);
    const itemLayout = { childX: x, nextY: startY };
    appendWidgetOps(child, parentId, state, itemLayout, 0);
    x += Math.max(0, size.w) + gap;
  }
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
    const style = controlSceneStyle();
    if (type === 'checkbox') {
      drawBox(state, id, size.w, size.h, style.background, style.border, style.radius);
      if (hasAttr(attrs, 'checked')) {
        state.ops.push(graphicsRect(id, 4, 4, Math.max(0, size.w - 8), Math.max(0, size.h - 8)), graphicsFill(id, style.accent, 1));
      } else if (hasAttr(attrs, 'indeterminate')) {
        state.ops.push(graphicsMoveTo(id, 4, 4), graphicsLineTo(id, Math.max(4, size.w - 4), Math.max(4, size.h - 4)), graphicsStroke(id, style.accent, 1, 2));
        state.ops.push(graphicsMoveTo(id, Math.max(4, size.w - 4), 4), graphicsLineTo(id, 4, Math.max(4, size.h - 4)), graphicsStroke(id, style.accent, 1, 2));
      }
    } else if (type === 'radio') {
      state.ops.push(graphicsClear(id), graphicsCircle(id, size.w / 2, size.h / 2, Math.min(size.w, size.h) / 2 - 1));
      state.ops.push(graphicsFill(id, style.background, 1), graphicsStroke(id, style.border, 1, 1));
      if (hasAttr(attrs, 'checked')) {
        state.ops.push(graphicsCircle(id, size.w / 2, size.h / 2, Math.min(size.w, size.h) / 2 - 5), graphicsFill(id, style.accent, 1));
      }
    } else {
      drawBox(state, id, size.w, size.h, style.background, style.border, style.radius);
    }
    return;
  }
  const temporalKind = temporalKindFromTag(tagName);
  if (temporalKind) {
    const style = temporalSceneStyle();
    drawBox(state, id, size.w, size.h, style.background, style.border, style.radius);
    if (temporalKind === 'datetime-local') {
      const sepX = Math.max(1, Math.round(size.w * 0.52));
      state.ops.push(graphicsMoveTo(id, sepX, 0), graphicsLineTo(id, sepX, size.h), graphicsStroke(id, style.border, 1, 1));
      state.ops.push(graphicsRect(id, Math.max(sepX, size.w - 30), 0, Math.max(0, size.w - Math.max(sepX, size.w - 30)), size.h), graphicsFill(id, style.insetFill, 1));
    } else {
      state.ops.push(graphicsRect(id, Math.max(0, size.w - 30), 0, 30, size.h), graphicsFill(id, style.insetFill, 1), graphicsStroke(id, THEME.border, 1, 1));
    }
    const chevronX = Math.max(0, size.w - 20);
    state.ops.push(graphicsMoveTo(id, chevronX, 14), graphicsLineTo(id, chevronX + 5, 20), graphicsLineTo(id, chevronX + 10, 14), graphicsStroke(id, style.text, 1, 2));
    return;
  }
  if (tagName === 'textarea' || tagName === 'select' || tagName === 'number') {
    const style = controlSceneStyle();
    drawBox(state, id, size.w, size.h, style.background, style.border, style.radius);
    if (tagName === 'select') {
      state.ops.push(graphicsRect(id, Math.max(0, size.w - 24), 0, 24, size.h), graphicsFill(id, style.insetFill, 1), graphicsStroke(id, THEME.border, 1, 1));
      state.ops.push(graphicsMoveTo(id, size.w - 21, 14), graphicsLineTo(id, size.w - 17, 20), graphicsLineTo(id, size.w - 13, 14), graphicsStroke(id, style.muted, 1, 2));
    }
    if (tagName === 'number') {
      state.ops.push(graphicsRect(id, size.w - 24, 0, 24, size.h), graphicsFill(id, style.insetFill, 1), graphicsStroke(id, THEME.border, 1, 1));
      state.ops.push(graphicsMoveTo(id, size.w - 20, 13), graphicsLineTo(id, size.w - 12, 7), graphicsLineTo(id, size.w - 4, 13), graphicsStroke(id, THEME.text, 1, 1));
      state.ops.push(graphicsMoveTo(id, size.w - 20, size.h - 13), graphicsLineTo(id, size.w - 12, size.h - 7), graphicsLineTo(id, size.w - 4, size.h - 13), graphicsStroke(id, THEME.text, 1, 1));
    }
    return;
  }
  if (tagName === 'progress' || tagName === 'meter' || tagName === 'slider') {
    const style = tagName === 'slider' ? sliderSceneStyle() : progressMeterSceneStyle();
    drawBox(state, id, size.w, size.h, style.background, style.border, 0);
    const pct = tagName === 'slider' ? sliderRatio(attrs) : progressMeterRatio(attrs, tagName);
    const pad = Number(style.innerPad ?? 3);
    const fillW = Math.max(0, Math.floor((size.w - pad * 2) * pct));
    state.ops.push(graphicsRect(id, pad, pad, fillW, Math.max(0, size.h - pad * 2)), graphicsFill(id, style.fill, 1));
    if (tagName === 'slider') {
      const knobX = pad + fillW;
      state.ops.push(graphicsMoveTo(id, knobX, -Math.max(3, size.h * 0.35)), graphicsLineTo(id, knobX, size.h + Math.max(3, size.h * 0.35)), graphicsStroke(id, style.indicator, 1, 2));
    }
    return;
  }
  if (tagName === 'color') {
    const style = colorSceneStyle();
    state.ops.push(graphicsClear(id));
    state.ops.push(graphicsRect(id, 0, 0, size.w, size.h), graphicsFill(id, style.background, 1), graphicsStroke(id, style.border, 1, 1));
    state.ops.push(graphicsRect(id, 12, 12, Math.max(0, size.w - 24), Math.max(0, size.h - 54)), graphicsFill(id, style.swatch, 1));
    state.ops.push(graphicsRect(id, 12, Math.max(12, size.h - 36), Math.max(0, size.w - 24), 12), graphicsFill(id, style.rail, 1));
    state.ops.push(graphicsRect(id, 12, Math.max(12, size.h - 20), Math.max(0, size.w - 24), 8), graphicsFill(id, style.railLight, 1), graphicsStroke(id, style.border, 1, 1));
    return;
  }
  if (tagName === 'img' || tagName === 'svg' || tagName === 'canvas') {
    const style = replacedElementSceneStyle();
    drawBox(state, id, size.w, size.h, style.background, style.border, 0);
    if (tagName === 'canvas') {
      state.ops.push(graphicsMoveTo(id, 8, size.h - 8), graphicsLineTo(id, size.w - 8, 8), graphicsStroke(id, style.guide, 1, 1));
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
  if (tagName === 'summary') {
    const style = detailsSummarySceneStyle(String(attrs['data-details-open'] ?? '') === '1' || detailsIsOpen(attrs));
    state.ops.push(graphicsClear(id), graphicsRect(id, 0, 0, Math.max(0, size.w), Math.max(0, size.h)), graphicsFill(id, 0xffffff, 0));
    const arrowY = Math.max(0, (size.h - style.arrowSize) / 2);
    const x0 = 4 + style.arrowPad;
    const x1 = 4 + style.arrowSize - style.arrowPad;
    const y0 = arrowY + style.arrowPad;
    const y1 = arrowY + style.arrowSize - style.arrowPad;
    if (style.arrowOpen) {
      state.ops.push(graphicsMoveTo(id, x0, y0), graphicsLineTo(id, (x0 + x1) / 2, y1), graphicsLineTo(id, x1, y0));
    } else {
      state.ops.push(graphicsMoveTo(id, x0, y0), graphicsLineTo(id, x1, (y0 + y1) / 2), graphicsLineTo(id, x0, y1));
    }
    state.ops.push(graphicsStroke(id, THEME.text, 1, style.arrowStroke));
    return;
  }
  if (tagName === 'table') {
    const style = tableSceneStyle();
    drawBox(state, id, size.w, size.h, style.background, style.border, 0);
    return;
  }
  if (tagName === 'td' || tagName === 'th') {
    const style = tableSceneStyle();
    drawBox(state, id, size.w, size.h, tagName === 'th' ? style.headerFill : style.background, style.cellBorder, 0);
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
    state.ops.push(textFill(id, textFillForContext(state.context), 1), text(id, value));
    state.textCount += 1;
    state.textBytes += value.length;
    layout.nextY += 24;
    return;
  }

  if (renderNode.kind !== 'block') return;
  const tagName = String(renderNode.tagName ?? 'block').toLowerCase();
  state.tagCounts[tagName] = (state.tagCounts[tagName] ?? 0) + 1;
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
    || tagName === 'td' || tagName === 'th' || tagName === 'summary' || temporalKindFromTag(tagName)
    || (tagName === 'iframe' && !iframeRoot);
  state.ops.push(node(id, draws ? KIND_GRAPHICS : KIND_CONTAINER), addChild(parentId, id), position(id, x, y));
  if (draws) drawWidgetChrome(tagName, renderNode, id, state, size);
  if (draws || INTERACTIVE_TAGS.has(tagName)) state.widgetCount += 1;
  if (INTERACTIVE_TAGS.has(tagName)) {
    state.ops.push(listen(id, 'pointerover'), listen(id, 'pointerout'), listen(id, 'pointerdown'), listen(id, 'pointermove'), listen(id, 'pointerup'));
  }
  if (tagName === 'button') {
    buttonLayoutDefaults();
    const style = buttonSceneStyle();
    blockAdvance = Math.max(blockAdvance, Number(style.height || 42) + 14);
    appendTextNode(collectInlineText(renderNode), id, state, 14, 10, { textFill: style.textFill });
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
  if (tagName === 'summary') childLayout = { childX: detailsSummarySceneStyle(false).textInset, nextY: 6 };
  if (tagName === 'td' || tagName === 'th') childLayout = { childX: 8, nextY: 6 };
  if (tagName === 'iframe' && !iframeRoot) childLayout = { childX: 8, nextY: 34 };
  const children = Array.isArray(renderNode.children) ? renderNode.children : [];
  const temporalKindForText = temporalKindFromTag(tagName);
  if ((tagName === 'input' || tagName === 'textarea') && String(renderNode.attrs?.type ?? 'text').toLowerCase() !== 'checkbox' && String(renderNode.attrs?.type ?? 'text').toLowerCase() !== 'radio') {
    appendTextNode(renderNode.attrs?.value ?? renderNode.attrs?.placeholder ?? '', id, state, 8, 8, { textFill: THEME.text });
  } else if (temporalKindForText === 'datetime-local') {
    const sepX = Math.max(1, Math.round(size.w * 0.52));
    appendTextNode(temporalDatePart(renderNode.attrs), id, state, 8, 8, { textFill: THEME.text });
    appendTextNode(temporalTimePart(renderNode.attrs), id, state, sepX + 8, 8, { textFill: THEME.text });
  } else if (temporalKindForText) {
    appendTextNode(temporalDisplayValue(renderNode.attrs, temporalKindForText), id, state, 8, 8, { textFill: THEME.text });
  } else if (tagName === 'select') {
    appendTextNode(selectedOptionLabel(renderNode.attrs), id, state, 8, 8, { textFill: THEME.text });
  } else if (tagName === 'number') {
    appendTextNode(renderNode.attrs?.value ?? '0', id, state, 8, 7, { textFill: THEME.text });
  } else if (tagName === 'sliderlabel') {
    appendTextNode(`${Math.round(sliderRatio({ value: renderNode.attrs?.['data-slider-init'] ?? 0 }) * 100)}`, id, state, 0, 2, { textFill: THEME.mutedText });
  } else if (tagName === 'img' || tagName === 'svg' || tagName === 'canvas') {
    appendTextNode(renderNode.attrs?.alt ?? tagName, id, state, 8, 8, { textFill: THEME.mutedText });
  } else if (tagName === 'iframe' && !iframeRoot) {
    appendTextNode(String(renderNode.attrs?.srcdoc ?? '').trim().length > 0 ? 'iframe (srcdoc)' : 'iframe (empty)', id, state, 8, 6, { textFill: THEME.mutedText });
  }
  if (ROW_TAGS.has(tagName)) {
    appendRowChildren(children, id, state, tagName === 'tr' ? 0 : 8, tagName === 'tr' ? 0 : 4, tagName === 'tr' ? 0 : 8);
  } else {
    for (let i = 0; i < children.length; i += 1) {
      if (tagName === 'button' && children[i]?.kind === 'text') continue;
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
    widgetCount: 0,
    tagCounts: {},
    textCount: 0,
    textBytes: 0,
  };
  const rootLayout = { childX: 0, nextY: 0 };
  for (let i = 0; i < renderTree.length; i += 1) appendWidgetOps(renderTree[i], ROOT_ID, state, rootLayout);
  const tagNames = Object.keys(state.tagCounts).sort();
  const tagCounts = tagNames.map((tag) => `${tag}:${state.tagCounts[tag]}`).join(',');

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
      detailsSummaryHelper: '/qjs/truesurfer/widgets/detailsSummary.mjs',
      formControlHelper: '/qjs/truesurfer/widgets/formControls.mjs',
      headingHelper: '/qjs/truesurfer/widgets/headings.mjs',
      iframeHelper: '/qjs/truesurfer/widgets/iframe.mjs',
      progressMeterHelper: '/qjs/truesurfer/widgets/progressMeter.mjs',
      sliderHelper: '/qjs/truesurfer/widgets/slider.mjs',
      colorHelper: '/qjs/truesurfer/widgets/color.mjs',
      replacedElementHelper: '/qjs/truesurfer/widgets/replacedElements.mjs',
      tableHelper: '/qjs/truesurfer/widgets/table.mjs',
      temporalHelper: '/qjs/truesurfer/widgets/temporal.mjs',
      renderer: 'parse5-render-tree-subset',
      tags: tagNames.join(','),
      tagCounts,
      iframeCount: state.iframeCount,
      iframeSrcdocCount: state.iframeSrcdocCount,
      buttonCount: state.buttonCount,
      widgetCount: state.widgetCount,
      textCount: state.textCount,
      textBytes: state.textBytes,
    },
  };
}

export const buildDemoTextWidgetScene = buildTextWidgetScene;
