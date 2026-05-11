# Cobrust Studio — 中文文档

AI agent 团队的项目管理与监看控制台。

## 这是什么

Cobrust Studio 是一个独立的控制平面，把 [Cobrust 语言项目](https://github.com/cobrust-lang/cobrust)的方法论 —— ADR 驱动的决策记录、finding 驱动的失败记录、wave 驱动的交付节奏、doc-coverage CI 闸、双语 + agent-doc 三轨文档 —— 包装在一个美观的 Web UI 后面。

用你自己的 LLM 端点 + API key 登录。指向一个 git repo。Studio 编排 AI agent，把每个决策捕获为 ADR、每个失败捕获为 finding、每次 dispatch 记入 token ledger。

## 状态

- **M0 — Scaffold（当前）**：workspace + 5 个 ADR + 5 道 CI 闸全绿。
- **M1 — Backend MVP**：Axum routes、SSE dispatch、LLM router 移植。
- **M2 — Frontend MVP**：SvelteKit UI、4 个核心页面。
- **M3 — Dogfood + 美学打磨**：Studio 用 Studio UI 管理自己的 ADR。
- **M4 — v0.1.0 发布**：单二进制、demo、外部 review。

5 天目标从 M0 到 M4。详见 [`../../../CLAUDE.md`](../../../CLAUDE.md) §6。

## 快速开始（M2 之后）

```bash
./cobrust-studio serve --project ~/my-repo --port 7878
open http://localhost:7878
```

## 架构

```
SvelteKit web (embedded) ──REST + SSE──> studio-server (Axum)
                                              │
                          ┌───────────────────┴───────────────┐
                          ▼                                   ▼
                   studio-store                        studio-router
                   (markdown + SQLite)                 (LLM providers)
```

设计决策见 `../../agent/adr/`。

## 语言

- English: `docs/human/en/`
- 中文：`docs/human/zh/`（当前目录）

## 许可证

Apache-2.0 + MIT 双许可。
