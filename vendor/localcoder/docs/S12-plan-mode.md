# S12 · 计划模式

> 实现 `EnterPlanMode` + `TodoWrite`，并补齐最小可用闭环：`ExitPlanMode` + `/plan`

**参考规划**: `docs/P00-plan.md` S12 章节  
**新增文件**: `src/plan.rs`, `src/tools/plan_tools.rs`  
**修改文件**: `src/tools/mod.rs`, `src/engine.rs`, `src/main.rs`, `src/repl.rs`

---

## 一、目标

S12 的核心不是“打印一个计划”，而是让模型在计划阶段真正进入只读模式，先分析、写 todo，再决定何时退出计划模式开始执行。

当前实现包括：

- `EnterPlanMode` 工具：进入计划模式
- `ExitPlanMode` 工具：退出计划模式
- `TodoWrite` 工具：写入或覆盖当前 todo 列表
- `/plan` 命令：查看和手动切换计划模式
- 运行时工具限制：计划模式下禁止写工具

---

## 二、计划模式行为

进入计划模式后，工具白名单会被收窄为：

- `Read`
- `Glob`
- `Grep`
- `EnterPlanMode`
- `ExitPlanMode`
- `TodoWrite`
- `skill_tool`

这意味着 `Edit`、`Write`、`Bash` 在计划模式中会被真实拦截，而不是只靠提示词约束。

如果同时启用了 Skill，Skill 的 `allowed-tools` 会和计划模式白名单做交集，进一步收窄权限。

---

## 三、Todo 存储

计划状态按项目持久化，目录为：

```text
~/.localcoder/projects/<project-hash>/plan/
```

当前会写入：

- `state.json`：结构化状态
- `TODO.md`：渲染后的 Markdown 视图

`TodoWrite` 支持 `pending` / `in_progress` / `completed`，并限制同一时刻最多一个 `in_progress`。

---

## 四、REPL 命令

可直接在 REPL 中手动操作：

```text
/plan
/plan on
/plan off
/plan clear
```

其中：

- `/plan`：查看当前模式和 todo
- `/plan on`：手动进入计划模式
- `/plan off`：退出计划模式
- `/plan clear`：清空 todo 列表

---

## 五、当前边界

这版优先实现“真正可限制工具的计划模式”，因此还有几个边界：

- 还没有“用户确认后自动退出计划模式”的交互拦截
- 还没有单独的计划 UI，只是文本渲染
- todo 目前是整表覆盖，不支持局部 patch 更新

但当前版本已经足够支撑一条完整链路：

1. `EnterPlanMode`
2. `Read/Glob/Grep`
3. `TodoWrite`
4. `ExitPlanMode`
5. 进入正常执行
