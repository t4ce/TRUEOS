# S02 · 文件操作工具

> 实现 Read / Edit / Write — 让 Claude 能够读写代码文件

**参考源码**: `src/tools/FileReadTool/`, `src/tools/FileEditTool/`, `src/tools/FileWriteTool/`  
**新增文件**: `src/tools/file_read.rs`, `src/tools/file_edit.rs`, `src/tools/file_write.rs`  
**修改文件**: `src/tools/mod.rs`, `src/main.rs`, `Cargo.toml`

---

## 一、目标

S01 完成了工具框架，但只有 `EchoTool` 这一个无实际意义的测试工具。S02 实现三个核心文件工具，让 Claude 真正能读写本地代码：

| 工具 | 对应 Claude Code 源码 | 功能 |
|------|---------------------|------|
| `Read` | `FileReadTool.ts` | 读取文件，附带行号，支持分段读取 |
| `Edit` | `FileEditTool.ts` | 精确字符串替换 |
| `Write` | `FileWriteTool.ts` | 创建或覆盖文件 |

---

## 二、ReadTool（`src/tools/file_read.rs`）

### 2.1 Schema

```json
{
  "type": "object",
  "properties": {
    "file_path": { "type": "string", "description": "绝对路径" },
    "offset":    { "type": "integer", "description": "起始行（1-based，可选）" },
    "limit":     { "type": "integer", "description": "最大行数（可选）" }
  },
  "required": ["file_path"]
}
```

### 2.2 输出格式

与 `cat -n` 保持一致 — 每行带右对齐的行号和 tab 分隔：

```
     1	fn main() {
     2	    println!("hello");
     3	}
```

行号从 `offset`（默认 1）开始计数，使 Claude 能精确引用行号进行后续 Edit。

### 2.3 offset / limit 语义

```
offset=3, limit=2 → 读取第 3、4 行，行号显示为 3、4
```

- `offset` 是 **1-based**（与用户界面、编辑器行号一致）
- 内部转换为 0-based 索引：`idx = offset - 1`
- `offset > total_lines` 时返回错误，提示文件实际行数

---

## 三、EditTool（`src/tools/file_edit.rs`）

### 3.1 Schema

```json
{
  "type": "object",
  "properties": {
    "file_path":   { "type": "string" },
    "old_string":  { "type": "string", "description": "要替换的文本（必须在文件中唯一）" },
    "new_string":  { "type": "string", "description": "替换后的文本" },
    "replace_all": { "type": "boolean", "description": "替换所有匹配（默认 false）" }
  },
  "required": ["file_path", "old_string", "new_string"]
}
```

### 3.2 错误条件

| 情况 | 错误信息 |
|------|---------|
| `old_string == new_string` | "no changes to make: old_string and new_string are exactly the same." |
| `old_string` 在文件中找不到 | "string to replace not found in file." |
| 多处匹配且 `replace_all=false` | "found N matches ... but replace_all is false." |

### 3.3 设计意图

**为什么不用行号范围？**  
行号方案在文件被其他工具修改后会偏移失效。字符串匹配更稳定 — 只要上下文足够唯一，就能精确定位，与 Claude Code 源码的 `FileEditTool` 一致。

**多匹配保护**  
如果 `old_string` 不唯一，强制 Claude 提供更多上下文（或明确设 `replace_all: true`），避免意外批量修改。

---

## 四、WriteTool（`src/tools/file_write.rs`）

### 4.1 Schema

```json
{
  "type": "object",
  "properties": {
    "file_path": { "type": "string", "description": "绝对路径（不存在则创建）" },
    "content":   { "type": "string", "description": "写入的完整内容" }
  },
  "required": ["file_path", "content"]
}
```

### 4.2 行为

- 文件不存在 → 创建（返回 "Successfully created ..."）
- 文件已存在 → 覆盖（返回 "Successfully overwrote ..."）
- 父目录不存在 → 自动 `create_dir_all`

### 4.3 与 EditTool 的分工

| 场景 | 用哪个工具 |
|------|---------|
| 修改已有文件的一部分 | `Edit`（更精确，diff 更小）|
| 创建新文件 | `Write` |
| 完全重写文件 | `Write` |

---

## 五、工具注册（`src/tools/mod.rs`）

### 5.1 模块结构

```
src/tools/
├── mod.rs         ← Tool trait + ToolRegistry + EchoTool（S01）
│                    + pub mod file_read/edit/write（S02）
├── file_read.rs   ← ReadTool
├── file_edit.rs   ← EditTool
└── file_write.rs  ← WriteTool
```

### 5.2 注册顺序（`src/main.rs`）

```rust
let mut registry = tools::ToolRegistry::new();
registry.register(tools::EchoTool);   // S01 smoke-test
registry.register(tools::ReadTool);   // S02
registry.register(tools::EditTool);   // S02
registry.register(tools::WriteTool);  // S02
```

`get_schemas()` 对工具名排序，Claude API 收到的顺序固定为：
`Edit → EchoTool → Read → Write`（字母序）。

---

## 六、端到端验证

### 6.1 单元测试（无需 API Key）

```bash
cargo test
```

覆盖点（15 个新增测试）：
- `ReadTool`: 完整读取 / offset+limit / 文件不存在 / 缺少参数
- `EditTool`: 单次替换 / 未找到 / 多匹配拒绝 / replace_all / 同字符串拒绝 / 缺少参数
- `WriteTool`: 创建新文件 / 覆盖已有文件 / 自动建目录 / 缺少参数

### 6.2 集成测试（需要 API Key）

```bash
export ANTHROPIC_AUTH_TOKEN=sk-ant-...
cargo run
```

```
💬 You > 读取 /etc/hosts 的前 5 行

🤖 Claude is thinking...

▶ Tool: Read (toolu_01abc)
     1	##
     2	# Host Database
     3	# localhost is used to configure the loopback interface
     4	127.0.0.1	localhost
     5	255.255.255.255	broadcasthost

文件的前 5 行是 /etc/hosts，显示了本地 hosts 配置...
```

```
💬 You > 在 /tmp/test.txt 中把 "hello" 替换成 "world"

▶ Tool: Edit (toolu_02xyz)
Successfully edited /tmp/test.txt (1 replacement)
```

---

## 七、文件变更汇总

| 文件 | 类型 | 主要变更 |
|------|------|---------|
| `Cargo.toml` | 修改 | 添加 `[dev-dependencies] tempfile = "3"` |
| `src/tools/file_read.rs` | 新建 | `ReadTool` — cat -n 格式，offset/limit 支持 |
| `src/tools/file_edit.rs` | 新建 | `EditTool` — 精确字符串替换，多匹配保护 |
| `src/tools/file_write.rs` | 新建 | `WriteTool` — 创建/覆盖文件，自动建目录 |
| `src/tools/mod.rs` | 修改 | 声明三个子模块，pub use 三个工具 |
| `src/main.rs` | 修改 | 注册 `ReadTool`、`EditTool`、`WriteTool` |

---

## 八、后续阶段的扩展点

S02 完成后，Claude 已经可以：
- 读文件 → 理解代码结构
- 改文件 → 修复 Bug、重构
- 写文件 → 生成新代码

S03 将添加 `GlobTool` 和 `GrepTool`，让 Claude 能在代码库中**搜索**文件和内容，完成真正意义上的代码库导航能力。
