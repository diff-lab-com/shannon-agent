# W6-1 · P2-8 VS Code 扩展(编码赛道最后一块)

> **Track**: Wave 6
> **Date**: 2026-08-08
> **Status**: ⏸️ **DEFERRED(2026-08-08,用户决策)** — VS Code 扩展暂缓。spike 与本方案保留,随时可重启。
> **重启条件**:① 编码赛道 IDE 入口成为下一优先级;② P2-7 HTTP API 经更多消费方验证稳定。
> (原定:Proposed,spike 已完成,待 S1 实施)
> **Estimate**: 2–3w · **Priority**: 🟡 中
> **Dependencies**: P2-7 HTTP API ✅(`3ed22799`)
> **Parent**: [wave-5-followup.md](./wave-5-followup.md) §2 W6-1 · **Spike**: [spikes/p2-8-vscode.md](../spikes/p2-8-vscode.md)

---

## 1. Context

VS Code 是编码赛道的 IDE 入口,当前缺失(scaffold 在 `legacy-archives/`)。spike([p2-8-vscode.md](../spikes/p2-8-vscode.md) + [pres1-validation](../spikes/p2-8-vscode-pres1-validation.md))已完成竞品调研(Cursor / Continue / Claude Code 扩展)与架构选型。P2-7 `shannon serve` 的 v1 session HTTP API 已就绪,比 legacy 的 NDJSON 子进程更稳。

完成本 task 后,Wave 3 进度从 7/8 → **8/8 = 100%**,Wave 1–3 合计 24/24。

## 2. 架构决策(来自 spike)

- **通信**:HTTP API(`shannon serve` :33420 v1 session),弃用 legacy NDJSON 子进程。
- **命令面**:启动/停止 session、发 prompt、读流式输出(含工具事件)、中止。
- **密钥**:VS Code SecretStorage API(Electron 原生,非 keytar);与桌面/CLI 共用 credential store 协议。

## 3. 文件锚点

| 产物 | 路径 |
|---|---|
| 活跃扩展源 | `extensions/vscode/`(新建,从 `legacy-archives/shannon-code/editors/vscode/` 迁移) |
| HTTP 客户端 | `extensions/vscode/src/shannonClient.ts`(调 `shannon serve`) |
| 引擎 API | `crates/shannon-core/src/api_server.rs`(已 ✅) |
| 发布配置 | `extensions/vscode/package.json`(vsce)+ CI workflow |

## 4. 实施步骤(分阶段)

### S1 · 骨架迁移 + HTTP 通信(3–4d)
1. 迁移 legacy 扩展到 `extensions/vscode/`,清掉 NDJSON 客户端。
2. 实现 `shannonClient.ts`:连 `shannon serve`,健康检查,session 生命周期。
3. 最小命令:`Shannon: Start Session` / `Shannon: Send Prompt`(输入框)。

### S2 · 流式输出 + 工具事件(3–4d)
1. SSE/NDJSON 流解析,渲染到 webview / output channel。
2. 工具调用事件高亮(bash / file / edit)对齐 CLI 体验。

### S3 · 密钥 + provider 配置(2–3d)
1. SecretStorage 存 API key,启动时注入 `shannon serve`。
2. provider / model 选择 UI(复用 CLI 的 tier 语义 fast/standard/pro)。

### S4 · 发布流程(2–3d)
1. `vsce package` 产 `.vsix`;CI 打 tag 时自动发布(复用 desktop release matrix)。
2. 先侧载 `.vsix` 验证,再 Marketplace。

## 5. 验收

- [ ] VS Code 里能起 session、发 prompt、看流式工具输出。
- [ ] API key 走 SecretStorage,不落明文。
- [ ] `.vsix` 侧载可用;tag 触发 Marketplace 发布。
- [ ] 文档:`docs/integrations/vscode.md`(配置 + 故障排查)。

## 6. 风险

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| `shannon serve` 进程管理(VS Code 生命周期) | 中 | 中 | 扩展 activate 时探测 :33420,未运行则 spawn |
| SecretStorage 跨平台差异 | 中 | 中 | spike 已调研;Linux 走 libsecret / keyring |
| Marketplace 发布审核 | 低 | 中 | 先侧载;遵循 VS Code 扩展规范 |

## 7. 参考

- [spikes/p2-8-vscode.md](../spikes/p2-8-vscode.md) §架构选型
- [spikes/p2-8-vscode-pres1-validation.md](../spikes/p2-8-vscode-pres1-validation.md)
- P2-7 HTTP API:`crates/shannon-core/src/api_server.rs`
