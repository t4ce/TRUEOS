# S03 · 搜索工具

> 实现 Glob / Grep — 让 Claude 能在代码库中定位文件和搜索内容

**参考源码**: `src/tools/GlobTool/`, `src/tools/GrepTool/`  
**新增文件**: `src/tools/glob_tool.rs`, `src/tools/grep_tool.rs`  
**修改文件**: `src/tools/mod.rs`, `src/main.rs`, `Cargo.toml`

---

## 一、目标

S02 让 Claude 能读写文件，但**不知道该读哪些文件**。S03 实现两个搜索工具，完成代码导航闭环：

| 工具 | 对应 Claude Code 源码 | 功能 |
|------|---------------------|------|
| `Glob` | `GlobTool.ts` | 按文件名模式查找文件 |
| `Grep` | `GrepTool.ts` | 按内容正则搜索文件 |

典型工作流：`Glob` 定位文件 → `Read` 读取 → `Edit` 修改。

---

## 二、GlobTool（`src/tools/glob_tool.rs`）

### 2.1 Schema

```json
{
  "type": "object",
  "properties": {
    "pattern": { "type": "string", "description": "Glob 模式，如 **/*.rs" },
    "path":   { "type": "string", "description": "搜索目录（默认 CWD）" }
  },
  "required": ["pattern"]
}
```

### 2.2 行为

1. 构建 `base_dir/pattern` 完整路径
2. 使用 `glob` crate 遍历匹配的文件
3. 按修改时间排序（最新优先）
4. 超过 100 条结果时截断并提示
5. 绝对路径转换为相对路径（节省 token）
6. 无匹配返回 `"No files found"`

### 2.3 依赖

```toml
glob = "0.3"
```

Rust 标准的 glob 实现，支持 `**` 递归、`*` 通配符、`?` 单字符。

### 2.4 结果截断

与 Claude Code 源码一致，硬上限 100 条。超出时追加提示行：

```
src/main.rs
src/api.rs
...（共 100 条）
(Results are truncated. Consider using a more specific path or pattern.)
```

---

## 三、GrepTool（`src/tools/grep_tool.rs`）

### 3.1 Schema

```json
{
  "type": "object",
  "properties": {
    "pattern":     { "type": "string" },
    "path":        { "type": "string" },
    "glob":        { "type": "string", "description": "文件名过滤，如 *.ts 或 *.{rs,toml}" },
    "output_mode": { "type": "string", "enum": ["files_with_matches", "content", "count"] },
    "-i":          { "type": "boolean", "description": "大小写不敏感" },
    "-C":          { "type": "integer", "description": "上下文行数" },
    "-B":          { "type": "integer", "description": "匹配前行数" },
    "-A":          { "type": "integer", "description": "匹配后行数" },
    "type":        { "type": "string", "description": "文件类型: rust/js/ts/py/go/java..." },
    "head_limit":  { "type": "integer", "description": "限制输出条数，默认 250，0=无限" },
    "offset":      { "type": "integer", "description": "跳过前 N 条" },
    "multiline":   { "type": "boolean", "description": "多行模式（. 匹配换行）" }
  },
  "required": ["pattern"]
}
```

### 3.2 三种输出模式

| 模式 | 输出 | 用途 |
|------|------|------|
| `files_with_matches`（默认） | 匹配的文件名列表 | 快速定位哪些文件包含关键词 |
| `content` | 匹配行 + 行号 + 可选上下文 | 查看具体匹配内容和位置 |
| `count` | 每文件匹配数 + 总计 | 统计匹配分布 |

#### content 模式格式

```
>src/api.rs:42:pub async fn query_streaming(
 src/api.rs:43:    prompt: &str,
 src/api.rs:44:    history: &[Message],
```

`>` 标记匹配行，空格标记上下文行。

### 3.3 依赖

```toml
ignore = "0.4"   # ripgrep 的文件遍历库，自动尊重 .gitignore
regex = "1"       # 正则引擎
```

**为什么用 `ignore` 而不是 `walkdir`？**  
`ignore` 是 ripgrep 的核心文件遍历库，自动跳过 `.gitignore` 中列出的文件和 `.git` 等版本控制目录。这避免了搜索 `node_modules/`、`target/` 等无关目录，与 Claude Code 使用 ripgrep 的行为一致。

### 3.4 文件类型过滤

`type` 字段映射到文件扩展名：

| type | 扩展名 |
|------|-------|
| `rust` | `.rs` |
| `js` | `.js`, `.mjs`, `.cjs` |
| `ts` | `.ts`, `.tsx` |
| `py` | `.py` |
| `go` | `.go` |
| `java` | `.java` |

### 3.5 Glob 过滤

`glob` 字段支持：
- 简单模式：`*.rs`
- 逗号分隔：`*.rs, *.toml`
- 大括号展开：`*.{rs,toml}`

大括号展开的实现：先检测 `{...}`，提取前缀/后缀/内部选项，展开后逐个匹配。

### 3.6 分页

`head_limit` + `offset` 实现结果分页：

```
# 前 250 条（默认）
Grep { pattern: "TODO", head_limit: 250 }

# 下一页
Grep { pattern: "TODO", offset: 250, head_limit: 250 }

# 全部结果
Grep { pattern: "TODO", head_limit: 0 }
```

默认 `head_limit: 250`，与 Claude Code 一致。`head_limit: 0` 表示不限。

### 3.7 上下文行

`-C` 优先级高于 `-B`/`-A`：

```
-C 3       → 前后各 3 行
-B 2 -A 5  → 前 2 行，后 5 行
-C 3 -B 1  → 上下文 3 行（-B 被忽略）
```

---

## 四、VCS 目录排除

两个工具都排除版本控制目录，与 Claude Code 一致：

```rust
const SKIP_DIRS: &[&str] = &[".git", ".svn", ".hg", ".bzr"];
```

GlobTool 通过不匹配目录实现；GrepTool 通过 `ignore` crate 的 `add_ignore()` 方法实现。

---

## 五、路径相对化

两个工具都把绝对路径转换为相对路径（相对于 CWD），节省 token：

```rust
fn relativize(path: &Path) -> String {
    let cwd = env::current_dir().unwrap_or_default();
    if let Ok(rel) = path.strip_prefix(&cwd) {
        rel.to_str().unwrap_or("?").to_string()
    } else {
        path.to_str().unwrap_or("?").to_string()
    }
}
```

---

## 六、工具注册

```rust
// src/main.rs
registry.register(tools::GlobTool);
registry.register(tools::GrepTool);
```

注册后，`get_schemas()` 按 name 排序输出：
`Edit → EchoTool → Glob → Grep → Read → Write`

---

## 七、测试覆盖

### 7.1 GlobTool（5 个测试）

| 测试 | 验证点 |
|------|--------|
| `glob_finds_matching_files` | `**/*.rs` 匹配 .rs 文件，不匹配 .ts |
| `glob_no_matches_returns_not_found` | 无匹配返回 "No files found" |
| `glob_errors_on_nonexistent_directory` | 不存在的目录报错 |
| `glob_missing_pattern_errors` | 缺少 pattern 报错 |
| `glob_truncates_at_max_results` | 超过 100 条截断 + 提示 |

### 7.2 GrepTool（10 个测试）

| 测试 | 验证点 |
|------|--------|
| `grep_files_with_matches` | 按内容匹配文件名 |
| `grep_content_mode` | 显示匹配行 + 行号 + 标记 |
| `grep_count_mode` | 统计匹配数 |
| `grep_case_insensitive` | `-i: true` 忽略大小写 |
| `grep_type_filter` | `type: "rust"` 只搜索 .rs 文件 |
| `grep_no_matches` | 无匹配返回 "No files found" |
| `grep_invalid_regex_errors` | 非法正则报错 |
| `grep_missing_pattern_errors` | 缺少 pattern 报错 |
| `grep_context_lines` | `-C: 1` 显示上下文行 |
| `glob_match_brace` | 大括号展开 `*.{rs,toml}` |
| `glob_match_comma_separated` | 逗号分隔 `*.rs, *.ts` |
| `glob_match_extension` | 简单扩展名 `*.rs` |

---

## 八、文件变更汇总

| 文件 | 类型 | 主要变更 |
|------|------|---------|
| `Cargo.toml` | 修改 | 添加 `glob = "0.3"`, `ignore = "0.4"`, `regex = "1"` |
| `src/tools/glob_tool.rs` | 新建 | `GlobTool` — glob 模式匹配，mtime 排序，100 条截断 |
| `src/tools/grep_tool.rs` | 新建 | `GrepTool` — 三种输出模式，.gitignore 支持，上下文/分页/类型过滤 |
| `src/tools/mod.rs` | 修改 | 声明 `glob_tool`/`grep_tool` 模块，pub use 两个工具 |
| `src/main.rs` | 修改 | 注册 `GlobTool`、`GrepTool` |

---

## 九、后续阶段的扩展点

S03 完成后，Claude 具备完整的文件导航能力：

```
Glob → 定位文件
Read → 理解内容
Edit → 精确修改
Write → 创建/重写
Grep → 搜索内容
```

S04 将添加 `BashTool`，让 Claude 能执行任意 shell 命令，完成测试运行、Git 操作等需要进程执行的任务。