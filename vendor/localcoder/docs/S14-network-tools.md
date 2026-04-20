# S14 · 网络工具

> 实现 `WebFetch` + `WebSearch`，并补上最小可用的 REPL 手动命令

**参考规划**: `docs/P00-plan.md` S14 章节  
**新增文件**: `src/tools/web_fetch.rs`, `src/tools/web_search.rs`  
**修改文件**: `src/tools/mod.rs`, `src/plan.rs`, `src/main.rs`, `src/repl.rs`

---

## 一、目标

S14 的目标是让模型不只读本地代码，还能读取公开网页和执行联网搜索。

当前实现包括：

- `WebFetch`：抓取公开 HTTP(S) 页面
- `WebSearch`：执行公网搜索并返回结果卡片
- 计划模式中允许使用网络读工具
- 手动命令：
  - `/web <query>`
  - `/fetch <url>`

---

## 二、WebFetch

`WebFetch` 的行为：

1. 校验 URL，只允许 `http` / `https`
2. 阻止 `localhost`、私有网段、loopback 等非公网目标
3. 发送 GET 请求并跟随重定向
4. 对 HTML 页面做文本抽取，输出 markdown-like 文本
5. 支持可选 `prompt` 聚焦相关段落
6. 结果按 `max_chars` 截断，避免上下文爆炸

返回内容会包含：

- 最终 URL
- Content-Type
- 页面标题（如果能提取）
- 页面正文摘录

---

## 三、WebSearch

当前 `WebSearch` 没有接 Anthropic beta API，而是走 **无需 key 的 RSS 搜索接口**，返回：

- 标题
- URL
- 摘要

支持参数：

- `query`
- `limit`
- `domain`

其中 `domain` 会转换成 `site:<domain>` 搜索限制。

---

## 四、REPL 命令

为了方便手动验证，当前也补了两个命令：

```text
/web rust async runtime best practices
/fetch https://docs.rs/tokio/latest/tokio/runtime/index.html
```

这两个命令直接复用工具层逻辑，不依赖模型主动调用。

---

## 五、与计划模式的关系

`WebFetch` 和 `WebSearch` 都属于只读网络工具，因此在 S12 计划模式中被加入白名单。  
这意味着模型在计划阶段可以先联网查资料，但仍然不能使用 `Edit`、`Write`、`Bash`。

---

## 六、当前边界

当前版本有几个明确边界：

- 还没有 S07 权限系统，因此没有“按域名确认授权”交互
- `WebSearch` 不是 Anthropic 内置 search beta，而是通用公网搜索实现
- `WebFetch` 做的是文本抽取，不是完整 DOM 结构保真转换

这版优先保证：

- 工具已可用
- 默认避免明显 SSRF 风险
- 返回内容足够被模型继续消费
