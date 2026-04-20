# S05 · 会话持久化

> 实现 JSONL 会话存储 + 恢复机制（`--continue` / `--resume` / REPL `/resume`）

**参考规划**: `docs/P00-plan.md` S05 章节  
**新增文件**: `src/session.rs`  
**修改文件**: `src/main.rs`, `src/repl.rs`

---

## 一、目标

S05 为本地会话增加“可恢复能力”：

- 对话以 JSONL 形式追加存储
- 支持恢复最新会话（`--continue`）
- 支持按会话 ID 恢复（`--resume <id>`）
- 在 REPL 内提供 `/resume` 会话列表选择

---

## 二、存储结构

会话按“项目维度”隔离存储：

```text
~/.localcoder/
└── sessions/
    └── <project-hash>/
        └── <session-id>.jsonl
```

- `project-hash`：基于项目绝对路径哈希生成
- `session-id`：形如 `s<unix_ts>-<pid>`

---

## 三、JSONL 格式

每行一个 JSON 事件，当前支持：

```jsonl
{"type":"user","content":"你好","timestamp":1712345678}
{"type":"assistant","content":"你好！","tool_calls":null,"timestamp":1712345679}
{"type":"tool_result","tool_name":"Read","content":"...","is_error":false,"timestamp":1712345680}
```

加载时会还原为消息数组（`user` / `assistant` / `tool`）供 agent loop 继续使用。

---

## 四、恢复入口

### 4.1 启动参数

```bash
cargo run -- --continue
cargo run -- --resume s1712345678-12345
```

- `--continue`：恢复当前项目最新会话；若无历史会话则进入“未开始会话”状态
- `--resume <id>`：恢复指定会话

### 4.2 REPL 命令 `/resume`

在 REPL 内可随时执行 `/resume`：

1. 列出当前项目会话（最近优先）
2. 列表主标题显示“最后一条 user 消息预览”
3. 附带显示 `[session-id]`
4. 输入编号后加载并打印历史对话

示例：

```text
📚 Available sessions:
  1. 修复 grep_tool 的分页问题... [s1712345678-12345]
  2. 给 bash tool 增加超时控制 [s1712341200-12001]
```

---

## 五、懒创建策略（避免空会话落盘）

为了满足“无对话不保存记录”：

- REPL 启动时不立即创建 session 文件
- 首条 user 消息发送时才创建会话并开始落盘
- 因此仅进入工具界面但未发言，不会产生 JSONL 文件

---

## 六、用户可见行为

- 启动无历史时显示：`Session: (not started yet)`
- 发送首条消息时显示：`Session started: <id>`
- `/resume` 选择后会立即打印 `Loaded conversation history`

---

## 七、验证

```bash
cargo test
```

当前实现已通过测试：

- 参数解析：`--continue` / `--resume`
- SessionStore：创建、追加、加载、最新会话、会话列表
- REPL：恢复后重建历史与命令行为不回归
