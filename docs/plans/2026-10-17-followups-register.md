# 跟进项登记册:建议推进清单(2026-10-17)

> **状态:草案,待 ericdong 审核**(未提交)
> **范围**:调研报告 v2 → PR #429(P0 三项)→ PR #430(桶 1 fast-follow)全程遗留的跟进项汇总与处置建议
> **判据**:改行为的攒到既定行为变更窗口;保真度/杂务攒小 PR 或绑定既有机会;定时类落账
> **已闭环备忘**(不再列出):#430 已收掉 T2⑦ 措辞、T3⑨ payload 回退、T3⑪ load 失败可见化、T3⑬ 旗子 help、T3⑩ 披露句

---

## A. 建议近期推进(小时级,可打包一个维护 PR)

| ID | 项 | 来源 | 性质 | 建议处置 | 成本 |
|---|---|---|---|---|---|
| A-1 | **deny.toml 重评触发器落账**:"calamine/上游出 zip≥9 兼容版时重评"目前只活在 deny.toml 注释里 | #429 CI 波折 | 文档 | `docs/tech-debt.md` 加一行,指向 deny.toml 注释与三条 RUSTSEC ID;现有 cargo-deny 每 CI 自动变红即提醒器,无需新机制 | 5 分钟 |
| A-2 | **`HookDef.command` 补 `#[serde(default)]`**:#430 发现 http/llm 钩子配置省略 `command` 字段会拖垮**整个文件**解析(静默失效);补 default 后这些配置开始生效 | #430 T2 发现 | **行为变更** | 独立小 PR,必须配套:①release note 显式说明"此前写坏的 http/llm 钩子配置将开始生效";②首次解析出新激活钩子时打一条审计行(warn 或 hook/fired 类行),防"静默激活" | 半天 |
| A-3 | **`fixture_hook_manager` 测试 hermetic 化**:`assembly_dump.rs` 既有测试仍用全局 `load()`(继承自 #429 的暴露,新测试已修) | #430 T2 评审 | 测试卫生 | 一行改 `load_from_path`,顺手 | 10 分钟 |
| A-4 | **`/api/assembly` 注册进 utoipa OpenAPI**:现不在 `/openapi.json`(shannon-server 的 ApiDoc) | #430 T1 评审 | chore | 需给 `AssemblyDump`/`Section<T>` 等加 `ToSchema` 派生,派生面中等;若近期无人消费 openapi,可降级为"文档注明路由" | 0.5 天 |

A-2 需要你单独点头(行为变更);A-1/A-3/A-4 纯维护。

## B. 并入既定波次(不单开,锚点已存在)

| ID | 项 | 锚点 | 说明 |
|---|---|---|---|
| B-1 | **PostToolUseFailure 解码臂 + 其余 9 个 NoProducer 事件的生产者补齐**(StopFailure/Notification/PermissionDenied/PermissionRequest/FileChanged/CwdChanged/PostToolBatch/Elicitation/ElicitationResult) | 9/21 复审 **R2.3 hooks 事件面扩展** | 一次行为变更、一轮测试与文档;semantics.rs 穷尽 match 表会强制同步——机检红利。解码臂先补 PostToolUseFailure(生产者已在,#429 落地) |
| B-2 | **api-protocol 陈旧 doc 注释**(`duration_ms` 相关描述已失实) | 下次任何动 `gen-protocol` 的机会 | 单独修也要跑一次 TS 再生成,不值 |
| B-3 | **signals `observe_tool_execution` 上车 wire schema** | SIGNALS_SCHEMA v2(出口 `tool_exec_snapshot()` 已备好) | 不碰冻结的 10 计数白名单 |
| B-4 | **端到端 duration 断言** | eval/场景测试日常扩展 | 映射级与透传已有单测覆盖 |
| B-5 | **agent_loop 阶段化**(报告 P1-1) | 9/21 复审 **R2.1 engine 拆分** | 先做一页纸"状态机轴 × 管线轴"调和,再进波次;大项(1-2 月) |

## C. 条件触发(到了条件再做)

| ID | 项 | 触发条件 |
|---|---|---|
| C-1 | hook 命令/payload 脱敏开关 | dump 输出面从 owner 扩大到非 owner 观众(团队共享 serve)时 |
| C-2 | hooks 快照 per-request reload | 用户反馈"改钩子后 dump 不更新"成为实际痛点时(现已在代码 doc 注明快照语义) |
| C-3 | `execute()` input clone 惰性化 | perf 基准显示工具调用路径开销显著时 |
| C-4 | 微观项:consecutive_denials 移位、多余 Arc、skill 目录镜像共享化 | 下次触碰同文件顺手,不单开 |

## D. 需要你单独裁决的大项(报告 §5 遗留,非杂务)

| ID | 项 | 建议 |
|---|---|---|
| D-1 | **ACP 子代理 spike**(报告 P1-3,定位级假设) | spike 定义已备好(报告 §5 P1-3):ACP client 骨架 + 真 Claude Code 子代理 round-trip + 凭据隔离;验收=一次端到端委派。**是否投 spike 由你裁决**;建议在排 R2 波次前定,因为它可能改变 R2 的子代理设计 |
| D-2 | **P0-1 write-intent 门**(报告 §5 P0-1 中"单独裁决"子项) | 依赖流式/长驻工具取消语义评估;建议与 B-1 同窗口评估(都动钩子/管线执行面) |

## E. 仅记录,无需动作

- EnforcedInline 路由在引擎内经 HooksFile 承载(反向依赖禁止)——已接受偏差,semantics.rs 与测试均有记录。
- `Stop` 的 exit-2-continue 语义不可达(`should_continue` 恒 false)——已入语义表 gotcha,修复属 R2.3 范围。
- matcher 正则先于管道分割、`if_condition` 已解析未求值——已入语义表 gotcha,修复属 R2.3 范围。

---

## 建议推进顺序(供审核)

1. **本周可做**:A-1(5 分钟)→ A-3(10 分钟)→ A-2(半天,含变更说明)——一个维护 PR 打包,或 A-2 独立成 PR 以便 release note 单独引用。
2. **下一波次前决策**:D-1(ACP spike 投不投)、D-2(write-intent 评估归期)——影响 R2 设计。
3. **波次内消化**:B-1 与 B-5 分别锚定 R2.3/R2.1;B-2/B-3/B-4 各找宿主。
4. **不动**:C 组条件未到;E 组已记录。

## 决策点(2026-10-17 推荐意见,供裁决)

| # | 决策 | 推荐 | 理由摘要 |
|---|---|---|---|
| K1 | 维护 PR 打包范围 | **A-1+A-3 打包;A-4 押后**(触发:openapi 出现真实消费者,或 dump 形状稳定一个版本) | dump 形状两天两变,过早注册 OpenAPI=冻结漂移中的 schema;A-4 价值未证实(无已知消费者),性质是"过早承诺"而非杂务 |
| K2 | A-2(serde default)归宿 | **独立 PR + release note + 首激活审计行**,搭下个小版本窗口 | 行为变更需可单独引用与回滚;混入维护 PR 会被 chore 淹没且诱导草率评审 |
| K3 | D-1 ACP spike | **有条件启动**:R0 清账后、R2 排期前,2-3 天 timebox(定义已备好:ACP client 骨架+CC round-trip+凭据隔离;验收=一次端到端委派) | spike 的价值=时机——产出要能塑造 R2 子代理设计,R2 开工后做就失效;编排者是分析报告最清晰的差异化杠杆且 dsh 已验证需求;不与 R0 抢资源(基本功优先,见报告不抄清单 #5) |
| K4 | B-1 hooks 扩面归期 | **维持 R2.3**;特定事件被真实用户需要时单点补(小时级) | R1 收敛仍优先;#429 语义表穷尽机检已把"拖后"的成本降低(不会烂掉);提前的整体收益小于挤占 R1 的代价 |

> 本表为登记册的最终建议版;若与上文 A/B/D 组正文有出入,以本表为准。

## 附:证据链

各項来源:#429 任务评审/终审的 deferred minors(分支 ledger 已销账为 OK-TO-MERGE)、#430 任务评审/终审(同)、报告 v2 §5 与 9/21 复审 §5。行为兼容基线:#429/#430 全部合并时 CI 十九项绿 + 本地 nextest 6,118/6,118。
