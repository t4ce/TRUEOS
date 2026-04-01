const MODEL = 'gpt-5.4';
const FILE_TREE_MAX_CHARS = 12_000;
const MODEL_TAG_COLOR = '\x1b[38;2;60;183;161m';
const ANSI_RESET = '\x1b[0m';

const SYSTEM_PROMPT = [
  'You are the TRUEOS shell AI mode.',
  'Reply for a terminal context.',
  'Be concise, concrete, and technically useful.',
  'Do not mention browser integration.',
].join(' ');

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

function normalizeJsonFileTree(raw) {
  if (typeof raw !== 'string' || !raw.trim()) {
    return '';
  }

  try {
    const parsed = JSON.parse(raw);
    const entries = Array.isArray(parsed && parsed.entries) ? parsed.entries : [];
    const compact = {
      version: Number(parsed && parsed.version || 1) || 1,
      root: String(parsed && parsed.root || '/'),
      max_entries: Number(parsed && parsed.max_entries || entries.length || 0) || 0,
      truncated: !!(parsed && parsed.truncated),
      entries: entries.map((entry) => ({
        path: String(entry && entry.path || ''),
        kind: String(entry && entry.kind || ''),
        depth: Number(entry && entry.depth || 0) || 0,
      })),
    };
    return JSON.stringify(compact).slice(0, FILE_TREE_MAX_CHARS);
  } catch {
    return String(raw).slice(0, FILE_TREE_MAX_CHARS);
  }
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
  const request = {
    model: MODEL,
    instructions: SYSTEM_PROMPT,
    input: buildInput(prompt, options.localFileContext),
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
    request.tools = [{ type: 'web_search' }];
    request.tool_choice = 'auto';
    request.include = ['web_search_call.action.sources'];
  } else if (options.fileSearch && options.vectorStoreIds.length > 0) {
    request.tools = [{
      type: 'file_search',
      vector_store_ids: options.vectorStoreIds,
    }];
    request.tool_choice = 'auto';
    request.include = ['file_search_call.results'];
  }

  return request;
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
  const conversationId = String(source.conversationId || '').trim();
  const vectorStoreIds = readVectorStoreIds();
  const localFileContext = fileSearch && vectorStoreIds.length <= 0
    ? maybeReadLocalFileContext(true)
    : '';

  if (fileSearch && vectorStoreIds.length <= 0 && localFileContext) {
    printLine('ai: file mode using local TRUEOS file tree json');
  }

  const request = buildRequest(prompt, {
    webSearch,
    fileSearch,
    conversationId,
    vectorStoreIds,
    localFileContext,
  });
  let response;
  try {
    response = await callOpenAiResponsesWithRetry(request, 2);
  } catch (error) {
    const normalized = normalizeRequestError(error);
    printLine(`ai: request failed: ${String(normalized && normalized.stack ? normalized.stack : normalized)}`);
    throw error;
  }

  maybePersistConversationId(response);

  const text = normalizeOutput(response);
  if (!text) {
    printLine('ai: empty response');
    return;
  }

  printResponseSummary(response, text);
}

export async function runNormalPrompt(promptText) {
  return runShellPrompt({ prompt: promptText });
}
