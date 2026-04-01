const MODEL = 'gpt-5.4';
const FILE_TREE_MAX_CHARS = 12_000;

const SYSTEM_PROMPT = [
  'You are the TRUEOS shell AI mode.',
  'Reply for a terminal context.',
  'Be concise, concrete, and technically useful.',
  'Do not mention browser integration.',
].join(' ');

function printLine(text) {
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

function normalizeOutput(response) {
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

function collapseWhitespace(text) {
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

function readEnv(name) {
  const env = globalThis.__env__;
  if (!env || typeof env !== 'object') {
    return '';
  }
  const value = env[name];
  return typeof value === 'string' ? value.trim() : '';
}

function getOpenAiBaseUrl() {
  const raw = readEnv('OPENAI_BASE_URL') || 'https://api.openai.com/v1';
  return String(raw).replace(/\/+$/, '');
}

function getOpenAiApiKey() {
  return readEnv('OPENAI_API_KEY');
}

async function callOpenAiResponses(request) {
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
    const text = await globalThis.__trueosFetchText(url, 'POST', body, apiKey);
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
    request.conversation = options.conversationId;
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

function maybePersistConversationId(response) {
  const conversationId = response && response.conversation && typeof response.conversation.id === 'string'
    ? response.conversation.id.trim()
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

function printResponseSummary(response, text) {
  const model = collapseWhitespace(response && response.model);
  const answer = clipText(text, 160);
  if (model || answer) {
    const parts = [];
    if (model) {
      parts.push(`model=${model}`);
    }
    if (answer) {
      parts.push(`answer=${quoteInline(answer)}`);
    }
    printLine(`ai: ${parts.join(' ')}`);
  }

  const usage = response && typeof response === 'object' ? response.usage : null;
  const inputTokens = Number(usage && usage.input_tokens || 0) || 0;
  const outputTokens = Number(usage && usage.output_tokens || 0) || 0;
  if ((inputTokens > 0 || outputTokens > 0) && typeof globalThis.__trueosAiAddUsageTotals === 'function') {
    const totalsRaw = globalThis.__trueosAiAddUsageTotals(inputTokens, outputTokens);
    if (typeof totalsRaw === 'string' && totalsRaw) {
      printLine(`ai: usage in=${inputTokens} out=${outputTokens} ${totalsRaw}`);
    } else {
      printLine(`ai: usage in=${inputTokens} out=${outputTokens}`);
    }
  } else if (inputTokens > 0 || outputTokens > 0) {
    printLine(`ai: usage in=${inputTokens} out=${outputTokens}`);
  }

  const reasoning = response && typeof response === 'object' ? response.reasoning : null;
  const effort = collapseWhitespace(reasoning && reasoning.effort);
  const summary = clipText(reasoningSummaryText(reasoning && reasoning.summary), 200);
  if ((effort && effort !== 'none') || summary) {
    const parts = [];
    if (effort && effort !== 'none') {
      parts.push(`effort=${effort}`);
    }
    if (summary) {
      parts.push(`summary=${quoteInline(summary)}`);
    }
    printLine(`ai: reasoning ${parts.join(' ')}`);
  }

  const tools = collectUsedToolNames(response);
  if (tools.length > 0) {
    printLine(`ai: tools ${tools.join(', ')}`);
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

  printLine('ai: sending request');

  const request = buildRequest(prompt, {
    webSearch,
    fileSearch,
    conversationId,
    vectorStoreIds,
    localFileContext,
  });
  let response;
  try {
    response = await callOpenAiResponses(request);
  } catch (error) {
    printLine(`ai: request failed: ${String(error && error.stack ? error.stack : error)}`);
    throw error;
  }

  maybePersistConversationId(response);

  const text = normalizeOutput(response);
  if (!text) {
    printLine('ai: empty response');
    return;
  }

  printResponseSummary(response, text);
  printMultiline(text);
}

export async function runNormalPrompt(promptText) {
  return runShellPrompt({ prompt: promptText });
}
