import { buttonLayoutDefaults, buttonSceneStyle } from './widgets/button.mjs';
import { headingTextContext, isHeadingTag } from './widgets/headings.mjs';
import { iframeContentOffset, iframeLayoutDefaults, iframeSceneOffset, isRootIframe } from './widgets/iframe.mjs';
import * as parse5 from 'parse5';

const ROOT_ID = 1;
const ROOT_IFRAME_ID = 2;

const KIND_CONTAINER = 0;
const KIND_TEXT = 2;

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
  if (s.indexOf('__trueos') >= 0) {
    s = s
      .replace(/__trueosNum/g, '')
      .replace(/__trueosNu/g, '')
      .replace(/__trueosN/g, '');
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

function toWidgetRenderTree(node, path = '0') {
  if (!isElement(node)) return [];

  const tagName = node.tagName ?? node.nodeName;
  const attrs = attrsToMap(node);
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

    children.push(...toWidgetRenderTree(child, `${path}.${i}`));
  }

  const tail = normalizeWhitespace(inlineText);
  if (tail.length > 0) children.push({ kind: 'text', text: tail });

  if (tagName === 'html' || tagName === 'body') return children;
  if (tagName === 'button' || tagName === 'h1' || tagName === 'p') {
    return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs, children }];
  }
  return children;
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

function graphicsFill(nodeId, rgb, alpha = 1) {
  return { code: 6, node: nodeId, a: rgb >>> 0, b: alpha };
}

function text(nodeId, value) {
  return { code: 8, node: nodeId, text: String(value ?? '') };
}

function textFill(nodeId, rgb, alpha = 1) {
  return { code: 9, node: nodeId, a: rgb >>> 0, b: alpha };
}

function textFillForContext(ctx) {
  if (ctx && typeof ctx.textFill === 'number') return ctx.textFill >>> 0;
  return ctx && ctx.bold ? 0x111111 : 0x333333;
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
  const iframeRoot = tagName === 'iframe' && isRootIframe(renderNode);
  const iframeOffset = tagName === 'iframe' ? iframeSceneOffset(renderNode) : null;
  const y = iframeRoot ? iframeOffset.y : layout.nextY;
  const x = iframeRoot ? iframeOffset.x : layout.childX;
  let blockAdvance = 56;
  let childLayout = { childX: 0, nextY: 0 };
  if (tagName === 'iframe') {
    const iframeDefaults = iframeLayoutDefaults(renderNode);
    const contentOffset = iframeContentOffset(renderNode);
    childLayout = { childX: contentOffset.x, nextY: contentOffset.y };
    blockAdvance = iframeRoot ? 0 : Math.max(56, Number(iframeDefaults.height ?? 240));
    state.iframeCount += 1;
    if (String(renderNode.attrs?.srcdoc ?? '').trim().length > 0) state.iframeSrcdocCount += 1;
  }
  state.ops.push(node(id, KIND_CONTAINER), addChild(parentId, id), position(id, x, y));
  if (tagName === 'button') {
    buttonLayoutDefaults();
    const style = buttonSceneStyle();
    state.ops.push(graphicsClear(id), graphicsRect(id, 0, 0, style.width, style.height), graphicsFill(id, style.fill, 1));
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
  if (tagName !== 'iframe') childLayout = { childX: 0, nextY: 0 };
  const children = Array.isArray(renderNode.children) ? renderNode.children : [];
  for (let i = 0; i < children.length; i += 1) {
    appendWidgetOps(children[i], id, state, childLayout, depth + 1);
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
      renderer: 'parse5-render-tree-subset',
      tags: 'iframe,h1,p,button',
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
