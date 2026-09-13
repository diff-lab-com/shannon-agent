# 全面审查报告（2026-09-14）

**范围**: 全仓（Rust workspace 20 crates + desktop/ui 前端 + CI + 文档 + 仓库卫生）
**基线**: dev @ `8a702630`（审查起点）→ 修复于 `c0adb84d`

## 1. 审查维度与方法

| 维度 | 方法 | 结果 |
|---|---|---|
| CI 健康 | gh run（上一 run 19/19 绿） | ✅ |
| Rust | cargo fmt --check、CI clippy/nextest/Insta/三平台 | ✅ 全绿 |
| 前端 | tsc、eslint、vitest(145 文件)、playwright(66)、token/overlay/contrast 三门禁 | ✅ 全绿 |
| 依赖安全 | pnpm audit --prod | ❌ 27 项（17 high）→ **已清零** |
| 依赖健康 | pnpm outdated | ✅ Base UI 1.8 等已最新 |
| 仓库卫生 | git ls-files 根目录盘点、.zcode 检查 | ❌ 杂项 → **已清理** |
| 密钥泄漏 | sk-/AKIA/PRIVATE KEY 模式全扫 | ✅ 全部假阳性（AWS 官方示例 key 的检测器测试常量） |
| 文档口径 | README vs docs/metrics.md 测试数 | ✅ 一致（11,752 = nextest list 可运行数口径；CI 执行数 12,279 为另一口径，勿混改） |
| 技术债登记 | docs/tech-debt.md | ✅ 4 条有意 deferred 均有触发条件与复核记录（TD-2/3/4/5） |
| TODO 密度 | 前端 0；Rust 15 | ✅ 健康 |

## 2. 发现与处置

| # | 级别 | 发现 | 处置 |
|---|---|---|---|
| F1 | 高 | 生产依赖 27 个已知漏洞（react-router DoS、brace-expansion、fast-uri、nanoid、postcss 等） | ✅ react-router-dom 7.17→7.18.3 + lockfile 刷新，audit 归零 |
| F2 | 中 | `.zcode/plans/` 5 个会话计划文件误入库（其中 1 个含 AWS 示例 key 假阳性字串） | ✅ 移除 + gitignore |
| F3 | 低 | 一次性报告散落根目录（fix-report.md、STREAMING_ADAPTER_VERIFICATION.md） | ✅ 归档 docs/archive/reports/，引用同步 |
| F4 | 低 | 无引用截图散落根目录（grafana-datasource.png、jaeger-span-tree.png） | ✅ 删除 |
| F5 | 信息 | hello.txt / notes.txt | 保留——eval fixture，tests/eval 与 shannon-tools 有引用 |
| F6 | 信息 | 密钥模式命中 5 文件 | 全为假阳性（检测器测试常量），无需处置 |

## 3. 确认为「已达成/已实现」的历史遗留（不再列为问题）

- UI 改进方案 Wave 1/2/3、组件库 Phase B/C/D：全部落地（见 docs/design/ui-audit-2026-09/）
- CI Desktop E2E 连续红：根因（双 Sidebar 渲染）已修，19/19 稳定绿
- G2 触发器矩阵：ScheduleForm 四触发器 + 入站端点展示已实现
- G5 入站触发可见性：WebhookTriggerCard 已落地
- I-6 VS Code 扩展：决策文档已出（Defer + 触发器式重启）

## 4. 后续观察项（不构成本轮行动）

1. TD-3（桌面状态层 SQLite，范围已收窄至收件箱/automation）——按登记触发条件推进
2. i18n 8 语言的**非核心键**翻译（核心 UI 已覆盖，其余 en 兜底）——翻译渠道建立后批量补
3. docs/decisions/ 目录尚无索引——条目增多后补 README
4. `metrics:start` 标记的 README 指标由 scripts/gen-metrics.sh 维护——改口径时走脚本勿手改
