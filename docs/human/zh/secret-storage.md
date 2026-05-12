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

## 跨重启持久化会话 (M8, ADR-0009)

默认情况下，内存中的 `SessionKey` 在 **二进制每次重启时都会被丢弃** ——
您需要重新通过 `/login` 输入口令以重新派生。对于本地开发工作流来说
这没问题（在 Apple M4 上约 70 ms 即可重新派生）。

但对于 **长期运行的部署**（systemd 单元、Docker 容器、无人值守的
服务器重启）来说，这种摩擦会累积。从 v0.4.0 开始，`cobrust-studio
serve` 支持可选的 `--persist-session` 标志，将口令包装到三种后端
之一。下次启动时，会话会自动解锁 —— 无需再走 `/login` 流程。

### 三种模式

| 模式 | CLI | 静态存储 | 信任模型 |
|------|-----|---------|---------|
| `none`（默认） | `--persist-session=none` | 不存任何东西 —— 进程消失，`SessionKey` 也消失 | v0.3.0 基线；每次重启重新输入口令 |
| `keychain` | `--persist-session=keychain` | OS 钥匙串（macOS Keychain / freedesktop secret-service / Windows Credential Manager 经 DPAPI） | 冷盘窃取防护最强；口令存在用户作用域钥匙串中，不会落盘 |
| `file` | `--persist-session=file --persist-session-file=/path/to/passphrase` | `0600` 模式明文文件 | 系统管理员友好；适用于没有钥匙串的环境（Docker、无 D-Bus 的 Linux）；与 `--dev-api-key` 同样的信任模型（操作员可信） |

默认是 `none` —— 仅在显式启用时才会持久化。现有 v0.3.x 部署
在不传递该标志时**行为不会改变**。

### 快速开始 —— 钥匙串后端（开发笔记本、单用户服务器）

```bash
cobrust-studio serve \
  --project /path/to/project \
  --persist-session=keychain
```

第一次 /login 时，口令会被写入 OS 钥匙串，service 为 `cobrust-
studio`、username 为 `session-passphrase`。下次启动时，服务器
读取回来，重新派生会话密钥，无需访问 `/login` 即可完成认证。

清除方式（例如交还笔记本、轮换凭据）：

```bash
# macOS:
security delete-generic-password -s cobrust-studio -a session-passphrase
# Linux (gnome-keyring / KWallet):
secret-tool clear service cobrust-studio username session-passphrase
# Windows (PowerShell):
cmdkey /delete:cobrust-studio
# 或通过 API:
curl -X POST "http://localhost:7878/api/logout?purge=true"
```

### 快速开始 —— 文件后端（Docker、systemd、无 D-Bus 的 headless）

```bash
cobrust-studio serve \
  --project /path/to/project \
  --persist-session=file \
  --persist-session-file=/etc/cobrust-studio/passphrase
```

文件在第一次 `/login` 时以 `0600` 模式创建（仅 Unix —— Windows
跳过此检查，Windows 上请优先选择 keychain）。后续启动时读取文件，
服务器自动解锁。

清除方式：

```bash
rm /etc/cobrust-studio/passphrase
# 或通过 API:
curl -X POST "http://localhost:7878/api/logout?purge=true"
```

环境变量等价形式：

```bash
export COBRUST_PERSIST_SESSION=file
export COBRUST_PERSIST_SESSION_FILE=/etc/cobrust-studio/passphrase
cobrust-studio serve --project /path/to/project
```

### 安全权衡表

| 威胁 | `none` | `keychain` | `file` |
|------|--------|------------|--------|
| 冷盘窃取（窃取 `.cobrust-studio/db`） | 受保护（需要口令） | 受保护（钥匙串中的口令是用户级隔离，不在磁盘镜像上） | **弱化**（文件在磁盘上；攻击者拿到磁盘+文件 = 完全解锁） |
| 系统管理员 / 同等 OS 用户攻击者（与服务器同用户） | 超范围（= ADR-0007 §"Threat model" #3） | 超范围（同信任级别可读取钥匙串） | 超范围（同信任级别可读取 0600 文件） |
| 容器逃逸 | 取决于部署 | 最强 —— 钥匙串绑定到宿主机 | 最差 —— 文件在容器文件系统中 |
| Docker 容器重启 + 持久化文件挂载 | N/A | N/A（宿主机钥匙串通常不可见） | **生效** —— 该模式的核心场景 |
| 操作员忘记口令，钥匙串/文件中无密钥 | 重新 /login | 重新 /login（钥匙串无法恢复已遗忘的口令） | 重新 /login |

**核心安全属性**：仅磁盘窃取仍然能被 keychain 后端防御（口令不会
出现在磁盘镜像中）。文件后端是为没有钥匙串的部署设计的回退方案
—— 如果环境支持，请选 `keychain`；Docker / 无 D-Bus 的 Linux /
NixOS 模块 / Kubernetes operator 等场景使用 `file`。

### `--persist-session=keychain|file` 下二进制重启会保留什么？

```
[--persist-session=keychain 或 =file]
  /login → 封装 blob + 把密钥放到内存 + 把口令 MIRROR 到后端
  [重启]
  启动 → 从后端读口令 → derive(blob[..16] salt) → 验证 open(blob) → 设置内存密钥
  /api/session/status → authenticated=true（无需 /login 流程）
```

**验证步骤**（M6 seal-salt-mismatch 教训）：启动流程不会盲目信任
持久化条目。它会从持久化口令重新派生密钥，然后调用 `key.open(&blob
.ciphertext)` 证明派生的密钥与 blob 一致。如果 open 失败（口令在
外部被轮换但持久化条目未清理；blob 损坏），持久化条目**会被自动
清除**，让用户回退到 `/login`。这防止了"我通过 sqlite3 轮换了口令
但忘记清钥匙串"的隐患被误以为是成功自动解锁。

### `/api/logout?purge=true`

普通的 `POST /api/logout` 会丢弃内存密钥（因此下次 `/api/dispatch`
返回 401），但**保留**持久化后端 —— 重启服务器后可以通过后端
auto-unlock 自动重新登录。

`POST /api/logout?purge=true` **同时**清除持久化后端（钥匙串条目
/ 文件）。在以下场景使用：交还笔记本、轮换凭据、演示产品时不希望
真实会话泄露。

### 长期部署（systemd、Docker）

README §"Configuration" 部分提供推荐的部署示例。简短版本：

- **systemd**：如果单元在拥有 D-Bus 会话的用户下运行（Linux 上
  开启 `linger`），使用 `--persist-session=keychain`；否则使用
  `--persist-session=file`，路径放在 `/etc/cobrust-studio/` 下，
  并确保单元的 `User=` 拥有该路径。
- **Docker**：推荐 `--persist-session=file`，将口令文件 bind-mount
  到容器中。文件放在镜像之外，避免 `docker build` 缓存意外把口令
  烤进镜像层。

---

## 相关文档

- ADR-0007:密钥存储 AEAD 轮次设计决策
- ADR-0008:多 provider /login (v0.3.0，Phase 2 已实现)
- ADR-0009:跨重启持久化会话 (v0.4.0，M8)
- ADR-0003:认证模型(自定义端点优先)
- `crates/studio-server/src/secret.rs`:AEAD 实现代码
- `crates/studio-server/src/persist.rs`:M8 持久化后端
- `crates/studio-server/src/routes/login.rs`:路由处理
