import { isMainThread, parentPort } from "node:worker_threads";
import { readEnv } from "../vendor/openai/es2022/internal/utils/env.mjs";
import { buildAiPcShellToolBundle, findAiPcShellCommandByToolName } from "./ai_pc_cmd.mjs";
import * as driverdev from "../dd/driverdev.mjs";
import {
  DEFAULT_PARALLEL_TOOL_CALLS,
  DEFAULT_REASONING_EFFORT,
  DEFAULT_RESPONSE_MODEL,
  buildResponsesRequest,
  createOpenAiClient,
  createResponse,
  createResponseStream,
  decorateResponseTools,
  getResponseOutputItems,
  getResponseOutputText,
} from "./openai_client.mjs";
import {
  keyboardKeyToKernelSpec,
  keyboardModifiersToMask,
  parseKeyboardInput,
} from "../input/keyboard_wire.mjs";

const DEFAULT_MAX_STEPS = 50;
const DEFAULT_TOOL_SEARCH = false;
const DEFAULT_COMPUTER_HANDOFF_AFTER_TOOL_CALLS = 2;
const DEFAULT_BROWSER_CONTEXT_MAX_TEXT_ROWS = 48;
const DEFAULT_BROWSER_CONTEXT_MAX_HTML_CHARS = 4000;
const DEFAULT_BROWSER_CONTEXT_MAX_DOM_NODES = 80;
const WORKER_RPC_TIMEOUT_MS = 30000;
const HOST_BROWSER_RPC_POLL_MS = 10;
const HOST_INPUT_POLL_MS = 50;
const INPUT_CURSOR_FLAG_ABSOLUTE = 1;
const INPUT_CURSOR_FLAG_BUTTONS_CHANGED = 1 << 2;
const INPUT_KEYBOARD_FLAG_SYNTHETIC = 1 << 1;
const FALLBACK_SCREENSHOT_DATA_URL = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAukB9VE1l0AAAAAASUVORK5CYII=";

function aiDiag(message) {
  try {
    console.log(`[ai_pc.mjs] ${message}`);
  } catch (_err) {}
}

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

function readStringEnv(name, fallback = "") {
  const raw = readEnv(name);
  if (typeof raw !== "string") {
    return fallback;
  }
  const trimmed = raw.trim();
  return trimmed ? trimmed : fallback;
}

const HIDE_BROWSER_KEYBOARD_IN_RESPONSE_TOOLS = readBooleanEnv(
  "TRUEOS_AI_PC_DEBUG_HIDE_BROWSER_KEYBOARD_IN_RESPONSE_TOOLS",
  false,
);
const OPENAI_COMPUTER_TOOL_TYPE = readStringEnv(
  "TRUEOS_OPENAI_COMPUTER_TOOL_TYPE",
  "computer",
);
const OPENAI_COMPUTER_ENVIRONMENT = readStringEnv(
  "TRUEOS_OPENAI_COMPUTER_ENVIRONMENT",
  "browser",
);
const OPENAI_COMPUTER_MODEL = readStringEnv(
  "TRUEOS_OPENAI_COMPUTER_MODEL",
  "gpt-5.4",
);
const OPENAI_COMPUTER_REASONING_EFFORT = readStringEnv(
  "TRUEOS_OPENAI_COMPUTER_REASONING_EFFORT",
  "medium",
);
const ENABLE_SHELL1_COMMAND_REGISTRY_TOOLS = readBooleanEnv(
  "TRUEOS_AI_ENABLE_SHELL1_COMMAND_REGISTRY_TOOLS",
  false,
);
const AI_PC_HARD_MODE_RETRY_PROMPT = [
  "This turn is running in ai-pc hard mode.",
  "Do not answer conversationally before using the computer tool.",
  "Use the computer tool now and perform a real computer action or screenshot-driven computer step for the user's request.",
].join(" ");
const AI_PC_TOOL_POLICY = [
  "The old shell1 command registry tools are disabled by default because AI work should stay anchored to the shell session it was launched from instead of detouring through shell1.",
  "Hosted tool_search is disabled by default. Use the explicit web mode/search tools only when the user requested web-backed lookup.",
  "For mounted TRUEOS filesystem inspection or DOM insertion of file lists, prefer read_trueosfs_tree or browser.getTrueosFsTreeHtml(...) over old shell-driven file wizards.",
  "When you need current page or browser context, use read_browser_context before asking the user for page details that can be inspected directly.",
  "When the task is to update the live browser page content, prefer the browser_set_body_html, browser_set_node_html, and browser_insert_html tools over opening a new surf data URL.",
  "For DOM replacement or page-update requests, do not detour through surf, cmd, tlb.*, generated files, or old shell registry commands unless the user explicitly asked for shell output instead of a browser DOM change.",
  "Never say that the DOM, page, UI, or browser content was changed unless you actually called a browser_set_* tool in this turn and received a successful tool result.",
  "For xHCI/USB driver debugging tasks, prefer driverdev_* tools over ad-hoc JavaScript snippets.",
  "When users ask what a USB HID device is, use driverdev_identify_hid_device and driverdev_get_hid_report_descriptor_hex before guessing.",
  "If asked to control HID/LED devices, use driverdev_set_hid_output_report_hex with explicit payload bytes and report IDs instead of claiming write access is unavailable.",
  "For devices classified as kind=leds, prefer the LED runtime interrupt-OUT tools before falling back to HID SET_REPORT control writes.",
  "Use ask_user only when the request is genuinely ambiguous or a required argument is missing.",
  "Treat cursor and pointer requests in browser/computer-use tasks as mouse-style pointer movement; prefer computer.moveCursor(...) instead of interpreting them as shell or terminal text-cursor requests unless the user explicitly says shell or terminal.",
  "For visible UI interaction, use the built-in OpenAI computer tool path backed by the local TRUEOS computer/browser runtime.",
  "In ai-pc mode, start with the computer tool immediately instead of chatting first. Only leave computer mode after at least one real computer action if structured tools are needed afterward.",
  "If old shell1 registry tools are explicitly enabled, treat them as a compatibility path only, not the default execution lane.",
].join(" ");
const AI_PC_INSTRUCTIONS = [
  "You are TRUEOS AI PC, a powerful kernel-level agent running inside TRUEOS.",
  "You can inspect and influence the UI, browser state, the invoking shell session, USB/HID devices, storage, networked services, and other hardware-facing subsystems in real time.",
  "Operate like a capable system agent in a safe session with rich hardware access, but stay grounded in the live tools and observed device state instead of assumptions.",
  "Prefer taking concrete actions with tools over giving abstract advice when the task is actionable.",
  AI_PC_TOOL_POLICY,
].join(" ");

function isDomMutationRequestText(text) {
  const value = typeof text === "string" ? text.toLowerCase() : "";
  if (!value) {
    return false;
  }
  return (
    value.includes("replace dom")
    || value.includes("replace the dom")
    || value.includes("replace page")
    || value.includes("replace the page")
    || value.includes("set body html")
    || value.includes("update dom")
    || value.includes("change dom")
    || value.includes("modify dom")
  );
}

function summarizeResponseItems(items) {
  if (!Array.isArray(items) || items.length === 0) {
    return "(none)";
  }
  const parts = [];
  for (const item of items) {
    if (!item || typeof item !== "object") {
      parts.push("unknown");
      continue;
    }
    let label = String(item.type || "unknown");
    if (label === "function_call" && typeof item.name === "string" && item.name) {
      label += `:${item.name}`;
    } else if (label === "message") {
      const content = Array.isArray(item.content) ? item.content : [];
      const contentTypes = content
        .map((part) => String(part && part.type ? part.type : "unknown"))
        .filter(Boolean);
      if (contentTypes.length > 0) {
        label += `:${contentTypes.join("+")}`;
      }
    }
    parts.push(label);
  }
  return parts.join(", ");
}

function extractReasoningText(item) {
  const texts = [];
  const pushText = (value) => {
    if (typeof value !== "string") {
      return;
    }
    const trimmed = value.trim();
    if (!trimmed) {
      return;
    }
    if (!texts.includes(trimmed)) {
      texts.push(trimmed);
    }
  };

  pushText(item && item.summary_text);

  const summary = Array.isArray(item && item.summary) ? item.summary : [];
  for (const entry of summary) {
    if (!entry || typeof entry !== "object") {
      continue;
    }
    pushText(entry.text);
    pushText(entry.summary_text);
  }

  const content = Array.isArray(item && item.content) ? item.content : [];
  for (const part of content) {
    if (!part || typeof part !== "object") {
      continue;
    }
    pushText(part.text);
    pushText(part.summary_text);
  }

  return texts.join(" ").trim();
}

function extractMessageText(item) {
  if (!item || item.type !== "message") {
    return "";
  }
  const parts = [];
  const content = Array.isArray(item.content) ? item.content : [];
  for (const part of content) {
    if (part && part.type === "output_text" && typeof part.text === "string" && part.text) {
      parts.push(part.text);
      continue;
    }
    if (typeof part === "string" && part) {
      parts.push(part);
      continue;
    }
    if (part && typeof part.text === "string" && part.text) {
      parts.push(part.text);
    }
  }
  return parts.join("");
}

function formatReasoningProgress(item) {
  const status = typeof item?.status === "string" && item.status
    ? item.status
    : "active";
  const text = extractReasoningText(item);
  const compact = text
    ? (text.length > 180 ? `${text.slice(0, 180)}...` : text)
    : "";
  return compact
    ? `reasoning (${status}): ${compact}`
    : `reasoning (${status})`;
}

function isHostedProgressResponseItem(item) {
  const type = String(item && item.type ? item.type : "");
  return (
    type === "tool_search_call"
    || type === "tool_search_output"
    || type === "web_search_call"
    || type === "web_search_preview_call"
    || type === "file_search_call"
  );
}

const WORKER_BROWSER_METHODS = [
  "getApiContract",
  "listUnavailable",
  "getWindowId",
  "getWindowInfo",
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
  "setWindowTitle",
  "setWindowIcon",
  "setWindowPosition",
  "setWindowSize",
  "setWindowDecorations",
  "setWindowVerticalScrollbarSide",
  "setWindowHorizontalScrollbarSide",
  "minimizeWindow",
  "maximizeWindow",
  "restoreWindow",
  "focusWindow",
  "closeWindow",
  "beginWindowMove",
  "beginWindowResize",
  "moveCursor",
  "click",
  "navigate",
  "keyboard",
  "typeText",
  "pressKey",
];

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
    aiDiag(`browser connection already set windowId=${current.windowId} role=${current.role}`);
    return current;
  }

  const runtime = bindHostRuntime();
  const browser = runtime && runtime.browser && typeof runtime.browser === "object"
    ? runtime.browser
    : null;
  if (!browser || typeof browser.getWindowId !== "function") {
    aiDiag("browser connection unresolved: getWindowId unavailable");
    return current;
  }

  try {
    const windowId = Number(await browser.getWindowId()) | 0;
    aiDiag(`browser getWindowId -> ${windowId}`);
    if (windowId > 0) {
      return connectBrowser(windowId);
    }
  } catch (err) {
    aiDiag(`browser getWindowId threw: ${String(err && err.message ? err.message : err)}`);
  }
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

  const argsJson = JSON.stringify(Array.isArray(args) ? args : []);
  let targetWindowId = 0;
  if (method !== "getWindowId") {
    const browserTarget = await ensureDefaultBrowserConnection();
    targetWindowId = Math.max(0, Number(browserTarget && browserTarget.windowId) | 0);
  }
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

function hasDirectScreenshot() {
  return typeof globalThis.__trueosCaptureScreenshot === "function";
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

function createComputerContract(hasDirectInput, hasScreenshot) {
  const available = ["getApiContract", "listUnavailable"];
  const unavailable = [];
  if (hasDirectInput) {
    available.push("moveCursor", "click");
    if (!HIDE_BROWSER_KEYBOARD_IN_RESPONSE_TOOLS) {
      available.push("keyboard", "typeText", "pressKey");
    }
  } else {
    unavailable.push("moveCursor", "click", "keyboard", "typeText", "pressKey");
  }
  if (hasScreenshot) {
    available.push("captureScreenshot");
  } else {
    unavailable.push("captureScreenshot");
  }
  return {
    version: 1,
    available,
    unavailable,
    notes: {
      intent: "Worker-facing computer-use facade backed by kernel/ui2 input and full composed screenshots.",
      targetShape: "Close to computer-use action harnesses: pointer, keyboard, screenshot.",
      screenshotShape: "captureScreenshot() returns the full composed UI, not just browser content.",
    },
  };
}

function createComputerProxy() {
  const runtime = getPcRuntime();
  if (runtime.computer && typeof runtime.computer === "object") {
    return runtime.computer;
  }

  const hasDirectInput = hasDirectKernelInput();
  const hasScreenshot = hasDirectScreenshot();
  const contract = createComputerContract(hasDirectInput, hasScreenshot);

  const proxy = {
    getApiContract() {
      return {
        version: contract.version,
        available: [...contract.available],
        unavailable: [...contract.unavailable],
        notes: { ...contract.notes },
      };
    },
    listUnavailable() {
      return [...contract.unavailable];
    },
    moveCursor(target = null) {
      if (!hasDirectInput) throw new Error("computer input unavailable: moveCursor");
      return Promise.resolve(directMoveCursor(target));
    },
    click(target = null) {
      if (!hasDirectInput) throw new Error("computer input unavailable: click");
      return Promise.resolve(directClick(target));
    },
    keyboard(input = null, options = null) {
      if (!hasDirectInput) throw new Error("computer input unavailable: keyboard");
      return Promise.resolve(directKeyboard(input, options));
    },
    typeText(text, options = null) {
      if (!hasDirectInput) throw new Error("computer input unavailable: typeText");
      return Promise.resolve(directKeyboard({
        type: "text",
        text: String(text || ""),
      }, options));
    },
    pressKey(key, options = null) {
      if (!hasDirectInput) throw new Error("computer input unavailable: pressKey");
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
    },
    captureScreenshot() {
      if (hasDirectScreenshot()) {
        const image = globalThis.__trueosCaptureScreenshot();
        if (typeof image === "string" && image) {
          return image;
        }
      }
      throw new Error("computer screenshot unavailable");
    },
  };

  runtime.computer = proxy;
  return proxy;
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
  runtime.browser = proxy;
  runtime.context = proxy;
  runtime.page = proxy;
  return proxy;
}

function bindHostRuntime() {
  const runtime = getPcRuntime();
  if (hasWorkerParentPort()) {
    aiDiag("bindHostRuntime: worker parent port mode");
    createWorkerBrowserProxy();
    createComputerProxy();
    return runtime;
  }
  const browser = globalThis.__trueosBrowser;
  if (browser && typeof browser === "object") {
    aiDiag("bindHostRuntime: using direct __trueosBrowser");
    runtime.browser = browser;
    runtime.context = browser;
    runtime.page = browser;
    createComputerProxy();
    return runtime;
  }
  if (hasHostBrowserRpc()) {
    aiDiag("bindHostRuntime: using host browser RPC");
    createHostBrowserProxy();
    createComputerProxy();
  } else {
    aiDiag("bindHostRuntime: no browser binding available");
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

let shellPrintPending = "";
let shellPrintTargetMask = 0;

function setShellPrintTargetMask(targetMask) {
  const value = Number(targetMask);
  shellPrintTargetMask = Number.isFinite(value) && value >= 0 ? (value | 0) : 0;
}

function shellEmitLine(text) {
  const value = typeof text === "string" ? text : String(text == null ? "" : text);
  if (!value) {
    return;
  }
  if (typeof globalThis.__trueosShell2PrintLine === "function") {
    try {
      globalThis.__trueosShell2PrintLine(value, shellPrintTargetMask);
      return;
    } catch (_err) {}
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

function shellNormalizeText(text) {
  return String(text == null ? "" : text).replace(/\r\n/g, "\n").replace(/\r/g, "\n");
}

function shellEmitBlock(text) {
  const normalized = shellNormalizeText(text);
  if (!normalized) {
    return;
  }
  const lines = normalized.split("\n");
  while (lines.length > 0 && lines[lines.length - 1] === "") {
    lines.pop();
  }
  for (let i = lines.length - 1; i >= 0; i -= 1) {
    shellEmitLine(lines[i]);
  }
}

function shellPrint(text) {
  const value = shellNormalizeText(text);
  if (!value) {
    return;
  }
  shellPrintPending += value;
}

function shellFlush() {
  if (!shellPrintPending) {
    return;
  }
  const block = shellPrintPending;
  shellPrintPending = "";
  shellEmitBlock(block);
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
    computerUse: !!(source && source.computerUse),
    shellTargetMask: source ? (Number(source.shellTargetMask) || 0) : 0,
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

function createReadBrowserContextTool() {
  return {
    type: "function",
    name: "read_browser_context",
    description: "Inspect the currently connected browser window and page context, including window info, viewport, and visible text rows. Use this before asking the user for page/browser context that may already be available.",
    parameters: {
      type: "object",
      properties: {
        includeHtml: {
          type: "boolean",
          description: "Include a trimmed HTML snapshot.",
        },
        includeDomSnapshot: {
          type: "boolean",
          description: "Include a trimmed DOM snapshot object.",
        },
        maxTextRows: {
          type: "integer",
          description: "Maximum number of text rows to include.",
          minimum: 1,
          maximum: 256,
        },
        maxHtmlChars: {
          type: "integer",
          description: "Maximum number of HTML characters to include when includeHtml is true.",
          minimum: 64,
          maximum: 24000,
        },
        maxDomNodes: {
          type: "integer",
          description: "Maximum number of DOM nodes to include when includeDomSnapshot is true.",
          minimum: 1,
          maximum: 512,
        },
      },
      required: [],
      additionalProperties: false,
    },
  };
}

function createBrowserSetBodyHtmlTool() {
  return {
    type: "function",
    name: "browser_set_body_html",
    description: "Replace the current browser document body contents with the provided HTML.",
    parameters: {
      type: "object",
      properties: {
        html: {
          type: "string",
          description: "HTML fragment to install as the body contents.",
        },
      },
      required: ["html"],
      additionalProperties: false,
    },
  };
}

function createBrowserSetNodeHtmlTool() {
  return {
    type: "function",
    name: "browser_set_node_html",
    description: "Replace the children of a specific DOM node matched by selector or explicit target descriptor.",
    parameters: {
      type: "object",
      properties: {
        target: {
          description: "DOM target selector string or structured target object accepted by the browser runtime.",
        },
        html: {
          type: "string",
          description: "HTML fragment to install inside the target node.",
        },
      },
      required: ["target", "html"],
      additionalProperties: false,
    },
  };
}

function createBrowserInsertHtmlTool() {
  return {
    type: "function",
    name: "browser_insert_html",
    description: "Insert HTML relative to a DOM target without replacing the entire node.",
    parameters: {
      type: "object",
      properties: {
        target: {
          description: "DOM target selector string or structured target object accepted by the browser runtime.",
        },
        html: {
          type: "string",
          description: "HTML fragment to insert.",
        },
        position: {
          type: "string",
          description: "Insertion position such as beforebegin, afterbegin, beforeend, or afterend.",
        },
      },
      required: ["target", "html"],
      additionalProperties: false,
    },
  };
}

function createShell1Tools() {
  if (!ENABLE_SHELL1_COMMAND_REGISTRY_TOOLS) {
    return [];
  }
  try {
    return buildAiPcShellToolBundle();
  } catch (_err) {
    return [];
  }
}

function getHostedComputerViewport() {
  const browser = globalThis.__trueosBrowser;
  if (browser && typeof browser.getViewport === "function") {
    try {
      const viewport = browser.getViewport();
      if (viewport && typeof viewport === "object") {
        const width = Math.max(
          1,
          Number(
            viewport.viewportWidth
            || viewport.width
            || viewport.contentWidth
            || 1024,
          ) | 0,
        );
        const height = Math.max(
          1,
          Number(
            viewport.viewportHeight
            || viewport.height
            || viewport.contentHeight
            || 768,
          ) | 0,
        );
        return { width, height };
      }
    } catch (_err) {}
  }
  return { width: 1024, height: 768 };
}

function isPreviewComputerToolType(toolType) {
  return String(toolType || "").trim().toLowerCase() === "computer_use_preview";
}

function createHostedComputerTool() {
  if (!isPreviewComputerToolType(OPENAI_COMPUTER_TOOL_TYPE)) {
    return {
      type: OPENAI_COMPUTER_TOOL_TYPE,
    };
  }
  const viewport = getHostedComputerViewport();
  return {
    type: OPENAI_COMPUTER_TOOL_TYPE,
    display_width: viewport.width,
    display_height: viewport.height,
    environment: OPENAI_COMPUTER_ENVIRONMENT,
  };
}

function buildComputerOnlyTools() {
  return [createHostedComputerTool()];
}

function createEnterComputerModeTool() {
  return {
    type: "function",
    name: "enter_computer_mode",
    description: "Switch into computer mode for visible UI control and screenshots. Use this after a little shell/search/tool reasoning when the task clearly requires interacting with the live UI.",
    parameters: {
      type: "object",
      properties: {
        reason: {
          type: "string",
          description: "Short reason why visible UI interaction is needed now.",
        },
      },
      additionalProperties: false,
    },
  };
}

function createExitComputerModeTool() {
  return {
    type: "function",
    name: "exit_computer_mode",
    description: "Leave computer mode and resume normal shell/search/function tools when visible UI control is no longer needed.",
    parameters: {
      type: "object",
      properties: {
        reason: {
          type: "string",
          description: "Short reason why UI control is complete or no longer needed.",
        },
      },
      additionalProperties: false,
    },
  };
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
      name: "driverdev_get_hid_protocol",
      description: "Read the current HID protocol for an interface (typically 0=boot, 1=report).",
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
      name: "driverdev_set_hid_protocol",
      description: "Send HID SET_PROTOCOL to switch an interface between boot/report protocol.",
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
          protocol: {
            type: "integer",
            description: "Protocol value to request. Common values are 0=boot and 1=report.",
            minimum: 0,
            maximum: 255,
          },
        },
        required: ["handle", "protocol"],
        additionalProperties: false,
      },
    },
    {
      type: "function",
      name: "driverdev_get_hid_idle",
      description: "Read HID idle duration (in 4ms units) for a report ID.",
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
        },
        required: ["handle"],
        additionalProperties: false,
      },
    },
    {
      type: "function",
      name: "driverdev_set_hid_idle",
      description: "Send HID SET_IDLE with duration expressed in 4ms units.",
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
          duration4ms: {
            type: "integer",
            description: "Idle duration in 4ms units. 0 commonly means send only on change.",
            minimum: 0,
            maximum: 255,
          },
        },
        required: ["handle", "duration4ms"],
        additionalProperties: false,
      },
    },
    {
      type: "function",
      name: "driverdev_get_hid_report_hex",
      description: "Send a HID class GET_REPORT request and return lower-case hex bytes.",
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
          length: {
            type: "integer",
            description: "Maximum report bytes to request.",
            minimum: 1,
            maximum: 256,
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

function buildTools(entry, options = null) {
  const cfg = options && typeof options === "object" ? options : {};
  const computerModeEligible = cfg.computerModeEligible === true;
  const computerModeActive = cfg.computerModeActive === true;
  const allowExitComputerMode = cfg.allowExitComputerMode !== false;
  const domMutationRequest = isDomMutationRequestText(entry && entry.text);
  const normalToolCallCount = Number.isFinite(cfg.normalToolCallCount)
    ? Math.max(0, Math.floor(cfg.normalToolCallCount))
    : 0;

  if (computerModeActive) {
    const tools = [...buildComputerOnlyTools()];
    if (allowExitComputerMode) {
      tools.push(createExitComputerModeTool());
    }
    return decorateResponseTools(
      tools,
      { deferLoading: false },
    );
  }

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
  tools.push(createReadBrowserContextTool());
  tools.push(createBrowserSetBodyHtmlTool());
  tools.push(createBrowserSetNodeHtmlTool());
  tools.push(createBrowserInsertHtmlTool());
  if (!domMutationRequest) {
    tools.push(...createShell1Tools());
  }
  tools.push(...createDriverDevTools());
  if (
    computerModeEligible
    && normalToolCallCount >= DEFAULT_COMPUTER_HANDOFF_AFTER_TOOL_CALLS
  ) {
    tools.push(createEnterComputerModeTool());
  }
  tools.push(createAskUserTool());
  return decorateResponseTools(tools, {
    deferLoading: DEFAULT_TOOL_SEARCH,
  });
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

function clampInteger(value, fallback, minValue, maxValue) {
  const num = Number(value);
  if (!Number.isFinite(num)) {
    return fallback;
  }
  const bounded = Math.floor(num);
  return Math.max(minValue, Math.min(maxValue, bounded));
}

function trimString(value, maxChars) {
  const text = typeof value === "string" ? value : String(value == null ? "" : value);
  if (text.length <= maxChars) {
    return { text, truncated: false, totalChars: text.length };
  }
  return {
    text: text.slice(0, maxChars),
    truncated: true,
    totalChars: text.length,
  };
}

function trimDomSnapshot(snapshot, maxNodes) {
  if (!snapshot || typeof snapshot !== "object") {
    return {
      snapshot,
      truncated: false,
      totalNodes: 0,
    };
  }
  const nodes = Array.isArray(snapshot.nodes) ? snapshot.nodes : null;
  if (!nodes) {
    return {
      snapshot,
      truncated: false,
      totalNodes: 0,
    };
  }
  const totalNodes = nodes.length;
  if (totalNodes <= maxNodes) {
    return {
      snapshot,
      truncated: false,
      totalNodes,
    };
  }
  return {
    snapshot: {
      ...snapshot,
      nodes: nodes.slice(0, maxNodes),
    },
    truncated: true,
    totalNodes,
  };
}

async function readBrowserContext(options = null) {
  const runtime = bindHostRuntime();
  const browser = runtime && runtime.browser && typeof runtime.browser === "object"
    ? runtime.browser
    : null;
  if (!browser) {
    throw new Error("browser context bridge is not available");
  }

  const source = options && typeof options === "object" ? options : {};
  const includeHtml = source.includeHtml === true;
  const includeDomSnapshot = source.includeDomSnapshot === true;
  const maxTextRows = clampInteger(
    source.maxTextRows,
    DEFAULT_BROWSER_CONTEXT_MAX_TEXT_ROWS,
    1,
    256,
  );
  const maxHtmlChars = clampInteger(
    source.maxHtmlChars,
    DEFAULT_BROWSER_CONTEXT_MAX_HTML_CHARS,
    64,
    24000,
  );
  const maxDomNodes = clampInteger(
    source.maxDomNodes,
    DEFAULT_BROWSER_CONTEXT_MAX_DOM_NODES,
    1,
    512,
  );

  const result = {
    browserTarget: getConnectedBrowser(),
    windowInfo: null,
    viewport: null,
    textRows: [],
    textRowsMeta: {
      totalRows: 0,
      returnedRows: 0,
      truncated: false,
    },
  };

  if (typeof browser.getWindowInfo === "function") {
    try {
      result.windowInfo = await browser.getWindowInfo();
    } catch (err) {
      result.windowInfoError = String(err && err.message ? err.message : err);
    }
  }
  if (typeof browser.getViewport === "function") {
    try {
      result.viewport = await browser.getViewport();
    } catch (err) {
      result.viewportError = String(err && err.message ? err.message : err);
    }
  }
  if (typeof browser.getTextRows === "function") {
    try {
      const rowsRaw = await browser.getTextRows();
      const rows = Array.isArray(rowsRaw)
        ? rowsRaw.map((row) => String(row == null ? "" : row))
        : [];
      result.textRowsMeta.totalRows = rows.length;
      result.textRows = rows.slice(0, maxTextRows);
      result.textRowsMeta.returnedRows = result.textRows.length;
      result.textRowsMeta.truncated = rows.length > result.textRows.length;
    } catch (err) {
      result.textRowsError = String(err && err.message ? err.message : err);
    }
  }

  if (includeHtml && typeof browser.getHtml === "function") {
    try {
      const htmlRaw = await browser.getHtml();
      const trimmed = trimString(htmlRaw, maxHtmlChars);
      result.html = trimmed.text;
      result.htmlMeta = {
        totalChars: trimmed.totalChars,
        returnedChars: trimmed.text.length,
        truncated: trimmed.truncated,
      };
    } catch (err) {
      result.htmlError = String(err && err.message ? err.message : err);
    }
  }

  if (includeDomSnapshot && typeof browser.getDomSnapshot === "function") {
    try {
      const domRaw = await browser.getDomSnapshot();
      const trimmed = trimDomSnapshot(domRaw, maxDomNodes);
      result.domSnapshot = trimmed.snapshot;
      result.domSnapshotMeta = {
        totalNodes: trimmed.totalNodes,
        returnedNodes: Array.isArray(trimmed.snapshot && trimmed.snapshot.nodes)
          ? trimmed.snapshot.nodes.length
          : 0,
        truncated: trimmed.truncated,
      };
    } catch (err) {
      result.domSnapshotError = String(err && err.message ? err.message : err);
    }
  }

  return result;
}

async function browserSetBodyHtml(html) {
  const runtime = bindHostRuntime();
  const browser = runtime && runtime.browser && typeof runtime.browser === "object"
    ? runtime.browser
    : null;
  if (!browser || typeof browser.setBodyHtml !== "function") {
    throw new Error("browser body html bridge is not available");
  }
  return await browser.setBodyHtml(String(html == null ? "" : html));
}

async function browserSetNodeHtml(target, html) {
  const runtime = bindHostRuntime();
  const browser = runtime && runtime.browser && typeof runtime.browser === "object"
    ? runtime.browser
    : null;
  if (!browser || typeof browser.setNodeHtml !== "function") {
    throw new Error("browser node html bridge is not available");
  }
  return await browser.setNodeHtml(target, String(html == null ? "" : html));
}

async function browserInsertHtml(target, html, position = "beforeend") {
  const runtime = bindHostRuntime();
  const browser = runtime && runtime.browser && typeof runtime.browser === "object"
    ? runtime.browser
    : null;
  if (!browser || typeof browser.insertHtml !== "function") {
    throw new Error("browser insert html bridge is not available");
  }
  return await browser.insertHtml(target, String(html == null ? "" : html), position);
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

function isHostedComputerTool(tool) {
  if (!tool || typeof tool !== "object") {
    return false;
  }
  return tool.type === "computer" || tool.type === "computer_use_preview";
}

function normalizeComputerActionType(action) {
  return String(
    action && (
      action.type
      || action.action
      || action.kind
      || action.name
      || ""
    ),
  ).trim().toLowerCase();
}

function normalizePoint(value) {
  const source = value && typeof value === "object" ? value : null;
  if (!source) {
    return null;
  }
  const x = Number(source.x);
  const y = Number(source.y);
  if (!Number.isFinite(x) || !Number.isFinite(y)) {
    return null;
  }
  return {
    x: Math.round(x),
    y: Math.round(y),
  };
}

function extractComputerActionPoint(action) {
  return normalizePoint(action)
    || normalizePoint(action && action.position)
    || normalizePoint(action && action.target)
    || normalizePoint(action && action.to)
    || normalizePoint(action && action.end);
}

function extractComputerActionPath(action) {
  const path = [];
  const start = normalizePoint(action && action.start);
  if (start) {
    path.push(start);
  }
  const entries = Array.isArray(action && action.path) ? action.path : [];
  for (const entry of entries) {
    const point = normalizePoint(entry);
    if (point) {
      path.push(point);
    }
  }
  const end = normalizePoint(action && (action.end || action.to || action.target));
  if (end) {
    const last = path.length > 0 ? path[path.length - 1] : null;
    if (!last || last.x !== end.x || last.y !== end.y) {
      path.push(end);
    }
  }
  return path;
}

function normalizeComputerButtonMask(action) {
  const raw = action && (
    action.buttonMask
    || action.button
    || action.mouse_button
    || action.mouseButton
    || 1
  );
  if (typeof raw === "string") {
    const value = raw.trim().toLowerCase();
    if (value === "right") return 2;
    if (value === "middle") return 4;
    return 1;
  }
  return Math.max(1, Number(raw || 1) | 0) >>> 0;
}

function normalizeComputerKeyName(value) {
  const raw = String(value == null ? "" : value).trim();
  if (!raw) {
    return "";
  }
  const lower = raw.toLowerCase();
  if (lower === "cmd") return "Meta";
  if (lower === "command") return "Meta";
  if (lower === "ctrl") return "Ctrl";
  if (lower === "control") return "Ctrl";
  if (lower === "alt") return "Alt";
  if (lower === "option") return "Alt";
  if (lower === "shift") return "Shift";
  if (lower === "meta") return "Meta";
  if (lower === "enter" || lower === "return") return "Enter";
  if (lower === "esc" || lower === "escape") return "Escape";
  if (lower === "space" || lower === "spacebar") return "Space";
  if (lower === "pgup" || lower === "pageup") return "PageUp";
  if (lower === "pgdn" || lower === "pagedown") return "PageDown";
  if (lower === "del") return "Delete";
  if (raw.length === 1) {
    return raw;
  }
  return raw.charAt(0).toUpperCase() + raw.slice(1);
}

function isModifierKeyName(value) {
  return value === "Ctrl" || value === "Alt" || value === "Shift" || value === "Meta";
}

async function performComputerScroll(runtime, action) {
  const browser = runtime && runtime.browser && typeof runtime.browser === "object"
    ? runtime.browser
    : null;
  if (!browser || typeof browser.getViewport !== "function" || typeof browser.setScroll !== "function") {
    throw new Error("browser scroll bridge unavailable for hosted computer action");
  }
  const viewport = await browser.getViewport();
  const currentX = Math.max(0, Number(viewport && viewport.scrollX || 0) | 0);
  const currentY = Math.max(0, Number(viewport && viewport.scrollY || 0) | 0);
  const dx = Math.round(Number(
    action && (
      action.deltaX
      || action.dx
      || action.scrollX
      || action.x
      || 0
    ),
  ) || 0);
  const dy = Math.round(Number(
    action && (
      action.deltaY
      || action.dy
      || action.scrollY
      || action.y
      || action.amount
      || 0
    ),
  ) || 0);
  return await browser.setScroll(
    Math.max(0, currentX + dx),
    Math.max(0, currentY + dy),
  );
}

async function performComputerKeypress(runtime, action) {
  const computer = runtime && runtime.computer && typeof runtime.computer === "object"
    ? runtime.computer
    : null;
  if (!computer || typeof computer.pressKey !== "function") {
    throw new Error("computer keyboard bridge unavailable");
  }
  const keys = [];
  if (Array.isArray(action && action.keys)) {
    for (const entry of action.keys) {
      const key = normalizeComputerKeyName(entry);
      if (key) {
        keys.push(key);
      }
    }
  } else {
    const key = normalizeComputerKeyName(action && (action.key || action.text));
    if (key) {
      keys.push(key);
    }
  }
  if (keys.length === 0) {
    return false;
  }
  const explicitModifiers = Array.isArray(action && action.modifiers)
    ? action.modifiers.map(normalizeComputerKeyName).filter(Boolean)
    : [];
  const trailingKey = keys[keys.length - 1];
  const implicitModifiers = keys.slice(0, -1).filter(isModifierKeyName);
  const modifiers = [...explicitModifiers, ...implicitModifiers];
  return await computer.pressKey(trailingKey, { modifiers });
}

async function performComputerDrag(runtime, action) {
  const computer = runtime && runtime.computer && typeof runtime.computer === "object"
    ? runtime.computer
    : null;
  if (!computer || typeof computer.moveCursor !== "function") {
    throw new Error("computer pointer bridge unavailable for drag");
  }
  const path = extractComputerActionPath(action);
  if (path.length === 0) {
    const point = extractComputerActionPoint(action);
    if (!point) {
      return false;
    }
    path.push(point);
  }
  const buttonMask = normalizeComputerButtonMask(action);
  const first = path[0];
  await computer.moveCursor({
    x: first.x,
    y: first.y,
  });
  await waitMs(20);
  await computer.moveCursor({
    x: first.x,
    y: first.y,
    buttonsDown: buttonMask,
    flags: INPUT_CURSOR_FLAG_ABSOLUTE | INPUT_CURSOR_FLAG_BUTTONS_CHANGED,
  });
  for (let i = 1; i < path.length; i += 1) {
    const point = path[i];
    await computer.moveCursor({
      x: point.x,
      y: point.y,
      buttonsDown: buttonMask,
    });
    await waitMs(16);
  }
  const last = path[path.length - 1];
  return await computer.moveCursor({
    x: last.x,
    y: last.y,
    buttonsDown: 0,
    flags: INPUT_CURSOR_FLAG_ABSOLUTE | INPUT_CURSOR_FLAG_BUTTONS_CHANGED,
  });
}

async function executeHostedComputerAction(runtime, action) {
  const type = normalizeComputerActionType(action);
  const point = extractComputerActionPoint(action);
  const computer = runtime && runtime.computer && typeof runtime.computer === "object"
    ? runtime.computer
    : null;
  switch (type) {
    case "":
    case "wait":
    case "pause": {
      const durationMs = Math.max(
        0,
        Number(action && (action.ms || action.durationMs || action.duration || 1000)) | 0,
      );
      await waitMs(durationMs);
      return true;
    }
    case "move":
    case "mousemove":
    case "mouse_move":
    case "move_cursor": {
      if (!point || !computer || typeof computer.moveCursor !== "function") {
        return false;
      }
      return await computer.moveCursor(point);
    }
    case "click":
    case "left_click":
    case "single_click": {
      if (!point || !computer || typeof computer.click !== "function") {
        return false;
      }
      return await computer.click({
        ...point,
        buttonMask: normalizeComputerButtonMask(action),
      });
    }
    case "double_click":
    case "doubleclick": {
      if (!point || !computer || typeof computer.click !== "function") {
        return false;
      }
      await computer.click({
        ...point,
        buttonMask: normalizeComputerButtonMask(action),
      });
      await waitMs(60);
      return await computer.click({
        ...point,
        buttonMask: normalizeComputerButtonMask(action),
      });
    }
    case "drag":
    case "drag_to":
    case "dragto":
      return await performComputerDrag(runtime, action);
    case "scroll":
      return await performComputerScroll(runtime, action);
    case "type":
    case "text":
    case "input_text": {
      if (!computer || typeof computer.typeText !== "function") {
        return false;
      }
      const text = String(action && (action.text || action.value || "") || "");
      return await computer.typeText(text);
    }
    case "keypress":
    case "key_press":
    case "press_key":
    case "hotkey":
    case "shortcut":
      return await performComputerKeypress(runtime, action);
    default:
      return false;
  }
}

async function createHostedComputerCallOutput(item) {
  const runtime = bindHostRuntime();
  const actions = Array.isArray(item && item.actions)
    ? item.actions.filter((entry) => entry && typeof entry === "object")
    : (item && typeof item.action === "object" ? [item.action] : []);
  for (const action of actions) {
    try {
      const actionType = normalizeComputerActionType(action) || "wait";
      if (actionType === "screenshot") {
        aiDiag("computer_call action=screenshot returning screenshot output");
        continue;
      }
      const handled = await executeHostedComputerAction(runtime, action);
      aiDiag(`computer_call action=${actionType} handled=${handled ? 1 : 0}`);
    } catch (err) {
      aiDiag(`computer_call action failed: ${String(err && err.message ? err.message : err)}`);
    }
  }
  let screenshot = "";
  try {
    screenshot = runtime.computer && typeof runtime.computer.captureScreenshot === "function"
      ? await runtime.computer.captureScreenshot()
      : "";
  } catch (err) {
    aiDiag(`computer screenshot unavailable: ${String(err && err.message ? err.message : err)}`);
  }
  const output = {
    type: "computer_screenshot",
    image_url: typeof screenshot === "string" && screenshot
      ? screenshot
      : FALLBACK_SCREENSHOT_DATA_URL,
  };
  const result = {
    type: "computer_call_output",
    call_id: item.call_id,
    output,
  };
  const pendingSafetyChecks = Array.isArray(item && item.pending_safety_checks)
    ? item.pending_safety_checks.filter((entry) => entry && typeof entry === "object")
    : [];
  if (pendingSafetyChecks.length > 0) {
    result.acknowledged_safety_checks = pendingSafetyChecks;
  }
  return result;
}

async function runTurn(client, entry, previousResponseId, maxSteps = DEFAULT_MAX_STEPS, model = DEFAULT_RESPONSE_MODEL) {
  const hardComputerMode = entry.computerUse === true;
  const computerModeEligible = hardComputerMode;
  const expectsDomMutation = isDomMutationRequestText(entry && entry.text);
  let computerModeActive = hardComputerMode;
  let computerActionStarted = false;
  let hardModeRetryUsed = false;
  let normalToolCallCount = 0;
  let browserMutationExecuted = false;
  let nextInput = [
    {
      role: "user",
      content: entry.text,
    },
  ];

  for (let i = 0; i < maxSteps; i += 1) {
    const requireComputerToolNow = hardComputerMode && !computerActionStarted;
    const tools = buildTools(entry, {
      computerModeEligible,
      computerModeActive,
      normalToolCallCount,
      allowExitComputerMode: !hardComputerMode || computerActionStarted,
    });
    const turnModel = computerModeActive ? OPENAI_COMPUTER_MODEL : model;
    const hasHostedComputerTool = tools.some(isHostedComputerTool);
    const needsPreviewComputerConfig = computerModeActive
      && hasHostedComputerTool
      && isPreviewComputerToolType(OPENAI_COMPUTER_TOOL_TYPE);
    const turnInstructions = requireComputerToolNow
      ? `${AI_PC_INSTRUCTIONS} ${AI_PC_HARD_MODE_RETRY_PROMPT}`
      : AI_PC_INSTRUCTIONS;
    const request = buildResponsesRequest({
      model: turnModel,
      instructions: turnInstructions,
      tools,
      input: nextInput,
      previousResponseId,
      parallelToolCalls: DEFAULT_PARALLEL_TOOL_CALLS,
      toolChoice: requireComputerToolNow ? "required" : undefined,
      truncation: needsPreviewComputerConfig ? "auto" : "",
      reasoningEffort: needsPreviewComputerConfig
        ? OPENAI_COMPUTER_REASONING_EFFORT
        : DEFAULT_REASONING_EFFORT,
    });

    logScreenshotUploadSizes(request.input);

    let resp = null;
    let streamedAssistantText = false;
    const responseStream = requireComputerToolNow ? null : createResponseStream(client, request);
    if (responseStream) {
      try {
        for await (const event of responseStream) {
          if (!event || event.type !== "response.output_text.delta") {
            continue;
          }
          if (typeof event.delta !== "string" || !event.delta) {
            continue;
          }
          streamedAssistantText = true;
          shellPrint(event.delta);
        }
        resp = await responseStream.finalResponse();
        if (streamedAssistantText) {
          shellFlush();
        }
      } catch (err) {
        if (streamedAssistantText) {
          throw err;
        }
        aiDiag(`responses.stream failed, falling back to create: ${String(err && err.message ? err.message : err)}`);
      }
    }
    if (!resp) {
      resp = await createResponse(client, request);
    }
    previousResponseId = resp.id;
    const responseItems = getResponseOutputItems(resp);
    aiDiag(
      `response id=${String(resp && resp.id ? resp.id : "")} status=${String(resp && resp.status ? resp.status : "")} output_items=${responseItems.length} output_text_chars=${getResponseOutputText(resp).length} item_types=${summarizeResponseItems(responseItems)}`,
    );

    let hadToolCall = false;
    let emittedAssistantText = streamedAssistantText;
    const toolOutputs = [];

    for (const item of responseItems) {
      if (item && item.type === "computer_call") {
        hadToolCall = true;
        computerActionStarted = true;
        toolOutputs.push(await createHostedComputerCallOutput(item));
        continue;
      }
      if (item && item.type === "reasoning") {
        const reasoningLine = formatReasoningProgress(item);
        aiDiag(reasoningLine);
        if (extractReasoningText(item)) {
          shellEmitLine(`ai: ${reasoningLine}`);
        }
        continue;
      }
      if (isHostedProgressResponseItem(item)) {
        hadToolCall = true;
        aiDiag(`hosted tool progress item type=${String(item.type)}`);
        continue;
      }

      const shell1Command = item.type === "function_call"
        ? findAiPcShellCommandByToolName(item.name)
        : null;
      if (item.type === "function_call" && item.name === "enter_computer_mode") {
        hadToolCall = true;
        computerModeActive = true;
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: "computer mode enabled; visible UI control and screenshots are now available.",
        });
      } else if (item.type === "function_call" && item.name === "exit_computer_mode") {
        hadToolCall = true;
        computerModeActive = false;
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: "computer mode disabled; normal shell/search/function tools are available again.",
        });
      } else if (shell1Command) {
        hadToolCall = true;
        normalToolCallCount += 1;
        let output = "submitted shell1 command.";
        try {
          const parsed = JSON.parse(item.arguments || "{}");
          const commandLine = buildShell1CommandLine(shell1Command, parsed);
          const baselineLines = shell1HistoryTotalLines();
          const submitted = submitShell1Input(commandLine);
          const shellOutput = await captureShell1OutputSince(baselineLines);
          shellEmitLine(`ai shell1: ${commandLine}`);
          if (shellOutput) {
            shellEmitBlock(shellOutput);
          } else {
            shellEmitLine("ai shell1: (no new shell1 history lines were captured)");
          }
          output = shellOutput
            ? `submitted shell1 command: ${commandLine} (${submitted} bytes)\n\nshell1 output:\n${shellOutput}`
            : `submitted shell1 command: ${commandLine} (${submitted} bytes). No new shell1 history lines were captured yet.`;
        } catch (err) {
          shellEmitLine(`ai shell1: command failed: ${String(err && err.message ? err.message : err)}`);
          output = `submitted shell1 command, but shell capture failed: ${String(err && err.message ? err.message : err)}`;
        }
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output,
        });
      } else if (item.type === "function_call" && item.name === "read_trueosfs_tree") {
        hadToolCall = true;
        normalToolCallCount += 1;
        const parsed = JSON.parse(item.arguments || "{}");
        const html = await readTrueosFsTree(parsed.maxEntries);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: typeof html === "string" ? html : "",
        });
      } else if (item.type === "function_call" && item.name === "read_browser_context") {
        hadToolCall = true;
        normalToolCallCount += 1;
        const parsed = JSON.parse(item.arguments || "{}");
        const context = await readBrowserContext(parsed);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify(context),
        });
      } else if (item.type === "function_call" && item.name === "browser_set_body_html") {
        hadToolCall = true;
        normalToolCallCount += 1;
        const parsed = JSON.parse(item.arguments || "{}");
        const ok = await browserSetBodyHtml(parsed.html);
        browserMutationExecuted = ok === true;
        aiDiag(`browser_set_body_html ok=${browserMutationExecuted ? 1 : 0}`);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify({
            ok: ok === true,
            operation: "setBodyHtml",
          }),
        });
      } else if (item.type === "function_call" && item.name === "browser_set_node_html") {
        hadToolCall = true;
        normalToolCallCount += 1;
        const parsed = JSON.parse(item.arguments || "{}");
        const ok = await browserSetNodeHtml(parsed.target, parsed.html);
        browserMutationExecuted = browserMutationExecuted || ok === true;
        aiDiag(`browser_set_node_html ok=${ok === true ? 1 : 0}`);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify({
            ok: ok === true,
            operation: "setNodeHtml",
            target: parsed.target,
          }),
        });
      } else if (item.type === "function_call" && item.name === "browser_insert_html") {
        hadToolCall = true;
        normalToolCallCount += 1;
        const parsed = JSON.parse(item.arguments || "{}");
        const ok = await browserInsertHtml(parsed.target, parsed.html, parsed.position);
        browserMutationExecuted = browserMutationExecuted || ok === true;
        aiDiag(`browser_insert_html ok=${ok === true ? 1 : 0}`);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify({
            ok: ok === true,
            operation: "insertHtml",
            target: parsed.target,
            position: typeof parsed.position === "string" && parsed.position
              ? parsed.position
              : "beforeend",
          }),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_list_devices") {
        hadToolCall = true;
        normalToolCallCount += 1;
        const devices = driverdev.listDevices();
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify(devices),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_get_device_descriptor") {
        hadToolCall = true;
        normalToolCallCount += 1;
        const parsed = JSON.parse(item.arguments || "{}");
        const desc = driverdev.getDeviceDescriptor(parsed.handle);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify(desc),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_get_descriptor_hex") {
        hadToolCall = true;
        normalToolCallCount += 1;
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
        normalToolCallCount += 1;
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
        normalToolCallCount += 1;
        const parsed = JSON.parse(item.arguments || "{}");
        const rc = driverdev.portReset(parsed.controllerId, parsed.portIdx);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify({ rc }),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_read_transfer_event") {
        hadToolCall = true;
        normalToolCallCount += 1;
        const parsed = JSON.parse(item.arguments || "{}");
        const event = driverdev.readTransferEvent(parsed.handle, parsed.epTarget);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: toJsonStringOrNull(event),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_get_hid_report_descriptor_hex") {
        hadToolCall = true;
        normalToolCallCount += 1;
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
        normalToolCallCount += 1;
        const parsed = JSON.parse(item.arguments || "{}");
        const interfaceNumber = Number.isInteger(parsed.interfaceNumber) ? parsed.interfaceNumber : 0;
        const result = driverdev.identifyHidDevice(parsed.handle, interfaceNumber);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: toJsonStringOrNull(result),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_get_hid_protocol") {
        hadToolCall = true;
        normalToolCallCount += 1;
        const parsed = JSON.parse(item.arguments || "{}");
        const interfaceNumber = Number.isInteger(parsed.interfaceNumber) ? parsed.interfaceNumber : 0;
        const protocol = driverdev.getHidProtocol(parsed.handle, interfaceNumber);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify({ protocol, interfaceNumber }),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_set_hid_protocol") {
        hadToolCall = true;
        normalToolCallCount += 1;
        const parsed = JSON.parse(item.arguments || "{}");
        const interfaceNumber = Number.isInteger(parsed.interfaceNumber) ? parsed.interfaceNumber : 0;
        const protocol = Number.isInteger(parsed.protocol) ? parsed.protocol : 0;
        const rc = driverdev.setHidProtocol(parsed.handle, interfaceNumber, protocol);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify({ rc, interfaceNumber, protocol }),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_get_hid_idle") {
        hadToolCall = true;
        normalToolCallCount += 1;
        const parsed = JSON.parse(item.arguments || "{}");
        const interfaceNumber = Number.isInteger(parsed.interfaceNumber) ? parsed.interfaceNumber : 0;
        const reportId = Number.isInteger(parsed.reportId) ? parsed.reportId : 0;
        const duration4ms = driverdev.getHidIdle(parsed.handle, interfaceNumber, reportId);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify({ duration4ms, interfaceNumber, reportId }),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_set_hid_idle") {
        hadToolCall = true;
        normalToolCallCount += 1;
        const parsed = JSON.parse(item.arguments || "{}");
        const interfaceNumber = Number.isInteger(parsed.interfaceNumber) ? parsed.interfaceNumber : 0;
        const reportId = Number.isInteger(parsed.reportId) ? parsed.reportId : 0;
        const duration4ms = Number.isInteger(parsed.duration4ms) ? parsed.duration4ms : 0;
        const rc = driverdev.setHidIdle(parsed.handle, interfaceNumber, reportId, duration4ms);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify({ rc, interfaceNumber, reportId, duration4ms }),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_get_hid_report_hex") {
        hadToolCall = true;
        normalToolCallCount += 1;
        const parsed = JSON.parse(item.arguments || "{}");
        const interfaceNumber = Number.isInteger(parsed.interfaceNumber) ? parsed.interfaceNumber : 0;
        const reportType = Number.isInteger(parsed.reportType) ? parsed.reportType : 1;
        const reportId = Number.isInteger(parsed.reportId) ? parsed.reportId : 0;
        const length = Number.isInteger(parsed.length) ? parsed.length : 64;
        const bytes = driverdev.getHidReport(parsed.handle, interfaceNumber, reportType, reportId, length);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: JSON.stringify({
            hex: bytesToHex(bytes),
            byteLength: bytes ? bytes.length : 0,
            interfaceNumber,
            reportType,
            reportId,
          }),
        });
      } else if (item.type === "function_call" && item.name === "driverdev_set_hid_report_hex") {
        hadToolCall = true;
        normalToolCallCount += 1;
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
        normalToolCallCount += 1;
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
        normalToolCallCount += 1;
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
        normalToolCallCount += 1;
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
        normalToolCallCount += 1;
        const parsed = JSON.parse(item.arguments || "{}");
        const question = parsed.question || "Please provide more information.";
        console.log(`MODEL QUESTION: ${question}`);
        shellEmitLine(`ai: ${question}`);
        const answerEntry = normalizeAiInputEntry(await awaitHostInput(question));
        const answer = answerEntry ? answerEntry.text : "";
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: answer,
        });
      } else if (item.type === "message") {
        if (requireComputerToolNow) {
          continue;
        }
        if (streamedAssistantText) {
          continue;
        }
        const text = extractMessageText(item);
        if (typeof text === "string" && text) {
          if (expectsDomMutation && !browserMutationExecuted) {
            shellEmitLine("ai: no DOM mutation tool was executed in this turn, so the page was not actually changed.");
            emittedAssistantText = true;
          } else {
            shellPrint(text);
            shellFlush();
            emittedAssistantText = true;
          }
        }
      } else {
        aiDiag(`unhandled response item type=${String(item && item.type ? item.type : "unknown")}`);
      }
    }

    if (expectsDomMutation && streamedAssistantText && !browserMutationExecuted) {
      shellEmitLine("ai: no DOM mutation tool was executed in this turn, so the page was not actually changed.");
      shellFlush();
      emittedAssistantText = true;
    }

    if (!hadToolCall && !emittedAssistantText && !requireComputerToolNow) {
      const fallbackText = getResponseOutputText(resp);
      if (typeof fallbackText === "string" && fallbackText) {
        if (expectsDomMutation && !browserMutationExecuted) {
          shellEmitLine("ai: no DOM mutation tool was executed in this turn, so the page was not actually changed.");
        } else {
          shellPrint(fallbackText);
          shellFlush();
        }
        emittedAssistantText = true;
      }
    }

    if (!hadToolCall && requireComputerToolNow) {
      if (!hardModeRetryUsed) {
        hardModeRetryUsed = true;
        aiDiag("ai-pc hard mode received no computer action; retrying with stricter prompt");
        nextInput = [
          {
            role: "user",
            content: AI_PC_HARD_MODE_RETRY_PROMPT,
          },
        ];
        continue;
      }
      shellEmitLine("ai-pc: model returned no computer action.");
      shellFlush();
      aiDiag("ai-pc hard mode failed to obtain a computer action after retry");
      return previousResponseId;
    }

    if (!hadToolCall && !emittedAssistantText) {
      shellEmitLine("ai: response completed but returned no visible text or tool calls.");
      aiDiag("response completed with no visible text or tool calls");
      shellFlush();
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
  aiDiag("startAiPc: begin");
  if (hasWorkerParentPort()) {
    aiDiag("startAiPc: enabling worker RPC");
    ensureWorkerRpcReady();
  }
  if (globalThis.__trueosAiPcStarted) {
    aiDiag("startAiPc: already started");
    return false;
  }
  globalThis.__trueosAiPcStarted = true;
  try {
    aiDiag("startAiPc: ensuring default browser connection");
    await ensureDefaultBrowserConnection();
    aiDiag("startAiPc: creating OpenAI client");
    const client = createOpenAiClient();
    let previousResponseId = null;
    aiDiag("startAiPc: entering input loop");
    while (true) {
      const entry = await waitForNextInput("");
      if (!entry) {
        continue;
      }
      setShellPrintTargetMask(entry.shellTargetMask);
      aiDiag(`startAiPc: received input newConversation=${entry.newConversation ? 1 : 0} text_len=${entry.text ? entry.text.length : 0}`);
      if (entry.newConversation) {
        previousResponseId = null;
      }
      previousResponseId = await runTurn(client, entry, previousResponseId);
    }
  } finally {
    globalThis.__trueosAiPcStarted = false;
    aiDiag("startAiPc: end");
  }
}

export async function startAiPcWorker() {
  ensureWorkerRpcReady();
  return await startAiPc();
}

if (isMainThread) {
  void startAiPc();
}
