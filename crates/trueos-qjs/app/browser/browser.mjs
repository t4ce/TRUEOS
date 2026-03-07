import * as parse5 from 'parse5';
import Yoga from 'yoga-layout';
import { HTML_APP_WINDOW_CONTENT_INSET, renderHtmlAppWindowWidget } from './widgets/htmlAppWindowWidget.mjs';
import { renderScrollbarWidget } from './widgets/scrollbarWidget.mjs';
import { layoutStatusbarItems, renderStatusbarWidget } from './widgets/statusbarWidget.mjs';
import { renderCheckboxWidget } from './widgets/checkboxWidget.mjs';
import { renderSummaryWidget } from './widgets/summaryWidget.mjs';
import { renderDetailsWidget } from './widgets/detailsWidget.mjs';
import { renderDialogWidget } from './widgets/dialogWidget.mjs';
import { renderButtonWidget } from './widgets/buttonWidget.mjs';
import { renderSvgWidget } from './widgets/svgWidget.mjs';
import { renderTableWidget } from './widgets/tableWidget.mjs';
import { renderFormWidget } from './widgets/formWidget.mjs';
import { renderRadioWidget } from './widgets/radioWidget.mjs';
import { applyYogaDefaultsSelectWidget, renderSelectWidget, selectDisplayText, selectOptionTexts } from './widgets/select.mjs';
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
const STATUSBAR_ROOT_ID = 'root/statusbar[1]';
// Tags that emit inline text runs through the native text lane.
const INLINE_TEXT_TAGS = new Set([
  'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
  'p', 'button', 'input', 'textarea',
  'timeinput', 'dateinput', 'monthinput', 'weekinput', 'datetimelocalinput',
]);
const HEADING_TEXT_TAGS = new Set(['h1', 'h2', 'h3', 'h4', 'h5', 'h6']);
const blockNodeById = new Map();
const detailsOpenById = new Map();
const minimizedHtmlAppWindowById = new Map();
const svgAssetIdByBlockId = new Map();
const iframeRenderByBlockId = new Map();
const iframeSpecByBlockId = new Map();
const iframeRuntimeByBlockId = new Map();
const iframeScrollById = new Map();
const iframeTraceByBlockId = new Map();
const iframeFocusState = { activeId: '' };
const IFRAME_FRAME_PROTOCOL = 'iframeFrameV1';
const svgImportWorker = {
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

function isStatusbarTag(tag) {
  return String(tag || '').toLowerCase() === 'statusbar';
}

function parseMinimizedAttr(node) {
  if (!node || !Array.isArray(node.attrs)) return false;
  const raw = String(getAttr(node, 'data-minimized') || getAttr(node, 'minimized') || '').toLowerCase();
  if (raw === '1' || raw === 'true' || raw === 'yes' || raw === 'minimized') return true;
  return node.attrs.some((a) => String(a && a.name || '').toLowerCase() === 'hidden');
}

function isHtmlAppWindowMinimized(blockId) {
  return minimizedHtmlAppWindowById.get(String(blockId || '')) === true;
}

function htmlAppWindowStatusLabel(node, blockId) {
  const src = node && node.srcNode ? node.srcNode : null;
  const preferred = collapseInlineWhitespace(String(
    (src && (getAttr(src, 'title') || getAttr(src, 'name') || getAttr(src, 'id'))) || ''
  ));
  if (preferred) return preferred.length > 28 ? `${preferred.slice(0, 25)}...` : preferred;
  const rawTag = String(src && (src.tagName || src.nodeName) || '').toLowerCase();
  const leaf = String(blockId || '').split('/').pop() || 'window';
  const fallback = rawTag === 'iframe' ? `app-window ${leaf}` : `window ${leaf}`;
  return fallback.length > 28 ? `${fallback.slice(0, 25)}...` : fallback;
}

function collectStatusbarItems() {
  const out = [];
  for (const [id, minimized] of minimizedHtmlAppWindowById.entries()) {
    if (minimized !== true) continue;
    if (id === HTML_APP_WINDOW_ROOT_ID) continue;
    const node = blockNodeById.get(id);
    if (!node) continue;
    out.push({ id, label: htmlAppWindowStatusLabel(node, id) });
  }
  out.sort((a, b) => String(a.id).localeCompare(String(b.id)));
  return out;
}

function statusbarTargetHeightPx(vw) {
  const rowH = Math.max(12, Number(G.__trueosThemeNodeH || 16));
  const minH = rowH + 4;
  const items = collectStatusbarItems();
  if (items.length <= 0) return minH;
  const chips = layoutStatusbarItems({ x: 0, y: 0, w: Math.max(1, Number(vw || 0)), h: minH }, items);
  return Math.max(minH, Number(chips.barH || minH));
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

function collectOwnText(node, out) {
  if (!isElement(node)) return;
  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i++) {
    const k = kids[i];
    if (isTextNode(k)) {
      if (k.value) out.push(k.value);
      continue;
    }
    if (!isElement(k)) continue;
    const tag = String(k.tagName || k.nodeName || '').toLowerCase();
    if (tag === 'br' || tag === 'wbr') out.push(' ');
  }
}

function nodeTextPreview(node, maxChars = 120) {
  const parts = [];
  collectNodeText(node, parts);
  const joined = collapseInlineWhitespace(parts.join(''));
  if (!joined) return '';
  if (joined.length <= maxChars) return joined;
  return `${joined.slice(0, Math.max(0, maxChars - 3))}...`;
}

function nodeOwnTextPreview(node, maxChars = 120) {
  const parts = [];
  collectOwnText(node, parts);
  const joined = collapseInlineWhitespace(parts.join(''));
  if (!joined) return '';
  if (joined.length <= maxChars) return joined;
  return `${joined.slice(0, Math.max(0, maxChars - 3))}...`;
}

function dialogTitleText(dialogNode) {
  if (!dialogNode || !Array.isArray(dialogNode.childNodes)) return '';
  const kids = dialogNode.childNodes;
  for (let i = 0; i < kids.length; i++) {
    const k = kids[i];
    if (!isElement(k)) continue;
    const tag = String(k.tagName || k.nodeName || '').toLowerCase();
    if (tag !== 'p' && tag !== 'summary') continue;
    const t = nodeTextPreview(k, 120);
    if (t) return t;
  }
  return nodeTextPreview(dialogNode, 120);
}

function intrinsicTextMinWidth(node, fallbackPx, maxChars = 96, ownTextOnly = false) {
  const txt = ownTextOnly ? nodeOwnTextPreview(node, maxChars) : nodeTextPreview(node, maxChars);
  if (!txt) return Math.max(1, Number(fallbackPx || 1));
  return Math.max(Math.max(1, Number(fallbackPx || 1)), txt.length * 8 + 16);
}

function nodeTextContent(node) {
  const parts = [];
  collectNodeText(node, parts);
  return collapseInlineWhitespace(parts.join(''));
}

function checkboxLabelText(inputNode) {
  if (!inputNode || !inputNode.parentNode) return '';
  const parent = inputNode.parentNode;
  const parentTag = String(parent && (parent.tagName || parent.nodeName) || '').toLowerCase();
  // Summary owns the disclosure label lane; avoid duplicated text from nested checkbox.
  if (parentTag === 'summary') return '';
  const own = nodeOwnTextPreview(parent, 120);
  if (own) return own;
  return nodeTextPreview(parent, 120);
}

function summaryLabelText(summaryNode) {
  if (!isElement(summaryNode)) return '';
  const parts = [];
  const kids = Array.isArray(summaryNode.childNodes) ? summaryNode.childNodes : [];
  for (let i = 0; i < kids.length; i++) {
    const k = kids[i];
    if (isTextNode(k)) {
      if (k.value) parts.push(k.value);
      continue;
    }
    if (!isElement(k)) continue;
    const tag = String(k.tagName || k.nodeName || '').toLowerCase();
    if (tag === 'br' || tag === 'wbr') parts.push(' ');
  }
  const own = collapseInlineWhitespace(parts.join(''));
  if (own) return own;

  // Optional fallback: use plain text that is directly under the parent <details>.
  const parent = summaryNode.parentNode;
  if (!isElement(parent) || String(parent.tagName || parent.nodeName || '').toLowerCase() !== 'details') {
    return '';
  }
  const pKids = Array.isArray(parent.childNodes) ? parent.childNodes : [];
  const dParts = [];
  for (let i = 0; i < pKids.length; i++) {
    const k = pKids[i];
    if (isTextNode(k) && k.value) dParts.push(k.value);
  }
  return collapseInlineWhitespace(dParts.join(''));
}

function detailsSummaryText(detailsNode) {
  if (!isElement(detailsNode)) return '';
  const kids = Array.isArray(detailsNode.childNodes) ? detailsNode.childNodes : [];
  for (let i = 0; i < kids.length; i++) {
    const k = kids[i];
    if (!isElement(k)) continue;
    const tag = String(k.tagName || k.nodeName || '').toLowerCase();
    if (tag !== 'summary') continue;
    return summaryLabelText(k);
  }
  // Native browser fallback label when <details> has no explicit <summary>.
  return 'Details';
}

function hrCaptionText(hrNode) {
  if (!isElement(hrNode)) return '';

  // Preferred: plain text directly inside <hr> (if parser keeps any children).
  const ownParts = [];
  const ownKids = Array.isArray(hrNode.childNodes) ? hrNode.childNodes : [];
  for (let i = 0; i < ownKids.length; i++) {
    const k = ownKids[i];
    if (isTextNode(k) && k.value) ownParts.push(k.value);
  }
  const own = collapseInlineWhitespace(ownParts.join(''));
  if (own) return own;

  // Tolerate malformed HTML like <hr>Caption</hr>: parse5 keeps caption as sibling text.
  const parent = hrNode.parentNode;
  if (!isElement(parent)) return '';
  const kids = Array.isArray(parent.childNodes) ? parent.childNodes : [];
  let idx = -1;
  for (let i = 0; i < kids.length; i++) {
    if (kids[i] === hrNode) {
      idx = i;
      break;
    }
  }
  if (idx < 0) return '';
  const next = idx + 1 < kids.length ? kids[idx + 1] : null;
  if (isTextNode(next) && next.value) {
    const txt = collapseInlineWhitespace(next.value);
    if (txt) return txt;
  }
  return '';
}

function buttonLabelText(buttonNode) {
  if (!buttonNode) return '';
  return nodeTextPreview(buttonNode, 160);
}

function radioLabelText(inputNode) {
  if (!inputNode || !inputNode.parentNode) return '';
  const own = nodeOwnTextPreview(inputNode.parentNode, 120);
  if (own) return own;
  return nodeTextPreview(inputNode.parentNode, 120);
}

function selectLabelText(selectNode) {
  return selectDisplayText(selectNode);
}

function textFieldDisplayText(tag, srcNode) {
  const t = String(tag || '').toLowerCase();
  if (t === 'textarea') {
    const txt = nodeTextPreview(srcNode, 120);
    if (txt) return txt;
    const ph = collapseInlineWhitespace(getAttr(srcNode, 'placeholder'));
    return ph || 'textarea';
  }
  const val = collapseInlineWhitespace(getAttr(srcNode, 'value'));
  if (val) return val;
  const ph = collapseInlineWhitespace(getAttr(srcNode, 'placeholder'));
  if (ph) return ph;
  const typ = String(getAttr(srcNode, 'type') || 'text').toLowerCase();
  return typ === 'password' ? 'password' : 'text';
}

function getBody(doc) {
  const html = (doc.childNodes || []).find((n) => isElement(n) && (n.tagName || n.nodeName) === 'html');
  if (!html) return null;
  return (html.childNodes || []).find((n) => isElement(n) && (n.tagName || n.nodeName) === 'body') || null;
}

function isStructuralTag(tag) {
  return tag === 'body' || tag === 'html' || tag === 'head' || tag === '#document' || tag === '#document-fragment'
    || tag === 'div' || tag === 'span' || tag === 'option'
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

function attrPx(node, name, fallback) {
  const raw = Number(getAttr(node, name));
  if (Number.isFinite(raw) && raw > 0) return raw;
  return Number(fallback || 0);
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

const ROOT_APP_WINDOW_ID = 'root/html_app_window[0]';
const ROOT_STATUSBAR_ID = 'root/statusbar[1]';

function statusbarHeight(themeNodeH) {
  const rowH = Math.max(12, Number(themeNodeH || 16));
  return rowH + 4;
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
    || tag === 'option'
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
  if (tag !== 'summary') {
    for (let i = 0; i < kids.length; i++) collectBlockTree(kids[i], children);
  }
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

function collectOwnText(node, out) {
  if (!isElement(node)) return;
  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i++) {
    const k = kids[i];
    if (isTextNode(k)) {
      if (k.value) out.push(k.value);
      continue;
    }
    if (!isElement(k)) continue;
    const tag = String(k.tagName || k.nodeName || '').toLowerCase();
    if (tag === 'br' || tag === 'wbr') out.push(' ');
  }
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

function nodeOwnTextPreview(node, maxChars = 96) {
  const parts = [];
  collectOwnText(node, parts);
  const joined = collapseInlineWhitespace(parts.join(''));
  if (!joined) return '';
  if (joined.length <= maxChars) return joined;
  return joined.slice(0, Math.max(0, maxChars - 3)) + '...';
}

function dialogTitleText(dialogNode) {
  if (!dialogNode || !Array.isArray(dialogNode.childNodes)) return '';
  const kids = dialogNode.childNodes;
  for (let i = 0; i < kids.length; i++) {
    const k = kids[i];
    if (!isElement(k)) continue;
    const tag = String(k.tagName || k.nodeName || '').toLowerCase();
    if (tag !== 'p' && tag !== 'summary') continue;
    const t = nodeTextPreview(k, 120);
    if (t) return t;
  }
  return nodeTextPreview(dialogNode, 120);
}

function makeYogaTree(node, allBlocks, depth, themeNodeH) {
  const tag = String(node && node.tagName || '').toLowerCase();
  const isHeading = tag === 'h1' || tag === 'h2' || tag === 'h3' || tag === 'h4' || tag === 'h5' || tag === 'h6';
  const isHr = tag === 'hr';
  const isImage = tag === 'img';
  const isTextMinTag = tag === 'p' || tag === 'label' || tag === 'summary' || tag === 'option' || tag === 'legend';
  const srcNode = node && node.srcNode ? node.srcNode : null;
  const ownText = isTextMinTag ? nodeOwnTextPreview(srcNode, 96) : '';
  const childBlocks = blockChildren(node);
  const pWrapsControls = tag === 'p' && childBlocks.length > 0 && !ownText;
  const useTextMinSizing = isTextMinTag && !pWrapsControls;
  const yn = Yoga.Node.create();
  const minNodeSize = Math.max(1, Number(themeNodeH || 16));
  yn.setFlexDirection(Yoga.FLEX_DIRECTION_COLUMN);
  yn.setAlignItems(Yoga.ALIGN_STRETCH);
  if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_STRETCH);
  if (depth > 0 && !pWrapsControls && !isHr && !isHeading && typeof yn.setWidthPercent === 'function') yn.setWidthPercent(100);
  yn.setMinHeight(minNodeSize);
  yn.setPadding(Yoga.EDGE_LEFT, 0);
  yn.setPadding(Yoga.EDGE_RIGHT, 0);
  yn.setPadding(Yoga.EDGE_TOP, 0);
  yn.setPadding(Yoga.EDGE_BOTTOM, 0);

  if (tag === 'scrollbar') {
    yn.setMinHeight(0);
    yn.setHeight(0);
    if (typeof yn.setWidth === 'function') yn.setWidth(0);
  } else if (tag === 'statusbar') {
    if (typeof yn.setWidthPercent === 'function') yn.setWidthPercent(100);
    if (typeof yn.setFlexDirection === 'function') yn.setFlexDirection(Yoga.FLEX_DIRECTION_ROW);
    if (typeof yn.setFlexWrap === 'function') yn.setFlexWrap(Yoga.WRAP_WRAP);
    if (typeof yn.setAlignItems === 'function') yn.setAlignItems(Yoga.ALIGN_FLEX_START);
    if (typeof yn.setAlignContent === 'function') yn.setAlignContent(Yoga.ALIGN_FLEX_START);
    if (typeof yn.setJustifyContent === 'function') yn.setJustifyContent(Yoga.JUSTIFY_FLEX_START);
    const sbH = statusbarHeight(themeNodeH);
    yn.setMinHeight(sbH);
    yn.setHeight(sbH);
  }

  if (pWrapsControls) {
    if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_STRETCH);
    if (typeof yn.setWidthPercent === 'function') yn.setWidthPercent(100);
    if (typeof yn.setMinWidth === 'function') yn.setMinWidth(0);
  }

  if (tag === 'checkbox' || tag === 'radio') {
    const box = tag === 'checkbox'
      ? Math.max(16, minNodeSize)
      : Math.max(8, Math.min(14, minNodeSize - 2));
    if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
    if (typeof yn.setWidth === 'function') yn.setWidth(box);
    if (typeof yn.setHeight === 'function') yn.setHeight(box);
    if (typeof yn.setMinWidth === 'function') yn.setMinWidth(box);
    if (typeof yn.setMinHeight === 'function') yn.setMinHeight(box);
  }
  if (tag === 'select') {
    const label = nodeTextPreview(srcNode, 120) || 'Select';
    const selectW = Math.max(minNodeSize, label.length * 8 + 30);
    const selectH = 18;
    if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
    if (typeof yn.setMeasureFunc === 'function') yn.setMeasureFunc(() => ({ width: selectW, height: selectH }));
    if (typeof yn.setWidth === 'function') yn.setWidth(selectW);
    if (typeof yn.setMinWidth === 'function') yn.setMinWidth(selectW);
    if (typeof yn.setHeight === 'function') yn.setHeight(selectH);
    if (typeof yn.setMinHeight === 'function') yn.setMinHeight(selectH);
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
  if (isHeading) {
    const headingMinW = intrinsicTextMinWidth(srcNode, minNodeSize, 120, false);
    if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
    if (typeof yn.setMinWidth === 'function') yn.setMinWidth(headingMinW);
    if (typeof yn.setWidth === 'function') yn.setWidth(headingMinW);
  }
  if (isImage) {
    const src = node && node.srcNode ? node.srcNode : null;
    const imgW = Math.max(1, attrPx(src, 'width', 64));
    const imgH = Math.max(1, attrPx(src, 'height', 64));
    if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
    if (typeof yn.setMinWidth === 'function') yn.setMinWidth(imgW);
    if (typeof yn.setMinHeight === 'function') yn.setMinHeight(imgH);
    if (typeof yn.setWidth === 'function') yn.setWidth(imgW);
    if (typeof yn.setHeight === 'function') yn.setHeight(imgH);
  }
  if (isHr) {
    const hasCaption = !!hrCaptionText(srcNode);
    if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_STRETCH);
    if (typeof yn.setWidthPercent === 'function') yn.setWidthPercent(100);
    if (typeof yn.setMinWidth === 'function') yn.setMinWidth(0);
    if (typeof yn.setMinHeight === 'function') yn.setMinHeight(hasCaption ? minNodeSize : 1);
    if (typeof yn.setHeight === 'function') yn.setHeight(hasCaption ? minNodeSize : 1);
    if (typeof yn.setMargin === 'function') {
      yn.setMargin(Yoga.EDGE_TOP, 2);
      yn.setMargin(Yoga.EDGE_BOTTOM, 2);
    }
  }
  if (tag === 'details') {
    if (typeof yn.setPadding === 'function') yn.setPadding(Yoga.EDGE_TOP, minNodeSize);
    if (typeof yn.setMinHeight === 'function') yn.setMinHeight(minNodeSize);
  }
  if (tag === 'summary') {
    const label = nodeOwnTextPreview(srcNode, 120);
    const summaryMinW = Math.max(minNodeSize, (label ? label.length * 8 : 0) + 36);
    if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
    if (typeof yn.setMinWidth === 'function') yn.setMinWidth(summaryMinW);
  }
  if (useTextMinSizing && tag !== 'button') {
    const textMinW = intrinsicTextMinWidth(srcNode, minNodeSize, 96, true);
    if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
    if (typeof yn.setMinWidth === 'function') yn.setMinWidth(textMinW);
  }
  if (tag === 'dialog') {
    const src = node && node.srcNode ? node.srcNode : null;
    const title = dialogTitleText(src);
    const titleMinW = Math.max(minNodeSize, title ? (title.length * 8 + 24) : minNodeSize);
    if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
    if (typeof yn.setMinWidth === 'function') yn.setMinWidth(titleMinW);
  }

  allBlocks.push({ node, yoga: yn, depth: depth });
  const kids = childBlocks;
  if (kids.length <= 0) {
    yn.setHeight(Math.max(1, minNodeSize));
    return yn;
  }
  for (let i = 0; i < kids.length; i++) {
    const kid = kids[i];
    const child = makeYogaTree(kid, allBlocks, depth + 1, themeNodeH);
    const parentTag = String(node && node.tagName || '').toLowerCase();
    const childTag = String(kid && kid.tagName || '').toLowerCase();
    const isRootLaneChild = parentTag === 'root' && (childTag === 'statusbar' || childTag === 'scrollbar' || childTag === 'html_app_window');
    if (i > 0 && !isRootLaneChild && typeof child.setMargin === 'function') child.setMargin(Yoga.EDGE_TOP, 1);
    yn.insertChild(child, yn.getChildCount());
  }
  return yn;
}

function isDialogLaneNode(tag, id) {
  return tag === 'dialog' || String(id || '').includes('/dialog[');
}

function computePackedLanes(blocks) {
  const basePackedRects = [];
  const overlayPackedRects = [];
  const entries = [];
  const absXByDepth = [];
  const absYByDepth = [];
  for (let i = 0; i < blocks.length; i++) {
    const e = blocks[i];
    const d = Math.max(0, Number(e.depth || 0));
    const yn = e.yoga;
    let x = Number(yn.getComputedLeft() || 0) + (d > 0 ? Number(absXByDepth[d - 1] || 0) : 0);
    const y = Number(yn.getComputedTop() || 0) + (d > 0 ? Number(absYByDepth[d - 1] || 0) : 0);
    absXByDepth[d] = x;
    absYByDepth[d] = y;
    const tag = String(e.node && e.node.tagName || '').toLowerCase();
    if (tag === 'root' || tag === 'statusbar' || tag === 'scrollbar') continue;
    const minW = tag === 'hr' ? 1 : 2;
    let w = Math.max(minW, Number(yn.getComputedWidth() || 0));
    const minH = tag === 'hr' ? 1 : 2;
    const h = Math.max(minH, Number(yn.getComputedHeight() || 0));
    if (tag === 'hr') {
      const gap = 16;
      const insetW = w - gap * 2;
      if (insetW > 0) {
        x += gap;
        w = insetW;
      }
    }
    const id = String(e.node && e.node.id || '');
    const lane = isDialogLaneNode(tag, id) ? 1 : 0;
    const dst = lane > 0 ? overlayPackedRects : basePackedRects;
    const style = tag === 'hr' ? 7 : 0;
    dst.push(x, y, w, h, d, 0, style);
    entries.push({ id, tag, x, y, w, h, depth: d, lane });
  }
  return { basePackedRects, overlayPackedRects, entries };
}

function appWindowMetrics(appWindowId, entries, scrollY) {
  let appWindowRect = null;
  for (let i = 0; i < entries.length; i++) {
    const r = entries[i];
    if (!r) continue;
    if (String(r.id || '') === appWindowId && String(r.tag || '') === 'html_app_window') {
      appWindowRect = r;
      break;
    }
  }
  if (!appWindowRect) return null;

  const appWindowScrollY = Math.max(0, Number(scrollY || 0));
  const viewportH = Math.max(1, Number(appWindowRect.h || 0));
  let contentBottom = Number(appWindowRect.y || 0) + viewportH;
  for (let i = 0; i < entries.length; i++) {
    const c = entries[i];
    if (!c) continue;
    const cid = String(c.id || '');
    if (!cid || cid === appWindowId) continue;
    if (!cid.startsWith(appWindowId + '/')) continue;
    if (cid.includes('/dialog[')) continue;
    const b = Number(c.y || 0) + Number(c.h || 0) + appWindowScrollY;
    if (b > contentBottom) contentBottom = b;
  }

  const contentH = Math.max(viewportH, contentBottom - Number(appWindowRect.y || 0));
  const maxScroll = Math.max(0, contentH - viewportH);
  const clamped = Math.min(maxScroll, appWindowScrollY);
  return { appWindowRect, viewportH, contentH, maxScroll, scrollY: clamped };
}

function appendStatusbarOverlay(overlayPackedRects, statusbarRect) {
  if (!statusbarRect) return;
  const x = Math.round(Number(statusbarRect.x || 0));
  const y = Math.round(Number(statusbarRect.y || 0));
  const w = Math.max(2, Math.round(Number(statusbarRect.w || 0)));
  const h = Math.max(2, Math.round(Number(statusbarRect.h || 0)));
  const depth = Math.max(0, Number(statusbarRect.depth || 0));
  overlayPackedRects.push(x, y, w, h, depth + 1, 0, 1);
}

function appendScrollbarOverlay(overlayPackedRects, appMetrics, depth) {
  if (!appMetrics || !appMetrics.appWindowRect) return;
  const appWindowRect = appMetrics.appWindowRect;
  const viewportH = Math.max(1, Number(appMetrics.viewportH || 1));
  const contentH = Math.max(viewportH, Number(appMetrics.contentH || viewportH));
  const maxScroll = Math.max(0, Number(appMetrics.maxScroll || 0));
  const clampedScroll = Math.max(0, Number(appMetrics.scrollY || 0));

  const barW = Math.max(4, Number(globalThis.__trueosThemeScrollbarW || 8));
  const x = Math.round(Number(appWindowRect.x || 0));
  const y = Math.round(Number(appWindowRect.y || 0));
  const w = Math.max(4, Math.round(Math.min(barW, Number(appWindowRect.w || 0))));
  const h = Math.max(4, Math.round(Number(appWindowRect.h || 0)));

  const innerX = x + 2;
  const innerY = y + 2;
  const innerW = Math.max(1, w - 4);
  const innerH = Math.max(1, h - 4);

  const ratio = maxScroll <= 0 ? 1 : (viewportH / Math.max(viewportH, contentH));
  const thumbRatio = Math.max(0.2, Math.min(1, ratio));
  const thumbH = maxScroll <= 0 ? innerH : Math.max(1, Math.round(innerH * thumbRatio));
  const thumbTravel = Math.max(0, innerH - thumbH);
  const thumbOff = maxScroll <= 0 ? 0 : Math.round((clampedScroll / maxScroll) * thumbTravel);
  const thumbY = innerY + thumbOff;

  const frameDepth = Math.max(0, Number(depth || 0) + 1);
  overlayPackedRects.push(
    x, y, w, h, frameDepth, 0, 1,
    innerX, thumbY, innerW, thumbH, frameDepth + 1, 0, 2,
  );
}

function collectTextRuns(entries, idToNode) {
  const out = [];
  for (let i = 0; i < entries.length; i++) {
    const r = entries[i];
    const tag = String(r.tag || '').toLowerCase();
    if (!(tag === 'h1' || tag === 'h2' || tag === 'h3' || tag === 'h4' || tag === 'h5' || tag === 'h6' || tag === 'p' || tag === 'button')) continue;
    const n = idToNode.get(String(r.id || ''));
    if (!n || !n.srcNode) continue;
    const t = tag === 'p' ? nodeOwnTextPreview(n.srcNode, 96) : nodeTextPreview(n.srcNode, 96);
    if (!t) continue;
    out.push(
      Math.round(Number(r.x || 0) + (tag === 'button' ? 8 : 2)),
      Math.round(Number(r.y || 0) + 2),
      t,
      Number(r.lane || 0) > 0 ? 1 : 0,
    );
  }
  return out;
}

function computeContentMetrics(entries, viewportH) {
  let contentBottom = 0;
  for (let i = 0; i < entries.length; i++) {
    const e = entries[i];
    if (!e || Number(e.lane || 0) > 0) continue;
    const b = Number(e.y || 0) + Number(e.h || 0);
    if (b > contentBottom) contentBottom = b;
  }
  const vh = Math.max(1, Number(viewportH || 1));
  const contentH = Math.max(vh, contentBottom);
  return {
    contentH,
    viewportH: vh,
    maxScroll: Math.max(0, contentH - vh),
  };
}

function buildLayoutFromHtml(html, viewportW, viewportH, themeNodeH, scrollY) {
  const doc = parse5.parse(String(html || ''));
  const body = getBody(doc) || doc;
  const appChildren = [];
  collectBlockTree(body, appChildren);
  const root = {
    kind: 'block',
    tagName: 'root',
    children: [
      { kind: 'block', tagName: 'html_app_window', children: appChildren, id: '' },
      { kind: 'block', tagName: 'statusbar', children: [], id: '' },
      { kind: 'block', tagName: 'scrollbar', children: [], id: '' },
    ],
    id: 'root',
  };
  assignBlockIds(root, 'root');

  const blocks = [];
  const yogaRoot = makeYogaTree(root, blocks, 0, themeNodeH);
  const vw = Math.max(1, Number(viewportW || 320));
  const vh = Math.max(1, Number(viewportH || 240));
  if (typeof yogaRoot.setWidth === 'function') yogaRoot.setWidth(vw);
  if (typeof yogaRoot.setHeight === 'function') yogaRoot.setHeight(vh);
  const sbH = statusbarHeight(themeNodeH);
  const appWindowH = Math.max(1, vh - sbH);
  for (let i = 0; i < blocks.length; i++) {
    const entry = blocks[i];
    if (!entry || !entry.yoga) continue;
    const tag = String(entry.node && entry.node.tagName || '').toLowerCase();
    if (tag === 'root') continue;
    if (Number(entry.depth || 0) === 1 && typeof entry.yoga.setWidth === 'function') {
      entry.yoga.setWidth(vw);
    }
    if (tag === 'html_app_window' && String(entry.node && entry.node.id || '') === ROOT_APP_WINDOW_ID && typeof entry.yoga.setHeight === 'function') {
      entry.yoga.setHeight(appWindowH);
      if (typeof entry.yoga.setMinHeight === 'function') entry.yoga.setMinHeight(appWindowH);
      if (typeof entry.yoga.setMaxHeight === 'function') entry.yoga.setMaxHeight(appWindowH);
    }
    if (tag === 'statusbar' && String(entry.node && entry.node.id || '') === ROOT_STATUSBAR_ID && typeof entry.yoga.setHeight === 'function') {
      entry.yoga.setHeight(sbH);
      if (typeof entry.yoga.setMinHeight === 'function') entry.yoga.setMinHeight(sbH);
      if (typeof entry.yoga.setMaxHeight === 'function') entry.yoga.setMaxHeight(sbH);
    }
  }
  yogaRoot.calculateLayout(vw, vh, Yoga.DIRECTION_LTR);
  const lanes = computePackedLanes(blocks);

  const idToNode = new Map();
  for (let i = 0; i < blocks.length; i++) {
    const n = blocks[i] && blocks[i].node;
    if (!n) continue;
    idToNode.set(String(n.id || ''), n);
  }
  const textRuns = collectTextRuns(lanes.entries, idToNode);
  let statusbarRect = null;
  let statusbarDepth = 0;
  for (let i = 0; i < blocks.length; i++) {
    const e = blocks[i];
    const tag = String(e && e.node && e.node.tagName || '').toLowerCase();
    const id = String(e && e.node && e.node.id || '');
    if (tag !== 'statusbar' || id !== ROOT_STATUSBAR_ID) continue;
    const d = Math.max(0, Number(e.depth || 0));
    const x = Number(e.yoga.getComputedLeft() || 0);
    const y = Number(e.yoga.getComputedTop() || 0);
    const w = Math.max(2, Number(e.yoga.getComputedWidth() || 0));
    const h = Math.max(2, Number(e.yoga.getComputedHeight() || 0));
    statusbarRect = { x, y, w, h, depth: d };
    statusbarDepth = d;
    break;
  }
  if (!statusbarRect) {
    statusbarRect = { x: 0, y: appWindowH, w: vw, h: sbH, depth: 1 };
    statusbarDepth = 1;
  }
  appendStatusbarOverlay(lanes.overlayPackedRects, statusbarRect);

  const metrics = appWindowMetrics(ROOT_APP_WINDOW_ID, lanes.entries, scrollY);
  appendScrollbarOverlay(lanes.overlayPackedRects, metrics, statusbarDepth);
  const contentMetrics = metrics
    ? { contentH: metrics.contentH, viewportH: metrics.viewportH, maxScroll: metrics.maxScroll }
    : computeContentMetrics(lanes.entries, appWindowH);
  return {
    basePackedRects: lanes.basePackedRects,
    overlayPackedRects: lanes.overlayPackedRects,
    textRuns,
    contentMetrics,
    viewportW: vw,
    viewportH: vh,
  };
}

function post(msg) {
  parentPort.postMessage(JSON.stringify(msg));
}

let cache = null;

parentPort.onMessage((raw) => {
  let msg = null;
  try {
    msg = JSON.parse(String(raw || ''));
  } catch (_) {
    return;
  }
  if (!msg || msg.type !== 'requestFrame') return;

  const t0 = Date.now();
  const reqId = Number(msg.reqId || 0) | 0;
  const seq = Number(msg.seq || 0) | 0;
  const blockId = String(msg.blockId || '');
  const reason = String(msg.reason || 'request');
  const html = String(msg.html || '');
  const viewport = msg.viewport || {};
  const viewportW = Math.max(1, Number(viewport.w || msg.viewportW || 320));
  const viewportH = Math.max(1, Number(viewport.h || msg.viewportH || 240));
  const scroll = msg.scroll || {};
  const scrollY = Math.max(0, Number(scroll.y || 0));
  const themeNodeH = Math.max(1, Number(msg.themeNodeH || 16));
  if (!reqId || !blockId || !html) {
    post({
      type: 'frameReady',
      protocol: 'iframeFrameV1',
      reqId,
      seq,
      blockId,
      viewport: { w: viewportW, h: viewportH },
      payload: {
        basePackedRects: [],
        overlayPackedRects: [],
        textRuns: [],
        contentMetrics: { contentH: viewportH, viewportH, maxScroll: 0 },
        dirtyRects: [0, 0, viewportW, viewportH],
      },
      timing: { buildMs: 0, totalMs: Date.now() - t0, reusedLayout: 0, reason },
    });
    return;
  }

  try {
    const cacheHit = !!cache
      && String(cache.html || '') === html
      && Number(cache.viewportW || 0) === viewportW
      && Number(cache.viewportH || 0) === viewportH
      && Number(cache.scrollY || 0) === scrollY
      && Number(cache.themeNodeH || 0) === themeNodeH;
    const out = cacheHit
      ? cache.layout
      : buildLayoutFromHtml(html, viewportW, viewportH, themeNodeH, scrollY);
    if (!cacheHit) {
      cache = {
        html,
        viewportW,
        viewportH,
        scrollY,
        themeNodeH,
        layout: out,
      };
    }

    const dirtyRects = (reason === 'scroll' && cacheHit)
      ? []
      : [0, 0, out.viewportW, out.viewportH];
    post({
      type: 'frameReady',
      protocol: 'iframeFrameV1',
      reqId,
      seq,
      blockId,
      viewport: { w: out.viewportW, h: out.viewportH },
      payload: {
        basePackedRects: out.basePackedRects,
        overlayPackedRects: out.overlayPackedRects,
        textRuns: out.textRuns,
        contentMetrics: out.contentMetrics,
        dirtyRects,
      },
      timing: {
        buildMs: cacheHit ? 0 : (Date.now() - t0),
        totalMs: Date.now() - t0,
        reusedLayout: cacheHit ? 1 : 0,
        reason,
      },
    });
  } catch (_) {
    post({
      type: 'frameReady',
      protocol: 'iframeFrameV1',
      reqId,
      seq,
      blockId,
      viewport: { w: viewportW, h: viewportH },
      payload: {
        basePackedRects: [],
        overlayPackedRects: [],
        textRuns: [],
        contentMetrics: { contentH: viewportH, viewportH, maxScroll: 0 },
        dirtyRects: [0, 0, viewportW, viewportH],
      },
      timing: { buildMs: 0, totalMs: Date.now() - t0, reusedLayout: 0, reason },
    });
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

function iframeViewportClass(vw, vh) {
  const w = Math.max(1, Number(vw || 0));
  const h = Math.max(1, Number(vh || 0));
  if (w <= 320 || h <= 220) return 'xs';
  if (w <= 640 || h <= 360) return 'sm';
  if (w <= 1024 || h <= 640) return 'md';
  return 'lg';
}

function sanitizePackedLane(raw) {
  if (!Array.isArray(raw)) return [];
  const out = [];
  for (let i = 0; i + 6 < raw.length; i += 7) {
    out.push(
      Number(raw[i + 0] || 0),
      Number(raw[i + 1] || 0),
      Math.max(1, Number(raw[i + 2] || 1)),
      Math.max(1, Number(raw[i + 3] || 1)),
      Math.max(0, Number(raw[i + 4] || 0)),
      Number(raw[i + 5] || 0),
      Math.max(0, Number(raw[i + 6] || 0)),
    );
  }
  return out;
}

function sanitizeTextRuns(raw) {
  if (!Array.isArray(raw)) return [];
  const out = [];
  if (raw.length >= 4) {
    for (let i = 0; i + 3 < raw.length; i += 4) {
      const text = String(raw[i + 2] || '');
      if (!text) continue;
      out.push(
        Number(raw[i + 0] || 0),
        Number(raw[i + 1] || 0),
        text,
        Number(raw[i + 3] || 0) > 0 ? 1 : 0,
      );
    }
    return out;
  }
  for (let i = 0; i + 2 < raw.length; i += 3) {
    const text = String(raw[i + 2] || '');
    if (!text) continue;
    out.push(Number(raw[i + 0] || 0), Number(raw[i + 1] || 0), text, 0);
  }
  return out;
}

function sanitizeDirtyRects(raw, viewportW, viewportH) {
  if (!Array.isArray(raw) || raw.length <= 0) {
    return [0, 0, Math.max(1, Number(viewportW || 1)), Math.max(1, Number(viewportH || 1))];
  }
  const out = [];
  for (let i = 0; i + 3 < raw.length; i += 4) {
    out.push(
      Number(raw[i + 0] || 0),
      Number(raw[i + 1] || 0),
      Math.max(1, Number(raw[i + 2] || 1)),
      Math.max(1, Number(raw[i + 3] || 1)),
    );
  }
  return out;
}

function sanitizeContentMetrics(raw, viewportH) {
  const vh = Math.max(1, Number(viewportH || 1));
  const contentH = Math.max(vh, Number(raw && raw.contentH || vh));
  const maxScroll = Math.max(0, Number(raw && raw.maxScroll || (contentH - vh)));
  return { contentH, viewportH: vh, maxScroll };
}

function iframeRuntimeHasPendingReason(runtime, reason) {
  if (!runtime || !runtime.pending) return false;
  for (const p of runtime.pending.values()) {
    if (String(p && p.reason || '') === String(reason || '')) return true;
  }
  return false;
}

function ensureIframeRuntime(blockId) {
  const id = String(blockId || '');
  if (!id) return null;
  let runtime = iframeRuntimeByBlockId.get(id);
  if (runtime) return runtime;

  runtime = {
    blockId: id,
    worker: null,
    started: false,
    nextReqId: 1,
    pending: new Map(),
    seq: 0,
    lastReadySeq: 0,
    droppedFrames: 0,
    lastWheelAtMs: 0,
  };
  iframeRuntimeByBlockId.set(id, runtime);

  if (typeof Worker !== 'function') return runtime;
  runtime.started = true;
  try {
    const w = new Worker(IFRAME_IMPORT_WORKER_CODE);
    runtime.worker = w;
    if (typeof w.onMessage === 'function') {
      w.onMessage((raw) => onIframeWorkerMessage(id, raw));
    }
  } catch (_) {
    runtime.worker = null;
  }
  return runtime;
}

function destroyIframeRuntime(blockId) {
  const id = String(blockId || '');
  if (!id) return;
  const runtime = iframeRuntimeByBlockId.get(id);
  if (runtime && runtime.worker && typeof runtime.worker.terminate === 'function') {
    try {
      runtime.worker.terminate();
    } catch (_) {}
  }
  iframeRuntimeByBlockId.delete(id);
  iframeSpecByBlockId.delete(id);
  iframeRenderByBlockId.delete(id);
  iframeScrollById.delete(id);
  iframeTraceByBlockId.delete(id);
  if (iframeFocusState.activeId === id) iframeFocusState.activeId = '';
}

function onIframeWorkerMessage(blockId, raw) {
  let msg = null;
  try {
    msg = JSON.parse(String(raw || ''));
  } catch (_) {
    return;
  }
  if (!msg || typeof msg !== 'object') return;
  if (msg.type === 'ready') return;
  if (msg.type !== 'frameReady') return;
  if (String(msg.protocol || '') !== IFRAME_FRAME_PROTOCOL) return;

  const id = String(blockId || msg.blockId || '');
  if (!id) return;
  const runtime = iframeRuntimeByBlockId.get(id);
  if (!runtime) return;

  const reqId = Number(msg.reqId || 0) | 0;
  if (!reqId) return;
  const pending = runtime.pending.get(reqId);
  if (!pending) return;
  runtime.pending.delete(reqId);

  const seq = Math.max(0, Number(msg.seq || pending.seq || 0));
  if (seq < runtime.lastReadySeq) {
    runtime.droppedFrames += 1;
    return;
  }

  const viewport = msg.viewport || {};
  const viewportW = Math.max(1, Number(viewport.w || pending.viewportW || 320));
  const viewportH = Math.max(1, Number(viewport.h || pending.viewportH || 240));
  const payload = msg.payload || {};
  const frame = {
    protocol: IFRAME_FRAME_PROTOCOL,
    seq,
    basePackedRects: sanitizePackedLane(payload.basePackedRects),
    overlayPackedRects: sanitizePackedLane(payload.overlayPackedRects),
    textRuns: sanitizeTextRuns(payload.textRuns),
    contentMetrics: sanitizeContentMetrics(payload.contentMetrics, viewportH),
    dirtyRects: sanitizeDirtyRects(payload.dirtyRects, viewportW, viewportH),
    viewportW,
    viewportH,
  };
  runtime.lastReadySeq = seq;
  iframeRenderByBlockId.set(id, frame);

  const now = Date.now();
  const timing = msg.timing || {};
  const totalMs = Math.max(0, Number(timing.totalMs || 0));
  const scrollLatencyMs = String(pending.reason || '') === 'scroll' ? Math.max(0, now - Number(pending.sentAtMs || now)) : 0;
  iframeTraceByBlockId.set(id, {
    seq,
    buildMs: Math.max(0, Number(timing.buildMs || 0)),
    totalMs,
    dirtyCount: Math.floor(frame.dirtyRects.length / 4),
    droppedFrames: Math.max(0, Number(runtime.droppedFrames || 0)),
    scrollLatencyMs,
    reason: String(timing.reason || pending.reason || ''),
    receivedAtMs: now,
  });
  relayoutAndPaint();
}

function requestIframeFrameInternal(blockId, opts) {
  const id = String(blockId || '');
  if (!id) return false;
  const spec = iframeSpecByBlockId.get(id) || {};
  const cfg = opts && typeof opts === 'object' ? opts : {};
  const html = String(cfg.srcdoc != null ? cfg.srcdoc : spec.srcdoc || '');
  if (!html) return false;
  const viewportW = Math.max(1, Number(cfg.viewportW || spec.viewportW || 320));
  const viewportH = Math.max(1, Number(cfg.viewportH || spec.viewportH || 240));
  const reason = String(cfg.reason || 'request');
  const invalidations = Array.isArray(cfg.invalidations) ? cfg.invalidations : [];

  const runtime = ensureIframeRuntime(id);
  if (!runtime || !runtime.worker || typeof runtime.worker.postMessage !== 'function') return false;
  const reqId = (runtime.nextReqId++) | 0;
  if (!reqId) return false;
  const seq = (runtime.seq = (runtime.seq + 1) | 0);
  const sentAtMs = Date.now();
  runtime.pending.set(reqId, {
    seq,
    reason,
    sentAtMs,
    viewportW,
    viewportH,
  });

  try {
    runtime.worker.postMessage(JSON.stringify({
      type: 'requestFrame',
      protocol: IFRAME_FRAME_PROTOCOL,
      reqId,
      seq,
      blockId: id,
      reason,
      html,
      viewport: { w: viewportW, h: viewportH },
      scroll: { y: Math.max(0, Number(iframeScrollById.get(id) || 0)) },
      invalidations,
      themeNodeH: Math.max(1, Number(G.__trueosThemeNodeH || 16)),
    }));
    return true;
  } catch (_) {
    runtime.pending.delete(reqId);
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
  const seen = new Set();

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
    const id = String(node.id || '');
    if (!id) continue;
    seen.add(id);

    const srcW = Math.max(1, Number(getAttr(node.srcNode, 'width') || 320));
    const srcH = Math.max(1, Number(getAttr(node.srcNode, 'height') || 240));
    const sizeClass = iframeViewportClass(srcW, srcH);
    const prev = iframeSpecByBlockId.get(id);
    const mustRebuild = !prev
      || String(prev.srcdoc || '') !== srcdoc
      || String(prev.sizeClass || '') !== sizeClass;
    iframeSpecByBlockId.set(id, { srcdoc, viewportW: srcW, viewportH: srcH, sizeClass });

    const hasFrame = iframeRenderByBlockId.has(id);
    if (!hasFrame || mustRebuild) {
      requestIframeFrameInternal(id, {
        srcdoc,
        viewportW: srcW,
        viewportH: srcH,
        reason: mustRebuild ? 'rebuild' : 'init',
        invalidations: mustRebuild ? ['srcdoc-or-size-class-change'] : ['initial-frame'],
      });
    }
  }

  const knownIds = Array.from(iframeSpecByBlockId.keys());
  for (let i = 0; i < knownIds.length; i++) {
    const id = knownIds[i];
    if (seen.has(id)) continue;
    destroyIframeRuntime(id);
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

function preferredTableColumnWidths(rows, cols) {
  const out = new Array(cols).fill(16);
  for (let r = 0; r < rows.length; r++) {
    const cells = tableCellsFromRow(rows[r]);
    for (let c = 0; c < cols; c++) {
      const cell = c < cells.length ? cells[c] : null;
      const txt = nodeTextPreview(cell, 120);
      if (!txt) continue;
      out[c] = Math.max(out[c], txt.length * 8 + 10);
    }
  }
  return out;
}

function fitTableColumnWidths(totalW, preferred) {
  const cols = preferred.length;
  if (cols <= 0) return [];
  const safeW = Math.max(cols, Math.round(Number(totalW || cols)));
  const pref = preferred.map((w) => Math.max(1, Math.round(Number(w || 1))));
  const prefSum = pref.reduce((a, b) => a + b, 0) || cols;
  const widths = pref.map((w) => Math.max(1, Math.floor((safeW * w) / prefSum)));
  let used = widths.reduce((a, b) => a + b, 0);
  let rem = safeW - used;
  for (let i = 0; rem > 0 && i < cols; i++, rem--) widths[i % cols] += 1;
  return widths;
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
  if (tag !== 'summary') {
    for (let i = 0; i < kids.length; i++) {
      collectBlockTree(kids[i], children);
    }
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
  const rootStatusbar = { kind: 'block', tagName: 'statusbar', children: [], id: '' };
  const rootScrollbar = { kind: 'block', tagName: 'scrollbar', children: [], id: '' };
  const root = { kind: 'block', tagName: 'root', children: [appWindow, rootStatusbar, rootScrollbar], id: 'root' };
  assignBlockIds(root, 'root');
  importIframeAppsOnce(root);
  importSvgAssetsOnce(root);
  return root;
}

function assignBlockIds(node, path) {
  if (!node || node.kind !== 'block') return;
  node.id = path;
  blockNodeById.set(path, node);
  if (String(node.tagName || '').toLowerCase() === 'html_app_window' && !minimizedHtmlAppWindowById.has(path)) {
    const src = node && node.srcNode ? node.srcNode : null;
    const rawTag = String(src && (src.tagName || src.nodeName) || '').toLowerCase();
    const attrMin = parseMinimizedAttr(src);
    const defaultMin = path !== HTML_APP_WINDOW_ROOT_ID && rawTag === 'iframe';
    minimizedHtmlAppWindowById.set(path, attrMin || defaultMin);
  }
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

function isSelectTag(tag) {
  return tag === 'select';
}

function isIframeBackedHtmlAppWindow(node) {
  if (!node) return false;
  if (String(node.tagName || '').toLowerCase() !== 'html_app_window') return false;
  const rawTag = String(node.srcNode && (node.srcNode.tagName || node.srcNode.nodeName) || '').toLowerCase();
  return rawTag === 'iframe';
}

function isEmbeddedIframeHtmlAppWindow(node) {
  if (!isIframeBackedHtmlAppWindow(node)) return false;
  const id = String(node && node.id || '');
  return !!id && id !== HTML_APP_WINDOW_ROOT_ID;
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
  const visibleKids = [];
  for (let i = 0; i < kids.length; i++) {
    const child = kids[i];
    if (String(child && child.tagName || '').toLowerCase() === 'html_app_window' && isHtmlAppWindowMinimized(child.id)) {
      continue;
    }
    visibleKids.push(child);
  }
  const tag = String(node && node.tagName || '').toLowerCase();
  if (tag !== 'details') return visibleKids;

  const contentKids = [];
  for (let i = 0; i < visibleKids.length; i++) {
    const childTag = String(visibleKids[i] && visibleKids[i].tagName || '').toLowerCase();
    if (childTag === 'summary') continue;
    contentKids.push(visibleKids[i]);
  }

  const open = isDetailsOpen(node);
  if (open) return contentKids;
  return [];
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
  const isHr = tag === 'hr';
  const isImage = tag === 'img';
  const isHeading = HEADING_TEXT_TAGS.has(tag);
  const isCompact = isCompactWidgetTag(tag);
  const isButton = isButtonTag(tag);
  const isSvg = isSvgTag(tag);
  const isTable = isTableTag(tag);
  const isRadio = isRadioTag(tag);
  const isSelect = isSelectTag(tag);
  const isStatusbar = isStatusbarTag(tag);
  const isTemporal = isTemporalTag(tag);
  const isRangeWidget = isRangeWidgetTag(tag);
  const isIframeWindow = isIframeBackedHtmlAppWindow(node);
  const isTextField = tag === 'input' || tag === 'textarea';
  const isTextMinTag = tag === 'p' || tag === 'label' || tag === 'summary' || tag === 'option' || tag === 'legend';
  const srcNode = node && node.srcNode ? node.srcNode : null;
  const ownText = isTextMinTag ? nodeOwnTextPreview(srcNode, 96) : '';
  const childBlocks = blockChildren(node);
  const pWrapsControls = tag === 'p' && childBlocks.length > 0 && !ownText;
  const useTextMinSizing = isTextMinTag && !pWrapsControls;
  const nodeId = String(node && node.id || '');
  let hasExplicitLeafHeight = false;
  const yn = Yoga.Node.create();
  yn.setFlexDirection(Yoga.FLEX_DIRECTION_COLUMN);
  yn.setAlignItems(Yoga.ALIGN_STRETCH);
  if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_STRETCH);
  if (!isHeading && !isCompact && !isButton && !isSvg && !isTable && !isRadio && !isSelect && !isTemporal && !isRangeWidget && !isImage && !isHr && (!useTextMinSizing) && !isTextField && !pWrapsControls && depth > 0 && typeof yn.setWidthPercent === 'function') yn.setWidthPercent(100);
  yn.setPadding(Yoga.EDGE_LEFT, 0);
  yn.setPadding(Yoga.EDGE_RIGHT, 0);
  yn.setPadding(Yoga.EDGE_TOP, 0);
  yn.setPadding(Yoga.EDGE_BOTTOM, 0);
  if (isScrollbarTag(tag)) {
    yn.setMinHeight(0);
    yn.setHeight(0);
    if (typeof yn.setWidth === 'function') yn.setWidth(0);
  } else if (isStatusbar) {
    if (typeof yn.setWidthPercent === 'function') yn.setWidthPercent(100);
    if (typeof yn.setFlexDirection === 'function') yn.setFlexDirection(Yoga.FLEX_DIRECTION_ROW);
    if (typeof yn.setFlexWrap === 'function') yn.setFlexWrap(Yoga.WRAP_WRAP);
    if (typeof yn.setAlignItems === 'function') yn.setAlignItems(Yoga.ALIGN_FLEX_START);
    if (typeof yn.setAlignContent === 'function') yn.setAlignContent(Yoga.ALIGN_FLEX_START);
    if (typeof yn.setJustifyContent === 'function') yn.setJustifyContent(Yoga.JUSTIFY_FLEX_START);
    const rowH = Math.max(12, Number(G.__trueosThemeNodeH || 16));
    const baseH = rowH + 4;
    yn.setMinHeight(baseH);
    yn.setHeight(baseH);
  } else {
    const minNodeSize = Math.max(1, Number(G.__trueosThemeNodeH || 16));
    yn.setMinHeight(minNodeSize);
    let minWidth = isInHtmlAppWindowSubtree(nodeId)
      ? Math.max(minNodeSize, htmlAppWindowMinWidthPx())
      : minNodeSize;
    if (pWrapsControls) minWidth = 0;
    if (isIframeWindow) {
      const src = node && node.srcNode ? node.srcNode : null;
      const iframeW = Math.max(1, attrPx(src, 'width', 320));
      const iframeH = Math.max(1, attrPx(src, 'height', 240));
      if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
      if (typeof yn.setWidth === 'function') yn.setWidth(iframeW);
      if (typeof yn.setMinWidth === 'function') yn.setMinWidth(iframeW);
      if (typeof yn.setMaxWidth === 'function') yn.setMaxWidth(iframeW);
      if (typeof yn.setHeight === 'function') yn.setHeight(iframeH);
      if (typeof yn.setMinHeight === 'function') yn.setMinHeight(iframeH);
      if (typeof yn.setMaxHeight === 'function') yn.setMaxHeight(iframeH);
      minWidth = iframeW;
      hasExplicitLeafHeight = true;
    }
    if (isHeading) {
      const headingMinW = intrinsicTextMinWidth(srcNode, minNodeSize, 120, false);
      minWidth = headingMinW;
      if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
      if (typeof yn.setMinWidth === 'function') yn.setMinWidth(headingMinW);
      if (typeof yn.setWidth === 'function') yn.setWidth(headingMinW);
    }
    if (useTextMinSizing && !isButton && !isSvg && !isTable && !isTemporal && !isRangeWidget && !isIframeWindow) {
      const textMinW = intrinsicTextMinWidth(srcNode, minWidth, 96, true);
      minWidth = textMinW;
      if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
    }
    if (isTextField) {
      const src = node && node.srcNode ? node.srcNode : null;
      const txt = textFieldDisplayText(tag, src);
      const fieldW = Math.max(84, Math.min(420, txt.length * 8 + 20));
      const fieldH = Math.max(minNodeSize + 2, 18);
      if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
      if (typeof yn.setWidth === 'function') yn.setWidth(fieldW);
      if (typeof yn.setMinWidth === 'function') yn.setMinWidth(fieldW);
      if (typeof yn.setMaxWidth === 'function') yn.setMaxWidth(fieldW);
      if (typeof yn.setHeight === 'function') yn.setHeight(fieldH);
      if (typeof yn.setMinHeight === 'function') yn.setMinHeight(fieldH);
      minWidth = fieldW;
      hasExplicitLeafHeight = true;
    }
    if (isImage) {
      const src = node && node.srcNode ? node.srcNode : null;
      const imgW = Math.max(1, attrPx(src, 'width', 64));
      const imgH = Math.max(1, attrPx(src, 'height', 64));
      if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
      if (typeof yn.setMinWidth === 'function') yn.setMinWidth(imgW);
      if (typeof yn.setMinHeight === 'function') yn.setMinHeight(imgH);
      if (typeof yn.setWidth === 'function') yn.setWidth(imgW);
      if (typeof yn.setHeight === 'function') yn.setHeight(imgH);
      minWidth = imgW;
      hasExplicitLeafHeight = true;
    }
    if (isHr) {
      const hasCaption = !!hrCaptionText(srcNode);
      if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_STRETCH);
      if (typeof yn.setWidthPercent === 'function') yn.setWidthPercent(100);
      if (typeof yn.setMinWidth === 'function') yn.setMinWidth(0);
      if (typeof yn.setMinHeight === 'function') yn.setMinHeight(hasCaption ? minNodeSize : 1);
      if (typeof yn.setHeight === 'function') yn.setHeight(hasCaption ? minNodeSize : 1);
      if (typeof yn.setMargin === 'function') {
        yn.setMargin(Yoga.EDGE_TOP, 2);
        yn.setMargin(Yoga.EDGE_BOTTOM, 2);
      }
      minWidth = 0;
      hasExplicitLeafHeight = true;
    }
    if (tag === 'details') {
      if (typeof yn.setPadding === 'function') yn.setPadding(Yoga.EDGE_TOP, minNodeSize);
      if (typeof yn.setMinHeight === 'function') yn.setMinHeight(minNodeSize);
      const detailsLabel = detailsSummaryText(srcNode);
      const detailsMinW = Math.max(minWidth, (detailsLabel ? detailsLabel.length * 8 : 0) + 36);
      if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
      if (typeof yn.setMinWidth === 'function') yn.setMinWidth(detailsMinW);
      minWidth = detailsMinW;
    }
    if (tag === 'summary') {
      const src = node && node.srcNode ? node.srcNode : null;
      const label = summaryLabelText(src);
      const summaryMinW = Math.max(minWidth, (label ? label.length * 8 : 0) + 36);
      if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
      if (typeof yn.setMinWidth === 'function') yn.setMinWidth(summaryMinW);
      minWidth = summaryMinW;
    }
    if (typeof yn.setMinWidth === 'function') yn.setMinWidth(minWidth);
    if (pWrapsControls) {
      if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_STRETCH);
      if (typeof yn.setWidthPercent === 'function') yn.setWidthPercent(100);
      if (typeof yn.setMinWidth === 'function') yn.setMinWidth(0);
    }
    if (isCompact) {
      const box = Math.max(16, minNodeSize);
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
      const prefCols = cols > 0 ? preferredTableColumnWidths(rows, cols) : [];
      const prefW = prefCols.length > 0 ? prefCols.reduce((a, b) => a + b, 0) : minWidth;
      const tableMinW = Math.max(minWidth, prefW);
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
    if (isSelect) {
      applyYogaDefaultsSelectWidget(yn, Yoga, tag, node && node.srcNode ? node.srcNode : null);
      hasExplicitLeafHeight = true;
    }
    if (tag === 'dialog') {
      const src = node && node.srcNode ? node.srcNode : null;
      const title = dialogTitleText(src);
      const titleMinW = Math.max(minWidth, title ? (title.length * 8 + 24) : minWidth);
      if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_FLEX_START);
      if (typeof yn.setMinWidth === 'function') yn.setMinWidth(titleMinW);
      minWidth = titleMinW;
    }
  }

  allBlocks.push({ node, yoga: yn, depth });

  const kids = visibleChildrenForNode(node);
  if (kids.length <= 0) {
    if (!hasExplicitLeafHeight) yn.setHeight(Math.max(1, Number(G.__trueosThemeNodeH || 16)));
    return yn;
  }

  for (let i = 0; i < kids.length; i++) {
    const kid = kids[i];
    const child = makeYogaTree(kid, allBlocks, depth + 1);
    // Keep synthetic root lanes tightly packed: app window, statusbar, scrollbar.
    const parentTag = String(node && node.tagName || '').toLowerCase();
    const childTag = String(kid && kid.tagName || '').toLowerCase();
    const isRootLaneChild = parentTag === 'root' && (childTag === 'statusbar' || childTag === 'scrollbar' || childTag === 'html_app_window');
    if (i > 0 && !isRootLaneChild && typeof child.setMargin === 'function') child.setMargin(Yoga.EDGE_TOP, 1);
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
    const hrCaption = tag === 'hr'
      ? hrCaptionText(entry && entry.node && entry.node.srcNode ? entry.node.srcNode : null)
      : '';
    const nodeId = String(entry && entry.node && entry.node.id || '');
    const isEmbeddedIframeWindow = isEmbeddedIframeHtmlAppWindow(entry && entry.node ? entry.node : null);
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

    // Keep normal UI content aligned to Yoga flow; depth-based x-indent made many blocks look detached.
    const drawIndent = 0;
    let x = absX + drawIndent;
    let y = absY;
    const minRectW = tag === 'hr'
      ? 1
      : (isInHtmlAppWindowSubtree(nodeId) ? htmlAppWindowMinWidthPx() : 2);
    let w = Math.max(minRectW, Number(yn.getComputedWidth() || 0) - drawIndent);
    const minRectH = tag === 'hr' ? 1 : 2;
    let h = Math.max(minRectH, Number(yn.getComputedHeight() || 0));
    if (tag === 'hr') {
      const gap = 16;
      const insetW = w - gap * 2;
      if (insetW > 0) {
        x += gap;
        w = insetW;
      }
    }
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
    } else if (isEmbeddedIframeWindow) {
      // Embedded html_app_window keeps normal flow/scroll position but renders in overlay z-order.
      drawDepth = Math.max(depth, 24);
    }
    if (nodeId && isInHtmlAppWindowSubtree(nodeId)) {
      const ownerAppId = htmlAppWindowAncestorId(nodeId) || HTML_APP_WINDOW_ROOT_ID;
      if (ownerAppId && nodeId !== ownerAppId) {
        const appRect = rectById.get(ownerAppId);
        if (appRect) {
          const inner = iframeInnerRectFromParentRect(appRect);
          if (tag === 'hr') {
            const gap = 16;
            x = inner.x + gap;
            w = Math.max(1, inner.w - gap * 2);
          }
          const clipped = rectIntersection(x, y, w, h, inner.x, inner.y, inner.w, inner.h);
          if (!clipped) continue;
          x = clipped.x;
          y = clipped.y;
          w = clipped.w;
          h = clipped.h;
        }
      }
    }
    const scrollable = isScrollableTag(tag) ? 1 : 0;
    const style = tag === 'hr' ? 7 : 0;
    if (!isScrollbarTag(tag)) {
      if (tag === 'hr' && hrCaption) {
        const captionW = Math.max(1, hrCaption.length * 8);
        const centerGap = 16;
        const capX = Math.round(x + (w - captionW) * 0.5);
        const leftEnd = capX - centerGap;
        const rightStart = capX + captionW + centerGap;
        const leftW = leftEnd - x;
        const rightW = (x + w) - rightStart;
        if (leftW > 0) out.push(x, y, leftW, h, drawDepth, scrollable, style);
        if (rightW > 0) out.push(rightStart, y, rightW, h, drawDepth, scrollable, style);
        if (leftW <= 0 && rightW <= 0) out.push(x, y, w, h, drawDepth, scrollable, style);
      } else {
        out.push(x, y, w, h, drawDepth, scrollable, style);
      }
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
let fpsTaskTicker = null;
const fpsTaskStats = {
  framesSinceSample: 0,
  lastSampleMs: 0,
  avgFps1s: 0,
};
let viewportW = 1280;
let viewportH = 800;

function isAvgFpsTaskNode(blockNode) {
  if (!blockNode || !blockNode.srcNode) return false;
  if (String(blockNode.tagName || '').toLowerCase() !== 'p') return false;
  const task = String(getAttr(blockNode.srcNode, 'data-fps-task') || '').toLowerCase();
  return task === 'avg1s';
}

function hasAvgFpsTaskNode() {
  for (let i = 0; i < blocks.length; i++) {
    const entry = blocks[i];
    if (!entry || !entry.node) continue;
    if (!isAvgFpsTaskNode(entry.node)) continue;
    return true;
  }
  return false;
}

function avgFpsTaskText() {
  const fps = Math.max(0, Number(fpsTaskStats.avgFps1s || 0));
  if (!(fps > 0)) return 'Average FPS (1s): --';
  return `Average FPS (1s): ${fps.toFixed(1)}`;
}

function stopFpsTaskTicker() {
  if (!fpsTaskTicker) return;
  clearInterval(fpsTaskTicker);
  fpsTaskTicker = null;
}

function ensureFpsTaskTicker() {
  if (!hasAvgFpsTaskNode()) {
    stopFpsTaskTicker();
    return;
  }
  if (fpsTaskTicker) return;
  fpsTaskStats.framesSinceSample = 0;
  fpsTaskStats.lastSampleMs = Date.now();
  fpsTaskTicker = setInterval(() => {
    const now = Date.now();
    const last = Number(fpsTaskStats.lastSampleMs || now);
    const dtMs = Math.max(1, now - last);
    const frames = Math.max(0, Number(fpsTaskStats.framesSinceSample || 0));
    fpsTaskStats.avgFps1s = (frames * 1000) / dtMs;
    fpsTaskStats.framesSinceSample = 0;
    fpsTaskStats.lastSampleMs = now;
    relayoutAndPaint();
  }, 1000);
}

function noteRenderedFrameForFpsTask() {
  if (!fpsTaskTicker) return;
  fpsTaskStats.framesSinceSample += 1;
}

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
  // Cursor-plane custom GPU commands are intentionally disabled.
  // Keep cursor state tracking active, but do not emit draw calls here.
  if (!cursorPlane.enabled) return false;
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
    // Position logic stays live via kernel cursor sampling.
    const _changed = refreshCursorPlaneFromKernel(cursorPlane.maxPointers);
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

function iframeMetricsForBlock(blockId) {
  const id = String(blockId || '');
  if (!id) return null;
  const snap = iframeRenderByBlockId.get(id);
  if (!snap || !snap.contentMetrics) return null;
  const viewportH = Math.max(1, Number(snap.contentMetrics.viewportH || snap.viewportH || 1));
  const contentH = Math.max(viewportH, Number(snap.contentMetrics.contentH || viewportH));
  const maxScroll = Math.max(0, Number(snap.contentMetrics.maxScroll || (contentH - viewportH)));
  const scrollY = Math.min(maxScroll, Math.max(0, iframeScrollOffsetFor(id)));
  return { contentH, viewportH, maxScroll, scrollY };
}

function setIframeScrollInternal(blockId, scrollY, repaint) {
  const id = String(blockId || '');
  if (!id) return false;
  const metrics = iframeMetricsForBlock(id);
  const maxScroll = Math.max(0, Number(metrics && metrics.maxScroll || 0));
  const next = Math.max(0, Math.min(maxScroll, Number(scrollY || 0)));
  const prev = Math.max(0, Number(iframeScrollById.get(id) || 0));
  if (Math.abs(prev - next) < 0.5) return false;
  iframeScrollById.set(id, next);

  const runtime = iframeRuntimeByBlockId.get(id);
  if (runtime) runtime.lastWheelAtMs = Date.now();
  if (runtime && !iframeRuntimeHasPendingReason(runtime, 'scroll')) {
    requestIframeFrameInternal(id, { reason: 'scroll', invalidations: ['scroll-delta'] });
  }
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

  const statusbarH = statusbarTargetHeightPx(vw);
  const appWindowH = Math.max(1, vh - statusbarH);

  for (let i = 0; i < blocks.length; i++) {
    const entry = blocks[i];
    if (!entry || !entry.yoga) continue;
    const tag = String(entry.node && entry.node.tagName || '').toLowerCase();
    if (tag === 'root') continue;
    // Document-level width source: only direct children of the synthetic root.
    if (Number(entry.depth || 0) === 1 && typeof entry.yoga.setWidth === 'function') {
      entry.yoga.setWidth(vw);
    }
    // Keep app window above the dedicated bottom statusbar lane.
    if (tag === 'html_app_window' && String(entry.node && entry.node.id || '') === HTML_APP_WINDOW_ROOT_ID && typeof entry.yoga.setHeight === 'function') {
      entry.yoga.setHeight(appWindowH);
      if (typeof entry.yoga.setMinHeight === 'function') entry.yoga.setMinHeight(appWindowH);
      if (typeof entry.yoga.setMaxHeight === 'function') entry.yoga.setMaxHeight(appWindowH);
    }
    if (isStatusbarTag(tag) && typeof entry.yoga.setHeight === 'function') {
      entry.yoga.setHeight(statusbarH);
      if (typeof entry.yoga.setMinHeight === 'function') entry.yoga.setMinHeight(statusbarH);
      if (typeof entry.yoga.setMaxHeight === 'function') entry.yoga.setMaxHeight(statusbarH);
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

  for (let i = 0; i < rects.entries.length; i++) {
    const rect = rects.entries[i];
    if (!rect) continue;
    if (String(rect.tag || '').toLowerCase() !== 'html_app_window') continue;
    const id = String(rect.id || '');
    if (!id) continue;
    const node = blockNodeById.get(id);
    if (!node || !node.srcNode) continue;
    const rawTag = String(node.srcNode.tagName || node.srcNode.nodeName || '').toLowerCase();
    if (rawTag !== 'iframe') continue;

    const spec = iframeSpecByBlockId.get(id);
    if (!spec || !spec.srcdoc) continue;
    const inner = iframeInnerRectFromParentRect(rect);
    const snap = iframeRenderByBlockId.get(id);
    const needsFrame = !snap
      || Math.abs(Number(snap.viewportW || 0) - Number(inner.w || 0)) >= 1
      || Math.abs(Number(snap.viewportH || 0) - Number(inner.h || 0)) >= 1;
    if (!needsFrame) continue;
    requestIframeFrameInternal(id, {
      srcdoc: spec.srcdoc,
      viewportW: inner.w,
      viewportH: inner.h,
      reason: snap ? 'resize' : 'first-frame',
      invalidations: [snap ? 'viewport-resize' : 'initial-frame'],
    });
  }

  return rects;
}

function paintLayout(packedRects, vw, vh) {
  if (typeof G.__trueosDrawLayoutRects === 'function') {
    const inlineTextRuns = collectInlineSemanticTextRuns(lastRectsById);
    G.__trueosDrawLayoutRects(packedRects, vw, vh, inlineTextRuns);
  }
}

function iframeInnerRectFromParentRect(rect) {
  const inset = HTML_APP_WINDOW_CONTENT_INSET || { left: 2, top: 18, right: 2, bottom: 2 };
  const scrollbarW = Math.max(4, Number(G.__trueosThemeScrollbarW || 8));
  const contentLeft = Math.max(Number(inset.left || 0), scrollbarW + 1);
  return {
    x: Math.round(Number(rect && rect.x || 0) + contentLeft),
    y: Math.round(Number(rect && rect.y || 0) + Number(inset.top || 0)),
    w: Math.max(1, Math.round(Number(rect && rect.w || 0) - contentLeft - Number(inset.right || 0))),
    h: Math.max(1, Math.round(Number(rect && rect.h || 0) - Number(inset.top || 0) - Number(inset.bottom || 0))),
  };
}

function rectIntersection(ax, ay, aw, ah, bx, by, bw, bh) {
  const x0 = Math.max(Number(ax || 0), Number(bx || 0));
  const y0 = Math.max(Number(ay || 0), Number(by || 0));
  const x1 = Math.min(
    Number(ax || 0) + Math.max(0, Number(aw || 0)),
    Number(bx || 0) + Math.max(0, Number(bw || 0)),
  );
  const y1 = Math.min(
    Number(ay || 0) + Math.max(0, Number(ah || 0)),
    Number(by || 0) + Math.max(0, Number(bh || 0)),
  );
  if (x1 <= x0 || y1 <= y0) return null;
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

function iframeScrollOffsetFor(blockId) {
  return Math.max(0, Number(iframeScrollById.get(String(blockId || '')) || 0));
}

function collectInlineSemanticTextRuns(rectsById) {
  const packed = [];
  if (!rectsById || typeof rectsById.values !== 'function') return packed;
  const temporalTextForRect = (tag, srcNode, rect) => {
    const raw = String(temporalDisplayText(tag, srcNode) || '');
    if (!raw) return '';
    const w = Math.max(8, Math.round(Number(rect && rect.w || 0)));
    const chevronW = Math.max(10, Math.min(18, Math.round(w * 0.18)));
    const usableW = Math.max(8, w - chevronW - 10);
    const maxChars = Math.max(1, Math.floor(usableW / 8));
    if (raw.length <= maxChars) return raw;
    if (maxChars <= 3) return raw.slice(0, maxChars);
    return `${raw.slice(0, Math.max(1, maxChars - 3))}...`;
  };

  const clampTextToRect = (txt, rect, leftPad = 4, rightPad = 4) => {
    const raw = String(txt || '');
    if (!raw) return '';
    const w = Math.max(1, Math.round(Number(rect && rect.w || 1)));
    const usableW = Math.max(1, w - Math.max(0, leftPad) - Math.max(0, rightPad));
    const maxChars = Math.max(1, Math.floor(usableW / 8));
    if (raw.length <= maxChars) return raw;
    if (maxChars <= 3) return raw.slice(0, maxChars);
    return `${raw.slice(0, Math.max(1, maxChars - 3))}...`;
  };

  const pushTextRun = (ownerId, x, y, text) => {
    const id = String(ownerId || '');
    const txt = String(text || '');
    if (!txt) return;
    let tx = Math.round(Number(x || 0));
    let ty = Math.round(Number(y || 0));

    if (id && isInHtmlAppWindowSubtree(id)) {
      const appId = htmlAppWindowAncestorId(id) || HTML_APP_WINDOW_ROOT_ID;
      const appRect = rectsById.get(appId);
      if (appRect) {
        const inner = iframeInnerRectFromParentRect(appRect);
        const minX = Math.round(Number(inner.x || 0));
        const maxX = Math.round(Number(inner.x || 0) + Number(inner.w || 0)) - 1;
        const minY = Math.round(Number(inner.y || 0));
        const maxY = Math.round(Number(inner.y || 0) + Number(inner.h || 0)) - 1;
        if (tx < minX || tx > maxX || ty < minY || ty > maxY) return;
      }
    }

    packed.push(tx, ty, txt);
  };

  for (const rect of rectsById.values()) {
    if (!rect) continue;
    const tag = String(rect.tag || '').toLowerCase();
    if (!INLINE_TEXT_TAGS.has(tag)) continue;
    const id = String(rect.id || '');
    if (!id) continue;
    const node = blockNodeById.get(id);
    if (!node || !node.srcNode) continue;
    let text = '';
    let x = 0;
    let y = 0;
    if (isTemporalTag(tag)) {
      text = temporalTextForRect(tag, node.srcNode, rect);
      x = Math.round(Number(rect.x || 0) + 4);
      y = Math.round(Number(rect.y || 0) + Math.max(2, Math.floor((Math.max(8, Number(rect.h || 8)) - 12) / 2)));
    } else if (isAvgFpsTaskNode(node)) {
      text = avgFpsTaskText();
      const insetX = tag === 'button' ? 8 : 2;
      x = Math.round(Number(rect.x || 0) + insetX);
      y = Math.round(Number(rect.y || 0) + 2);
    } else if (tag === 'input' || tag === 'textarea') {
      text = clampTextToRect(textFieldDisplayText(tag, node.srcNode), rect, 4, 4);
      x = Math.round(Number(rect.x || 0) + 4);
      y = Math.round(Number(rect.y || 0) + 2);
    } else {
      text = (tag === 'p' || tag === 'summary' || tag === 'label')
        ? nodeOwnTextPreview(node.srcNode, 120)
        : nodeTextPreview(node.srcNode);
      const insetX = tag === 'button' ? 8 : 2;
      x = Math.round(Number(rect.x || 0) + insetX);
      y = Math.round(Number(rect.y || 0) + 2);
    }
    if (!text) continue;
    pushTextRun(id, x, y, text);
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
    pushTextRun(id, x, y, text);
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
    pushTextRun(id, x, y, text);
  }

  // Select text lane: keep text emission consistent with checkbox/radio and leave icon lane on the left.
  for (const rect of rectsById.values()) {
    if (!rect) continue;
    const tag = String(rect.tag || '').toLowerCase();
    if (tag !== 'select') continue;
    const id = String(rect.id || '');
    if (!id) continue;
    const node = blockNodeById.get(id);
    if (!node || !node.srcNode) continue;
    const x = Math.round(Number(rect.x || 0) + 20);
    const y = Math.round(Number(rect.y || 0));
    const headerText = selectLabelText(node.srcNode);
    if (headerText) pushTextRun(id, x, y + 3, headerText);
    const options = selectOptionTexts(node.srcNode);
    for (let i = 0; i < options.length; i++) {
      const txt = String(options[i] || '');
      if (!txt) continue;
      pushTextRun(id, x, y + 20 + i * 16, txt);
    }
  }

  // Details caption lane: caption text is owned by details header row.
  for (const rect of rectsById.values()) {
    if (!rect) continue;
    const tag = String(rect.tag || '').toLowerCase();
    if (tag !== 'details') continue;
    const id = String(rect.id || '');
    if (!id) continue;
    const node = blockNodeById.get(id);
    if (!node || !node.srcNode) continue;
    const text = detailsSummaryText(node.srcNode);
    if (!text) continue;
    const x = Math.round(Number(rect.x || 0) + 28);
    const y = Math.round(Number(rect.y || 0) + 2);
    pushTextRun(id, x, y, text);
  }

  // HR caption lane: center caption text in the HR row when present.
  for (const rect of rectsById.values()) {
    if (!rect) continue;
    const tag = String(rect.tag || '').toLowerCase();
    if (tag !== 'hr') continue;
    const id = String(rect.id || '');
    if (!id) continue;
    const node = blockNodeById.get(id);
    if (!node || !node.srcNode) continue;
    const text = hrCaptionText(node.srcNode);
    if (!text) continue;
    const rx = Math.round(Number(rect.x || 0));
    const ry = Math.round(Number(rect.y || 0));
    const rw = Math.max(1, Math.round(Number(rect.w || 0)));
    const captionW = Math.max(1, text.length * 8);
    const tx = Math.round(rx + (rw - captionW) * 0.5);
    const ty = Math.round(ry + Math.max(0, Math.floor((Math.max(1, Number(rect.h || 1)) - 12) / 2)));
    pushTextRun(id, tx, ty, text);
  }

  // iframe/html_app_window child lane from iframeFrameV1 payload.
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
    if (!snap || !Array.isArray(snap.textRuns) || snap.textRuns.length <= 0) continue;

    const inner = iframeInnerRectFromParentRect(rect);
    const innerX = inner.x;
    const innerY = inner.y;
    const innerW = inner.w;
    const innerH = inner.h;
    const srcW = Math.max(1, Number(snap.viewportW || innerW));
    const srcH = Math.max(1, Number(snap.viewportH || innerH));
    const sx = innerW / srcW;
    const sy = innerH / srcH;
    const childScrollY = Math.max(0, iframeScrollOffsetFor(id));

    for (let i = 0; i + 3 < snap.textRuns.length; i += 4) {
      const lane = Number(snap.textRuns[i + 3] || 0) > 0 ? 1 : 0;
      const localX = Number(snap.textRuns[i + 0] || 0);
      const localY = Number(snap.textRuns[i + 1] || 0) - (lane > 0 ? 0 : childScrollY);
      const tx = innerX + Math.round(localX * sx);
      const ty = innerY + Math.round(localY * sy);
      const tt = String(snap.textRuns[i + 2] || '');
      if (!tt) continue;
      if (ty < innerY || ty > innerY + innerH) continue;
      pushTextRun(id, tx, ty, tt);
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
    const prefCols = preferredTableColumnWidths(rows, cols);
    const colWidths = fitTableColumnWidths(w, prefCols);
    const colStarts = new Array(cols + 1).fill(0);
    for (let c = 0; c < cols; c++) colStarts[c + 1] = colStarts[c] + colWidths[c];

    for (let r = 0; r < rows.length; r++) {
      const cells = tableCellsFromRow(rows[r]);
      const cy = y0 + r * rowH;
      for (let c = 0; c < cells.length; c++) {
        const cx = x0 + colStarts[c];
        const txt = nodeTextPreview(cells[c], 48);
        if (!txt) continue;
        pushTextRun(id, cx + 4, cy + 3, txt);
      }
    }
  }

  // Bottom statusbar labels for minimized html_app_window entries.
  for (const rect of rectsById.values()) {
    if (!rect) continue;
    const tag = String(rect.tag || '').toLowerCase();
    if (tag !== 'statusbar') continue;
    const chips = layoutStatusbarItems(rect, collectStatusbarItems());
    if (!chips || !Array.isArray(chips.items)) continue;
    for (let i = 0; i < chips.items.length; i++) {
      const item = chips.items[i];
      const label = String(item.label || '').trim();
      if (!label) continue;
      pushTextRun(String(rect.id || ''), Math.round(Number(item.x || 0) + 6), Math.round(Number(item.y || 0) + 2), label);
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
  const rectById = new Map();
  for (let i = 0; i < rectEntries.length; i++) {
    const r = rectEntries[i];
    if (!r) continue;
    rectById.set(String(r.id || ''), r);
  }

  const pushPackedClipped = (ownerId, rx, ry, rw, rh, rd, rs, rstyle) => {
    let x = Number(rx || 0);
    let y = Number(ry || 0);
    let w = Math.max(1, Number(rw || 1));
    let h = Math.max(1, Number(rh || 1));
    const owner = String(ownerId || '');

    if (owner && isInHtmlAppWindowSubtree(owner)) {
      const appId = htmlAppWindowAncestorId(owner) || HTML_APP_WINDOW_ROOT_ID;
      const appRect = rectById.get(appId);
      if (appRect && owner !== appId) {
        const inner = iframeInnerRectFromParentRect(appRect);
        const ax0 = Number(inner.x || 0);
        const ay0 = Number(inner.y || 0);
        const ax1 = ax0 + Number(inner.w || 0);
        const ay1 = ay0 + Number(inner.h || 0);
        const bx0 = x;
        const by0 = y;
        const bx1 = x + w;
        const by1 = y + h;
        const cx0 = Math.max(ax0, bx0);
        const cy0 = Math.max(ay0, by0);
        const cx1 = Math.min(ax1, bx1);
        const cy1 = Math.min(ay1, by1);
        if (cx1 <= cx0 || cy1 <= cy0) return;
        x = cx0;
        y = cy0;
        w = Math.max(1, cx1 - cx0);
        h = Math.max(1, cy1 - cy0);
      }
    }

    packed.push(
      Math.round(x),
      Math.round(y),
      Math.max(1, Math.round(w)),
      Math.max(1, Math.round(h)),
      Math.max(0, Number(rd || 0)),
      Number(rs || 0),
      Math.max(0, Number(rstyle || 0)),
    );
  };

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
    if (!snap) continue;

    const inner = iframeInnerRectFromParentRect(rect);
    const innerX = inner.x;
    const innerY = inner.y;
    const innerW = inner.w;
    const innerH = inner.h;
    const srcW = Math.max(1, Number(snap.viewportW || innerW));
    const srcH = Math.max(1, Number(snap.viewportH || innerH));
    const sx = innerW / srcW;
    const sy = innerH / srcH;
    const childScrollY = Math.max(0, iframeScrollOffsetFor(id));
    const appendLane = (lanePacked, laneDepthBoost, applyScroll) => {
      if (!Array.isArray(lanePacked) || lanePacked.length <= 0) return;
      for (let j = 0; j + 6 < lanePacked.length; j += 7) {
        const localX = Number(lanePacked[j + 0] || 0);
        const localY = Number(lanePacked[j + 1] || 0) - (applyScroll ? childScrollY : 0);
        const localW = Math.max(1, Number(lanePacked[j + 2] || 1));
        const localH = Math.max(1, Number(lanePacked[j + 3] || 1));
        const rx = innerX + Math.round(localX * sx);
        const ry = innerY + Math.round(localY * sy);
        const rw = Math.max(1, Math.round(localW * sx));
        const rh = Math.max(1, Math.round(localH * sy));
        const clipped = rectIntersection(rx, ry, rw, rh, innerX, innerY, innerW, innerH);
        if (!clipped) continue;
        const rd = Math.max(0, Number(rect.depth || 0) + laneDepthBoost + Math.max(0, Number(lanePacked[j + 4] || 0)));
        const rs = Number(lanePacked[j + 5] || 0);
        const rstyle = Math.max(0, Number(lanePacked[j + 6] || 0));
        pushPackedClipped(id, clipped.x, clipped.y, clipped.w, clipped.h, rd, rs, rstyle);
      }
    };

    appendLane(snap.basePackedRects, 1, true);
    appendLane(snap.overlayPackedRects, 48, false);
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
        getStatusbarItems: () => collectStatusbarItems(),
      });
      if (!Array.isArray(contributed)) continue;
      for (let j = 0; j + 6 < contributed.length; j += 7) {
        pushPackedClipped(
          String(rect.id || ''),
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

function collectIframeDebugPackedRects(rectEntries) {
  const packed = [];
  for (let i = 0; i < rectEntries.length; i++) {
    const rect = rectEntries[i];
    if (!rect) continue;
    if (String(rect.tag || '').toLowerCase() !== 'html_app_window') continue;
    const id = String(rect.id || '');
    if (!id) continue;
    const node = blockNodeById.get(id);
    if (!node || !node.srcNode) continue;
    const rawTag = String(node.srcNode.tagName || node.srcNode.nodeName || '').toLowerCase();
    if (rawTag !== 'iframe') continue;

    const outerX = Math.round(Number(rect.x || 0));
    const outerY = Math.round(Number(rect.y || 0));
    const outerW = Math.max(2, Math.round(Number(rect.w || 0)));
    const outerH = Math.max(2, Math.round(Number(rect.h || 0)));
    const inner = iframeInnerRectFromParentRect(rect);
    const depth = Math.max(0, Number(rect.depth || 0));

    // Outer iframe rect highlight.
    packed.push(outerX, outerY, outerW, outerH, depth + 96, 0, 4);
    // Inner content mapping rect highlight.
    packed.push(inner.x, inner.y, inner.w, inner.h, depth + 97, 0, 3);
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
  ensureFpsTaskTicker();
  const { vw, vh } = computeViewport();
  const rects = relayout(vw, vh);
  const widgetRects = collectWidgetPackedRects(rects.entries, vw, vh);
  const pulseRects = collectPulsePackedRects(rects.entries, Date.now());
  const debugRects = debugScroll.timer ? collectIframeDebugPackedRects(rects.entries) : [];
  let combinedRects = rects.packed;
  if (widgetRects.length > 0) combinedRects = combinedRects.concat(widgetRects);
  if (pulseRects.length > 0) combinedRects = combinedRects.concat(pulseRects);
  if (debugRects.length > 0) combinedRects = combinedRects.concat(debugRects);
  paintLayout(combinedRects, vw, vh);
  paintWidgets(rects.entries, vw, vh);
  noteRenderedFrameForFpsTask();
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

function mapGlobalToIframeCoords(blockId, globalX, globalY) {
  const id = String(blockId || '');
  if (!id) return { ok: false };
  const rect = lastRectsById.get(id);
  if (!rect) return { ok: false };
  const snap = iframeRenderByBlockId.get(id);
  if (!snap) return { ok: false };
  const inner = iframeInnerRectFromParentRect(rect);
  const gx = Number(globalX || 0);
  const gy = Number(globalY || 0);
  if (gx < inner.x || gy < inner.y || gx > inner.x + inner.w || gy > inner.y + inner.h) {
    return { ok: false };
  }
  const srcW = Math.max(1, Number(snap.viewportW || inner.w));
  const srcH = Math.max(1, Number(snap.viewportH || inner.h));
  const sx = inner.w / srcW;
  const sy = inner.h / srcH;
  const localViewportX = (gx - inner.x) / Math.max(1e-6, sx);
  const localViewportY = (gy - inner.y) / Math.max(1e-6, sy);
  const scrollY = Math.max(0, iframeScrollOffsetFor(id));
  return {
    ok: true,
    blockId: id,
    localViewportX,
    localViewportY,
    localContentX: localViewportX,
    localContentY: localViewportY + scrollY,
    scrollY,
    viewportW: srcW,
    viewportH: srcH,
  };
}

function findIframeAtGlobal(globalX, globalY) {
  const gx = Number(globalX || 0);
  const gy = Number(globalY || 0);
  let best = null;
  for (const rect of lastRectsById.values()) {
    if (!rect) continue;
    if (String(rect.tag || '').toLowerCase() !== 'html_app_window') continue;
    const id = String(rect.id || '');
    if (!id) continue;
    const node = blockNodeById.get(id);
    if (!node || !node.srcNode) continue;
    const rawTag = String(node.srcNode.tagName || node.srcNode.nodeName || '').toLowerCase();
    if (rawTag !== 'iframe') continue;
    const inner = iframeInnerRectFromParentRect(rect);
    if (gx < inner.x || gy < inner.y || gx > inner.x + inner.w || gy > inner.y + inner.h) continue;
    if (!best || Number(rect.depth || 0) >= Number(best.depth || 0)) {
      best = { blockId: id, depth: Number(rect.depth || 0) };
    }
  }
  return best ? String(best.blockId || '') : '';
}

function findStatusbarItemAtGlobal(globalX, globalY) {
  const gx = Number(globalX || 0);
  const gy = Number(globalY || 0);
  for (const rect of lastRectsById.values()) {
    if (!rect) continue;
    if (String(rect.tag || '').toLowerCase() !== 'statusbar') continue;
    const chips = layoutStatusbarItems(rect, collectStatusbarItems());
    if (!chips || !Array.isArray(chips.items)) continue;
    for (let i = 0; i < chips.items.length; i++) {
      const item = chips.items[i] || {};
      const x = Number(item.x || 0);
      const y = Number(item.y || 0);
      const w = Math.max(1, Number(item.w || 1));
      const h = Math.max(1, Number(item.h || 1));
      if (gx < x || gy < y || gx > x + w || gy > y + h) continue;
      return String(item.id || '');
    }
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
  requestIframeFrame(blockId, opts) {
    return requestIframeFrameInternal(blockId, opts);
  },
  getIframeFrame(blockId) {
    return iframeRenderByBlockId.get(String(blockId || '')) || null;
  },
  getIframeMetrics(blockId) {
    return iframeMetricsForBlock(blockId);
  },
  setScroll(blockId, scrollY) {
    const id = String(blockId || '');
    const node = blockNodeById.get(id);
    if (node && node.srcNode && String(node.srcNode.tagName || node.srcNode.nodeName || '').toLowerCase() === 'iframe') {
      return setIframeScrollInternal(id, scrollY, true);
    }
    return setScrollInternal(blockId, scrollY, true);
  },
  setScrollNoRepaint(blockId, scrollY) {
    const id = String(blockId || '');
    const node = blockNodeById.get(id);
    if (node && node.srcNode && String(node.srcNode.tagName || node.srcNode.nodeName || '').toLowerCase() === 'iframe') {
      return setIframeScrollInternal(id, scrollY, false);
    }
    return setScrollInternal(blockId, scrollY, false);
  },
  setIframeScroll(blockId, scrollY) {
    return setIframeScrollInternal(blockId, scrollY, true);
  },
  setIframeScrollNoRepaint(blockId, scrollY) {
    return setIframeScrollInternal(blockId, scrollY, false);
  },
  getIframeScroll(blockId) {
    return iframeScrollOffsetFor(blockId);
  },
  globalToIframeCoords(blockId, globalX, globalY) {
    return mapGlobalToIframeCoords(blockId, globalX, globalY);
  },
  setActiveIframe(blockId) {
    iframeFocusState.activeId = String(blockId || '');
    return iframeFocusState.activeId;
  },
  getActiveIframe() {
    return String(iframeFocusState.activeId || '');
  },
  routePointerToIframe(globalX, globalY, phase = 'move') {
    const targetId = findIframeAtGlobal(globalX, globalY);
    if (!targetId) return { ok: false, reason: 'no-iframe-hit' };
    iframeFocusState.activeId = targetId;
    const mapped = mapGlobalToIframeCoords(targetId, globalX, globalY);
    if (!mapped.ok) return mapped;
    requestIframeFrameInternal(targetId, {
      reason: `pointer-${String(phase || 'move')}`,
      invalidations: ['pointer-event'],
    });
    return mapped;
  },
  routeWheelToIframe(globalX, globalY, deltaY) {
    const targetId = findIframeAtGlobal(globalX, globalY) || String(iframeFocusState.activeId || '');
    if (!targetId) return false;
    iframeFocusState.activeId = targetId;
    const cur = Math.max(0, iframeScrollOffsetFor(targetId));
    return setIframeScrollInternal(targetId, cur + Number(deltaY || 0), true);
  },
  routeKeyboardToIframe(key, phase = 'down') {
    const targetId = String(iframeFocusState.activeId || '');
    if (!targetId) return false;
    requestIframeFrameInternal(targetId, {
      reason: `keyboard-${String(phase || 'down')}`,
      invalidations: [`key:${String(key || '')}`],
    });
    return true;
  },
  widgetDidUpdate(blockId) {
    return markWidgetUpdated(blockId);
  },
  setCursorPlaneEnabled(enabled) {
    cursorPlane.enabled = enabled !== false;
    if (cursorPlane.enabled) {
      ensureCursorPlaneTicker();
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
    return true;
  },
  clearCursorPointer(pointerId) {
    const id = Math.max(1, Number(pointerId || 0) | 0);
    return cursorPlane.pointers.delete(id);
  },
  refreshCursorPlane(maxPointers) {
    return refreshCursorPlaneFromKernel(maxPointers);
  },
  repaintCursorPlane() {
    return true;
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
  setHtmlAppWindowMinimized(blockId, minimized) {
    const id = String(blockId || '');
    if (!id || id === HTML_APP_WINDOW_ROOT_ID || id === STATUSBAR_ROOT_ID) return false;
    const node = blockNodeById.get(id);
    if (!node || String(node.tagName || '').toLowerCase() !== 'html_app_window') return false;
    minimizedHtmlAppWindowById.set(id, minimized === true);
    relayoutAndPaint();
    return true;
  },
  toggleHtmlAppWindowMinimized(blockId) {
    const id = String(blockId || '');
    if (!id || id === HTML_APP_WINDOW_ROOT_ID || id === STATUSBAR_ROOT_ID) return false;
    const node = blockNodeById.get(id);
    if (!node || String(node.tagName || '').toLowerCase() !== 'html_app_window') return false;
    const next = !isHtmlAppWindowMinimized(id);
    minimizedHtmlAppWindowById.set(id, next);
    relayoutAndPaint();
    return next;
  },
  getHtmlAppWindowMinimized(blockId) {
    return isHtmlAppWindowMinimized(blockId);
  },
  listStatusbarItems() {
    return collectStatusbarItems();
  },
  hitStatusbarItem(globalX, globalY) {
    return findStatusbarItemAtGlobal(globalX, globalY);
  },
  activateStatusbarItem(globalX, globalY) {
    const id = findStatusbarItemAtGlobal(globalX, globalY);
    if (!id) return '';
    const node = blockNodeById.get(id);
    if (!node || String(node.tagName || '').toLowerCase() !== 'html_app_window') return '';
    minimizedHtmlAppWindowById.set(id, false);
    relayoutAndPaint();
    return id;
  },
  getIframeTrace(blockId) {
    return iframeTraceByBlockId.get(String(blockId || '')) || null;
  },
  listIframeTraces() {
    const out = [];
    for (const [id, trace] of iframeTraceByBlockId.entries()) {
      out.push({ blockId: id, trace });
    }
    return out;
  },
};

G.__trueosBrowser.registerWidget('html_app_window', renderHtmlAppWindowWidget);
G.__trueosBrowser.registerWidget('statusbar', renderStatusbarWidget);
G.__trueosBrowser.registerWidget('scrollbar', renderScrollbarWidget);
G.__trueosBrowser.registerWidget('checkbox', renderCheckboxWidget);
G.__trueosBrowser.registerWidget('summary', renderSummaryWidget);
G.__trueosBrowser.registerWidget('details', renderDetailsWidget);
G.__trueosBrowser.registerWidget('dialog', renderDialogWidget);
G.__trueosBrowser.registerWidget('button', renderButtonWidget);
G.__trueosBrowser.registerWidget('svg', renderSvgWidget);
G.__trueosBrowser.registerWidget('table', renderTableWidget);
G.__trueosBrowser.registerWidget('form', renderFormWidget);
G.__trueosBrowser.registerWidget('radio', renderRadioWidget);
G.__trueosBrowser.registerWidget('select', renderSelectWidget);
G.__trueosBrowser.registerWidget('timeinput', renderTemporalWidget);
G.__trueosBrowser.registerWidget('dateinput', renderTemporalWidget);
G.__trueosBrowser.registerWidget('monthinput', renderTemporalWidget);
G.__trueosBrowser.registerWidget('weekinput', renderTemporalWidget);
G.__trueosBrowser.registerWidget('datetimelocalinput', renderTemporalWidget);
G.__trueosBrowser.registerWidget('slider', renderRangeWidget);
G.__trueosBrowser.registerWidget('progress', renderRangeWidget);
G.__trueosBrowser.registerWidget('meter', renderRangeWidget);

const DEBUG_AUTOSCROLL_BOOT = false;

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
