# S18 · 输出样式

> 实现 `output-styles` + `/output-style`，支持样式加载、持久化切换和 system prompt 注入

**参考规划**: `docs/P00-plan.md` S18 章节  
**新增文件**: `src/output_style.rs`, `output-styles/bundled/concise.md`, `output-styles/bundled/detailed.md`  
**修改文件**: `src/config.rs`, `src/main.rs`, `src/repl.rs`

---

## 一、目标

S18 让用户可以通过 Markdown 文件定义回答风格，并在运行时切换。  
当前实现包括：

- 输出样式加载器
- 用户级 / 项目级样式覆盖
- `AppConfig` 持久化当前选中样式
- `/output-style` 查看和切换样式
- 在 system prompt 中注入当前样式

---

## 二、加载规则

当前按以下优先级加载同名样式：

1. Built-in：内置样式
2. User：`~/.localcoder/output-styles/*.md`
3. Project：`.claude/output-styles/*.md`

项目级优先级最高，可覆盖用户级和内置样式。

当前内置样式包括：

- `default`
- `concise`
- `detailed`

---

## 三、样式文件格式

示例：

```md
---
name: concise
description: 简洁模式，直接给答案
keep-coding-instructions: true
---

回复时：
- 先给结论，再补必要细节
- 避免重复问题
- 不要写空泛客套话
```

已支持 frontmatter：

- `name`
- `description`
- `keep-coding-instructions`

---

## 四、命令用法

查看可用样式：

```text
/output-style
```

切换样式：

```text
/output-style concise
/output-style detailed
/output-style default
```

切换结果会写回当前配置文件，因此下次启动仍然生效。

---

## 五、注入方式

当前实现会把样式内容注入到 system prompt。

- `keep-coding-instructions: true`
  - 保留已有运行时提示，再附加输出样式说明
- `keep-coding-instructions: false`
  - 让样式提示排在更前面，优先级更高

注意：这版不会丢掉记忆、技能、计划状态等运行时上下文，只调整样式提示的注入顺序。

---

## 六、当前边界

- 当前没有单独的“样式管理 UI”，只支持 `/output-style`
- 当前只支持 Markdown 文件，不支持更复杂的模板逻辑
- `default` 是内置默认样式，不额外注入 prompt

这版优先保证：

- 样式可以被加载
- 样式切换可持久化
- 后续回答风格确实会受到影响
