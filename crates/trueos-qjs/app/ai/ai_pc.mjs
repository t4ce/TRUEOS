import OpenAI from "openai";

const DEFAULT_PROMPT = "Go to Hacker News, click on the most interesting link (be prepared to justify your choice), take a screenshot, and give me a critique of the visual layout.";
const DEFAULT_MAX_STEPS = 50;
const DEFAULT_MODEL = "gpt-5.4";

function getPcRuntime() {
  if (!globalThis.__trueosAiPcRuntime) {
    globalThis.__trueosAiPcRuntime = {
      browser: null,
      context: null,
      page: null,
      jsOutput: [],
    };
  }
  return globalThis.__trueosAiPcRuntime;
}

function bindHostRuntime() {
  const runtime = getPcRuntime();
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
  runtime.jsOutput.push({
    type: "input_image",
    image_url: `data:image/png;base64,${base64Image}`,
    detail: "original",
  });
}

async function askUserViaHost(question) {
  if (typeof globalThis.__trueosShell1Ask === "function") {
    return await globalThis.__trueosShell1Ask(question);
  }
  throw new Error("ask_user is not wired; host must expose globalThis.__trueosShell1Ask(question)");
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

async function main(prompt = DEFAULT_PROMPT, maxSteps = DEFAULT_MAX_STEPS, model = DEFAULT_MODEL) {
  const client = new OpenAI();
  const conversation = [];

  conversation.push({
    role: "user",
    content: prompt,
  });

  for (let i = 0; i < maxSteps; i += 1) {
    const resp = await client.responses.create({
      model,
      tools: [
        {
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
- display(base64_image_string): Use this to view a base64-encoded image.
- Do not write screenshots or image data to temporary files or disk just to pass them back. Keep image data in memory and send it directly to display().
- browser: read-only TRUEOS browser facade if available. Use methods like getHtml(), getTextRows(), getDomSnapshot(), getViewport(), paint(), setScroll(y).
- context: same object as browser for now.
- page: same object as browser for now.
`,
              },
            },
            required: ["code"],
            additionalProperties: false,
          },
        },
        {
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
        },
      ],
      input: conversation,
      reasoning: {
        effort: "low",
      },
    });

    conversation.push(...resp.output);

    let hadToolCall = false;
    let latestPhase = null;

    for (const item of resp.output) {
      if (item.type === "function_call" && item.name === "exec_js") {
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

        conversation.push({
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
        const answer = await askUserViaHost(question);
        conversation.push({
          type: "function_call_output",
          call_id: item.call_id,
          output: answer,
        });
      } else if (item.type === "message") {
        const content = Array.isArray(item.content) ? item.content[0] : item.content;
        console.log(content && content.text ? content.text : content);
        if ("phase" in item) {
          latestPhase = item.phase || null;
        }
      } else if (item.type === "output_item.done" && "phase" in item) {
        latestPhase = item.phase || null;
      }
    }

    if (!hadToolCall && latestPhase === "final_answer") {
      return;
    }
  }
}

export async function startAiPc(prompt = getHostPrompt()) {
  if (globalThis.__trueosAiPcStarted) {
    return false;
  }
  globalThis.__trueosAiPcStarted = true;
  try {
    await main(prompt);
    return true;
  } finally {
    globalThis.__trueosAiPcStarted = false;
  }
}

function getHostPrompt() {
  if (typeof globalThis.__trueosAiPcPrompt === "string" && globalThis.__trueosAiPcPrompt) {
    return globalThis.__trueosAiPcPrompt;
  }
  return DEFAULT_PROMPT;
}

void startAiPc(getHostPrompt());