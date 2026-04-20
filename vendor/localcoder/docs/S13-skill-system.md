# S13 · 技能系统

> 实现 `SKILL.md` + `skill_tool`，支持三层加载、技能列表、斜杠调用和运行时工具限制

**新增文件**: `src/skills.rs`, `src/tools/skill_tool.rs`  
**修改文件**: `src/main.rs`, `src/repl.rs`, `src/engine.rs`, `src/tools/mod.rs`  
**内置技能**: `skills/bundled/simplify/`, `skills/bundled/review-diff/`, `skills/bundled/commit-message/`

---

## 一、目标

S13 让 `localcoder` 支持用 Markdown 定义可复用工作流。技能文件使用固定目录结构：

```text
.claude/skills/<name>/SKILL.md
```

当前实现包括：

- 解析 `SKILL.md` frontmatter
- 三层技能加载：Bundled / User / Project
- `skill_tool` 供模型主动调用
- `/skills` 查看可调用技能
- `/<skill-name> [args]` 直接触发用户技能
- `allowed-tools` 在运行时生效

---

## 二、加载优先级

同名技能按后者覆盖前者：

1. Bundled：仓库内置技能
2. User：`~/.localcoder/skills/<name>/SKILL.md`
3. Project：`.claude/skills/<name>/SKILL.md`

项目级技能优先级最高，适合放项目特定流程。

---

## 三、SKILL.md 格式

示例：

```md
---
name: simplify
description: 简化代码或文本
when_to_use: 用户要求简化、降复杂度时
allowed-tools: [Read, Glob, Grep, Edit, Write]
context: inline
user-invocable: true
argument-hint: "[target]"
---

请先理解目标，再做最小必要修改。

$ARGUMENTS
```

已支持的变量替换：

- `$ARGUMENTS`
- `${CLAUDE_SKILL_DIR}`
- `${CLAUDE_SESSION_ID}`

---

## 四、运行方式

模型可以主动调用：

```text
skill_tool { skill: "simplify", args: "src/main.rs" }
```

用户也可以直接输入：

```text
/skills
/simplify src/main.rs
```

执行技能时，`allowed-tools` 会变成当前回合的工具白名单。也就是说，Skill 激活后，后续 Agent 循环里暴露给模型的工具 schema 和实际执行权限都会被收窄。

---

## 五、当前边界

- `context: fork` 已解析，但当前 **不会启动子代理**
- 由于 S11 尚未实现，`fork` 会自动降级为 `inline`
- `paths` 已解析，但当前 **未做自动激活匹配**
- 内置示例技能选择了不和现有 `/commit`、`/review` REPL 命令冲突的名字

当前版本优先保证：

- 技能文件可加载
- 技能能被模型和用户调用
- 工具约束真正生效
