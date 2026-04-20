# S16 · 多平台支持

> **结论：S16 暂不实现。**

当前 `localcoder` 的主运行路径是本地模型（Ollama）优先，API 抽象层也围绕单一 provider 构建。  
在这个阶段引入 AWS Bedrock、Google Vertex AI、Azure Foundry 等多平台适配，会显著扩大接入、鉴权、错误处理和测试矩阵，收益暂时不高。

**参考规划**: `docs/P00-plan.md` S16 章节

---

## 一、规划目标

S16 的目标是让模型后端支持多平台切换，例如：

- Anthropic first-party
- AWS Bedrock
- Google Vertex AI
- Azure AI Foundry

规划中的能力包括：

- `APIProvider` 枚举和 provider 自动检测
- 各平台独立 client 适配器
- 鉴权头和 endpoint 路由切换
- 各平台 token usage / pricing 适配

---

## 二、为什么暂不实现

当前不做 S16，原因主要有三点：

- 现有 `api.rs` 仍是单 provider 设计，直接扩展会把简单链路拉复杂
- 不同 provider 的鉴权方式差异明显，例如 SigV4、GCP Bearer、Azure endpoint/key
- 当前仓库主要目标还是本地工具链和 REPL 体验，不是云平台编排

换句话说，S16 不是“加几个 base URL”就能完成，而是会改动配置结构、错误模型、usage 统计和测试策略。

---

## 三、后续实现前提

如果后续要重启 S16，建议先满足这些前提：

1. 把 `api.rs` 重构成 provider trait / adapter 模式
2. 把配置系统扩展到 provider 级别，而不是只存 Ollama URL/model
3. 明确每个平台的最小支持范围，不要一次性全量铺开

更稳妥的推进顺序是：

1. 先抽象统一请求接口
2. 先做一个云端 provider
3. 再扩展到 Bedrock / Vertex / Foundry

---

## 四、结论

S16 有价值，但当前阶段会明显增加系统复杂度，而且和“本地模型优先”的定位不一致。  
因此这一步先记录为 **暂不实现**，后续等 provider 抽象成熟后再启动。
