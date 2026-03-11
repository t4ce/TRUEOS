import OpenAI from "openai";
import { isMainThread, parentPort } from "node:worker_threads";
import { buildAiPcShellToolBundle, findAiPcShellCommandByToolName } from "./ai_pc_cmd.mjs";

const DEFAULT_MAX_STEPS = 50;
const DEFAULT_MODEL = "gpt-5.4";
const WORKER_RPC_TIMEOUT_MS = 30000;
const AI_PC_TOOL_POLICY = [
  "Use shell1 function tools whenever the user is asking to run, open, launch, inspect, or control something that maps to a shell1 command.",
  "Use ask_user only when the request is genuinely ambiguous or a required argument is missing.",
  "Do not claim you cannot launch local apps or shell commands when a shell1 tool is available for the task.",
].join(" ");
const WORKER_BROWSER_METHODS = [
  "getApiContract",
  "listUnavailable",
  "getHtml",
  "getTextRows",
  "getDomSnapshot",
  "getViewport",
  "paint",
  "setScroll",
  "click",
  "navigate",
  "typeText",
  "pressKey",
  "captureScreenshot",
];

function getPcRuntime() {
  if (!globalThis.__trueosAiPcRuntime) {
    globalThis.__trueosAiPcRuntime = {
      browser: null,
      context: null,
      page: null,
      jsOutput: [],
      workerRpcSeq: 1,
      workerRpcPending: Object.create(null),
      workerRpcReady: false,
    };
  }
  return globalThis.__trueosAiPcRuntime;
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

function displayImage(base64Image) {
  const runtime = getPcRuntime();
  const imageUrl = typeof base64Image === "string" && base64Image.startsWith("data:image/")
    ? base64Image
    : `data:image/png;base64,${base64Image}`;
  runtime.jsOutput.push({
    type: "input_image",
    image_url: imageUrl,
    detail: "original",
  });
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
- console.log(x): Use this to read contents back to you. But be minimal: otherwise the output may be too long. Avoid using console.log() for large base64 payloads like screenshots or buffer. If you create an image or screenshot, pass the base64 string to display().
- display(base64_or_data_url): Use this to view a base64-encoded image or a full data URL.
- Do not write screenshots or image data to temporary files or disk just to pass them back. Keep image data in memory and send it directly to display().
- browser: TRUEOS browser facade. Call browser.getApiContract() first for the supported contract. Current live methods include getHtml(), getTextRows(), getDomSnapshot(), getViewport(), paint(), setScroll(y), click(...), navigate(...), pressKey(...), captureScreenshot(), and listUnavailable(). typeText() may still report not-yet-available.
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

function createShell1Tools() {
  try {
    return buildAiPcShellToolBundle();
  } catch (_err) {
    return [];
  }
}

function buildTools(entry) {
  const tools = [];
  if (entry.webSearch) {
    tools.push({ type: "web_search" });
  }
  tools.push(...createShell1Tools());
  if (entry.computerUse) {
    tools.push(createExecJsTool());
  }
  tools.push(createAskUserTool());
  return tools;
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

async function runTurn(client, entry, previousResponseId, maxSteps = DEFAULT_MAX_STEPS, model = DEFAULT_MODEL) {
  const tools = buildTools(entry);
  let nextInput = [
    {
      role: "system",
      content: AI_PC_TOOL_POLICY,
    },
    {
      role: "user",
      content: entry.text,
    },
  ];

  for (let i = 0; i < maxSteps; i += 1) {
    const request = {
      model,
      tools,
      input: nextInput,
      reasoning: {
        effort: "low",
      },
    };
    if (previousResponseId) {
      request.previous_response_id = previousResponseId;
    }

    const resp = await client.responses.create(request);
    if (typeof resp.id !== "string" || !resp.id) {
      throw new Error("responses.create() did not return a response id");
    }
    previousResponseId = resp.id;

    let hadToolCall = false;
    const toolOutputs = [];

    for (const item of resp.output) {
      const shell1Command = item.type === "function_call"
        ? findAiPcShellCommandByToolName(item.name)
        : null;
      if (shell1Command) {
        hadToolCall = true;
        const parsed = JSON.parse(item.arguments || "{}");
        const commandLine = buildShell1CommandLine(shell1Command, parsed);
        const submitted = submitShell1Input(commandLine);
        toolOutputs.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: `submitted shell1 command: ${commandLine} (${submitted} bytes). Output will appear in the shell1 terminal.`,
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
            console.log("JS IMAGE: [base64 string omitted]");
          }
        }
        console.log("=====");

        runtime.jsOutput.length = 0;
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
    const client = new OpenAI();
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