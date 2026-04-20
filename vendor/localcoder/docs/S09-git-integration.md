# S09 · Git 集成

> 实现常用 Git 工作流命令：`/diff`、`/review`、`/commit`

**结论**: 当前 S09 是 **REPL 命令层 Git 集成**，不是 **LLM 工具层 Git 集成**。  
也就是说，用户可以手动输入 `/diff`、`/review`、`/commit` 来触发 Git 工作流，但这些能力**没有注册到 `ToolRegistry`**，模型不会像调用 `Read`、`Edit`、`Bash` 那样自主选择 Git 操作。

这样设计的原因很直接：

- 当前版本优先解决“手动可用”的 Git 工作流
- `git commit` / `git add` 属于高风险写操作
- S07 权限系统尚未实现，不适合直接暴露给 LLM 自主调用

因此现阶段的边界是：

- REPL 命令可用
- Git helper 已实现
- LLM 不直接拥有 Git 写权限

后续如果要扩展到工具层，建议优先把只读操作（如 `GitDiff`、`GitStatus`）工具化，把 `GitCommit` 这类写操作留到 S07 落地后再开放。

**参考规划**: `docs/P00-plan.md` S09 章节  
**新增文件**: `src/git.rs`  
**修改文件**: `src/repl.rs`, `src/api.rs`, `src/main.rs`

---

## 一、目标

S09 把 REPL 从“能改代码”推进到“能走一轮基本 Git 工作流”：

- 查看当前差异
- 用模型审查当前 diff
- 自动生成提交信息并执行提交

当前版本先实现最核心的三个命令，不包含 `/branch`。

---

## 二、命令说明

### 2.1 `/diff`

显示当前 Git diff：

- 优先显示 `git diff --staged`
- 若 staged 为空，则回退到 `git diff HEAD`

无变更时提示：

```text
No git changes to show
```

### 2.2 `/review`

将当前 diff 发送给模型，请其输出代码审查结果，重点关注：

1. 潜在 bug 和行为回归
2. 安全风险
3. 测试缺口
4. 可维护性问题

这是“prompt 模式 review”，不执行任何 Git 写操作。

### 2.3 `/commit`

流程：

1. 优先读取 staged diff
2. 若 staged 为空，则读取 working diff
3. 调用模型生成一行 Conventional Commit 风格提交消息
4. 交互确认：`[Y/n/e]`
5. 若当前没有 staged 内容，则自动执行 `git add -A`
6. 执行 `git commit -m "<message>"`

`e` 允许手动编辑提交信息。

---

## 三、实现细节

`src/git.rs` 目前提供：

- `ensure_git_repo`
- `get_staged_diff`
- `get_working_diff`
- `get_combined_diff`
- `has_staged_changes`
- `stage_all`
- `commit`

全部使用非交互式 `git` 命令，避免进入交互界面。

---

## 四、模型调用

`src/api.rs` 新增：

```rust
complete_prompt(prompt, max_tokens)
```

它用于：

- `/review` 生成审查结果
- `/commit` 生成提交消息

这样 S09 不需要额外的专门 API 层。

---

## 五、限制

- 当前 `/commit` 不附加 `Co-Authored-By`
- 当前 `/review` 只审查当前 diff，不做历史基线分析
- 当前未实现 `/branch`
- 长 diff 会做截断后再发送给模型，避免请求过大

---

## 六、验证

```bash
cargo test
```

当前实现包含 Git helper 的基础测试，并通过全部测试。
