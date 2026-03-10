import * as parse5 from 'parse5';
import Yoga from 'yoga-layout';
import { createFpsOverlay } from './fps.mjs';
import { extractCssRows } from './css.mjs';
import { renderScene } from './scene.mjs';
import { BLOCK_TAGS, TEXT_LEVEL_SEMANTICS_TAGS } from './htmlDefaults.mjs';
import { LEFT_PAD, TOP_PAD, LINE_H } from './theme.mjs';

const runtime = resolveRuntime();

const WHEEL_STEP_PX = 32;
const INDENT_PX = 12;
const AUTO_PAINT_MS = 200;
const OMIT_TAGS = new Set(['html', 'body', 'script', 'style', 'meta', 'link', 'li']);
const SHOW_CLOSING_TAG_ROWS = false;

let cachedHtml = '';
let cachedDoc = null;
let cursorReadSeq = 0;
let scrollY = 0;

const fpsOverlay = createFpsOverlay();

function resolveRuntime() {
  const host = (typeof globalThis !== 'undefined') ? globalThis : this;
  if (!host.window) host.window = host;

  return {
    host,
    readCursorEventsSince: host.__trueosReadCursorEventsSince,
  };
}

function computeViewport() {
  const W = runtime.host.window || runtime.host;
  const vw = Math.max(1, Number(W.innerWidth || 1280));
  const vh = Math.max(1, Number(W.innerHeight || 800));
  return { vw, vh };
}

function collapseWhitespace(s) {
  return String(s || '').replace(/\s+/g, ' ').trim();
}

function isElement(node) {
  return !!node && typeof node === 'object' && typeof node.tagName === 'string';
}

function isTextNode(node) {
  return !!node && typeof node === 'object' && node.nodeName === '#text' && typeof node.value === 'string';
}

function pushRow(rows, text, depth, kind = 'text') {
  const t = collapseWhitespace(text);
  if (!t) return;
  rows.push({ depth: Math.max(0, Number(depth || 0) | 0), text: t, kind: String(kind || 'text') });
}

function shouldOmitElement(tagName) {
  return OMIT_TAGS.has(String(tagName || '').toLowerCase());
}

function shouldRenderTagLines(tagName) {
  const tag = String(tagName || '').toLowerCase();
  if (tag === 'p') return false;
  if (TEXT_LEVEL_SEMANTICS_TAGS.includes(tag)) return false;
  return BLOCK_TAGS.has(tag);
}

function collectRows(node, depth, rows, parentTag = '') {
  if (!node || typeof node !== 'object') return;

  if (isTextNode(node)) {
    const parent = String(parentTag || '').toLowerCase();
    const kind = parent === 'title'
      ? 'title-text'
      : (parent === 'li' ? 'li-text' : 'text');
    pushRow(rows, node.value, depth, kind);
    return;
  }

  if (isElement(node)) {
    const tag = String(node.tagName || '').toLowerCase();
    const renderTagLines = !shouldOmitElement(tag) && shouldRenderTagLines(tag);
    if (renderTagLines && SHOW_CLOSING_TAG_ROWS) pushRow(rows, `<${tag}>`, depth, 'tag-open');
    const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
    for (let i = 0; i < kids.length; i++) {
      collectRows(kids[i], renderTagLines ? depth + 1 : depth, rows, tag);
    }
    if (renderTagLines && SHOW_CLOSING_TAG_ROWS) pushRow(rows, `</${tag}>`, depth, 'tag-close');
    return;
  }

  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i++) {
    collectRows(kids[i], depth, rows, parentTag);
  }
}

function buildDocFromHtml(html, vw) {
  let parsed;
  try {
    parsed = parse5.parse(String(html || ''));
  } catch (_) {
    parsed = parse5.parse('');
  }

  const rows = [];
  collectRows(parsed, 0, rows);
  const cssSection = (() => {
    try {
      return typeof extractCssRows === 'function' ? extractCssRows(parsed) : null;
    } catch (_) {
      return null;
    }
  })();
  /* debug
  const cssRows = Array.isArray(cssSection && cssSection.rows) ? cssSection.rows : [];
  for (let i = 0; i < cssRows.length; i++) {
    const r = cssRows[i];
    rows.push({
      depth: Math.max(0, Number(r && r.depth || 0) | 0),
      text: String(r && r.text || ''),
      kind: 'css',
    });
  }
  */
  runtime.host.__trueosKernelCssObjects = Array.isArray(cssSection && cssSection.cssObjects) ? cssSection.cssObjects : [];
  // applyLightning;
  const layout = applyYoga(rows, vw);
  return {
    dom: parsed,
    rows,
    rowX: layout.rowX,
    rowY: layout.rowY,
    contentH: layout.contentH,
    width: vw,
  };
}

function applyYoga(rows, vw) {
  const root = Yoga.Node.create();
  root.setFlexDirection(Yoga.FLEX_DIRECTION_COLUMN);
  root.setAlignItems(Yoga.ALIGN_FLEX_START);
  root.setWidth(vw);
  root.setPadding(Yoga.EDGE_LEFT, LEFT_PAD);
  root.setPadding(Yoga.EDGE_TOP, TOP_PAD);

  const nodes = [];
  const rowX = [];
  const rowY = [];
  for (let i = 0; i < rows.length; i++) {
    const r = rows[i];
    const indent = r.depth * INDENT_PX;
    const n = Yoga.Node.create();
    n.setHeight(LINE_H);
    n.setMinHeight(LINE_H);
    if (r.kind === 'title-text') {
      // Draw path places text at node-left, so center by placing a content-width node
      // at a centered left margin within the same inner row width as normal rows.
      const textW = Math.max(1, Math.round(String(r.text || '').length * 8));
      const innerRowW = Math.max(1, vw - (LEFT_PAD * 2));
      const centeredLeft = Math.max(0, Math.floor((innerRowW - textW) * 0.5));
      n.setWidth(textW);
      n.setMargin(Yoga.EDGE_LEFT, centeredLeft);
    } else {
      n.setWidth(Math.max(1, vw - (LEFT_PAD * 2) - indent));
      n.setMargin(Yoga.EDGE_LEFT, indent);
    }
    root.insertChild(n, i);
    nodes.push(n);
  }

  root.calculateLayout(vw, NaN, Yoga.DIRECTION_LTR);

  for (let i = 0; i < nodes.length; i++) {
    rowX.push(Math.round(Number(nodes[i].getComputedLeft() || 0)));
    rowY.push(Math.round(Number(nodes[i].getComputedTop() || 0)));
  }

  const contentH = Math.max(1, Math.round(Number(root.getComputedHeight() || 0)));
  root.freeRecursive();

  return { rowX, rowY, contentH };
}

function ensureDoc(vw) {
  if (!cachedDoc || cachedDoc.width !== vw) {
    cachedDoc = buildDocFromHtml(cachedHtml, vw);
  }
  return cachedDoc;
}

function setHtml(nextHtml) {
  cachedHtml = String(nextHtml || '');
  cachedDoc = null;
  paint();
}

function paint() {
  const { vw, vh } = computeViewport();
  const doc = ensureDoc(vw);
  const maxScroll = Math.max(0, Math.round(Number(doc.contentH || vh) - vh));
  if (scrollY > maxScroll) scrollY = maxScroll;

  const overlayRuns = [];
  fpsOverlay.appendRuns(overlayRuns, vw);

  renderScene(doc, vw, vh, scrollY, overlayRuns);

  return true;
}

function onWheelDelta(deltaY) {
  const { vw, vh } = computeViewport();
  const doc = ensureDoc(vw);
  const maxScroll = Math.max(0, Math.round(Number(doc.contentH || vh) - vh));
  if (maxScroll <= 0) return false;
  const next = Math.max(0, Math.min(maxScroll, Math.round(scrollY + Number(deltaY || 0))));
  if (next === scrollY) return false;
  scrollY = next;
  paint();
  return true;
}

function pumpCursorEvents() {
  const fn = runtime.readCursorEventsSince;
  if (typeof fn !== 'function') return 0;

  let packed = null;
  try {
    packed = fn(Number(cursorReadSeq || 0));
  } catch (_) {
    return 0;
  }
  if (!Array.isArray(packed) || packed.length < 3) return 0;

  const nextSeq = Number(packed[0] || cursorReadSeq || 0);
  const wrote = Math.max(0, Number(packed[2] || 0) | 0);
  let updated = 0;
  let p = 3;
  for (let i = 0; i < wrote; i++) {
    if (p + 5 >= packed.length) break;
    const wheel = Number(packed[p + 4] || 0) | 0;
    if (wheel !== 0) {
      const dy = Number(wheel) * -WHEEL_STEP_PX;
      if (onWheelDelta(dy)) updated += 1;
    }
    p += 6;
  }
  cursorReadSeq = nextSeq;
  return updated;
}

function startWheelPump() {
  const host = runtime.host;
  if (typeof host.setInterval === 'function') {
    try {
      host.setInterval(pumpCursorEvents, 16);
      return;
    } catch (_) {}
  }
  if (typeof host.requestAnimationFrame === 'function') {
    const step = () => {
      pumpCursorEvents();
      try { host.requestAnimationFrame(step); } catch (_) {}
    };
    try { host.requestAnimationFrame(step); } catch (_) {}
  }
}

function startAutoPaint() {
  const host = runtime.host;
  if (typeof host.setInterval !== 'function') return;
  try {
    host.setInterval(() => {
      paint();
    }, AUTO_PAINT_MS);
  } catch (_) {}
}

runtime.host.__trueosBrowser = {
  paint,
  setHtml,
  setScroll(y) {
    scrollY = Math.max(0, Math.round(Number(y || 0)));
    paint();
  },
};

if (typeof (runtime.host.window || runtime.host).addEventListener === 'function') {
  (runtime.host.window || runtime.host).addEventListener('resize', paint);
}

setHtml(runtime.host.__trueosUiHtml || '');
paint();
startWheelPump();
startAutoPaint();