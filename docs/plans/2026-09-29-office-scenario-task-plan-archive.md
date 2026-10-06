# Task Plan: 办公场景竞品深度调研报告 (2026-09-29)

## Goal
深度调研竞品的**办公相关**功能/UI设计/user journey map/user stories/典型场景和用例，
结合 shannon-desktop 当前对办公相关的支持，产出深度调研报告（中文，Markdown），
列出：发现的问题、建议的改进点、缺失的功能。用户审核。

> 旧 task_plan.md（memory/doc/RAG 审查，PR #116）已完成，其结论存档于
> docs/plans/2026-09-25-memory-doc-rag-review.md，本文件为新一轮任务的计划。

## 背景定位（已确认）
- Shannon Desktop = Tauri v2 + React 19 的 AI workspace 桌面应用；引擎在 crates/*。
- 已有 2026-06-13 竞品分析 desktop/COMPETITIVE-ANALYSIS.md —— 视角是「开发者 Agent 编排器」，
  对标 Claude Code Desktop / Codex / Cursor / Hermes / OpenClaw。
- **本次换视角：办公生产力场景**（文档 docx/pdf、表格 xlsx、幻灯片 pptx、邮件 IMAP、
  会议/日程/自动化 routine、模板、文件管理），竞品对象是 AI 办公产品。

## Phases
- [x] Phase 0: 定向 — 读 desktop/CLAUDE.md、COMPETITIVE-ANALYSIS.md、目录结构
- [x] Phase 1: 代码库现状盘点（3 个 Explore 并行，✅ 全部完成）
- [x] Phase 2: 竞品网络调研（4 个 web agent，✅ W2/W3 限流后重试成功）
- [x] Phase 3: 报告已写入 docs/research/2026-09-29-office-scenario-competitive-research.md（427 行）
- [x] Phase 4: 质量自查 ✅ — 抽查 5 处关键 file:line 引用（commands.rs 附件黑洞、
      welcome/constants.ts:89 假门、data_source_fetchers dispatch、read.rs 二进制嗅探、
      DataSources Verified 徽章）全部与源码一致；已交用户审核

## Key Facts (accumulating)
- desktop 59+ Tauri 命令；extensions_commands.rs 有原生数据源 Obsidian + IMAP；
  commands_files.rs 有文本保存/文件 diff/文件树；文件拖拽已有。
- OPC-SCHEDULED-GAP-ANALYSIS.md / OPC metric aggregation 存在（OPC = Open Packaging
  Convention，即 docx/xlsx/pptx 容器格式，待确认具体内容）。
- 根目录 skills/ 与 crates 侧 SkillRegistry 存在，办公技能可能已内置。
- UI: React19 + Tailwind4 + MD3 token + 8 主题 + Material Symbols 图标规范；i18n en/zh-CN
  （另有 locales/ 8 套 frozen locales 在 monorepo 根）。

## Errors Encountered
| Error | Attempt | Resolution |
|-------|---------|------------|
| (none yet) | | |

## Session Log (2026-09-29)
- 7 个调研代理（3 Explore + 4 web）全部完成；W2/W3 首次因账户限流失败，落盘 findings 后重试成功。
- 关键发现：仓库 OPC=One Person Company 非文档格式；Office 附件黑洞（docx/xlsx/pptx 拖入模型不可见）；
  IMAP/Obsidian 数据源查询断路但 UI 显示 Verified；README "drafts email" 无实现。
- 交付：docs/research/2026-09-29-office-scenario-competitive-research.md（427 行，含 5 条破损旅程走查、
  17 项问题清单、Wave1/2/3 改进路线、明确不建议项）。
- 自查：5 处 load-bearing file:line 抽查全部与源码一致。

## Session Log (2026-09-29 下午)
- 用户决策：B4 会议纪要流水线移除 → 主报告 §10 已删行、§11 已同步。
- 对抗性审查完成：docs/reviews/2026-09-29-office-plan-adversarial-review.md
  🔴6 / 🟡8 / 🔵7。最重发现：§10 与 improvement-plan-2026-09 P2-3 瘦身决议正面冲突未声明（R5）；
  A2 复刻 PDF 静默截断缺陷（R1）；A3×B1 依赖倒置（R2）。
- 待用户裁决 §9 六项（Q5 合并方式是排期生效前提）。

## Session Log (2026-09-29 傍晚)
- 用户同意全部复审建议 → 改进方案 v2 落地：
  1) 主报告 §10 整节替换（10.0 已裁决决策 Q1-Q6 / 横切工作项 / Wave1 五项 / Wave1.5 两项 /
     Wave2 八项按 B1→B6'→B3→B2→B5→B9'→B7'→B4' 排序、B2 标注首个让位 / Wave3 带前置 /
     不建议做 4 条 / 排期口径含 traction 门）；§7/§9-17/§11/报告头同步修正。
  2) improvement-plan-2026-09.md 决议区追加 2026-09-29 修订注记（Q5=b：P2-3 框架扩展，
     Wave1+1.5 立即排入，Wave2 设 traction 门）。
- 待用户审核 v2 方案。

## Execution Phase (2026-09-29 晚): Waves 1.5/2/3 全量实施（用户指令：解除 traction 门）
- Wave 1 = PR #157 (feat/office-wave-1)，已 push。
- 分支叠放：feat/office-wave-15 ← feat/office-wave-1；feat/office-wave-2 ← 15；feat/office-wave-3 ← 2。
  PR 全部 --base dev（#157 合并后自动重定向）。
- Wave 1.5 = A2'(desktop/src/document_parse.rs: roxml+zip 解析 docx/pptx、calamine xlsx/ods、csv；
  提取文本落 ~/.shannon/cache/extracted/<hash>.txt 带章节标记；send_message 注入摘要+显式截断+
  cache 路径供 Read/Grep 分页；zip 炸弹/条目/解压比护栏；PDF pdftotext -f/-l 页范围)
  + B8b(pdfjs-dist 内联预览，FileCard 加预览按钮)。
- Wave 2 = B1(引擎原生 xlsx 工具 rust_xlsxwriter) B2v1(PPT 大纲确认对话框) B3v1(IMAP/Obsidian
  fetcher+注入会话按钮) B4'(转写纪要技能) B5(3+ productivity 例程) B6'(例行结果 webhook 投递+存草稿)
  B7'(diff 渲染增强+FileCard Review) B9'(引用式文件索引)。
- Wave 3 = C1(叙事文档) C2(CSV 批量提示构建器) C3(伴随窗口 spike→实现) C4(来源集合基础)
  C5(风格反提技能) C7(extensions productivity 类目) C8(引用 pill) C6(时间线导出)。
  C9 逐页局部重生成仍按审查建议不实现（记录于 PR）。
- 完成判据：每 Wave 全门禁绿 + PR 创建；合并视 CI/分支策略，受阻则报告。

## Execution Complete (2026-09-30)
- 全部四个 PR 已合并进 dev：#157 (Wave 1) / #161 (Wave 1.5) / #162 (Wave 2) / #164 (Wave 3)。
- CI 修复轮（合并态 gate）：doc 链接私有项 ×2、PathSandbox 未解析、office_skills_tests fmt、
  timeline role=list aria、RUSTSEC-2023-0086（imap unmaintained，按 audit.toml 既有 per-tool 政策
  追加并恢复被误覆写的维护清单）、FilesPage primary/10 chip 对比度、ACL workspace 残留清理。
- 顺带修复的 dev 侧既有缺陷：PDF 注入死代码、query_data_source kind 读错段、
  secondary 色在 8 主题不达新 AA 契约（theme-source 微调 11 主题）。
- 未竟项（记录在案）：C9 逐页局部重生成（审查决定不做）、伴随窗口全局快捷键（托盘入口已上）、
  会话来源持久化+send 管线注入（v1 草稿板语义）、IMAP Drafts 的 UI 接线（builder 已备）。
- worktree 保留：shannon-mono.worktrees/office-wave{1,15,2,3}（分支已合并，可随时清理）。
