# 密钥存储与 AEAD 加密（M6）

## 概述

Cobrust Studio 使用 **AES-256-GCM + Argon2id** 加密方案来保护您的 API 密钥和端点配置。这意味着您的凭据在写入磁盘之前会被加密，只有在您提供正确的口令时才能解密。

本功能对应 ADR-0007（M6 里程碑），关闭了 Sarah v2 试用审核的第 2 个关口：*"AEAD 加密轮次已上线，环境变量临时方案已移除"*。

---

## 工作原理

### 登录流程

1. 在 `/login` 页面填写：
   - **端点 URL**（如 `https://api.anthropic.com`）
   - **API 密钥**（如 `sk-ant-...`）
   - **模型名称**（如 `claude-opus-4-7`）
   - **口令**（自定义，用于加密密钥，不会存储）
2. 点击保存后，服务器会：
   - 使用 **Argon2id**（m=64 MiB, t=3, p=1）从口令派生一个 32 字节的 AES-256 密钥（约需 500 ms，这是有意为之的，以防暴力破解）
   - 将 `(端点, API 密钥, 模型)` 打包为 JSON
   - 使用 **AES-256-GCM** 加密该 JSON，并附带随机盐值和 nonce
   - 将加密结果存储到 SQLite 的 `session_kv` 表中
   - 将派生的密钥保存在服务器内存中（进程生命周期内有效）

### 存储格式

```
session_kv.value = <16 字节盐值> || <12 字节 nonce> || <AES-GCM 密文+标签>
session_kv.scheme = "aes-gcm-256/argon2id-v1"
```

### 调度流程

当您在 `/agent` 页面发送一条消息时：
1. 服务器从内存中读取派生密钥
2. 从 `session_kv` 读取加密数据块
3. 用内存中的密钥解密，获得明文端点和 API 密钥
4. 将明文密钥传递给 LLM 提供商（调用完成后立即丢弃）

---

## 重启行为

当 `cobrust-studio serve` 进程重启时：
- 内存中的派生密钥**自动清除**
- 磁盘上的加密数据块**保留**
- 下次调度时会返回 `401 no_session`，前端会自动跳转到 `/login`
- 只需重新输入口令即可重新派生密钥（无需再次填写 API 密钥）

---

## 安全性说明

| 威胁场景 | 是否防护 | 说明 |
|---------|---------|------|
| 磁盘冷启动攻击（盗取数据文件） | ✅ 防护 | 没有口令无法解密 |
| 进程内存转储 | ❌ 超范围 | 单用户模式，操作系统级防护是用户责任 |
| 传输层嗅探 | ❌ 超范围 | 使用 TLS 或 127.0.0.1 是用户责任 |
| 多用户 / 多租户隔离 | ❌ 超范围 | v0.3.x 再议 |

---

## 开发者逃生通道（`--dev-api-key`）

对于 CI、Playwright 固件或无界面脚本，可以绕过 `/login` 流程：

```bash
cobrust-studio serve \
  --project /path/to/project \
  --dev-api-key sk-ant-xxx \
  --dev-endpoint https://api.anthropic.com \
  --dev-model claude-opus-4-7
```

服务器启动时会自动注入凭据。**`/login` 页面仍是交互式使用的正式流程**；`--dev-api-key` 是明确的可选项。

也支持环境变量：

```bash
export COBRUST_DEV_API_KEY=sk-ant-xxx
export COBRUST_DEV_ENDPOINT=https://api.anthropic.com
export COBRUST_DEV_MODEL=claude-opus-4-7
cobrust-studio serve --project /path/to/project
```

---

## 性能 — Argon2id 实测耗时

Argon2id 故意慢,交互登录延迟由
`crates/studio-server/src/secret.rs::SessionKey::derive` 里的
`m_cost / t_cost / p_cost` 参数决定。当前值(`-v1` scheme):
`m=64 MiB, t=3, p=1, out=32 B`。

实测(release 构建):

| 硬件 | N=5 中位耗时 |
|---|---|
| Apple M4 (2024 MacBook) | **70 ms** |
| Apple M2 (估算) | ~120 ms |
| GitHub Actions ubuntu-latest runner (2 vCPU 共享) | ~300-400 ms 估算 |
| 老笔记本 (2018 时代 Intel i5) | ~500-800 ms 估算 |

硬上限 2 秒,由 `secret::tests::bench_argon2id_derive` 在 release 模式
下强制。如果你的硬件超时,提个 finding —— `m_cost` 可能需要针对该类型
target 调低。自己跑 bench:

```bash
cargo test --release -p studio-server --lib -- --ignored --nocapture bench_argon2id_derive
```

未来 AEAD 参数修订会把 scheme tag 升到 `-v2`、`-v3` 等。旧 blob 仍然
可读,因为 scheme tag 就是版本锚(见 ADR-0007 §"Storage wire format")。

---

## 轮换 passphrase

**v0.2.x 没有 `POST /api/change-passphrase` 路由** —— 这是计划中的
v0.3.x ADR 项目。在那之前,轮换 passphrase 的步骤是:

1. 停止 server。
2. 删除 session_kv 表里存加密 blob 的行:
   ```bash
   sqlite3 .cobrust-studio/studio.db "DELETE FROM session_kv WHERE key = 'endpoint';"
   ```
3. 启动 server。
4. 访问 `/login` 提交新的 passphrase + endpoint / API key / model。Studio 会用新 passphrase seal 一个新 blob。

这种做法完全忘记旧的加密 blob。还没有"先验证旧密码再轮换"的流程 —— 
直接删除是唯一不需要旧 passphrase 的路径,这对"我忘记 passphrase 了"
的场景很重要。

---

---

## Provider 选择（M7，ADR-0008）

从 v0.3.0 起，`/login` 页面在模型字段和口令字段之间新增了 **Provider** 下拉选项。

### 下拉选项

| 值 | 标签 | 适用场景 |
|----|------|---------|
| `anthropic` | Anthropic API | `api.anthropic.com` 或兼容接口 |
| `openai` | OpenAI 兼容接口（vLLM / DeepSeek / Together / OpenRouter / Groq / Ollama） | 任意 `POST /chat/completions` 端点 |

### URL 自动提示

填写 Base URL 时，表单会自动建议对应的 provider 类型：

- URL 包含 `anthropic.com` → 自动选择 **Anthropic API**。
- URL 非空且不包含 `anthropic.com` → 自动选择 **OpenAI 兼容接口**。

用户可在建议后手动修改下拉选项。

### 请求体格式

`provider_kind` 是 POST 体中的新增字段（向后兼容）：

```json
{
  "endpoint": "https://api.openai.com/v1",
  "api_key": "sk-...",
  "model": "gpt-5",
  "passphrase": "...",
  "provider_kind": "openai"
}
```

省略 `provider_kind`（例如旧版 curl 脚本）时，服务器默认为 `"anthropic"`，
保持与 v0.2.x 的向后兼容性。

### Synthetic provider 不允许通过 /login

`provider_kind: "synthetic"` 会被服务器拒绝，返回 `400 { code: "invalid_provider_kind" }`。
Synthetic provider 是仅限 CLI/开发模式使用的构造（参见下方 `--dev-api-key`），
没有真实的端点和密钥对，通过登录表单提交毫无意义。

### `--dev-api-key` + `--dev-provider-kind`

CLI 标志 `--dev-api-key`（或环境变量 `COBRUST_DEV_API_KEY`）可与
`--dev-provider-kind` 组合使用，在启动时注入 OpenAI 兼容的会话，无需通过
`/login` 界面：

```bash
cobrust-studio serve \
  --project /path/to/project \
  --dev-api-key sk-... \
  --dev-endpoint https://api.openai.com/v1 \
  --dev-model gpt-5 \
  --dev-provider-kind openai
```

省略 `--dev-provider-kind` 时默认为 `anthropic`（保持 v0.2.x 向后兼容性）。

---

## 相关文档

- ADR-0007:密钥存储 AEAD 轮次设计决策
- ADR-0008:多 provider /login (v0.3.0，Phase 2 已实现)
- ADR-0003:认证模型(自定义端点优先)
- `crates/studio-server/src/secret.rs`:实现代码
- `crates/studio-server/src/routes/login.rs`:路由处理
