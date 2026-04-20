# S06 · 配置系统

> 在 REPL 增加 `/config`，支持 Theme 与 Tips 两项配置，并持久化到 `settings.json`

**参考设计**: `src/utils/settings/`（Claude Code 配置系统思路）  
**新增文件**: `src/config.rs`  
**修改文件**: `src/main.rs`, `src/repl.rs`

---

## 一、目标

S06 提供用户级可配置能力，先落地两项最常用 UI 配置：

- `Theme`：终端 UI 主题标记（`default` / `light` / `dark`）
- `Tips`：是否在启动时展示详细命令提示

并通过 `/config` 在 REPL 内交互设置，无需手动编辑 JSON。

---

## 二、配置存储

配置复用现有设置文件：

- 优先：`./.localcoder/settings.json`
- 其次：`$HOME/.localcoder/settings.json`

在根对象新增 `ui` 字段：

```json
{
  "ollama": {
    "url": "http://localhost:11434",
    "model": "qwen3.5:4b"
  },
  "ui": {
    "theme": "dark",
    "tips": false
  }
}
```

`config.rs` 采用“读-改-写”策略，保留已有 `ollama` 配置，仅更新 `ui`。

---

## 三、REPL 命令

### 3.1 `/config` 主菜单

```text
⚙️ Config Menu:
  1. Theme
  2. Tips
```

回车可取消。

### 3.2 Theme 子菜单

支持：

- `1` → `default`
- `2` → `light`
- `3` → `dark`

选择后立即写入配置文件并提示保存路径。

### 3.3 Tips 子菜单

支持：

- `1` → `on`
- `2` → `off`

选择后立即持久化。

---

## 四、行为变更

- `repl.rs` 启动时加载 `AppConfig`
- `print_instructions` 根据 `tips` 决定显示“完整提示”还是“简要提示”
- `/help` 增加 `/config` 说明
- 配置更新后，立即刷新并重新显示 Instructions

---

## 五、核心类型

`src/config.rs` 提供：

- `Theme` 枚举：`Default` / `Light` / `Dark`
- `AppConfig { theme, tips }`
- `AppConfig::load(project_dir)`
- `AppConfig::save(project_dir)`

默认值：

- `theme = default`
- `tips = true`

---

## 六、验证

```bash
cargo test
```

当前结果：`92 passed; 0 failed`，包含新增配置模块测试：

- `load_defaults_when_missing_ui`
- `save_and_reload_ui_config`
