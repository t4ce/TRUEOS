import {
  callOpenAiResponsesWithRetry,
  maybePersistConversationId,
  normalizeOutput,
  normalizeRequestError,
  printLine,
  printResponseSummary,
} from './ai_shell_normal.mjs';
import {
  executeAiPcFileTool,
  buildAiPcIntelCommandLine,
  buildAiPcShellCommandArgs,
  buildAiPcShellCommandLine,
  buildAiPcToolBundle,
  executeAiPcDriverdevTool,
  findAiPcShellCommandByToolName,
  isAiPcFileToolName,
  isAiPcIntelToolName,
} from './ai_pc_cmd.mjs';

const MODEL = 'gpt-5.4';
const AI_PC_MAX_TOOL_ROUNDS = 10;

function buildAiPcSystemPrompt() {
  const shellToolNames = buildAiPcToolBundle()
    .map((tool) => String(tool?.name || '').trim())
    .filter((name) => name.startsWith('shell_'))
    .join(', ');

  return [
    'You are the TRUEOS ai-pc mode.',
    'The provided tools are real callable tools in this session, not text examples.',
    shellToolNames
      ? `Callable shell tools in this session are: ${shellToolNames}.`
      : 'Callable shell tools may be available in this session.',
    'Use the provided TRUEOS-native shell, Intel adapter, custom file adapter, and driverdev tools when they help.',
    'Use shell_file for shell file actions like list, ramdisc, or format.',
    'Use file_adapter_get_primary_tree for read-only TRUEOS filesystem tree visibility.',
    'For Intel GPU work, prefer the dedicated intel_adapter tool over generic shell tools.',
    'Prefer inspecting state before making changes.',
    'For shell tools, follow each tool parameter schema exactly.',
    'Do not claim to have used browser automation in this mode.',
    'Finish with a concise terminal-style answer after tool use is complete.',
  ].join(' ');
}

function buildInitialInput(prompt) {
  return [{
    role: 'user',
    content: [{ type: 'input_text', text: String(prompt || '') }],
  }];
}

function buildToolLoopRequest(input, previousResponseId) {
  const request = {
    model: MODEL,
    instructions: buildAiPcSystemPrompt(),
    input,
    tools: buildAiPcToolBundle(),
    tool_choice: 'auto',
    truncation: 'auto',
    text: {
      format: { type: 'text' },
      verbosity: 'low',
    },
  };
  if (previousResponseId) {
    request.previous_response_id = previousResponseId;
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

function executeAiPcShellTool(call, parsedArgs, targetMask) {
  const command = findAiPcShellCommandByToolName(call.name);
  if (!command) {
    throw new Error(`unknown ai-pc shell tool: ${String(call.name || '')}`);
  }
  if (typeof globalThis.__trueosAiPcExecuteShellCommand !== 'function') {
    throw new Error('TRUEOS ai-pc shell bridge is unavailable');
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

function executeAiPcIntelTool(call, parsedArgs, targetMask) {
  if (typeof globalThis.__trueosAiPcExecuteShellCommand !== 'function') {
    throw new Error('TRUEOS ai-pc shell bridge is unavailable');
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

function executeAiPcToolCall(call, targetMask) {
  const parsedArgs = parseToolArguments(call);
  if (isAiPcIntelToolName(call.name)) {
    return executeAiPcIntelTool(call, parsedArgs, targetMask);
  }
  if (isAiPcFileToolName(call.name)) {
    return executeAiPcFileTool(call.name, parsedArgs);
  }
  if (findAiPcShellCommandByToolName(call.name)) {
    return executeAiPcShellTool(call, parsedArgs, targetMask);
  }
  return executeAiPcDriverdevTool(call.name, parsedArgs);
}

export async function runShellPrompt(config = null) {
  const source = config && typeof config === 'object' ? config : {};
  const prompt = String(source.prompt || '').trim();
  if (!prompt) {
    printLine('ai: empty prompt');
    return;
  }

  let previousResponseId = String(source.conversationId || '').trim();
  const targetMask = Number(source.targetMask || 0) || 0;
  let nextInput = buildInitialInput(prompt);

  for (let round = 0; round < AI_PC_MAX_TOOL_ROUNDS; round += 1) {
    let response;
    try {
      response = await callOpenAiResponsesWithRetry(
        buildToolLoopRequest(nextInput, previousResponseId),
        2,
      );
    } catch (error) {
      const normalized = normalizeRequestError(error);
      printLine(`ai: request failed: ${String(normalized?.stack || normalized)}`);
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
        toolResult = executeAiPcToolCall(call, targetMask);
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
  }

  printLine('ai: tool loop exceeded round limit');
}
