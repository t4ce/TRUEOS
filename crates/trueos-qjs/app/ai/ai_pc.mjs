import { isMainThread, parentPort } from "node:worker_threads";
import { readEnv } from "../vendor/openai/es2022/internal/utils/env.mjs";
import { buildAiPcShellToolBundle, findAiPcShellCommandByToolName } from "./ai_pc_cmd.mjs";
import * as driverdev from "../dd/driverdev.mjs";
import {
  DEFAULT_PARALLEL_TOOL_CALLS,
  DEFAULT_RESPONSE_MODEL,
  buildResponsesRequest,
  createOpenAiClient,
  createResponse,
  decorateResponseTools,
  getResponseOutputItems,
} from "./openai_client.mjs";
import {
  keyboardKeyToKernelSpec,
  keyboardModifiersToMask,
  parseKeyboardInput,
} from "../input/keyboard_wire.mjs";

const DEFAULT_MAX_STEPS = 50;
const DEFAULT_TOOL_SEARCH = true;
const WORKER_RPC_TIMEOUT_MS = 30000;
const HOST_BROWSER_RPC_POLL_MS = 10;
const HOST_INPUT_POLL_MS = 50;
const INPUT_CURSOR_FLAG_ABSOLUTE = 1;
const INPUT_CURSOR_FLAG_BUTTONS_CHANGED = 1 << 2;
const INPUT_KEYBOARD_FLAG_SYNTHETIC = 1 << 1;

function readBooleanEnv(name, fallback = false) {
  const raw = readEnv(name);
  if (typeof raw !== "string") {
    return fallback;
  }
  switch (raw.trim().toLowerCase()) {
    case "1":
    case "true":
    case "yes":
    case "on":
      return true;
    case "0":
    case "false":
    case "no":
    case "off":
      return false;
    default:
      return fallback;
  }
}

const HIDE_BROWSER_KEYBOARD_IN_RESPONSE_TOOLS = readBooleanEnv(
  "TRUEOS_AI_PC_DEBUG_HIDE_BROWSER_KEYBOARD_IN_RESPONSE_TOOLS",
  false,
);
const AI_PC_TOOL_POLICY = [
  "Use shell1 function tools whenever the user is asking to run, open, launch, inspect, or control something that maps to a shell1 command.",
  "For mounted TRUEOS filesystem inspection or DOM insertion of file lists, prefer read_trueosfs_tree or browser.getTrueosFsTreeHtml(...) over the interactive shell1 file wizard.",
  "For xHCI/USB driver debugging tasks, prefer driverdev_* tools over ad-hoc JavaScript snippets.",
  "When users ask what a USB HID device is, use driverdev_identify_hid_device and driverdev_get_hid_report_descriptor_hex before guessing.",
  "If asked to control HID/LED devices, use driverdev_set_hid_output_report_hex with explicit payload bytes and report IDs instead of claiming write access is unavailable.",
  "For devices classified as kind=leds, prefer the LED runtime interrupt-OUT tools before falling back to HID SET_REPORT control writes.",
  "Use ask_user only when the request is genuinely ambiguous or a required argument is missing.",
  "Treat cursor and pointer requests in browser/computer-use tasks as mouse-style pointer movement; prefer browser.moveCursor(...) instead of interpreting them as shell or terminal text-cursor requests unless the user explicitly says shell or terminal.",
  "Do not claim you cannot launch local apps or shell commands when a shell1 tool is available for the task.",
].join(" ");
const AI_PC_INSTRUCTIONS = [
  "You are TRUEOS AI PC, a powerful kernel-level agent running inside TRUEOS.",
  "You can inspect and influence the UI, browser state, shell1, USB/HID devices, storage, networked services, and other hardware-facing subsystems in real time.",
  "Operate like a capable system agent in a safe session with rich hardware access, but stay grounded in the live tools and observed device state instead of assumptions.",
  "Prefer taking concrete actions with tools over giving abstract advice when the task is actionable.",
  AI_PC_TOOL_POLICY,
].join(" ");
const WORKER_BROWSER_METHODS = [
  "getApiContract",
  "listUnavailable",
  "getWindowId",
  "getHtml",
  "getTextRows",
  "getDomSnapshot",
  "getTrueosFsTreeHtml",
  "setNodeHtml",
  "setBodyHtml",
  "insertHtml",
  "getViewport",
  "paint",
  "setScroll",
  "moveCursor",
  "click",
  "navigate",
  "keyboard",
  "typeText",
  "pressKey",
  "captureScreenshot",
];

function getExecJsBrowserApiDescription() {
  const methodList = HIDE_BROWSER_KEYBOARD_IN_RESPONSE_TOOLS
    ? "getHtml(), getTextRows(), getDomSnapshot(), getTrueosFsTreeHtml(maxEntries?), setNodeHtml(pathOrTarget, html), setBodyHtml(html), insertHtml(pathOrTarget, html, position), getViewport(), paint(), setScroll(y), moveCursor({ x, y, aiCursorId?, slotId?, buttonsDown?, flags? }), click(...), navigate(...), captureScreenshot(), and listUnavailable()."
    : "getHtml(), getTextRows(), getDomSnapshot(), getTrueosFsTreeHtml(maxEntries?), setNodeHtml(pathOrTarget, html), setBodyHtml(html), insertHtml(pathOrTarget, html, position), getViewport(), paint(), setScroll(y), moveCursor({ x, y, aiCursorId?, slotId?, buttonsDown?, flags? }), click(...), navigate(...), keyboard(...), typeText(...), pressKey(...), captureScreenshot(), and listUnavailable().";
  const keyboardDetails = HIDE_BROWSER_KEYBOARD_IN_RESPONSE_TOOLS
    ? ""
    : " keyboard(...) is the canonical keyboard API: send Unicode text with { type: \"text\", text: \"...\" } and named keys with { type: \"key\", key: \"Enter\", modifiers: [\"Ctrl\"]? }; typeText(...) and pressKey(...) compile into that same path.";
  return `TRUEOS browser facade. Call browser.getApiContract() first for the supported contract. Current live methods include ${methodList} Prefer setBodyHtml(html) when replacing the visible page content instead of guessing DOM paths. getDomSnapshot() returns a rooted tree object with a stable path field on each node; for flat scans, use snap.nodes. click(...) now drives the real cursor/button path and accepts coordinates, stable paths, text=..., plain caption text, and simple selectors like a[href="..."] when the target is interactive.${keyboardDetails} insertHtml() supports beforebegin, afterbegin, beforeend, and afterend. In the standalone AI runtime, moveCursor and keyboard writes go directly to kernel input while DOM and screenshot reads still come from the browser service. Use moveCursor for visible pointer movement rather than asking about a terminal text cursor.`;
}

function getFileSearchVectorStoreIds() {
  const vectorStoreId = readEnv("OPENAI_FILE_SEARCH_VECTOR_STORE_ID");
  return typeof vectorStoreId === "string" && vectorStoreId
    ? [vectorStoreId]
    : [];
}

function getPcRuntime() {
  if (!globalThis.__trueosAiPcRuntime) {
    globalThis.__trueosAiPcRuntime = {
      aiTarget: { id: "openai-embassy-primary", role: "primary" },
      browser: null,
      browserTarget: { windowId: 0, role: "primary" },
      context: null,
      page: null,
      jsOutput: [],
      inputSlotSeq: 1,
      inputSlots: Object.create(null),
      workerRpcSeq: 1,
      workerRpcPending: Object.create(null),
      workerRpcReady: false,
    };
  }
  return globalThis.__trueosAiPcRuntime;
}

function cloneAiTarget(target) {
  if (typeof target === "string" && target) {
    return { id: target, role: "named" };
  }
  if (!target || typeof target !== "object") {
    return { id: "openai-embassy-primary", role: "primary" };
  }
  return {
    id: typeof target.id === "string" && target.id ? target.id : "openai-embassy-primary",
    role: typeof target.role === "string" && target.role ? target.role : "primary",
  };
}

function cloneBrowserTarget(target) {
  if (!target || typeof target !== "object") {
    return { windowId: 0, role: "primary" };
  }
  return {
    windowId: Math.max(0, Number(target.windowId || 0) | 0),
    role: typeof target.role === "string" && target.role ? target.role : "primary",
  };
}

export function getConnectedAi() {
  return cloneAiTarget(getPcRuntime().aiTarget);
}

export function connect(ai = null, browser = null) {
  const runtime = getPcRuntime();
  const currentAi = cloneAiTarget(runtime.aiTarget);
  if (ai != null) {
    const requestedAi = cloneAiTarget(ai);
    if (requestedAi.id !== currentAi.id) {
      const err = new Error(
        `unsupported ai target: requested ${requestedAi.id}, active ${currentAi.id}`,
      );
      err.code = "TRUEOS_AI_TARGET_UNAVAILABLE";
      throw err;
    }
  }
  const nextBrowser = browser === null ? getConnectedBrowser() : connectBrowser(browser);
  return {
    ai: currentAi,
    browser: nextBrowser,
  };
}

export function connectBrowser(target = null) {
  const runtime = getPcRuntime();
  if (target == null) {
    runtime.browserTarget = { windowId: 0, role: "primary" };
    return cloneBrowserTarget(runtime.browserTarget);
  }
  if (typeof target === "number") {
    runtime.browserTarget = {
      windowId: Math.max(0, Number(target) | 0),
      role: "window",
    };
    return cloneBrowserTarget(runtime.browserTarget);
  }
  runtime.browserTarget = cloneBrowserTarget(target);
  return cloneBrowserTarget(runtime.browserTarget);
}

export function getConnectedBrowser() {
  return cloneBrowserTarget(getPcRuntime().browserTarget);
}

async function ensureDefaultBrowserConnection() {
  const current = getConnectedBrowser();
  if (current.windowId > 0 || current.role !== "primary") {
    return current;
  }

  const runtime = bindHostRuntime();
  const browser = runtime && runtime.browser && typeof runtime.browser === "object"
    ? runtime.browser
    : null;
  if (!browser || typeof browser.getWindowId !== "function") {
    return current;
  }

  try {
    const windowId = Number(await browser.getWindowId()) | 0;
    if (windowId > 0) {
      return connectBrowser(windowId);
    }
  } catch (_err) {}
  return current;
}

function hasWorkerParentPort() {
  return !isMainThread && !!parentPort && typeof parentPort.postMessage === "function";
}

function parseWorkerMessage(raw) {
  if (typeof raw !== "string" || !raw) {
    return null;
  }
  try {
    return JSON.parse(raw);
  } catch (_err) {
    return null;
  }
}

function ensureWorkerRpcReady() {
  const runtime = getPcRuntime();
  if (runtime.workerRpcReady || !hasWorkerParentPort()) {
    return;
  }
  parentPort.onMessage((raw) => {
    const message = parseWorkerMessage(raw);
    if (!message || message.kind !== "rpc_result") {
      return;
    }

    const pending = runtime.workerRpcPending[message.id];
    if (!pending) {
      return;
    }

    delete runtime.workerRpcPending[message.id];
    if (pending.timer && typeof clearTimeout === "function") {
      try {
        clearTimeout(pending.timer);
      } catch (_err) {}
    }

    if (message.ok) {
      pending.resolve(message.result);
      return;
    }

    const error = new Error(typeof message.error === "string" ? message.error : "worker rpc failed");
    if (message.code) {
      error.code = message.code;
    }
    pending.reject(error);
  });
  runtime.workerRpcReady = true;
}

function workerRpc(method, args = []) {
  if (!hasWorkerParentPort()) {
    return Promise.reject(new Error(`worker rpc unavailable for ${method}`));
  }

  ensureWorkerRpcReady();

  const runtime = getPcRuntime();
  const id = runtime.workerRpcSeq;
  runtime.workerRpcSeq += 1;

  return new Promise((resolve, reject) => {
    let timer = 0;
    if (typeof setTimeout === "function") {
      timer = setTimeout(() => {
        delete runtime.workerRpcPending[id];
        reject(new Error(`worker rpc timeout: ${method}`));
      }, WORKER_RPC_TIMEOUT_MS);
    }

    runtime.workerRpcPending[id] = { resolve, reject, timer };

    parentPort.postMessage(JSON.stringify({
      kind: "rpc_request",
      id,
      method,
      args,
    }));
  });
}

function sleepMs(ms) {
  return new Promise((resolve) => {
    if (typeof setTimeout === "function") {
      setTimeout(resolve, ms);
      return;
    }
    resolve();
  });
}

function hasHostBrowserRpc() {
  return typeof globalThis.__trueosBrowserRpcStart === "function"
    && typeof globalThis.__trueosBrowserRpcPoll === "function";
}

async function hostBrowserRpc(method, args = []) {
  if (!hasHostBrowserRpc()) {
    throw new Error(`host browser rpc unavailable for ${method}`);
  }

  const browserTarget = await ensureDefaultBrowserConnection();
  const argsJson = JSON.stringify(Array.isArray(args) ? args : []);
  const targetWindowId = Math.max(0, Number(browserTarget && browserTarget.windowId) | 0);
  const id = Number(
    globalThis.__trueosBrowserRpcStart(String(method || ""), argsJson, targetWindowId) || 0,
  ) | 0;
  if (id <= 0) {
    throw new Error(`host browser rpc start failed for ${method}`);
  }

  for (;;) {
    const raw = globalThis.__trueosBrowserRpcPoll(id);
    if (typeof raw === "string" && raw) {
      const message = JSON.parse(raw);
      if (message && message.ok) {
        return message.result;
      }
      const error = new Error(
        message && typeof message.error === "string"
          ? message.error
          : `host browser rpc failed for ${method}`,
      );
      if (message && message.code) {
        error.code = message.code;
      }
      throw error;
    }
    await sleepMs(HOST_BROWSER_RPC_POLL_MS);
  }
}

function hasDirectKernelInput() {
  return typeof globalThis.__trueosInputWriteCursor === "function"
    && typeof globalThis.__trueosInputWriteKeyboardText === "function"
    && typeof globalThis.__trueosInputWriteKeyboardKey === "function";
}

function resolveInputSlotId(source = null) {
  const runtime = getPcRuntime();
  const explicit = Number(source && source.slotId);
  if (Number.isFinite(explicit) && explicit >= 1) {
    return Math.floor(explicit);
  }

  const tagRaw = source && typeof source === "object"
    ? (source.aiKeyboardId || source.aiCursorId || source.slotTag || "")
    : "";
  const tag = typeof tagRaw === "string" && tagRaw.trim() ? tagRaw.trim() : "ai-default";
  if (!runtime.inputSlots[tag]) {
    runtime.inputSlots[tag] = runtime.inputSlotSeq;
    runtime.inputSlotSeq += 1;
  }
  return runtime.inputSlots[tag];
}

function directMoveCursor(target = null) {
  if (!hasDirectKernelInput()) {
    throw new Error("direct kernel input unavailable for moveCursor");
  }
  const source = target && typeof target === "object" ? target : {};
  const x = Number(source.x);
  const y = Number(source.y);
  if (!Number.isFinite(x) || !Number.isFinite(y)) {
    return false;
  }

  const slotId = resolveInputSlotId(source);
  const buttonsDown = Object.prototype.hasOwnProperty.call(source, "buttonsDown")
    ? (Number(source.buttonsDown || 0) >>> 0)
    : 0;
  const flags = Object.prototype.hasOwnProperty.call(source, "flags")
    ? (Number(source.flags || 0) >>> 0)
    : INPUT_CURSOR_FLAG_ABSOLUTE;
  const ok = Number(
    globalThis.__trueosInputWriteCursor(
      slotId,
      Math.round(x),
      Math.round(y),
      buttonsDown,
      0,
      flags,
    ) || 0,
  ) > 0;
  return ok;
}

function directClick(target = null) {
  if (!hasDirectKernelInput()) {
    throw new Error("direct kernel input unavailable for click");
  }
  const source = target && typeof target === "object" ? target : {};
  const x = Number(source.x);
  const y = Number(source.y);
  if (!Number.isFinite(x) || !Number.isFinite(y)) {
    return null;
  }

  const slotId = resolveInputSlotId(source);
  const buttonMask = Math.max(1, Number(source.buttonMask || source.button || 1) | 0) >>> 0;
  const moveOk = directMoveCursor({ ...source, slotId, x, y });
  const downOk = Number(
    globalThis.__trueosInputWriteCursor(
      slotId,
      Math.round(x),
      Math.round(y),
      buttonMask,
      0,
      INPUT_CURSOR_FLAG_ABSOLUTE | INPUT_CURSOR_FLAG_BUTTONS_CHANGED,
    ) || 0,
  ) > 0;
  const upOk = Number(
    globalThis.__trueosInputWriteCursor(
      slotId,
      Math.round(x),
      Math.round(y),
      0,
      0,
      INPUT_CURSOR_FLAG_ABSOLUTE | INPUT_CURSOR_FLAG_BUTTONS_CHANGED,
    ) || 0,
  ) > 0;
  return {
    ok: moveOk && downOk && upOk ? 1 : 0,
    handled: moveOk && downOk && upOk ? 1 : 0,
    simulated: 0,
    slotId,
    x: Math.round(x),
    y: Math.round(y),
  };
}

function directKeyboard(input = null, options = null) {
  if (!hasDirectKernelInput()) {
    throw new Error("direct kernel input unavailable for keyboard");
  }
  const parsed = parseKeyboardInput(input, options, (message) => {
    const err = new Error(message);
    err.code = "TRUEOS_BROWSER_KEYBOARD_INVALID";
    throw err;
  });
  const source = input && typeof input === "object" && !Array.isArray(input) ? input : null;
  const opts = options && typeof options === "object" ? options : null;
  const slotId = resolveInputSlotId(source || opts || null);

  let dispatched = 0;
  for (let i = 0; i < parsed.events.length; i += 1) {
    const event = parsed.events[i];
    if (event.type === "text") {
      const wrote = Number(
        globalThis.__trueosInputWriteKeyboardText(
          slotId,
          String(event.text || ""),
          INPUT_KEYBOARD_FLAG_SYNTHETIC,
        ) || 0,
      );
      if (wrote > 0) {
        dispatched += 1;
      }
      continue;
    }

    const spec = keyboardKeyToKernelSpec(event.key);
    if (!spec) {
      const err = new Error(`unsupported keyboard key: ${String(event.key || "")}`);
      err.code = "TRUEOS_BROWSER_KEYBOARD_INVALID";
      throw err;
    }
    const modifiers = keyboardModifiersToMask(event.modifiers);
    const repeat = Math.max(1, Number(event.repeat || 1) | 0);
    for (let rep = 0; rep < repeat; rep += 1) {
      const ok = Number(
        globalThis.__trueosInputWriteKeyboardKey(
          slotId,
          spec.codepoint >>> 0,
          spec.keyCode >>> 0,
          modifiers >>> 0,
          INPUT_KEYBOARD_FLAG_SYNTHETIC,
        ) || 0,
      ) > 0;
      if (ok) {
        dispatched += 1;
      }
    }
  }

  return {
    ok: 1,
    handled: dispatched > 0 ? 1 : 0,
    simulated: 0,
    logOnly: parsed.logOnly ? 1 : 0,
    slotId,
    eventCount: parsed.events.length,
    events: parsed.events,
  };
}

function augmentBrowserContractForHostInput(contract = null) {
  const base = contract && typeof contract === "object" ? contract : {};
  const available = Array.isArray(base.available) ? [...base.available] : [];
  const unavailable = Array.isArray(base.unavailable) ? [...base.unavailable] : [];
  for (const name of ["moveCursor", "click"]) {
    if (!available.includes(name)) {
      available.push(name);
    }
  }
  const filteredUnavailable = unavailable.filter((name) => name !== "moveCursor" && name !== "click");
  return {
    ...base,
    available,
    unavailable: filteredUnavailable,
  };
}

function createWorkerBrowserProxy() {
  const runtime = getPcRuntime();
  if (runtime.browser) {
    return runtime.browser;
  }

  const proxy = {};
  for (const method of WORKER_BROWSER_METHODS) {
    proxy[method] = (...args) => workerRpc(`browser.${method}`, args);
  }
  runtime.browser = proxy;
  runtime.context = proxy;
  runtime.page = proxy;
  return proxy;
}

function createHostBrowserProxy() {
  const runtime = getPcRuntime();
  if (runtime.browser) {
    return runtime.browser;
  }

  const proxy = {};
  for (const method of WORKER_BROWSER_METHODS) {
    proxy[method] = (...args) => hostBrowserRpc(method, args);
  }
  if (hasDirectKernelInput()) {
    const rpcGetApiContract = proxy.getApiContract;
    const rpcListUnavailable = proxy.listUnavailable;
    proxy.getApiContract = async (...args) => {
      const contract = await rpcGetApiContract(...args);
      return augmentBrowserContractForHostInput(contract);
    };
    proxy.listUnavailable = async (...args) => {
      const unavailable = await rpcListUnavailable(...args);
      return Array.isArray(unavailable)
        ? unavailable.filter((name) => name !== "moveCursor" && name !== "click")
        : [];
    };
    proxy.moveCursor = (target = null) => Promise.resolve(directMoveCursor(target));
    proxy.keyboard = (input = null, options = null) => Promise.resolve(directKeyboard(input, options));
    proxy.typeText = (text, options = null) => Promise.resolve(directKeyboard({
      type: "text",
      text: String(text || ""),
    }, options));
    proxy.pressKey = (key, options = null) => {
      const keySource = key && typeof key === "object" && !Array.isArray(key) ? key : null;
      const keyName = typeof key === "string"
        ? key
        : String(keySource && keySource.key != null ? keySource.key : "");
      const source = options && typeof options === "object"
        ? options
        : (keySource || {});
      return Promise.resolve(directKeyboard({
        type: "key",
        key: keyName,
        modifiers: source.modifiers || source.mods,
        repeat: source.repeat,
      }, source));
    };
    proxy.click = (target = null) => Promise.resolve(directClick(target));
  }
  runtime.browser = proxy;
  runtime.context = proxy;
  runtime.page = proxy;
  return proxy;
}

function bindHostRuntime() {
  const runtime = getPcRuntime();
  if (hasWorkerParentPort()) {
    createWorkerBrowserProxy();
    return runtime;
  }
  const browser = globalThis.__trueosBrowser;
  if (browser && typeof browser === "object") {
    runtime.browser = browser;
    runtime.context = browser;
    runtime.page = browser;
    return runtime;
  }
  if (hasHostBrowserRpc()) {
    createHostBrowserProxy();
  }
  return runtime;
}

function formatValue(value, depth) {
  if (depth > 2) {
    return "[depth-limit]";
  }
  if (value === null) {
    return "null";
  }
  const t = typeof value;
  if (t === "string") {
    return value;
  }
  if (t === "number" || t === "boolean" || t === "bigint" || t === "undefined") {
    return String(value);
  }
  if (t === "function") {
    return `[function ${value.name || "anonymous"}]`;
  }
  if (Array.isArray(value)) {
    const out = [];
    const limit = value.length > 8 ? 8 : value.length;
    for (let i = 0; i < limit; i += 1) {
      out.push(formatValue(value[i], depth + 1));
    }
    if (value.length > limit) {
      out.push(`...(${value.length - limit} more)`);
    }
    return `[${out.join(", ")}]`;
  }
  if (t === "object") {
    try {
      return JSON.stringify(value);
    } catch (_err) {
      const keys = Object.keys(value);
      const out = [];
      const limit = keys.length > 8 ? 8 : keys.length;
      for (let i = 0; i < limit; i += 1) {
        const key = keys[i];
        out.push(`${key}: ${formatValue(value[key], depth + 1)}`);
      }
      if (keys.length > limit) {
        out.push(`...(${keys.length - limit} more)`);
      }
      return `{${out.join(", ")}}`;
    }
  }
  return String(value);
}

function formatArgs(args) {
  const out = [];
  for (let i = 0; i < args.length; i += 1) {
    out.push(formatValue(args[i], 0));
  }
  return out.join(" ");
}

function makeExecConsole(jsOutput) {
  return {
    log(...args) {
      jsOutput.push({
        type: "input_text",
        text: formatArgs(args),
      });
    },
  };
}

function displayImage(imageInput) {
  const runtime = getPcRuntime();
  const imageUrl = typeof imageInput === "string" && imageInput.startsWith("data:image/")
    ? imageInput
    : `data:image/png;base64,${imageInput}`;
  runtime.jsOutput.push({
    type: "input_image",
    image_url: imageUrl,
    detail: "original",
  });
}

function base64DecodedByteLength(value) {
  const text = typeof value === "string" ? value.trim() : "";
  if (!text) {
    return 0;
  }
  const padding = text.endsWith("==") ? 2 : (text.endsWith("=") ? 1 : 0);
  return Math.max(0, Math.floor((text.length * 3) / 4) - padding);
}

function extractImageUrlByteLength(imageUrl) {
  if (typeof imageUrl !== "string") {
    return 0;
  }
  const marker = ";base64,";
  const markerIndex = imageUrl.indexOf(marker);
  if (markerIndex < 0) {
    return 0;
  }
  return base64DecodedByteLength(imageUrl.slice(markerIndex + marker.length));
}

function logScreenshotUploadSizes(input) {
  if (!Array.isArray(input)) {
    return;
  }
  for (const item of input) {
    if (!item || typeof item !== "object") {
      continue;
    }
    const content = Array.isArray(item.content) ? item.content : [];
    for (const part of content) {
      if (!part || part.type !== "input_image") {
        continue;
      }
      const byteLength = extractImageUrlByteLength(part.image_url);
      if (byteLength > 0) {
        console.log(`[ai_pc.mjs] screenshot upload png_bytes=${byteLength}`);
      }
    }
  }
}

function shellPrint(text) {
  const value = typeof text === "string" ? text : String(text == null ? "" : text);
  if (!value) {
    return;
  }
  if (typeof globalThis.__trueosUart1ShellWrite === "function") {
    try {
      globalThis.__trueosUart1ShellWrite(value);
      return;
    } catch (_err) {}
  }
  if (hasWorkerParentPort()) {
    void workerRpc("host.shellPrint", [value]);
  }
}

async function awaitHostInput(question = "") {
  if (typeof globalThis.__trueosAiAwaitInput === "function") {
    return await globalThis.__trueosAiAwaitInput(question);
  }
  if (typeof globalThis.__trueosAiInputPop === "function") {
    for (;;) {
      const raw = globalThis.__trueosAiInputPop(question);
      if (typeof raw === "string" && raw) {
        try {
          return JSON.parse(raw);
        } catch (_err) {
          return { text: raw };
        }
      }
      await sleepMs(HOST_INPUT_POLL_MS);
    }
  }
  if (hasWorkerParentPort()) {
    return await workerRpc("host.awaitInput", [question]);
  }
  throw new Error("AI input is not wired; host must expose an AI input bridge");
}

function normalizeAiInputEntry(entry) {
  const source = entry && typeof entry === "object" && !Array.isArray(entry) ? entry : null;
  const text = typeof entry === "string"
    ? entry
    : (source && typeof source.text === "string" ? source.text : "");
  const value = text.trim();
  if (!value) {
    return null;
  }
  return {
    text: value,
    webSearch: !!(source && source.webSearch),
    fileSearch: !!(source && source.fileSearch),
    newConversation: !!(source && source.newConversation),
    computerUse: !source || source.computerUse !== false,
  };
}

function createExecJsTool() {
  return {
    type: "function",
    name: "exec_js",
    description: "Execute provided interactive JavaScript in a persistent REPL context.",
    parameters: {
      type: "object",
      properties: {
        code: {
          type: "string",
          description: `
JavaScript to execute. Write small snippets of interactive code. To persist variables or functions across tool calls, you must save them to globalThis. Code is executed in an async persistent eval context, so you can use await. You have access to ONLY the following:
- console.log(x): Use this to read contents back to you. But be minimal: otherwise the output may be too long. Avoid using console.log() for large image payloads like screenshots or buffers. If you create an image or screenshot, pass the image data directly to display().
- display(base64_or_data_url): Use this to view either a bare base64-encoded PNG payload or a full data URL. browser.captureScreenshot() already returns a full data URL.
- Do not write screenshots or image data to temporary files or disk just to pass them back. Keep image data in memory and send it directly to display().
- browser: ${getExecJsBrowserApiDescription()}
- context: same object as browser for now.
- page: same object as browser for now.
`,
        },
      },
      required: ["code"],
      additionalProperties: false,
    },
  };
}

function createAskUserTool() {
  return {
    type: "function",
    name: "ask_user",
    description: "Ask the user a clarification question and wait for their response.",
    parameters: {
      type: "object",
      properties: {
        question: {
          type: "string",
          description: "The exact question to show the human. Use this instead of answering with a freeform clarifying question in a final answer.",
        },
      },
      required: ["question"],
      additionalProperties: false,
    },
  };
}

function createReadTrueosFsTreeTool() {
  return {
    type: "function",
    name: "read_trueosfs_tree",
    description: "Read the mounted TRUEOSFS cached filesystem tree as preformatted HTML from the primary mounted root.",
    parameters: {
      type: "object",
      properties: {
        maxEntries: {
          type: "integer",
          description: "Maximum number of filesystem entries to include in the returned tree HTML.",
          minimum: 1,
        },
      },
      required: [],
      additionalProperties: false,
    },
  };
}

function createShell1Tools() {
  try {
    return buildAiPcShellToolBundle();
  } catch (_err) {
    return [];
  }
}

function bytesToHex(bytes) {
  if (!bytes || typeof bytes.length !== "number") {
    return "";
  }
  let out = "";
  for (let i = 0; i < bytes.length; i += 1) {
    const value = bytes[i] & 0xFF;
    const hex = value.toString(16).padStart(2, "0");
    out += hex;
  }
  return out;
}

function toJsonStringOrNull(value) {
  if (value == null) {
    return "null";
  }
  try {
    const encoded = JSON.stringify(value);
    return typeof encoded === "string" ? encoded : "null";
  } catch (_err) {
    return "null";
  }
}

function ledRcHint(rc) {
  if (rc === 0) {
    return "ok";
  }
  if (rc === -2) {
    return "invalid payloadHex (non-hex, odd length, or exceeds LED bridge limit)";
  }
  if (rc === -1) {
    return "send failed (device/runtime rejected or handle not claimed by leds runtime)";
  }
  return "unknown return code";
}

function createDriverDevTools() {
  return [
    {
      type: "function",
      name: "driverdev_list_devices",
      description: "List enumerated xHCI devices as JSON summaries from /qjs/dd/driverdev.mjs.",
      parameters: {
        type: "object",
        properties: {},
        required: [],
        additionalProperties: false,
      },
    },
    {
      type: "function",
      name: "driverdev_get_device_descriptor",
      description: "Fetch and decode the USB device descriptor for a device handle.",
      parameters: {
        type: "object",
        properties: {
          handle: {
            type: "integer",
            description: "Packed TRUEOS driverdev handle (controller<<24 | slot).",
            minimum: 0,
          },
        },
        required: ["handle"],
        additionalProperties: false,
      },
    },
    {
      type: "function",
      name: "driverdev_get_descriptor_hex",
      description: "Read a raw descriptor and return lower-case hex bytes.",
      parameters: {
        type: "object",
        properties: {
          handle: {
            type: "integer",
            description: "Packed TRUEOS driverdev handle (controller<<24 | slot).",
            minimum: 0,
          },
          descType: {
            type: "integer",
            description: "USB descriptor type (for example 1=device, 2=configuration, 3=string).",
            minimum: 0,
            maximum: 255,
          },
          descIndex: {
            type: "integer",
            description: "Descriptor index for this type.",
            minimum: 0,
          },
          length: {
            type: "integer",
            description: "Requested descriptor byte length.",
            minimum: 1,
            maximum: 4096,
          },
        },
        required: ["handle", "descType"],
        additionalProperties: false,
      },
    },
    {
      type: "function",
      name: "driverdev_get_string",
      description: "Fetch and decode a USB string descriptor by index.",
      parameters: {
        type: "object",
        properties: {
          handle: {
            type: "integer",
            description: "Packed TRUEOS driverdev handle (controller<<24 | slot).",
            minimum: 0,
          },
          index: {
            type: "integer",
            description: "USB string descriptor index.",
            minimum: 0,
            maximum: 255,
          },
          langId: {
            type: "integer",
            description: "USB LANGID for string decoding, default 0x0409.",
            minimum: 0,
            maximum: 65535,
          },
        },
        required: ["handle", "index"],
        additionalProperties: false,
      },
    },
    {
      type: "function",
      name: "driverdev_port_reset",
      description: "Reset an xHCI root hub port on a specific controller.",
      parameters: {
        type: "object",
        properties: {
          controllerId: {
            type: "integer",
            description: "Controller id reported by driverdev listDevices().",
            minimum: 0,
            maximum: 255,
          },
          portIdx: {
            type: "integer",
            description: "Zero-based port index.",
            minimum: 0,
          },
        },
        required: ["controllerId", "portIdx"],
        additionalProperties: false,
      },
    },
    {
      type: "function",
      name: "driverdev_read_transfer_event",
      description: "Read a buffered xHCI transfer completion event for an endpoint target.",
      parameters: {
        type: "object",
        properties: {
          handle: {
            type: "integer",
            description: "Packed TRUEOS driverdev handle (controller<<24 | slot).",
            minimum: 0,
          },
          epTarget: {
            type: "integer",
            description: "Endpoint target used by the transfer path.",
            minimum: 0,
            maximum: 255,
          },
        },
        required: ["handle", "epTarget"],
        additionalProperties: false,
      },
    },
    {
      type: "function",
      name: "driverdev_get_hid_report_descriptor_hex",
      description: "Read a HID report descriptor from an interface and return lower-case hex bytes.",
      parameters: {
        type: "object",
        properties: {
          handle: {
            type: "integer",
            description: "Packed TRUEOS driverdev handle (controller<<24 | slot).",
            minimum: 0,
          },
          interfaceNumber: {
            type: "integer",
            description: "USB interface number for the HID function.",
            minimum: 0,
            maximum: 255,
          },
          length: {
            type: "integer",
            description: "Requested HID report descriptor byte length.",
            minimum: 1,
            maximum: 4096,
          },
        },
        required: ["handle"],
        additionalProperties: false,
      },
    },
    {
      type: "function",
      name: "driverdev_identify_hid_device",
      description: "Identify likely HID device kind by parsing the first application collection usage from its report descriptor.",
      parameters: {
        type: "object",
        properties: {
          handle: {
            type: "integer",
            description: "Packed TRUEOS driverdev handle (controller<<24 | slot).",
            minimum: 0,
          },
          interfaceNumber: {
            type: "integer",
            description: "USB interface number for the HID function.",
            minimum: 0,
            maximum: 255,
          },
        },
        required: ["handle"],
        additionalProperties: false,
      },
    },
    {
      type: "function",
      name: "driverdev_set_hid_report_hex",
      description: "Send a HID class SET_REPORT control transfer using hex payload bytes.",
      parameters: {
        type: "object",
        properties: {
          handle: {
            type: "integer",
            description: "Packed TRUEOS driverdev handle (controller<<24 | slot).",
            minimum: 0,
          },
          interfaceNumber: {
            type: "integer",
            description: "USB interface number for the HID function.",
            minimum: 0,
            maximum: 255,
          },
          reportType: {
            type: "integer",
            description: "HID report type: 1=input, 2=output, 3=feature.",
            minimum: 1,
            maximum: 3,
          },
          reportId: {
            type: "integer",
            description: "HID report ID (0 when reports are unnumbered).",
            minimum: 0,
            maximum: 255,
          },
          payloadHex: {
            type: "string",
            description: "Even-length hex string containing payload bytes without spaces.",
          },
        },
        required: ["handle", "payloadHex"],
        additionalProperties: false,
      },
    },
    {
      type: "function",
      name: "driverdev_set_hid_output_report_hex",
      description: "Convenience wrapper for HID output report writes (reportType=2).",
      parameters: {
        type: "object",
        properties: {
          handle: {
            type: "integer",
            description: "Packed TRUEOS driverdev handle (controller<<24 | slot).",
            minimum: 0,
          },
          interfaceNumber: {
            type: "integer",
            description: "USB interface number for the HID function.",
            minimum: 0,
            maximum: 255,
          },
          reportId: {
            type: "integer",
            description: "HID report ID (0 when reports are unnumbered).",
            minimum: 0,
            maximum: 255,
          },
          payloadHex: {
            type: "string",
            description: "Even-length hex string containing payload bytes without spaces.",
          },
        },
        required: ["handle", "payloadHex"],
        additionalProperties: false,
      },
    },
    {
      type: "function",
      name: "driverdev_leds_send_output_report_hex",
      description: "Send a report over the claimed LED runtime interrupt-OUT endpoint using an explicit report ID and payload hex.",
      parameters: {
        type: "object",
        properties: {
          handle: {
            type: "integer",
            description: "Packed TRUEOS driverdev handle (controller<<24 | slot) for a device claimed by the leds runtime.",
            minimum: 0,
          },
          reportId: {
            type: "integer",
            description: "Report ID byte to prefix, or 0 when the device expects no report ID prefix.",
            minimum: 0,
            maximum: 255,
          },
          payloadHex: {
            type: "string",
            description: "Even-length hex string containing payload bytes without spaces.",
          },
        },
        required: ["handle", "payloadHex"],
        additionalProperties: false,
      },
    },
    {
      type: "function",
      name: "driverdev_leds_send_preferred_output_report_hex",
      description: "Send an interrupt-OUT LED report using the runtime's preferred report ID and expected report length padding.",
      parameters: {
        type: "object",
        properties: {
          handle: {
            type: "integer",
            description: "Packed TRUEOS driverdev handle (controller<<24 | slot) for a device claimed by the leds runtime.",
            minimum: 0,
          },
          payloadHex: {
            type: "string",
            description: "Even-length hex string containing payload bytes without spaces.",
          },
        },
        required: ["handle", "payloadHex"],
        additionalProperties: false,
      },
    },
  ];
}

function buildTools(entry) {
  const tools = [];
  if (DEFAULT_TOOL_SEARCH) {
    tools.push({ type: "tool_search" });
  }
  if (entry.webSearch) {
    tools.push({ type: "web_search" });
  }
  if (entry.fileSearch) {
    const vectorStoreIds = getFileSearchVectorStoreIds();
    if (vectorStoreIds.length > 0) {
      tools.push({
        type: "file_search",
        vector_store_ids: vectorStoreIds,
      });
    }
  }
  tools.push(createReadTrueosFsTreeTool());
  tools.push(...createShell1Tools());
  tools.push(...createDriverDevTools());
  if (entry.computerUse) {
    tools.push(createExecJsTool());
  }
  tools.push(createAskUserTool());
  return decorateResponseTools(tools);
}

async function readTrueosFsTree(maxEntries = 64) {
  const runtime = bindHostRuntime();
  if (!runtime.browser || typeof runtime.browser.getTrueosFsTreeHtml !== "function") {
    throw new Error("TRUEOSFS tree bridge is not available");
  }
  const limit = Number(maxEntries);
  const normalized = Number.isFinite(limit) && limit > 0 ? Math.floor(limit) : 64;
  return await runtime.browser.getTrueosFsTreeHtml(normalized);
}

function submitShell1Input(line) {
  const commandLine = typeof line === "string" ? line : String(line == null ? "" : line);
  if (!commandLine) {
    throw new Error("shell1 command line is empty");
  }
  if (typeof globalThis.__trueosShell1SubmitInput !== "function") {
    throw new Error("shell1 input bridge is not available");
  }
  let submitted = 0;
  submitted += Number(globalThis.__trueosShell1SubmitInput(commandLine)) || 0;
  submitted += Number(globalThis.__trueosShell1SubmitInput("\r")) || 0;
  return submitted;
}

function getShell1Runtime() {
  const runtime = globalThis.__trueosShell1Runtime;
  return runtime && typeof runtime === "object" ? runtime : null;
}

function shell1HistoryTotalLines() {
  try {
    const runtime = getShell1Runtime();
    if (!runtime || typeof runtime.historyTotalLines !== "function") {
      return 0;
    }
    return Math.max(0, Number(runtime.historyTotalLines()) || 0);
  } catch (_err) {
    return 0;
  }
}

function shell1HistoryTextSince(startLine, maxLines = 64) {
  try {
    const runtime = getShell1Runtime();
    if (!runtime || typeof runtime.historyTextSince !== "function") {
      return "";
    }
    return String(runtime.historyTextSince(startLine, maxLines) || "");
  } catch (_err) {
    return "";
  }
}

function waitMs(ms) {
  return new Promise((resolve) => {
    setTimeout(resolve, Math.max(0, Number(ms) || 0));
  });
}

function sanitizeTransportText(text, maxChars = 6000) {
  const value = typeof text === "string" ? text : String(text == null ? "" : text);
  if (!value) {
    return "";
  }
  let out = value
    // Strip common ANSI/ECMA-48 CSI sequences if any sneak through.
    .replace(/\u001B\[[0-9;?]*[ -/]*[@-~]/g, "")
    // Strip other C0 controls except tab/newline/carriage return.
    .replace(/[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F]/g, "");
  if (out.length > maxChars) {
    out = `${out.slice(0, maxChars)}\n...[truncated]`;
  }
  return out;
}

async function captureShell1OutputSince(startLine, options = null) {
  const cfg = options && typeof options === "object" ? options : {};
  const maxLines = Number.isFinite(cfg.maxLines) ? Math.max(1, Math.floor(cfg.maxLines)) : 48;
  const settleMs = Number.isFinite(cfg.settleMs) ? Math.max(0, Math.floor(cfg.settleMs)) : 140;
  const pollMs = Number.isFinite(cfg.pollMs) ? Math.max(0, Math.floor(cfg.pollMs)) : 120;
  const maxPolls = Number.isFinite(cfg.maxPolls) ? Math.max(1, Math.floor(cfg.maxPolls)) : 5;

  try {
    let lastCount = shell1HistoryTotalLines();
    let stableCount = 0;
    for (let i = 0; i < maxPolls; i += 1) {
      await waitMs(i === 0 ? settleMs : pollMs);
      const nextCount = shell1HistoryTotalLines();
      if (nextCount === lastCount) {
        stableCount += 1;
        if (stableCount >= 2) {
          break;
        }
      } else {
        stableCount = 0;
        lastCount = nextCount;
      }
    }
    return sanitizeTransportText(shell1HistoryTextSince(startLine, maxLines).trim());
  } catch (_err) {
    return sanitizeTransportText(shell1HistoryTextSince(startLine, maxLines).trim());
  }
}

function buildShell1CommandLine(command, payload) {
  const parts = [command.command];
  for (const arg of command.args) {
    if (!Object.prototype.hasOwnProperty.call(payload, arg.name)) {
      continue;
    }
    const value = payload[arg.name];
    if (value == null || value === "") {
      continue;
    }
    parts.push(String(value));
  }
  return parts.join(" ");
}

async function execJs(code) {
  const runtime = bindHostRuntime();
  const execConsole = makeExecConsole(runtime.jsOutput);
  const wrappedCode = `
    (async (console, display, browser, context, page) => {
      ${code}
    })
  `;
  const factory = (0, eval)(wrappedCode);
  return await factory(execConsole, displayImage, runtime.browser, runtime.context, runtime.page);
}

async function runTurn(client, entry, previousResponseId, maxSteps = DEFAULT_MAX_STEPS, model = DEFAULT_RESPONSE_MODEL) {
  const tools = buildTools(entry);
  let nextInput = [
    {
      role: "user",
      content: entry.text,
    },
  ];

  for (let i = 0; i < maxSteps; i += 1) {
    const request = buildResponsesRequest({
      model,
      instructions: AI_PC_INSTRUCTIONS,
      tools,
      input: nextInput,
      previousResponseId,
      parallelToolCalls: DEFAULT_PARALLEL_TOOL_CALLS,
    });

    logScreenshotUploadSizes(request.input);

    const resp = await createResponse(client, request);
    previousResponseId = resp.id;

    let hadToolCall = false;
    const toolOutputs = [];

    for (const item of getResponseOutputItems(resp)) {
      const shell1Command = item.type === "function_call"
        ? findAiPcShellCommandByToolName(item.name)
        : null;
      if (shell1Command) {
        hadToolCall = true;
        let output = "submitted shell1 command.";
        try {
          const parsed = JSON.parse(item.arguments || "{}");
          const commandLine = buildShell1CommandLine(shell1Command, parsed);
          const baselineLines = shell1HistoryTotalLines();
          const submitted = submitShell1Input(commandLine);
          const shellOutput = await captureShell1OutputSince(baselineLines);
          output = shellOutput
            ? `submitted shell1 command: ${commandLine} (${submitted} bytes)\n\nshell1 output:\n${shellOutput}`
            : `submitted shell1 command: ${commandLine} (${submitted} bytes). No new shell1 history lines were captured yet.`;
        } catch (err) {
          output = `submitted shell1 command, but shell capture failed: ${String(err && err.message ? err.message : err)}`;
        }
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output,
        });
      } else if (item.type === "function_call" && item.name === "exec_js") {
        hadToolCall = true;
        const parsed = JSON.parse(item.arguments || "{}");
        const code = parsed.code || "";
        const runtime = bindHostRuntime();

        console.log(code);
        console.log("----");

        try {
          await execJs(code);
        } catch (e) {
          runtime.jsOutput.push({
            type: "input_text",
            text: formatArgs([e, e && e.message, e && e.stack]),
          });
        }

        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: runtime.jsOutput.slice(),
        });

        for (const out of runtime.jsOutput) {
          if (out.type === "input_text") {
            console.log("JS LOG:", out.text);
          } else if (out.type === "input_image") {
            console.log("JS IMAGE: [image payload omitted]");
          }
        }
        console.log("=====");

        runtime.jsOutput.length = 0;
      } else if (item.type === "function_call" && item.name === "read_trueosfs_tree") {
        hadToolCall = true;
        const parsed = JSON.parse(item.arguments || "{}");
        const html = await readTrueosFsTree(parsed.maxEntries);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: typeof html === "string" ? html : "",
        });
      } else if (item.type === "function_call" && item.name === "driverdev_list_devices") {
        hadToolCall = true;
        const devices = driverdev.listDevices();
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify(devices),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_get_device_descriptor") {
        hadToolCall = true;
        const parsed = JSON.parse(item.arguments || "{}");
        const desc = driverdev.getDeviceDescriptor(parsed.handle);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify(desc),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_get_descriptor_hex") {
        hadToolCall = true;
        const parsed = JSON.parse(item.arguments || "{}");
        const descIndex = Number.isInteger(parsed.descIndex) ? parsed.descIndex : 0;
        const length = Number.isInteger(parsed.length) ? parsed.length : 255;
        const bytes = driverdev.getDescriptor(parsed.handle, parsed.descType, descIndex, length);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify({
            hex: bytesToHex(bytes),
            byteLength: bytes ? bytes.length : 0,
          }),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_get_string") {
        hadToolCall = true;
        const parsed = JSON.parse(item.arguments || "{}");
        const langId = Number.isInteger(parsed.langId) ? parsed.langId : 0x0409;
        const value = driverdev.getString(parsed.handle, parsed.index, langId);
        const output = typeof value === "string"
          ? value
          : (value && typeof value.length === "number"
            ? bytesToHex(value)
            : "null");
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output,
        });
      } else if (item.type === "function_call" && item.name === "driverdev_port_reset") {
        hadToolCall = true;
        const parsed = JSON.parse(item.arguments || "{}");
        const rc = driverdev.portReset(parsed.controllerId, parsed.portIdx);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify({ rc }),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_read_transfer_event") {
        hadToolCall = true;
        const parsed = JSON.parse(item.arguments || "{}");
        const event = driverdev.readTransferEvent(parsed.handle, parsed.epTarget);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: toJsonStringOrNull(event),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_get_hid_report_descriptor_hex") {
        hadToolCall = true;
        const parsed = JSON.parse(item.arguments || "{}");
        const interfaceNumber = Number.isInteger(parsed.interfaceNumber) ? parsed.interfaceNumber : 0;
        const length = Number.isInteger(parsed.length) ? parsed.length : 512;
        const bytes = driverdev.getHidReportDescriptor(parsed.handle, interfaceNumber, length);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify({
            hex: bytesToHex(bytes),
            byteLength: bytes ? bytes.length : 0,
            interfaceNumber,
          }),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_identify_hid_device") {
        hadToolCall = true;
        const parsed = JSON.parse(item.arguments || "{}");
        const interfaceNumber = Number.isInteger(parsed.interfaceNumber) ? parsed.interfaceNumber : 0;
        const result = driverdev.identifyHidDevice(parsed.handle, interfaceNumber);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: toJsonStringOrNull(result),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_set_hid_report_hex") {
        hadToolCall = true;
        const parsed = JSON.parse(item.arguments || "{}");
        const interfaceNumber = Number.isInteger(parsed.interfaceNumber) ? parsed.interfaceNumber : 0;
        const reportType = Number.isInteger(parsed.reportType) ? parsed.reportType : 2;
        const reportId = Number.isInteger(parsed.reportId) ? parsed.reportId : 0;
        const payloadHex = typeof parsed.payloadHex === "string" ? parsed.payloadHex : "";
        const rc = driverdev.setHidReport(parsed.handle, interfaceNumber, reportType, reportId, payloadHex);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify({
            rc,
            interfaceNumber,
            reportType,
            reportId,
            payloadLenBytes: Math.floor(payloadHex.length / 2),
          }),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_set_hid_output_report_hex") {
        hadToolCall = true;
        const parsed = JSON.parse(item.arguments || "{}");
        const interfaceNumber = Number.isInteger(parsed.interfaceNumber) ? parsed.interfaceNumber : 0;
        const reportId = Number.isInteger(parsed.reportId) ? parsed.reportId : 0;
        const payloadHex = typeof parsed.payloadHex === "string" ? parsed.payloadHex : "";
        const rc = driverdev.setHidReport(parsed.handle, interfaceNumber, 2, reportId, payloadHex);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify({
            rc,
            interfaceNumber,
            reportType: 2,
            reportId,
            payloadLenBytes: Math.floor(payloadHex.length / 2),
          }),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_leds_send_output_report_hex") {
        hadToolCall = true;
        const parsed = JSON.parse(item.arguments || "{}");
        const reportId = Number.isInteger(parsed.reportId) ? parsed.reportId : 0;
        const payloadHex = typeof parsed.payloadHex === "string" ? parsed.payloadHex : "";
        const rc = driverdev.sendLedOutputReport(parsed.handle, reportId, payloadHex);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify({
            rc,
            reportId,
            payloadLenBytes: Math.floor(payloadHex.length / 2),
            rcHint: ledRcHint(rc),
          }),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_leds_send_preferred_output_report_hex") {
        hadToolCall = true;
        const parsed = JSON.parse(item.arguments || "{}");
        const payloadHex = typeof parsed.payloadHex === "string" ? parsed.payloadHex : "";
        const rc = driverdev.sendLedPreferredOutputReport(parsed.handle, payloadHex);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify({
            rc,
            payloadLenBytes: Math.floor(payloadHex.length / 2),
            rcHint: ledRcHint(rc),
          }),
        });
      } else if (item.type === "function_call" && item.name === "ask_user") {
        hadToolCall = true;
        const parsed = JSON.parse(item.arguments || "{}");
        const question = parsed.question || "Please provide more information.";
        console.log(`MODEL QUESTION: ${question}`);
        shellPrint(`\r\nai: ${question}\r\n`);
        const answerEntry = normalizeAiInputEntry(await awaitHostInput(question));
        const answer = answerEntry ? answerEntry.text : "";
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: answer,
        });
      } else if (item.type === "message") {
        const content = Array.isArray(item.content) ? item.content[0] : item.content;
        const text = content && content.text ? content.text : content;
        console.log(text);
        if (typeof text === "string" && text) {
          shellPrint(`${text}\r\n`);
        }
      }
    }

    if (!hadToolCall) {
      return previousResponseId;
    }

    nextInput = toolOutputs;
  }

  return previousResponseId;
}

async function waitForNextInput(question = "") {
  try {
    return normalizeAiInputEntry(await awaitHostInput(question));
  } catch (_err) {
    return null;
  }
}

export async function startAiPc() {
  if (hasWorkerParentPort()) {
    ensureWorkerRpcReady();
  }
  if (globalThis.__trueosAiPcStarted) {
    return false;
  }
  globalThis.__trueosAiPcStarted = true;
  try {
    await ensureDefaultBrowserConnection();
    const client = createOpenAiClient();
    let previousResponseId = null;
    while (true) {
      const entry = await waitForNextInput("");
      if (!entry) {
        continue;
      }
      if (entry.newConversation) {
        previousResponseId = null;
      }
      previousResponseId = await runTurn(client, entry, previousResponseId);
    }
  } finally {
    globalThis.__trueosAiPcStarted = false;
  }
}

export async function startAiPcWorker() {
  ensureWorkerRpcReady();
  return await startAiPc();
}

if (isMainThread) {
  void startAiPc();
}
