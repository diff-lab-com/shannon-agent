# Provider/Model R2 实施排期稿(2026-10-05,决策①-⑪已拍板)

> 方案:[docs/reviews/2026-10-05-provider-model-config-review-r2.md](../reviews/2026-10-05-provider-model-config-review-r2.md)(v1.2 可执行稿) · 红队:[…-redteam.md](../reviews/2026-10-05-provider-model-config-review-r2-redteam.md)
> 实施方式沿用 #154/chat-r3 口径:每个 PR 一个 worktree + 分支,文件域隔离的 agent 并行实施;验收门槛 = fmt / clippy(lib+bin+test,`-D warnings`)/ nextest(触及 crates)/ rustdoc / `pnpm lint`(tsc+eslint+design-token+i18n-check)/ vitest 全量 / 全套 desktop e2e(CI 复核)/ mock tripwire。基线:dev @ `827122346`(PR #290 合并点)。

## 波次与 PR 映射

| 波次 | PR | 分支 | 范围(方案项) | 依赖 |
|---|---|---|---|---|
| W0 | docs | `docs/provider-model-r2-plan` | 本排期稿 + 方案 v1.2 + 红队报告入库 | — |
| W1 | S1-A | `feat/provider-r2-s1-error-kinds` | S1-1 错误分类链(引擎单源 kind → 桌面 402/429/403 横幅+深链+i18n 全 locale) | — |
| W1 | S1-B | `feat/provider-r2-s1-ux-polish` | S1-2 pin×failover 文案 · S1-3 dynamic 徽章删除 · S1-5 Header 空目录反馈 | 与 S1-A 不同 locale 命名空间,可并行 |
| W1 | S1-C | `feat/provider-r2-s1-ollama-quickfill` | S1-4 Ollama 默认端口探测(TTL 缓存护栏)+ quick-fill 现役 id + fetch 联动预填 | — |
| W2-3 | S2-A | `feat/provider-r2-s2-model-vault` | S2-1 模型仓(fetch 固化+护栏⑥+优先级钉)+ S2-3 max_output 接线(⑩)+ S2-5 tier 扫声明 + S2-4b tool 位与门控(⑧,批尾) | S1 全合并后开工 |
| W2-3 | S2-B | `feat/provider-r2-s2-meta-editor` | S2-2 per-model 元数据桌面编辑器 | 依赖 S2-A 模型仓写路径,同批后段 |
| W2-3 | S2-C | `feat/provider-r2-s2-vision-ux` | S2-4a vision 预检+切换建议+文案桌面化+碰撞钉测 | 与 S2-A 并行(不同文件域) |
| W2-3 | S2-D | `feat/provider-r2-s2-azure` | S2-6 Azure catalog≥3 + api-version/deployment 前置验证 + 探测前置提示(P-N25)+ 裁定⑤ 收紧切换 | 与 S2-A 并行 |
| W4-6 | S3-A | `feat/provider-r2-s3-switch-converge` | S3-1 切换面收敛+来源/覆盖标签 · S3-2 profile×覆盖提示 | S2 合并后 |
| W4-6 | S3-B | `feat/provider-r2-s3-utility-tiers` | S3-3 utility 槽(裁定②⑦:两槽+正交化,L) | 依赖 S2-5 |
| W4-6 | S3-C | `feat/provider-r2-s3-failover-guide` | S3-4 推荐降级链一键 + 流中途语义文档化 | — |
| W4-6 | S3-D | `feat/provider-r2-s3-effort-cost` | S3-5 effort 二级交互(⑪) · S3-6 发送前成本预估(同源计数钉测) | S3-A |
| W6 | S4 | `fix/provider-r2-s4-hygiene` | i18n 回填/补 e2e/mock 补齐/a11y/TS union/models_url/导出安全(裁定④)/dedup/模块头注释/--explain 桌面键/configuration.md 覆盖语义 | 随批搭车项已并入各批,此处收尾 |

## 合并顺序与变基纪律

1. W1 三个 PR 无代码依赖,按 S1-A → S1-B → S1-C 顺序合入;后合者如有 locale JSON 冲突(不同键域,预期可自动合并),变基重跑门禁后再合。
2. 每批开工前 `git fetch origin dev` 重照快照;worktree 一律从最新 `origin/dev` 创建。
3. PR 合并用 merge commit(沿 #154/#267 惯例);CI 未绿时先本地门禁复核再 admin 合并,失败复盘记入 PR。
