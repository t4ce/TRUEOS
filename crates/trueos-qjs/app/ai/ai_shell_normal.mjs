import OpenAI from 'openai';

const MODEL = 'gpt-5.4';

const SYSTEM_PROMPT = [
  'You are the TRUEOS shell AI mode.',
  'Reply for a terminal context.',
  'Be concise, concrete, and technically useful.',
  'Do not assume tools are available.',
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

export async function runNormalPrompt(promptText) {
  const prompt = String(promptText || '').trim();
  if (!prompt) {
    printLine('ai: empty prompt');
    return;
  }

  const client = new OpenAI();
  const response = await client.responses.create({
    model: MODEL,
    input: [
      {
        role: 'system',
        content: [{ type: 'input_text', text: SYSTEM_PROMPT }],
      },
      {
        role: 'user',
        content: [{ type: 'input_text', text: prompt }],
      },
    ],
  });

  const text = normalizeOutput(response);
  if (!text) {
    printLine('ai: empty response');
    return;
  }

  printMultiline(text);
}