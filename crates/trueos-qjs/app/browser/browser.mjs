import * as parse5 from 'parse5';
import * as cmdStream from 'trueos:cmd_stream';
import Yoga from 'yoga-layout';
import { Worker } from 'node:worker_threads';
import { createFpsOverlay } from './fps.mjs';
import { createBrowserAssetManager } from './browser_assets.mjs';
import { createBrowserCursorController } from './browser_cursor.mjs';
import { createBrowserPageState } from './browser_page_state.mjs';
import { extractCssSection, resolveNodeStyle } from './css.mjs';
import { registerSvgDemoRoute } from './svg_demo.mjs';
import {
  renderScene,
  renderSceneRegionToCurrentTarget,
  composeSceneRegionsToCurrentTarget,
} from './scene.mjs';
import { BLOCK_TAGS, TEXT_LEVEL_SEMANTICS_TAGS } from './htmlDefaults.mjs';
import { LEFT_PAD, TOP_PAD, LINE_H, FONT_PX } from './theme.mjs';

const runtime = resolveRuntime();
registerSvgDemoRoute(runtime.host, { iconSize: 64 });

const INDENT_PX = 12;
const AUTO_PAINT_MS = Math.max(0, Number(runtime.host.__trueosBrowserAutoPaintMs || 0) || 0);
const HOSTED_BY_UI2 = !!runtime.host.__trueosBrowserHostedByUi2;
const ERROR_PREVIEW_MAX = 160;
const MAX_RENDER_TEXT_CHARS = 512;
const HTML_READY_TIMEOUT_MS = 10000;
const OMIT_TAGS = new Set(['html', 'body', 'script', 'style', 'meta', 'link', 'li']);
const SHOW_CLOSING_TAG_ROWS = false;
const BROWSER_REGION_CACHE_MAX = 4;
const BROWSER_REGION_PREFETCH_SCREENS = 1;
const BROWSER_REGION_TILE_MIN_PX = 512;
const BROWSER_REGION_TILE_MAX_PX = 2048;
const BROWSER_REGION_TILE_ALIGN_PX = 256;
const BROWSER_KEYBOARD_LOG_MAX = 128;

let cachedHtml = '';
let cachedDoc = null;
let scrollY = 0;
let currentPageUrl = String(
  runtime.host.__trueosBrowserCurrentUrl
  || runtime.host.__trueosBrowserUrl
  || '',
);
let browserActionSeq = 0;
let aiStartPromise = null;
let aiStartSpecifier = '';
let aiWorker = null;
const aiInputQueue = [];
const aiInputWaiters = [];
const qjsInputQueue = [];
let qjsInputDrainPromise = Promise.resolve();
let fpsOverlayEnabled = false;
let browserCanRenderScene = false;
let browserContentReadySignaled = false;
let htmlReadyTimeoutId = null;
const browserRegionCache = [];
let browserRegionCacheSeq = 0;
let browserRegionCacheRevision = 1;
let browserRegionCacheWidth = 0;
let browserRegionTileHeight = 0;
let browserRowPreviewKey = '';

const fpsOverlay = createFpsOverlay();
runtime.host.__trueosBrowserShowClosingTagRows = SHOW_CLOSING_TAG_ROWS;
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
    'setBodyHtml',
    'insertHtml',
    'getViewport',
    'paint',
    'setScroll',
    'getWindowId',
    'getWindowInfo',
    'setWindowTitle',
    'setWindowPosition',
    'setWindowSize',
    'setWindowDecorations',
    'minimizeWindow',
    'maximizeWindow',
    'restoreWindow',
    'focusWindow',
    'closeWindow',
    'beginWindowMove',
    'beginWindowResize',
    'moveCursor',
    'click',
    'navigate',
    'keyboard',
    'typeText',
    'pressKey',
    'captureScreenshot',
  ],
  unavailable: [
  ],
  notes: {
    intent: 'Worker-facing browser contract for the AI task. Keep this surface explicit so agent logic remains isolated from the browser VM.',
    targetShape: 'Close to future computer-use style APIs while still reflecting TRUEOS capabilities today.',
    keyboardShape: 'keyboard(...) accepts Unicode text entries and strict key entries with optional modifiers; pressKey(...) and typeText(...) compile into that canonical event list.',
  },
};

const KEYBOARD_MODIFIER_ALIASES = Object.freeze({
  alt: 'Alt',
  cmd: 'Meta',
  command: 'Meta',
  control: 'Ctrl',
  ctrl: 'Ctrl',
  meta: 'Meta',
  option: 'Alt',
  shift: 'Shift',
  super: 'Meta',
});

const KEYBOARD_KEY_ALIASES = Object.freeze({
  backspace: 'Backspace',
  del: 'Delete',
  delete: 'Delete',
  down: 'ArrowDown',
  end: 'End',
  enter: 'Enter',
  esc: 'Escape',
  escape: 'Escape',
  home: 'Home',
  ins: 'Insert',
  insert: 'Insert',
  left: 'ArrowLeft',
  pagedown: 'PageDown',
  pageup: 'PageUp',
  pgdn: 'PageDown',
  pgdown: 'PageDown',
  pgup: 'PageUp',
  return: 'Enter',
  right: 'ArrowRight',
  space: 'Space',
  spacebar: 'Space',
  tab: 'Tab',
  up: 'ArrowUp',
});

function normalizeKeyboardModifier(value) {
  const raw = String(value || '').trim();
  if (!raw) return '';
  const lowered = raw.toLowerCase();
  return KEYBOARD_MODIFIER_ALIASES[lowered] || '';
}

function normalizeKeyboardModifiers(value) {
  const items = Array.isArray(value)
    ? value
    : (value == null ? [] : [value]);
  const out = [];
  const seen = new Set();
  for (let i = 0; i < items.length; i += 1) {
    const normalized = normalizeKeyboardModifier(items[i]);
    if (!normalized || seen.has(normalized)) continue;
    seen.add(normalized);
    out.push(normalized);
  }
  return out;
}

function normalizeKeyboardKey(value) {
  const raw = String(value || '').trim();
  if (!raw) return '';
  if (raw.length === 1) return raw;
  const lowered = raw.toLowerCase().replace(/[\s_-]+/g, '');
  if (KEYBOARD_KEY_ALIASES[lowered]) {
    return KEYBOARD_KEY_ALIASES[lowered];
  }
  if (/^f\d{1,2}$/i.test(raw)) {
    return `F${raw.slice(1)}`;
  }
  return raw;
}

function clampKeyboardRepeat(value) {
  const count = Number(value);
  if (!Number.isFinite(count)) return 1;
  return Math.max(1, Math.min(64, Math.floor(count)));
}

function normalizeKeyboardEntry(entry) {
  if (typeof entry === 'string') {
    if (!entry) {
      raiseBrowserError('TRUEOS_BROWSER_KEYBOARD_INVALID', 'keyboard text entry is empty');
    }
    return { type: 'text', text: entry };
  }
  if (!entry || typeof entry !== 'object' || Array.isArray(entry)) {
    raiseBrowserError('TRUEOS_BROWSER_KEYBOARD_INVALID', 'keyboard entry must be a string or object');
  }

  const type = typeof entry.type === 'string' ? entry.type.trim().toLowerCase() : '';
  if (type === 'text' || (!type && typeof entry.text === 'string' && entry.key == null)) {
    const text = typeof entry.text === 'string' ? entry.text : String(entry.text || '');
    if (!text) {
      raiseBrowserError('TRUEOS_BROWSER_KEYBOARD_INVALID', 'keyboard text entry is empty');
    }
    return { type: 'text', text };
  }

  if (type === 'key' || entry.key != null) {
    const key = normalizeKeyboardKey(entry.key);
    if (!key) {
      raiseBrowserError('TRUEOS_BROWSER_KEYBOARD_INVALID', 'keyboard key entry is missing a key');
    }
    const modifiers = normalizeKeyboardModifiers(entry.modifiers || entry.mods);
    const repeat = clampKeyboardRepeat(entry.repeat);
    return { type: 'key', key, modifiers, repeat };
  }

  raiseBrowserError(
    'TRUEOS_BROWSER_KEYBOARD_INVALID',
    'keyboard entry must declare type=text or type=key',
  );
}

function parseKeyboardInput(input = null, options = null) {
  const source = input && typeof input === 'object' && !Array.isArray(input) ? input : null;
  const opts = options && typeof options === 'object' ? options : {};
  const entriesRaw = Array.isArray(input)
    ? input
    : (source && Array.isArray(source.events)
      ? source.events
      : [input]);
  const events = [];
  for (let i = 0; i < entriesRaw.length; i += 1) {
    const entry = normalizeKeyboardEntry(entriesRaw[i]);
    if (entry) events.push(entry);
  }
  if (events.length === 0) {
    raiseBrowserError('TRUEOS_BROWSER_KEYBOARD_INVALID', 'keyboard input did not contain any events');
  }

  const logOnly = source && Object.prototype.hasOwnProperty.call(source, 'logOnly')
    ? source.logOnly !== false
    : (opts.logOnly !== false);
  return { events, logOnly };
}

function recordKeyboardLog(payload) {
  let queue = runtime.host.__trueosBrowserKeyboardLog;
  if (!Array.isArray(queue)) {
    queue = [];
    runtime.host.__trueosBrowserKeyboardLog = queue;
  }

  const nextSeq = Math.max(1, Number(runtime.host.__trueosBrowserKeyboardSeq || 0) + 1);
  runtime.host.__trueosBrowserKeyboardSeq = nextSeq;
  const entry = {
    seq: nextSeq,
    timestampMs: nowMs(),
    events: payload.events.map((event) => (
      event.type === 'text'
        ? { type: 'text', text: event.text }
        : {
          type: 'key',
          key: event.key,
          modifiers: [...event.modifiers],
          repeat: event.repeat,
        }
    )),
    logOnly: payload.logOnly !== false,
  };
  queue.push(entry);
  if (queue.length > BROWSER_KEYBOARD_LOG_MAX) {
    queue.splice(0, queue.length - BROWSER_KEYBOARD_LOG_MAX);
  }
  return entry;
}

const assetManager = createBrowserAssetManager({
  cmdStream,
  host: runtime.host,
  paint: requestBrowserContentRepaint,
  resolveNavigationUrl,
  raiseBrowserError,
  describeError,
  onAssetStateChanged: refreshBrowserPageState,
});
const browserPageState = createBrowserPageState({
  host: runtime.host,
  nowMs,
  getCurrentUrl: () => currentPageUrl,
  getHtmlBytes: () => cachedHtml.length,
  summarizeAssets: (urls) => assetManager.summarizeImageUrls(urls),
});

const BROWSER_INTERACTION_EXTERNAL_RESULT = Object.freeze({
  ok: 0,
  handled: 0,
  reason: 'ui2-owned-input',
});

function refreshBrowserPageState(reason = 'state-refresh') {
  return browserPageState.refresh(reason);
}

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

function decodeWindowState(stateId) {
  if ((Number(stateId) | 0) === 1) return 'minimized';
  if ((Number(stateId) | 0) === 2) return 'maximized';
  return 'normal';
}

function decodeWindowDecorations(modeId) {
  if ((Number(modeId) | 0) === 1) return 'client';
  if ((Number(modeId) | 0) === 2) return 'none';
  return 'system';
}

function encodeWindowDecorations(mode) {
  const value = String(mode || '').trim().toLowerCase();
  if (value === 'client') return 1;
  if (value === 'none') return 2;
  return 0;
}

function encodeWindowResizeEdges(edges) {
  if (Number.isFinite(edges)) return Math.max(0, Number(edges) | 0) >>> 0;
  if (Array.isArray(edges)) {
    let mask = 0;
    for (const entry of edges) mask |= encodeWindowResizeEdges(entry);
    return mask >>> 0;
  }
  if (edges && typeof edges === 'object') {
    let mask = 0;
    if (edges.left) mask |= 1;
    if (edges.top) mask |= 2;
    if (edges.right) mask |= 4;
    if (edges.bottom) mask |= 8;
    return mask >>> 0;
  }
  const value = String(edges || '').trim().toLowerCase();
  if (!value) return 0;
  let mask = 0;
  for (const token of value.split(/[\s,_-]+/)) {
    if (token === 'left' || token === 'l' || token === 'west' || token === 'w') mask |= 1;
    if (token === 'top' || token === 't' || token === 'north' || token === 'n') mask |= 2;
    if (token === 'right' || token === 'r' || token === 'east' || token === 'e') mask |= 4;
    if (token === 'bottom' || token === 'b' || token === 'south' || token === 's') mask |= 8;
  }
  return mask >>> 0;
}

function currentWindowId() {
  if (typeof runtime.host.__trueosPrimaryWindowId !== 'function') return 0;
  const value = Number(runtime.host.__trueosPrimaryWindowId() || 0) | 0;
  return value > 0 ? value : 0;
}

function resolveWindowId(windowId = null) {
  if (windowId == null) return currentWindowId();
  const value = Number(windowId || 0) | 0;
  return value > 0 ? value : 0;
}

function getWindowInfo(windowId = null) {
  const id = resolveWindowId(windowId);
  if (id <= 0 || typeof runtime.host.__trueosWindowGetInfo !== 'function') {
    return null;
  }
  const info = runtime.host.__trueosWindowGetInfo(id);
  if (!info || typeof info !== 'object') return null;
  return {
    ...info,
    id,
    stateName: decodeWindowState(info.state),
    decorationName: decodeWindowDecorations(info.decorationMode),
  };
}

function runWindowAction(windowId, fnName, ...args) {
  const id = resolveWindowId(windowId);
  const fn = runtime.host[fnName];
  if (id <= 0 || typeof fn !== 'function') return false;
  return !!fn(id, ...args);
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

function normalizeQjsInput(entry) {
  const source = entry && typeof entry === 'object' && !Array.isArray(entry) ? entry : null;
  const code = source && typeof source.code === 'string'
    ? source.code.trim()
    : (typeof entry === 'string' ? entry.trim() : '');
  if (!code) return null;
  return {
    code,
    repl: !(source && source.repl === false),
  };
}

function qjsShellWrite(text) {
  const line = typeof text === 'string' ? text : String(text == null ? '' : text);
  if (!line) return;
  try { console.log(`[browser.qjs] ${line}`); } catch (_) {}
  if (typeof runtime.host.__trueosShell2PrintLine === 'function') {
    try {
      runtime.host.__trueosShell2PrintLine(line);
      return true;
    } catch (_) {}
  }
  return false;
}

function summarizeQjsValue(value) {
  if (value === undefined) return 'undefined';
  if (value === null) return 'null';
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean' || typeof value === 'bigint') {
    return String(value);
  }
  try {
    return JSON.stringify(value);
  } catch (_) {
    return String(value);
  }
}

async function runReplQjs(code) {
  let result = (0, eval)(code);
  if (result && typeof result.then === 'function') {
    result = await result;
  }
  return result;
}

async function runOneShotQjs(code) {
  const factory = (0, eval)(`(async () => {\n${code}\n})`);
  return await factory();
}

async function drainQjsInputQueue() {
  while (qjsInputQueue.length > 0) {
    const entry = qjsInputQueue.shift();
    if (!entry) continue;
    try {
      const result = entry.repl
        ? await runReplQjs(entry.code)
        : await runOneShotQjs(entry.code);
      if (result !== undefined) {
        qjsShellWrite(`qjs ${entry.repl ? 'repl' : 'eval'} => ${summarizeQjsValue(result)}`);
      } else {
        qjsShellWrite(`qjs ${entry.repl ? 'repl' : 'eval'} ok`);
      }
    } catch (err) {
      qjsShellWrite(`qjs ${entry.repl ? 'repl' : 'eval'} error: ${String(err && err.stack ? err.stack : err)}`);
    }
  }
}

function pushQjsInput(entry) {
  const value = normalizeQjsInput(entry);
  if (!value) return false;
  qjsInputQueue.push(value);
  qjsInputDrainPromise = qjsInputDrainPromise
    .then(() => drainQjsInputQueue())
    .catch((err) => {
      qjsShellWrite(`qjs drain error: ${String(err && err.stack ? err.stack : err)}`);
    });
  return true;
}

function installQjsInputBridge() {
  runtime.host.__trueosQjsInputPush = pushQjsInput;
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

function normalizeViewportSize(value, fallback) {
  const next = Math.round(Number(value || 0));
  if (!Number.isFinite(next) || next <= 0) {
    return Math.max(1, Math.round(Number(fallback || 1) || 1));
  }
  return next;
}

function readViewportOverride() {
  const override = runtime.host.__trueosBrowserViewport;
  if (!override || typeof override !== 'object') return null;
  const width = normalizeViewportSize(
    override.width,
    (runtime.host.window || runtime.host).innerWidth || 1280,
  );
  const height = normalizeViewportSize(
    override.height,
    (runtime.host.window || runtime.host).innerHeight || 800,
  );
  return { width, height };
}

function readContentRectOverride(vw, vh) {
  const rect = runtime.host.__trueosBrowserContentRect;
  if (!rect || typeof rect !== 'object') {
    return { x: 0, y: 0, width: vw, height: vh };
  }
  return {
    x: Math.round(Number(rect.x || 0) || 0),
    y: Math.round(Number(rect.y || 0) || 0),
    width: normalizeViewportSize(rect.width, vw),
    height: normalizeViewportSize(rect.height, vh),
  };
}

function computeViewport() {
  const W = runtime.host.window || runtime.host;
  const override = readViewportOverride();
  const vw = override ? override.width : Math.max(1, Number(W.innerWidth || 1280));
  const vh = override ? override.height : Math.max(1, Number(W.innerHeight || 800));
  return { vw, vh };
}

function collapseWhitespace(s) {
  return String(s || '').replace(/\s+/g, ' ').trim();
}

function capRenderableText(value) {
  const text = String(value || '');
  if (text.length <= MAX_RENDER_TEXT_CHARS) return text;
  return text.slice(0, MAX_RENDER_TEXT_CHARS);
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
    const nextRect = {
      x: Math.round(Number(xs[i] ?? LEFT_PAD)),
      y: Math.round(Number(ys[i] ?? (i * LINE_H))),
      width: Math.max(1, estimateTextWidthPx(String(row.text || ''), FONT_PX)),
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
  maybeLogRowPreview(rows, layout.rowX, layout.rowY, context);
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

function shouldLogRowPreview() {
  const url = String(currentPageUrl || '');
  return url.includes('w3.org/Graphics/PNG/Inline-img.html');
}

function maybeLogRowPreview(rows, rowX, rowY, context = 'document') {
  if (!shouldLogRowPreview()) return;
  const key = `${String(currentPageUrl || '')}::${String(context || '')}::${Math.max(0, Number(rows && rows.length || 0) | 0)}::${Math.max(0, Number(rowY && rowY[0] || 0) | 0)}`;
  if (browserRowPreviewKey === key) return;
  browserRowPreviewKey = key;

  const list = Array.isArray(rows) ? rows : [];
  const xs = Array.isArray(rowX) ? rowX : [];
  const ys = Array.isArray(rowY) ? rowY : [];
  const preview = [];
  for (let i = 0; i < list.length && i < 12; i += 1) {
    const row = list[i] || null;
    preview.push(
      `${i}:${String(row && row.kind || '')}@(${Math.round(Number(xs[i] || 0))},${Math.round(Number(ys[i] || 0))}) ` +
      `${JSON.stringify(String(row && row.text || ''))}`,
    );
  }
  try {
    console.log(`[browser.mjs] row-preview ${String(context || '')} rows=${list.length} ${preview.join(' | ')}`);
  } catch (_) {}
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
  const value = capRenderableText(text);
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
    root.setPadding(Yoga.EDGE_TOP, 0);

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
    root.setPadding(Yoga.EDGE_TOP, 0);

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
  assetManager.applyResourcesToRows(cachedDoc && cachedDoc.rows ? cachedDoc.rows : []);
  assetManager.requestAssetsForRows(cachedDoc && cachedDoc.rows ? cachedDoc.rows : []);
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

function setBodyHtml(html) {
  return setNodeHtml('body', html);
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
  const contentRect = readContentRectOverride(vw, vh);
  return {
    width: vw,
    height: vh,
    x: contentRect.x,
    y: contentRect.y,
    contentWidth: contentRect.width,
    contentHeight: contentRect.height,
    scrollY: scrollY,
  };
}

function setHtml(nextHtml) {
  cachedHtml = String(nextHtml || '');
  cachedDoc = null;
  invalidateBrowserRegionCache(true);
  browserPageState.beginLoad('html-set');
  if (cachedHtml.trim()) {
    browserCanRenderScene = true;
    if (htmlReadyTimeoutId != null && typeof runtime.host.clearTimeout === 'function') {
      try { runtime.host.clearTimeout(htmlReadyTimeoutId); } catch (_) {}
      htmlReadyTimeoutId = null;
    }
  }
  if (!browserCanRenderScene) {
    return true;
  }
  paint();
  const htmlSnapshot = cachedHtml;
  const pageLoadSeqSnapshot = browserPageState.getState('html-snapshot').seq;
  if (typeof Promise === 'function' && typeof Promise.resolve === 'function') {
    Promise.resolve().then(() => {
      if (cachedHtml !== htmlSnapshot || browserPageState.getState('html-snapshot-check').seq !== pageLoadSeqSnapshot) return;
      const urls = assetManager.primeHtmlImageUrls(htmlSnapshot);
      browserPageState.updateAssetUrls(urls, 'html-assets-primed');
    }).catch(() => {});
  } else {
    const urls = assetManager.primeHtmlImageUrls(htmlSnapshot);
    browserPageState.updateAssetUrls(urls, 'html-assets-primed');
  }
  return true;
}

function nowMs() {
  if (typeof Date !== 'undefined' && typeof Date.now === 'function') {
    return Number(Date.now()) || 0;
  }
  return 0;
}

function docHasTextRows(doc) {
  const rows = Array.isArray(doc && doc.rows) ? doc.rows : [];
  for (let i = 0; i < rows.length; i += 1) {
    const row = rows[i];
    const kind = String(row && row.kind || '');
    if (kind === 'image' || kind === 'hr') continue;
    if (String(row && row.text || '').trim()) {
      return true;
    }
  }
  return false;
}

function finalizePaintState(doc) {
  const hasRealSceneContent = Array.isArray(doc && doc.rows) && doc.rows.length > 0;
  const hasTextContent = docHasTextRows(doc);
  browserPageState.markRendered('first-page-paint');
  if (!fpsOverlayEnabled && hasRealSceneContent) {
    fpsOverlayEnabled = true;
  }
  if (!browserContentReadySignaled && hasTextContent && typeof cmdStream.signalLoadscreenEnd === 'function') {
    cmdStream.signalLoadscreenEnd();
    browserContentReadySignaled = true;
  }
}

function requestBrowserContentRepaint() {
  invalidateBrowserRegionCache(false);
  paint();
}

function destroyBrowserRegion(entry) {
  const texId = Math.max(0, Number(entry && entry.texId || 0) | 0);
  if (texId > 0 && typeof cmdStream.destroyTexture === 'function') {
    try {
      cmdStream.destroyTexture(texId);
    } catch (_) {}
  }
}

function destroyBrowserRegionCache() {
  for (let i = 0; i < browserRegionCache.length; i += 1) {
    destroyBrowserRegion(browserRegionCache[i]);
  }
  browserRegionCache.length = 0;
  browserRegionCacheWidth = 0;
  browserRegionTileHeight = 0;
}

function invalidateBrowserRegionCache(reset = false) {
  browserRegionCacheRevision = (browserRegionCacheRevision + 1) >>> 0;
  if (browserRegionCacheRevision === 0) browserRegionCacheRevision = 1;
  if (reset) {
    destroyBrowserRegionCache();
    return;
  }
  for (let i = 0; i < browserRegionCache.length; i += 1) {
    browserRegionCache[i].dirty = true;
  }
}

function computeBrowserRegionTileHeight(vh) {
  const raw = Math.max(BROWSER_REGION_TILE_MIN_PX, Math.round(Number(vh || 1) * 1.5));
  const bounded = Math.min(BROWSER_REGION_TILE_MAX_PX, raw);
  const aligned = Math.ceil(bounded / BROWSER_REGION_TILE_ALIGN_PX) * BROWSER_REGION_TILE_ALIGN_PX;
  return Math.max(BROWSER_REGION_TILE_MIN_PX, Math.min(BROWSER_REGION_TILE_MAX_PX, aligned));
}

function createBrowserRegionEntry(width, height, docY) {
  const texId = Math.max(0, Number(
    typeof cmdStream.createRenderTarget === 'function'
      ? cmdStream.createRenderTarget(width, height)
      : 0,
  ) | 0);
  if (texId <= 0) return null;
  return {
    texId,
    width,
    height,
    docY,
    revision: 0,
    dirty: true,
    lastUsedSeq: 0,
  };
}

function browserRegionVisibleTop() {
  return Math.max(0, Math.round(Number(scrollY || 0)));
}

function ensureBrowserRegions(doc, vw, vh, contentH, contentTopY) {
  const width = Math.max(1, Number(vw || 1) | 0);
  const tileHeight = computeBrowserRegionTileHeight(vh);
  if (browserRegionCacheWidth !== width || browserRegionTileHeight !== tileHeight) {
    destroyBrowserRegionCache();
    browserRegionCacheWidth = width;
    browserRegionTileHeight = tileHeight;
  }

  const visibleTop = browserRegionVisibleTop();
  const visibleBottom = Math.max(visibleTop + 1, Math.min(contentH, visibleTop + Math.max(1, Number(vh || 1) | 0)));
  const prefetchPx = tileHeight * BROWSER_REGION_PREFETCH_SCREENS;
  const wantedTop = Math.max(0, visibleTop - prefetchPx);
  const wantedBottom = Math.max(visibleBottom, Math.min(contentH, visibleBottom + prefetchPx));
  const firstDocY = Math.max(0, Math.floor(wantedTop / tileHeight) * tileHeight);
  const wantedEntries = [];
  const wantedDocYs = new Set();

  for (let docY = firstDocY; docY < wantedBottom || wantedEntries.length <= 0; docY += tileHeight) {
    if (docY >= contentH && wantedEntries.length > 0) break;
    const height = Math.max(1, Math.min(tileHeight, contentH - docY));
    const key = `${docY}:${height}`;
    wantedDocYs.add(key);

    let entry = null;
    for (let i = 0; i < browserRegionCache.length; i += 1) {
      const candidate = browserRegionCache[i];
      if (candidate && candidate.docY === docY && candidate.height === height && candidate.width === width) {
        entry = candidate;
        break;
      }
    }
    if (!entry) {
      entry = createBrowserRegionEntry(width, height, docY);
      if (!entry) {
        destroyBrowserRegionCache();
        return null;
      }
      browserRegionCache.push(entry);
    }

    entry.lastUsedSeq = ++browserRegionCacheSeq;
    if (entry.dirty || entry.revision !== browserRegionCacheRevision) {
      cmdStream.setRenderTarget(entry.texId);
      cmdStream.setViewport(entry.width, entry.height);
      try {
        renderSceneRegionToCurrentTarget(doc, entry.width, entry.docY, entry.height);
      } finally {
        cmdStream.clearRenderTarget();
      }
      entry.revision = browserRegionCacheRevision;
      entry.dirty = false;
    }
    wantedEntries.push(entry);
  }

  for (let i = browserRegionCache.length - 1; i >= 0; i -= 1) {
    const entry = browserRegionCache[i];
    const key = `${entry.docY}:${entry.height}`;
    if (wantedDocYs.has(key)) continue;
    destroyBrowserRegion(entry);
    browserRegionCache.splice(i, 1);
  }

  while (browserRegionCache.length > BROWSER_REGION_CACHE_MAX) {
    let dropIdx = -1;
    let dropSeq = Number.POSITIVE_INFINITY;
    for (let i = 0; i < browserRegionCache.length; i += 1) {
      const entry = browserRegionCache[i];
      const key = `${entry.docY}:${entry.height}`;
      if (wantedDocYs.has(key)) continue;
      if (entry.lastUsedSeq < dropSeq) {
        dropSeq = entry.lastUsedSeq;
        dropIdx = i;
      }
    }
    if (dropIdx < 0) break;
    destroyBrowserRegion(browserRegionCache[dropIdx]);
    browserRegionCache.splice(dropIdx, 1);
  }

  wantedEntries.sort((lhs, rhs) => lhs.docY - rhs.docY);
  return wantedEntries;
}

function docContentHeight(doc, vh) {
  const raw = Math.max(
    Number(doc && doc.contentH || 0),
    Number(doc && doc.themeLayout && doc.themeLayout.contentH || 0),
    Number(vh || 0),
  );
  return Math.max(1, Math.round(Number.isFinite(raw) ? raw : Number(vh || 1)));
}

function docContentTopY(doc) {
  return 0;
}

function clampScrollForDoc(doc, vh) {
  const contentH = docContentHeight(doc, vh);
  const contentTopY = docContentTopY(doc);
  const maxScroll = Math.max(0, contentH - Math.max(1, Number(vh || 1)));
  if (scrollY < 0) scrollY = 0;
  if (scrollY > maxScroll) scrollY = maxScroll;
  return { contentH, contentTopY, maxScroll };
}

function paintToCurrentTarget(options = null) {
  if (!browserCanRenderScene) {
    return false;
  }
  const opts = options && typeof options === 'object' ? options : null;
  const { vw, vh } = computeViewport();
  const doc = ensureDoc(vw);
  const { contentH, contentTopY } = clampScrollForDoc(doc, vh);
  publishThemeLayoutInteractives(doc && doc.themeLayout ? doc.themeLayout : null);

  const overlayRuns = [];
  if (!opts || opts.includeFpsOverlay !== false) {
    if (fpsOverlayEnabled) {
      fpsOverlay.appendRuns(overlayRuns, vw);
    }
  }
  const composeViewportWidth = normalizeViewportSize(opts && opts.viewportWidth, vw);
  const composeViewportHeight = normalizeViewportSize(opts && opts.viewportHeight, vh);

  const regions = ensureBrowserRegions(doc, vw, vh, contentH, contentTopY);
  if (!regions || regions.length <= 0) {
    return false;
  }

  cmdStream.clearRenderTarget();
  cmdStream.setViewport(composeViewportWidth, composeViewportHeight);
  composeSceneRegionsToCurrentTarget(regions, vw, vh, scrollY, contentTopY, overlayRuns, null);
  cmdStream.clearRenderTarget();
  finalizePaintState(doc);

  return true;
}

function armHtmlReadyFallback() {
  if (htmlReadyTimeoutId != null) return;
  if (typeof runtime.host.setTimeout !== 'function') return;
  try {
    htmlReadyTimeoutId = runtime.host.setTimeout(() => {
      htmlReadyTimeoutId = null;
      if (browserCanRenderScene) return;
      setHtml(runtime.host.__trueosUiHtml || '');
    }, HTML_READY_TIMEOUT_MS);
  } catch (_) {
    htmlReadyTimeoutId = null;
  }
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
  if (typeof runtime.host.__trueosPrewarmUrl === 'function') {
    try { runtime.host.__trueosPrewarmUrl(resolved); } catch (_) {}
  }
  setCurrentPageUrl(resolved);
  const hook = runtime.host.__trueosBrowserNavigate;
  if (typeof hook === 'function') {
    const hookResult = dispatchBrowserAction('navigate', {
      url: currentPageUrl,
      request,
    }, '__trueosBrowserNavigate');
    if (hookResult && hookResult.handled) {
      return hookResult;
    }
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

  try {
    if (HOSTED_BY_UI2) {
      const doc = ensureDoc(vw);
      const { contentH, contentTopY } = clampScrollForDoc(doc, vh);
      publishThemeLayoutInteractives(doc && doc.themeLayout ? doc.themeLayout : null);
      if (typeof cmdStream.beginFrame === 'function' && typeof cmdStream.endFrame === 'function') {
        cmdStream.beginFrame();
        try {
          ensureBrowserRegions(doc, vw, vh, contentH, contentTopY);
        } finally {
          cmdStream.endFrame();
        }
      } else {
        ensureBrowserRegions(doc, vw, vh, contentH, contentTopY);
      }
      finalizePaintState(doc);
      return true;
    }

    if (typeof cmdStream.createRenderTarget === 'function') {
      cmdStream.setClearRgb(0xF4F4F4);
      cmdStream.setViewport(Math.max(1, Number(vw || 1) | 0), Math.max(1, Number(vh || 1) | 0));
      cmdStream.beginFrame();
      try {
        if (paintToCurrentTarget({
          viewportWidth: vw,
          viewportHeight: vh,
          includeFpsOverlay: true,
        })) {
          return true;
        }
      } finally {
        cmdStream.endFrame();
      }
    }

    const doc = ensureDoc(vw);
    const overlayRuns = [];
    if (fpsOverlayEnabled) {
      fpsOverlay.appendRuns(overlayRuns, vw);
    }
    renderScene(doc, vw, vh, scrollY, overlayRuns, null);
    finalizePaintState(doc);
    return true;
  } catch (err) {
    raiseBrowserError(
      'TRUEOS_BROWSER_RENDER_FAILED',
      'Browser paint failed while rendering scene',
      {
        reason: describeError(err),
        viewportWidth: vw,
        viewportHeight: vh,
        scrollY,
        rowCount: Array.isArray(cachedDoc && cachedDoc.rows) ? cachedDoc.rows.length : 0,
      },
      err,
    );
  }

  return true;
}

function getThemeInteractives(vw) {
  const doc = ensureDoc(vw);
  const interactives = publishThemeLayoutInteractives(doc && doc.themeLayout ? doc.themeLayout : null).interactives;
  return Array.isArray(interactives) ? interactives : [];
}

function getInteractiveState() {
  const { vw, vh } = computeViewport();
  const interactives = getThemeInteractives(vw);
  return {
    viewportWidth: Math.max(1, Number(vw || 1) | 0),
    viewportHeight: Math.max(1, Number(vh || 1) | 0),
    interactives: interactives.map((entry, index) => ({
      itemId: Math.max(1, index + 1),
      kindId: String(entry && entry.kind || '') === 'link'
        ? 2
        : (String(entry && entry.kind || '') === 'button' ? 1 : 0),
      x: Math.max(0, Number(entry && entry.x || 0) | 0),
      y: Math.max(0, Number(entry && entry.y || 0) | 0),
      width: Math.max(1, Number(entry && entry.width || 0) | 0),
      height: Math.max(1, Number(entry && entry.height || 0) | 0),
    })),
  };
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

const browserCursor = createBrowserCursorController({
  host: runtime.host,
  computeViewport,
  paint,
  onWheelDelta(dy) {
    const nextScroll = Math.max(0, Math.round(Number(scrollY || 0) + Number(dy || 0)));
    if (nextScroll === scrollY) return false;
    scrollY = nextScroll;
    paint();
    return true;
  },
  onReleaseRect() {},
});

browserCursor.startPump();

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

function InjectOpenAi() {
  void startAi('/qjs/ai/ai_pc.mjs');
}

runtime.host.__trueosBrowser = {
  paint,
  paintToCurrentTarget(options = null) {
    return paintToCurrentTarget(options);
  },
  setHtml,
  setNodeHtml,
  setBodyHtml,
  insertHtml,
  injectCursorEvent(event = null) {
    return browserCursor.injectCursorEvent(event);
  },
  getKernelCursors() {
    return browserCursor.getKernelCursors();
  },
  getKernelCursor(slotId) {
    return browserCursor.getKernelCursor(slotId);
  },
  popCursorButtonEvent() {
    return browserCursor.popCursorButtonEvent();
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
  dispatchDomClick(_path, _payload = null) {
    return { ...BROWSER_INTERACTION_EXTERNAL_RESULT };
  },
  dispatchButtonClick(_path, _payload = null) {
    return { ...BROWSER_INTERACTION_EXTERNAL_RESULT };
  },
  getTrueosFsTreeHtml(maxEntries = 64) {
    return getTrueosFsTreeHtml(maxEntries);
  },
  getViewport() {
    return getViewport();
  },
  getInteractiveState() {
    return getInteractiveState();
  },
  setViewportOverride(viewport = null, contentRect = null) {
    if (!viewport || typeof viewport !== 'object') {
      delete runtime.host.__trueosBrowserViewport;
      delete runtime.host.__trueosBrowserContentRect;
      invalidateBrowserRegionCache(true);
      paint();
      return true;
    }
    const nextViewport = {
      width: normalizeViewportSize(viewport.width, computeViewport().vw),
      height: normalizeViewportSize(viewport.height, computeViewport().vh),
    };
    const prevViewport = runtime.host.__trueosBrowserViewport;
    const prevContentRect = runtime.host.__trueosBrowserContentRect;
    const nextContentRect = contentRect && typeof contentRect === 'object'
      ? {
        x: Math.round(Number(contentRect.x || 0) || 0),
        y: Math.round(Number(contentRect.y || 0) || 0),
        width: normalizeViewportSize(contentRect.width, nextViewport.width),
        height: normalizeViewportSize(contentRect.height, nextViewport.height),
      }
      : {
        x: 0,
        y: 0,
        width: nextViewport.width,
        height: nextViewport.height,
      };
    const viewportUnchanged = !!prevViewport
      && Number(prevViewport.width || 0) === nextViewport.width
      && Number(prevViewport.height || 0) === nextViewport.height;
    const contentSizeUnchanged = !!prevContentRect
      && Number(prevContentRect.width || 0) === nextContentRect.width
      && Number(prevContentRect.height || 0) === nextContentRect.height;
    const contentOnlyMoved = viewportUnchanged
      && contentSizeUnchanged
      && (Number(prevContentRect.x || 0) !== nextContentRect.x
        || Number(prevContentRect.y || 0) !== nextContentRect.y);
    if (contentOnlyMoved) {
      return true;
    }
    runtime.host.__trueosBrowserViewport = nextViewport;
    runtime.host.__trueosBrowserContentRect = nextContentRect;
    invalidateBrowserRegionCache(true);
    paint();
    return true;
  },
  getSurfaceState() {
    const { vw, vh } = computeViewport();
    const doc = browserCanRenderScene ? ensureDoc(vw) : null;
    const { contentH, contentTopY } = doc
      ? clampScrollForDoc(doc, vh)
      : { contentH: vh, contentTopY: 0 };
    return {
      cacheRevision: browserRegionCacheRevision,
      cacheWidth: browserRegionCacheWidth,
      tileHeight: browserRegionTileHeight,
      regionCount: browserRegionCache.length,
      regions: browserRegionCache.map((entry) => ({
        texId: Math.max(0, Number(entry && entry.texId || 0) | 0),
        docY: Math.max(0, Number(entry && entry.docY || 0) | 0),
        width: Math.max(0, Number(entry && entry.width || 0) | 0),
        height: Math.max(0, Number(entry && entry.height || 0) | 0),
        revision: Math.max(0, Number(entry && entry.revision || 0) | 0),
        dirty: !!(entry && entry.dirty),
      })),
      viewportWidth: vw,
      viewportHeight: vh,
      contentHeight: Math.max(1, Number(contentH || vh) | 0),
      contentTopY: Math.max(0, Number(contentTopY || 0) | 0),
      scrollY,
    };
  },
  getPageLoadedState() {
    return browserPageState.getState('api');
  },
  popPageLoadedEvent() {
    return browserPageState.popEvent();
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
  getWindowId() {
    return currentWindowId();
  },
  getWindowInfo(windowId = null) {
    return getWindowInfo(windowId);
  },
  setWindowTitle(title, windowId = null) {
    const id = resolveWindowId(windowId);
    const fn = runtime.host.__trueosWindowSetTitle;
    if (id <= 0 || typeof fn !== 'function') return false;
    return !!fn(id, String(title || ''));
  },
  setWindowPosition(x, y, windowId = null) {
    return runWindowAction(windowId, '__trueosWindowSetPosition', Number(x || 0), Number(y || 0));
  },
  setWindowSize(width, height, windowId = null) {
    return runWindowAction(windowId, '__trueosWindowSetSize', Number(width || 0), Number(height || 0));
  },
  setWindowDecorations(mode = 'system', windowId = null) {
    return runWindowAction(windowId, '__trueosWindowSetDecorations', encodeWindowDecorations(mode));
  },
  minimizeWindow(windowId = null) {
    return runWindowAction(windowId, '__trueosWindowMinimize');
  },
  maximizeWindow(windowId = null) {
    return runWindowAction(windowId, '__trueosWindowMaximize');
  },
  restoreWindow(windowId = null) {
    return runWindowAction(windowId, '__trueosWindowRestore');
  },
  focusWindow(windowId = null) {
    return runWindowAction(windowId, '__trueosWindowFocus');
  },
  closeWindow(windowId = null) {
    return runWindowAction(windowId, '__trueosWindowClose');
  },
  beginWindowMove(windowId = null) {
    return runWindowAction(windowId, '__trueosWindowBeginMove');
  },
  beginWindowResize(edges, windowId = null) {
    const edgeMask = encodeWindowResizeEdges(edges);
    if (edgeMask === 0) return false;
    return runWindowAction(windowId, '__trueosWindowBeginResize', edgeMask);
  },
  setCurrentPageUrl(url = '') {
    return setCurrentPageUrl(url);
  },
  setScroll(y) {
    scrollY = Math.max(0, Math.round(Number(y || 0)));
    paint();
    return true;
  },
  moveCursor(target = null) {
    return browserCursor.moveCursor(target);
  },
  click(target = null) {
    return browserCursor.synthesizeClickAt(target, resolveInteractiveTarget);
  },
  navigate(input = null) {
    const request = input && typeof input === 'object' && !Array.isArray(input)
      ? { ...input }
      : { url: String(input || '') };
    const url = typeof request.url === 'string' ? request.url.trim() : '';
    return loadUrlIntoBrowser(url, request);
  },
  keyboard(input = null, options = null) {
    const parsed = parseKeyboardInput(input, options);
    const logEntry = recordKeyboardLog(parsed);
    try {
      console.log(`[browser-keyboard] seq=${logEntry.seq} events=${JSON.stringify(logEntry.events)}`);
    } catch (_err) {}

    const result = dispatchBrowserAction('keyboard', {
      events: logEntry.events.map((event) => (
        event.type === 'text'
          ? { type: 'text', text: event.text }
          : {
            type: 'key',
            key: event.key,
            modifiers: [...event.modifiers],
            repeat: event.repeat,
          }
      )),
      logOnly: logEntry.logOnly,
      seq: logEntry.seq,
    }, '__trueosBrowserKeyboard');

    return {
      ok: 1,
      handled: result && Number(result.handled || 0) > 0 ? 1 : 0,
      simulated: result && Number(result.simulated || 0) > 0 ? 1 : 0,
      logOnly: logEntry.logOnly ? 1 : 0,
      seq: logEntry.seq,
      eventCount: logEntry.events.length,
      events: logEntry.events,
    };
  },
  typeText(text, options = null) {
    return this.keyboard({ type: 'text', text: String(text || '') }, options);
  },
  pressKey(key, options = null) {
    const keySource = key && typeof key === 'object' && !Array.isArray(key) ? key : null;
    const keyName = typeof key === 'string'
      ? key
      : String(keySource && keySource.key != null ? keySource.key : '');
    const source = options && typeof options === 'object'
      ? options
      : (keySource || {});
    return this.keyboard({
      type: 'key',
      key: keyName,
      modifiers: source.modifiers || source.mods,
      repeat: source.repeat,
    }, source);
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

InjectOpenAi();
armHtmlReadyFallback();
installAiInputBridge();
installQjsInputBridge();
startAutoPaint();
