# S01 · 工具系统架构

> 为所有后续工具建立统一的注册、分发、执行框架

**参考源码**: `src/Tool.ts`, `src/tools.ts`, `src/query.ts`  
**新增文件**: `src/tools/mod.rs`, `src/engine.rs`  
**修改文件**: `src/api.rs`, `src/types.rs`, `src/repl.rs`, `src/main.rs`

---

## 一、背景：工具调用的本质

Claude 的"工具调用"并不是真正在 AI 侧执行代码，而是一个**约定好的 JSON 协议**：

```
用户发送 prompt
    ↓
Claude API 返回 stop_reason = "tool_use"
    ↓
本地程序解析 tool_use 内容块，执行对应函数
    ↓
将执行结果作为 tool_result 追加到 messages[]
    ↓
再次调用 Claude API（携带完整历史）
    ↓
Claude 基于工具结果继续回答
    ↓
stop_reason = "end_turn" → 循环结束
```

S00 只有单轮对话（一次 API 调用）。S01 将其扩展为**可以无限次 API 调用直到 stop_reason != "tool_use"** 的 Agent Loop。

---

## 二、核心设计：三层结构

```
┌─────────────────────────────────────────────────────┐
│  REPL / one_shot  (src/repl.rs, src/main.rs)        │  ← 用户交互层
│  负责：读取输入、调用 engine、显示结果               │
└────────────────────┬────────────────────────────────┘
                     │ messages: Vec<Value>
                     │ registry: &ToolRegistry
                     ▼
┌─────────────────────────────────────────────────────┐
│  Agent Loop Engine  (src/engine.rs)                  │  ← 循环控制层
│  负责：循环调用 API、分发工具调用、组装消息历史       │
└────────────────────┬────────────────────────────────┘
                     │ call_with_tools()
                     ▼
┌─────────────────────────────────────────────────────┐
│  ClaudeClient / ToolRegistry  (src/api.rs, tools/)   │  ← 基础服务层
│  负责：HTTP 通信、SSE 解析、工具注册与执行            │
└─────────────────────────────────────────────────────┘
```

---

## 三、`Tool` trait（`src/tools/mod.rs`）

### 3.1 接口定义

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;               // Claude tool_use 中的 name 字段
    fn description(&self) -> &str;        // 显示给用户
    fn schema(&self) -> Value;            // JSON Schema，传给 Claude API
    async fn execute(&self, input: Value) -> Result<String>;
}
```

**为什么 `execute` 是异步的？**  
后续工具（`BashTool`、`WebFetchTool`）需要 spawn 子进程或发起 HTTP 请求，必须是 async。S01 的 `EchoTool` 虽然不需要 async，但统一接口避免日后改动 trait。

**为什么用 `async_trait` crate？**  
Rust edition 2021 的 trait 方法不支持 `async fn`（edition 2024 才原生支持）。`async_trait` 宏将 async 方法脱糖为返回 `Pin<Box<dyn Future>>` 的普通方法。

### 3.2 JSON Schema 格式

传给 Claude API 的 `tools` 数组格式：

```json
[
  {
    "name": "echo_tool",
    "description": "Echo back the provided text.",
    "input_schema": {
      "type": "object",
      "properties": {
        "text": { "type": "string", "description": "The text to echo" }
      },
      "required": ["text"]
    }
  }
]
```

`Tool::schema()` 返回的是 `input_schema` 部分（object schema），`ToolRegistry::get_schemas()` 负责包装成完整格式。

### 3.3 `EchoTool` — 内置验证工具

`EchoTool` 是唯一内置工具，用于验证整个 tool_use → tool_result 闭环可以正常运行，不依赖文件系统或外部进程：

```rust
pub struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str { "echo_tool" }
    fn schema(&self) -> Value {
        json!({ "type": "object", "properties": { "text": { "type": "string" } }, "required": ["text"] })
    }
    async fn execute(&self, input: Value) -> Result<String> {
        let text = input["text"].as_str().ok_or_else(|| anyhow!("missing 'text'"))?;
        Ok(text.to_string())
    }
}
```

---

## 四、`ToolRegistry`（`src/tools/mod.rs`）

### 4.1 结构

```rust
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}
```

- key 是工具名称（与 Claude API 的 `tool_use.name` 一一对应）
- value 是 trait object，运行时多态

### 4.2 关键方法

| 方法 | 作用 | 对应源码 |
|------|------|---------|
| `register(tool)` | 注册工具，同名覆盖 | `src/tools.ts` 数组 push |
| `get_schemas()` | 返回排序后的 JSON Schema 列表 | 构建 API 请求的 `tools` 字段 |
| `execute(name, input)` | 查找并调用工具 | `src/query.ts` tool dispatch |

### 4.3 `get_schemas()` 排序的原因

Anthropic API 不要求 tools 有固定顺序，但排序后：
1. API 请求 payload 确定性强，便于测试 snapshot
2. 不同版本的日志更容易 diff

---

## 五、新增类型（`src/types.rs`）

### 5.1 `ToolUseCall`

```rust
pub struct ToolUseCall {
    pub id: String,          // Claude 生成的唯一 ID，必须原样回传
    pub name: String,        // 工具名
    pub input_json: String,  // 累积的 JSON 字符串（流式拼接）
}
```

`input_json` 之所以是 `String` 而不是 `Value`，是因为在流式响应中输入是分片追加的（`input_json_delta` 事件），需要先完整拼接再解析。

### 5.2 `AgentResponse`

```rust
pub struct AgentResponse {
    pub text: String,               // 文本内容（已实时打印到 stdout）
    pub stop_reason: String,        // "end_turn" | "tool_use" | "max_tokens"
    pub tool_uses: Vec<ToolUseCall>,
}
```

### 5.3 `StreamDelta` 扩展

原 `Delta` 新增 `partial_json` 字段，用于接收 `input_json_delta` 事件：

```rust
pub struct StreamDelta {
    pub delta_type: String,
    pub text: Option<String>,          // text_delta
    pub partial_json: Option<String>,  // input_json_delta（工具输入）
    pub stop_reason: Option<String>,   // message_delta
}
```

---

## 六、`call_with_tools`（`src/api.rs`）

### 6.1 SSE 状态机

流式响应中，工具调用的内容块以多个 SSE 事件分批到达，需要按 `index` 跟踪每个 block 的状态：

```
content_block_start  {index:0, type:"text"}
content_block_delta  {index:0, delta:{type:"text_delta", text:"I'll echo "}}
content_block_stop   {index:0}
content_block_start  {index:1, type:"tool_use", id:"toolu_01", name:"echo_tool"}
content_block_delta  {index:1, delta:{type:"input_json_delta", partial_json:"{\"text\""}}
content_block_delta  {index:1, delta:{type:"input_json_delta", partial_json:":\"hello\"}"}}
content_block_stop   {index:1}
message_delta        {delta:{stop_reason:"tool_use"}}
message_stop
```

内部使用枚举追踪每个 block 的类型：

```rust
enum BlockState {
    Text(String),
    ToolUse { id: String, name: String, input_json: String },
}

let mut blocks: Vec<BlockState> = Vec::new();
```

收到 `content_block_start` 时 push 新 block；收到 `content_block_delta` 时按 `index` 找到对应 block 追加内容。

### 6.2 文本实时打印

文本 delta 到达时**立即**打印到 stdout（与 S00 一致），工具 delta 不打印：

```rust
BlockState::Text(text) => {
    if let Some(fragment) = &delta.text {
        print!("{}", fragment);   // 实时输出
        text.push_str(fragment);
    }
}
BlockState::ToolUse { input_json, .. } => {
    if let Some(fragment) = &delta.partial_json {
        input_json.push_str(fragment);   // 静默累积
    }
}
```

### 6.3 与 `query_streaming` 的区别

| | `query_streaming` | `call_with_tools` |
|---|---|---|
| 参数 | `prompt: &str, history: &[Message]` | `messages: &[Value], tools: &[Value]` |
| 历史格式 | `Message { role, content: String }` | `Value`（支持数组 content） |
| 工具支持 | 无 | 有 |
| 返回类型 | `String` | `AgentResponse` |
| 用途 | S00 遗留接口 | S01+ Agent Loop |

`query_streaming` 保留是为了不破坏 S00 的 examples 和测试。新代码统一走 `call_with_tools`。

---

## 七、Agent Loop（`src/engine.rs`）

### 7.1 完整流程

```rust
pub async fn run_agent_loop(
    client: &ClaudeClient,
    registry: &ToolRegistry,
    messages: &mut Vec<Value>,   // 原地追加，调用方持有完整历史
) -> Result<String> {
    let tools = registry.get_schemas();

    loop {
        // 1. 调用 API（流式打印文本）
        let response = client.call_with_tools(messages, &tools).await?;

        // 2. 追加 assistant 消息（含 text + tool_use blocks）
        messages.push(build_assistant_message(&response.text, &response.tool_uses));

        // 3. 无工具调用或正常结束 → 退出
        if response.stop_reason != "tool_use" || response.tool_uses.is_empty() {
            return Ok(response.text);
        }

        // 4. 执行所有工具调用，收集结果
        let mut tool_results = Vec::new();
        for call in &response.tool_uses {
            let input = serde_json::from_str(&call.input_json).unwrap_or(Value::Null);
            let (content, is_error) = match registry.execute(&call.name, input).await {
                Ok(result) => (result, false),
                Err(e)     => (e.to_string(), true),
            };
            tool_results.push(json!({
                "type": "tool_result",
                "tool_use_id": call.id,
                "content": content,
                "is_error": is_error
            }));
        }

        // 5. 追加 tool_result 用户消息 → 继续循环
        messages.push(json!({"role": "user", "content": tool_results}));
    }
}
```

### 7.2 为什么 `messages` 用 `&mut Vec<Value>` 而不是返回新列表？

- 调用方（REPL / one_shot）持有历史，函数结束后历史自动更新
- 不需要 clone 整个消息列表
- 与 Claude Code 源码 `src/query.ts` 的 `messages.push()` 模式一致

### 7.3 `build_assistant_message`

assistant 消息的 `content` 必须是数组（包含所有 text 和 tool_use blocks），不能是纯字符串，否则 Claude 无法通过 `tool_use_id` 匹配结果：

```rust
// 正确格式（content 是数组）
{
  "role": "assistant",
  "content": [
    { "type": "text", "text": "I'll echo this:" },
    { "type": "tool_use", "id": "toolu_01", "name": "echo_tool", "input": {"text":"hi"} }
  ]
}

// 错误格式（Claude API 会拒绝带有 tool_use 的纯字符串 content）
{
  "role": "assistant",
  "content": "I'll echo this:"
}
```

### 7.4 工具执行错误处理

工具执行失败时**不终止循环**，而是返回 `is_error: true` 的 tool_result，让 Claude 自行决定如何处理（重试、换工具、告知用户）。这与 Claude Code 源码的处理方式一致。

---

## 八、消息历史格式变化

S00 使用 `ConversationHistory`（只支持 `content: String`）。S01 切换为 `Vec<serde_json::Value>`，支持三种消息格式：

```json
// 1. 普通文本消息（用户/助手）
{ "role": "user", "content": "帮我 echo hello" }

// 2. 包含工具调用的助手消息
{
  "role": "assistant",
  "content": [
    { "type": "text", "text": "好的，我来调用 echo_tool：" },
    { "type": "tool_use", "id": "toolu_01", "name": "echo_tool", "input": {"text":"hello"} }
  ]
}

// 3. 工具结果用户消息
{
  "role": "user",
  "content": [
    { "type": "tool_result", "tool_use_id": "toolu_01", "content": "hello", "is_error": false }
  ]
}
```

---

## 九、`main.rs` 集成

```rust
// 注册工具
let mut registry = ToolRegistry::new();
registry.register(EchoTool);

// REPL 模式
repl::start_repl(&api_key, registry).await?;

// one_shot 模式
let mut messages = vec![json!({"role": "user", "content": prompt})];
engine::run_agent_loop(&client, &registry, &mut messages).await?;
```

---

## 十、验证：里程碑测试

### 10.1 单元测试（无需 API Key）

```bash
cargo test
```

覆盖点：
- `EchoTool` 正确执行 / 缺少字段报错
- `ToolRegistry` 注册、覆盖、排序、未知工具报错
- `build_assistant_message` 生成正确格式
- `handle_command` 各命令行为

### 10.2 集成测试（需要 API Key）

```bash
export ANTHROPIC_AUTH_TOKEN=sk-ant-...
cargo run
```

```
💬 You > 用 echo_tool 工具把 "hello world" 原样返回给我

🤖 Claude is thinking...

好的，我来调用 echo_tool：
▶ Tool: echo_tool (toolu_01abc)
  [echo_tool] → hello world

echo_tool 执行结果是：hello world
```

预期消息历史结构：

```json
[
  {"role": "user",      "content": "用 echo_tool 工具把 \"hello world\" 原样返回给我"},
  {"role": "assistant", "content": [{"type":"tool_use","id":"toolu_01abc","name":"echo_tool","input":{"text":"hello world"}}]},
  {"role": "user",      "content": [{"type":"tool_result","tool_use_id":"toolu_01abc","content":"hello world","is_error":false}]},
  {"role": "assistant", "content": [{"type":"text","text":"echo_tool 执行结果是：hello world"}]}
]
```

---

## 十一、文件变更汇总

| 文件 | 类型 | 主要变更 |
|------|------|---------|
| `Cargo.toml` | 修改 | 添加 `async-trait = "0.1"` |
| `src/types.rs` | 修改 | 添加 `ToolUseCall`, `AgentResponse`, 扩展 `StreamDelta` |
| `src/tools/mod.rs` | 新建 | `Tool` trait + `ToolRegistry` + `EchoTool` |
| `src/engine.rs` | 新建 | `run_agent_loop` + `build_assistant_message` |
| `src/api.rs` | 修改 | 添加 `call_with_tools` (SSE 状态机处理 tool_use) |
| `src/repl.rs` | 修改 | 接收 `ToolRegistry`，历史改为 `Vec<Value>` |
| `src/main.rs` | 修改 | 声明 `mod tools; mod engine;`，注册工具并传入 REPL |

---

## 十二、后续阶段的扩展点

S01 完成后，添加新工具只需：

```rust
// src/tools/file_read.rs
pub struct ReadTool;
#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str { "Read" }
    // ...
}

// main.rs
registry.register(ReadTool);
```

无需修改 `engine.rs` 或 `api.rs`。这就是工具架构的价值：**所有工具共享同一个循环和分发机制**。
