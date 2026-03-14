import OpenAI from "openai";

export const DEFAULT_RESPONSE_MODEL = "gpt-5.4";
export const DEFAULT_REASONING_EFFORT = "low";
export const DEFAULT_PARALLEL_TOOL_CALLS = false;

export function createOpenAiClient(options = undefined) {
  return new OpenAI(options);
}

export function decorateResponseTools(tools, options = null) {
  const cfg = options && typeof options === "object" ? options : {};
  const strictFunctionTools = cfg.strictFunctionTools === true;
  const deferLoading = cfg.deferLoading !== false;
  if (!Array.isArray(tools)) {
    return [];
  }

  const out = [];
  for (const tool of tools) {
    if (!tool || typeof tool !== "object") {
      continue;
    }
    if (tool.type !== "function") {
      out.push(tool);
      continue;
    }

    const next = { ...tool };
    if (strictFunctionTools && next.strict !== true) {
      next.strict = true;
    }
    if (deferLoading && next.defer_loading !== true) {
      next.defer_loading = true;
    }
    out.push(next);
  }
  return out;
}

export function buildResponsesRequest(options) {
  const cfg = options && typeof options === "object" ? options : {};
  const request = {
    model: typeof cfg.model === "string" && cfg.model ? cfg.model : DEFAULT_RESPONSE_MODEL,
    input: Array.isArray(cfg.input) ? cfg.input : [],
    parallel_tool_calls: cfg.parallelToolCalls === undefined
      ? DEFAULT_PARALLEL_TOOL_CALLS
      : !!cfg.parallelToolCalls,
  };

  if (typeof cfg.instructions === "string" && cfg.instructions) {
    request.instructions = cfg.instructions;
  }
  if (Array.isArray(cfg.tools) && cfg.tools.length > 0) {
    request.tools = cfg.tools;
  }
  if (typeof cfg.previousResponseId === "string" && cfg.previousResponseId) {
    request.previous_response_id = cfg.previousResponseId;
  }

  const effort = typeof cfg.reasoningEffort === "string" && cfg.reasoningEffort
    ? cfg.reasoningEffort
    : DEFAULT_REASONING_EFFORT;
  if (effort) {
    request.reasoning = { effort };
  }

  return request;
}

export async function createResponse(client, request) {
  const response = await client.responses.create(request);
  if (typeof response?.id !== "string" || !response.id) {
    throw new Error("responses.create() did not return a response id");
  }
  return response;
}

export function getResponseOutputItems(response) {
  return Array.isArray(response && response.output) ? response.output : [];
}

export function getResponseOutputText(response) {
  if (response && typeof response.output_text === "string") {
    return response.output_text;
  }
  const chunks = [];
  const output = getResponseOutputItems(response);
  for (const item of output) {
    if (!item || item.type !== "message") {
      continue;
    }
    const content = Array.isArray(item.content) ? item.content : [];
    for (const part of content) {
      if (part && part.type === "output_text" && typeof part.text === "string") {
        chunks.push(part.text);
      }
    }
  }
  return chunks.join("");
}

export function getResponseFunctionCalls(response) {
  const output = getResponseOutputItems(response);
  return output.filter((item) => item && item.type === "function_call");
}
