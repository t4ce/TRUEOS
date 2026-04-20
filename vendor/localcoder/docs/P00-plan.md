# Claude Code 递进实现计划

> 从零到完整的 Claude Code 分 19 阶段递进实现方案
>
> 每个阶段均可**独立运行**，在上一阶段基础上叠加功能

---

## 📊 总览

```
阶段  名称              核心交付物                  参考源码
─────────────────────────────────────────────────────────────
S00  基础对话循环  ✅   REPL + 流式 API             src/query.ts
S01  工具系统架构  ✅   Tool 接口 + 注册表           src/Tool.ts
S02  文件操作工具  ✅   Read / Edit / Write          src/tools/FileReadTool/
S03  搜索工具     ✅    Glob / Grep                  src/tools/GlobTool/
S04  命令执行     ✅    Bash 工具 + 安全检查         src/tools/BashTool/
S05  会话持久化   ✅    JSONL 存储 + resume          src/utils/sessionStorage.js
S06  配置系统     ✅    settings.json + /config      src/utils/settings/
S07  权限系统     ❌    规则引擎 + 用户确认          src/utils/permissions/
S08  上下文压缩   ✅     autoCompact + token 计数     src/services/compact/
S09  Git 集成    ✅     /commit / /diff / /review    src/utils/git/
S10  记忆系统     ✅     4 种记忆类型 + 自动提取      src/memdir/
S11  子代理      ❌     fork + worktree 隔离         src/tools/AgentTool/
S12  计划模式     ✅     TodoWrite + EnterPlanMode    src/tools/EnterPlanModeTool/
S13  技能系统     ✅     SKILL.md + SkillTool         src/tools/SkillTool/
S14  网络工具     ✅    WebFetch + WebSearch          src/tools/WebFetchTool/
S15  费用追踪     ❌    Token 计数 + /cost 命令       src/cost-tracker.ts
S16  多平台支持   ❌    Bedrock / Vertex / Foundry    src/utils/model/providers.ts
S17  MCP 集成    ❌    MCP 协议客户端 + 5 种传输     src/services/mcp/
S18  输出样式     ✅    output-styles + /output-style  src/services/loadOutputStylesDir.ts
S19  LSP 集成    ✅     语言服务器 + 代码导航         src/services/lsp/
```

---

## S00 · 基础对话循环 ✅ 已完成

> 最小可运行版本，实现与 Claude API 的基础通信

### 交付物
- [x] `ClaudeClient` — HTTP 客户端，流式 SSE 解析
- [x] `ConversationHistory` — 对话历史管理
- [x] `start_repl()` — 交互式命令行界面
- [x] 单次查询模式（`claude <prompt>`）
- [x] `/clear` `/history` `/exit` 基础命令

### 文件结构
```
rust/src/
├── main.rs       ← 入口，路由到 repl 或 one-shot
├── api.rs        ← ClaudeClient, query_streaming()
├── types.rs      ← Message, ConversationHistory
└── repl.rs       ← REPL 主循环, 命令分发
```

### 关键接口
```rust
// 核心循环：send → stream → print → loop
async fn query_streaming(&self, prompt: &str, history: &[Message]) -> Result<String>
```

### 运行方式
```bash
cargo run                    # REPL 模式
cargo run -- "你好"          # 单次查询
```

---

## S01 · 工具系统架构

> 为后续所有工具建立统一的注册、分发、执行框架

### 目标
Claude 的"工具调用"本质是：API 返回 `stop_reason=tool_use` → 本地执行对应函数 → 结果追加为 `tool_result` → 继续循环。此阶段建立这个分发骨架。

### 交付物
- [ ] `Tool` trait — 统一工具接口
- [ ] `ToolRegistry` — 工具注册表
- [ ] `ToolCall` 解析 — 从 API 响应提取工具调用
- [ ] 工具执行循环 — 主循环从单轮扩展为 `stop_reason != tool_use` 的迭代

### 关键设计

**参考源码**: `src/Tool.ts:buildTool()`

```rust
// src/tools/mod.rs
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn schema(&self) -> serde_json::Value;            // JSON Schema，传给 API
    fn execute(&self, input: serde_json::Value) -> Result<String>;  // 同步执行
}

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn register(&mut self, tool: impl Tool + 'static);
    pub fn get_schemas(&self) -> Vec<serde_json::Value>;   // 传给 Claude API
    pub fn execute(&self, name: &str, input: serde_json::Value) -> Result<String>;
}
```

**主循环改造**（参考 `src/query.ts` 的 while 循环）：
```rust
loop {
    let response = api.call_with_tools(&messages, &registry.get_schemas()).await?;

    match response.stop_reason.as_str() {
        "tool_use" => {
            for tool_call in &response.tool_uses {
                let result = registry.execute(&tool_call.name, tool_call.input.clone())?;
                messages.push(tool_result_message(tool_call.id, result));
            }
            // 继续循环
        }
        _ => {
            // 输出文本，结束
            break;
        }
    }
}
```

### 文件结构
```
rust/src/
├── tools/
│   └── mod.rs    ← Tool trait + ToolRegistry
├── api.rs        ← 扩展：支持 tools 参数
└── loop.rs       ← 新文件：提取主循环逻辑
```

### 里程碑验证
```bash
# 注册一个 echo 工具，让 Claude 调用它
> 用 echo_tool 工具打印 "hello"
[工具执行] echo_tool → "hello"
Claude: 已执行，结果是 hello
```

---

## S02 · 文件操作工具

> 实现 Read / Edit / Write — 让 Claude 能够读写代码文件

### 目标
这是最核心的功能。有了文件读写，Claude 就能真正帮你改代码。

### 交付物
- [ ] `ReadTool` — 读取文件（含行号格式，支持 offset/limit）
- [ ] `EditTool` — 精确字符串替换（`old_string → new_string`）
- [ ] `WriteTool` — 创建/覆盖文件
- [ ] 路径安全检查 — 防止越权访问

### 关键设计

**参考源码**: `src/tools/FileReadTool/`, `src/tools/FileEditTool/`

```rust
// src/tools/file_read.rs
pub struct ReadTool;
impl Tool for ReadTool {
    fn name(&self) -> &str { "Read" }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": { "type": "string" },
                "offset": { "type": "number" },   // 起始行（可选）
                "limit": { "type": "number" }     // 最大行数（可选）
            },
            "required": ["file_path"]
        })
    }
    fn execute(&self, input: Value) -> Result<String> {
        let path = input["file_path"].as_str()?;
        let content = read_with_line_numbers(path, offset, limit)?;
        Ok(content)
    }
}

// src/tools/file_edit.rs
pub struct EditTool;
impl Tool for EditTool {
    fn execute(&self, input: Value) -> Result<String> {
        let old = input["old_string"].as_str()?;
        let new = input["new_string"].as_str()?;
        // 要求 old_string 唯一，否则报错
        let count = content.matches(old).count();
        if count != 1 { bail!("old_string 匹配到 {} 处，需唯一", count); }
        let new_content = content.replace(old, new);
        fs::write(path, new_content)?;
        Ok(format!("已编辑: {}", path))
    }
}
```

**输出格式**（参考 `src/tools/FileReadTool/FileReadTool.ts`）：
```
1→  use std::fs;
2→  use anyhow::Result;
3→  
4→  fn main() {
```

### 安全检查
```rust
fn validate_path(path: &str) -> Result<PathBuf> {
    let abs = fs::canonicalize(path)?;
    let cwd = std::env::current_dir()?;
    // 不允许访问 cwd 之外的目录（可配置例外）
    if !abs.starts_with(&cwd) {
        bail!("路径越界: {}", path);
    }
    Ok(abs)
}
```

### 文件结构
```
rust/src/tools/
├── mod.rs         ← Tool trait (从 S01 移入)
├── file_read.rs
├── file_edit.rs
└── file_write.rs
```

### 里程碑验证
```bash
> 读取 src/main.rs 的前 20 行
1→  ...
> 把 "hello" 改成 "world"
已编辑: src/main.rs
```

---

## S03 · 搜索工具

> 实现 Glob / Grep — 让 Claude 能在代码库中定位文件

### 交付物
- [ ] `GlobTool` — 文件名模式匹配（`**/*.rs`）
- [ ] `GrepTool` — 内容搜索（正则 + 文件类型过滤）

### 关键设计

**参考源码**: `src/tools/GlobTool/`, `src/tools/GrepTool/`

```rust
// GlobTool: 使用 glob crate
fn execute(&self, input: Value) -> Result<String> {
    let pattern = input["pattern"].as_str()?;
    let matches: Vec<_> = glob::glob(pattern)?.collect();
    // 按修改时间排序（最近修改的在前）
    Ok(matches.join("\n"))
}

// GrepTool: 调用 ripgrep (rg)
fn execute(&self, input: Value) -> Result<String> {
    let pattern = input["pattern"].as_str()?;
    let mut cmd = Command::new("rg");
    cmd.arg("--line-number").arg(pattern);
    if let Some(glob) = input["glob"].as_str() {
        cmd.arg("--glob").arg(glob);
    }
    let output = cmd.output()?;
    Ok(String::from_utf8(output.stdout)?)
}
```

**输出模式**（参考源码中的 output_mode）：
```
files_with_matches  → 只显示文件路径（默认）
content             → 显示匹配行内容
count               → 显示每文件匹配数
```

### 新增 Cargo 依赖
```toml
glob = "0.3"
# GrepTool 直接 shell out 到 rg，不需要 crate
```

### 里程碑验证
```bash
> 找出所有包含 "TODO" 的 Rust 文件
src/api.rs:45: // TODO: add retry
src/repl.rs:12: // TODO: history
```

---

## S04 · 命令执行

> 实现 Bash 工具 — 让 Claude 能运行 shell 命令

### 交付物
- [ ] `BashTool` — 执行 bash 命令，捕获输出
- [ ] 安全检查 — 危险命令拦截
- [ ] 超时控制 — 默认 120 秒
- [ ] 后台执行 — `run_in_background` 选项

### 关键设计

**参考源码**: `src/tools/BashTool/bashSecurity.ts`, `src/tools/BashTool/destructiveCommandWarning.ts`

```rust
// src/tools/bash.rs
const BLOCKED_PATTERNS: &[&str] = &[
    "rm -rf /",
    "rm -rf ~",
    ":(){:|:&};:",   // fork bomb
    "dd if=/dev/zero of=/dev/",
    "mkfs",
];

fn is_dangerous(cmd: &str) -> Option<&'static str> {
    BLOCKED_PATTERNS.iter()
        .find(|&&p| cmd.contains(p))
        .copied()
}

fn execute(&self, input: Value) -> Result<String> {
    let command = input["command"].as_str()?;
    let timeout_ms = input["timeout"].as_u64().unwrap_or(120_000);

    if let Some(reason) = is_dangerous(command) {
        bail!("危险命令已拦截: {}", reason);
    }

    let output = Command::new("bash")
        .arg("-c")
        .arg(command)
        .timeout(Duration::from_millis(timeout_ms))
        .output()?;

    // 合并 stdout + stderr，带退出码
    Ok(format_output(output))
}
```

**后台执行**（参考 `src/tasks/LocalShellTask/`）：
```rust
if input["run_in_background"].as_bool().unwrap_or(false) {
    let task_id = spawn_background(command);
    return Ok(format!("后台任务已启动，ID: {}", task_id));
}
```

### 里程碑验证
```bash
> 运行 cargo build
   Compiling claude-mini-rust v1.0.0
    Finished dev target
```

---

## S05 · 会话持久化

> 将对话历史写入磁盘，支持跨进程的 `--resume`

### 交付物
- [ ] JSONL 格式存储（每条消息一行）
- [ ] 会话 ID 生成（UUID）
- [ ] `--continue` 恢复最近会话
- [ ] `--resume <id>` 恢复指定会话
- [ ] `/session list` 命令

### 关键设计

**参考源码**: `src/utils/sessionStorage.js`

**存储路径**：
```
~/.claude-mini/
└── sessions/
    └── <project-hash>/
        ├── <session-id-1>.jsonl
        └── <session-id-2>.jsonl
```

**JSONL 格式**（每行一个 JSON）：
```jsonl
{"type":"user","content":"你好","timestamp":1712345678}
{"type":"assistant","content":"你好！","timestamp":1712345679}
{"type":"tool_use","name":"Read","input":{"file_path":"main.rs"}}
{"type":"tool_result","id":"xxx","content":"1→ fn main()..."}
```

```rust
// src/session.rs
pub struct Session {
    pub id: String,
    pub path: PathBuf,
    pub messages: Vec<Message>,
}

impl Session {
    pub fn new(project_dir: &Path) -> Self;
    pub fn load(session_id: &str) -> Result<Self>;
    pub fn load_latest(project_dir: &Path) -> Result<Option<Self>>;
    pub fn append(&mut self, msg: &Message) -> Result<()>;  // 追加写（不重写全文件）
    pub fn flush(&self) -> Result<()>;
}
```

**追加写**（性能关键）：
```rust
// 用户消息：同步写（崩溃恢复）
// 助手消息：追加到缓冲队列
fn append_user_message(&mut self, msg: &Message) -> Result<()> {
    let mut file = OpenOptions::new().append(true).open(&self.path)?;
    writeln!(file, "{}", serde_json::to_string(msg)?)?;
    file.flush()?;    // 立即刷盘
    Ok(())
}
```

### 里程碑验证
```bash
cargo run -- --continue          # 恢复上次对话
cargo run -- --resume abc123     # 恢复指定会话
```

---

## S06 · 配置系统

> 读写 `settings.json`，支持用户自定义行为

### 交付物
- [ ] `Settings` 结构体 — 类型化配置
- [ ] `~/.claude-mini/settings.json` 读写
- [ ] 项目级配置（`.claude/settings.json`）覆盖全局
- [ ] `/config` 命令 — 交互式编辑
- [ ] `/model <name>` 命令
- [ ] 环境变量覆盖支持

### 关键设计

**参考源码**: `src/utils/settings/`

```rust
// src/config.rs
#[derive(Serialize, Deserialize, Clone)]
pub struct Settings {
    pub model: String,
    pub max_tokens: u32,
    pub theme: Theme,
    pub vim_mode: bool,
    pub permission_mode: PermissionMode,
    pub always_allow_rules: Vec<String>,
    pub always_deny_rules: Vec<String>,
    pub hooks: HookConfig,
}

impl Settings {
    // 优先级：env var > 项目配置 > 全局配置 > 默认值
    pub fn load() -> Result<Self> {
        let global = load_file(&global_settings_path())?;
        let project = load_file(&project_settings_path())?;
        Ok(global.merge(project).apply_env())
    }
    pub fn save(&self) -> Result<()>;
}
```

**配置路径**：
```
~/.claude-mini/settings.json        ← 全局配置
.claude/settings.json               ← 项目配置（优先级更高）
ANTHROPIC_API_KEY                   ← 环境变量（最高优先）
```

**`/config` 命令**（简单 TUI）：
```
╔══ 配置 ══════════════════╗
║ model:    claude-opus-4  ║
║ theme:    dark           ║
║ vim_mode: false          ║
╚═══════════════════════════╝
> 修改 model:
```

### 里程碑验证
```bash
> /model claude-sonnet-4-5
已切换模型: claude-sonnet-4-5

> /config
[交互式配置界面]
```

---

## S07 · 权限系统

> 多层授权机制，在工具执行前拦截和审批

### 目标
让用户控制哪些工具调用需要确认，防止意外删除文件等操作。

### 交付物
- [ ] `PermissionChecker` — 规则引擎
- [ ] `always_allow_rules` — 自动允许（无需确认）
- [ ] `always_deny_rules` — 自动拒绝
- [ ] 交互式确认提示（Allow Once / Always / Deny）
- [ ] 钩子脚本支持（`PreToolUse`）

### 关键设计

**参考源码**: `src/utils/permissions/`, `src/utils/hooks/`

```
工具调用请求
     ↓
[1] 钩子脚本 PreToolUse   → approve / deny / modify
     ↓
[2] always_deny_rules     → 匹配则直接拒绝
     ↓
[3] always_allow_rules    → 匹配则直接允许
     ↓
[4] 交互式确认            → 用户选择
     ↓
[5] 工具内部检查          → 路径安全等
     ↓
   执行
```

```rust
// src/permissions.rs
pub enum PermissionDecision {
    Allow,
    AllowAlways,   // 写入 always_allow_rules
    Deny,
}

pub struct PermissionChecker {
    settings: Arc<Settings>,
}

impl PermissionChecker {
    pub fn check(&self, tool_name: &str, input: &Value) -> Result<PermissionDecision> {
        // 1. 运行 PreToolUse 钩子
        if let Some(hook) = &self.settings.hooks.pre_tool_use {
            match run_hook(hook, tool_name, input)? {
                HookResult::Approve => return Ok(Allow),
                HookResult::Deny(msg) => bail!(msg),
                HookResult::Continue => {}
            }
        }
        // 2. always_deny
        for rule in &self.settings.always_deny_rules {
            if matches_rule(rule, tool_name, input) { bail!("已拒绝: {}", rule); }
        }
        // 3. always_allow
        for rule in &self.settings.always_allow_rules {
            if matches_rule(rule, tool_name, input) { return Ok(Allow); }
        }
        // 4. 交互式询问
        ask_user(tool_name, input)
    }
}
```

**规则匹配语法**（参考源码）：
```
"Read"              → 允许所有 Read 调用
"Read:src/**"       → 只允许读 src/ 下的文件
"Bash:git *"        → 只允许 git 命令
"Bash:rm*"          → 拒绝所有 rm 命令
```

**交互提示**：
```
⚠️  工具调用请求
工具: Bash
命令: rm -rf dist/

[a] 允许一次  [A] 始终允许  [d] 拒绝  [?] 帮助
> _
```

### 里程碑验证
```bash
> 删除 dist 目录

⚠️  Bash: rm -rf dist/
[a/A/d] > a
已执行
```

---

## S08 · 上下文压缩

> 对话历史超过 token 限制时自动压缩，维持长会话能力

### 目标
当 `messages[]` 的 token 总量超过阈值时，将旧消息摘要化，只保留最近的完整消息。

### 交付物
- [ ] Token 估算器（字符数 / 4 快速估算）
- [ ] 压缩触发逻辑（阈值检查）
- [ ] `auto_compact()` — 调用 Claude API 生成摘要
- [ ] `compact_boundary` 标记 — 区分摘要和原始消息
- [ ] `/compact` 手动触发命令

### 关键设计

**参考源码**: `src/services/compact/autoCompact.js`

```rust
// src/compact.rs
const TOKEN_THRESHOLD: usize = 160_000;   // 200K 上下文留 40K 空间
const CHARS_PER_TOKEN: usize = 4;

fn estimate_tokens(messages: &[Message]) -> usize {
    messages.iter()
        .map(|m| m.content.len() / CHARS_PER_TOKEN)
        .sum()
}

pub async fn maybe_compact(
    client: &ClaudeClient,
    messages: &mut Vec<Message>,
) -> Result<bool> {
    if estimate_tokens(messages) < TOKEN_THRESHOLD {
        return Ok(false);
    }

    // 找到压缩边界（保留最近 N 条消息不压缩）
    let keep_recent = 10;
    let (to_compress, recent) = messages.split_at(messages.len() - keep_recent);

    // 调用 Claude 生成摘要
    let summary = client.summarize(to_compress).await?;

    // 重组 messages
    *messages = vec![
        Message::system(format!("[对话摘要]\n{}", summary)),
        // compact_boundary 标记
        Message::compact_boundary(),
    ];
    messages.extend_from_slice(recent);

    Ok(true)
}
```

**摘要提示词**（参考源码）：
```
以下是一段对话历史，请生成简洁的摘要，保留：
1. 完成的任务和结果
2. 重要的文件修改
3. 用户的关键偏好和决定
4. 未完成的任务

对话历史：
{messages}
```

### 里程碑验证
```
[对话进行到 40 分钟后]
⚡ 上下文已自动压缩（160K → 12K tokens）
```

---

## S09 · Git 集成

> 实现常用 Git 工作流命令

### 交付物
- [ ] `/commit` — 自动生成提交消息并提交
- [ ] `/diff` — 显示当前差异
- [ ] `/branch` — 分支操作
- [ ] `/review` — 代码审查（Prompt 模式）
- [ ] 提交消息自动生成（让 Claude 分析 diff 生成）

### 关键设计

**参考源码**: `src/utils/git/`, `src/commands/commit.js`

**Git 工具封装**：
```rust
// src/git.rs
pub fn get_diff() -> Result<String> {
    let output = Command::new("git").args(["diff", "HEAD"]).output()?;
    Ok(String::from_utf8(output.stdout)?)
}

pub fn get_staged_diff() -> Result<String> {
    let output = Command::new("git").args(["diff", "--staged"]).output()?;
    Ok(String::from_utf8(output.stdout)?)
}

pub fn commit(message: &str) -> Result<()> {
    Command::new("git")
        .args(["commit", "-m", message])
        .status()?;
    Ok(())
}
```

**`/commit` 命令流程**：
```
1. git diff --staged 获取变更
2. 发送给 Claude: "根据这个 diff 生成简洁的提交消息"
3. 显示建议消息，用户确认 / 修改
4. git commit -m "<message>
   
   Co-Authored-By: Claude"
```

**`/review` 命令**（Prompt 模式）：
```rust
// 直接用 Prompt 类型命令
const REVIEW_PROMPT: &str = "
请审查以下代码变更，关注：
1. 潜在的 bug 和安全问题
2. 代码质量和可读性
3. 测试覆盖

变更内容：
{diff}
";
```

### 里程碑验证
```bash
> /diff
--- a/src/api.rs
+++ b/src/api.rs
@@ -45,3 +45,5 @@

> /commit
建议提交消息: "feat(api): add retry logic for 529 errors"
[确认? y/n] y
已提交
```

---

## S10 · 记忆系统

> 跨会话持久化用户偏好、项目信息和行为反馈

### 交付物
- [ ] 四种记忆类型（user / feedback / project / reference）
- [ ] `~/.claude-mini/projects/<hash>/memory/` 存储
- [ ] `MEMORY.md` 索引文件（自动加载到系统提示）
- [ ] 自动保存触发（用户说"记住..."时）
- [ ] `/memory` 命令（list / add / remove）

### 关键设计

**参考源码**: `src/memdir/`, `src/utils/memory/`

**记忆文件格式**：
```markdown
---
name: 用户技术偏好
description: 用户是 Rust 开发者，偏好函数式编程风格
type: user
---

用户是有 5 年经验的 Rust 开发者。
偏好：
- 函数式编程风格
- 使用 anyhow 做错误处理
- 不喜欢不必要的 clone()
```

**MEMORY.md 索引**（载入系统提示）：
```markdown
- [用户技术偏好](user_tech.md) — Rust 开发者，函数式风格
- [项目架构](project_arch.md) — 微服务，PostgreSQL
- [代码规范反馈](feedback_style.md) — 避免 unwrap()，总写测试
```

**自动注入到系统提示**：
```rust
// 在每次 API 调用前加载记忆
fn build_system_prompt(memory: &MemoryStore) -> String {
    let index = memory.load_index();
    format!("{}

[持久记忆]
{}", BASE_SYSTEM_PROMPT, index)
}
```

**自动检测保存触发**：
```
用户输入包含以下模式时自动保存：
- "记住..."
- "以后总是..."
- "不要再..."
- "我是一个..."
```

### 里程碑验证
```bash
> 记住我不喜欢在 Rust 中用 unwrap()，总用 ? 操作符
已保存记忆: feedback_no_unwrap.md

[新会话]
> 帮我重构这段代码
[Claude 自动使用 ? 而不是 unwrap，无需再提醒]
```

---

## S11 · 子代理

> 让 Claude 能创建子任务代理，实现复杂任务分解

### 目标
主代理可以 fork 一个新的 Claude 实例，给它独立的上下文和目标，最后汇总结果。

### 交付物
- [ ] `AgentTool` — 创建子代理
- [ ] **fork 模式** — 子进程，独立 messages[]
- [ ] **worktree 模式** — 独立 Git 工作树
- [ ] 子代理结果回传给父代理
- [ ] 并发子代理支持

### 关键设计

**参考源码**: `src/tools/AgentTool/forkSubagent.ts`, `src/tools/AgentTool/runAgent.ts`

**AgentTool 接口**：
```rust
// AgentTool 的 JSON Schema
{
    "name": "Agent",
    "properties": {
        "prompt": { "type": "string" },            // 子代理任务
        "subagent_type": {                         // 代理类型
            "enum": ["general-purpose", "explore", "plan"]
        },
        "isolation": {
            "enum": ["none", "worktree"]           // 隔离模式
        }
    }
}
```

**Fork 执行**：
```rust
async fn execute_fork(prompt: &str) -> Result<String> {
    // 1. 启动子进程（复用当前二进制）
    let mut child = Command::new(current_exe()?)
        .arg("--headless")         // 无交互模式
        .arg("--prompt")
        .arg(prompt)
        .stdout(Stdio::piped())
        .spawn()?;

    // 2. 读取结果
    let output = child.wait_with_output().await?;
    Ok(String::from_utf8(output.stdout)?)
}
```

**Worktree 模式**：
```rust
async fn execute_worktree(prompt: &str) -> Result<String> {
    let worktree_path = create_git_worktree()?;

    // 在 worktree 中运行子代理
    let result = Command::new(current_exe()?)
        .arg("--headless")
        .arg("--cwd").arg(&worktree_path)
        .arg("--prompt").arg(prompt)
        .output().await?;

    cleanup_worktree(&worktree_path)?;
    Ok(String::from_utf8(result.stdout)?)
}
```

### 里程碑验证
```bash
> 创建一个探索代理，分析 src/ 目录，生成架构图

[启动子代理: explore]
子代理正在分析...
子代理完成，结果如下：
[架构图]
```

---

## S12 · 计划模式

> 在执行前先制定计划，提升复杂任务的完成率

### 目标
对于多步骤任务，先让 Claude 列出步骤清单并获得用户确认，再逐步执行。

### 交付物
- [ ] `EnterPlanMode` 工具 — 进入只读模式
- [ ] `ExitPlanMode` 工具 — 退出并开始执行
- [ ] `TodoWrite` 工具 — 维护待办清单
- [ ] 计划模式 UI — 显示计划进度
- [ ] `/plan` 命令 — 手动切换

### 关键设计

**参考源码**: `src/tools/EnterPlanModeTool/`, `src/tools/ExitPlanModeTool/`

**计划模式的本质**：在 `plan` 模式下，只允许 `Read`、`Glob`、`Grep` 等只读工具运行，禁止 `Edit`、`Write`、`Bash` 等写操作，直到 `ExitPlanMode` 被调用。

```rust
// src/tools/plan.rs
pub enum PlanMode {
    Off,
    Planning,   // 只允许只读工具
    Executing,  // 恢复正常
}

// 在 PermissionChecker 中集成
fn check_in_plan_mode(&self, tool_name: &str) -> Result<()> {
    if self.plan_mode == PlanMode::Planning {
        let readonly_tools = ["Read", "Glob", "Grep", "WebSearch"];
        if !readonly_tools.contains(&tool_name) {
            bail!("计划模式中禁止执行 {}，请先退出计划模式", tool_name);
        }
    }
    Ok(())
}
```

**TodoWrite 工具**：
```rust
// 维护 .claude-mini/plan.md
pub struct TodoItem {
    pub id: u32,
    pub status: TodoStatus,  // pending / in_progress / completed
    pub text: String,
}

pub struct TodoWrite;
impl Tool for TodoWrite {
    fn execute(&self, input: Value) -> Result<String> {
        let todos: Vec<TodoItem> = serde_json::from_value(input["todos"].clone())?;
        save_todos(&todos)?;
        render_todo_list(&todos)   // 返回 Markdown 格式
    }
}
```

**计划模式流程**：
```
用户: 重构整个认证模块

[进入计划模式]
Claude 调用 EnterPlanMode
Claude 调用 Read, Glob 分析代码
Claude 调用 TodoWrite 制定计划:
  ☐ 1. 分析现有认证代码
  ☐ 2. 设计新接口
  ☐ 3. 实现 JWT 验证
  ☐ 4. 迁移现有调用
  ☐ 5. 编写测试

[等待用户确认]
Claude 调用 ExitPlanMode

[开始执行，todos 逐步更新]
  ✅ 1. 分析现有认证代码
  🔄 2. 设计新接口
  ☐ 3. 实现 JWT 验证
  ...
```

### 里程碑验证
```bash
> /plan
已进入计划模式（只读）

> 重构 api.rs 中的错误处理

[Claude 分析代码，制定计划]
📋 计划:
  ☐ 1. 读取现有代码
  ☐ 2. 识别所有 unwrap() 调用
  ☐ 3. 逐一替换为 ? 操作符

[用户确认]
> 开始执行
[退出计划模式，按步骤执行]
```

---

## S13 · 技能系统

> 让用户和项目通过 Markdown 文件定义可复用的 AI 工作流（Skill）

### 目标
Skills 是 `.claude/skills/<name>/SKILL.md` 文件，Claude 将其识别为 `/skill-name` 斜杠命令。调用时有两种执行方式：**inline**（把技能 Prompt 注入当前对话）和 **fork**（在独立子代理中运行）。

### 交付物
- [ ] `Skill` 结构体 — 解析 SKILL.md YAML frontmatter
- [ ] `SkillRegistry` — 从 bundled / user / project 三层加载
- [ ] `SkillTool` — 验证、权限检查、inline/fork 分发
- [ ] 参数替换 — `$ARGUMENTS`、`${CLAUDE_SKILL_DIR}`、`${CLAUDE_SESSION_ID}`
- [ ] `/skills` 命令 — 列出可用技能
- [ ] 内置 Bundled Skills — commit、review、simplify 示例

### 关键设计

**参考源码**: `src/tools/SkillTool/SkillTool.ts`, `src/skills/loadSkillsDir.ts`

**SKILL.md frontmatter 格式**：
```markdown
---
name: review
description: 审查当前代码变更
when_to_use: 用户要求代码审查时
allowed-tools: [Read, Glob, Grep, Bash]
context: inline          # inline | fork
paths: ["*.rs", "*.ts"]  # 可选：匹配文件时自动激活
user-invocable: true     # 是否出现在 /skills 列表
argument-hint: "[文件路径]"
---

请审查以下代码变更，关注潜在问题和改进建议。

$ARGUMENTS
```

**三层加载优先级**（后面覆盖前面同名 skill）：
```
1. Bundled   — 编译进二进制的内置技能
2. User      — ~/.claude-mini/skills/<name>/SKILL.md
3. Project   — .claude/skills/<name>/SKILL.md  （最高优先级）
```

**Skill 结构体**：
```rust
// src/skills.rs
#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub when_to_use: Option<String>,
    pub allowed_tools: Vec<String>,
    pub context: SkillContext,   // Inline | Fork
    pub paths: Vec<String>,      // glob patterns for conditional activation
    pub user_invocable: bool,
    pub argument_hint: Option<String>,
    pub content: String,         // Markdown body (after frontmatter)
    pub loaded_from: LoadedFrom, // Bundled | User | Project
}

#[derive(Debug, Clone)]
pub enum SkillContext {
    Inline,
    Fork,
}

#[derive(Debug, Clone)]
pub enum LoadedFrom {
    Bundled,
    User,
    Project,
}
```

**SkillRegistry 加载**：
```rust
impl SkillRegistry {
    pub fn load(cwd: &Path) -> Result<Self> {
        let mut skills = HashMap::new();

        // 1. 内置技能（硬编码）
        for skill in bundled_skills() {
            skills.insert(skill.name.clone(), skill);
        }

        // 2. 用户级技能
        let user_dir = home_dir().unwrap().join(".claude-mini/skills");
        load_skills_from_dir(&user_dir, LoadedFrom::User, &mut skills)?;

        // 3. 项目级技能（优先级最高）
        let project_dir = cwd.join(".claude/skills");
        load_skills_from_dir(&project_dir, LoadedFrom::Project, &mut skills)?;

        Ok(Self { skills })
    }

    fn load_skills_from_dir(dir: &Path, from: LoadedFrom, map: &mut HashMap<String, Skill>) {
        for entry in fs::read_dir(dir).ok()?.flatten() {
            let skill_md = entry.path().join("SKILL.md");
            if skill_md.exists() {
                if let Ok(skill) = parse_skill_md(&skill_md, from.clone()) {
                    map.insert(skill.name.clone(), skill);
                }
            }
        }
    }
}
```

**SkillTool 执行**：
```rust
impl Tool for SkillTool {
    fn execute(&self, input: Value) -> Result<String> {
        let skill_name = input["skill"].as_str().ok_or_else(|| anyhow!("missing skill"))?;
        let args = input["args"].as_str().unwrap_or("");

        let skill = self.registry.get(skill_name)
            .ok_or_else(|| anyhow!("skill '{}' not found", skill_name))?;

        // 参数替换
        let content = skill.content
            .replace("$ARGUMENTS", args)
            .replace("${CLAUDE_SKILL_DIR}", &skill_dir_path(skill))
            .replace("${CLAUDE_SESSION_ID}", &self.session_id);

        match skill.context {
            SkillContext::Inline => {
                // 注入为用户消息，进入当前对话
                Ok(format!("[Skill: {}]\n\n{}", skill.name, content))
            }
            SkillContext::Fork => {
                // 复用 S11 的 fork 子代理机制
                execute_fork_agent(&content)
            }
        }
    }
}
```

**内置 Bundled Skills（示例）**：
```rust
fn bundled_skills() -> Vec<Skill> {
    vec![
        Skill {
            name: "commit".into(),
            description: "生成提交消息并创建 git commit".into(),
            context: SkillContext::Fork,
            content: include_str!("../skills/commit.md").into(),
            loaded_from: LoadedFrom::Bundled,
            ..Default::default()
        },
        Skill {
            name: "review".into(),
            description: "审查当前代码变更".into(),
            context: SkillContext::Inline,
            content: include_str!("../skills/review.md").into(),
            loaded_from: LoadedFrom::Bundled,
            ..Default::default()
        },
    ]
}
```

**`/skills` 命令**：
```
/skills
可用技能 (3):
  /commit    [Bundled] 生成提交消息并创建 git commit
  /review    [Bundled] 审查当前代码变更
  /lint      [Project] 运行 lint 并自动修复
```

**条件技能激活**（可选进阶）：
当 `Read`/`Edit` 工具访问某个文件时，如果该文件匹配某个 skill 的 `paths` frontmatter，自动将该 skill 注入到系统提示中。例如，编辑 `.rs` 文件时激活 Rust-specific skill。

### 里程碑验证
```bash
# 使用内置技能
> /commit
[Fork 子代理分析 git diff，生成提交消息，执行 git commit]

# 使用自定义项目技能
# .claude/skills/deploy/SKILL.md 存在
> /deploy production
[Inline 注入技能内容，Claude 执行部署流程]

# 查看可用技能
> /skills
可用技能 (4):
  /commit  [Bundled] 生成提交消息
  /review  [Bundled] 代码审查
  /deploy  [Project] 部署到指定环境
  /lint    [Project] Lint 并修复
```

---

## S14 · 网络工具

> 让 Claude 能抓取网页内容和调用联网搜索

### 交付物
- [ ] `WebFetchTool` — HTTP 请求、HTML→Markdown 转换、域名权限检查
- [ ] `WebSearchTool` — 调用 Anthropic 内置搜索 beta API
- [ ] 域名白名单权限（`WebFetch:domain:github.com`）
- [ ] `/web` 相关基础命令

### 关键设计

**参考源码**: `src/tools/WebFetchTool/`, `src/tools/WebSearchTool/`

**WebFetch 工作流**：
```rust
// src/tools/web_fetch.rs
pub async fn fetch_url(url: &str, prompt: &str) -> Result<String> {
    // 1. 域名权限检查
    let hostname = Url::parse(url)?.host_str().unwrap_or("").to_string();
    check_permission(&format!("domain:{}", hostname))?;

    // 2. HTTP GET，跟随重定向
    let resp = reqwest::get(url).await?;
    let html = resp.text().await?;

    // 3. HTML → Markdown（用 htmd 或 scraper crate）
    let markdown = html_to_markdown(&html);

    // 4. 超长时截断（max 100K chars）
    let truncated = truncate_markdown(&markdown, 100_000);

    Ok(truncated)
}
```

**WebSearch（使用 Anthropic beta API）**：
```rust
// WebSearch 实际上是 Anthropic API 内置工具，在请求中声明即可
// 在 tools 列表中加入：
{
  "type": "web_search_20250305",
  "name": "web_search"
}
// API 返回 BetaWebSearchToolResult，解析并展示 URL 列表
```

**权限规则**：
```
默认：需要每次确认
alwaysAllow: ["WebFetch:domain:docs.rs", "WebFetch:domain:crates.io"]
alwaysDeny:  ["WebFetch:domain:internal.company.com"]
```

### 里程碑验证
```bash
> 帮我查一下 tokio 最新版本的 API 文档
[WebFetch] 正在抓取 docs.rs/tokio...
[Claude 基于文档内容回答]

> 搜索 Rust async runtime 最佳实践 2025
[WebSearch] 返回 5 个搜索结果
[Claude 基于搜索结果总结]
```

---

## S15 · 费用追踪

> 实时统计 Token 用量和 API 调用成本，支持 `/cost` 命令

### 交付物
- [ ] `CostTracker` — 跟踪 input/output/cache tokens
- [ ] 每次 API 调用后更新累计统计
- [ ] `/cost` 命令 — 显示当前会话成本
- [ ] 会话结束时自动打印费用摘要
- [ ] Token 估算（发送前预估 prompt 大小）

### 关键设计

**参考源码**: `src/cost-tracker.ts`, `src/services/tokenEstimation.ts`

**成本模型**（按 Claude 3 Sonnet 定价）：
```rust
// src/cost.rs
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
}

pub struct ModelPricing {
    pub input_per_mtok: f64,        // $ per million tokens
    pub output_per_mtok: f64,
    pub cache_creation_per_mtok: f64,
    pub cache_read_per_mtok: f64,
}

impl TokenUsage {
    pub fn cost_usd(&self, pricing: &ModelPricing) -> f64 {
        (self.input_tokens as f64 * pricing.input_per_mtok
            + self.output_tokens as f64 * pricing.output_per_mtok
            + self.cache_creation_tokens as f64 * pricing.cache_creation_per_mtok
            + self.cache_read_tokens as f64 * pricing.cache_read_per_mtok)
            / 1_000_000.0
    }
}
```

**会话累计**：
```rust
pub struct CostTracker {
    pub total: TokenUsage,
    pub api_calls: u32,
    pub session_start: Instant,
}

impl CostTracker {
    pub fn update_from_response(&mut self, usage: &Usage) {
        self.total.input_tokens += usage.input_tokens as u64;
        self.total.output_tokens += usage.output_tokens as u64;
        self.api_calls += 1;
    }

    pub fn format_summary(&self) -> String {
        let cost = self.total.cost_usd(&current_model_pricing());
        format!(
            "API 调用: {}次 | Tokens: {}in {}out | 费用: ${:.4}",
            self.api_calls, self.total.input_tokens, self.total.output_tokens, cost
        )
    }
}
```

### 里程碑验证
```bash
> /cost
当前会话费用:
  API 调用:   12 次
  输入 Token: 45,231
  输出 Token: 8,102
  缓存命中:   12,000 (节省 $0.0024)
  总费用:     $0.0187
  会话时长:   8 分 32 秒
```

---

## S16 · 多平台支持

> 支持 AWS Bedrock、Google Vertex AI、Azure Foundry，通过环境变量切换

### 交付物
- [ ] `APIProvider` 枚举（firstParty / bedrock / vertex / foundry）
- [ ] 每个 provider 的 client 适配器
- [ ] 环境变量检测 + 自动切换
- [ ] 各 provider 的 token 计数 API 适配

### 关键设计

**参考源码**: `src/utils/model/providers.ts`, `src/utils/model/bedrock.ts`

**Provider 检测**：
```rust
// src/providers.rs
pub enum APIProvider {
    FirstParty,          // 默认，直接调用 api.anthropic.com
    Bedrock,             // AWS Bedrock
    Vertex,              // Google Cloud Vertex AI
    Foundry,             // Azure AI Foundry
}

pub fn get_api_provider() -> APIProvider {
    if env::var("CLAUDE_CODE_USE_BEDROCK").is_ok() {
        APIProvider::Bedrock
    } else if env::var("CLAUDE_CODE_USE_VERTEX").is_ok() {
        APIProvider::Vertex
    } else if env::var("CLAUDE_CODE_USE_FOUNDRY").is_ok() {
        APIProvider::Foundry
    } else {
        APIProvider::FirstParty
    }
}
```

**各 Provider 端点配置**：
```rust
impl APIProvider {
    pub fn base_url(&self) -> String {
        match self {
            Self::FirstParty => "https://api.anthropic.com".into(),
            Self::Bedrock => format!(
                "https://bedrock-runtime.{}.amazonaws.com",
                env::var("AWS_REGION").unwrap_or("us-east-1".into())
            ),
            Self::Vertex => format!(
                "https://{}-aiplatform.googleapis.com/v1/projects/{}/...",
                env::var("CLOUD_ML_REGION").unwrap_or("us-east5".into()),
                env::var("ANTHROPIC_VERTEX_PROJECT_ID").unwrap_or_default()
            ),
            Self::Foundry => env::var("ANTHROPIC_BASE_URL").unwrap_or_default(),
        }
    }

    pub fn auth_header(&self) -> (String, String) {
        match self {
            Self::FirstParty => (
                "x-api-key".into(),
                env::var("ANTHROPIC_API_KEY").unwrap_or_default()
            ),
            Self::Bedrock => {
                // AWS SigV4 签名（用 aws-sigv4 crate）
                ("Authorization".into(), aws_sigv4_sign())
            }
            Self::Vertex => (
                "Authorization".into(),
                format!("Bearer {}", gcp_access_token())
            ),
            Self::Foundry => (
                "Authorization".into(),
                format!("Bearer {}", env::var("AZURE_API_KEY").unwrap_or_default())
            ),
        }
    }
}
```

### 里程碑验证
```bash
# 切换到 AWS Bedrock
CLAUDE_CODE_USE_BEDROCK=1 AWS_REGION=us-east-1 cargo run

# 切换到 Google Vertex
CLAUDE_CODE_USE_VERTEX=1 ANTHROPIC_VERTEX_PROJECT_ID=my-project cargo run

# 默认使用 Anthropic API
ANTHROPIC_API_KEY=sk-... cargo run
```

---

## S17 · MCP 集成

> 实现 Model Context Protocol 客户端，让 Claude 能连接外部工具和数据源

### 目标
MCP 是 Anthropic 主导的开放协议，允许外部服务（文件系统、数据库、浏览器、IDE 等）向 Claude 暴露工具和资源。实现后，用户可以通过配置连接任意 MCP 服务器，Claude 会将其工具视为本地工具调用。

### 交付物
- [ ] `McpClient` — 连接和管理 MCP 服务器连接
- [ ] **stdio 传输** — 本地进程（最常用）
- [ ] **SSE 传输** — HTTP Server-Sent Events 远程服务
- [ ] **HTTP 传输** — Streamable HTTP（新版 MCP）
- [ ] 动态工具注册 — MCP 工具合并到 ToolRegistry
- [ ] 资源访问 — `ListMcpResources` + `ReadMcpResource`
- [ ] `/mcp` 命令 — 列出连接状态
- [ ] MCP 服务器配置（`settings.json` 中的 `mcpServers`）

### 关键设计

**参考源码**: `src/services/mcp/client.ts` (3,348 行), `src/tools/MCPTool/`

**配置格式**：
```json
// ~/.claude/settings.json
{
  "mcpServers": {
    "filesystem": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    },
    "github": {
      "type": "sse",
      "url": "https://mcp.github.com/sse",
      "headers": { "Authorization": "Bearer ghp_xxx" }
    }
  }
}
```

**MCP 连接生命周期**：
```rust
// src/mcp/client.rs
pub struct McpClient {
    pub name: String,
    pub transport: Box<dyn McpTransport>,
    pub tools: Vec<McpToolDef>,      // 服务器暴露的工具
    pub resources: Vec<McpResource>, // 服务器暴露的资源
}

impl McpClient {
    pub async fn connect(config: &McpServerConfig) -> Result<Self> {
        let transport = match config.transport_type {
            TransportType::Stdio => {
                StdioTransport::spawn(&config.command, &config.args).await?
            }
            TransportType::Sse => {
                SseTransport::connect(&config.url, &config.headers).await?
            }
            TransportType::Http => {
                HttpTransport::connect(&config.url).await?
            }
        };

        // MCP 握手：发送 initialize 请求
        let init_result = transport.send(json!({
            "jsonrpc": "2.0",
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {}, "resources": {} },
                "clientInfo": { "name": "claude-mini", "version": "0.1.0" }
            }
        })).await?;

        // 获取工具列表
        let tools_result = transport.send(json!({
            "jsonrpc": "2.0",
            "method": "tools/list"
        })).await?;

        Ok(Self { name: config.name.clone(), transport, tools: parse_tools(tools_result) })
    }

    pub async fn call_tool(&self, name: &str, input: Value) -> Result<String> {
        let result = self.transport.send(json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": { "name": name, "arguments": input }
        })).await?;
        Ok(result["content"][0]["text"].as_str().unwrap_or("").to_string())
    }
}
```

**合并到 ToolRegistry**：
```rust
// MCP 工具包装为本地 Tool
struct McpWrappedTool {
    client: Arc<McpClient>,
    tool_def: McpToolDef,
}
impl Tool for McpWrappedTool {
    fn name(&self) -> &str { &self.tool_def.name }
    fn description(&self) -> &str { &self.tool_def.description }
    fn schema(&self) -> Value { self.tool_def.input_schema.clone() }
    async fn execute(&self, input: Value) -> Result<String> {
        self.client.call_tool(&self.tool_def.name, input).await
    }
}
```

### 里程碑验证
```bash
# 配置 MCP 文件系统服务器后
> /mcp
MCP 服务器状态:
  ✅ filesystem (stdio) — 3 个工具: read_file, write_file, list_directory
  ✅ github     (sse)   — 5 个工具: search_repos, create_issue, ...

> 列出 /tmp 目录下的文件
[MCP filesystem::list_directory 被调用]
```

---

## S18 · 输出样式

> 通过 Markdown 文件定义 Claude 回复的格式风格，类似技能但针对输出格式

### 目标
用户可以在 `.claude/output-styles/` 目录放置 Markdown 文件，定义 Claude 的输出风格（如：简洁模式、详细模式、表格优先、中文输出等）。选择样式后，样式 Prompt 会注入系统提示。

### 交付物
- [ ] `OutputStyleRegistry` — 从用户/项目目录加载 `.md` 样式文件
- [ ] `/output-style` 命令 — 列出和切换输出样式
- [ ] 样式注入到系统提示
- [ ] `keep-coding-instructions` frontmatter 选项

### 关键设计

**参考源码**: `src/services/loadOutputStylesDir.ts`, `src/constants/outputStyles.ts`

**样式文件格式**：
```markdown
<!-- .claude/output-styles/concise.md -->
---
name: concise
description: 简洁回复，避免冗余解释
keep-coding-instructions: true
---

回复时：
- 直接给出答案，不要重复问题
- 代码示例要完整但尽量简短
- 不加"希望这对你有帮助"等客套话
- 错误信息直接说原因和修复方法
```

**两层加载**：
```
1. 用户级: ~/.claude-mini/output-styles/*.md
2. 项目级: .claude/output-styles/*.md  （优先级更高，会覆盖同名样式）
```

**注入系统提示**：
```rust
pub fn apply_output_style(base_prompt: &str, style: &OutputStyle) -> String {
    if style.keep_coding_instructions {
        // 追加到 coding 指令之后
        format!("{}\n\n[输出样式: {}]\n{}", base_prompt, style.name, style.content)
    } else {
        // 替换整个系统提示
        style.content.clone()
    }
}
```

### 里程碑验证
```bash
> /output-style
可用样式 (3):
  default   [内置] 标准 Claude 回复风格
  concise   [Project] 简洁模式，直接给答案
  detailed  [User] 详细模式，含原理解释

> /output-style concise
已切换到输出样式: concise

> 什么是 Rust 的所有权？
[Claude 用简洁风格回答，不做过多铺垫]
```

---

## S19 · LSP 集成

> 连接语言服务器（tsserver、rust-analyzer 等），提供跳转定义、查找引用等代码导航能力

### 目标
Claude 在分析代码时可以调用 LSP（Language Server Protocol）获取精确的语义信息，而不仅靠 Grep/Glob 的文本搜索。当用户问"这个函数在哪里被调用"或"跳转到这个类型的定义"时，Claude 会用 LSP 查询精确位置。

### 交付物
- [ ] `LspServerManager` — 根据文件扩展名启动对应语言服务器
- [ ] `LspClient` — 发送 LSP 请求/接收通知
- [ ] `LSPTool` — 暴露给 Claude 的统一工具（多种操作）
- [ ] 支持操作：go-to-definition、find-references、hover、workspace-symbols、call-hierarchy
- [ ] 文件同步（open/change/save/close 通知）

### 关键设计

**参考源码**: `src/tools/LSPTool/LSPTool.ts`, `src/services/lsp/`

**支持的 LSP 操作**：
```rust
pub enum LspOperation {
    GoToDefinition { file: String, line: u32, character: u32 },
    FindReferences  { file: String, line: u32, character: u32 },
    Hover           { file: String, line: u32, character: u32 },
    DocumentSymbols { file: String },
    WorkspaceSymbols { query: String },
    PrepareCallHierarchy { file: String, line: u32, character: u32 },
    IncomingCalls { item: CallHierarchyItem },
    OutgoingCalls { item: CallHierarchyItem },
}
```

**语言服务器配置**（从用户设置或自动检测）：
```rust
pub struct LspServerConfig {
    pub name: String,
    pub command: String,           // e.g. "rust-analyzer", "typescript-language-server"
    pub args: Vec<String>,
    pub extensions: Vec<String>,   // e.g. [".rs"], [".ts", ".tsx"]
    pub root_uri: String,
}

pub fn default_servers() -> Vec<LspServerConfig> {
    vec![
        LspServerConfig {
            name: "rust-analyzer".into(),
            command: "rust-analyzer".into(),
            args: vec![],
            extensions: vec![".rs".into()],
            root_uri: cwd_uri(),
        },
        LspServerConfig {
            name: "typescript".into(),
            command: "typescript-language-server".into(),
            args: vec!["--stdio".into()],
            extensions: vec![".ts".into(), ".tsx".into(), ".js".into()],
            root_uri: cwd_uri(),
        },
    ]
}
```

**LSP 通信（JSON-RPC over stdio）**：
```rust
// src/lsp/client.rs
pub struct LspClient {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    request_id: AtomicU64,
}

impl LspClient {
    pub async fn send_request<T: Serialize, R: DeserializeOwned>(
        &mut self,
        method: &str,
        params: T,
    ) -> Result<R> {
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });
        // LSP 消息头: "Content-Length: N\r\n\r\n" + body
        let body = msg.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
        self.read_response(id).await
    }
}
```

### 里程碑验证
```bash
> 这个 ClaudeClient 结构体在哪些地方被使用？
[LSP find_references: api.rs:5:12]
结果：
  src/repl.rs:23   — ClaudeClient::new()
  src/repl.rs:67   — client.query_streaming()
  src/main.rs:15   — let client = ClaudeClient::new()

> 跳转到 query_streaming 的定义
[LSP go_to_definition]
src/api.rs:45: pub async fn query_streaming(...)
```

---

## 📊 阶段汇总

| 阶段 | 名称 | 新增代码 | 累计代码 | 开发时间 | 完成后能做什么 |
|-----|------|---------|---------|---------|-------------|
| S00 | 基础对话 ✅ | 450 行 | 450 行 | 已完成 | 基础 AI 对话 |
| S01 | 工具架构 | +200 行 | 650 行 | 2 天 | 框架就绪，可注册任意工具 |
| S02 | 文件操作 | +300 行 | 950 行 | 3 天 | 读写修改代码文件 |
| S03 | 搜索工具 | +150 行 | 1,100 行 | 1 天 | 在代码库中定位文件 |
| S04 | 命令执行 | +250 行 | 1,350 行 | 2 天 | 运行任意命令 |
| S05 | 会话持久化 | +300 行 | 1,650 行 | 3 天 | 跨进程恢复对话 |
| S06 | 配置系统 | +200 行 | 1,850 行 | 2 天 | 自定义行为 |
| S07 | 权限系统 | +350 行 | 2,200 行 | 4 天 | 安全控制工具执行 |
| S08 | 上下文压缩 | +250 行 | 2,450 行 | 3 天 | 支持超长会话 |
| S09 | Git 集成 | +300 行 | 2,750 行 | 3 天 | 自动提交/审查 |
| S10 | 记忆系统 | +350 行 | 3,100 行 | 4 天 | 记住用户偏好 |
| S11 | 子代理 | +500 行 | 3,600 行 | 5 天 | 并行复杂任务 |
| S12 | 计划模式 | +400 行 | 4,000 行 | 4 天 | 结构化任务执行 |
| S13 | 技能系统 | +350 行 | 4,350 行 | 3 天 | 自定义可复用 AI 工作流 |
| S14 | 网络工具 | +300 行 | 4,650 行 | 3 天 | 抓取网页 + 联网搜索 |
| S15 | 费用追踪 | +200 行 | 4,850 行 | 2 天 | 实时查看 Token 用量和成本 |
| S16 | 多平台支持 | +250 行 | 5,100 行 | 3 天 | 接入 Bedrock / Vertex / Foundry |
| S17 | MCP 集成 | +700 行 | 5,800 行 | 7 天 | 连接外部工具和数据源 |
| S18 | 输出样式 | +150 行 | 5,950 行 | 1 天 | 自定义 Claude 回复格式 |
| S19 | LSP 集成 | +600 行 | 6,550 行 | 6 天 | 跳转定义 + 查找引用 |

**总计**: ~7,000 行 Rust 代码，约 63 天（原版: 512,000 行）

---

## 🗂️ 最终文件结构

```
rust/src/
├── main.rs              ← 入口，CLI 参数解析
├── loop.rs              ← 核心 Agent 循环（S01 提取）
├── api.rs               ← ClaudeClient（S00 基础）
├── types.rs             ← Message, Session 等类型
├── config.rs            ← Settings（S06）
├── session.rs           ← JSONL 持久化（S05）
├── permissions.rs       ← 权限检查器（S07）
├── compact.rs           ← 上下文压缩（S08）
├── memory.rs            ← 记忆系统（S10）
├── skills.rs            ← SkillRegistry + Skill 解析（S13）
├── web.rs               ← WebFetch + WebSearch 工具（S14）
├── cost.rs              ← CostTracker + TokenUsage（S15）
├── providers.rs         ← APIProvider 枚举 + 多平台适配（S16）
├── output_styles.rs     ← OutputStyleRegistry（S18）
├── git.rs               ← Git 操作封装（S09）
├── repl.rs              ← REPL 界面（S00 基础）
│
└── tools/
    ├── mod.rs           ← Tool trait + ToolRegistry（S01）
    ├── file_read.rs     ← ReadTool（S02）
    ├── file_edit.rs     ← EditTool（S02）
    ├── file_write.rs    ← WriteTool（S02）
    ├── glob.rs          ← GlobTool（S03）
    ├── grep.rs          ← GrepTool（S03）
    ├── bash.rs          ← BashTool（S04）
    ├── agent.rs         ← AgentTool（S11）
    ├── plan.rs          ← EnterPlanMode / ExitPlanMode / TodoWrite（S12）
    ├── skill_tool.rs    ← SkillTool，inline/fork 分发（S13）
    ├── web_fetch.rs     ← WebFetchTool（S14）
    ├── web_search.rs    ← WebSearchTool（S14）
    ├── mcp_tool.rs      ← MCPTool + McpClient 包装（S17）
    ├── lsp_tool.rs      ← LSPTool，多操作入口（S19）
    └── config_tool.rs   ← ConfigTool（S06）

mcp/
    ├── client.rs        ← McpClient，connect/call_tool（S17）
    ├── transport.rs     ← StdioTransport / SseTransport / HttpTransport（S17）
    └── types.rs         ← McpToolDef / McpResource（S17）

lsp/
    ├── manager.rs       ← LspServerManager（S19）
    ├── client.rs        ← LspClient，JSON-RPC over stdio（S19）
    └── types.rs         ← LspOperation / CallHierarchyItem（S19）
```

---

## 💡 实现建议

### 从 S01 开始的关键原则

1. **先让工具框架跑起来**（S01），用一个假 `EchoTool` 验证整个循环，再一个个加真实工具
2. **S02 是价值最高的单阶段**，完成后就已经是一个实用的 AI 代码助手
3. **S07 权限系统在 S04 前后做均可**，但 Bash 工具在无权限控制时风险较高，建议同步实现
4. **S08 压缩是稳定性保障**，长时间使用前必须加，否则会触 API 限制
5. **S11 子代理依赖 S05 会话持久化**，否则子进程无法通信结果

### 每阶段的验证方式

每阶段完成后都应通过这个问题验证：

> "现在能否独立完成一个有价值的真实任务？"

- S02 后：**能帮我把代码从 Python 风格重构成 Rust 风格吗？**
- S04 后：**能帮我跑测试、查看日志、修复报错吗？**
- S09 后：**能帮我 review 这个 PR 并提交修改吗？**
- S12 后：**能帮我规划并实现一个完整的新功能吗？**
- S13 后：**能让我用 `/deploy` 一键执行自定义部署流程吗？**
- S14 后：**能帮我查最新文档、搜索解决方案吗？**
- S15 后：**这次会话花了多少钱？**
- S17 后：**能连接我的 GitHub MCP 服务器，直接操作 Issue 吗？**
- S19 后：**能告诉我这个函数在哪里被调用，然后帮我重命名吗？**

---

## 🚫 有意排除的系统

> 以下系统在 Claude Code v2.1.88 源码中存在，但**有意不纳入**本实现计划。
> 每项均注明排除原因，以便未来重新评估。

### 1. NotebookEdit 工具

**源码位置**: `src/tools/NotebookEditTool/`

**功能**: 编辑 Jupyter Notebook（`.ipynb`）单元格，支持 replace / insert / delete 操作，通过 `cell_id` 定位单元格。

**排除原因**: 属于 S02 文件操作工具的特化变体，本质是对 JSON 格式 `.ipynb` 文件的结构化写入。如需支持，在 S02 中额外解析 `ipynb` 格式即可，不需要独立阶段。

---

### 2. Feature-flagged 系统（编译时禁用）

以下系统在发布的 npm 包中通过 `feature('FLAG_NAME')` 返回 `false` 被完全禁用，源码存在但代码路径不可达。

#### 2a. KAIROS / 自主代理模式

**源码位置**: `src/commands/assistant.ts`, `src/commands/brief.ts`, `src/tools/BriefTool/`

**功能**: 完全自主的助手模式（`/assistant`），支持主动推送简报（`BriefTool`），Claude 可在无用户触发的情况下发起对话。

**特性标志**: `KAIROS`, `KAIROS_BRIEF`

**排除原因**: Feature-flagged，外部用户代码路径不可达。设计面向 Anthropic 内部产品形态，与通用 CLI 工具定位不同。

#### 2b. DAEMON / 后台守护进程

**源码位置**: `src/tasks/DreamTask/`

**功能**: 后台持续运行的守护进程，允许 Claude 在用户不主动交互时执行预定任务。

**特性标志**: `DAEMON`

**排除原因**: Feature-flagged。守护进程的安全边界、进程管理复杂度超出当前计划范围。

#### 2c. PROACTIVE / 主动行为模式

**源码位置**: `src/commands/btw.ts`, `src/tools/SleepTool/`

**功能**: Claude 可以主动发起操作（`/proactive`），`SleepTool` 允许在工具链中插入延时，配合 `ScheduleCronTool` 实现定时触发。

**特性标志**: `PROACTIVE`

**排除原因**: Feature-flagged。主动行为模式在无充分权限控制的情况下安全风险较高。

#### 2d. CONTEXT_COLLAPSE / 上下文重构压缩

**源码位置**: `src/services/compact/` 中的高级压缩路径

**功能**: S08 autoCompact 之上的第三层压缩策略，彻底重构对话历史结构（而非仅摘要），用于极长会话。

**特性标志**: `CONTEXT_COLLAPSE`

**排除原因**: Feature-flagged。S08 的 autoCompact 已足够处理常规长度会话。

#### 2e. VOICE_MODE / 语音模式

**源码位置**: `src/voice.ts`, `src/services/voiceStreamSTT.ts`, `src/services/voiceKeyterms.ts`

**功能**: 语音输入（STT）和语音输出（TTS），支持实时语音对话。

**特性标志**: `VOICE_MODE`

**排除原因**: Feature-flagged + 依赖 native 音频捕获模块（`vendor/audio-capture`，仅有 stub，无编译产物）。

#### 2f. COORDINATOR_MODE / 多代理协调

**源码位置**: `src/coordinator/coordinatorMode.ts`

**功能**: 专用的协调者代理，管理多个子代理的任务分配、进度汇报和结果聚合，超越 S11 的 fork 模式。

**特性标志**: `COORDINATOR_MODE`

**排除原因**: Feature-flagged。S11 子代理已满足并行任务需求，协调者模式是更高阶的多代理编排，复杂度极高。

#### 2g. HISTORY_SNIP / 快照压缩

**源码位置**: `src/services/compact/` snip 路径, `src/commands/force-snip.ts`

**功能**: 在会话中插入"快照点"，之前的对话历史可被裁剪但保留摘要引用。

**特性标志**: `HISTORY_SNIP`

**排除原因**: Feature-flagged。S08 的 autoCompact 策略已覆盖上下文管理需求。

---

### 3. 运营/基础设施系统

#### 3a. 遥测与分析

**源码位置**: `src/services/analytics/`, `src/services/diagnosticTracking.ts`

**功能**: 双轨遥测（Anthropic 1st-party + Datadog APM），收集环境指纹、仓库哈希、工具调用等数据，每小时轮询远程配置端点。

**排除原因**: 克隆版本中不应复制遥测系统。详细分析见 `docs/en/01-telemetry-and-privacy.md`。

#### 3b. OAuth 认证系统

**源码位置**: `src/services/oauth/`

**功能**: OAuth 2.0 + PKCE 流程，用于 claude.ai 账号登录（`/login` 命令）和 MCP 服务器的 OAuth 授权（`McpAuthTool`）。

**排除原因**: 依赖浏览器跳转和本地 HTTP 监听器。S17 MCP 集成中的 OAuth 可简化为直接配置 Bearer Token，无需完整 OAuth 流程。如需接入 claude.ai 账号，直接使用 `ANTHROPIC_API_KEY` 环境变量即可。

#### 3c. Bridge 模式

**源码位置**: `src/bridge/`（30+ 文件）

**功能**: 通过 JWT + HTTP 长轮询将 CLI 连接到 Claude Desktop 或 claude.ai 网页版，实现会话同步和跨端协作（`/desktop`, `/teleport`, `/bridge`）。

**排除原因**: 高度依赖 Anthropic 云端基础设施（特定 API 端点、JWT 签发），且属于产品生态集成而非核心 AI 功能。

#### 3d. 插件安装管理器

**源码位置**: `src/services/plugins/PluginInstallationManager.ts`, `src/plugins/`

**功能**: 从 npm 动态安装插件包，管理插件生命周期（安装/更新/卸载），插件可扩展工具、命令和输出样式。

**排除原因**: 动态 npm 安装在 Rust 二进制中需要依赖 Node.js 环境，与实现目标不符。S13 Skills 系统已提供轻量级的功能扩展机制（文件即插件）。

#### 3e. Remote 会话管理

**源码位置**: `src/remote/`, `src/tasks/RemoteAgentTask/`

**功能**: 在远程容器中运行代理（最高隔离级别），通过 WebSocket 管理会话，支持云端扩展。

**排除原因**: 依赖 Anthropic 云端调度基础设施，无法在本地独立实现。S11 的 worktree 模式已提供高隔离级别的本地代理。

---

### 4. 条件实现项（未来可扩展）

以下功能技术上可实现，但因优先级/复杂度比，建议在 S19 完成后按需添加：

| 功能 | 源码位置 | 说明 |
|------|---------|------|
| NotebookEdit | `src/tools/NotebookEditTool/` | S02 的 `.ipynb` 扩展，30 分钟可实现 |
| ScheduleCron | `src/tools/ScheduleCronTool/` | 会话内定时任务，依赖 PROACTIVE flag |
| PowerShell | `src/tools/PowerShellTool/` | Windows 用户的 Bash 替代，S04 的变体 |
| REPLTool | `src/tools/REPLTool/` | VM 沙箱中执行 JS/Python，需要隔离运行时 |
| AskUserQuestion | `src/tools/AskUserQuestionTool/` | S01 后可轻松添加，为 Claude 提供交互确认能力 |
