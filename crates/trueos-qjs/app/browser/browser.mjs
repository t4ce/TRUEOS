import * as parse5 from 'parse5';
import * as cmdStream from 'trueos:cmd_stream';
import Yoga from 'yoga-layout';
import { Buffer } from 'node:buffer';
import { Worker } from 'node:worker_threads';
import { createFpsOverlay } from './fps.mjs';
import { extractCssSection, resolveNodeStyle } from './css.mjs';
import {
  attachThemeLayoutRuntime as attachParryThemeLayoutRuntime,
  dispatchDomClick as dispatchParryDomClick,
} from './parry.mjs';
import { LI_TEXT_X_OFFSET, renderScene } from './scene.mjs';
import { BLOCK_TAGS, TEXT_LEVEL_SEMANTICS_TAGS } from './htmlDefaults.mjs';
import { LEFT_PAD, TOP_PAD, LINE_H, FONT_PX } from './theme.mjs';

const runtime = resolveRuntime();

const AI_CURSOR_SLOT_BASE = 10000;
const DEFAULT_AI_CURSOR_ID = 'ai-default';
const WHEEL_STEP_PX = 16;
const CURSOR_EVENT_FIELDS = 6;
const CURSOR_FLAG_BUTTONS_CHANGED = 1 << 2;
const INDENT_PX = 12;
const AUTO_PAINT_MS = Math.max(0, Number(runtime.host.__trueosBrowserAutoPaintMs || 0) || 0);
const ERROR_PREVIEW_MAX = 160;
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
let cursorMoveLogCount = 0;
const pendingSyntheticCursorEchoes = [];
const imageTextureCache = new Map();
const imageTextureLoads = new Map();

const fpsOverlay = createFpsOverlay();
const DEFAULT_AI_INPUT_OPTIONS = Object.freeze({
  webSearch: false,
  fileSearch: false,
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
    'getTrueosFsTreeHtml',
    'setNodeHtml',
    'insertHtml',
    'getViewport',
    'paint',
    'setScroll',
    'moveCursor',
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

function getTrueosFsTreeHtml(maxEntries = 64) {
  if (typeof runtime.host.__trueosReadPrimaryTrueosFsTreeHtml !== 'function') {
    return null;
  }
  const limit = Number(maxEntries);
  const normalized = Number.isFinite(limit) && limit > 0 ? Math.floor(limit) : 64;
  const html = runtime.host.__trueosReadPrimaryTrueosFsTreeHtml(normalized);
  return typeof html === 'string' && html ? html : null;
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
    fileSearch: !!cfg.fileSearch,
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

function cursorCoordToViewportPx(value, extent) {
  const next = Number(value);
  const size = Math.max(1, Number(extent || 0));
  if (!Number.isFinite(next)) return 0;
  if (next >= 0 && next <= 1) {
    return Math.round(next * size);
  }
  return Math.round(next);
}

function buildReleaseRect(origin, currentX, currentY) {
  if (!origin || typeof origin !== 'object') return null;

  const { vw, vh } = computeViewport();
  const startX = cursorCoordToViewportPx(origin.x, vw);
  const startY = cursorCoordToViewportPx(origin.y, vh);
  const endX = cursorCoordToViewportPx(currentX, vw);
  const endY = cursorCoordToViewportPx(currentY, vh);
  const left = Math.min(startX, endX);
  const top = Math.min(startY, endY);
  const width = Math.abs(endX - startX);
  const height = Math.abs(endY - startY);

  if (width <= 0 || height <= 0) return null;
  return {
    x: left,
    y: top,
    width,
    height,
    rgba: RELEASE_RECT_RGBA,
  };
}

function collapseWhitespace(s) {
  return String(s || '').replace(/\s+/g, ' ').trim();
}

function summarizeForError(value, maxLen = ERROR_PREVIEW_MAX) {
  const text = collapseWhitespace(value);
  if (!text) return '(empty)';
  if (text.length <= maxLen) return text;
  return `${text.slice(0, Math.max(1, maxLen - 3))}...`;
}

function describeError(err) {
  if (err && typeof err.message === 'string' && err.message.trim()) {
    return err.message.trim();
  }
  const text = String(err || '').trim();
  return text || 'unknown error';
}

function detailSuffix(details = null) {
  if (!details || typeof details !== 'object') return '';
  const parts = [];
  const keys = Object.keys(details);
  for (let i = 0; i < keys.length; i += 1) {
    const key = keys[i];
    const raw = details[key];
    if (raw == null || raw === '') continue;
    parts.push(`${key}=${summarizeForError(raw)}`);
  }
  return parts.length > 0 ? ` [${parts.join(' ')}]` : '';
}

function dataUrlToBytes(url) {
  const value = String(url || '');
  const match = value.match(/^data:([^,]*?),(.*)$/s);
  if (!match) {
    raiseBrowserError('TRUEOS_BROWSER_IMAGE_DATA_URL_INVALID', 'Image data URL could not be decoded', {
      url: value,
    });
  }

  const meta = String(match[1] || '');
  const payload = String(match[2] || '');
  const parts = meta.split(';').map((part) => String(part || '').trim().toLowerCase()).filter(Boolean);
  const mime = parts.length > 0 && !parts[0].includes('=') ? parts[0] : 'text/plain';
  const isBase64 = parts.includes('base64');

  if (isBase64) {
    const decoded = Buffer.from(payload, 'base64');
    return {
      mime,
      bytes: new Uint8Array(decoded.buffer, decoded.byteOffset, decoded.byteLength).slice(),
    };
  }

  const text = decodeURIComponent(payload);
  const bytes = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i += 1) {
    bytes[i] = text.charCodeAt(i) & 0xFF;
  }
  return { mime, bytes };
}

function looksLikePng(bytes) {
  return !!bytes
    && bytes.length >= 8
    && bytes[0] === 0x89
    && bytes[1] === 0x50
    && bytes[2] === 0x4E
    && bytes[3] === 0x47
    && bytes[4] === 0x0D
    && bytes[5] === 0x0A
    && bytes[6] === 0x1A
    && bytes[7] === 0x0A;
}

function looksLikeBmp(bytes) {
  return !!bytes && bytes.length >= 2 && bytes[0] === 0x42 && bytes[1] === 0x4D;
}

function queueImageRepaint() {
  if (typeof runtime.host.requestAnimationFrame === 'function') {
    runtime.host.requestAnimationFrame(() => {
      try {
        paint();
      } catch (_) {}
    });
    return;
  }
  try {
    paint();
  } catch (_) {}
}

async function fetchImageBytes(url) {
  const value = String(url || '').trim();
  if (!value) {
    raiseBrowserError('TRUEOS_BROWSER_IMAGE_URL_MISSING', 'Image fetch failed because src was empty');
  }

  if (value.startsWith('data:')) {
    return dataUrlToBytes(value);
  }
  if (typeof fetch !== 'function') {
    raiseBrowserError('TRUEOS_BROWSER_IMAGE_FETCH_UNAVAILABLE', 'Image fetch failed because fetch is unavailable', {
      url: value,
    });
  }

  const response = await fetch(value, { method: 'GET' });
  if (!response || response.ok === false) {
    raiseBrowserError('TRUEOS_BROWSER_IMAGE_FETCH_FAILED', 'Image fetch returned an error response', {
      url: value,
      status: response && typeof response.status !== 'undefined' ? response.status : 'unknown',
    });
  }
  if (typeof response.arrayBuffer !== 'function') {
    raiseBrowserError('TRUEOS_BROWSER_IMAGE_FETCH_BINARY_UNAVAILABLE', 'Image fetch could not read binary response data', {
      url: value,
    });
  }

  const buffer = await response.arrayBuffer();
  const mime = response && response.headers && typeof response.headers.get === 'function'
    ? String(response.headers.get('content-type') || '')
    : '';
  return {
    mime,
    bytes: new Uint8Array(buffer),
  };
}

function resolveImageUploadKind(url, mime, bytes) {
  const normalizedUrl = String(url || '').toLowerCase();
  const normalizedMime = String(mime || '').toLowerCase();
  if (normalizedMime.includes('image/png') || /\.png(?:$|[?#])/.test(normalizedUrl) || looksLikePng(bytes)) {
    return 'png';
  }
  if (normalizedMime.includes('image/bmp') || normalizedMime.includes('image/x-ms-bmp') || /\.bmp(?:$|[?#])/.test(normalizedUrl) || looksLikeBmp(bytes)) {
    return 'bmp';
  }
  return '';
}

async function ensureImageTexture(resolvedUrl) {
  const cacheKey = String(resolvedUrl || '').trim();
  if (!cacheKey) return null;

  const cached = imageTextureCache.get(cacheKey) || null;
  if (cached && (cached.state === 'ready' || cached.state === 'error')) {
    return cached;
  }

  const inFlight = imageTextureLoads.get(cacheKey) || null;
  if (inFlight) {
    return inFlight;
  }

  imageTextureCache.set(cacheKey, {
    state: 'loading',
    texId: 0,
    url: cacheKey,
    mime: '',
    error: '',
  });

  const task = (async () => {
    try {
      const { mime, bytes } = await fetchImageBytes(cacheKey);
      const kind = resolveImageUploadKind(cacheKey, mime, bytes);
      if (kind === 'bmp') {
        throw new Error('bmp upload is not wired yet');
      }
      if (kind !== 'png') {
        throw new Error('unsupported image format');
      }

      const texId = Number(cmdStream.createTexturePng(bytes) || 0);
      if (!Number.isFinite(texId) || texId <= 0) {
        throw new Error('texture upload failed');
      }

      const ready = {
        state: 'ready',
        texId,
        url: cacheKey,
        mime: String(mime || 'image/png'),
        error: '',
      };
      imageTextureCache.set(cacheKey, ready);
      return ready;
    } catch (err) {
      const failed = {
        state: 'error',
        texId: 0,
        url: cacheKey,
        mime: '',
        error: describeError(err),
      };
      imageTextureCache.set(cacheKey, failed);
      return failed;
    } finally {
      imageTextureLoads.delete(cacheKey);
      queueImageRepaint();
    }
  })();

  imageTextureLoads.set(cacheKey, task);
  return task;
}

function applyImageResourcesToRows(rows) {
  const list = Array.isArray(rows) ? rows : [];
  for (let i = 0; i < list.length; i += 1) {
    const row = list[i];
    if (!row || String(row.kind || '') !== 'image') continue;
    const rawSrc = String(row.src || '').trim();
    const resolvedSrc = rawSrc ? resolveNavigationUrl(rawSrc) : '';
    row.resolvedSrc = resolvedSrc;
    row.texId = 0;
    if (!resolvedSrc) continue;
    const cached = imageTextureCache.get(resolvedSrc) || null;
    if (cached && cached.state === 'ready') {
      row.texId = Number(cached.texId || 0);
    }
  }
}

function primeImageRows(rows) {
  const list = Array.isArray(rows) ? rows : [];
  for (let i = 0; i < list.length; i += 1) {
    const row = list[i];
    if (!row || String(row.kind || '') !== 'image') continue;
    const resolvedSrc = String(row.resolvedSrc || '').trim();
    if (!resolvedSrc) continue;
    const cached = imageTextureCache.get(resolvedSrc) || null;
    if (cached && (cached.state === 'ready' || cached.state === 'loading' || cached.state === 'error')) {
      continue;
    }
    void ensureImageTexture(resolvedSrc);
  }
}

function createBrowserError(code, message, details = null, cause = null) {
  const err = new Error(`${message}${detailSuffix(details)}`);
  err.code = String(code || 'TRUEOS_BROWSER_ERROR');
  if (details && typeof details === 'object') err.details = { ...details };
  if (cause) err.cause = cause;
  runtime.host.__trueosBrowserLastError = {
    code: err.code,
    message: err.message,
    details: err.details || null,
  };
  return err;
}

function raiseBrowserError(code, message, details = null, cause = null) {
  throw createBrowserError(code, message, details, cause);
}

function isElement(node) {
  return !!node && typeof node === 'object' && typeof node.tagName === 'string';
}

function isTextNode(node) {
  return !!node && typeof node === 'object' && node.nodeName === '#text' && typeof node.value === 'string';
}

function pushRow(rows, text, depth, kind = 'text', style = null, meta = null) {
  const t = collapseWhitespace(text);
  if (!t) return;
  const row = {
    depth: Math.max(0, Number(depth || 0) | 0),
    text: t,
    kind: String(kind || 'text'),
    style,
  };
  if (meta && typeof meta === 'object') {
    if (typeof meta.path === 'string' && meta.path) row.path = meta.path;
    if (typeof meta.targetPath === 'string' && meta.targetPath) row.targetPath = meta.targetPath;
    if (typeof meta.targetTag === 'string' && meta.targetTag) row.targetTag = meta.targetTag;
    if (typeof meta.href === 'string' && meta.href) row.href = meta.href;
  }
  rows.push(row);
}

function pushImageRow(rows, depth, widthPx, heightPx, style = null, meta = null, text = 'img') {
  const row = {
    depth: Math.max(0, Number(depth || 0) | 0),
    text: collapseWhitespace(text) || 'img',
    kind: 'image',
    style,
    widthPx: Math.max(1, Math.round(Number(widthPx || 0) || 1)),
    heightPx: Math.max(1, Math.round(Number(heightPx || 0) || 1)),
  };
  if (meta && typeof meta === 'object') {
    if (typeof meta.path === 'string' && meta.path) row.path = meta.path;
    if (typeof meta.src === 'string' && meta.src) row.src = meta.src;
  }
  rows.push(row);
}

function pushHrRow(rows, depth, style = null, meta = null) {
  const row = {
    depth: Math.max(0, Number(depth || 0) | 0),
    text: '',
    kind: 'hr',
    style,
    heightPx: 1 + (TOP_PAD * 2),
    ruleHeightPx: 1,
  };
  if (meta && typeof meta === 'object') {
    if (typeof meta.path === 'string' && meta.path) row.path = meta.path;
  }
  rows.push(row);
}

function parsePositiveAttrPx(node, name, fallback = 0) {
  const raw = String(getNodeAttr(node, name) || '').trim();
  if (!raw) return Math.max(0, Number(fallback || 0) || 0);
  const value = Number.parseFloat(raw);
  if (!Number.isFinite(value) || value <= 0) {
    return Math.max(0, Number(fallback || 0) || 0);
  }
  return value;
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

function detectLabelMarkerKind(node) {
  const kids = Array.isArray(node && node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i += 1) {
    const kid = kids[i];
    if (!isElement(kid) || String(kid.tagName || '').toLowerCase() !== 'input') continue;
    const type = String(getNodeAttr(kid, 'type') || '').trim().toLowerCase();
    if (type === 'checkbox') return 'checkbox-text';
    if (type === 'radio') return 'radio-text';
  }
  return 'text';
}

function collectRows(node, depth, rows, cssSection, parentMeta = null, path = 'root', ancestors = []) {
  if (!node || typeof node !== 'object') return;

  if (isTextNode(node)) {
    const parent = String(parentMeta && parentMeta.tag || '').toLowerCase();
    const kind = parent === 'title'
      ? 'title-text'
      : (parent === 'summary'
        ? 'summary-text'
        : (parent === 'label' && parentMeta && typeof parentMeta.markerKind === 'string'
          ? parentMeta.markerKind
          : (parent === 'li'
            ? 'li-text'
            : (parent === 'a'
              ? 'link-text'
              : (parent === 'button' ? 'button-text' : 'text')))))
    pushRow(rows, node.value, depth, kind, parentMeta && parentMeta.style ? parentMeta.style : null, {
      path: String(path || 'root'),
      targetPath: parent === 'a' || parent === 'button' ? String(parentMeta && parentMeta.path || '') : '',
      targetTag: parent,
      href: parent === 'a' ? String(parentMeta && parentMeta.href || '') : '',
    });
    return;
  }

  if (isElement(node)) {
    const tag = String(node.tagName || '').toLowerCase();
    const style = resolveNodeStyle(node, path, cssSection, ancestors, parentMeta && parentMeta.style ? parentMeta.style : null);
    if (tag === 'hr') {
      pushHrRow(rows, depth, style, { path: String(path || 'root') });
      return;
    }
    if (tag === 'img') {
      const widthPx = parsePositiveAttrPx(node, 'width', 160);
      const heightPx = parsePositiveAttrPx(node, 'height', widthPx > 0 ? widthPx : 120);
      pushImageRow(
        rows,
        depth,
        widthPx,
        heightPx,
        style,
        { path: String(path || 'root'), src: String(getNodeAttr(node, 'src') || '') },
        String(getNodeAttr(node, 'alt') || 'img'),
      );
      return;
    }
    const renderTagLines = !shouldOmitElement(tag) && shouldRenderTagLines(tag);
    if (renderTagLines && SHOW_CLOSING_TAG_ROWS) pushRow(rows, `<${tag}>`, depth, 'tag-open', style, { path: String(path || 'root') });
    const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
    const nextAncestors = ancestors.concat([{ node, path }]);
    const nextParentMeta = {
      tag,
      path: String(path || 'root'),
      style,
      href: tag === 'a' ? String(getNodeAttr(node, 'href') || '') : '',
      markerKind: tag === 'label' ? detectLabelMarkerKind(node) : 'text',
    };
    for (let i = 0; i < kids.length; i++) {
      collectRows(kids[i], renderTagLines ? depth + 1 : depth, rows, cssSection, nextParentMeta, `${path}.${i}`, nextAncestors);
    }
    if (renderTagLines && SHOW_CLOSING_TAG_ROWS) pushRow(rows, `</${tag}>`, depth, 'tag-close', style, { path: String(path || 'root') });
    return;
  }

  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i++) {
    collectRows(kids[i], depth, rows, cssSection, parentMeta, `${path}.${i}`, ancestors);
  }
}

function buildInteractiveRowRects(rows, rowX, rowY) {
  const out = Object.create(null);
  const list = Array.isArray(rows) ? rows : [];
  const xs = Array.isArray(rowX) ? rowX : [];
  const ys = Array.isArray(rowY) ? rowY : [];
  for (let i = 0; i < list.length; i += 1) {
    const row = list[i];
    const targetPath = typeof row.targetPath === 'string' ? row.targetPath : '';
    if (!targetPath) continue;
    const isLink = String(row.targetTag || '').toLowerCase() === 'a' || String(row.kind || '') === 'link-text';
    const nextRect = {
      x: Math.round(Number(xs[i] ?? LEFT_PAD)),
      y: Math.round(Number(ys[i] ?? (i * LINE_H))),
      width: Math.max(1, estimateTextWidthPx(String(row.text || ''), FONT_PX) + (isLink ? LI_TEXT_X_OFFSET : 0)),
      height: LINE_H,
    };
    const prev = out[targetPath];
    if (!prev) {
      out[targetPath] = nextRect;
      continue;
    }
    const minX = Math.min(prev.x, nextRect.x);
    const minY = Math.min(prev.y, nextRect.y);
    const maxX = Math.max(prev.x + prev.width, nextRect.x + nextRect.width);
    const maxY = Math.max(prev.y + prev.height, nextRect.y + nextRect.height);
    out[targetPath] = {
      x: minX,
      y: minY,
      width: Math.max(1, maxX - minX),
      height: Math.max(1, maxY - minY),
    };
  }
  return out;
}

function alignThemeLayoutToRows(themeLayout, rows, rowX, rowY) {
  if (!themeLayout || typeof themeLayout !== 'object') return themeLayout;
  const interactives = Array.isArray(themeLayout.interactives) ? themeLayout.interactives : [];
  if (interactives.length <= 0) return themeLayout;
  const rowRects = buildInteractiveRowRects(rows, rowX, rowY);
  const nextInteractives = [];
  const nextButtons = [];
  const nextByPath = Object.create(null);
  for (let i = 0; i < interactives.length; i += 1) {
    const entry = interactives[i];
    const rect = rowRects[String(entry && entry.path || '')] || null;
    const next = rect ? { ...entry, ...rect } : { ...entry };
    nextInteractives.push(next);
    if (next.kind === 'button') nextButtons.push(next);
    if (typeof next.path === 'string' && next.path) nextByPath[next.path] = next;
  }
  return {
    ...themeLayout,
    interactives: nextInteractives,
    buttons: nextButtons,
    byPath: nextByPath,
  };
}

function viewportThemeLayout(themeLayout) {
  const source = themeLayout && typeof themeLayout === 'object' ? themeLayout : null;
  const interactives = Array.isArray(source && source.interactives) ? source.interactives : [];
  const nextInteractives = [];
  const nextButtons = [];
  const nextByPath = Object.create(null);
  for (let i = 0; i < interactives.length; i += 1) {
    const entry = interactives[i];
    const next = {
      ...entry,
      x: Math.round(Number(entry && entry.x || 0)),
      y: Math.round(Number(entry && entry.y || 0) - Number(scrollY || 0)),
      width: Math.max(1, Math.round(Number(entry && entry.width || 0))),
      height: Math.max(1, Math.round(Number(entry && entry.height || 0))),
    };
    nextInteractives.push(next);
    if (next.kind === 'button') nextButtons.push(next);
    if (typeof next.path === 'string' && next.path) nextByPath[next.path] = next;
  }
  return {
    interactives: nextInteractives,
    buttons: nextButtons,
    byPath: nextByPath,
  };
}

function publishThemeLayoutInteractives(themeLayout) {
  const viewportLayout = viewportThemeLayout(themeLayout);
  runtime.host.__trueosBrowserThemeLayoutInteractives = viewportLayout.interactives;
  runtime.host.__trueosBrowserThemeLayoutButtons = viewportLayout.buttons;
  return viewportLayout;
}

function buildDocFromParsed(parsed, vw, context = 'document') {
  const doc = parsed && typeof parsed === 'object'
    ? parsed
    : parse5.parse('');

  const rows = [];
  const cssSection = (() => {
    try {
      return typeof extractCssSection === 'function' ? extractCssSection(doc) : null;
    } catch (err) {
      raiseBrowserError(
        'TRUEOS_BROWSER_CSS_PARSE_FAILED',
        `CSS extraction failed while building ${context}`,
        { context, reason: describeError(err) },
        err,
      );
    }
  })();
  try {
    collectRows(doc, 0, rows, cssSection, null, 'root', []);
  } catch (err) {
    raiseBrowserError(
      'TRUEOS_BROWSER_DOM_BUILD_FAILED',
      `DOM row collection failed while building ${context}`,
      { context, reason: describeError(err) },
      err,
    );
  }
  const layout = applyYoga(rows, vw, context);
  const themeLayout = (() => {
    try {
      return buildThemeLayout(doc, cssSection, vw, context, rows, layout.rowX, layout.rowY);
    } catch (err) {
      raiseBrowserError(
        'TRUEOS_BROWSER_THEME_LAYOUT_FAILED',
        `Theme layout build failed while building ${context}`,
        { context, reason: describeError(err) },
        err,
      );
    }
  })();
  attachParryThemeLayoutRuntime(doc, themeLayout, {
    isElement,
    dispatchBrowserAction,
  });
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
  publishThemeLayoutInteractives(themeLayout);
  return {
    dom: doc,
    css: cssSection,
    themeLayout,
    rows,
    rowX: layout.rowX,
    rowY: layout.rowY,
    contentH: layout.contentH,
    width: vw,
  };
}

function buildDocFromHtml(html, vw, context = 'document') {
  const source = String(html || '');
  let parsed;
  try {
    parsed = parse5.parse(source);
  } catch (err) {
    raiseBrowserError(
      'TRUEOS_BROWSER_HTML_PARSE_FAILED',
      `HTML parse failed while building ${context}`,
      { context, reason: describeError(err), html: source },
      err,
    );
  }
  return buildDocFromParsed(parsed, vw, context);
}

function readNodeText(node) {
  if (!node || typeof node !== 'object') return '';
  if (isTextNode(node)) return String(node.value || '');
  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  let out = '';
  for (let i = 0; i < kids.length; i++) {
    out += readNodeText(kids[i]);
  }
  return out;
}

function estimateTextWidthPx(text, fontSizePx = FONT_PX) {
  const value = String(text || '');
  const fontPx = Math.max(1, Number(fontSizePx || FONT_PX) || FONT_PX);
  const baseFontPx = Math.max(1, Number(runtime.host.__trueosBrowserDefaultFontPx || FONT_PX) || FONT_PX);
  const widthTable = Array.isArray(runtime.host.__trueosBrowserTextWidthByChar)
    ? runtime.host.__trueosBrowserTextWidthByChar
    : null;
  const scale = fontPx / baseFontPx;
  if (!value) return Math.max(4, Math.round(baseFontPx * 0.5 * scale));
  if (!widthTable || widthTable.length < 256) {
    const glyphPx = Math.max(6, Math.round(fontPx * 0.56));
    return Math.max(glyphPx, value.length * glyphPx);
  }

  let total = 0;
  for (let i = 0; i < value.length; i += 1) {
    const code = value.charCodeAt(i);
    if (code === 10 || code === 13) continue;
    if (code >= 0 && code < 256) {
      total += Number(widthTable[code] || 0);
      continue;
    }
    total += Number(widthTable[63] || 0);
  }
  return Math.max(1, Math.round(total * scale));
}

function collectThemeLayoutInteractives(node, cssSection, parentStyle = null, path = 'root', ancestors = [], out = []) {
  if (!node || typeof node !== 'object') return out;

  if (isElement(node)) {
    const tag = String(node.tagName || '').toLowerCase();
    const style = resolveNodeStyle(node, path, cssSection, ancestors, parentStyle);
    const nextAncestors = ancestors.concat([{ node, path }]);
    const href = String(getNodeAttr(node, 'href') || '');
    if (tag === 'button' || (tag === 'a' && href)) {
      const caption = collapseWhitespace(readNodeText(node)) || String(getNodeAttr(node, 'value') || href || tag);
      out.push({
        path,
        tag,
        kind: tag === 'button' ? 'button' : 'link',
        caption,
        href,
        style,
      });
    }
    const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
    for (let i = 0; i < kids.length; i++) {
      collectThemeLayoutInteractives(kids[i], cssSection, style, `${path}.${i}`, nextAncestors, out);
    }
    return out;
  }

  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i++) {
    collectThemeLayoutInteractives(kids[i], cssSection, parentStyle, `${path}.${i}`, ancestors, out);
  }
  return out;
}

function applyThemeLayoutYoga(entries, vw, context = 'document') {
  let root = null;
  try {
    root = Yoga.Node.create();
    root.setFlexDirection(Yoga.FLEX_DIRECTION_COLUMN);
    root.setAlignItems(Yoga.ALIGN_FLEX_START);
    root.setWidth(vw);
    root.setPadding(Yoga.EDGE_LEFT, LEFT_PAD);
    root.setPadding(Yoga.EDGE_TOP, TOP_PAD);

    const nodes = [];
    for (let i = 0; i < entries.length; i++) {
      const entry = entries[i];
      const style = entry && entry.style && typeof entry.style === 'object' ? entry.style : {};
      const node = Yoga.Node.create();
      node.setMargin(Yoga.EDGE_LEFT, Number(style.marginLeftPx || 0));
      node.setMargin(Yoga.EDGE_TOP, Number(style.marginTopPx || 0));
      node.setMargin(Yoga.EDGE_RIGHT, Number(style.marginRightPx || 0));
      node.setMargin(Yoga.EDGE_BOTTOM, Number(style.marginBottomPx || 0));
      node.setMeasureFunc(() => {
        const fontPx = Math.max(1, Number(style.fontSizePx || FONT_PX) || FONT_PX);
        const paddingLeft = Math.max(0, Number(style.paddingLeftPx || 0));
        const paddingTop = Math.max(0, Number(style.paddingTopPx || 0));
        const paddingRight = Math.max(0, Number(style.paddingRightPx || 0));
        const paddingBottom = Math.max(0, Number(style.paddingBottomPx || 0));
        const textW = estimateTextWidthPx(entry && entry.caption ? entry.caption : entry.tag, fontPx);
        const lineH = Math.max(LINE_H, Math.ceil(fontPx * 1.35));
        return {
          width: textW + paddingLeft + paddingRight,
          height: lineH + paddingTop + paddingBottom,
        };
      });
      root.insertChild(node, i);
      nodes.push(node);
    }

    root.calculateLayout(vw, NaN, Yoga.DIRECTION_LTR);

    const interactives = [];
    const buttons = [];
    const byPath = Object.create(null);
    for (let i = 0; i < nodes.length; i++) {
      const entry = entries[i];
      const rect = {
        x: Math.round(Number(nodes[i].getComputedLeft() || 0)),
        y: Math.round(Number(nodes[i].getComputedTop() || 0)),
        width: Math.max(1, Math.round(Number(nodes[i].getComputedWidth() || 0))),
        height: Math.max(1, Math.round(Number(nodes[i].getComputedHeight() || 0))),
      };
      const layout = {
        ...rect,
        path: entry.path,
        tag: entry.tag,
        kind: entry.kind,
        caption: entry.caption,
        href: entry.href,
      };
      interactives.push(layout);
      if (entry.kind === 'button') {
        buttons.push(layout);
      }
      byPath[entry.path] = layout;
    }

    return {
      interactives,
      buttons,
      byPath,
      contentH: Math.max(1, Math.round(Number(root.getComputedHeight() || 0))),
      viewportWidth: vw,
      context,
    };
  } finally {
    if (root) {
      try {
        root.freeRecursive();
      } catch (_) {}
    }
  }
}

function buildThemeLayout(doc, cssSection, vw, context = 'document', rows = [], rowX = [], rowY = []) {
  const entries = collectThemeLayoutInteractives(doc, cssSection, null, 'root', []);
  if (entries.length <= 0) {
    return {
      interactives: [],
      buttons: [],
      byPath: Object.create(null),
      contentH: 0,
      viewportWidth: vw,
      context,
    };
  }
  return alignThemeLayoutToRows(applyThemeLayoutYoga(entries, vw, context), rows, rowX, rowY);
}

function applyYoga(rows, vw, context = 'document') {
  let root = null;
  try {
    root = Yoga.Node.create();
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
      if (r.kind === 'image') {
        const imageW = Math.max(1, Math.round(Number(r.widthPx || 0) || 1));
        const imageH = Math.max(1, Math.round(Number(r.heightPx || 0) || 1));
        const maxW = Math.max(1, vw - (LEFT_PAD * 2) - indent);
        n.setWidth(Math.min(imageW, maxW));
        n.setHeight(imageH);
        n.setMinHeight(imageH);
        n.setMargin(Yoga.EDGE_LEFT, indent);
      } else if (r.kind === 'hr') {
        const hrH = Math.max(1, Math.round(Number(r.heightPx || 0) || 1));
        n.setWidth(Math.max(1, vw - (LEFT_PAD * 2) - indent));
        n.setHeight(hrH);
        n.setMinHeight(hrH);
        n.setMargin(Yoga.EDGE_LEFT, indent);
      } else if (r.kind === 'title-text') {
        n.setHeight(LINE_H);
        n.setMinHeight(LINE_H);
        // Draw path places text at node-left, so center by placing a content-width node
        // at a centered left margin within the same inner row width as normal rows.
        const textW = Math.max(1, estimateTextWidthPx(String(r.text || ''), FONT_PX));
        const innerRowW = Math.max(1, vw - (LEFT_PAD * 2));
        const centeredLeft = Math.max(0, Math.floor((innerRowW - textW) * 0.5));
        n.setWidth(textW);
        n.setMargin(Yoga.EDGE_LEFT, centeredLeft);
      } else {
        n.setHeight(LINE_H);
        n.setMinHeight(LINE_H);
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
    return { rowX, rowY, contentH };
  } catch (err) {
    raiseBrowserError(
      'TRUEOS_BROWSER_LAYOUT_FAILED',
      `Yoga layout failed while building ${context}`,
      { context, reason: describeError(err), viewportWidth: vw, rowCount: Array.isArray(rows) ? rows.length : 0 },
      err,
    );
  } finally {
    if (root) {
      try {
        root.freeRecursive();
      } catch (_) {}
    }
  }
}

function ensureDoc(vw) {
  if (!cachedDoc) {
    cachedDoc = buildDocFromHtml(cachedHtml, vw, 'cached html document');
  } else if (cachedDoc.width !== vw) {
    cachedDoc = buildDocFromParsed(cachedDoc.dom, vw, 'cached html document');
  }
  applyImageResourcesToRows(cachedDoc && cachedDoc.rows ? cachedDoc.rows : []);
  primeImageRows(cachedDoc && cachedDoc.rows ? cachedDoc.rows : []);
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

function getNodeAttr(node, name) {
  const attrs = Array.isArray(node && node.attrs) ? node.attrs : [];
  for (let i = 0; i < attrs.length; i += 1) {
    const attr = attrs[i];
    if (!attr || typeof attr.name !== 'string') continue;
    if (attr.name === name) return String(attr.value || '');
  }
  return '';
}

function findFirstNode(root, predicate) {
  if (!root || typeof root !== 'object' || typeof predicate !== 'function') return null;
  const queue = [root];
  while (queue.length > 0) {
    const node = queue.shift();
    if (!node || typeof node !== 'object') continue;
    if (predicate(node)) return node;
    const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
    for (let i = 0; i < kids.length; i += 1) {
      queue.push(kids[i]);
    }
  }
  return null;
}

function resolveDomTargetNode(root, target) {
  if (!root || typeof root !== 'object') return null;
  const rawTarget = normalizeDomTarget(target);
  if (!rawTarget) return null;

  const byPath = getNodeByPath(root, rawTarget);
  if (byPath) return byPath;

  const lower = rawTarget.toLowerCase();
  if (lower === 'document' || lower === 'root') return root;
  if (lower === 'html' || lower === 'body') {
    return findFirstNode(root, (node) => isElement(node) && String(node.tagName || '').toLowerCase() === lower);
  }
  if (rawTarget.startsWith('#')) {
    const wantedId = rawTarget.slice(1);
    if (!wantedId) return null;
    return findFirstNode(root, (node) => isElement(node) && getNodeAttr(node, 'id') === wantedId);
  }

  return findFirstNode(root, (node) => isElement(node) && String(node.tagName || '').toLowerCase() === lower);
}

function parseFragmentForNode(node, html) {
  const source = String(html || '');
  try {
    if (isElement(node)) {
      return parse5.parseFragment(node, source);
    }
    return parse5.parseFragment(source);
  } catch (err) {
    const tagName = isElement(node) ? String(node.tagName || '').toLowerCase() : String(node && node.nodeName || 'node');
    raiseBrowserError(
      'TRUEOS_BROWSER_HTML_FRAGMENT_PARSE_FAILED',
      'HTML fragment parse failed during DOM mutation',
      { targetTag: tagName, reason: describeError(err), html: source },
      err,
    );
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
  } catch (err) {
    raiseBrowserError(
      'TRUEOS_BROWSER_HTML_SERIALIZE_FAILED',
      'HTML serialization failed after DOM mutation',
      { reason: describeError(err) },
      err,
    );
  }
}

function commitDomMutation(doc) {
  const sourceDoc = doc && typeof doc === 'object'
    ? doc
    : (cachedDoc && cachedDoc.dom ? cachedDoc.dom : null);
  if (!sourceDoc) {
    raiseBrowserError('TRUEOS_BROWSER_DOM_COMMIT_FAILED', 'DOM mutation commit failed', {
      reason: 'missing source document',
    });
  }

  syncCachedHtmlFromDoc(sourceDoc);
  const { vw } = computeViewport();
  cachedDoc = buildDocFromParsed(sourceDoc, vw, 'mutated html document');
  paint();
  return true;
}

function setNodeHtml(target, html) {
  const { vw } = computeViewport();
  const doc = ensureDoc(vw);
  const node = resolveDomTargetNode(doc && doc.dom ? doc.dom : null, target);
  if (!node || typeof node !== 'object') {
    raiseBrowserError('TRUEOS_BROWSER_DOM_TARGET_NOT_FOUND', 'DOM mutation target was not found', {
      target: normalizeDomTarget(target) || '(empty)',
      op: 'setNodeHtml',
    });
  }
  if (isTextNode(node)) {
    raiseBrowserError('TRUEOS_BROWSER_DOM_TARGET_INVALID', 'DOM mutation target cannot be a text node', {
      target: normalizeDomTarget(target) || '(empty)',
      op: 'setNodeHtml',
    });
  }

  const nextChildren = adoptFragmentChildren(parseFragmentForNode(node, html), node);
  node.childNodes = nextChildren;
  return commitDomMutation(doc.dom);
}

function insertHtml(target, html, position = 'beforeend') {
  const { vw } = computeViewport();
  const doc = ensureDoc(vw);
  const node = resolveDomTargetNode(doc && doc.dom ? doc.dom : null, target);
  if (!node || typeof node !== 'object') {
    raiseBrowserError('TRUEOS_BROWSER_DOM_TARGET_NOT_FOUND', 'DOM insertion target was not found', {
      target: normalizeDomTarget(target) || '(empty)',
      op: 'insertHtml',
      position,
    });
  }

  const where = normalizeInsertPosition(position);
  let parent = null;
  let siblings = null;
  let insertAt = 0;

  if (where === 'beforebegin' || where === 'afterend') {
    parent = node.parentNode || null;
    if (!parent || typeof parent !== 'object') {
      raiseBrowserError('TRUEOS_BROWSER_DOM_TARGET_INVALID', 'DOM insertion target has no parent for sibling insertion', {
        target: normalizeDomTarget(target) || '(empty)',
        op: 'insertHtml',
        position: where,
      });
    }
    siblings = ensureChildNodes(parent);
    const nodeIndex = siblings.indexOf(node);
    if (nodeIndex < 0) {
      raiseBrowserError('TRUEOS_BROWSER_DOM_TARGET_INVALID', 'DOM insertion target was detached from its parent', {
        target: normalizeDomTarget(target) || '(empty)',
        op: 'insertHtml',
        position: where,
      });
    }
    insertAt = where === 'beforebegin' ? nodeIndex : nodeIndex + 1;
  } else {
    if (isTextNode(node)) {
      raiseBrowserError('TRUEOS_BROWSER_DOM_TARGET_INVALID', 'DOM insertion target cannot be a text node for child insertion', {
        target: normalizeDomTarget(target) || '(empty)',
        op: 'insertHtml',
        position: where,
      });
    }
    parent = node;
    siblings = ensureChildNodes(node);
    insertAt = where === 'afterbegin' ? 0 : siblings.length;
  }

  const inserted = adoptFragmentChildren(parseFragmentForNode(parent, html), parent);
  if (inserted.length <= 0) {
    raiseBrowserError('TRUEOS_BROWSER_DOM_INSERT_EMPTY', 'DOM insertion produced no nodes', {
      target: normalizeDomTarget(target) || '(empty)',
      op: 'insertHtml',
      position: where,
      html,
    });
  }
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

function serializeNode(node, depth = 0, path = 'root', themeLayout = null) {
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
  const layoutByPath = themeLayout && typeof themeLayout === 'object' ? themeLayout.byPath : null;
  const rect = layoutByPath && typeof layoutByPath === 'object' ? layoutByPath[path] : null;
  if (rect && typeof rect === 'object') {
    out.rect = {
      x: Number(rect.x || 0),
      y: Number(rect.y || 0),
      width: Number(rect.width || 0),
      height: Number(rect.height || 0),
    };
    out.caption = String(rect.caption || '');
  }
  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i += 1) {
    const child = serializeNode(kids[i], depth + 1, `${path}.${i}`, themeLayout);
    if (child) out.children.push(child);
  }
  return out;
}

function flattenSerializedSnapshot(root, out = []) {
  if (!root || typeof root !== 'object') return out;
  const entry = {
    type: typeof root.type === 'string' ? root.type : 'node',
    path: typeof root.path === 'string' ? root.path : 'root',
  };
  if (typeof root.tag === 'string') {
    entry.tag = root.tag;
    entry.tagName = root.tag;
  }
  if (typeof root.text === 'string') entry.text = root.text;
  if (root.attrs && typeof root.attrs === 'object' && !Array.isArray(root.attrs)) {
    entry.attrs = { ...root.attrs };
    if (typeof root.attrs.href === 'string' && root.attrs.href) entry.href = root.attrs.href;
  }
  if (root.rect && typeof root.rect === 'object') {
    entry.rect = {
      x: Number(root.rect.x || 0),
      y: Number(root.rect.y || 0),
      width: Number(root.rect.width || 0),
      height: Number(root.rect.height || 0),
    };
  }
  if (typeof root.caption === 'string' && root.caption) {
    entry.caption = root.caption;
  }
  out.push(entry);
  const kids = Array.isArray(root.children) ? root.children : [];
  for (let i = 0; i < kids.length; i += 1) {
    flattenSerializedSnapshot(kids[i], out);
  }
  return out;
}

function appendThemeLayoutSnapshotNodes(themeLayout, out = []) {
  const interactives = Array.isArray(themeLayout && themeLayout.interactives) ? themeLayout.interactives : [];
  if (interactives.length <= 0) return out;
  const byPath = new Map();
  for (let i = 0; i < out.length; i += 1) {
    const item = out[i];
    const path = item && typeof item.path === 'string' ? item.path : '';
    if (path) byPath.set(path, item);
  }
  for (let i = 0; i < interactives.length; i += 1) {
    const entry = interactives[i];
    const path = entry && typeof entry.path === 'string' ? entry.path : '';
    if (!path) continue;
    const interactiveNode = {
      type: 'interactive',
      path,
      tag: typeof entry.tag === 'string' ? entry.tag : '',
      tagName: typeof entry.tag === 'string' ? entry.tag : '',
      kind: typeof entry.kind === 'string' ? entry.kind : '',
      text: typeof entry.caption === 'string' ? entry.caption : '',
      caption: typeof entry.caption === 'string' ? entry.caption : '',
      href: typeof entry.href === 'string' ? entry.href : '',
      rect: {
        x: Math.round(Number(entry.x || 0)),
        y: Math.round(Number(entry.y || 0)),
        width: Math.max(1, Math.round(Number(entry.width || 0))),
        height: Math.max(1, Math.round(Number(entry.height || 0))),
      },
    };
    const existing = byPath.get(path);
    if (existing && typeof existing === 'object') {
      Object.assign(existing, interactiveNode);
      continue;
    }
    out.push(interactiveNode);
    byPath.set(path, interactiveNode);
  }
  return out;
}

function getDomSnapshot() {
  const { vw } = computeViewport();
  const doc = ensureDoc(vw);
  const root = serializeNode(doc && doc.dom ? doc.dom : null, 0, 'root', doc && doc.themeLayout ? doc.themeLayout : null);
  if (!root || typeof root !== 'object') return root;
  root.nodes = appendThemeLayoutSnapshotNodes(doc && doc.themeLayout ? doc.themeLayout : null, flattenSerializedSnapshot(root, []));
  return root;
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
  return true;
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

function getUrlOrigin(url) {
  const value = typeof url === 'string' ? url.trim() : '';
  const match = value.match(/^([a-z][a-z0-9+.-]*:\/\/[^/?#]+)/i);
  return match ? match[1] : '';
}

function getUrlDirectory(url) {
  const value = typeof url === 'string' ? url.trim() : '';
  const origin = getUrlOrigin(value);
  if (!origin) return '';
  const rest = value.slice(origin.length);
  const qIndex = rest.search(/[?#]/);
  const pathOnly = qIndex >= 0 ? rest.slice(0, qIndex) : rest;
  const slash = pathOnly.lastIndexOf('/');
  if (slash < 0) return `${origin}/`;
  return `${origin}${pathOnly.slice(0, slash + 1)}`;
}

function resolveNavigationUrl(url) {
  const href = typeof url === 'string' ? url.trim() : '';
  if (!href) return '';
  if (/^[a-z][a-z0-9+.-]*:/i.test(href)) return href;
  const base = typeof currentPageUrl === 'string' ? currentPageUrl.trim() : '';
  const origin = getUrlOrigin(base);
  if (href.startsWith('//')) {
    const schemeMatch = base.match(/^([a-z][a-z0-9+.-]*:)/i);
    return schemeMatch ? `${schemeMatch[1]}${href}` : `https:${href}`;
  }
  if (href.startsWith('/')) {
    return origin ? `${origin}${href}` : href;
  }
  const dir = getUrlDirectory(base);
  return dir ? `${dir}${href}` : href;
}

function setCurrentPageUrl(url) {
  const next = typeof url === 'string' ? url.trim() : '';
  currentPageUrl = next;
  runtime.host.__trueosBrowserCurrentUrl = currentPageUrl;
  runtime.host.__trueosBrowserUrl = currentPageUrl;
  return currentPageUrl;
}

async function loadUrlIntoBrowser(url, request = null) {
  const resolved = resolveNavigationUrl(url);
  if (!resolved) {
    return { ok: 0, handled: 0, reason: 'missing-url' };
  }
  setCurrentPageUrl(resolved);
  const hook = runtime.host.__trueosBrowserNavigate;
  if (typeof hook === 'function') {
    return dispatchBrowserAction('navigate', {
      url: currentPageUrl,
      request,
    }, '__trueosBrowserNavigate');
  }
  if (typeof fetch !== 'function') {
    return dispatchBrowserAction('navigate', {
      url: currentPageUrl,
      request,
    }, '__trueosBrowserNavigate');
  }
  try {
    const response = await fetch(currentPageUrl, { method: 'GET' });
    const html = await response.text();
    setHtml(html);
    return {
      ok: 1,
      handled: 1,
      loaded: 1,
      url: currentPageUrl,
      request,
    };
  } catch (err) {
    return {
      ok: 0,
      handled: 0,
      reason: 'navigate-fetch-failed',
      url: currentPageUrl,
      error: describeError(err),
    };
  }
}

function surfToUrl(url, event = null) {
  const href = typeof url === 'string' ? url.trim() : '';
  if (!href) {
    return { ok: 0, handled: 0, reason: 'missing-url' };
  }
  return loadUrlIntoBrowser(href, {
    url: resolveNavigationUrl(href),
    source: 'dom-link-click',
    event: event && typeof event === 'object' ? { ...event } : null,
  });
}

function paint() {
  const { vw, vh } = computeViewport();
  const doc = ensureDoc(vw);
  if (scrollY < 0) scrollY = 0;
  publishThemeLayoutInteractives(doc && doc.themeLayout ? doc.themeLayout : null);

  const overlayRuns = [];
  fpsOverlay.appendRuns(overlayRuns, vw);
  const releaseRect = pendingReleaseRect;
  pendingReleaseRect = null;

  try {
    renderScene(doc, vw, vh, scrollY, overlayRuns, releaseRect);
  } catch (err) {
    raiseBrowserError(
      'TRUEOS_BROWSER_RENDER_FAILED',
      'Browser paint failed while rendering scene',
      {
        reason: describeError(err),
        viewportWidth: vw,
        viewportHeight: vh,
        scrollY,
        rowCount: Array.isArray(doc && doc.rows) ? doc.rows.length : 0,
      },
      err,
    );
  }

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

function rememberSyntheticCursorEcho(slotId, x, y, buttonsDown, wheel, flags) {
  pendingSyntheticCursorEchoes.push(`${Number(slotId) | 0}:${Number(x || 0)}:${Number(y || 0)}:${Number(buttonsDown || 0) >>> 0}:${Number(wheel || 0) | 0}:${Number(flags || 0) >>> 0}`);
  if (pendingSyntheticCursorEchoes.length > 64) {
    pendingSyntheticCursorEchoes.splice(0, pendingSyntheticCursorEchoes.length - 64);
  }
}

function consumeSyntheticCursorEcho(slotId, x, y, buttonsDown, wheel, flags) {
  const signature = `${Number(slotId) | 0}:${Number(x || 0)}:${Number(y || 0)}:${Number(buttonsDown || 0) >>> 0}:${Number(wheel || 0) | 0}:${Number(flags || 0) >>> 0}`;
  const index = pendingSyntheticCursorEchoes.indexOf(signature);
  if (index < 0) return false;
  pendingSyntheticCursorEchoes.splice(index, 1);
  return true;
}

function logCursorDebug(message) {
  try {
    console.log(message);
  } catch (_) {}
}

function resolveAiCursorId(value) {
  if (typeof value === 'string') {
    const trimmed = value.trim();
    return trimmed || DEFAULT_AI_CURSOR_ID;
  }
  if (typeof value === 'number' && Number.isFinite(value)) {
    return String(value);
  }
  return DEFAULT_AI_CURSOR_ID;
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
        pendingReleaseRect = buildReleaseRect(origin, nextX, nextY);
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
  const prev = kernelCursorState.get(Number(slotId || 0) | 0) || null;
  let updated = rememberKernelCursor(slotId, x, y, buttonsDown, flags);

  if (!prev || Number(prev.x || 0) !== Number(x || 0) || Number(prev.y || 0) !== Number(y || 0)) {
    cursorMoveLogCount += 1;
    if ((cursorMoveLogCount % 25) === 0) {
      logCursorDebug(`[browser.mjs] cursor move chunk=25 source=${source} slot=${slotId} x=${Number(x || 0)} y=${Number(y || 0)}`);
    }
    updated += 1;
  }

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
  const aiCursorId = resolveAiCursorId(source.aiCursorId);
  const resolvedSlotId = slotId > 0 ? slotId : getOrCreateAiCursorSlot(aiCursorId);
  if (resolvedSlotId <= 0) return false;

  const x = Number(source.x || 0);
  const y = Number(source.y || 0);
  const buttonsDown = Number(source.buttonsDown || 0) >>> 0;
  const wheel = Number(source.wheel || 0) | 0;
  const flags = Number(source.flags || 0) >>> 0;

  const writeCursorEvent = runtime.host.__trueosWriteCursorEvent;
  if (typeof writeCursorEvent === 'function') {
    try {
      if (writeCursorEvent(resolvedSlotId, x, y, buttonsDown, wheel, flags)) {
        rememberSyntheticCursorEcho(resolvedSlotId, x, y, buttonsDown, wheel, flags);
        const updated = processCursorEvent(resolvedSlotId, x, y, buttonsDown, wheel, flags, 'synthetic');
        if (updated > 0 && wheel === 0) {
          paint();
        }
        return true;
      }
    } catch (_) {}
  }

  processCursorEvent(resolvedSlotId, x, y, buttonsDown, wheel, flags, 'synthetic');
  return true;
}

function moveCursor(target = null) {
  const source = target && typeof target === 'object' ? target : {};
  const slotId = Number(source.slotId || 0) | 0;
  const aiCursorId = resolveAiCursorId(source.aiCursorId);
  const resolvedSlotId = slotId > 0 ? slotId : getOrCreateAiCursorSlot(aiCursorId);
  if (resolvedSlotId <= 0) return false;

  const state = kernelCursorState.get(resolvedSlotId);
  const x = Number(source.x);
  const y = Number(source.y);
  if (!Number.isFinite(x) || !Number.isFinite(y)) return false;

  const buttonsDown = Object.prototype.hasOwnProperty.call(source, 'buttonsDown')
    ? (Number(source.buttonsDown || 0) >>> 0)
    : (state ? (Number(state.buttonsDown || 0) >>> 0) : 0);
  const flags = Object.prototype.hasOwnProperty.call(source, 'flags')
    ? (Number(source.flags || 0) >>> 0)
    : 1;

  return injectCursorEvent({
    slotId: resolvedSlotId,
    x,
    y,
    buttonsDown,
    wheel: 0,
    flags,
  });
}

function getThemeInteractives(vw) {
  const doc = ensureDoc(vw);
  const interactives = publishThemeLayoutInteractives(doc && doc.themeLayout ? doc.themeLayout : null).interactives;
  return Array.isArray(interactives) ? interactives : [];
}

function normalizeInteractiveText(value) {
  return collapseWhitespace(String(value || ''));
}

function normalizeInteractiveFold(value) {
  return normalizeInteractiveText(value).toLowerCase();
}

function parseSimpleInteractiveSelector(value) {
  const raw = typeof value === 'string' ? value.trim() : '';
  if (!raw) return null;
  const match = raw.match(/^([a-z0-9_-]+)?\s*\[\s*([a-z0-9:_-]+)\s*=\s*(["']?)(.*?)\3\s*\]$/i);
  if (!match) return null;
  return {
    tag: match[1] ? String(match[1]).toLowerCase() : '',
    attr: String(match[2] || '').toLowerCase(),
    value: String(match[4] || ''),
  };
}

function findInteractiveByPredicate(interactives, predicate) {
  if (!Array.isArray(interactives) || typeof predicate !== 'function') return null;
  for (let i = 0; i < interactives.length; i += 1) {
    const entry = interactives[i];
    if (entry && typeof entry === 'object' && predicate(entry)) {
      return entry;
    }
  }
  return null;
}

function resolveInteractiveTarget(target = null) {
  const source = target && typeof target === 'object' && !Array.isArray(target) ? target : null;
  const raw = typeof target === 'string'
    ? target.trim()
    : (source && typeof source.path === 'string' ? source.path.trim() : '');
  const directX = source ? Number(source.x) : NaN;
  const directY = source ? Number(source.y) : NaN;
  if (Number.isFinite(directX) && Number.isFinite(directY)) {
    return {
      x: directX,
      y: directY,
      path: source && typeof source.path === 'string' ? source.path.trim() : '',
      interactive: null,
    };
  }

  const { vw } = computeViewport();
  const interactives = getThemeInteractives(vw);
  const wantPath = raw && raw.startsWith('root') ? raw : (source && typeof source.path === 'string' ? source.path.trim() : '');
  if (wantPath) {
    const byPath = findInteractiveByPredicate(interactives, (entry) => String(entry.path || '') === wantPath);
    if (byPath) {
      return {
        x: Number(byPath.x || 0) + Math.max(1, Number(byPath.width || 1)) * 0.5,
        y: Number(byPath.y || 0) + Math.max(1, Number(byPath.height || 1)) * 0.5,
        path: String(byPath.path || ''),
        interactive: byPath,
      };
    }
  }

  const hrefNeedle = source && typeof source.href === 'string' ? source.href.trim() : '';
  if (hrefNeedle) {
    const byHref = findInteractiveByPredicate(interactives, (entry) => String(entry.href || '') === hrefNeedle);
    if (byHref) {
      return {
        x: Number(byHref.x || 0) + Math.max(1, Number(byHref.width || 1)) * 0.5,
        y: Number(byHref.y || 0) + Math.max(1, Number(byHref.height || 1)) * 0.5,
        path: String(byHref.path || ''),
        interactive: byHref,
      };
    }
  }

  const textNeedle = source
    ? normalizeInteractiveFold(source.text || source.caption || source.value || '')
    : '';
  if (textNeedle) {
    const byText = findInteractiveByPredicate(interactives, (entry) => normalizeInteractiveFold(entry.caption || '') === textNeedle);
    if (byText) {
      return {
        x: Number(byText.x || 0) + Math.max(1, Number(byText.width || 1)) * 0.5,
        y: Number(byText.y || 0) + Math.max(1, Number(byText.height || 1)) * 0.5,
        path: String(byText.path || ''),
        interactive: byText,
      };
    }
  }

  if (!raw) return null;

  const lower = raw.toLowerCase();
  if (lower.startsWith('text=')) {
    const needle = normalizeInteractiveFold(raw.slice(5));
    const byText = findInteractiveByPredicate(interactives, (entry) => normalizeInteractiveFold(entry.caption || '') === needle);
    if (byText) {
      return {
        x: Number(byText.x || 0) + Math.max(1, Number(byText.width || 1)) * 0.5,
        y: Number(byText.y || 0) + Math.max(1, Number(byText.height || 1)) * 0.5,
        path: String(byText.path || ''),
        interactive: byText,
      };
    }
  }

  const selector = parseSimpleInteractiveSelector(raw);
  if (selector) {
    const bySelector = findInteractiveByPredicate(interactives, (entry) => {
      const tag = String(entry.tag || '').toLowerCase();
      if (selector.tag && tag !== selector.tag) return false;
      if (selector.attr === 'href') return String(entry.href || '') === selector.value;
      if (selector.attr === 'path') return String(entry.path || '') === selector.value;
      if (selector.attr === 'caption' || selector.attr === 'text') {
        return normalizeInteractiveText(entry.caption || '') === normalizeInteractiveText(selector.value);
      }
      return false;
    });
    if (bySelector) {
      return {
        x: Number(bySelector.x || 0) + Math.max(1, Number(bySelector.width || 1)) * 0.5,
        y: Number(bySelector.y || 0) + Math.max(1, Number(bySelector.height || 1)) * 0.5,
        path: String(bySelector.path || ''),
        interactive: bySelector,
      };
    }
  }

  const byCaption = findInteractiveByPredicate(interactives, (entry) => normalizeInteractiveFold(entry.caption || '') === normalizeInteractiveFold(raw));
  if (byCaption) {
    return {
      x: Number(byCaption.x || 0) + Math.max(1, Number(byCaption.width || 1)) * 0.5,
      y: Number(byCaption.y || 0) + Math.max(1, Number(byCaption.height || 1)) * 0.5,
      path: String(byCaption.path || ''),
      interactive: byCaption,
    };
  }

  const byHref = findInteractiveByPredicate(interactives, (entry) => String(entry.href || '') === raw);
  if (byHref) {
    return {
      x: Number(byHref.x || 0) + Math.max(1, Number(byHref.width || 1)) * 0.5,
      y: Number(byHref.y || 0) + Math.max(1, Number(byHref.height || 1)) * 0.5,
      path: String(byHref.path || ''),
      interactive: byHref,
    };
  }

  return null;
}

function synthesizeClick(target = null) {
  const source = target && typeof target === 'object' && !Array.isArray(target) ? target : {};
  const slotId = Number(source.slotId || 0) | 0;
  const aiCursorId = resolveAiCursorId(source.aiCursorId);
  const resolvedSlotId = slotId > 0 ? slotId : getOrCreateAiCursorSlot(aiCursorId);
  if (resolvedSlotId <= 0) {
    return { ok: 0, handled: 0, reason: 'invalid-slot' };
  }

  const resolved = resolveInteractiveTarget(target);
  const state = kernelCursorState.get(resolvedSlotId) || null;
  const x = resolved && Number.isFinite(Number(resolved.x))
    ? Number(resolved.x)
    : Number(state && state.x);
  const y = resolved && Number.isFinite(Number(resolved.y))
    ? Number(resolved.y)
    : Number(state && state.y);
  if (!Number.isFinite(x) || !Number.isFinite(y)) {
    return { ok: 0, handled: 0, reason: 'target-not-found' };
  }

  const buttonMask = Math.max(1, Number(source.buttonMask || source.button || 1) | 0) >>> 0;
  moveCursor({
    slotId: resolvedSlotId,
    aiCursorId,
    x,
    y,
  });
  const downOk = injectCursorEvent({
    slotId: resolvedSlotId,
    x,
    y,
    buttonsDown: buttonMask,
    wheel: 0,
    flags: 1 | CURSOR_FLAG_BUTTONS_CHANGED,
  });
  const upOk = injectCursorEvent({
    slotId: resolvedSlotId,
    x,
    y,
    buttonsDown: 0,
    wheel: 0,
    flags: 1 | CURSOR_FLAG_BUTTONS_CHANGED,
  });
  if (!downOk || !upOk) {
    return { ok: 0, handled: 0, reason: 'cursor-injection-failed' };
  }

  const interactive = resolved && resolved.interactive && typeof resolved.interactive === 'object'
    ? resolved.interactive
    : null;
  return {
    ok: 1,
    handled: 1,
    simulated: 0,
    slotId: resolvedSlotId,
    x,
    y,
    path: resolved && typeof resolved.path === 'string' ? resolved.path : '',
    href: interactive ? String(interactive.href || '') : '',
    caption: interactive ? String(interactive.caption || '') : '',
    kind: interactive ? String(interactive.kind || '') : '',
  };
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
      p += CURSOR_EVENT_FIELDS;

      if (consumeSyntheticCursorEcho(slotId, x, y, buttonsDown, wheel, flags)) {
        continue;
      }

      const prevState = kernelCursorState.get(slotId) || null;
      const prevButtonsDown = prevState ? (Number(prevState.buttonsDown || 0) >>> 0) : 0;

      updated += processCursorEvent(slotId, x, y, buttonsDown, wheel, flags, 'kernel');
      if (prevButtonsDown !== 0 && buttonsDown === 0 && pendingReleaseRect) {
        paint();
      }
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

function placeDefaultAiCursor() {
  const { vw, vh } = computeViewport();
  const x = Math.max(0, Math.floor(vw * 0.25));
  const y = Math.max(0, Math.floor(vh * 0.25));
  moveCursor({
    aiCursorId: DEFAULT_AI_CURSOR_ID,
    x,
    y,
  });
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
      placeDefaultAiCursor();
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
  dispatchDomClick(path, payload = null) {
    return dispatchParryDomClick(path, payload, {
      computeViewport,
      ensureDoc,
      isElement,
      raiseBrowserError,
      describeError,
      surfToUrl,
    });
  },
  dispatchButtonClick(path, payload = null) {
    return dispatchParryDomClick(path, payload, {
      computeViewport,
      ensureDoc,
      isElement,
      raiseBrowserError,
      describeError,
      surfToUrl,
    });
  },
  getTrueosFsTreeHtml(maxEntries = 64) {
    return getTrueosFsTreeHtml(maxEntries);
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
  setCurrentPageUrl(url = '') {
    return setCurrentPageUrl(url);
  },
  setScroll(y) {
    scrollY = Math.max(0, Math.round(Number(y || 0)));
    paint();
  },
  moveCursor(target = null) {
    return moveCursor(target);
  },
  click(target = null) {
    const result = synthesizeClick(target);
    if (result && result.ok) return result;
    const hook = runtime.host.__trueosBrowserClick;
    if (typeof hook === 'function') {
      const payload = target && typeof target === 'object' && !Array.isArray(target)
        ? { target: { ...target } }
        : { target: target == null ? null : { value: target } };
      return dispatchBrowserAction('click', payload, '__trueosBrowserClick');
    }
    return result;
  },
  navigate(input = null) {
    const request = input && typeof input === 'object' && !Array.isArray(input)
      ? { ...input }
      : { url: String(input || '') };
    const url = typeof request.url === 'string' ? request.url.trim() : '';
    return loadUrlIntoBrowser(url, request);
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