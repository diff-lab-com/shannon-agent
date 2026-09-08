# Shannon 改进方案 · 执行简报（agent team 派发规程）

- 日期：2026-08-27 ｜ 配套：[master plan v1.4](shannon-improvement-master-plan.md) ｜ 范围：4.1–4.15（**4.16 WASM 暂缓，不派发**）
- 用法：在**具备 Agent/文件/终端工具**的会话中，按 §2 波次派发；每个 agent 收到「全局守则 + 对应任务简报」；波内并行、波间串行（依赖 = master plan §5）。

## 1. 全局守则（每份 agent 提示词必须附带，逐字）

1. 分支：从 dev 切出 feature/&lt;task-id&gt; 独立分支；只 git add 自己创建/修改的文件。
2. **绝不 stage**：工作区已有的 untracked 测试文件（desktop/ui/src/__tests__/ 下 CodeEditor/NotificationsSettings/QuickFix/SessionsPanel 等）、docs/ 目录任何文件、.playwright-mcp/。
3. 验证门（全绿才算完成）：cargo clippy --workspace -- -D warnings；cargo fmt --all -- --check；cargo nextest run -p &lt;涉及crate&gt;；涉及 CLI 时另跑 cargo nextest run -p shannon-cli。
4. 惯例：edition 2024；库 crate 用 thiserror；expect("理由") 不用 unwrap()；**每个新源文件至少一个 #[test]**；注释密度与周边一致。
5. 已知坑：mockito 匹配器按序生效（.expect(N)）；shannon-tools 的 git 相关测试在全套并行时因 CWD 已知会挂，单独跑验证；Edit/Write 后可能有 auto-commit hook 把多文件改动拆成碎 commit，任务收尾用 git reset --soft HEAD~N && git commit 合并为单提交（信息格式：feat(scope): 描述，对齐 git log 风格）。
6. 任务书 = master plan §4.x（先读它再动手）；本简报只列增量要点。
7. 完成定义：守则 3 的命令输出贴回 + 改动文件清单 + 与任务书「验证标准」逐条对照结论。不许为了绿灯降级/跳过测试。

## 2. 波次表（并行组内可同发多 agent）

| 波 | 任务 | 前置 |
|---|---|---|
| W0 | 4.1 + 4.3 + 4.5 | 无 |
| W1 | 4.2 + 4.4 | 4.1 / 4.3 |
| W2 | 4.7 + 4.9 | 4.2+4.4 / 4.5 |
| W3 | 4.8 | 4.1（词汇冻结）+W2 权限结论 |
| W4 | 4.6 + 4.11 | 4.2 / 无（4.11 仅盘点依赖） |
| W5 | 4.10 + 4.12 | 4.8+4.9 / 4.11 |
| W6 | 4.13 + 4.14 + 4.15 | 4.7 / 4.6 / 4.7 |

（4.6 直切与 4.8 总线都动 permission 事件发射点，故分波串行：先 4.8 定总线管道，4.6 在其上做直切与 trace 命令。）

## 3. 任务简报（增量要点，任务书为准）

### B-4.1 词汇表 + 日志写者
- 新增：crates/shannon-types/src/session_event.rs（纯类型，零 engine 依赖）；crates/shannon-core/src/session_log/{mod,writer,reader}.rs。
- 关键点：serde untagged/tag=kind；未知 kind 读侧报错（required 语义）；writer 独占（二次 open 拒绝）；尾行截断恢复写 error 事件；chunk 聚合 flush（50 条/50ms），tool/result、turn/end、request/header 后强制 flush。
- 验收命令：cargo nextest run -p shannon-types -p shannon-core。

### B-4.3 断言词汇
- 改：crates/shannon-core/src/testing/scenario.rs + tests/scenarios/ 新增正反用例 YAML。
- 关键点：新 4 规则字段全部 optional；TrajectoryContains 用子序列匹配；CostBelow per=task|turn。
- 验收：just scenarios；现有 10 场景零变化。

### B-4.5 权限验证（只写测试）
- 新增集成测试（crates/shannon-core/tests/ 或 plugin 相关测试目录）：伪造插件 manifest（read=true/write=false/exec=false）走真实加载与工具执行路径。
- 产出：矩阵表写进 PR 描述（六字段 × 执行点：生效/不生效/无对应执行点）。
- 约束：不改任何实现代码。

### B-4.2 tee 落盘
- 改：query_engine 主循环单点注入（QueryEvent 广播处旁路映射写入）；request/header 取 adapter 序列化原产物。
- 关键点：>256KB 截断（hash+原长留头）但 header 例外；最小脱敏（sk-/ghp_/xoxb- 前缀 + env 密钥值）；SHANNON_SESSION_LOG 开关；先落基准基线再改。
- 验收：mockito 字节级重建等价测试；cli_e2e 零变化。

### B-4.4 eval runner + 20 题
- 新增：crates/shannon-core/src/testing/eval_runner.rs；tests/eval/tasks/*.toml×20（read3/edit5/search3/multi6/recovery3）；justfile 增 eval 配方。
- 关键点：子进程跑 shannon --prompt 采 NDJSON；每任务临时工作区；limits{max_turns,max_tokens,timeout}；report.json+md 双输出。
- 验收：just eval 全量跑通出双报告（需 API key 的真实跑由 ed 手动触发，agent 用 --dry-run 或 mock 模式验证管线）。

### B-4.7 指标 + 失败分类
- 新增：eval 指标提取器（消费 events.jsonl）+ 分类规则 TOML + 报告升级。
- 关键点：循环=同工具+参数哈希连续≥3；7 类失败；版本对比字段。
- 验收：20 题（mock 管线）指标字段零缺失。

### B-4.9 权限强制修复
- 输入：B-4.5 矩阵。按矩阵逐执行点接线（write_files→文件工具前、execute_commands→bash、network→MCP transport、llm_api→API client）。
- 关键点：未声明=现状宽松（兼容测试）；拒绝走统一错误+permission/decision 事件（暂用现有事件通道，4.8 后切总线）。

### B-4.8 统一事件总线
- 新增：crates/shannon-core/src/bus.rs（四模式 dispatch + RegistrationGuard Drop 注销）；迁移 PermissionManager 判定与 HookManager 30 事件为总线节点/订阅；L0 写者挂为内置订阅者。
- 关键点：旧 QueryEvent mpsc 保留为外观，消费方（shannon-ui/shannon-server sse.rs/desktop events.rs）不改；先旁路双分发对账再切换；分发基准 <100µs/事件。

### B-4.6 直切 + trace 子命令
- 改：会话恢复读 events.jsonl 重建状态；删 session_transcript 写路径、analytics 散点采集、recording 模块、单文件快照写路径；新增 session_log/projections.rs（analytics 聚合派生）；shannon-cli 增 trace show/replay/diff/export。
- 关键点：切换前 golden 对账（旧路径 vs L0 投影）通过才删；/rewind 测试零变化；CHANGELOG 破坏性说明（旧会话文件弃用）。
- 验收：恢复往返等价测试；insta replay 快照一致。

### B-4.11 fs/process 接缝
- 新增 trait：shannon-tool-interface 的 FileSystemProvider/ProcessProvider；LocalFs/LocalProcess 平移现有逻辑；工具经 ToolRegistry 注入 Arc&lt;dyn Provider&gt;。
- 关键点：spawn 接口暴露沙箱包装点（给 4.12）；验收含 grep 审计（shannon-tools 内 std::fs/std::process::Command 直呼清零）。

### B-4.10 manifest v2
- 改：plugin/manifest.rs（v2 字段+读取兼容 v1/claude）、registry.rs（显式报错替换静默跳过+安装期校验）；新增 --dump-config；生态约定文档。
- 关键点：解析矩阵测试 v1/v2/claude 三格式；黄金快照。

### B-4.12 sandbox/Landlock
- 输入：4.11 provider。SandboxProvider trait + SandboxedFs/SandboxedProcess 装饰器 + Landlock 后端（rust.landlock）；配置 sandbox=off|local|landlock 缺省 off。
- 关键点：运行时探测降级显式告警；非 Linux 测试跳过并标注；off 模式逐字节等价快照；拒绝分类 sandbox_denied 进 L0。

### B-4.13 三基准
- 新增 adapter：Terminal-Bench / SWE-bench Verified 50（题号清单入仓）/ 自建回归集 10（从 CHANGELOG+PR 历史提炼）。
- 关键点：判据用基准原生；n=3；报告与 4.7 指标同表。真实跑分由 ed 手动执行，agent 保证管线与 dry-run。

### B-4.14 OTLP + 脱敏 + Timeline
- 改：telemetry.rs 重写为 L0→OTLP（缺省 NOOP）；RedactionPolicy 完整版（~/.shannon/redaction.toml）；desktop 增 trace_timeline command + Turn Timeline 面板。
- 关键点：desktop 侧守 desktop/CLAUDE.md 全部规范（Material Symbols、不 import lucide、i18n en+zh-CN 同 commit、lint --max-warnings 0、vitest+e2e mock 模式）。

### B-4.15 在线信号 + 看板
- 新增：聚合计数（反馈/中断/接管//rewind 使用）+ opt-in 上报（复用 notifier/webhook 管道）+ 静态 HTML 看板。
- 关键点：默认关；关闭态零外发测试断言；不上报任何内容字段。

## 4. 派发启动模板（贴给 orchestrator 会话）

「读 docs/plans/shannon-improvement-master-plan.md（v1.4）与 docs/plans/shannon-improvement-execution-briefs.md。按简报 §2 波次表从 W0 开始派发 agent：每 agent 附全局守则（§1 逐字）+ 对应任务简报（§3）+ master plan §4.x 任务书。波内并行同发，全部完成并过守则 3 验证门后才开下一波。4.16 不派发。」

## 5. 状态记录

派发会话应在 master plan §5 旁维护一份进度勾选（或 .omc/notepad），格式：[ ] 4.1 → [x] 4.1（分支名/PR 号/验证门日期）。
