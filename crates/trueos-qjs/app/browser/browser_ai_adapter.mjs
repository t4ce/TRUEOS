import { Worker } from 'node:worker_threads';

const DEFAULT_AI_INPUT_OPTIONS = Object.freeze({
  webSearch: false,
  fileSearch: false,
  newConversation: false,
  computerUse: true,
});
const DEFAULT_AI_CURSOR_ID = 'ai-default';

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
    'moveCursor',
    'click',
    'navigate',
    'keyboard',
    'typeText',
    'pressKey',
    'captureScreenshot',
  ],
  unavailable: [],
  notes: {
    intent: 'Worker-facing browser contract for the AI task. Keep this surface explicit so agent logic remains isolated from the browser VM.',
    targetShape: 'Close to future computer-use style APIs while still reflecting TRUEOS capabilities today.',
    domSnapshotShape: 'Returns a rooted tree object; use snap.nodes for a flat compatibility index.',
    clickShape: 'click() uses the real cursor/button path and can resolve coordinates, stable paths, text=..., plain captions, and simple href/path/text selectors for interactive targets.',
    keyboardShape: 'keyboard(...) accepts Unicode text entries and strict key entries with optional modifiers; pressKey(...) and typeText(...) compile into that canonical event list.',
  },
};

function cloneApiContract(host) {
  const contract = host.__trueosBrowserAiApiContract;
  const source = contract && typeof contract === 'object'
    ? contract
    : FALLBACK_BROWSER_API_CONTRACT;
  return JSON.parse(JSON.stringify(source));
}

function notYetAvailable(host, name) {
  const err = new Error(`browser API not yet available: ${name}`);
  err.code = typeof host.__trueosBrowserAiApiUnavailableCode === 'string'
    ? host.__trueosBrowserAiApiUnavailableCode
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

function ensureState(browser, host) {
  if (host.__trueosBrowserAiState && host.__trueosBrowserAiState.browser === browser) {
    return host.__trueosBrowserAiState;
  }
  const state = {
    browser,
    host,
    aiStartPromise: null,
    aiStartSpecifier: '',
    aiWorker: null,
    aiInputQueue: [],
    aiInputWaiters: [],
  };
  host.__trueosBrowserAiState = state;
  return state;
}

function pushAiInput(state, entry, options = null) {
  const value = normalizeAiInput(entry, options);
  if (!value) return false;

  const waiter = state.aiInputWaiters.shift();
  if (waiter) {
    waiter(value);
    return true;
  }

  state.aiInputQueue.push(value);
  return true;
}

function awaitAiInput(state, question = '') {
  const prompt = typeof question === 'string' ? question.trim() : '';
  if (prompt) {
    try { console.log(`[browser-ai] ai input requested: ${prompt}`); } catch (_) {}
  }
  if (state.aiInputQueue.length > 0) {
    return Promise.resolve(state.aiInputQueue.shift());
  }
  return new Promise((resolve) => {
    state.aiInputWaiters.push(resolve);
  });
}

async function dispatchAiWorkerRpc(state, method, args) {
  const api = state.browser;
  if (typeof method !== 'string' || !method.startsWith('browser.')) {
    throw new Error(`unsupported worker rpc method: ${method}`);
  }

  const name = method.slice('browser.'.length);
  if (name === 'getApiContract') return cloneApiContract(state.host);
  if (name === 'listUnavailable') return cloneApiContract(state.host).unavailable;

  if (!api || typeof api[name] !== 'function') {
    throw new Error(`browser rpc missing method: ${name}`);
  }

  return await api[name](...(Array.isArray(args) ? args : []));
}

async function handleAiWorkerMessage(state, worker, raw) {
  const message = parseWorkerJson(raw);
  if (!message) return;

  if (typeof message.dbg === 'string') {
    try { console.log('[browser-ai] ai worker', message.dbg); } catch (_) {}
    return;
  }

  if (message.kind !== 'rpc_request') return;

  try {
    let result;
    if (message.method === 'host.awaitInput') {
      result = await awaitAiInput(state, String((message.args && message.args[0]) || ''));
    } else if (message.method === 'host.shellPrint') {
      const text = String((message.args && message.args[0]) || '');
      if (typeof state.host.__trueosUart1ShellWrite === 'function' && text) {
        state.host.__trueosUart1ShellWrite(text);
      }
      result = true;
    } else {
      result = await dispatchAiWorkerRpc(state, message.method, message.args);
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

function buildAiWorkerSource(specifier) {
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
      console.log('[browser-ai-worker] start failed', String(err && err.stack ? err.stack : err));
    } catch (_) {}
  });
`;
}

function attachAiWorker(state, worker) {
  worker.onMessage((raw) => {
    void handleAiWorkerMessage(state, worker, raw);
  });
  return worker;
}

function normalizeAiSpecifier(specifier) {
  if (typeof specifier === 'string' && specifier) {
    return specifier;
  }
  return '/qjs/ai/ai_pc.mjs';
}

function placeDefaultAiCursor(browser) {
  if (!browser || typeof browser.moveCursor !== 'function' || typeof browser.getViewport !== 'function') {
    return;
  }
  Promise.resolve(browser.getViewport())
    .then((viewport) => {
      const width = Math.max(1, Number(viewport && viewport.width || 0));
      const height = Math.max(1, Number(viewport && viewport.height || 0));
      return browser.moveCursor({
        aiCursorId: DEFAULT_AI_CURSOR_ID,
        x: Math.floor(width * 0.25),
        y: Math.floor(height * 0.25),
      });
    })
    .catch(() => {});
}

function startAi(state, specifier = '/qjs/ai/ai_pc.mjs', options = null) {
  const resolvedSpecifier = normalizeAiSpecifier(specifier);
  const opts = options && typeof options === 'object' ? options : null;
  const initialInput = opts && Object.prototype.hasOwnProperty.call(opts, 'input')
    ? normalizeAiInput(opts.input, opts)
    : null;
  if (state.aiStartPromise && state.aiStartSpecifier === resolvedSpecifier) {
    if (initialInput) {
      pushAiInput(state, initialInput);
    }
    return state.aiStartPromise;
  }
  if (state.aiWorker) {
    try { state.aiWorker.terminate(); } catch (_) {}
    state.aiWorker = null;
  }
  state.aiStartSpecifier = resolvedSpecifier;
  state.aiStartPromise = Promise.resolve()
    .then(() => {
      const worker = attachAiWorker(state, new Worker(buildAiWorkerSource(resolvedSpecifier)));
      state.aiWorker = worker;
      placeDefaultAiCursor(state.browser);
      if (initialInput) {
        pushAiInput(state, initialInput);
      }
      return worker;
    })
    .catch((err) => {
      state.aiStartPromise = null;
      state.aiStartSpecifier = '';
      state.aiWorker = null;
      try {
        console.log('[browser-ai] ai worker start failed', String(err && err.stack ? err.stack : err));
      } catch (_) {}
      throw err;
    });
  return state.aiStartPromise;
}

export function installBrowserAi(browser = null, host = null) {
  const runtimeHost = host && typeof host === 'object'
    ? host
    : ((typeof globalThis !== 'undefined') ? globalThis : this);
  const targetBrowser = browser && typeof browser === 'object'
    ? browser
    : runtimeHost.__trueosBrowser;
  if (!targetBrowser || typeof targetBrowser !== 'object') {
    throw new Error('browser ai adapter requires a browser object');
  }

  const state = ensureState(targetBrowser, runtimeHost);

  runtimeHost.__trueosAiInputPush = (entry, options = null) => pushAiInput(state, entry, options);
  runtimeHost.__trueosAiAwaitInput = (question = '') => awaitAiInput(state, question);

  targetBrowser.getApiContract = () => cloneApiContract(runtimeHost);
  targetBrowser.listUnavailable = () => cloneApiContract(runtimeHost).unavailable;
  if (typeof targetBrowser.setNodeHtml === 'function') {
    targetBrowser.setBodyHtml = (html) => targetBrowser.setNodeHtml('body', html);
  }
  targetBrowser.getTrueosFsTreeHtml = (maxEntries = 64) => {
    if (typeof runtimeHost.__trueosReadPrimaryTrueosFsTreeHtml !== 'function') {
      return null;
    }
    const limit = Number(maxEntries);
    const normalized = Number.isFinite(limit) && limit > 0 ? Math.floor(limit) : 64;
    const html = runtimeHost.__trueosReadPrimaryTrueosFsTreeHtml(normalized);
    return typeof html === 'string' && html ? html : null;
  };
  targetBrowser.startAi = (specifier, options) => startAi(state, specifier, options);
  targetBrowser.startAiPc = (input, options = null) => {
    const opts = options && typeof options === 'object' ? { ...options, input } : { input };
    return startAi(state, '/qjs/ai/ai_pc.mjs', opts);
  };
  targetBrowser.submitAiInput = (input, options = null) => pushAiInput(state, input, options);
  if (typeof targetBrowser.keyboard !== 'function') {
    targetBrowser.keyboard = () => notYetAvailable(runtimeHost, 'keyboard');
  }
  targetBrowser.typeText = (text, options = null) => targetBrowser.keyboard({
    type: 'text',
    text: String(text || ''),
  }, options);
  targetBrowser.captureScreenshot = () => {
    if (typeof runtimeHost.__trueosCaptureScreenshot !== 'function') {
      return notYetAvailable(runtimeHost, 'captureScreenshot');
    }
    const image = runtimeHost.__trueosCaptureScreenshot();
    if (typeof image !== 'string' || !image) {
      return notYetAvailable(runtimeHost, 'captureScreenshot');
    }
    return image;
  };

  return targetBrowser;
}
