import * as parse5 from 'parse5';
import Yoga from 'yoga-layout';
import { renderHtmlAppWindowWidget } from './widgets/htmlAppWindowWidget.mjs';
import { renderScrollbarWidget } from './widgets/scrollbarWidget.mjs';
import { renderCheckboxWidget } from './widgets/checkboxWidget.mjs';
import { renderSummaryWidget } from './widgets/summaryWidget.mjs';
import { renderDialogWidget } from './widgets/dialogWidget.mjs';
import { renderButtonWidget } from './widgets/buttonWidget.mjs';
import {
  applyYogaDefaultsTemporalInput,
  isTemporalTag,
  renderTemporalWidget,
  temporalDisplayText,
  temporalTagForInputType,
} from './widgets/tempo.mjs';

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

function getBody(doc) {
  const html = (doc.childNodes || []).find((n) => isElement(n) && (n.tagName || n.nodeName) === 'html');
  if (!html) return null;
  return (html.childNodes || []).find((n) => isElement(n) && (n.tagName || n.nodeName) === 'body') || null;
}

function isStructuralTag(tag) {
  return tag === 'body' || tag === 'html' || tag === 'head' || tag === '#document' || tag === '#document-fragment';
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

function classifyTag(node, rawTag) {
  const tag = String(rawTag || '').toLowerCase();
  if (tag === 'iframe') return 'html_app_window';
  if (tag === 'input') {
    const t = String(getAttr(node, 'type') || '').toLowerCase();
    const temporal = temporalTagForInputType(t);
    if (temporal) return temporal;
    if (t === 'checkbox') return 'checkbox';
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
  const isTemporal = isTemporalTag(tag);
  const nodeId = String(node && node.id || '');
  const yn = Yoga.Node.create();
  yn.setFlexDirection(Yoga.FLEX_DIRECTION_COLUMN);
  yn.setAlignItems(Yoga.ALIGN_STRETCH);
  if (typeof yn.setAlignSelf === 'function') yn.setAlignSelf(Yoga.ALIGN_STRETCH);
  if (!isHeading && !isCompact && !isButton && !isTemporal && depth > 0 && typeof yn.setWidthPercent === 'function') yn.setWidthPercent(100);
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
    }
    if (isTemporal) {
      applyYogaDefaultsTemporalInput(yn, Yoga, tag, node && node.srcNode ? node.srcNode : null);
    }
  }

  allBlocks.push({ node, yoga: yn, depth });

  const kids = visibleChildrenForNode(node);
  if (kids.length <= 0) {
    yn.setHeight(Math.max(1, Number(G.__trueosThemeNodeH || 16)));
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
    const renderWidget = widgetByTag.get(rect.tag);
    if (typeof renderWidget !== 'function') continue;
    try {
      const contributed = renderWidget(rect, {
        viewportW: vw,
        viewportH: vh,
        mode: 'collect',
        rectEntries,
        scrollOffsetFor,
      });
      if (!Array.isArray(contributed)) continue;
      for (let j = 0; j + 6 < contributed.length; j += 7) {
        packed.push(
          Number(contributed[j + 0] || 0),
          Number(contributed[j + 1] || 0),
          Math.max(1, Number(contributed[j + 2] || 1)),
          Math.max(1, Number(contributed[j + 3] || 1)),
          Math.max(0, Number(contributed[j + 4] || 0)),
          Number(contributed[j + 5] || 0) >= 0.5 ? 1 : 0,
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
G.__trueosBrowser.registerWidget('timeinput', renderTemporalWidget);
G.__trueosBrowser.registerWidget('dateinput', renderTemporalWidget);
G.__trueosBrowser.registerWidget('monthinput', renderTemporalWidget);
G.__trueosBrowser.registerWidget('weekinput', renderTemporalWidget);
G.__trueosBrowser.registerWidget('datetimelocalinput', renderTemporalWidget);

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
