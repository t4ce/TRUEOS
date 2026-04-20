# S19 LSP 集成

S19 为 `localcoder` 增加了 Language Server Protocol 能力，模型现在可以通过 `Lsp` 工具做精确代码导航，而不只依赖 `Glob` / `Grep` 的文本搜索。

## 已实现能力

- `go_to_definition`
- `find_references`
- `hover`
- `document_symbols`
- `workspace_symbols`
- `call_hierarchy`，支持 `incoming` / `outgoing`

这些操作会把文件路径、行列号和代码行摘要一起返回，便于模型继续分析或编辑。

## 默认语言服务器

内置默认配置如下：

- Rust: `rust-analyzer`
- TypeScript / JavaScript: `typescript-language-server --stdio`
- Python: `pyright-langserver --stdio`
- Go: `gopls`

`workspace_symbols` 会优先根据当前项目已有文件后缀筛选可用服务器，避免无关语言全部启动。

## 配置方式

可在项目级或用户级 `.localcoder/settings.json` 中增加 `lsp` 配置：

```json
{
  "lsp": {
    "enabled": true,
    "servers": [
      {
        "name": "lua-language-server",
        "command": "lua-language-server",
        "args": [],
        "extensions": [".lua"],
        "language_id": "lua"
      }
    ]
  }
}
```

规则：

- 项目 `.localcoder/settings.json` 优先于 `$HOME/.localcoder/settings.json`
- `lsp.enabled=false` 会禁用 LSP
- 自定义 `servers` 存在时，会覆盖默认内置列表

## 同步机制

工具请求会按需启动语言服务器，并在发送请求前自动同步文件：

- 首次访问文件时发送 `didOpen`
- 文件内容变化时发送 `didChange`
- 每次同步后发送 `didSave`

这样即使文件刚被 `Edit` / `Write` 工具改过，LSP 查询也能看到最新内容。

## 当前边界

- 本阶段只提供给模型使用的 `Lsp` 工具，没有单独的 `/lsp` 交互命令
- 关闭通知目前是进程生命周期级清理，不额外维护独立会话状态
- 是否可用取决于本机是否已安装对应语言服务器
