import OpenAI from "openai";



try {
  console.log("ai: start");
  const client = new OpenAI({ apiKey: "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" });
  console.log("ai: request.begin");


  const tools = [
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
  ];

  const response = await client.responses.create({
    model: "gpt-5.4",
    tools: tools,
    input: `Whats the worlds most important News Today? Date: ${kernelDateDayMonthYear() } AND start Tetris please.`,
  });
  console.log("ai: request.done");
  const outputItems = response && Array.isArray(response.output) ? response.output : [];
  for (const item of outputItems) {
    if (item && item.type === "function_call") {
      try {
        console.log(`ai: function_call ${JSON.stringify(item)}`);
      } catch (_e) {
        console.log("ai: function_call [unserializable]");
      }
    }
  }
  if (response && typeof response.output_text === "string") {
    console.log(response.output_text);
  } else {
    console.log(`ai: response missing output_text (${typeof response})`);
  }
  console.log("ai: done");
} catch (e) {
  console.log(`ai error: ${String(e && e.message ? e.message : e)}`);
}