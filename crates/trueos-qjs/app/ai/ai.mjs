import OpenAI from "openai";

function withTimeout(promise, ms, label) {
  if (typeof setTimeout !== "function") {
    return promise;
  }
  let timeoutId;
  const timeoutPromise = new Promise((_, reject) => {
    timeoutId = setTimeout(
      () => reject(new Error(`${label} timed out after ${ms}ms`)),
      ms
    );
  });

  return Promise.race([promise, timeoutPromise]).finally(() => {
    if (typeof clearTimeout === "function" && timeoutId !== undefined) {
      clearTimeout(timeoutId);
    }
  });
}

try {
  console.log("ai: start");
  const client = new OpenAI({ apiKey: "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", timeout: 12000 });
  console.log("ai: request.begin");


  const tools = [
    { type: "web_search" },
    {
      type: "function",
      name: "start_tetris",
      description: "Start a new Tetris game - Shell Version.",
      strict: "true",
      additionalProperties: "false",
    },
  ];

  const response = await withTimeout(
    client.responses.create({
      model: "gpt-5.4",
      tools: tools,
      input: "Whats the worlds most important News Today? ",
    }),
    15000,
    "responses.create"
  );
  console.log("ai: request.done");
  if (response && typeof response.output_text === "string") {
    console.log(response.output_text);
  } else {
    console.log(`ai: response missing output_text (${typeof response})`);
  }
  console.log("ai: done");
} catch (e) {
  console.log(`ai error: ${String(e && e.message ? e.message : e)}`);
}