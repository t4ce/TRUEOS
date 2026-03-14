import {
  buildResponsesRequest,
  createOpenAiClient,
  createResponse,
  decorateResponseTools,
  getResponseFunctionCalls,
  getResponseOutputText,
} from "./openai_client.mjs";



try {
  console.log("ai: start");
  const client = createOpenAiClient();
  console.log("ai: request.begin");


  const tools = decorateResponseTools([
    { type: "web_search" },
    {
      type: "function",
      name: "start_tetris",
      description: "Start a new Tetris game - Shell Version.",
      strict: true,
      parameters: {
        type: "object",
        properties: {},
        additionalProperties: false,
      },
    },
  ]);

  const request = buildResponsesRequest({
    tools,
    input: `Whats the worlds most important News Today? Date: ${kernelDateDayMonthYear() } AND start Tetris please.`,
  });
  const response = await createResponse(client, request);
  console.log("ai: request.done");
  for (const item of getResponseFunctionCalls(response)) {
    try {
      console.log(`ai: function_call ${JSON.stringify(item)}`);
    } catch (_e) {
      console.log("ai: function_call [unserializable]");
    }
  }
  const outputText = getResponseOutputText(response);
  if (outputText) {
    console.log(outputText);
  } else {
    console.log(`ai: response missing output_text (${typeof response})`);
  }
  console.log("ai: done");
} catch (e) {
  console.log(`ai error: ${String(e && e.message ? e.message : e)}`);
}
