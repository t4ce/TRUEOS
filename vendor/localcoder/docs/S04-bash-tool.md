# S04 · 命令执行工具

> 实现 Bash 工具 + 安全检查 — 让 Claude 能执行 shell 命令完成编译、测试、诊断任务

**参考规划**: `docs/P00-plan.md` S04 章节  
**新增文件**: `src/tools/bash_tool.rs`  
**修改文件**: `src/tools/mod.rs`, `src/main.rs`

---

## 一、目标

S03 之后，Claude 已能“读/写/搜”代码，但还不能“运行”代码。S04 引入 `Bash` 工具，补齐执行能力：

- 运行构建与测试（如 `cargo build`, `cargo test`）
- 执行诊断命令（如 `git status`, `ls`, `rg`）
- 在受控前提下支持后台任务

同时加入基础安全策略，拦截明显危险命令。

---

## 二、BashTool（`src/tools/bash_tool.rs`）

### 2.1 Schema

```json
{
  "type": "object",
  "properties": {
    "command": {
      "type": "string",
      "description": "Bash command to run"
    },
    "timeout": {
      "type": "integer",
      "description": "Timeout in milliseconds (default 120000, max 600000)"
    },
    "run_in_background": {
      "type": "boolean",
      "description": "Run command in background and return immediately"
    }
  },
  "required": ["command"]
}
```

### 2.2 执行行为

- 命令通过 `bash -lc <command>` 执行。
- `timeout` 默认 `120000ms`，并限制在 `1..=600000ms`。
- 前台执行返回完整结果：`exit_code` + `stdout` + `stderr`。
- 无输出时返回 `(no output)`，避免空响应。

### 2.3 后台执行

当 `run_in_background: true`：

- 启动子进程并立即返回
- 标准输出/错误重定向到 `null`
- 返回 `pid` 供排查

返回示例：

```
Started background command (pid 12345): sleep 10
```

---

## 三、安全检查（Dangerous Command Blocking）

### 3.1 拦截策略

对输入命令做大小写归一化后，执行字符串模式匹配；命中即拒绝执行。

当前拦截模式包括：

- `rm -rf /`
- `rm -rf ~`
- `:(){:|:&};:`（fork bomb）
- `dd if=/dev/zero of=/dev/`
- `mkfs`
- `shutdown`
- `reboot`
- `> /dev/sda`

### 3.2 错误格式

命中后返回错误：

```
Bash: blocked dangerous command pattern '<pattern>': <reason>
```

> 说明：这是 S04 阶段的“基础拦截”。更细粒度权限系统（Allow Once / Always / Deny）在后续 S07 实现。

---

## 四、工具注册

### 4.1 模块导出

在 `src/tools/mod.rs` 中新增：

- `pub mod bash_tool;`
- `pub use bash_tool::BashTool;`

### 4.2 主程序注册

在 `src/main.rs` 注册：

```rust
registry.register(tools::BashTool);
```

注册后，模型即可通过工具调用触发命令执行。

---

## 五、测试覆盖

`src/tools/bash_tool.rs` 新增 5 个单元测试：

- `bash_executes_simple_command`：验证正常命令执行
- `bash_blocks_dangerous_command`：验证危险命令拦截
- `bash_respects_timeout`：验证超时中断
- `bash_background_returns_pid_message`：验证后台模式返回
- `bash_missing_command_errors`：验证缺失参数报错

执行：

```bash
cargo test
```

当前结果：`82 passed; 0 failed`（包含 BashTool 新增测试）。

---

## 六、文件变更汇总

| 文件 | 类型 | 主要变更 |
|------|------|---------|
| `src/tools/bash_tool.rs` | 新建 | `BashTool` 实现：执行、超时、后台、安全拦截 |
| `src/tools/mod.rs` | 修改 | 声明并导出 `BashTool` |
| `src/main.rs` | 修改 | 注册 `BashTool` |

---

## 七、后续扩展建议

S04 完成的是“可用版”命令执行。下一步可增强：

1. 命令分段解析（`|`, `&&`, `;`）并逐段风险评估  
2. 与 S07 权限系统整合（规则匹配 + 交互确认）  
3. 后台任务管理（任务列表、日志查看、终止）  
4. 工作目录与环境变量白名单控制
