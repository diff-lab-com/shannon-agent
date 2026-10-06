# 后续改进路线图 (2026-09-28 审批版)

> 源自 2026-09-28 全面审查(docs/plans/2026-09-28-comprehensive-review-and-hardening.md)
> 的显式后续项、实施 batch 偏差与新暴露的债。已获批准,按三个周批次实施,
> 单 PR 合入 dev(分支 followups/2026-09-28)。

## S1(09-29 ~ 10-03)稳定性快赢

| ID | 任务 | 来源 |
|----|------|------|
| T1 | DiffReviewBody j 键焦点测试稳定化(CI 偶发、本地稳定的时序 flaky) | 新暴露 |
| T2 | desktop/ui vitest.config.ts 重复 `exclude` 键 | 新暴露 |
| T3 | 测试目标 clippy 清零 + CI 口径收紧到 `--all-targets`(含 main.rs 重复 `#[test]`) | 基线测量 |
| T4 | shannon-server graceful shutdown + 请求超时(F46) | 审查遗留 |
| T5 | SecretGuard 首次审计命中后一次性 opt-in redact 提示(F23;默认行为不变) | 审查遗留 |
| T6 | SessionTee 边界 fsync 移出 tokio worker(F13) | 审查遗留 |
| T7 | metrics 周刷新自动化确认/修复 + 重生成 docs/metrics.md(F60) | 审查遗留 |

## S2(10-06 ~ 10-10)契约与审批闭环

| ID | 任务 | 来源 |
|----|------|------|
| T8 | headless NDJSON envelope 统一(F38;破坏性:旧词表保留 compat 期,内部消费方同 PR 迁移) | 审查遗留 |
| T9 | gateway↔desktop 配对审批 RPC(F42 完整形态:桌面审批入口 + `shannon/pairing.approve`) | 审查遗留 |
| T10 | repl 命令输出 i18n 批量迁移(F39;新键入 en+zh,其余 locale 由 T14 补齐) | 审查遗留 |
| T11 | MCP 审批绑定补 source-path provenance(B3 偏差) | batch 偏差 |

## S3(10-13 ~ 10-17)性能与 i18n 偿债

| ID | 任务 | 来源 |
|----|------|------|
| T12 | repomap 裁剪缓存(pack 非破坏化 + injector 缓存) | batch 偏差 |
| T13 | libspa/pipewire 开发环境问题(不动默认特性的前提下:文档 + 快速检查 recipe + 优雅报错) | 基线发现 |
| T14 | 8 locale 翻译补齐(ar/bn/es/fr/hi/ja/pt/ru 各 177 键) | 审查遗留 |
| T15 | 小债合并:Cooldown None-key 永生条目、`is_synthetic_reminder` 补 user_notices、HostGuard `extra_hosts` 配置化、CHANGELOG 0.11.0 回填(可选) | batch 偏差 |

## 验收门槛(同 #148)

fmt / clippy(lib+bin, -D warnings, desktop 用 tauri 特性)/ nextest 全量 / gateway
typecheck+test+build / rustdoc -D warnings / 版本锁步 / locale 键集对齐。
T3 完成后追加:clippy `--all-targets` 口径。
