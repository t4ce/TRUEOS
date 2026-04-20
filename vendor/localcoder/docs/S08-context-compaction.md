# S08 · 上下文压缩

> 实现自动上下文压缩 + 手动 `/compact`，避免长会话持续增长导致上下文失控

**参考规划**: `docs/P00-plan.md` S08 章节  
**新增文件**: `src/compact.rs`  
**修改文件**: `src/api.rs`, `src/repl.rs`, `src/session.rs`, `src/main.rs`

---

## 一、目标

S08 的目标是让长对话在持续使用时仍然可控：

- 自动检测上下文长度并触发压缩
- 保留最近消息，不影响当前工作流
- 用摘要替换旧消息，显著缩短上下文
- 将压缩后的结果写回会话文件，保证恢复时一致

---

## 二、实现方式

### 2.1 token 估算

当前使用近似估算：

```rust
estimated_tokens = total_chars / 4
```

这是轻量实现，足够支撑本地版自动压缩。

### 2.2 自动压缩

压缩入口：

- `compact::maybe_compact(...)`

默认策略：

- 阈值：`12_000` estimated tokens
- 保留最近 `10` 条消息
- 更早的消息发送给模型生成摘要

压缩后消息结构：

```text
system: [对话摘要]\n...
system: [compact_boundary]
recent messages...
```

其中 `compact_boundary` 用于显式标记“前面是压缩摘要，后面是原始近期上下文”。

### 2.3 手动压缩

REPL 新增：

```text
/compact
```

当历史足够长时，手动触发压缩；否则提示“当前历史不足以压缩”。

---

## 三、摘要生成

`LLMClient::summarize_messages(...)` 会调用 Ollama `/api/chat`，使用专门的摘要提示词，保留：

1. 已完成任务与结果
2. 重要文件修改
3. 用户偏好与关键决策
4. 未完成任务

为了控制摘要请求大小，压缩模块会先将待压缩消息格式化并截断后再发送。

---

## 四、会话持久化一致性

这是 S08 的关键点。

如果只在内存中压缩，而不更新 JSONL，会导致 `/resume` 恢复时重新加载旧的未压缩历史，行为与运行时不一致。

因此 `SessionStore` 新增：

```rust
replace_messages(&[Value])
```

每次压缩成功后，会用压缩后的消息集合重写当前会话文件。

同时，`session.rs` 现在支持 `system` 消息持久化与恢复。

---

## 五、REPL 行为

- 自动压缩：在用户输入后、模型请求前检查
- 手动压缩：`/compact`
- 帮助提示中新增 `/compact`
- 压缩成功后会输出当前估算 token 数

示例：

```text
⚡ Context compacted automatically. Estimated tokens: 2841
```

---

## 六、验证

```bash
cargo test
```

当前结果：`95 passed; 0 failed`

新增覆盖包括：

- `compact::estimate_tokens_uses_content_length`
- `compact::summarize_for_prompt_formats_roles`
- `session::replace_messages_rewrites_file_with_system_message`
