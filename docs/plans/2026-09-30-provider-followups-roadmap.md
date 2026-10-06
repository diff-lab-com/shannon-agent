# Provider/Model 后续任务路线图(2026-09-30 提案 v1.1,待审批)

> 来源:2026-09-29 provider/model 配置深度评审([docs/reviews/2026-09-29-provider-model-config-review.md](../reviews/2026-09-29-provider-model-config-review.md))。
> 其 Phase A(P0)+ Phase B 低风险项已随 **#154** 合入 dev(merge commit `a3311d3b`)。
> 本文覆盖剩余后续项 + 实施期间新暴露的债,作为 2026-09-28 路线图(T1–T15,#150 已完成)之后的下一份批次计划。
>
> 实施方式沿用 #154:每批一个 worktree + 分支 + agent 团队(文件域隔离),单 PR 合入 dev;
> 验收门槛 = #148/#154 口径 + **全套 desktop e2e** + R1-2 落地后的 mock tripwire。
> 每批开工前从最新 dev 重新快照(变基流程已在 #154 验证)。

---

## 0 · 已拍板的决策(2026-09-30)

| # | 决策点 | 决议 |
|---|--------|------|
| ① | CI 覆盖 dev 直推 | **方案 B+,并修门控盲区**:changes 门控当前只认 crates/ 等路径,desktop/ui、gateway 直推不触发任何 job(UI token 大改即由此溜进 dev)。修复:门控新增 `desktop-ui` 输出;dev push 跑 smoke = fmt + clippy(lib+bin)+ desktop lint + **全套 e2e**(健康时约 1 分钟量级,不必缩水成 app.smoke) |
| ② | `/profile` 命名冲突 | **权限命令迁往 `/permissions`**(与 Claude Code 生态一致);provider profile 以 `/profiles` 上线;`/profile` 保留一版过渡期(输出迁移提示),下个发布版正式切换为 provider profile 别名 |
| ③ | 存储收敛 | **写入收敛推迟出本周期**。本周期只做:只读解释视图 `shannon config --explain <key>`(显示该键当前哪层生效、在哪改)+ configuration.md 标注 `config.json`/`preferences.json` 为"UI 便利缓存,引擎不读"。待 R2-4 落地、providers.toml v2 地位稳固后再评估收敛 |
| ④ | i18n 机翻 | **接受机翻,范围收窄**:只翻旅程关键命名空间(`welcome.*`、`settings.models.*`、`chat.banner.*`、`chat.error.*`,约 250 键 × 8 locale),不做全量 3200 键。护栏见 R4-1 |

---

## R1 · 稳定性与卫生(第 1 周,单快速 PR)

| ID | 任务 | 范围与验收 | 规模 |
|----|------|-----------|------|
| R1-1 | 修 `event_bus_reconciliation` 确定性失败 | dev @ `6a96b765+` 上 bus 路径 `turn/end` 的 `llm_steps:0` vs bypass `2`(#149 引入)。先 RCA(注意:PR CI 的 Test job 通过而本地必现——需查 CI 口径是否漏跑该测试),再修 bus 路径携带 llm_steps 或证属固有双路径差异后改测试语义。验收:本地 `nextest --workspace` 全绿且 CI 真能跑到它 | S–M |
| R1-2 | TS mock-handler tripwire | 仿 Rust 侧 `app_command_acl_coverage`:vitest 扫描 `src` 全部 `invoke('cmd')` 字面量,断言均在 `handlers.ts` 注册。验收:删任一 handler 时 tripwire 红;#154 的三个命令已有 handler,tripwire 绿 | S |
| R1-3 | changes 门控修复 + dev push smoke(决策①) | 门控新增 `desktop-ui`/`gateway` 输出并接线对应 job;dev push 触发 fmt + clippy(lib+bin)+ desktop lint + 全套 e2e。验收:含 desktop/ui 改动的直推不再 skipped | S |
| R1-4 | TUI `/config set` 秘密键语义对齐 | TUI 仍把秘密键写入引擎不读的 config.json(CLI 已拒绝)。两条路径统一拒绝,A1 口径提示。验收:行为一致 + 测试 | S |
| R1-5 | 抛光打包 | `/provider health` 对 Gemini/Bedrock/Azure/Replicate 输出逐家跳过原因;TUI 模型 picker 加 "all" 档(可回到全量列表) | S |
| R1-6 | `/permissions` 别名(决策②第一步) | 权限 profile 命令挂 `/permissions` 别名;`/profile` 保留并输出一次性迁移提示;`/profiles` 尚未占用。验收:两命令均可用 + 提示测试 | S |

## R2 · 桌面模型体验(第 2–3 周,用户感知最强)

| ID | 任务 | 范围与验收 | 规模 |
|----|------|-----------|------|
| R2-1 | 会话级模型覆盖 | composer 模型 chip 只改当前会话;菜单内"设为默认"才写全局。复用网关 `shannon/model.switch` 的 in-memory override 模式,落 engine 会话状态 + `commands_config`。验收:多会话切换互不影响、新会话沿用默认、重启恢复;单测 + e2e | M–L |
| R2-2 | Settings"刷新模型目录"按钮 | 接通已有 models.dev/LiteLLM 拉取(现仅 CLI `/model refresh`)。新 tauri command + 按钮 + 失败原因内联展示 | S |
| R2-3 | composer picker 信息增强 | 模型行显示 context/单价/能力徽章(与 Settings 目录列表一致;未知显示 "—") | S–M |
| R2-4 | 自定义模型元数据(schema+引擎先行) | openai-compatible 每个 model 可声明 context/max_output/输入输出单价/能力,存 providers.toml v2;喂给计费、compaction 预算、tier 推断,**一并了结定价双表漂移(评审 P2-22)**。桌面编辑 UI 放 R3。验收:声明价与 `find_pricing` 一致钉测(先例:glm-5.3-flash、openai/gpt-5-mini 子串碰撞) | M |

## R3 · 可靠性与 profile(第 4–5 周)

| ID | 任务 | 范围与验收 | 规模 |
|----|------|-----------|------|
| R3-1 | 接通 failover | `fallback_models` 字段与 Advanced UI 已存在但引擎从不赋值。按 provider 显式配置后,429/5xx/529 按序重试;降级事件进事件流(可回放)+ UI/日志明示"已降级到 X"。不做隐式路由 | M |
| R3-2 | 多 profile 二期 | providers.toml v2 多 profile/路由权重已就绪但仅单 `default` 生效。`/profiles list\|use\|new` + 桌面 UI;R2-4 的模型元数据归 profile 所有。前置 R1-6 完成命名让位 | L |
| R3-3 | Plan/Act 双档位 | chat 头部"规划档位/执行档位"开关(对标 Cline 双模型、Claude `opusplan`),映射 tier 系统;与 R2-1 会话覆盖的优先级需显式定义(会话覆盖 > 模式档位 > 全局默认) | M |
| R3-4 | 能力门控 | 发送图片/计算机使用前检查 vision 位;无能力给"切换到 X?"建议而非裸 provider 报错;能力未知不拦 | S–M |

## R4 · 偿债与差异化(弹性排期)

| ID | 任务 | 说明 | 规模 |
|----|------|------|------|
| R4-1 | i18n 旅程翻译(决策④) | 旅程关键 ~250 键 × 8 locale。护栏:小术语表(品牌/provider/model/tier 等保持英文,复用 T14 术语对齐);zh-TW 自 zh-CN 派生;ja/de/fr/ru 抽检;按 locale 拆 PR;只翻值不动键 | M |
| R4-2 | 配置导出/导入 | providers.toml + 脱敏偏好快照一键导出导入,与 MigrationWizard(仅导入)互补 | S–M |
| R4-3 | 多 key 轮换 | per-provider 多 key 管理 + 轮换(Cherry Studio 式);受众偏窄,可后置 | M |
| R4-4a | `shannon config --explain`(决策③只读部分) | 人话版分层解释:该键当前哪层生效、在哪改、各层文件路径 | S |

## 明确不做

- 按任务类型自动选型的 model router(engine 设计声明 non-goal;`auto` tier 除外)
- Ollama 模型下载管理 UI、provider 市场/聚合托管
- 存储写入收敛(决策③,出本周期;R4-4a 只读视图除外)

## 验收门槛(每批 PR)

fmt / clippy(lib+bin+test,`-D warnings`,desktop 用 tauri 特性)/ nextest 全量 / rustdoc `-D warnings` / design-token 0 error / locale 键集对齐 / **全套 desktop e2e** / R1-2 tripwire 绿 / 版本锁步(如涉版本)。

## 依赖与风险

| 风险 | 缓解 |
|------|------|
| dev 持续快速前进 | 每批开工重照快照;tarball 三方合并 + API 重放流程已在 #154 全程验证 |
| R1-1 RCA 可能牵出 CI Test 口径缺口 | 作为 R1-3 的一部分一并核(门控修复后 dev push 必跑 Test) |
| R2-1 触及 engine 会话状态 | 先落引擎会话覆盖(单测),桌面仅消费;网关已有同构机制可对照 |
| R2-4 定价子串碰撞 | 显式条目 + 一致性钉测;沿用 #154 目录批次的测试模式 |
| R3-1 failover 与重试/退避叠加 | 仅显式配置启用;降级计数上限;事件流留痕可回放 |

## 时间线

W1 = R1(单快速 PR);W2–3 = R2;W4–5 = R3;W6+ = R4 弹性(可与其他批并行)。
