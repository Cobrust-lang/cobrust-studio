# Cobrust Studio — 中文文档

AI agent 团队的桌面优先项目管理与监看控制台。

## 这是什么

Cobrust Studio 是一个独立的控制平面，把 [Cobrust 语言项目](https://github.com/cobrust-lang/cobrust)的方法论 —— ADR 驱动的决策记录、finding 驱动的失败记录、wave 驱动的交付节奏、doc-coverage CI 闸、双语 + agent-doc 三轨文档 —— 包装在桌面优先的 Tauri shell 里，底层仍复用同一套 SvelteKit UI 与 Rust 后端。

用你自己的 LLM 端点 + API key 登录。指向一个 git repo。Studio 把每个决策捕获为 ADR、每个失败捕获为 finding、每次 dispatch 记入 token ledger。当前的 `/agent` 已经是一个有边界的 agent-turn 时间线：它可以使用内建、受项目根目录约束的工具进行多轮调查，并把迭代与工具结果流式展示到 UI。M9 增加可选 `task_tag` dispatch 元数据，用来按任务类型分析 ledger 成本，同时不污染 provider 请求体。M10 增加可见的 `[ EN | 中 ]` UI 切换，login 与五个核心页面都可在英文/中文之间切换。

## 状态

- **M0 — Scaffold（当前）**：workspace + 5 个 ADR + 5 道 CI 闸全绿。
- **M1 — Backend MVP**：Axum routes、SSE dispatch、LLM router 移植。
- **M2 — Frontend MVP**：SvelteKit UI、4 个核心页面。
- **M3 — Dogfood + 美学打磨**：Studio 用 Studio UI 管理自己的 ADR。
- **M4 — v0.1.0 发布**：单二进制、demo、外部 review。
- **M9T/M9/M10/M11 — v0.4.x 桌面 + ledger 元数据 + i18n + 有边界的
  agent turn**：Tauri shell、持久 session、`task_tag` ledger plumbing、
  zh/en UI 切换，以及 `/agent` 迭代时间线。

5 天目标从 M0 到 M4。详见 [`../../../CLAUDE.md`](../../../CLAUDE.md) §6。

## 快速开始

```bash
# 从源码启动桌面 shell
export COBRUST_STUDIO_PROJECT=$PWD
pnpm --dir web install
pnpm --dir web tauri:dev

# Headless/server 兼容模式
./cobrust-studio serve --project ~/my-repo --port 7878
open http://localhost:7878
```

## 架构

```
Tauri desktop shell ──loopback HTTP──> studio-server (Axum)
        │                                      │
        ▼                                      ▼
  SvelteKit UI                         REST + SSE API
                                               │
                           ┌───────────────────┴───────────────┐
                           ▼                                   ▼
                    studio-store                        studio-router
                    (markdown + SQLite)                 (LLM providers)
```

设计决策见 `../../agent/adr/`。

## 语言

- UI：使用右上角 `[ EN | 中 ]` 切换；Studio 会把选择保存到 `localStorage` 的 `cobrust-studio-locale`。
- English docs: `docs/human/en/`
- 中文文档：`docs/human/zh/`（当前目录）

## 许可证

Apache-2.0 + MIT 双许可。
