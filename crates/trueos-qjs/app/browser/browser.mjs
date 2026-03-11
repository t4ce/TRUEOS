import * as parse5 from 'parse5';
import Yoga from 'yoga-layout';
import { Worker } from 'node:worker_threads';
import { createFpsOverlay } from './fps.mjs';
import { extractCssSection, resolveNodeStyle } from './css.mjs';
import { renderScene } from './scene.mjs';
import { BLOCK_TAGS, TEXT_LEVEL_SEMANTICS_TAGS } from './htmlDefaults.mjs';
import { LEFT_PAD, TOP_PAD, LINE_H } from './theme.mjs';

const runtime = resolveRuntime();

const AI_CURSOR_SLOT_BASE = 10000;
const WHEEL_STEP_PX = 32;
const CURSOR_EVENT_FIELDS = 6;
const CURSOR_FLAG_BUTTONS_CHANGED = 1 << 2;
const INDENT_PX = 12;
const AUTO_PAINT_MS = Math.max(0, Number(runtime.host.__trueosBrowserAutoPaintMs || 0) || 0);
const OMIT_TAGS = new Set(['html', 'body', 'script', 'style', 'meta', 'link', 'li']);
const SHOW_CLOSING_TAG_ROWS = false;
const RELEASE_RECT_RGBA = 0x3f2f7fff;

let cachedHtml = '';
let cachedDoc = null;
let cursorReadSeq = 0;
let scrollY = 0;
let currentPageUrl = String(runtime.host.__trueosBrowserCurrentUrl || runtime.host.__trueosBrowserUrl || '');
const kernelCursorState = new Map();
const cursorButtonEvents = [];
const cursorDragOrigins = new Map();
const aiCursorSlots = new Map();
let nextAiCursorSlot = AI_CURSOR_SLOT_BASE;
let browserActionSeq = 0;
let aiStartPromise = null;
let aiStartSpecifier = '';
let aiWorker = null;
const aiInputQueue = [];
const aiInputWaiters = [];
let pendingReleaseRect = null;

const fpsOverlay = createFpsOverlay();
const DEFAULT_AI_INPUT_OPTIONS = Object.freeze({
  webSearch: false,
  newConversation: false,
  computerUse: true,
});

const FALLBACK_BROWSER_API_CONTRACT = {
  version: 1,
  available: [
    'getApiContract',
    'listUnavailable',
    'getHtml',
    'getTextRows',
    'getDomSnapshot',
    'setNodeHtml',
    'insertHtml',
    'getViewport',
    'paint',
    'setScroll',
    'click',
    'navigate',
    'pressKey',
  ],
  unavailable: [
    'typeText',
    'captureScreenshot',
  ],
  notes: {
    intent: 'Worker-facing browser contract for the AI task. Keep this surface explicit so agent logic remains isolated from the browser VM.',
    targetShape: 'Close to future computer-use style APIs while still reflecting TRUEOS capabilities today.',
  },
};

function resolveRuntime() {
  const host = (typeof globalThis !== 'undefined') ? globalThis : this;
  if (!host.window) host.window = host;

  return {
    host,
  };
}

function cloneApiContract() {
  const contract = runtime.host.__trueosBrowserAiApiContract;
  const source = contract && typeof contract === 'object'
    ? contract
    : FALLBACK_BROWSER_API_CONTRACT;
  return JSON.parse(JSON.stringify(source));
}

function notYetAvailable(name) {
  const err = new Error(`browser API not yet available: ${name}`);
  err.code = typeof runtime.host.__trueosBrowserAiApiUnavailableCode === 'string'
    ? runtime.host.__trueosBrowserAiApiUnavailableCode
    : 'TRUEOS_BROWSER_API_UNAVAILABLE';
  throw err;
}

function parseWorkerJson(raw) {
  if (typeof raw !== 'string' || !raw) return null;
  try {
    return JSON.parse(raw);
  } catch (_err) {
    return null;
  }
}

function postWorkerJson(worker, payload) {
  if (!worker || typeof worker.postMessage !== 'function') return;
  worker.postMessage(JSON.stringify(payload));
}

function normalizeAiInput(entry, options = null) {
  const source = entry && typeof entry === 'object' && !Array.isArray(entry) ? entry : null;
  const opts = options && typeof options === 'object' ? options : null;
  const text = typeof entry === 'string'
    ? entry
    : (source && typeof source.text === 'string' ? source.text : '');
  const value = text.trim();
  if (!value) return null;

  const cfg = source || opts || DEFAULT_AI_INPUT_OPTIONS;
  return {
    text: value,
    webSearch: !!cfg.webSearch,
    newConversation: !!cfg.newConversation,
    computerUse: cfg.computerUse !== false,
  };
}

function pushAiInput(entry, options = null) {
  const value = normalizeAiInput(entry, options);
  if (!value) return false;

  const waiter = aiInputWaiters.shift();
  if (waiter) {
    waiter(value);
    return true;
  }

  aiInputQueue.push(value);
  return true;
}

function awaitAiInput(question = '') {
  const prompt = typeof question === 'string' ? question.trim() : '';
  if (prompt) {
    try { console.log(`[browser.mjs] ai input requested: ${prompt}`); } catch (_) {}
  }
  if (aiInputQueue.length > 0) {
    return Promise.resolve(aiInputQueue.shift());
  }
  return new Promise((resolve) => {
    aiInputWaiters.push(resolve);
  });
}

function installAiInputBridge() {
  runtime.host.__trueosAiInputPush = pushAiInput;
  runtime.host.__trueosAiAwaitInput = awaitAiInput;
}

async function dispatchAiWorkerRpc(method, args) {
  const api = runtime.host.__trueosBrowser;
  if (typeof method !== 'string' || !method.startsWith('browser.')) {
    throw new Error(`unsupported worker rpc method: ${method}`);
  }

  const name = method.slice('browser.'.length);
  if (name === 'getApiContract') return cloneApiContract();
  if (name === 'listUnavailable') return cloneApiContract().unavailable;

  if (!api || typeof api[name] !== 'function') {
    throw new Error(`browser rpc missing method: ${name}`);
  }

  return await api[name](...(Array.isArray(args) ? args : []));
}

async function handleAiWorkerMessage(worker, raw) {
  const message = parseWorkerJson(raw);
  if (!message) {
    return;
  }

  if (typeof message.dbg === 'string') {
    try { console.log('[browser.mjs] ai worker', message.dbg); } catch (_) {}
    return;
  }

  if (message.kind !== 'rpc_request') {
    return;
  }

  try {
    let result;
    if (message.method === 'host.awaitInput') {
      result = await awaitAiInput(String((message.args && message.args[0]) || ''));
    } else if (message.method === 'host.shellPrint') {
      const text = String((message.args && message.args[0]) || '');
      if (typeof runtime.host.__trueosUart1ShellWrite === 'function' && text) {
        runtime.host.__trueosUart1ShellWrite(text);
      }
      result = true;
    } else {
      result = await dispatchAiWorkerRpc(message.method, message.args);
    }
    postWorkerJson(worker, {
      kind: 'rpc_result',
      id: message.id,
      ok: true,
      result,
    });
  } catch (err) {
    postWorkerJson(worker, {
      kind: 'rpc_result',
      id: message.id,
      ok: false,
      error: String(err && err.message ? err.message : err),
      code: err && err.code ? String(err.code) : undefined,
    });
  }
}

function buildAiWorkerSource(specifier, options) {
  const resolvedSpecifier = typeof specifier === 'string' && specifier ? specifier : '/qjs/ai/ai_pc.mjs';
  const specLiteral = JSON.stringify(resolvedSpecifier);
  return `
const __spec = ${specLiteral};
import(__spec)
  .then((mod) => {
    if (mod && typeof mod.startAiPcWorker === 'function') {
      return mod.startAiPcWorker();
    }
    if (mod && typeof mod.startAiPc === 'function') {
      return mod.startAiPc();
    }
    throw new Error('AI module missing startAiPcWorker/startAiPc export');
  })
  .catch((err) => {
    try {
      console.log('[ai-worker] start failed', String(err && err.stack ? err.stack : err));
    } catch (_) {}
  });
`;
}

function attachAiWorker(worker) {
  worker.onMessage((raw) => {
    void handleAiWorkerMessage(worker, raw);
  });
  return worker;
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

function pushRow(rows, text, depth, kind = 'text', style = null) {
  const t = collapseWhitespace(text);
  if (!t) return;
  rows.push({
    depth: Math.max(0, Number(depth || 0) | 0),
    text: t,
    kind: String(kind || 'text'),
    style,
  });
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

function collectRows(node, depth, rows, cssSection, parentTag = '', parentStyle = null, path = 'root', ancestors = []) {
  if (!node || typeof node !== 'object') return;

  if (isTextNode(node)) {
    const parent = String(parentTag || '').toLowerCase();
    const kind = parent === 'title'
      ? 'title-text'
      : (parent === 'li' ? 'li-text' : 'text');
    pushRow(rows, node.value, depth, kind, parentStyle);
    return;
  }

  if (isElement(node)) {
    const tag = String(node.tagName || '').toLowerCase();
    const style = resolveNodeStyle(node, path, cssSection, ancestors, parentStyle);
    const renderTagLines = !shouldOmitElement(tag) && shouldRenderTagLines(tag);
    if (renderTagLines && SHOW_CLOSING_TAG_ROWS) pushRow(rows, `<${tag}>`, depth, 'tag-open', style);
    const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
    const nextAncestors = ancestors.concat([{ node, path }]);
    for (let i = 0; i < kids.length; i++) {
      collectRows(kids[i], renderTagLines ? depth + 1 : depth, rows, cssSection, tag, style, `${path}.${i}`, nextAncestors);
    }
    if (renderTagLines && SHOW_CLOSING_TAG_ROWS) pushRow(rows, `</${tag}>`, depth, 'tag-close', style);
    return;
  }

  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i++) {
    collectRows(kids[i], depth, rows, cssSection, parentTag, parentStyle, `${path}.${i}`, ancestors);
  }
}

function buildDocFromParsed(parsed, vw) {
  const doc = parsed && typeof parsed === 'object'
    ? parsed
    : parse5.parse('');

  const rows = [];
  const cssSection = (() => {
    try {
      return typeof extractCssSection === 'function' ? extractCssSection(doc) : null;
    } catch (_) {
      return null;
    }
  })();
  collectRows(doc, 0, rows, cssSection);
  /* debug
  const cssRows = Array.isArray(cssSection && cssSection.rows) ? cssSection.rows : [];
  for (let i = 0; i < cssRows.length; i++) {
    const r = cssRows[i];
    rows.push({
      depth: Math.max(0, Number(r && r.depth || 0) | 0),
      text: String(r && r.text || ''),
      kind: 'css',
      style: null,
    });
  }
  */
  runtime.host.__trueosKernelCssObjects = Array.isArray(cssSection && cssSection.cssObjects) ? cssSection.cssObjects : [];
  const layout = applyYoga(rows, vw);
  return {
    dom: doc,
    css: cssSection,
    rows,
    rowX: layout.rowX,
    rowY: layout.rowY,
    contentH: layout.contentH,
    width: vw,
  };
}

function buildDocFromHtml(html, vw) {
  let parsed;
  try {
    parsed = parse5.parse(String(html || ''));
  } catch (_) {
    parsed = parse5.parse('');
  }
  return buildDocFromParsed(parsed, vw);
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
  if (!cachedDoc) {
    cachedDoc = buildDocFromHtml(cachedHtml, vw);
  } else if (cachedDoc.width !== vw) {
    cachedDoc = buildDocFromParsed(cachedDoc.dom, vw);
  }
  return cachedDoc;
}

function ensureChildNodes(node) {
  if (!node || typeof node !== 'object') return [];
  if (!Array.isArray(node.childNodes)) node.childNodes = [];
  return node.childNodes;
}

function setParentNode(node, parent) {
  if (node && typeof node === 'object') {
    node.parentNode = parent || null;
  }
  return node;
}

function getNodeByPath(root, path) {
  if (!root || typeof root !== 'object') return null;
  const rawPath = typeof path === 'string' ? path.trim() : '';
  if (!rawPath || rawPath === 'root') return root;
  if (!rawPath.startsWith('root.')) return null;

  const parts = rawPath.slice('root.'.length).split('.');
  let node = root;
  for (let i = 0; i < parts.length; i += 1) {
    const index = Number(parts[i]);
    if (!Number.isInteger(index) || index < 0) return null;
    const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
    node = kids[index] || null;
    if (!node || typeof node !== 'object') return null;
  }
  return node;
}

function parseFragmentForNode(node, html) {
  const source = String(html || '');
  try {
    if (isElement(node)) {
      return parse5.parseFragment(node, source);
    }
    return parse5.parseFragment(source);
  } catch (_) {
    return parse5.parseFragment('');
  }
}

function adoptFragmentChildren(fragment, parent) {
  const kids = Array.isArray(fragment && fragment.childNodes) ? fragment.childNodes : [];
  const out = [];
  for (let i = 0; i < kids.length; i += 1) {
    out.push(setParentNode(kids[i], parent));
  }
  return out;
}

function normalizeDomTarget(target) {
  if (typeof target === 'string') return target.trim();
  if (target && typeof target === 'object' && typeof target.path === 'string') {
    return target.path.trim();
  }
  return '';
}

function normalizeInsertPosition(position) {
  const value = String(position || 'beforeend').trim().toLowerCase();
  if (value === 'beforebegin' || value === 'afterbegin' || value === 'beforeend' || value === 'afterend') {
    return value;
  }
  return 'beforeend';
}

function syncCachedHtmlFromDoc(doc) {
  try {
    cachedHtml = String(parse5.serialize(doc) || '');
  } catch (_) {
    cachedHtml = '';
  }
}

function commitDomMutation(doc) {
  const sourceDoc = doc && typeof doc === 'object'
    ? doc
    : (cachedDoc && cachedDoc.dom ? cachedDoc.dom : null);
  if (!sourceDoc) return false;

  syncCachedHtmlFromDoc(sourceDoc);
  const { vw } = computeViewport();
  cachedDoc = buildDocFromParsed(sourceDoc, vw);
  paint();
  return true;
}

function setNodeHtml(target, html) {
  const { vw } = computeViewport();
  const doc = ensureDoc(vw);
  const node = getNodeByPath(doc && doc.dom ? doc.dom : null, normalizeDomTarget(target));
  if (!node || typeof node !== 'object') return false;
  if (isTextNode(node)) return false;

  const nextChildren = adoptFragmentChildren(parseFragmentForNode(node, html), node);
  node.childNodes = nextChildren;
  return commitDomMutation(doc.dom);
}

function insertHtml(target, html, position = 'beforeend') {
  const { vw } = computeViewport();
  const doc = ensureDoc(vw);
  const node = getNodeByPath(doc && doc.dom ? doc.dom : null, normalizeDomTarget(target));
  if (!node || typeof node !== 'object') return false;

  const where = normalizeInsertPosition(position);
  let parent = null;
  let siblings = null;
  let insertAt = 0;

  if (where === 'beforebegin' || where === 'afterend') {
    parent = node.parentNode || null;
    if (!parent || typeof parent !== 'object') return false;
    siblings = ensureChildNodes(parent);
    const nodeIndex = siblings.indexOf(node);
    if (nodeIndex < 0) return false;
    insertAt = where === 'beforebegin' ? nodeIndex : nodeIndex + 1;
  } else {
    if (isTextNode(node)) return false;
    parent = node;
    siblings = ensureChildNodes(node);
    insertAt = where === 'afterbegin' ? 0 : siblings.length;
  }

  const inserted = adoptFragmentChildren(parseFragmentForNode(parent, html), parent);
  if (inserted.length <= 0) return false;
  siblings.splice(insertAt, 0, ...inserted);
  return commitDomMutation(doc.dom);
}

function cloneRows(rows) {
  if (!Array.isArray(rows)) return [];
  const out = [];
  for (let i = 0; i < rows.length; i += 1) {
    const row = rows[i] || {};
    out.push({
      depth: Number(row.depth || 0) | 0,
      text: String(row.text || ''),
      kind: String(row.kind || 'text'),
    });
  }
  return out;
}

function serializeNode(node, depth = 0, path = 'root') {
  if (!node || typeof node !== 'object') return null;
  if (depth > 10) {
    return { type: 'limit', path: String(path || 'root') };
  }
  if (isTextNode(node)) {
    return {
      type: 'text',
      path: String(path || 'root'),
      text: String(node.value || ''),
    };
  }
  const out = {
    type: isElement(node) ? 'element' : 'node',
    path: String(path || 'root'),
    tag: isElement(node) ? String(node.tagName || '').toLowerCase() : String(node.nodeName || ''),
    attrs: {},
    children: [],
  };
  if (Array.isArray(node.attrs)) {
    for (let i = 0; i < node.attrs.length; i += 1) {
      const attr = node.attrs[i];
      if (!attr || typeof attr.name !== 'string') continue;
      out.attrs[attr.name] = String(attr.value || '');
    }
  }
  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i += 1) {
    const child = serializeNode(kids[i], depth + 1, `${path}.${i}`);
    if (child) out.children.push(child);
  }
  return out;
}

function getDomSnapshot() {
  const { vw } = computeViewport();
  const doc = ensureDoc(vw);
  return serializeNode(doc && doc.dom ? doc.dom : null, 0);
}

function getRows() {
  const { vw } = computeViewport();
  const doc = ensureDoc(vw);
  return cloneRows(doc && doc.rows ? doc.rows : []);
}

function getViewport() {
  const { vw, vh } = computeViewport();
  return {
    width: vw,
    height: vh,
    scrollY: scrollY,
  };
}

function setHtml(nextHtml) {
  cachedHtml = String(nextHtml || '');
  cachedDoc = null;
  paint();
}

function nowMs() {
  if (typeof Date !== 'undefined' && typeof Date.now === 'function') {
    return Number(Date.now()) || 0;
  }
  return 0;
}

function pushBrowserAction(event) {
  browserActionSeq += 1;
  const next = {
    seq: browserActionSeq,
    tMs: nowMs(),
    ...event,
  };

  runtime.host.__trueosBrowserLastAction = next;
  runtime.host.__trueosBrowserActionSeq = browserActionSeq;

  let queue = runtime.host.__trueosBrowserActionQueue;
  if (!Array.isArray(queue)) {
    queue = [];
    runtime.host.__trueosBrowserActionQueue = queue;
  }
  queue.push(next);
  if (queue.length > 128) {
    queue.splice(0, queue.length - 128);
  }

  return next;
}

function dispatchBrowserAction(kind, payload, hookName) {
  const event = pushBrowserAction({ kind, ...payload });
  const hook = runtime.host[hookName];
  if (typeof hook === 'function') {
    return hook(event);
  }
  return {
    ok: 1,
    simulated: 1,
    event,
  };
}

function paint() {
  const { vw, vh } = computeViewport();
  const doc = ensureDoc(vw);
  if (scrollY < 0) scrollY = 0;

  const overlayRuns = [];
  fpsOverlay.appendRuns(overlayRuns, vw);
  const releaseRect = pendingReleaseRect;
  pendingReleaseRect = null;

  renderScene(doc, vw, vh, scrollY, overlayRuns, releaseRect);

  return true;
}

function onWheelDelta(deltaY) {
  const next = Math.max(0, Math.round(scrollY + Number(deltaY || 0)));
  if (next === scrollY) return false;
  scrollY = next;
  paint();
  return true;
}

function queueCursorButtonEvent(event) {
  cursorButtonEvents.push(event);
  if (cursorButtonEvents.length > 128) {
    cursorButtonEvents.splice(0, cursorButtonEvents.length - 128);
  }
}

function logCursorDebug(message) {
  try {
    console.log(message);
  } catch (_) {}
}

function getOrCreateAiCursorSlot(cursorId) {
  const key = String(cursorId || '').trim();
  if (!key) return 0;
  const existing = aiCursorSlots.get(key);
  if (existing) return existing;

  const slotId = nextAiCursorSlot;
  nextAiCursorSlot += 1;
  aiCursorSlots.set(key, slotId);
  return slotId;
}

function rememberKernelCursor(slotId, x, y, buttonsDown, flags) {
  if (!Number.isFinite(slotId) || slotId <= 0) return 0;

  const id = Number(slotId) | 0;
  const nextButtons = Number(buttonsDown || 0) >>> 0;
  const nextX = Number(x || 0);
  const nextY = Number(y || 0);
  const nextFlags = Number(flags || 0) >>> 0;
  const prev = kernelCursorState.get(id);
  const prevButtons = prev ? (Number(prev.buttonsDown || 0) >>> 0) : 0;

  kernelCursorState.set(id, {
    slotId: id,
    x: nextX,
    y: nextY,
    buttonsDown: nextButtons,
    flags: nextFlags,
  });

  if (((nextFlags & CURSOR_FLAG_BUTTONS_CHANGED) !== 0) || nextButtons !== prevButtons) {
    if (prevButtons === 0 && nextButtons !== 0) {
      cursorDragOrigins.set(id, {
        x: nextX,
        y: nextY,
      });
    } else if (prevButtons !== 0 && nextButtons === 0) {
      const origin = cursorDragOrigins.get(id);
      if (origin) {
        pendingReleaseRect = {
          x: Number(origin.x || 0),
          y: Number(origin.y || 0),
          width: nextX - Number(origin.x || 0),
          height: nextY - Number(origin.y || 0),
          rgba: RELEASE_RECT_RGBA,
        };
      }
      cursorDragOrigins.delete(id);
    }
    logCursorDebug(`[browser.mjs] cursor buttons slot=${id} prev=${prevButtons} next=${nextButtons}`);
    queueCursorButtonEvent({
      slotId: id,
      x: nextX,
      y: nextY,
      buttonsDown: nextButtons,
      previousButtonsDown: prevButtons,
      flags: nextFlags,
    });
    return 1;
  }

  return 0;
}

function processCursorEvent(slotId, x, y, buttonsDown, wheel, flags, source = 'kernel') {
  let updated = rememberKernelCursor(slotId, x, y, buttonsDown, flags);

  if (wheel !== 0) {
    logCursorDebug(`[browser.mjs] cursor wheel source=${source} slot=${slotId} wheel=${wheel}`);
    const dy = Number(wheel) * -WHEEL_STEP_PX;
    if (onWheelDelta(dy)) updated += 1;
  }

  return updated;
}

function injectCursorEvent(event = null) {
  const source = event && typeof event === 'object' ? event : {};
  const slotId = Number(source.slotId || 0) | 0;
  const aiCursorId = typeof source.aiCursorId === 'string' || typeof source.aiCursorId === 'number'
    ? source.aiCursorId
    : '';
  const resolvedSlotId = slotId > 0 ? slotId : getOrCreateAiCursorSlot(aiCursorId);
  if (resolvedSlotId <= 0) return false;

  const x = Number(source.x || 0);
  const y = Number(source.y || 0);
  const buttonsDown = Number(source.buttonsDown || 0) >>> 0;
  const wheel = Number(source.wheel || 0) | 0;
  const flags = Number(source.flags || 0) >>> 0;

  processCursorEvent(resolvedSlotId, x, y, buttonsDown, wheel, flags, 'synthetic');
  return true;
}

function pumpCursorEvents() {
  const fn = runtime.host.__trueosReadCursorEventsSince;
  if (typeof fn !== 'function') return 0;

  let updated = 0;
  for (let batch = 0; batch < 8; batch++) {
    let packed = null;
    try {
      packed = fn(Number(cursorReadSeq || 0));
    } catch (_) {
      break;
    }
    if (!Array.isArray(packed) || packed.length < 3) break;

    const nextSeq = Number(packed[0] || cursorReadSeq || 0);
    const wrote = Math.max(0, Number(packed[2] || 0) | 0);
    let p = 3;
    for (let i = 0; i < wrote; i++) {
      if (p + 5 >= packed.length) break;
      const slotId = Number(packed[p + 0] || 0) | 0;
      const x = Number(packed[p + 1] || 0);
      const y = Number(packed[p + 2] || 0);
      const buttonsDown = Number(packed[p + 3] || 0) >>> 0;
      const wheel = Number(packed[p + 4] || 0) | 0;
      const flags = Number(packed[p + 5] || 0) >>> 0;

      updated += processCursorEvent(slotId, x, y, buttonsDown, wheel, flags, 'kernel');
      p += CURSOR_EVENT_FIELDS;
    }

    cursorReadSeq = nextSeq;
    if (wrote < 32) break;
  }

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
  if (AUTO_PAINT_MS <= 0) return;
  if (typeof host.setInterval !== 'function') return;
  try {
    host.setInterval(() => {
      paint();
    }, AUTO_PAINT_MS);
  } catch (_) {}
}

function normalizeAiSpecifier(specifier) {
  if (typeof specifier === 'string' && specifier) {
    return specifier;
  }
  return '/qjs/ai/ai_pc.mjs';
}

function startAi(specifier = '/qjs/ai/ai_pc.mjs', options = null) {
  const resolvedSpecifier = normalizeAiSpecifier(specifier);
  const opts = options && typeof options === 'object' ? options : null;
  const initialInput = opts && Object.prototype.hasOwnProperty.call(opts, 'input')
    ? normalizeAiInput(opts.input, opts)
    : null;
  if (aiStartPromise && aiStartSpecifier === resolvedSpecifier) {
    if (initialInput) {
      pushAiInput(initialInput);
    }
    return aiStartPromise;
  }
  if (aiWorker) {
    try { aiWorker.terminate(); } catch (_) {}
    aiWorker = null;
  }
  aiStartSpecifier = resolvedSpecifier;
  aiStartPromise = Promise.resolve()
    .then(() => {
      const worker = attachAiWorker(new Worker(buildAiWorkerSource(resolvedSpecifier, opts)));
      aiWorker = worker;
      try {
        console.log('[browser.mjs] ai worker started', resolvedSpecifier);
      } catch (_) {}
      if (initialInput) {
        pushAiInput(initialInput);
      }
      return worker;
    })
    .catch((err) => {
      aiStartPromise = null;
      aiStartSpecifier = '';
      aiWorker = null;
      try {
        console.log('[browser.mjs] ai worker start failed', String(err && err.stack ? err.stack : err));
      } catch (_) {}
      throw err;
    });
  return aiStartPromise;
}

function maybeAutostartAi() {
  const cfg = runtime.host.__trueosBrowserAutoStartAi;
  if (!cfg) return;
  if (cfg === true) {
    void startAi('/qjs/ai/ai_pc.mjs');
    return;
  }
  if (typeof cfg === 'string') {
    void startAi(cfg);
    return;
  }
  if (typeof cfg === 'object') {
    const specifier = typeof cfg.specifier === 'string' && cfg.specifier ? cfg.specifier : '/qjs/ai/ai_pc.mjs';
    void startAi(specifier, cfg);
  }
}

runtime.host.__trueosBrowser = {
  paint,
  setHtml,
  setNodeHtml,
  insertHtml,
  injectCursorEvent,
  getKernelCursors() {
    return Array.from(kernelCursorState.values()).sort((a, b) => a.slotId - b.slotId);
  },
  getKernelCursor(slotId) {
    const id = Number(slotId || 0) | 0;
    if (id <= 0) return null;
    const state = kernelCursorState.get(id);
    return state ? { ...state } : null;
  },
  popCursorButtonEvent() {
    if (cursorButtonEvents.length <= 0) return null;
    return cursorButtonEvents.shift() || null;
  },
  getApiContract() {
    return cloneApiContract();
  },
  listUnavailable() {
    return cloneApiContract().unavailable;
  },
  getHtml() {
    return cachedHtml;
  },
  getTextRows() {
    return getRows();
  },
  getDomSnapshot() {
    return getDomSnapshot();
  },
  getViewport() {
    return getViewport();
  },
  startAi(specifier, options) {
    return startAi(specifier, options);
  },
  startAiPc(input, options = null) {
    const opts = options && typeof options === 'object' ? { ...options, input } : { input };
    return startAi('/qjs/ai/ai_pc.mjs', opts);
  },
  submitAiInput(input, options = null) {
    return pushAiInput(input, options);
  },
  setScroll(y) {
    scrollY = Math.max(0, Math.round(Number(y || 0)));
    paint();
  },
  click(target = null) {
    const payload = target && typeof target === 'object' && !Array.isArray(target)
      ? { target: { ...target } }
      : { target: target == null ? null : { value: target } };
    return dispatchBrowserAction('click', payload, '__trueosBrowserClick');
  },
  navigate(input = null) {
    const request = input && typeof input === 'object' && !Array.isArray(input)
      ? { ...input }
      : { url: String(input || '') };
    const url = typeof request.url === 'string' ? request.url.trim() : '';
    if (url) {
      currentPageUrl = url;
      runtime.host.__trueosBrowserCurrentUrl = currentPageUrl;
    }
    return dispatchBrowserAction('navigate', {
      url: currentPageUrl,
      request,
    }, '__trueosBrowserNavigate');
  },
  typeText() {
    return notYetAvailable('typeText');
  },
  pressKey(key, options = null) {
    const keyName = typeof key === 'string'
      ? key
      : String(key && typeof key === 'object' && key.key != null ? key.key : '');
    return dispatchBrowserAction('pressKey', {
      key: keyName,
      options: options && typeof options === 'object' ? { ...options } : null,
    }, '__trueosBrowserPressKey');
  },
  captureScreenshot() {
    if (typeof runtime.host.__trueosCaptureScreenshot !== 'function') {
      return notYetAvailable('captureScreenshot');
    }
    const image = runtime.host.__trueosCaptureScreenshot();
    if (typeof image !== 'string' || !image) {
      return notYetAvailable('captureScreenshot');
    }
    return image;
  },
};

if (typeof (runtime.host.window || runtime.host).addEventListener === 'function') {
  (runtime.host.window || runtime.host).addEventListener('resize', paint);
}

setHtml(runtime.host.__trueosUiHtml || '');
installAiInputBridge();
paint();
startWheelPump();
startAutoPaint();
maybeAutostartAi();