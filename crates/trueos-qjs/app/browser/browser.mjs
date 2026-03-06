import * as parse5 from 'parse5';
import Yoga from 'yoga-layout';

const G = (typeof globalThis !== 'undefined') ? globalThis : this;
const HTML = (typeof G.__trueosUiHtml === 'string' && G.__trueosUiHtml.length > 0)
  ? G.__trueosUiHtml
  : '<!DOCTYPE html><html><body><div>empty ui_html</div></body></html>';

const PAD = 8;
const NODE_H = 16;
const INDENT_PER_DEPTH = 10;
const MAX_INDENT = 80;

function isElement(node) {
  return !!node && typeof node === 'object' && typeof node.nodeName === 'string' && Array.isArray(node.childNodes);
}

function getBody(doc) {
  const html = (doc.childNodes || []).find((n) => isElement(n) && (n.tagName || n.nodeName) === 'html');
  if (!html) return null;
  return (html.childNodes || []).find((n) => isElement(n) && (n.tagName || n.nodeName) === 'body') || null;
}

function isStructuralTag(tag) {
  return tag === 'body' || tag === 'html' || tag === 'head' || tag === '#document' || tag === '#document-fragment';
}

function collectBlockTree(node, out) {
  if (!isElement(node)) return;
  const tag = String(node.tagName || node.nodeName || '').toLowerCase();
  const children = [];
  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i++) {
    collectBlockTree(kids[i], children);
  }

  if (isStructuralTag(tag)) {
    for (let i = 0; i < children.length; i++) out.push(children[i]);
    return;
  }

  out.push({ kind: 'block', tagName: tag, children });
}

function makeTreeFromHtml() {
  const doc = parse5.parse(HTML);
  const body = getBody(doc) || doc;
  const children = [];
  collectBlockTree(body, children);
  return { kind: 'block', tagName: 'root', children };
}

function blockChildren(node) {
  const kids = Array.isArray(node && node.children) ? node.children : [];
  return kids.filter((k) => k && k.kind === 'block');
}

function isScrollableTag(tag) {
  return tag === 'scrollable';
}

function makeYogaTree(node, allBlocks, depth = 0) {
  const yn = Yoga.Node.create();
  yn.setFlexDirection(Yoga.FLEX_DIRECTION_COLUMN);
  yn.setAlignItems(Yoga.ALIGN_STRETCH);
  if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_STRETCH);
  if (typeof yn.setWidthPercent === 'function') yn.setWidthPercent(100);
  yn.setPadding(Yoga.EDGE_LEFT, depth === 0 ? PAD : 0);
  yn.setPadding(Yoga.EDGE_RIGHT, depth === 0 ? PAD : 0);
  yn.setPadding(Yoga.EDGE_TOP, 0);
  yn.setPadding(Yoga.EDGE_BOTTOM, 0);
  yn.setMinHeight(NODE_H);

  allBlocks.push({ node, yoga: yn, depth });

  const kids = blockChildren(node);
  if (kids.length <= 0) {
    yn.setHeight(NODE_H);
    return yn;
  }

  for (let i = 0; i < kids.length; i++) {
    const child = makeYogaTree(kids[i], allBlocks, depth + 1);
    if (i > 0 && typeof child.setMargin === 'function') child.setMargin(Yoga.EDGE_TOP, 1);
    yn.insertChild(child, yn.getChildCount());
  }
  return yn;
}

function computeRects(blocks) {
  const ordered = blocks;
  const out = [];
  const absXByDepth = [];
  const absYByDepth = [];
  for (let i = 0; i < ordered.length; i++) {
    const entry = ordered[i];
    const tag = String(entry && entry.node && entry.node.tagName || '').toLowerCase();
    const depth = Math.max(0, Number(entry.depth || 0));
    const yn = entry.yoga;
    const localX = Number(yn.getComputedLeft() || 0);
    const localY = Number(yn.getComputedTop() || 0);
    const parentX = depth > 0 ? Number(absXByDepth[depth - 1] || 0) : 0;
    const parentY = depth > 0 ? Number(absYByDepth[depth - 1] || 0) : 0;
    const absX = parentX + localX;
    const absY = parentY + localY;
    absXByDepth[depth] = absX;
    absYByDepth[depth] = absY;

    if (tag === 'root') continue;

    const indent = Math.min(MAX_INDENT, depth * INDENT_PER_DEPTH);
    const x = absX + indent;
    const y = absY;
    const w = Math.max(2, Number(yn.getComputedWidth() || 0) - indent);
    const h = Math.max(2, Number(yn.getComputedHeight() || 0));
    const scrollable = isScrollableTag(tag) ? 1 : 0;
    out.push(x, y, w, h, depth, scrollable);
  }
  return out;
}

const logicalRoot = makeTreeFromHtml();
const blocks = [];
const yogaRoot = makeYogaTree(logicalRoot, blocks, 0);

function applyViewportConstraints(vw, vh) {
  if (typeof yogaRoot.setWidth === 'function') yogaRoot.setWidth(vw);
  if (typeof yogaRoot.setHeight === 'function') yogaRoot.setHeight(vh);

  for (let i = 0; i < blocks.length; i++) {
    const entry = blocks[i];
    if (!entry || !entry.yoga) continue;
    if ((entry.node && entry.node.tagName) === 'root') continue;
    const depth = Math.max(0, Number(entry.depth || 0));
    const indent = Math.min(MAX_INDENT, depth * INDENT_PER_DEPTH);
    const width = Math.max(2, vw - (PAD * 2) - indent);
    if (typeof entry.yoga.setWidth === 'function') entry.yoga.setWidth(width);
  }
}

function render() {
  const G = globalThis;
  const W = G.window || G;
  const vw = Math.max(1, Number(W.innerWidth || 1280));
  const vh = Math.max(1, Number(W.innerHeight || 800));
  applyViewportConstraints(vw, vh);
  yogaRoot.calculateLayout(vw, vh, Yoga.DIRECTION_LTR);
  const rects = computeRects(blocks);
  if (typeof G.__trueosDrawLayoutRects === 'function') {
    G.__trueosDrawLayoutRects(rects, vw, vh);
  }
}

render();
if (typeof (globalThis.window || globalThis).addEventListener === 'function') {
  (globalThis.window || globalThis).addEventListener('resize', render);
}
if (typeof globalThis.setInterval === 'function') {
  globalThis.setInterval(render, 33);
}

try {
  console.log(`[browser.mjs] parse5+yoga online blocks=${blocks.length}`);
} catch (_) {}
