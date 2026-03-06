import * as parse5 from 'parse5';
import Yoga from 'yoga-layout';
import { renderHtmlAppWindowWidget } from './widgets/htmlAppWindowWidget.mjs';
import { renderScrollbarWidget } from './widgets/scrollbarWidget.mjs';
import { renderCheckboxWidget } from './widgets/checkboxWidget.mjs';
import { renderSummaryWidget } from './widgets/summaryWidget.mjs';
import { renderDialogWidget } from './widgets/dialogWidget.mjs';
import { renderButtonWidget } from './widgets/buttonWidget.mjs';
import { renderSvgWidget } from './widgets/svgWidget.mjs';
import { renderTableWidget } from './widgets/tableWidget.mjs';
import { renderFormWidget } from './widgets/formWidget.mjs';
import { renderRadioWidget } from './widgets/radioWidget.mjs';
import {
  applyYogaDefaultsTemporalInput,
  isTemporalTag,
  renderTemporalWidget,
  temporalDisplayText,
  temporalTagForInputType,
} from './widgets/tempo.mjs';
import {
  applyYogaDefaultsRangeWidget,
  isRangeWidgetTag,
  rangeWidgetTagForInputType,
  renderRangeWidget,
} from './widgets/slider.mjs';
import { Worker } from 'node:worker_threads';

const G = (typeof globalThis !== 'undefined') ? globalThis : this;
const HTML = G.__trueosUiHtml;
const HTML_APP_WINDOW_ROOT_ID = 'root/html_app_window[0]';
// Tags that emit inline text runs through the native text lane.
const INLINE_TEXT_TAGS = new Set([
  'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
  'p', 'button',
  'timeinput', 'dateinput', 'monthinput', 'weekinput', 'datetimelocalinput',
]);
const HEADING_TEXT_TAGS = new Set(['h1', 'h2', 'h3', 'h4', 'h5', 'h6']);
const blockNodeById = new Map();
const detailsOpenById = new Map();
const svgAssetIdByBlockId = new Map();
const iframeRenderByBlockId = new Map();
const svgImportWorker = {
  worker: null,
  started: false,
  nextReqId: 1,
  pending: new Map(),
};
const iframeImportWorker = {
  worker: null,
  started: false,
  nextReqId: 1,
  pending: new Map(),
};

function htmlAppWindowMinWidthPx() {
  return Math.max(1, Number(G.__trueosThemeHtmlAppWindowMinW || 16));
}

function isInHtmlAppWindowSubtree(nodeId) {
  const id = String(nodeId || '');
  return id === HTML_APP_WINDOW_ROOT_ID || id.startsWith(`${HTML_APP_WINDOW_ROOT_ID}/`);
}

function htmlAppWindowAncestorId(nodeId) {
  const id = String(nodeId || '');
  if (!id) return '';
  const parts = id.split('/');
  for (let i = parts.length - 1; i >= 0; i--) {
    if (!parts[i].startsWith('html_app_window[')) continue;
    const ancestor = parts.slice(0, i + 1).join('/');
    if (ancestor) return ancestor;
  }
  return '';
}

function isDialogOverlayNode(nodeId, tag) {
  return String(tag || '').toLowerCase() === 'dialog' && isInHtmlAppWindowSubtree(nodeId);
}

function isDialogSubtreeId(nodeId) {
  const id = String(nodeId || '');
  return id.includes('/dialog[');
}

function isElement(node) {
  return !!node && typeof node === 'object' && typeof node.nodeName === 'string' && Array.isArray(node.childNodes);
}

function isTextNode(node) {
  return !!node && typeof node === 'object' && node.nodeName === '#text' && typeof node.value === 'string';
}

function collapseInlineWhitespace(s) {
  return String(s || '').replace(/\s+/g, ' ').trim();
}

function collectNodeText(node, out) {
  if (!node) return;
  if (isTextNode(node)) {
    if (node.value) out.push(node.value);
    return;
  }
  if (!isElement(node)) return;
  const tag = String(node.tagName || node.nodeName || '').toLowerCase();
  if (tag === 'br' || tag === 'wbr') {
    out.push(' ');
    return;
  }
  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i++) collectNodeText(kids[i], out);
}

function nodeTextPreview(node, maxChars = 120) {
  const parts = [];
  collectNodeText(node, parts);
  const joined = collapseInlineWhitespace(parts.join(''));
  if (!joined) return '';
  if (joined.length <= maxChars) return joined;
  return `${joined.slice(0, Math.max(0, maxChars - 3))}...`;
}

function nodeTextContent(node) {
  const parts = [];
  collectNodeText(node, parts);
  return collapseInlineWhitespace(parts.join(''));
}

function checkboxLabelText(inputNode) {
  if (!inputNode || !inputNode.parentNode) return '';
  // For `<summary><input ...> Label</summary>` and `<label><input ...> Label</label>`,
  // use the parent's text content and skip tiny placeholders.
  return nodeTextPreview(inputNode.parentNode, 120);
}

function summaryLabelText(summaryNode) {
  if (!summaryNode) return '';
  return nodeTextPreview(summaryNode, 120);
}

function buttonLabelText(buttonNode) {
  if (!buttonNode) return '';
  return nodeTextPreview(buttonNode, 160);
}

function radioLabelText(inputNode) {
  if (!inputNode || !inputNode.parentNode) return '';
  return nodeTextPreview(inputNode.parentNode, 120);
}

function getBody(doc) {
  const html = (doc.childNodes || []).find((n) => isElement(n) && (n.tagName || n.nodeName) === 'html');
  if (!html) return null;
  return (html.childNodes || []).find((n) => isElement(n) && (n.tagName || n.nodeName) === 'body') || null;
}

function isStructuralTag(tag) {
  return tag === 'body' || tag === 'html' || tag === 'head' || tag === '#document' || tag === '#document-fragment'
    || tag === 'tr' || tag === 'td' || tag === 'th' || tag === 'thead' || tag === 'tbody' || tag === 'tfoot'
    || tag === 'rect' || tag === 'circle' || tag === 'path' || tag === 'line'
    || tag === 'polyline' || tag === 'polygon' || tag === 'g';
}

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

function parseViewBox(svgNode) {
  const vb = String(getAttr(svgNode, 'viewBox') || '').trim();
  const wAttr = Number(getAttr(svgNode, 'width'));
  const hAttr = Number(getAttr(svgNode, 'height'));
  if (vb) {
    const vals = vb.split(/[\s,]+/).map((x) => Number(x)).filter((x) => Number.isFinite(x));
    if (vals.length >= 4 && vals[2] > 0 && vals[3] > 0) {
      return {
        minX: vals[0],
        minY: vals[1],
        width: vals[2],
        height: vals[3],
        outW: Number.isFinite(wAttr) && wAttr > 1 ? Math.round(wAttr) : Math.round(vals[2]),
        outH: Number.isFinite(hAttr) && hAttr > 1 ? Math.round(hAttr) : Math.round(vals[3]),
      };
    }
  }
  return {
    minX: 0,
    minY: 0,
    width: Number.isFinite(wAttr) && wAttr > 1 ? wAttr : 64,
    height: Number.isFinite(hAttr) && hAttr > 1 ? hAttr : 64,
    outW: Number.isFinite(wAttr) && wAttr > 1 ? Math.round(wAttr) : 64,
    outH: Number.isFinite(hAttr) && hAttr > 1 ? Math.round(hAttr) : 64,
  };
}

function hash32(str) {
  let h = 0x811c9dc5 >>> 0;
  const s = String(str || '');
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i) & 0xff;
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h >>> 0;
}

const SVG_IMPORT_WORKER_CODE = String.raw`
import { parentPort } from 'node:worker_threads';

function clamp01(n) {
  const x = Number(n);
  if (!Number.isFinite(x) || x <= 0) return 0;
  if (x >= 1) return 1;
  return x;
}

function parseColor(value) {
  const v = String(value || '').trim().toLowerCase();
  if (!v || v === 'none' || v === 'transparent') return 0;
  if (v.startsWith('#')) {
    const hex = v.slice(1);
    if (/^[0-9a-f]{6}$/.test(hex)) {
      const n = Number.parseInt(hex, 16) >>> 0;
      return ((0xFF << 24) | n) >>> 0;
    }
    if (/^[0-9a-f]{3}$/.test(hex)) {
      const r = Number.parseInt(hex[0] + hex[0], 16);
      const g = Number.parseInt(hex[1] + hex[1], 16);
      const b = Number.parseInt(hex[2] + hex[2], 16);
      return ((0xFF << 24) | (r << 16) | (g << 8) | b) >>> 0;
    }
  }
  return 0;
}

function parsePathPoints(d) {
  const s = String(d || '').trim();
  if (!s) return [];
  const tokens = s.match(/[MLml]|-?\d+(?:\.\d+)?/g) || [];
  const pts = [];
  let i = 0;
  let mode = '';
  let cx = 0;
  let cy = 0;
  while (i < tokens.length) {
    const t = tokens[i++];
    if (t === 'M' || t === 'L' || t === 'm' || t === 'l') {
      mode = t;
      continue;
    }
    const x = Number(t);
    if (!Number.isFinite(x) || i >= tokens.length) break;
    const y = Number(tokens[i++]);
    if (!Number.isFinite(y)) break;
    if (mode === 'm' || mode === 'l') {
      cx += x;
      cy += y;
    } else {
      cx = x;
      cy = y;
    }
    pts.push({ x: cx, y: cy });
  }
  return pts;
}

function normX(vb, x) {
  return clamp01((Number(x) - vb.minX) / Math.max(1e-6, vb.width));
}

function normY(vb, y) {
  return clamp01((Number(y) - vb.minY) / Math.max(1e-6, vb.height));
}

function normR(vb, r) {
  const scale = Math.max(1e-6, Math.min(vb.width, vb.height));
  return clamp01(Number(r) / scale);
}

function post(msg) {
  parentPort.postMessage(JSON.stringify(msg));
}

function buildCmds(vb, shapes) {
  const cmds = [];
  for (let i = 0; i < shapes.length; i++) {
    const c = shapes[i] || {};
    const t = String(c.tag || '').toLowerCase();
    if (t === 'rect') {
      const x = normX(vb, Number(c.x || 0));
      const y = normY(vb, Number(c.y || 0));
      const w = clamp01(Math.max(0, Number(c.width || 0) / Math.max(1e-6, vb.width)));
      const h = clamp01(Math.max(0, Number(c.height || 0) / Math.max(1e-6, vb.height)));
      const fill = parseColor(c.fill);
      const stroke = parseColor(c.stroke);
      const sw = Math.max(0, Number(c.strokeWidth || 0));
      cmds.push(1, x, y, w, h, fill >>> 0, stroke >>> 0, sw);
      continue;
    }

    if (t === 'circle') {
      const cx = normX(vb, Number(c.cx || 0));
      const cy = normY(vb, Number(c.cy || 0));
      const r = normR(vb, Number(c.r || 0));
      const fill = parseColor(c.fill);
      const stroke = parseColor(c.stroke);
      const sw = Math.max(0, Number(c.strokeWidth || 0));
      cmds.push(2, cx, cy, r, fill >>> 0, stroke >>> 0, sw);
      continue;
    }

    if (t === 'path') {
      const pts = parsePathPoints(c.d);
      const stroke = parseColor(c.stroke);
      const sw = Math.max(0, Number(c.strokeWidth || 0));
      if (pts.length >= 2 && stroke !== 0) {
        cmds.push(3, pts.length);
        for (let p = 0; p < pts.length; p++) {
          cmds.push(normX(vb, pts[p].x), normY(vb, pts[p].y));
        }
        cmds.push(stroke >>> 0, sw);
      }
    }
  }
  return cmds;
}

parentPort.onMessage((raw) => {
  let msg = null;
  try {
    msg = JSON.parse(String(raw || ''));
  } catch (_) {
    return;
  }
  if (!msg || msg.type !== 'build') return;

  const reqId = Number(msg.reqId || 0) | 0;
  const vbIn = msg.vb || {};
  const vb = {
    minX: Number(vbIn.minX || 0),
    minY: Number(vbIn.minY || 0),
    width: Math.max(1e-6, Number(vbIn.width || 64)),
    height: Math.max(1e-6, Number(vbIn.height || 64)),
  };
  const shapes = Array.isArray(msg.shapes) ? msg.shapes : [];
  const cmds = buildCmds(vb, shapes);
  post({ type: 'built', reqId, cmds });
});

post({ type: 'ready' });
`;

const IFRAME_IMPORT_WORKER_CODE = String.raw`
import * as parse5 from '/qjs/vendor/parse5.mjs';
import Yoga from '/qjs/vendor/yoga.mjs';
import { parentPort } from 'node:worker_threads';

function isElement(node) {
  return !!node && typeof node === 'object' && typeof node.nodeName === 'string' && Array.isArray(node.childNodes);
}

function isTextNode(node) {
  return !!node && typeof node === 'object' && node.nodeName === '#text' && typeof node.value === 'string';
}

function collapseInlineWhitespace(s) {
  return String(s || '').replace(/\s+/g, ' ').trim();
}

function getBody(doc) {
  const html = (doc.childNodes || []).find((n) => isElement(n) && (n.tagName || n.nodeName) === 'html');
  if (!html) return null;
  return (html.childNodes || []).find((n) => isElement(n) && (n.tagName || n.nodeName) === 'body') || null;
}

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

function isStructuralTag(tag) {
  return tag === 'body' || tag === 'html' || tag === 'head' || tag === '#document' || tag === '#document-fragment'
    || tag === 'rect' || tag === 'circle' || tag === 'path' || tag === 'line'
    || tag === 'polyline' || tag === 'polygon' || tag === 'g';
}

function classifyTag(node, rawTag) {
  const tag = String(rawTag || '').toLowerCase();
  if (tag === 'iframe') return 'html_app_window';
  if (tag === 'input') {
    const t = String(getAttr(node, 'type') || '').toLowerCase();
    if (t === 'checkbox') return 'checkbox';
    if (t === 'radio') return 'radio';
    if (t === 'button' || t === 'submit' || t === 'reset') return 'button';
  }
  return tag;
}

function collectBlockTree(node, out) {
  if (!isElement(node)) return;
  const rawTag = String(node.tagName || node.nodeName || '').toLowerCase();
  const tag = classifyTag(node, rawTag);
  const children = [];
  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i++) collectBlockTree(kids[i], children);
  if (isStructuralTag(tag)) {
    for (let i = 0; i < children.length; i++) out.push(children[i]);
    return;
  }
  out.push({ kind: 'block', tagName: tag, children, id: '', srcNode: node });
}

function blockChildren(node) {
  const kids = Array.isArray(node && node.children) ? node.children : [];
  return kids.filter((k) => k && k.kind === 'block');
}

function assignBlockIds(node, path) {
  if (!node || node.kind !== 'block') return;
  node.id = path;
  const kids = blockChildren(node);
  for (let i = 0; i < kids.length; i++) {
    const child = kids[i];
    const childTag = String(child.tagName || child.nodeName || 'block').toLowerCase();
    assignBlockIds(child, path + '/' + childTag + '[' + i + ']');
  }
}

function collectNodeText(node, out) {
  if (!node) return;
  if (isTextNode(node)) {
    if (node.value) out.push(node.value);
    return;
  }
  if (!isElement(node)) return;
  const tag = String(node.tagName || node.nodeName || '').toLowerCase();
  if (tag === 'br' || tag === 'wbr') {
    out.push(' ');
    return;
  }
  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i++) collectNodeText(kids[i], out);
}

function nodeTextContent(node) {
  const parts = [];
  collectNodeText(node, parts);
  return collapseInlineWhitespace(parts.join(''));
}

function nodeTextPreview(node, maxChars = 96) {
  const joined = nodeTextContent(node);
  if (!joined) return '';
  if (joined.length <= maxChars) return joined;
  return joined.slice(0, Math.max(0, maxChars - 3)) + '...';
}

function makeYogaTree(node, allBlocks, depth, themeNodeH) {
  const tag = String(node && node.tagName || '').toLowerCase();
  const yn = Yoga.Node.create();
  const minNodeSize = Math.max(1, Number(themeNodeH || 16));
  yn.setFlexDirection(Yoga.FLEX_DIRECTION_COLUMN);
  yn.setAlignItems(Yoga.ALIGN_STRETCH);
  if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_STRETCH);
  if (depth > 0 && typeof yn.setWidthPercent === 'function') yn.setWidthPercent(100);
  yn.setMinHeight(minNodeSize);
  yn.setPadding(Yoga.EDGE_LEFT, 0);
  yn.setPadding(Yoga.EDGE_RIGHT, 0);
  yn.setPadding(Yoga.EDGE_TOP, 0);
  yn.setPadding(Yoga.EDGE_BOTTOM, 0);

  if (tag === 'checkbox' || tag === 'radio') {
    const box = Math.max(8, Math.min(14, minNodeSize - 2));
    if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
    if (typeof yn.setWidth === 'function') yn.setWidth(box);
    if (typeof yn.setHeight === 'function') yn.setHeight(box);
    if (typeof yn.setMinWidth === 'function') yn.setMinWidth(box);
    if (typeof yn.setMinHeight === 'function') yn.setMinHeight(box);
  }
  if (tag === 'button') {
    const txt = nodeTextPreview(node && node.srcNode ? node.srcNode : null, 80);
    const btnW = Math.max(32, txt.length * 8 + 24);
    const btnH = Math.max(minNodeSize + 4, 16);
    if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
    if (typeof yn.setWidth === 'function') yn.setWidth(btnW);
    if (typeof yn.setHeight === 'function') yn.setHeight(btnH);
    if (typeof yn.setMinWidth === 'function') yn.setMinWidth(btnW);
    if (typeof yn.setMinHeight === 'function') yn.setMinHeight(btnH);
  }

  allBlocks.push({ node, yoga: yn, depth: depth });
  const kids = blockChildren(node);
  if (kids.length <= 0) {
    yn.setHeight(Math.max(1, minNodeSize));
    return yn;
  }
  for (let i = 0; i < kids.length; i++) {
    const child = makeYogaTree(kids[i], allBlocks, depth + 1, themeNodeH);
    if (i > 0 && typeof child.setMargin === 'function') child.setMargin(Yoga.EDGE_TOP, 1);
    yn.insertChild(child, yn.getChildCount());
  }
  return yn;
}

function computeRects(blocks) {
  const out = [];
  const entries = [];
  const absXByDepth = [];
  const absYByDepth = [];
  for (let i = 0; i < blocks.length; i++) {
    const e = blocks[i];
    const d = Math.max(0, Number(e.depth || 0));
    const yn = e.yoga;
    const x = Number(yn.getComputedLeft() || 0) + (d > 0 ? Number(absXByDepth[d - 1] || 0) : 0);
    const y = Number(yn.getComputedTop() || 0) + (d > 0 ? Number(absYByDepth[d - 1] || 0) : 0);
    absXByDepth[d] = x;
    absYByDepth[d] = y;
    const tag = String(e.node && e.node.tagName || '').toLowerCase();
    if (tag === 'root') continue;
    const w = Math.max(2, Number(yn.getComputedWidth() || 0));
    const h = Math.max(2, Number(yn.getComputedHeight() || 0));
    const id = String(e.node && e.node.id || '');
    out.push(x, y, w, h, d, 0, 0);
    entries.push({ id, tag, x, y, w, h, depth: d });
  }
  return { packed: out, entries };
}

function collectTextRuns(entries, idToNode) {
  const out = [];
  for (let i = 0; i < entries.length; i++) {
    const r = entries[i];
    const tag = String(r.tag || '').toLowerCase();
    if (!(tag === 'h1' || tag === 'h2' || tag === 'h3' || tag === 'h4' || tag === 'h5' || tag === 'h6' || tag === 'p' || tag === 'button')) continue;
    const n = idToNode.get(String(r.id || ''));
    if (!n || !n.srcNode) continue;
    const t = nodeTextPreview(n.srcNode, 96);
    if (!t) continue;
    out.push(Math.round(Number(r.x || 0) + (tag === 'button' ? 8 : 2)), Math.round(Number(r.y || 0) + 2), t);
  }
  return out;
}

function buildLayoutFromHtml(html, viewportW, viewportH, themeNodeH) {
  const doc = parse5.parse(String(html || ''));
  const body = getBody(doc) || doc;
  const appChildren = [];
  collectBlockTree(body, appChildren);
  const root = {
    kind: 'block',
    tagName: 'root',
    children: [{ kind: 'block', tagName: 'html_app_window', children: appChildren, id: '' }],
    id: 'root',
  };
  assignBlockIds(root, 'root');

  const blocks = [];
  const yogaRoot = makeYogaTree(root, blocks, 0, themeNodeH);
  const vw = Math.max(1, Number(viewportW || 320));
  const vh = Math.max(1, Number(viewportH || 240));
  if (typeof yogaRoot.setWidth === 'function') yogaRoot.setWidth(vw);
  if (typeof yogaRoot.setHeight === 'function') yogaRoot.setHeight(vh);
  yogaRoot.calculateLayout(vw, vh, Yoga.DIRECTION_LTR);
  const rects = computeRects(blocks);

  const idToNode = new Map();
  for (let i = 0; i < blocks.length; i++) {
    const n = blocks[i] && blocks[i].node;
    if (!n) continue;
    idToNode.set(String(n.id || ''), n);
  }
  const texts = collectTextRuns(rects.entries, idToNode);
  return { packed: rects.packed, texts, viewportW: vw, viewportH: vh };
}

function post(msg) {
  parentPort.postMessage(JSON.stringify(msg));
}

parentPort.onMessage((raw) => {
  let msg = null;
  try {
    msg = JSON.parse(String(raw || ''));
  } catch (_) {
    return;
  }
  if (!msg || msg.type !== 'parse') return;

  const reqId = Number(msg.reqId || 0) | 0;
  const blockId = String(msg.blockId || '');
  const html = String(msg.html || '');
  const viewportW = Math.max(1, Number(msg.viewportW || 320));
  const viewportH = Math.max(1, Number(msg.viewportH || 240));
  const themeNodeH = Math.max(1, Number(msg.themeNodeH || 16));
  if (!reqId || !blockId || !html) {
    post({ type: 'parsed', reqId, blockId, packed: [], texts: [], viewportW, viewportH });
    return;
  }

  try {
    const out = buildLayoutFromHtml(html, viewportW, viewportH, themeNodeH);
    post({ type: 'parsed', reqId, blockId, packed: out.packed, texts: out.texts, viewportW: out.viewportW, viewportH: out.viewportH });
  } catch (_) {
    post({ type: 'parsed', reqId, blockId, packed: [], texts: [], viewportW, viewportH });
  }
});

post({ type: 'ready' });
`;

function onSvgWorkerMessage(raw) {
  let msg = null;
  try {
    msg = JSON.parse(String(raw || ''));
  } catch (_) {
    return;
  }
  if (!msg || typeof msg !== 'object') return;

  if (msg.type === 'ready') return;

  if (msg.type !== 'built') return;
  const reqId = Number(msg.reqId || 0) | 0;
  if (!reqId) return;

  const pending = svgImportWorker.pending.get(reqId);
  if (!pending) return;
  svgImportWorker.pending.delete(reqId);

  const importFn = G.__trueosImportSvgAsset;
  if (typeof importFn !== 'function') return;
  const cmds = Array.isArray(msg.cmds) ? msg.cmds : [];
  if (cmds.length <= 0) return;

  const ok = Number(importFn(pending.assetId, pending.outW, pending.outH, cmds) || 0) >= 0.5;
  if (!ok) return;
  svgAssetIdByBlockId.set(pending.blockId, pending.assetId);
  relayoutAndPaint();
}

function ensureSvgImportWorker() {
  if (svgImportWorker.started) return !!svgImportWorker.worker;
  svgImportWorker.started = true;
  if (typeof Worker !== 'function') return false;
  try {
    const w = new Worker(SVG_IMPORT_WORKER_CODE);
    svgImportWorker.worker = w;
    if (typeof w.onMessage === 'function') w.onMessage(onSvgWorkerMessage);
    return true;
  } catch (_) {
    svgImportWorker.worker = null;
    return false;
  }
}

function requestSvgBuildAsync(blockId, assetId, vb, outW, outH, shapes) {
  if (!ensureSvgImportWorker()) return false;
  const w = svgImportWorker.worker;
  if (!w || typeof w.postMessage !== 'function') return false;

  const reqId = (svgImportWorker.nextReqId++) | 0;
  if (!reqId) return false;
  svgImportWorker.pending.set(reqId, {
    blockId: String(blockId || ''),
    assetId: Number(assetId || 0) >>> 0,
    outW: Math.max(1, Number(outW || 1) | 0),
    outH: Math.max(1, Number(outH || 1) | 0),
  });

  try {
    w.postMessage(JSON.stringify({ type: 'build', reqId, vb, shapes }));
    return true;
  } catch (_) {
    svgImportWorker.pending.delete(reqId);
    return false;
  }
}

function onIframeWorkerMessage(raw) {
  let msg = null;
  try {
    msg = JSON.parse(String(raw || ''));
  } catch (_) {
    return;
  }
  if (!msg || typeof msg !== 'object') return;
  if (msg.type === 'ready') return;
  if (msg.type !== 'parsed') return;

  const reqId = Number(msg.reqId || 0) | 0;
  if (!reqId) return;
  const pending = iframeImportWorker.pending.get(reqId);
  if (!pending) return;
  iframeImportWorker.pending.delete(reqId);

  const packed = Array.isArray(msg.packed) ? msg.packed.map((v) => Number(v || 0)) : [];
  const texts = Array.isArray(msg.texts) ? msg.texts : [];
  iframeRenderByBlockId.set(pending.blockId, {
    packed,
    texts,
    viewportW: Math.max(1, Number(msg.viewportW || pending.viewportW || 320)),
    viewportH: Math.max(1, Number(msg.viewportH || pending.viewportH || 240)),
  });
  relayoutAndPaint();
}

function ensureIframeImportWorker() {
  if (iframeImportWorker.started) return !!iframeImportWorker.worker;
  iframeImportWorker.started = true;
  if (typeof Worker !== 'function') return false;
  try {
    const w = new Worker(IFRAME_IMPORT_WORKER_CODE);
    iframeImportWorker.worker = w;
    if (typeof w.onMessage === 'function') w.onMessage(onIframeWorkerMessage);
    return true;
  } catch (_) {
    iframeImportWorker.worker = null;
    return false;
  }
}

function requestIframeParseAsync(blockId, srcdocHtml, viewportW, viewportH) {
  if (!ensureIframeImportWorker()) return false;
  const w = iframeImportWorker.worker;
  if (!w || typeof w.postMessage !== 'function') return false;

  const reqId = (iframeImportWorker.nextReqId++) | 0;
  if (!reqId) return false;
  const id = String(blockId || '');
  if (!id) return false;
  iframeImportWorker.pending.set(reqId, {
    blockId: id,
    viewportW: Math.max(1, Number(viewportW || 320)),
    viewportH: Math.max(1, Number(viewportH || 240)),
  });

  try {
    w.postMessage(JSON.stringify({
      type: 'parse',
      reqId,
      blockId: id,
      html: String(srcdocHtml || ''),
      viewportW: Math.max(1, Number(viewportW || 320)),
      viewportH: Math.max(1, Number(viewportH || 240)),
      themeNodeH: Math.max(1, Number(G.__trueosThemeNodeH || 16)),
    }));
    return true;
  } catch (_) {
    iframeImportWorker.pending.delete(reqId);
    return false;
  }
}

function listSvgShapeInputs(svgNode) {
  const shapes = [];
  const shapeKids = Array.isArray(svgNode && svgNode.childNodes) ? svgNode.childNodes : [];
  for (let i = 0; i < shapeKids.length; i++) {
    const c = shapeKids[i];
    if (!c || !c.tagName) continue;
    const t = String(c.tagName || c.nodeName || '').toLowerCase();
    if (t === 'rect') {
      shapes.push({
        tag: 'rect',
        x: Number(getAttr(c, 'x') || 0),
        y: Number(getAttr(c, 'y') || 0),
        width: Number(getAttr(c, 'width') || 0),
        height: Number(getAttr(c, 'height') || 0),
        fill: getAttr(c, 'fill'),
        stroke: getAttr(c, 'stroke'),
        strokeWidth: Number(getAttr(c, 'stroke-width') || 0),
      });
      continue;
    }
    if (t === 'circle') {
      shapes.push({
        tag: 'circle',
        cx: Number(getAttr(c, 'cx') || 0),
        cy: Number(getAttr(c, 'cy') || 0),
        r: Number(getAttr(c, 'r') || 0),
        fill: getAttr(c, 'fill'),
        stroke: getAttr(c, 'stroke'),
        strokeWidth: Number(getAttr(c, 'stroke-width') || 0),
      });
      continue;
    }
    if (t === 'path') {
      shapes.push({
        tag: 'path',
        d: getAttr(c, 'd'),
        stroke: getAttr(c, 'stroke'),
        strokeWidth: Number(getAttr(c, 'stroke-width') || 0),
      });
    }
  }
  return shapes;
}

function importSvgAssetsOnce(root) {
  svgAssetIdByBlockId.clear();
  svgImportWorker.pending.clear();
  const importFn = G.__trueosImportSvgAsset;
  if (typeof importFn !== 'function') return;
  if (!ensureSvgImportWorker()) return;

  const stack = [root];
  while (stack.length > 0) {
    const node = stack.pop();
    if (!node || node.kind !== 'block') continue;
    const kids = blockChildren(node);
    for (let i = kids.length - 1; i >= 0; i--) stack.push(kids[i]);

    if (String(node.tagName || '').toLowerCase() !== 'svg' || !node.srcNode) continue;
    const vb = parseViewBox(node.srcNode);
    const shapes = listSvgShapeInputs(node.srcNode);
    if (shapes.length <= 0) continue;
    const assetId = (hash32(node.id) || 1) >>> 0;
    requestSvgBuildAsync(String(node.id), assetId, {
      minX: vb.minX,
      minY: vb.minY,
      width: vb.width,
      height: vb.height,
    }, vb.outW, vb.outH, shapes);
  }
}

function importIframeAppsOnce(root) {
  iframeRenderByBlockId.clear();
  iframeImportWorker.pending.clear();
  if (!ensureIframeImportWorker()) return;

  const stack = [root];
  while (stack.length > 0) {
    const node = stack.pop();
    if (!node || node.kind !== 'block') continue;
    const kids = blockChildren(node);
    for (let i = kids.length - 1; i >= 0; i--) stack.push(kids[i]);

    const tag = String(node.tagName || '').toLowerCase();
    if (tag !== 'html_app_window') continue;
    const rawTag = String(node.srcNode && (node.srcNode.tagName || node.srcNode.nodeName) || '').toLowerCase();
    if (rawTag !== 'iframe') continue;
    const srcdoc = String(getAttr(node.srcNode, 'srcdoc') || '');
    if (!srcdoc) continue;
    const srcW = Math.max(1, Number(getAttr(node.srcNode, 'width') || 320));
    const srcH = Math.max(1, Number(getAttr(node.srcNode, 'height') || 240));
    requestIframeParseAsync(String(node.id), srcdoc, srcW, srcH);
  }
}

function tableRowsFromNode(tableNode) {
  if (!tableNode || !Array.isArray(tableNode.childNodes)) return [];
  const rows = [];
  const kids = tableNode.childNodes;
  for (let i = 0; i < kids.length; i++) {
    const n = kids[i];
    if (!isElement(n)) continue;
    const tag = String(n.tagName || n.nodeName || '').toLowerCase();
    if (tag === 'tr') {
      rows.push(n);
      continue;
    }
    if (tag !== 'thead' && tag !== 'tbody' && tag !== 'tfoot') continue;
    const secKids = Array.isArray(n.childNodes) ? n.childNodes : [];
    for (let j = 0; j < secKids.length; j++) {
      const r = secKids[j];
      if (!isElement(r)) continue;
      if (String(r.tagName || r.nodeName || '').toLowerCase() === 'tr') rows.push(r);
    }
  }
  return rows;
}

function tableCellsFromRow(rowNode) {
  if (!rowNode || !Array.isArray(rowNode.childNodes)) return [];
  const cells = [];
  const kids = rowNode.childNodes;
  for (let i = 0; i < kids.length; i++) {
    const n = kids[i];
    if (!isElement(n)) continue;
    const tag = String(n.tagName || n.nodeName || '').toLowerCase();
    if (tag === 'th' || tag === 'td') cells.push(n);
  }
  return cells;
}

function classifyTag(node, rawTag) {
  const tag = String(rawTag || '').toLowerCase();
  if (tag === 'iframe') return 'html_app_window';
  if (tag === 'input') {
    const t = String(getAttr(node, 'type') || '').toLowerCase();
    const temporal = temporalTagForInputType(t);
    if (temporal) return temporal;
    const rangeTag = rangeWidgetTagForInputType(t);
    if (rangeTag) return rangeTag;
    if (t === 'checkbox') return 'checkbox';
    if (t === 'radio') return 'radio';
    if (t === 'button' || t === 'submit' || t === 'reset') return 'button';
  }
  return tag;
}

function collectBlockTree(node, out) {
  if (!isElement(node)) return;
  const rawTag = String(node.tagName || node.nodeName || '').toLowerCase();
  const tag = classifyTag(node, rawTag);
  const children = [];

  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i++) {
    collectBlockTree(kids[i], children);
  }

  if (isStructuralTag(tag)) {
    for (let i = 0; i < children.length; i++) out.push(children[i]);
    return;
  }

  out.push({ kind: 'block', tagName: tag, children, id: '', srcNode: node });
}

function makeTreeFromHtml() {
  const doc = parse5.parse(HTML);
  const body = getBody(doc) || doc;
  const appWindowChildren = [];
  collectBlockTree(body, appWindowChildren);
  const appWindow = { kind: 'block', tagName: 'html_app_window', children: appWindowChildren, id: '' };
  // Synthetic root-level sibling widget lane.
  const rootScrollbar = { kind: 'block', tagName: 'scrollbar', children: [], id: '' };
  const root = { kind: 'block', tagName: 'root', children: [appWindow, rootScrollbar], id: 'root' };
  assignBlockIds(root, 'root');
  importIframeAppsOnce(root);
  importSvgAssetsOnce(root);
  return root;
}

function assignBlockIds(node, path) {
  if (!node || node.kind !== 'block') return;
  node.id = path;
  blockNodeById.set(path, node);
  const kids = blockChildren(node);
  for (let i = 0; i < kids.length; i++) {
    const child = kids[i];
    const childTag = String(child.tagName || child.nodeName || 'block').toLowerCase();
    assignBlockIds(child, `${path}/${childTag}[${i}]`);
  }
}

function blockChildren(node) {
  const kids = Array.isArray(node && node.children) ? node.children : [];
  return kids.filter((k) => k && k.kind === 'block');
}

function isScrollableTag(tag) {
  return tag === 'scrollable' || tag === 'html_app_window';
}

function isScrollbarTag(tag) {
  return tag === 'scrollbar';
}

function isCompactWidgetTag(tag) {
  return tag === 'checkbox';
}

function isButtonTag(tag) {
  return tag === 'button';
}

function isSvgTag(tag) {
  return tag === 'svg';
}

function isTableTag(tag) {
  return tag === 'table';
}

function isRadioTag(tag) {
  return tag === 'radio';
}

function defaultDetailsOpen(node) {
  if (!node || !node.srcNode) return false;
  // Default is collapsed; opt in to open with data-open="true"/"1"/"open".
  const v = String(getAttr(node.srcNode, 'data-open') || '').toLowerCase();
  return v === '1' || v === 'true' || v === 'open';
}

function isDetailsOpen(node) {
  if (!node || String(node.tagName || '').toLowerCase() !== 'details') return true;
  const id = String(node.id || '');
  if (!id) return defaultDetailsOpen(node);
  if (detailsOpenById.has(id)) return detailsOpenById.get(id) !== false;
  const open = defaultDetailsOpen(node);
  detailsOpenById.set(id, open);
  return open;
}

function visibleChildrenForNode(node) {
  const kids = blockChildren(node);
  const tag = String(node && node.tagName || '').toLowerCase();
  if (tag !== 'details') return kids;

  const open = isDetailsOpen(node);
  if (open) return kids;

  // Closed details keeps only summary child (or first child if summary is absent).
  for (let i = 0; i < kids.length; i++) {
    if (String(kids[i].tagName || '').toLowerCase() === 'summary') return [kids[i]];
  }
  return kids.length > 0 ? [kids[0]] : [];
}

function resolveDetailsId(blockId) {
  const id = String(blockId || '');
  if (!id) return '';
  const node = blockNodeById.get(id);
  const tag = String(node && node.tagName || '').toLowerCase();
  if (tag === 'details') return id;
  if (tag === 'summary') {
    const slash = id.lastIndexOf('/');
    if (slash > 0) {
      const parentId = id.slice(0, slash);
      const parent = blockNodeById.get(parentId);
      if (String(parent && parent.tagName || '').toLowerCase() === 'details') return parentId;
    }
  }
  return '';
}

function makeYogaTree(node, allBlocks, depth = 0) {
  const tag = String(node && node.tagName || '').toLowerCase();
  const isHeading = HEADING_TEXT_TAGS.has(tag);
  const isCompact = isCompactWidgetTag(tag);
  const isButton = isButtonTag(tag);
  const isSvg = isSvgTag(tag);
  const isTable = isTableTag(tag);
  const isRadio = isRadioTag(tag);
  const isTemporal = isTemporalTag(tag);
  const isRangeWidget = isRangeWidgetTag(tag);
  const nodeId = String(node && node.id || '');
  let hasExplicitLeafHeight = false;
  const yn = Yoga.Node.create();
  yn.setFlexDirection(Yoga.FLEX_DIRECTION_COLUMN);
  yn.setAlignItems(Yoga.ALIGN_STRETCH);
  if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_STRETCH);
  if (!isHeading && !isCompact && !isButton && !isSvg && !isTable && !isRadio && !isTemporal && !isRangeWidget && depth > 0 && typeof yn.setWidthPercent === 'function') yn.setWidthPercent(100);
  yn.setPadding(Yoga.EDGE_LEFT, 0);
  yn.setPadding(Yoga.EDGE_RIGHT, 0);
  yn.setPadding(Yoga.EDGE_TOP, 0);
  yn.setPadding(Yoga.EDGE_BOTTOM, 0);
  if (isScrollbarTag(tag)) {
    yn.setMinHeight(0);
    yn.setHeight(0);
    if (typeof yn.setWidth === 'function') yn.setWidth(0);
  } else {
    const minNodeSize = Math.max(1, Number(G.__trueosThemeNodeH || 16));
    yn.setMinHeight(minNodeSize);
    let minWidth = isInHtmlAppWindowSubtree(nodeId)
      ? Math.max(minNodeSize, htmlAppWindowMinWidthPx())
      : minNodeSize;
    if (isHeading) {
      const txt = nodeTextContent(node && node.srcNode ? node.srcNode : null);
      const headingTextWidth = Math.max(minNodeSize, txt.length * 16);
      minWidth = headingTextWidth;
      if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
      if (typeof yn.setWidth === 'function') yn.setWidth(headingTextWidth);
    }
    if (typeof yn.setMinWidth === 'function') yn.setMinWidth(minWidth);
    if (isCompact) {
      const box = Math.max(8, Math.min(14, minNodeSize - 2));
      if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
      if (typeof yn.setWidth === 'function') yn.setWidth(box);
      if (typeof yn.setMinWidth === 'function') yn.setMinWidth(box);
      if (typeof yn.setHeight === 'function') yn.setHeight(box);
      if (typeof yn.setMinHeight === 'function') yn.setMinHeight(box);
      hasExplicitLeafHeight = true;
    }
    if (isButton) {
      const txt = buttonLabelText(node && node.srcNode ? node.srcNode : null);
      const padX = 12;
      const btnW = Math.max(32, txt.length * 8 + padX * 2);
      const btnH = Math.max(minNodeSize + 4, 16);
      if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
      if (typeof yn.setWidth === 'function') yn.setWidth(btnW);
      if (typeof yn.setMinWidth === 'function') yn.setMinWidth(btnW);
      if (typeof yn.setHeight === 'function') yn.setHeight(btnH);
      if (typeof yn.setMinHeight === 'function') yn.setMinHeight(btnH);
      hasExplicitLeafHeight = true;
    }
    if (isRadio) {
      const box = Math.max(8, Math.min(14, minNodeSize - 2));
      if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
      if (typeof yn.setWidth === 'function') yn.setWidth(box);
      if (typeof yn.setMinWidth === 'function') yn.setMinWidth(box);
      if (typeof yn.setHeight === 'function') yn.setHeight(box);
      if (typeof yn.setMinHeight === 'function') yn.setMinHeight(box);
      hasExplicitLeafHeight = true;
    }
    if (isSvg) {
      const src = node && node.srcNode ? node.srcNode : null;
      const rawW = Number(getAttr(src, 'width'));
      const rawH = Number(getAttr(src, 'height'));
      const svgW = Number.isFinite(rawW) && rawW > 8 ? Math.round(rawW) : 160;
      const svgH = Number.isFinite(rawH) && rawH > 8 ? Math.round(rawH) : 96;
      if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
      if (typeof yn.setWidth === 'function') yn.setWidth(svgW);
      if (typeof yn.setMinWidth === 'function') yn.setMinWidth(svgW);
      if (typeof yn.setHeight === 'function') yn.setHeight(svgH);
      if (typeof yn.setMinHeight === 'function') yn.setMinHeight(svgH);
      hasExplicitLeafHeight = true;
    }
    if (isTable) {
      const src = node && node.srcNode ? node.srcNode : null;
      const rows = tableRowsFromNode(src);
      let cols = 0;
      for (let i = 0; i < rows.length; i++) cols = Math.max(cols, tableCellsFromRow(rows[i]).length);
      const rowPx = Math.max(12, minNodeSize + 2);
      const tableH = Math.max(minNodeSize, rows.length > 0 ? (rows.length * rowPx + 2) : (rowPx * 2));
      const tableMinW = Math.max(minWidth, cols > 0 ? (cols * 48) : minWidth);
      if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_STRETCH);
      if (typeof yn.setWidthPercent === 'function') yn.setWidthPercent(100);
      if (typeof yn.setMinWidth === 'function') yn.setMinWidth(tableMinW);
      if (typeof yn.setHeight === 'function') yn.setHeight(tableH);
      if (typeof yn.setMinHeight === 'function') yn.setMinHeight(tableH);
      hasExplicitLeafHeight = true;
    }
    if (isTemporal) {
      applyYogaDefaultsTemporalInput(yn, Yoga, tag, node && node.srcNode ? node.srcNode : null);
      hasExplicitLeafHeight = true;
    }
    if (isRangeWidget) {
      applyYogaDefaultsRangeWidget(yn, Yoga, tag, node && node.srcNode ? node.srcNode : null);
      hasExplicitLeafHeight = true;
    }
  }

  allBlocks.push({ node, yoga: yn, depth });

  const kids = visibleChildrenForNode(node);
  if (kids.length <= 0) {
    if (!hasExplicitLeafHeight) yn.setHeight(Math.max(1, Number(G.__trueosThemeNodeH || 16)));
    return yn;
  }

  for (let i = 0; i < kids.length; i++) {
    const child = makeYogaTree(kids[i], allBlocks, depth + 1);
    if (i > 0 && typeof child.setMargin === 'function') child.setMargin(Yoga.EDGE_TOP, 1);
    yn.insertChild(child, yn.getChildCount());
  }
  return yn;
}

const scrollById = new Map();

function scrollOffsetFor(nodeId) {
  return Math.max(0, Number(scrollById.get(String(nodeId || '')) || 0));
}

function computeRects(blocks) {
  const ordered = blocks;
  const out = [];
  const rectEntries = [];
  const rectById = new Map();
  const absXByDepth = [];
  const absYByDepth = [];
  const scrollByDepth = [];
  const overlayCancelByDepth = [];
  for (let i = 0; i < ordered.length; i++) {
    const entry = ordered[i];
    const tag = String(entry && entry.node && entry.node.tagName || '').toLowerCase();
    const nodeId = String(entry && entry.node && entry.node.id || '');
    const depth = Math.max(0, Number(entry.depth || 0));
    const yn = entry.yoga;
    const localX = Number(yn.getComputedLeft() || 0);
    const localY = Number(yn.getComputedTop() || 0);
    const inheritedScrollY = depth > 0 ? Number(scrollByDepth[depth - 1] || 0) : 0;
    const inheritedOverlayCancel = depth > 0 ? Number(overlayCancelByDepth[depth - 1] || 0) : 0;
    const parentX = depth > 0 ? Number(absXByDepth[depth - 1] || 0) : 0;
    const parentY = depth > 0 ? Number(absYByDepth[depth - 1] || 0) : 0;
    let overlayCancel = inheritedOverlayCancel;
    if (isDialogOverlayNode(nodeId, tag) && overlayCancel <= 0) {
      const ownerAppWindowId = htmlAppWindowAncestorId(nodeId);
      if (ownerAppWindowId) overlayCancel = Math.max(0, Number(scrollOffsetFor(ownerAppWindowId) || 0));
    }
    const absX = parentX + localX;
    const absY = parentY + localY - inheritedScrollY + overlayCancel;
    absXByDepth[depth] = absX;
    absYByDepth[depth] = absY;
    overlayCancelByDepth[depth] = overlayCancel;

    const selfScrollY = isScrollableTag(tag) ? scrollOffsetFor(nodeId) : 0;
    scrollByDepth[depth] = inheritedScrollY + selfScrollY;

    if (tag === 'root') continue;

    const drawIndent = Math.max(0, depth - 1) * Math.max(0, Number(G.__trueosThemeHierarchyIndent || 8));
    let x = absX + drawIndent;
    let y = absY;
    const minRectW = isInHtmlAppWindowSubtree(nodeId) ? htmlAppWindowMinWidthPx() : 2;
    let w = Math.max(minRectW, Number(yn.getComputedWidth() || 0) - drawIndent);
    let h = Math.max(2, Number(yn.getComputedHeight() || 0));
    let drawDepth = depth;
    if (isDialogOverlayNode(nodeId, tag)) {
      const ownerAppWindowId = htmlAppWindowAncestorId(nodeId);
      const owner = ownerAppWindowId ? rectById.get(ownerAppWindowId) : null;
      if (owner) {
        const margin = 8;
        const innerW = Math.max(24, Number(owner.w || 0) - margin * 2);
        const innerH = Math.max(24, Number(owner.h || 0) - margin * 2);
        w = Math.min(w, innerW);
        h = Math.min(h, innerH);
        x = Math.round(Number(owner.x || 0) + margin);
        y = Math.round(Number(owner.y || 0) + margin);
        // Keep subtree anchored to the floating dialog origin.
        absXByDepth[depth] = x - drawIndent;
        absYByDepth[depth] = y;
      }
      // Overlay dialogs render above the normal app-window content lane.
      drawDepth = Math.max(depth, 32);
    }
    const scrollable = isScrollableTag(tag) ? 1 : 0;
    if (!isScrollbarTag(tag)) {
      out.push(x, y, w, h, drawDepth, scrollable, 0);
    }
    const rectEntry = { id: nodeId, tag, x, y, w, h, depth: drawDepth, scrollable };
    rectEntries.push(rectEntry);
    rectById.set(nodeId, rectEntry);
  }
  return { packed: out, entries: rectEntries };
}

const logicalRoot = makeTreeFromHtml();
const blocks = [];
const yogaRoot = makeYogaTree(logicalRoot, blocks, 0);
const widgetByTag = new Map();
const lastRectsById = new Map();
const debugScroll = { timer: null, lastTs: 0, phase: 0, appWindowId: '' };
const widgetPulseById = new Map();
const cursorPlane = {
  pointers: new Map(),
  timer: null,
  enabled: true,
  maxPointers: 4,
  followKernelCount: 1,
};
let pulseTicker = null;
const WIDGET_PULSE_MS = 500;
let viewportW = 1280;
let viewportH = 800;

function cursorGlyphSizePx() {
  return Math.max(6, Number(G.__trueosThemeCursorSize || 12));
}

function cursorColorForId(id) {
  const palette = [0x111111, 0x2563eb, 0x16a34a, 0xdc2626, 0x0ea5e9, 0xf59e0b];
  return palette[Math.max(0, Number(id || 0) - 1) % palette.length];
}

function seedCursorPlaneDefaults(vw, vh) {
  if (cursorPlane.pointers.size > 0) return;
  const seeds = [
    { id: 1, x: 0.31, y: 0.58 },
    { id: 2, x: 0.36, y: 0.54 },
    { id: 3, x: 0.42, y: 0.62 },
    { id: 4, x: 0.47, y: 0.57 },
  ];
  for (let i = 0; i < seeds.length; i++) {
    const s = seeds[i];
    cursorPlane.pointers.set(s.id, {
      x: Math.max(0, Number(vw) * s.x),
      y: Math.max(0, Number(vh) * s.y),
      color: cursorColorForId(s.id),
      visible: true,
    });
  }
}

function collectCursorPlanePacked(vw, vh) {
  const packed = [];
  const size = cursorGlyphSizePx();
  for (const [id, p] of cursorPlane.pointers.entries()) {
    if (!p || p.visible === false) continue;
    const x = Math.max(0, Math.min(Number(vw) - 1, Number(p.x || 0)));
    const y = Math.max(0, Math.min(Number(vh) - 1, Number(p.y || 0)));
    const color = Number(p.color != null ? p.color : cursorColorForId(id)) >>> 0;
    packed.push(x, y, size, color);
  }
  return packed;
}

function paintCursorPlaneOnly() {
  if (!cursorPlane.enabled) return false;
  if (typeof G.__trueosDrawCursorPlane !== 'function') return false;
  const packed = collectCursorPlanePacked(viewportW, viewportH);
  G.__trueosDrawCursorPlane(packed, viewportW, viewportH);
  return true;
}

function refreshCursorPlaneFromKernel(maxPointers = cursorPlane.maxPointers) {
  if (typeof G.__trueosReadCursorState !== 'function') return 0;
  const max = Math.max(
    1,
    Math.min(
      Number(maxPointers || 0) | 0,
      Number(cursorPlane.followKernelCount || 0) | 0,
    ),
  );
  let updated = 0;
  for (let id = 1; id <= max; id++) {
    const s = G.__trueosReadCursorState(id);
    if (!s || Number(s.ok || 0) < 1) continue;
    const prev = cursorPlane.pointers.get(id);
    const nx = Math.max(0, Math.min(viewportW - 1, Number(s.x || 0)));
    const ny = Math.max(0, Math.min(viewportH - 1, Number(s.y || 0)));
    if (!prev || Math.abs(nx - Number(prev.x || 0)) >= 1 || Math.abs(ny - Number(prev.y || 0)) >= 1) {
      cursorPlane.pointers.set(id, {
        x: nx,
        y: ny,
        color: Number(prev && prev.color != null ? prev.color : cursorColorForId(id)) >>> 0,
        visible: true,
      });
      updated += 1;
    }
  }
  return updated;
}

function stopCursorPlaneTicker() {
  if (cursorPlane.timer) {
    clearInterval(cursorPlane.timer);
    cursorPlane.timer = null;
  }
}

function ensureCursorPlaneTicker() {
  if (cursorPlane.timer) return;
  cursorPlane.timer = setInterval(() => {
    const changed = refreshCursorPlaneFromKernel(cursorPlane.maxPointers);
    if (changed > 0) paintCursorPlaneOnly();
  }, 16);
}

function hasActivePulse(nowMs) {
  for (const pulse of widgetPulseById.values()) {
    if (pulse && Number(pulse.untilMs || 0) > nowMs) return true;
  }
  return false;
}

function pruneExpiredPulses(nowMs) {
  for (const [id, pulse] of widgetPulseById.entries()) {
    if (!pulse || Number(pulse.untilMs || 0) <= nowMs) widgetPulseById.delete(id);
  }
}

function stopPulseTicker() {
  if (pulseTicker) {
    clearInterval(pulseTicker);
    pulseTicker = null;
  }
}

function ensurePulseTicker() {
  if (pulseTicker) return;
  pulseTicker = setInterval(() => {
    const now = Date.now();
    pruneExpiredPulses(now);
    if (!hasActivePulse(now)) {
      stopPulseTicker();
      return;
    }
    relayoutAndPaint();
  }, 33);
}

function markWidgetUpdated(blockId) {
  const id = String(blockId || '');
  if (!id) return false;
  const now = Date.now();
  widgetPulseById.set(id, { startMs: now, untilMs: now + WIDGET_PULSE_MS });
  ensurePulseTicker();
  relayoutAndPaint();
  return true;
}

function setScrollInternal(blockId, scrollY, repaint) {
  const id = String(blockId || '');
  if (!id) return false;
  scrollById.set(id, Math.max(0, Number(scrollY || 0)));
  if (repaint) relayoutAndPaint();
  return true;
}

function collectCurrentRects() {
  const out = [];
  for (const rect of lastRectsById.values()) out.push(rect);
  return out;
}

function appWindowMetrics(appWindowId, rectEntries, selfScrollbarId = '') {
  let appWindowRect = null;
  for (let i = 0; i < rectEntries.length; i++) {
    const r = rectEntries[i];
    if (!r) continue;
    if (String(r.id || '') === appWindowId && String(r.tag || '') === 'html_app_window') {
      appWindowRect = r;
      break;
    }
  }
  if (!appWindowRect) return null;

  const appWindowScrollY = Math.max(0, Number(scrollOffsetFor(appWindowId) || 0));
  const viewportH = Math.max(1, Number(appWindowRect.h || 0));
  let contentBottom = Number(appWindowRect.y || 0) + viewportH;
  for (let i = 0; i < rectEntries.length; i++) {
    const c = rectEntries[i];
    if (!c) continue;
    const cid = String(c.id || '');
    if (!cid || cid === selfScrollbarId || cid === appWindowId) continue;
    if (!cid.startsWith(`${appWindowId}/`)) continue;
    if (isDialogSubtreeId(cid)) continue;
    if (String(c.tag || '') === 'scrollbar') continue;
    const b = Number(c.y || 0) + Number(c.h || 0) + appWindowScrollY;
    if (b > contentBottom) contentBottom = b;
  }

  const contentH = Math.max(viewportH, contentBottom - Number(appWindowRect.y || 0));
  const maxScroll = Math.max(0, contentH - viewportH);
  return { appWindowRect, viewportH, contentH, maxScroll, scrollY: Math.min(maxScroll, appWindowScrollY) };
}


function applyViewportConstraints(vw, vh) {
  if (typeof yogaRoot.setWidth === 'function') yogaRoot.setWidth(vw);
  if (typeof yogaRoot.setHeight === 'function') yogaRoot.setHeight(vh);

  for (let i = 0; i < blocks.length; i++) {
    const entry = blocks[i];
    if (!entry || !entry.yoga) continue;
    const tag = String(entry.node && entry.node.tagName || '').toLowerCase();
    if (tag === 'root') continue;
    // Document-level width source: only direct children of the synthetic root.
    if (Number(entry.depth || 0) === 1 && typeof entry.yoga.setWidth === 'function') {
      entry.yoga.setWidth(vw);
    }
    // The synthetic html_app_window is our viewport container; keep it bounded to window height.
    if (tag === 'html_app_window' && typeof entry.yoga.setHeight === 'function') {
      entry.yoga.setHeight(vh);
      if (typeof entry.yoga.setMinHeight === 'function') entry.yoga.setMinHeight(vh);
      if (typeof entry.yoga.setMaxHeight === 'function') entry.yoga.setMaxHeight(vh);
    }
  }
}

function computeViewport() {
  const W = G.window || G;
  const vw = Math.max(1, Number(W.innerWidth || 1280));
  const vh = Math.max(1, Number(W.innerHeight || 800));
  viewportW = vw;
  viewportH = vh;
  seedCursorPlaneDefaults(vw, vh);
  return { vw, vh };
}

function relayout(vw, vh) {
  applyViewportConstraints(vw, vh);
  yogaRoot.calculateLayout(vw, vh, Yoga.DIRECTION_LTR);
  const rects = computeRects(blocks);
  lastRectsById.clear();
  for (let i = 0; i < rects.entries.length; i++) {
    const e = rects.entries[i];
    lastRectsById.set(e.id, e);
  }
  return rects;
}

function paintLayout(packedRects, vw, vh) {
  if (typeof G.__trueosDrawLayoutRects === 'function') {
    const inlineTextRuns = collectInlineSemanticTextRuns(lastRectsById);
    G.__trueosDrawLayoutRects(packedRects, vw, vh, inlineTextRuns);
  }
}

function collectInlineSemanticTextRuns(rectsById) {
  const packed = [];
  if (!rectsById || typeof rectsById.values !== 'function') return packed;
  for (const rect of rectsById.values()) {
    if (!rect) continue;
    const tag = String(rect.tag || '').toLowerCase();
    if (!INLINE_TEXT_TAGS.has(tag)) continue;
    const id = String(rect.id || '');
    if (!id) continue;
    const node = blockNodeById.get(id);
    if (!node || !node.srcNode) continue;
    let text = '';
    if (isTemporalTag(tag)) text = temporalDisplayText(tag, node.srcNode);
    else text = nodeTextPreview(node.srcNode);
    if (!text) continue;
    const insetX = tag === 'button' ? 8 : 2;
    const x = Math.round(Number(rect.x || 0) + insetX);
    const y = Math.round(Number(rect.y || 0) + 2);
    packed.push(x, y, text);
  }

  // Checkbox text lane: place label text 16px to the right of each checkbox.
  for (const rect of rectsById.values()) {
    if (!rect) continue;
    const tag = String(rect.tag || '').toLowerCase();
    if (tag !== 'checkbox') continue;
    const id = String(rect.id || '');
    if (!id) continue;
    const node = blockNodeById.get(id);
    if (!node || !node.srcNode) continue;
    const text = checkboxLabelText(node.srcNode);
    if (!text) continue;
    const x = Math.round(Number(rect.x || 0) + Number(rect.w || 0) + 16);
    const y = Math.round(Number(rect.y || 0));
    packed.push(x, y, text);
  }

  // Radio text lane: place label text 16px to the right of each radio circle.
  for (const rect of rectsById.values()) {
    if (!rect) continue;
    const tag = String(rect.tag || '').toLowerCase();
    if (tag !== 'radio') continue;
    const id = String(rect.id || '');
    if (!id) continue;
    const node = blockNodeById.get(id);
    if (!node || !node.srcNode) continue;
    const text = radioLabelText(node.srcNode);
    if (!text) continue;
    const x = Math.round(Number(rect.x || 0) + Number(rect.w || 0) + 16);
    const y = Math.round(Number(rect.y || 0));
    packed.push(x, y, text);
  }

  // Summary text lane: place summary label text 16px to the right of disclosure box.
  for (const rect of rectsById.values()) {
    if (!rect) continue;
    const tag = String(rect.tag || '').toLowerCase();
    if (tag !== 'summary') continue;
    const id = String(rect.id || '');
    if (!id) continue;
    const node = blockNodeById.get(id);
    if (!node || !node.srcNode) continue;
    const text = summaryLabelText(node.srcNode);
    if (!text) continue;
    const x = Math.round(Number(rect.x || 0) + 16);
    const y = Math.round(Number(rect.y || 0));
    packed.push(x, y, text);
  }

  // iframe/html_app_window child lane: text runs generated by a dedicated worker
  // from iframe srcdoc parse5+yoga sub-layout.
  for (const rect of rectsById.values()) {
    if (!rect) continue;
    const tag = String(rect.tag || '').toLowerCase();
    if (tag !== 'html_app_window') continue;
    const id = String(rect.id || '');
    if (!id) continue;
    const node = blockNodeById.get(id);
    if (!node || !node.srcNode) continue;
    const rawTag = String(node.srcNode.tagName || node.srcNode.nodeName || '').toLowerCase();
    if (rawTag !== 'iframe') continue;
    const snap = iframeRenderByBlockId.get(id);
    if (!snap || !Array.isArray(snap.texts) || snap.texts.length <= 0) continue;

    const innerX = Math.round(Number(rect.x || 0) + 2);
    const innerY = Math.round(Number(rect.y || 0) + 18);
    const innerW = Math.max(1, Math.round(Number(rect.w || 0) - 4));
    const innerH = Math.max(1, Math.round(Number(rect.h || 0) - 20));
    const srcW = Math.max(1, Number(snap.viewportW || innerW));
    const srcH = Math.max(1, Number(snap.viewportH || innerH));
    const sx = innerW / srcW;
    const sy = innerH / srcH;

    for (let i = 0; i + 2 < snap.texts.length; i += 3) {
      const tx = innerX + Math.round(Number(snap.texts[i + 0] || 0) * sx);
      const ty = innerY + Math.round(Number(snap.texts[i + 1] || 0) * sy);
      const tt = String(snap.texts[i + 2] || '');
      if (!tt) continue;
      if (ty < innerY || ty > innerY + innerH) continue;
      packed.push(tx, ty, tt);
    }
  }

  // Table cell text lane: widget draws the grid; we place th/td text in cell slots.
  for (const rect of rectsById.values()) {
    if (!rect) continue;
    const tag = String(rect.tag || '').toLowerCase();
    if (tag !== 'table') continue;
    const id = String(rect.id || '');
    if (!id) continue;
    const node = blockNodeById.get(id);
    if (!node || !node.srcNode) continue;

    const rows = tableRowsFromNode(node.srcNode);
    if (rows.length <= 0) continue;
    let cols = 0;
    for (let r = 0; r < rows.length; r++) cols = Math.max(cols, tableCellsFromRow(rows[r]).length);
    if (cols <= 0) continue;

    const x0 = Math.round(Number(rect.x || 0));
    const y0 = Math.round(Number(rect.y || 0));
    const w = Math.max(1, Math.round(Number(rect.w || 0)));
    const h = Math.max(1, Math.round(Number(rect.h || 0)));
    const rowH = Math.max(10, Math.floor(h / rows.length));
    const colW = Math.max(16, Math.floor(w / cols));

    for (let r = 0; r < rows.length; r++) {
      const cells = tableCellsFromRow(rows[r]);
      const cy = y0 + r * rowH;
      for (let c = 0; c < cells.length; c++) {
        const cx = x0 + c * colW;
        const txt = nodeTextPreview(cells[c], 48);
        if (!txt) continue;
        packed.push(cx + 4, cy + 3, txt);
      }
    }
  }

  return packed;
}

function paintWidgets(rectEntries, vw, vh) {
  for (let i = 0; i < rectEntries.length; i++) {
    const rect = rectEntries[i];
    const renderWidget = widgetByTag.get(rect.tag);
    if (typeof renderWidget !== 'function') continue;
    try {
      renderWidget(rect, { viewportW: vw, viewportH: vh, mode: 'immediate' });
    } catch (_) {}
  }
}

function collectWidgetPackedRects(rectEntries, vw, vh) {
  const packed = [];

  for (let i = 0; i < rectEntries.length; i++) {
    const rect = rectEntries[i];
    if (!rect) continue;
    const tag = String(rect.tag || '').toLowerCase();
    if (tag !== 'html_app_window') continue;
    const id = String(rect.id || '');
    if (!id) continue;
    const node = blockNodeById.get(id);
    if (!node || !node.srcNode) continue;
    const rawTag = String(node.srcNode.tagName || node.srcNode.nodeName || '').toLowerCase();
    if (rawTag !== 'iframe') continue;
    const snap = iframeRenderByBlockId.get(id);
    if (!snap || !Array.isArray(snap.packed) || snap.packed.length <= 0) continue;

    const innerX = Math.round(Number(rect.x || 0) + 2);
    const innerY = Math.round(Number(rect.y || 0) + 18);
    const innerW = Math.max(1, Math.round(Number(rect.w || 0) - 4));
    const innerH = Math.max(1, Math.round(Number(rect.h || 0) - 20));
    const srcW = Math.max(1, Number(snap.viewportW || innerW));
    const srcH = Math.max(1, Number(snap.viewportH || innerH));
    const sx = innerW / srcW;
    const sy = innerH / srcH;

    for (let j = 0; j + 6 < snap.packed.length; j += 7) {
      const rx = innerX + Math.round(Number(snap.packed[j + 0] || 0) * sx);
      const ry = innerY + Math.round(Number(snap.packed[j + 1] || 0) * sy);
      const rw = Math.max(1, Math.round(Number(snap.packed[j + 2] || 1) * sx));
      const rh = Math.max(1, Math.round(Number(snap.packed[j + 3] || 1) * sy));
      const rd = Math.max(0, Number(rect.depth || 0) + 1 + Math.max(0, Number(snap.packed[j + 4] || 0)));
      const rs = Number(snap.packed[j + 5] || 0);
      const rstyle = Math.max(0, Number(snap.packed[j + 6] || 0));
      packed.push(rx, ry, rw, rh, rd, rs, rstyle);
    }
  }

  for (let i = 0; i < rectEntries.length; i++) {
    const rect = rectEntries[i];
    const renderWidget = widgetByTag.get(rect.tag);
    if (typeof renderWidget !== 'function') continue;
    try {
      const contributed = renderWidget(rect, {
        viewportW: vw,
        viewportH: vh,
        mode: 'collect',
        rectEntries,
        scrollOffsetFor,
        getSourceNodeById: (blockId) => {
          const id = String(blockId || '');
          const node = blockNodeById.get(id);
          return node && node.srcNode ? node.srcNode : null;
        },
        getSvgAssetIdByBlockId: (blockId) => {
          return Number(svgAssetIdByBlockId.get(String(blockId || '')) || 0) >>> 0;
        },
      });
      if (!Array.isArray(contributed)) continue;
      for (let j = 0; j + 6 < contributed.length; j += 7) {
        packed.push(
          Number(contributed[j + 0] || 0),
          Number(contributed[j + 1] || 0),
          Math.max(1, Number(contributed[j + 2] || 1)),
          Math.max(1, Number(contributed[j + 3] || 1)),
          Math.max(0, Number(contributed[j + 4] || 0)),
          Number(contributed[j + 5] || 0),
          Math.max(0, Number(contributed[j + 6] || 0)),
        );
      }
    } catch (_) {}
  }
  return packed;
}

function collectPulsePackedRects(rectEntries, nowMs) {
  const packed = [];
  for (let i = 0; i < rectEntries.length; i++) {
    const rect = rectEntries[i];
    if (!rect) continue;
    const id = String(rect.id || '');
    if (!id) continue;
    const pulse = widgetPulseById.get(id);
    if (!pulse) continue;

    const startMs = Number(pulse.startMs || 0);
    const untilMs = Number(pulse.untilMs || 0);
    if (nowMs >= untilMs) continue;

    // 4Hz pulsing between two highlight tones.
    const t = Math.max(0, (nowMs - startMs) / 1000);
    const fast = Math.sin(t * Math.PI * 8) >= 0 ? 3 : 4;
    const x = Math.round(Number(rect.x || 0));
    const y = Math.round(Number(rect.y || 0));
    const w = Math.max(2, Math.round(Number(rect.w || 0)));
    const h = Math.max(2, Math.round(Number(rect.h || 0)));
    packed.push(x, y, w, h, Math.max(0, Number(rect.depth || 0) + 4), 0, fast);
  }
  return packed;
}

function stopDebugAutoScroll() {
  if (debugScroll.timer) {
    clearInterval(debugScroll.timer);
    debugScroll.timer = null;
  }
  debugScroll.lastTs = 0;
}

function startDebugAutoScroll(opts) {
  stopDebugAutoScroll();
  const cfg = opts && typeof opts === 'object' ? opts : {};
  const repaint = cfg.repaint === true;
  const intervalMs = Math.max(16, Number(cfg.intervalMs || 50));
  const cyclesPerSec = Math.max(0.03, Number(cfg.cyclesPerSec || 0.06));
  const appWindowId = String(cfg.appWindowId || findFirstBlockIdByTag('html_app_window') || '');
  if (!appWindowId) return false;
  debugScroll.appWindowId = appWindowId;
  debugScroll.phase = 0;

  debugScroll.timer = setInterval(() => {
    const now = Date.now();
    const last = debugScroll.lastTs || now;
    const dt = Math.max(0.001, (now - last) / 1000);
    debugScroll.lastTs = now;

    const metrics = appWindowMetrics(appWindowId, collectCurrentRects());
    if (!metrics) return;
    const maxScroll = Math.max(0, Number(metrics.maxScroll || 0));
    if (maxScroll <= 0) {
      setScrollInternal(appWindowId, 0, repaint);
      return;
    }

    debugScroll.phase += dt * cyclesPerSec * Math.PI * 2;
    const wave01 = (Math.sin(debugScroll.phase) + 1) * 0.5;
    const target = wave01 * maxScroll;
    setScrollInternal(appWindowId, target, repaint);
  }, intervalMs);
  return true;
}

function relayoutAndPaint() {
  const { vw, vh } = computeViewport();
  const rects = relayout(vw, vh);
  const widgetRects = collectWidgetPackedRects(rects.entries, vw, vh);
  const pulseRects = collectPulsePackedRects(rects.entries, Date.now());
  let combinedRects = rects.packed;
  if (widgetRects.length > 0) combinedRects = combinedRects.concat(widgetRects);
  if (pulseRects.length > 0) combinedRects = combinedRects.concat(pulseRects);
  paintLayout(combinedRects, vw, vh);
  paintWidgets(rects.entries, vw, vh);
  paintCursorPlaneOnly();
  return true;
}

function paintWidgetNow(blockId) {
  const rect = lastRectsById.get(String(blockId || ''));
  if (!rect) return false;
  const renderWidget = widgetByTag.get(rect.tag);
  if (typeof renderWidget !== 'function') return false;
  try {
    renderWidget(rect, { viewportW, viewportH });
    return true;
  } catch (_) {
    return false;
  }
}

function findFirstBlockIdByTag(tag) {
  const t = String(tag || '').toLowerCase();
  for (let i = 0; i < blocks.length; i++) {
    const entry = blocks[i];
    if (!entry || !entry.node) continue;
    if (String(entry.node.tagName || '').toLowerCase() !== t) continue;
    return String(entry.node.id || '');
  }
  return '';
}

G.__trueosBrowser = {
  relayoutAndPaint,
  // Widget renderers paint immediately into their own rect.
  registerWidget(tagName, paintFn) {
    const tag = String(tagName || '').toLowerCase();
    if (!tag || typeof paintFn !== 'function') return false;
    widgetByTag.set(tag, paintFn);
    return true;
  },
  unregisterWidget(tagName) {
    return widgetByTag.delete(String(tagName || '').toLowerCase());
  },
  paintWidgetById(blockId) {
    const ok = paintWidgetNow(blockId);
    if (ok) markWidgetUpdated(blockId);
    return ok;
  },
  paintWidgetsByTag(tagName) {
    const tag = String(tagName || '').toLowerCase();
    let painted = 0;
    for (const rect of lastRectsById.values()) {
      if (rect.tag !== tag) continue;
      const fn = widgetByTag.get(rect.tag);
      if (typeof fn !== 'function') continue;
      try {
        fn(rect, { viewportW, viewportH });
        painted += 1;
      } catch (_) {}
    }
    return painted;
  },
  getRectById(blockId) {
    return lastRectsById.get(String(blockId || '')) || null;
  },
  getFirstBlockIdByTag(tagName) {
    return findFirstBlockIdByTag(tagName);
  },
  setScroll(blockId, scrollY) {
    return setScrollInternal(blockId, scrollY, true);
  },
  setScrollNoRepaint(blockId, scrollY) {
    return setScrollInternal(blockId, scrollY, false);
  },
  widgetDidUpdate(blockId) {
    return markWidgetUpdated(blockId);
  },
  setCursorPlaneEnabled(enabled) {
    cursorPlane.enabled = enabled !== false;
    if (cursorPlane.enabled) {
      ensureCursorPlaneTicker();
      paintCursorPlaneOnly();
    } else {
      stopCursorPlaneTicker();
    }
    return cursorPlane.enabled;
  },
  setCursorKernelFollowCount(count) {
    cursorPlane.followKernelCount = Math.max(0, Math.min(cursorPlane.maxPointers, Number(count || 0) | 0));
    return cursorPlane.followKernelCount;
  },
  setCursorPointer(pointerId, x, y, color) {
    const id = Math.max(1, Number(pointerId || 0) | 0);
    cursorPlane.pointers.set(id, {
      x: Math.max(0, Math.min(viewportW - 1, Number(x || 0))),
      y: Math.max(0, Math.min(viewportH - 1, Number(y || 0))),
      color: Number(color != null ? color : cursorColorForId(id)) >>> 0,
      visible: true,
    });
    return paintCursorPlaneOnly();
  },
  clearCursorPointer(pointerId) {
    const id = Math.max(1, Number(pointerId || 0) | 0);
    const ok = cursorPlane.pointers.delete(id);
    paintCursorPlaneOnly();
    return ok;
  },
  refreshCursorPlane(maxPointers) {
    const changed = refreshCursorPlaneFromKernel(maxPointers);
    if (changed > 0) paintCursorPlaneOnly();
    return changed;
  },
  repaintCursorPlane() {
    return paintCursorPlaneOnly();
  },
  widgetTagDidUpdate(tagName) {
    const tag = String(tagName || '').toLowerCase();
    if (!tag) return 0;
    let count = 0;
    for (const rect of lastRectsById.values()) {
      if (String(rect.tag || '') !== tag) continue;
      if (markWidgetUpdated(rect.id)) count += 1;
    }
    return count;
  },
  startDebugAutoScroll(opts) {
    return startDebugAutoScroll(opts);
  },
  stopDebugAutoScroll() {
    stopDebugAutoScroll();
    return true;
  },
  getDetailsOpen(blockId) {
    const id = resolveDetailsId(blockId);
    if (!id) return true;
    if (detailsOpenById.has(id)) return detailsOpenById.get(id) !== false;
    const node = blockNodeById.get(id);
    return defaultDetailsOpen(node);
  },
  setDetailsOpen(blockId, open) {
    const id = resolveDetailsId(blockId);
    if (!id) return false;
    detailsOpenById.set(id, open !== false);
    relayoutAndPaint();
    return true;
  },
  toggleDetails(blockId) {
    const id = resolveDetailsId(blockId);
    if (!id) return false;
    const cur = detailsOpenById.has(id)
      ? (detailsOpenById.get(id) !== false)
      : defaultDetailsOpen(blockNodeById.get(id));
    detailsOpenById.set(id, !cur);
    relayoutAndPaint();
    return !cur;
  },
};

G.__trueosBrowser.registerWidget('html_app_window', renderHtmlAppWindowWidget);
G.__trueosBrowser.registerWidget('scrollbar', renderScrollbarWidget);
G.__trueosBrowser.registerWidget('checkbox', renderCheckboxWidget);
G.__trueosBrowser.registerWidget('summary', renderSummaryWidget);
G.__trueosBrowser.registerWidget('dialog', renderDialogWidget);
G.__trueosBrowser.registerWidget('button', renderButtonWidget);
G.__trueosBrowser.registerWidget('svg', renderSvgWidget);
G.__trueosBrowser.registerWidget('table', renderTableWidget);
G.__trueosBrowser.registerWidget('form', renderFormWidget);
G.__trueosBrowser.registerWidget('radio', renderRadioWidget);
G.__trueosBrowser.registerWidget('timeinput', renderTemporalWidget);
G.__trueosBrowser.registerWidget('dateinput', renderTemporalWidget);
G.__trueosBrowser.registerWidget('monthinput', renderTemporalWidget);
G.__trueosBrowser.registerWidget('weekinput', renderTemporalWidget);
G.__trueosBrowser.registerWidget('datetimelocalinput', renderTemporalWidget);
G.__trueosBrowser.registerWidget('slider', renderRangeWidget);
G.__trueosBrowser.registerWidget('progress', renderRangeWidget);
G.__trueosBrowser.registerWidget('meter', renderRangeWidget);

const DEBUG_AUTOSCROLL_BOOT = true;

if (typeof (globalThis.window || globalThis).addEventListener === 'function') {
  (globalThis.window || globalThis).addEventListener('resize', relayoutAndPaint);
}

relayoutAndPaint();
ensureCursorPlaneTicker();

if (DEBUG_AUTOSCROLL_BOOT) {
  // Visual debug lane: keep default API logic-only, but show movement on boot.
  G.__trueosBrowser.startDebugAutoScroll({ repaint: true, intervalMs: 50, cyclesPerSec: 0.05 });
}

try {
  console.log(`[browser.mjs] parse5+yoga online blocks=${blocks.length} mode=widget-on-demand`);
} catch (_) {}
