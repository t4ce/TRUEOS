import { headingTextContext, isHeadingTag } from './widgets/headings.mjs';

const ROOT_ID = 1;
const ROOT_IFRAME_ID = 2;
const H1_BLOCK_ID = 3;
const H1_TEXT_ID = 4;
const P_BLOCK_ID = 5;
const P_TEXT_ID = 6;

const KIND_CONTAINER = 0;
const KIND_TEXT = 2;

const DEFAULT_H1 = 'Hello UI3';
const DEFAULT_P = 'Parse5 text widget baseline.';

function collapseWhitespace(value) {
  let out = '';
  let pending = false;
  const s = String(value ?? '');
  for (let i = 0; i < s.length; i += 1) {
    const c = s.charCodeAt(i);
    const ws = c === 32 || c === 9 || c === 10 || c === 13 || c === 12;
    if (ws) {
      pending = out.length > 0;
      continue;
    }
    if (pending) {
      out += ' ';
      pending = false;
    }
    out += s.charAt(i);
  }
  return out;
}

function stripTags(value) {
  let out = '';
  let inTag = false;
  const s = String(value ?? '');
  for (let i = 0; i < s.length; i += 1) {
    const ch = s.charAt(i);
    if (ch === '<') inTag = true;
    else if (ch === '>') inTag = false;
    else if (!inTag) out += ch;
  }
  return out;
}

function decodeBasicEntities(value) {
  return String(value ?? '')
    .replace(/&nbsp;/g, ' ')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&amp;/g, '&')
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'");
}

function extractTagText(html, tagName, fallback) {
  const source = String(html ?? '');
  const lower = source.toLowerCase();
  const tag = String(tagName || '').toLowerCase();
  let cursor = 0;
  while (cursor < lower.length) {
    const open = lower.indexOf(`<${tag}`, cursor);
    if (open < 0) break;
    const next = lower.charAt(open + tag.length + 1);
    if (next === '>' || next === '/' || /\s/.test(next)) {
      const openEnd = lower.indexOf('>', open);
      if (openEnd < 0) break;
      const close = lower.indexOf(`</${tag}>`, openEnd + 1);
      if (close < 0) break;
      const text = collapseWhitespace(decodeBasicEntities(stripTags(source.slice(openEnd + 1, close))));
      return text || fallback;
    }
    cursor = open + tag.length + 1;
  }
  return fallback;
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

function text(nodeId, value) {
  return { code: 8, node: nodeId, text: String(value ?? '') };
}

function textFill(nodeId, rgb, alpha = 1) {
  return { code: 9, node: nodeId, a: rgb >>> 0, b: alpha };
}

function textFillForContext(ctx) {
  return ctx && ctx.bold ? 0x111111 : 0x333333;
}

export function buildDemoTextWidgetScene(html) {
  const h1Tag = 'h1';
  const h1 = extractTagText(html, h1Tag, DEFAULT_H1);
  const p = extractTagText(html, 'p', DEFAULT_P);
  const h1Ctx = headingTextContext(h1Tag, {});
  const pCtx = {};
  const ops = [
    node(ROOT_IFRAME_ID, KIND_CONTAINER),
    node(H1_BLOCK_ID, KIND_CONTAINER),
    node(H1_TEXT_ID, KIND_TEXT),
    node(P_BLOCK_ID, KIND_CONTAINER),
    node(P_TEXT_ID, KIND_TEXT),

    addChild(ROOT_ID, ROOT_IFRAME_ID),
    addChild(ROOT_IFRAME_ID, H1_BLOCK_ID),
    addChild(H1_BLOCK_ID, H1_TEXT_ID),
    addChild(ROOT_IFRAME_ID, P_BLOCK_ID),
    addChild(P_BLOCK_ID, P_TEXT_ID),

    position(ROOT_IFRAME_ID, 32, 32),
    position(H1_BLOCK_ID, 0, 0),
    position(H1_TEXT_ID, 12, 6),
    position(P_BLOCK_ID, 0, 56),
    position(P_TEXT_ID, 12, 4),

    textFill(H1_TEXT_ID, textFillForContext(h1Ctx), 1),
    text(H1_TEXT_ID, h1),
    textFill(P_TEXT_ID, textFillForContext(pCtx), 1),
    text(P_TEXT_ID, p),
  ];

  return {
    ok: 1,
    ui3Scene: {
      version: 1,
      commandSource: 'qjs-widget-demo-text',
      rootId: ROOT_ID,
      opCount: ops.length,
      ops,
    },
    widget: {
      module: '/qjs/truesurfer/text_widget_scene.mjs',
      headingHelper: '/qjs/truesurfer/widgets/headings.mjs',
      h1IsHeading: isHeadingTag(h1Tag) ? 1 : 0,
      tags: 'h1,p',
      h1Bytes: h1.length,
      pBytes: p.length,
    },
  };
}

globalThis.__trueosBuildDemoTextWidgetScene = buildDemoTextWidgetScene;
