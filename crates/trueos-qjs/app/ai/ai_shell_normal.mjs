import { normalizeJsonFileTree } from './ai_file_adapter.mjs';
import {
  AI_TOOL_PROFILE_DRIVERDEV,
  AI_TOOL_PROFILE_INTELDEV,
  AI_TOOL_PROFILE_NORMAL,
  buildAiPcIntelCommandLine,
  buildAiPcShellCommandArgs,
  buildAiPcShellCommandLine,
  buildAiPcToolBundle,
  executeAiPcDriverdevTool,
  executeAiPcFileTool,
  findAiPcShellCommandByToolName,
  isAiPcFileToolName,
  isAiPcIntelToolName,
} from './ai_pc_cmd.mjs';

const MODEL = 'gpt-5.4';
const MODEL_TAG_COLOR = '\x1b[38;2;60;183;161m';
const ANSI_RESET = '\x1b[0m';
const LOCAL_TOOL_MAX_ROUNDS = 10;

const SYSTEM_PROMPT = [
  'You are the TRUEOS shell AI mode.',
  'Reply for a terminal context.',
  'Be concise, concrete, and technically useful.',
  'Do not mention browser integration.',
  'Use the provided TRUEOS-native tools when they help.',
].join(' ');

function normalizeToolProfile(profile) {
  switch (String(profile || AI_TOOL_PROFILE_NORMAL).trim()) {
    case AI_TOOL_PROFILE_INTELDEV:
      return AI_TOOL_PROFILE_INTELDEV;
    case AI_TOOL_PROFILE_DRIVERDEV:
      return AI_TOOL_PROFILE_DRIVERDEV;
    default:
      return AI_TOOL_PROFILE_NORMAL;
  }
}

function buildSystemPrompt(toolProfile) {
  switch (toolProfile) {
    case AI_TOOL_PROFILE_INTELDEV:
      return [
        SYSTEM_PROMPT,
        'This session is in inteldev mode.',
        'Use the Intel adapter for Intel GPU bring-up and debugging.',
        'Generic shell tools are also available.',
      ].join(' ');
    case AI_TOOL_PROFILE_DRIVERDEV:
      return [
        SYSTEM_PROMPT,
        'This session is in driverdev mode.',
        'Use the driverdev tools for live xHCI and USB inspection.',
        'Generic shell tools are also available.',
      ].join(' ');
    default:
      return [
        SYSTEM_PROMPT,
        'This session is in normal mode.',
        'Web search and local TRUEOS filesystem context are available when useful.',
        'Keep the interaction lightweight unless the prompt requires deeper inspection.',
      ].join(' ');
  }
}

export function printLine(text) {
  const value = String(text ?? '');
  if (typeof globalThis.__trueosAiPrintLine === 'function') {
    globalThis.__trueosAiPrintLine(value);
  }
}

function printMultiline(text) {
  const value = String(text ?? '');
  const lines = value.split(/\r?\n/);
  for (const line of lines) {
    printLine(line);
  }
}

export function normalizeOutput(response) {
  if (response && typeof response.output_text === 'string' && response.output_text.trim()) {
    return response.output_text.trim();
  }

  const chunks = [];
  const output = Array.isArray(response && response.output) ? response.output : [];
  for (const item of output) {
    if (!item || item.type !== 'message' || !Array.isArray(item.content)) {
      continue;
    }
    for (const part of item.content) {
      if (part && part.type === 'output_text' && typeof part.text === 'string' && part.text) {
        chunks.push(part.text);
      }
    }
  }
  return chunks.join('\n').trim();
}

export function collapseWhitespace(text) {
  return String(text ?? '').replace(/\s+/g, ' ').trim();
}

function clipText(text, maxChars) {
  const value = collapseWhitespace(text);
  if (!value) {
    return '';
  }
  if (value.length <= maxChars) {
    return value;
  }
  return `${value.slice(0, Math.max(0, maxChars - 3))}...`;
}

function quoteInline(text) {
  const value = String(text ?? '');
  return `"${value.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
}

export function readEnv(name) {
  const env = globalThis.__env__;
  if (!env || typeof env !== 'object') {
    return '';
  }
  const value = env[name];
  return typeof value === 'string' ? value.trim() : '';
}

export function getOpenAiBaseUrl() {
  const raw = readEnv('OPENAI_BASE_URL') || 'https://api.openai.com/v1';
  return String(raw).replace(/\/+$/, '');
}

export function getOpenAiApiKey() {
  return readEnv('OPENAI_API_KEY');
}

function sleep(ms) {
  return new Promise((resolve) => {
    setTimeout(resolve, Math.max(0, Number(ms) || 0));
  });
}

export function decodeTrueOsFetchRc(error) {
  const code = typeof error === 'number'
    ? error
    : (typeof error === 'string' && /^-?\d+$/.test(error.trim()) ? Number(error.trim()) : NaN);
  if (!Number.isFinite(code)) {
    return '';
  }

  switch (code) {
    case -1: return 'FS_ERR_BAD_UTF8';
    case -2: return 'FS_ERR_IO';
    case -3: return 'FS_ERR_NO_SPACE';
    case -4: return 'FS_ERR_BAD_PARAM';
    case -5: return 'FS_ERR_USBMS_NOT_FOUND';
    case -6: return 'FS_ERR_BAD_PATH';
    case -7: return 'FS_ERR_TOO_LARGE';
    case -8: return 'FS_ERR_NOT_FOUND';
    case -9: return 'FS_ERR_ALREADY_EXISTS';
    case -10: return 'NET_ERR_BAD_URL';
    case -11: return 'NET_ERR_TIMEOUT';
    case -12: return 'NET_ERR_HTTP';
    case -13: return 'NET_ERR_TLS';
    case -14: return 'FS_ERR_TIMEOUT';
    case -111: return 'NET_ERR_TIMEOUT_DNS';
    case -112: return 'NET_ERR_TIMEOUT_CONNECT';
    case -113: return 'NET_ERR_TIMEOUT_TLS';
    case -114: return 'NET_ERR_TIMEOUT_BODY';
    default: return `UNKNOWN_RC_${code}`;
  }
}

export function normalizeRequestError(error) {
  if (error instanceof Error) {
    return error;
  }
  const rcName = decodeTrueOsFetchRc(error);
  if (rcName) {
    return new Error(`${rcName} (${String(error)})`);
  }
  return new Error(String(error ?? 'unknown ai request error'));
}

function shouldRetryRequestError(error) {
  const message = String(error && error.message ? error.message : error || '');
  if (!message) {
    return true;
  }
  if (message.includes('OPENAI_API_KEY is missing')) {
    return false;
  }
  if (message.includes('TRUEOS fetch bridge is unavailable')) {
    return false;
  }
  if (/\bHTTP (400|401|403|404)\b/.test(message)) {
    return false;
  }
  return true;
}

export async function callOpenAiResponses(request) {
  const apiKey = getOpenAiApiKey();
  if (!apiKey) {
    throw new Error('OPENAI_API_KEY is missing');
  }

  const url = `${getOpenAiBaseUrl()}/responses`;
  const body = JSON.stringify(request);

  function parseJsonResponse(text) {
    try {
      return text ? JSON.parse(text) : {};
    } catch (error) {
      const preview = String(text || '').slice(0, 240).replace(/\s+/g, ' ');
      throw new Error(`openai response json parse failed: ${String(error && error.message ? error.message : error)} preview=${preview}`);
    }
  }

  if (typeof globalThis.__trueosFetchText === 'function') {
    let text;
    try {
      text = await globalThis.__trueosFetchText(url, 'POST', body, apiKey);
    } catch (error) {
      throw normalizeRequestError(error);
    }
    return parseJsonResponse(text);
  }

  if (typeof globalThis.fetch !== 'function') {
    throw new Error('TRUEOS fetch bridge is unavailable');
  }

  const response = await globalThis.fetch(url, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${apiKey}`,
      'Content-Type': 'application/json',
      Accept: 'application/json',
    },
    body,
  });

  const text = await response.text();
  if (!response.ok) {
    throw new Error(text || `HTTP ${String(response.status || 0)}`);
  }
  return parseJsonResponse(text);
}

export async function callOpenAiResponsesWithRetry(request, maxRetries = 2) {
  let lastError = null;
  const totalAttempts = Math.max(1, (Number(maxRetries) || 0) + 1);
  for (let attempt = 0; attempt < totalAttempts; attempt += 1) {
    try {
      return await callOpenAiResponses(request);
    } catch (error) {
      lastError = error;
      if (attempt + 1 >= totalAttempts || !shouldRetryRequestError(error)) {
        throw error;
      }
      await sleep(250 + (attempt * 500));
    }
  }
  throw lastError || new Error('openai request failed');
}

function readVectorStoreIds() {
  return readEnv('OPENAI_VECTOR_STORE_IDS')
    .split(/[\s,;]+/)
    .map((value) => value.trim())
    .filter(Boolean);
}

function buildInput(prompt, localFileContext) {
  const input = [];
  if (localFileContext) {
    input.push({
      role: 'system',
      content: [{
        type: 'input_text',
        text: [
          'Local TRUEOS file tree JSON follows.',
          'It is a compact broad-first listing capped by the runtime, not a full recursive dump.',
          'Use it when relevant and ask for deeper layered folders or specific file contents when needed.',
          '',
          localFileContext,
        ].join('\n'),
      }],
    });
  }
  input.push({
    role: 'user',
    content: [{ type: 'input_text', text: prompt }],
  });
  return input;
}

function buildRequest(prompt, options) {
  const localTools = buildAiPcToolBundle(options.toolProfile);
  const request = {
    model: MODEL,
    instructions: buildSystemPrompt(options.toolProfile),
    input: buildInput(prompt, options.localFileContext),
    tools: options.webSearch
      ? [...localTools, { type: 'web_search' }]
      : localTools,
    tool_choice: 'auto',
    parallel_tool_calls: false,
    text: {
      format: { type: 'text' },
      verbosity: 'low',
    },
    truncation: 'auto',
  };

  if (options.conversationId) {
    request.previous_response_id = options.conversationId;
  }

  if (options.webSearch) {
    request.include = ['web_search_call.action.sources'];
  } else if (options.fileSearch && options.vectorStoreIds.length > 0) {
    request.tools = [...localTools, {
      type: 'file_search',
      vector_store_ids: options.vectorStoreIds,
    }];
    request.include = ['file_search_call.results'];
  }

  return request;
}

function collectFunctionCalls(response) {
  const out = [];
  const output = Array.isArray(response?.output) ? response.output : [];
  for (const item of output) {
    if (item && item.type === 'function_call' && typeof item.call_id === 'string') {
      out.push(item);
    }
  }
  return out;
}

function parseToolArguments(call) {
  const raw = typeof call?.arguments === 'string' ? call.arguments : '{}';
  try {
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === 'object' ? parsed : {};
  } catch (error) {
    throw new Error(`tool arguments json parse failed for ${String(call?.name || '')}: ${String(error?.message || error)}`);
  }
}

function executeLocalShellTool(call, parsedArgs, targetMask) {
  const command = findAiPcShellCommandByToolName(call.name);
  if (!command) {
    throw new Error(`unknown shell tool: ${String(call.name || '')}`);
  }
  if (typeof globalThis.__trueosAiPcExecuteShellCommand !== 'function') {
    throw new Error('TRUEOS shell bridge is unavailable');
  }
  const commandLine = buildAiPcShellCommandLine(call.name, parsedArgs);
  const commandArgs = buildAiPcShellCommandArgs(call.name, parsedArgs);
  const result = globalThis.__trueosAiPcExecuteShellCommand(
    command.command,
    JSON.stringify(commandArgs),
    Number(targetMask || 0),
  );
  return {
    ...(result && typeof result === 'object' ? result : {}),
    tool_name: String(call.name || ''),
    command_line: commandLine,
  };
}

function executeLocalIntelTool(call, parsedArgs, targetMask) {
  if (typeof globalThis.__trueosAiPcExecuteShellCommand !== 'function') {
    throw new Error('TRUEOS shell bridge is unavailable');
  }
  const commandLine = buildAiPcIntelCommandLine(parsedArgs);
  const commandArgs = buildAiPcShellCommandArgs('shell_inteldev', parsedArgs);
  const result = globalThis.__trueosAiPcExecuteShellCommand(
    'inteldev',
    JSON.stringify(commandArgs),
    Number(targetMask || 0),
  );
  return {
    ...(result && typeof result === 'object' ? result : {}),
    tool_name: 'intel_adapter',
    command_line: commandLine,
  };
}

function executeLocalToolCall(call, targetMask, toolProfile) {
  const parsedArgs = parseToolArguments(call);
  if (toolProfile === AI_TOOL_PROFILE_INTELDEV && isAiPcIntelToolName(call.name)) {
    return executeLocalIntelTool(call, parsedArgs, targetMask);
  }
  if (toolProfile === AI_TOOL_PROFILE_NORMAL && isAiPcFileToolName(call.name)) {
    return executeAiPcFileTool(call.name, parsedArgs);
  }
  if (findAiPcShellCommandByToolName(call.name)) {
    return executeLocalShellTool(call, parsedArgs, targetMask);
  }
  if (toolProfile === AI_TOOL_PROFILE_DRIVERDEV) {
    return executeAiPcDriverdevTool(call.name, parsedArgs);
  }
  throw new Error(`tool unavailable in ${toolProfile} mode: ${String(call?.name || '')}`);
}

function maybeReadLocalFileContext(fileSearch) {
  if (!fileSearch || typeof globalThis.__trueosAiReadPrimaryFsTreeJsonAll !== 'function') {
    return '';
  }
  const json = globalThis.__trueosAiReadPrimaryFsTreeJsonAll(100);
  if (typeof json !== 'string' || !json.trim()) {
    return '';
  }
  return normalizeJsonFileTree(json);
}

export function maybePersistConversationId(response) {
  const conversationId = response && typeof response.id === 'string'
    ? response.id.trim()
    : '';
  if (conversationId && typeof globalThis.__trueosAiSetConversationId === 'function') {
    globalThis.__trueosAiSetConversationId(conversationId);
  }
}

function collectUsedToolNames(response) {
  const names = [];
  const seen = new Set();
  const output = Array.isArray(response && response.output) ? response.output : [];
  for (const item of output) {
    const type = typeof item?.type === 'string' ? item.type.trim() : '';
    if (!type || type === 'message' || type === 'reasoning') {
      continue;
    }
    const name = type.endsWith('_call') ? type.slice(0, -5) : type;
    if (!name || seen.has(name)) {
      continue;
    }
    seen.add(name);
    names.push(name);
  }
  return names;
}

function reasoningSummaryText(summary) {
  if (typeof summary === 'string') {
    return collapseWhitespace(summary);
  }
  if (Array.isArray(summary)) {
    const parts = [];
    for (const item of summary) {
      if (typeof item === 'string') {
        const next = collapseWhitespace(item);
        if (next) {
          parts.push(next);
        }
        continue;
      }
      const text = typeof item?.text === 'string' ? collapseWhitespace(item.text) : '';
      if (text) {
        parts.push(text);
      }
    }
    return collapseWhitespace(parts.join(' '));
  }
  return '';
}

export function printResponseSummary(response, text) {
  const model = collapseWhitespace(response && response.model);
  const answer = String(text ?? '').trim();
  if (answer) {
    printLine(model ? `${answer} ${MODEL_TAG_COLOR}[${model}]${ANSI_RESET}` : answer);
  }

  const usage = response && typeof response === 'object' ? response.usage : null;
  const inputTokens = Number(usage && usage.input_tokens || 0) || 0;
  const outputTokens = Number(usage && usage.output_tokens || 0) || 0;
  if ((inputTokens > 0 || outputTokens > 0) && typeof globalThis.__trueosAiAddUsageTotals === 'function') {
    globalThis.__trueosAiAddUsageTotals(inputTokens, outputTokens);
    printLine(`{Usage in ${inputTokens} out ${outputTokens}}`);
  } else if (inputTokens > 0 || outputTokens > 0) {
    printLine(`{Usage in ${inputTokens} out ${outputTokens}}`);
  }
}

export async function runShellPrompt(config = null) {
  const source = config && typeof config === 'object' ? config : {};
  const prompt = String(source.prompt || '').trim();
  if (!prompt) {
    printLine('ai: empty prompt');
    return;
  }

  const webSearch = !!source.webSearch;
  const fileSearch = !!source.fileSearch;
  const toolProfile = normalizeToolProfile(source.modeProfile);
  const conversationId = String(source.conversationId || '').trim();
  const targetMask = Number(source.targetMask || 0) || 0;
  const vectorStoreIds = readVectorStoreIds();
  const localFileContext = fileSearch
    ? maybeReadLocalFileContext(true)
    : '';

  if (fileSearch && localFileContext) {
    printLine('ai: file mode using local TRUEOS file tree json');
  }

  let previousResponseId = conversationId;
  let nextInput = buildInput(prompt, localFileContext);

  for (let round = 0; round < LOCAL_TOOL_MAX_ROUNDS; round += 1) {
    let response;
    try {
      response = await callOpenAiResponsesWithRetry({
        ...buildRequest(prompt, {
          webSearch,
          fileSearch,
          toolProfile,
          conversationId: previousResponseId,
          vectorStoreIds,
          localFileContext: '',
        }),
        input: nextInput,
      }, 2);
    } catch (error) {
      const normalized = normalizeRequestError(error);
      printLine(`ai: request failed: ${String(normalized && normalized.stack ? normalized.stack : normalized)}`);
      throw error;
    }

    maybePersistConversationId(response);
    previousResponseId = typeof response?.id === 'string' ? response.id.trim() : previousResponseId;

    const functionCalls = collectFunctionCalls(response);
    if (functionCalls.length === 0) {
      const text = normalizeOutput(response);
      if (!text) {
        printLine('ai: empty response');
        return;
      }
      printResponseSummary(response, text);
      return;
    }

    nextInput = [];
    for (const call of functionCalls) {
      let toolResult;
      try {
        toolResult = executeLocalToolCall(call, targetMask, toolProfile);
      } catch (error) {
        toolResult = {
          ok: false,
          tool_name: String(call?.name || ''),
          stderr: String(error?.stack || error),
          exit_code: 1,
        };
      }
      nextInput.push({
        type: 'function_call_output',
        call_id: String(call.call_id),
        output: JSON.stringify(toolResult),
      });
    }
    printLine(`ai: tool round ${round + 1} ok`);
  }

  printLine('ai: tool loop exceeded round limit');
}

export async function runNormalPrompt(promptText) {
  return runShellPrompt({ prompt: promptText });
}
