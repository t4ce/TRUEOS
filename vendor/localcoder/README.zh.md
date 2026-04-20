# Localcoder

> 说明：本仓库在开发过程中使用了 Claude Code 和 Codex。如果这让你感到不适，抱歉。

英文版：[README.md](./README.md)

## 📖 简介

Localcoder 是一个基于 Rust 实现的 Claude-like 命令行 AI 助手，当前实现已经包括：

- ✅ 基于 Ollama 的对话、流式响应和单次查询模式
- ✅ 文件、搜索、Bash、Web、LSP 等工具调用运行时
- ✅ 带模型切换、会话恢复、配置菜单和输出风格的交互式 REPL
- ✅ 上下文压缩、Git 工作流、记忆提取、计划模式和技能系统
- ✅ 轻量级（启动快、内存占用低）

> 相比 JavaScript 版本，Rust 版本启动时间快 **10 倍**，内存占用少 **10 倍**。

---

## 📊 实现状态

[`docs/P00-plan.md`](./docs/P00-plan.md) 中的阶段路线图大部分已经落地。当前进度：**20 个阶段中已完成 15 个**。

| 阶段 | 模块 | 状态 | 核心交付物 |
|------|------|------|------|
| S00 | 基础对话循环 | ✅ | REPL、流式 API、单次查询 |
| S01 | 工具系统架构 | ✅ | `Tool` trait、注册表、工具分发循环 |
| S02 | 文件工具 | ✅ | `Read` / `Edit` / `Write` |
| S03 | 搜索工具 | ✅ | `Glob` / `Grep` |
| S04 | 命令执行 | ✅ | 带安全检查的 `Bash` 工具 |
| S05 | 会话持久化 | ✅ | JSONL 会话存储、`--continue`、`--resume`、`/resume` |
| S06 | 配置系统 | ✅ | `settings.json`、`/config`、持久化 UI 配置 |
| S07 | 权限系统 | ❌ | 规则引擎和用户确认尚未实现 |
| S08 | 上下文压缩 | ✅ | 自动压缩、token 估算、`/compact` |
| S09 | Git 集成 | ✅ | `/diff`、`/review`、`/commit` |
| S10 | 记忆系统 | ✅ | 四种记忆类型和自动提取 |
| S11 | 子代理 | ❌ | fork 子代理和 worktree 隔离尚未实现 |
| S12 | 计划模式 | ✅ | `EnterPlanMode`、`ExitPlanMode`、`TodoWrite`、`/plan` |
| S13 | 技能系统 | ✅ | `SKILL.md`、`skill_tool`、`/skills`、`/<skill-name>` |
| S14 | 网络工具 | ✅ | `WebSearch`、`WebFetch`、`/web`、`/fetch` |
| S15 | 费用追踪 | ❌ | token 计费统计和 `/cost` 尚未实现 |
| S16 | 多平台支持 | ❌ | Bedrock / Vertex / Foundry 尚未实现 |
| S17 | MCP 集成 | ❌ | MCP 客户端和多传输支持尚未实现 |
| S18 | 输出样式 | ✅ | 输出样式加载和 `/output-style` |
| S19 | LSP 集成 | ✅ | 基于语言服务器的代码导航 `Lsp` |

---

## 🚀 快速开始

### 1. 安装二进制

**方法一：使用官方安装脚本**

```bash
curl -fsSL https://raw.githubusercontent.com/iamwjun/localcoder/main/install.sh | bash
```

支持平台：
- macOS (arm64 / x86_64)
- Linux (x86_64 / aarch64)

**方法二：手动编译**

```bash
git clone https://github.com/iamwjun/localcoder.git
cd localcoder
cargo build --release
```

---

### 2. 启动 Ollama

确保本地 Ollama 服务已经启动，并且至少拉取了一个模型：

```bash
ollama serve
ollama pull qwen3.5:4b
```

---

### 3. 首次运行

```bash
# REPL 交互模式
localcoder
```

程序启动时会自动检查配置文件：

- 优先读取当前目录的 `.localcoder/settings.json`
- 如果当前目录没有，则读取 `$HOME/.localcoder/settings.json`
- 如果两处都没有，则在当前目录自动创建默认配置

默认配置格式如下：

```json
{
  "ollama": {
    "url": "http://localhost:11434",
    "model": "qwen3.5:4b"
  }
}
```

你也可以手动编辑这个文件，或在 REPL 中使用 `/model` 指令切换模型。

---

### 4. 运行

```bash
# REPL 交互模式
localcoder

# 单次查询（快速测试）
localcoder -- "你好，介绍一下你自己"

# 继续当前项目最近一次会话
localcoder --continue

# 恢复指定会话
localcoder --resume s1712345678-12345
```

---

## 🛠️ 内置工具

当前内置工具包括：

- 文件工具：`Read`、`Edit`、`Write`
- 搜索工具：`Glob`、`Grep`
- Shell 执行：`Bash`
- Web 访问：`WebSearch`、`WebFetch`
- 代码智能：`Lsp`

示例提示词：

```bash
localcoder -- "读取 src/main.rs 的前 5 行"
localcoder -- "在 /tmp/test.txt 中写入'hello world'"
localcoder -- "搜索 process_chunk 函数"
localcoder -- "在项目根目录运行 rg \"SessionStore\""
localcoder -- "抓取 https://www.rust-lang.org/"
```

---

## 📝 REPL 命令

| 命令 | 描述 |
|------|------|
| `/resume` | 列出并恢复历史会话 |
| `/compact` | 手动压缩过长的对话上下文 |
| `/diff` | 显示当前 Git diff |
| `/review` | 使用模型审查当前 Git diff |
| `/commit [title]` | 生成提交信息并执行 git commit |
| `/memory` | 列出已保存的记忆 |
| `/output-style [name]` | 列出或切换输出风格 |
| `/web <query>` | 直接搜索公网内容 |
| `/fetch <url>` | 抓取公开网页 |
| `/plan` | 查看计划模式状态 |
| `/plan on` | 手动启用计划模式 |
| `/plan off` | 手动关闭计划模式 |
| `/plan clear` | 清除持久化 todo 列表 |
| `/skills` | 列出可直接调用的技能 |
| `/<skill-name> [args]` | 直接调用某个技能 |
| `/config` | 配置 UI 设置，如主题和提示开关 |
| `/help` | 显示可用命令列表 |
| `/clear` | 清空对话历史 |
| `/history` | 查看对话历史（JSON 格式） |
| `/model` | 从 `/api/tags` 获取模型列表并切换当前模型，同时更新 `$HOME/.localcoder/settings.json` |
| `/count` | 显示消息数量 |
| `/version` | 显示当前版本 |
| `/quit` | 退出 REPL |
| `/exit` | 退出 REPL |

---

## 📦 项目结构

```text
localcoder/
├── install.sh           # 安装脚本（自动检测平台）
├── Cargo.toml           # Rust 项目配置
├── CHANGELOG.md         # 版本变更日志
├── README.md            # 英文说明
├── README.zh.md         # 中文说明
├── docs/                # 路线图与分阶段实现文档
│   ├── P00-plan.md      # 总体阶段计划
│   └── S00-S19*.md      # 各阶段详细说明
├── examples/            # 示例代码
│   ├── basic.rs          # 基本 API 调用
│   ├── streaming.rs      # 流式响应
│   ├── conversation.rs   # 多轮对话
│   ├── custom_model.rs   # 自定义模型参数
│   └── error_handling.rs # 错误处理
└── src/                 # 源代码
    ├── main.rs           # 程序入口
    ├── api.rs            # Ollama 客户端与流式请求
    ├── compact.rs        # 上下文压缩
    ├── config.rs         # REPL/UI 配置加载与持久化
    ├── engine.rs         # Agent 循环与工具分发
    ├── git.rs            # Git 工作流辅助
    ├── memory.rs         # 记忆提取与存储
    ├── output_style.rs   # 输出风格加载与 prompt 注入
    ├── plan.rs           # 计划模式状态与 todo 管理
    ├── repl.rs           # 交互式 REPL
    ├── session.rs        # JSONL 会话持久化
    ├── skills.rs         # SKILL.md 加载与激活
    ├── tools/            # 内置工具
    ├── services/lsp/     # 语言服务器集成
    └── types.rs          # 共享类型
```

---

## 📋 技术栈

| 组件 | 技术选型 |
|------|----------|
| 异步运行时 | tokio 1.40 |
| HTTP 客户端 | reqwest 0.12 |
| JSON 处理 | serde + serde_json 1.0 |
| 命令行编辑 | rustyline 14.0 |
| 错误处理 | anyhow |
| 终端彩色 | colored 2.1 |

---

## 📈 性能对比

| 指标 | JavaScript | Rust | 提升 |
|------|------------|------|------|
| 启动时间 | ~100ms | ~10ms | **10x** |
| 内存占用 | ~50MB | ~5MB | **10x** |
| 二进制大小 | N/A | 5-8MB | 独立部署 |

---

## 📚 学习价值

通过这个项目，你可以学到：

1. **Rust 异步编程** - tokio 运行时、async/await、Stream 处理
2. **HTTP 客户端** - reqwest、JSON API 调用
3. **系统编程** - 错误处理、所有权、类型安全
4. **CLI 开发** - rustyline REPL、命令行参数
5. **Ollama 集成** - `/api/chat`、`/api/tags`、模型配置管理

---

## 🤖 后续扩展方向

可以基于此项目继续扩展：

- 权限管理与沙箱
- 子代理协作
- Token 费用追踪
- Bedrock / Vertex / Foundry 等多平台后端
- MCP 集成
- GUI 界面（egui/iced）
- WebAssembly（浏览器运行）

---

## 📄 License

MIT License
