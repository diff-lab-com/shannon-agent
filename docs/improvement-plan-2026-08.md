# Shannon 综合改进任务列表与实施方案(v2)

> 编制日期:2026-08-02(v2,基于用户评审反馈重构)
> 编制视角:高级产品经理 + 高级架构师
> **用途:供 ericdong 评审。本文档只产出计划,不自动执行。**
> **v2 变更**:① 双赛道战略视图(编码线+办公线);② 新增桌面 chat 升级详细任务块(附件/语音/多线程/美化);③ 每个高优任务补**文件锚点 + 实施步骤 + 验收标准**(替代 v1 的大概描述)。
> 输入:[openworker-research.md](./openworker-research.md)、[competitor-feature-matrix.md](./competitor-feature-matrix.md)、[project-review-2026-08.md](./project-review-2026-08.md)、[aichat-ui-library-evaluation.md](./aichat-ui-library-evaluation.md)

---

## 0. 战略主线(v2,双赛道)

> **"先收敛、再补短、后扩张"** —— 两条产品线,一套引擎,分线补短。

**双产品线**:
- **shannon-code**(编程赛道):对标 Claude Code/Codex CLI/OpenCode/Reasonix。必修:repo map、auto-test loop、IDE 扩展、dead code 清理。
- **shannon-desktop**(办公赛道):对标 Claude Desktop/Codex Desktop/openworker/Hermes/WorkBuddy。必修:**SaaS 集成**、**沙盒**、**chat 升级(附件/语音/多线程/美化)**、artifact 显性化。

**三条纪律**:
1. **不追 openworker 办公赛道的全量 SaaS**(25+ 太重),用 **MCP 低成本补开发者/办公高频 SaaS**。
2. **坚持 Rust 差异化**(轻量/可审计/单二进制),语音走 whisper-rs 本地方案。
3. **chat 升级借 assistant-ui 骨架**(ROI 已翻转,见 [aichat-ui 评审 v2](./aichat-ui-library-evaluation.md)),不自研全部。

**用户已授权的决策(v1 评审)**:
- ✅ 战略主线
- ✅ P1-3 SaaS 集成 5 顺序(GitHub/Slack/Jira/Notion/Linear)
- ✅ P1-1 dead code 裁决(/pdf 删、/diff+/review_pr+/export 接通)
- ✅ P2-5 Chat UI(已升级为"以 assistant-ui 为骨架的完整 chat 升级")
- ⏸ 资源评估暂缓

---

## 1. Wave 1:收敛与对齐(本月,~1.5w)

目标:建立可信度基线,解除合并阻塞,做实卖点。

### P0-1 · 完成 ADR-0008 交互 QA 并合并
- **描述**:当前分支 `fix/provider-model-command-remediation` P0–P3 代码已落地 + 测试门绿(10258/10258),20 条行为验收待交互 QA。
- **文件锚点**:[`docs/plans/adr-0008-qa-checklist.md`](./plans/adr-0008-qa-checklist.md)、[`docs/plans/provider-model-command-remediation.md`](./plans/provider-model-command-remediation.md)
- **实施步骤**:
  1. 启动交互式 REPL(`cargo run -- REPL`),按 QA checklist 逐条验证 20 条行为项(卡片即时更新、zh 翻译完整、/disconnect、/model refresh 后台化等)。
  2. 失败项回到对应 P 编号修复;通过项在 checklist 勾选。
  3. 全绿后 `git checkout main && git merge fix/provider-model-command-remediation`(或开 PR)。
  4. 验证合并后 `just dev`(check+lint+test)通过。
- **验收**:
  - [ ] 20 条 QA 全部勾选
  - [ ] `just dev` 在 main 上通过
  - [ ] 合并提交无冲突
- **估时**:0.5d · **依赖**:无 · **风险**:低

### P0-2 · 文档与代码一致性校对
- **描述**:修复文档漂移(最严重:`desktop-architecture.md` 写 React 18,实际 React 19)。
- **文件锚点**:
  - `docs/desktop-architecture.md`(React 18→19;核查组件示例 vs `desktop/ui/src/components/`)
  - `docs/SPEC.md`(crate 行数同步)
  - `CLAUDE.md`(测试数引用,见 P0-3)
- **实施步骤**:
  1. `grep -rn "React 18" docs/` → 全部改为 "React 19"。
  2. 对照 `desktop/ui/src/components/` 实际组件树,修正 `desktop-architecture.md` 的组件示例。
  3. `cloc crates/` 产出实际行数,同步 SPEC.md 表格。
  4. CLAUDE.md 的"~7889 测试"改为引用 `docs/metrics.md`(P0-3 产物)。
- **验收**:
  - [ ] `grep -rn "React 18" docs/` 无结果
  - [ ] SPEC.md 行数与 `cloc` 一致
- **估时**:0.5d · **依赖**:P0-3(metrics) · **风险**:低

### P0-3 · 统一度量基线
- **描述**:建立单一权威度量源,解决测试数三处打架(~7889/~9181/3180)。
- **文件锚点**:新增 `docs/metrics.md`;新增 `scripts/gen-metrics.sh`;CI 配置(`.github/workflows/` 或 Gitea 等价物)
- **实施步骤**:
  1. 写 `scripts/gen-metrics.sh`:跑 `cargo nextest run --workspace --no-run` 统计测试数 + `cloc crates/ desktop/src/` 行数 + `cargo clippy -- -D warnings` 状态 + `cargo deny check` 状态,输出 markdown 到 `docs/metrics.md`。
  2. CI 在每次 main 构建后运行该脚本,提交更新到 `docs/metrics.md`(或作为 artifact)。
  3. CLAUDE.md / SPEC.md / 调研文档把写死的测试数/行数替换为"见 [metrics.md](./metrics.md)"。
- **验收**:
  - [ ] `docs/metrics.md` 存在且 CI 自动更新
  - [ ] 其它文档不再有写死的测试数
- **估时**:1d · **依赖**:无 · **风险**:低

### P1-1 · Dead code 清理(裁决 + 执行)
- **描述**:对 9 个 dead 模块逐个裁决接通 or 删除(用户已授权:/pdf 删、/diff+/review_pr+/export 接通、/debug 接通)。
- **文件锚点**:
  - 删除:`crates/shannon-commands/src/builtin/pdf.rs` + 在 `builtin/mod.rs` 去注册 + ROADMAP 移除
  - 接通:`crates/shannon-commands/src/builtin/{diff,review_pr,export,debug}.rs`
- **实施步骤**:
  1. **/pdf 删除**(0.5d):删 `pdf.rs`,从 `builtin::mod.rs` 移除 `pub mod pdf;` 与命令注册;`grep -rn "pdf\|PdfTable\|ImageFormat"` 清残留;从 ROADMAP-FUTURE 移除条目。
  2. **/export 接通**(1d):实现 `export_to_markdown()` + `export_to_json()`,接 `ExportOptions` 到命令 handler;接 `session_transcript` 取历史;加单测。
  3. **/diff 接通**(2d):接 `ChangeCategory` 到 diff 输出管线;实现 `DiffAnalysis::summary()`;连 `DiffPattern` 正则到 diff 解析;加 `has_test_changes()` 用于智能 commit message。
  4. **/review_pr 接通**(2d):实现 `ReviewCategory::from_str()`;接 `ReviewSuggestion` 到 LLM prompt 结构化输出;连 `PRAnalysis` 到 git diff;加严重度过滤与展示。
  5. **/debug 接通**(1d):接 `DebugCategory` 过滤;连 `LogLevel` 到 `InternalLogger`;加运行时日志级别切换。
- **验收**:
  - [ ] `grep -rn "allow(dead_code)" crates/shannon-commands/src/builtin/{diff,review_pr,export,debug}.rs` 无 dead 残留
  - [ ] 每个接通命令有单测 + REPL 手测通过
  - [ ] pdf.rs 彻底移除,构建无警告
- **估时**:接通 ~6d + 删除 0.5d · **依赖**:无 · **风险**:中(/diff、/review_pr 涉及 LLM prompt 工程)

### P1-2 · Hook dead events 接通(前 3 个)
- **描述**:接通 3 个低成本 dead events(UserPromptExpansion / InstructionsLoaded / ConfigChange)。
- **文件锚点**:
  - `crates/shannon-core/src/hooks/events.rs`(事件已定义)
  - `crates/shannon-skills/`(模板展开器,发 UserPromptExpansion)
  - `crates/shannon-core/src/instructions/`(InstructionsLoader,发 InstructionsLoaded)
  - `crates/shannon-core/src/config/`(Config reload path + file watcher,发 ConfigChange)
- **实施步骤**:
  1. **UserPromptExpansion**(~1h):在 skill/command 模板展开器(`$ARGUMENTS`/`$FILE_PATH` 解析后)单点 emit。
  2. **InstructionsLoaded**(~1h):在 `InstructionsLoader` 合并 CLAUDE.md + `.claude/rules/*.md` 后 emit。
  3. **ConfigChange**(~3h):在 config 模块加 file watcher(用 `notify` crate),`.shannon.toml` 变更触发 reload + emit。
  4. 三处 emit 后,更新 `every_variant_round_trips` fixture 测试已覆盖(无需新增)。
- **验收**:
  - [ ] `grep -rn "HookEvent::UserPromptExpansion\|HookEvent::InstructionsLoaded\|HookEvent::ConfigChange"` 在生产代码有 emit 点
  - [ ] 手测:改 `.shannon.toml` 触发 ConfigChange hook
- **估时**:1d · **依赖**:无 · **风险**:低

---

## 2. Wave 2:补齐短板(~6–8w)

目标:补两条赛道的硬短板(编码线 repo map/auto-test;办公线 SaaS 集成)。

### P1-3 · MCP 化补 SaaS 集成 ⭐(办公线最高优先)
- **描述**:把 5 个高频 SaaS 做成 MCP server(GitHub Issues/PR → Slack → Jira → Notion → Linear)。
- **文件锚点**:
  - 新增 `crates/shannon-mcp-saas/`(或独立 MCP server 仓库)
  - 复用 `desktop/src/commands_connections.rs`(已有 platform credential/keyring 雏形)
  - 复用 `desktop/src/gateway_supervisor.rs`(connector gateway 监督)
  - 配置:`~/.shannon/mcp-servers.json`
- **实施步骤(每个 SaaS 2–3d)**:
  1. **GitHub**(Issues/PR):用 GitHub REST/GraphQL API + 现成 MCP 模板,实现 tools: `list_issues`/`get_issue`/`create_issue`/`comment`/`list_prs`/`review_pr`。OAuth/token via keyring。
  2. **Slack**:用 Slack Web API,tools: `post_message`/`search`/`read_channel`/`thread_reply`。复用 `commands_connections.rs` 的 `slack/bot-token` keyring key。
  3. **Jira**:REST API,tools: `search_issues`/`get_issue`/`create_issue`/`transition`。
  4. **Notion**:Notion API,tools: `search_pages`/`get_page`/`append_block`/`create_page`。
  5. **Linear**:GraphQL API,tools: `list_issues`/`get_issue`/`create_issue`/`update_status`。
  6. 每个 server:OAuth flow(复用 gateway broker)+ API key fallback + 速率限制处理 + 工具粒度权限控制(接 Shannon 权限分级)。
  7. 文档:每个 SaaS 一篇 `docs/integrations/<saa>.md`,含配置与权限。
- **验收**:
  - [ ] 5 个 MCP server 各自 `tools/list` 可被 Shannon 发现
  - [ ] 每个 server 有 mockito/回放测试
  - [ ] REPL 实测:经 Shannon 触发 GitHub create_issue + Slack post 端到端通过
  - [ ] 密钥走 keyring,不落明文
- **估时**:2–3w · **依赖**:无 · **风险**:中(OAuth/速率限制;但有 gateway 雏形可复用)· **来源**:[openworker](./openworker-research.md) §9.2

### P1-4 · tree-sitter repo map(编码线最高优先)
- **描述**:构建全 repo 符号地图(类/函数/类型/调用签名),随请求发给 LLM。
- **文件锚点**:新增 `crates/shannon-repomap/`(依赖 tree-sitter + 语法 grammars);接入 `crates/shannon-core/src/query_engine.rs`(注入 system prompt)
- **实施步骤**:
  1. 新 crate `shannon-repomap`:用 tree-sitter 解析 repo,提取符号(先支持 Rust/TS/Python/Go,后扩)。
  2. 实现 token 预算控制(参照 Aider pack 算法,按重要性裁剪到 ~2–4K tokens)。
  3. 增量更新:监听文件变更(`notify`),缓存符号树,避免每次全量解析。
  4. 接 query_engine:每次请求把 repo map 注入 system prompt(可配置开关 + 预算)。
  5. 测试:多语言 fixture repo + token 预算断言。
- **验收**:
  - [ ] 4 种语言的 fixture repo 能产出符号地图
  - [ ] 地图 token 数在预算内(可配置)
  - [ ] 增量更新比全量快 >5x
- **估时**:2–3w · **依赖**:无 · **风险**:中(多语言 grammar 维护、预算调优)

### P1-5 · auto-test loop
- **描述**:编辑 → 跑测试 → 修失败 → 循环至通过。
- **文件锚点**:`crates/shannon-core/src/query_engine.rs`(新循环);`crates/shannon-tools/src/bash.rs`(测试执行)
- **实施步骤**:
  1. query_engine 加 `auto_test` 模式:工具执行后自动跑配置的测试命令(`cargo nextest`/`npm test`)。
  2. 解析失败 → 注入失败信息到下一轮 LLM 上下文 → LLM 修复 → 重复。
  3. 防死循环:最大轮次、超时、无进展检测。
  4. 配置:`.shannon.toml` 的 `[auto_test]` 段(command、max_iterations、timeout)。
- **验收**:
  - [ ] 故意引入失败测试,agent 自动修复至通过
  - [ ] 死循环防护(超轮次/超时)生效
- **估时**:1–2w · **依赖**:无 · **风险**:中(死循环、超时)

### P2-1 · Compact 多策略接通
- **文件锚点**:`crates/shannon-core/src/compact.rs`
- **实施步骤**:接通 token-based / summary-based 策略;按对话类型自适应(token 数、消息数、是否含代码);加策略选择单测 + 压缩质量回归。
- **估时**:1w · **依赖**:P1-4(repo map 辅助摘要) · **风险**:低

### P2-3 · 测试工程缺口(insta + 不变量)
- **文件锚点**:引入 `insta` crate;新增 `tests/architecture_invariants.rs`;强化 `tests/` MCP mock
- **实施步骤**:
  1. insta:对 chat 渲染输出、命令输出建 snapshot;`cargo insta review` 流程。
  2. 架构不变量:用 `cargo-deny`/自定义脚本断言 crate 间无非法依赖、stable_api 标记完整。
  3. MCP mock:补全 MCP server 行为模拟。
- **估时**:2w · **依赖**:P0-3 · **风险**:低

### P2-4 · CI Rust 门禁修复
- **描述**:解决 Gitea runner 连不上 github.com 导致 CI 是 UI-only。
- **文件锚点**:`.github/workflows/`(或 Gitea 等价)、`desktop/scripts/hooks/pre-push`
- **实施步骤**:评估三方案 —— ① 自托管 runner 可访问内网 mirror;② vendor 依赖(`cargo vendor`)离线构建;③ 镜像 github.com 依赖到私有 registry。选其一落地,CI 跑 `cargo clippy -- -D warnings` + `cargo nextest` + `cargo deny`。
- **估时**:1–2w · **依赖**:无 · **风险**:中(需运维)

---

## 3. Wave 3:扩张与差异化(~6–8w)

### P2-5 · 桌面 chat 升级(以 assistant-ui 为骨架)⭐(任务 b 核心)
- **描述**:执行 [aichat-ui 评审 v2](./aichat-ui-library-evaluation.md) §6.2 的五步实施 —— runtime adapter + 多线程 + 附件 + 美化 + 语音。
- **总估时**:4–6w(单人),可与 Wave 3 其它桌面任务部分并行。
- **五子任务(均含文件锚点 + 步骤 + 验收)**:

#### P2-5a · Runtime adapter(关键使能件,3–5d)
- **文件锚点**:新增 `desktop/ui/src/lib/runtime/shannonTauriRuntime.ts`;改 `desktop/ui/src/context/ChatContext.tsx`
- **实施步骤**:
  1. 装 `@assistant-ui/react`(`pnpm add @assistant-ui/react` —— `desktop/ui/`)。
  2. 实现 `ShannonTauriRuntime` 实现 `ExternalStoreRuntime`:订阅 Tauri 事件(`query:text`/`tool-start`/`tool-result`/`thinking`/`completed`),映射到 assistant-ui `ThreadMessage` 状态。
  3. 实现 `ChatModelAdapter`:`onNewMessage` → 调 `invoke('send_message', ...)`,流式事件回流。
  4. 在 `ChatContext` 用 `<AssistantRuntimeProvider runtime={shannonRuntime}>` 包裹。
  5. 锁定 assistant-ui 版本;配 insta snapshot 防接口漂移。
- **验收**:
  - [ ] 单 thread 下,assistant-ui Thread 能渲染 Shannon 的文本/tool/thinking 消息
  - [ ] 流式输出 + 滚动锚定正常
  - [ ] 旧 `Chat.tsx` 行为不回归(feature flag 回退可用)

#### P2-5b · 多线程管理(4–6d)
- **文件锚点**:新增 `desktop/ui/src/components/chat/ThreadSidebar.tsx`、`ThreadTabs.tsx`;改 `desktop/ui/src/pages/Chat.tsx`;复用 `desktop/src/commands_sessions.rs`(branch_session)
- **实施步骤**:
  1. 验证 `QueryCoordinator` 是否支持多 query 并发(若否,补并发隔离)。
  2. 用 assistant-ui Thread 抽象:每线程一个 `Thread` 实例,共享 runtime 但独立 state。
  3. 前端 `ThreadSidebar`:列出线程、新建、切换、fork(从某消息 branch_session)、重命名、删除。
  4. adapter 层做线程→session 映射 + 事件路由(避免串线)。
  5. UI:未读指示、最后消息预览、活跃线程高亮。
- **验收**:
  - [ ] 可同时运行 ≥3 线程,各自独立流式输出不串线
  - [ ] fork 从指定消息分叉出新线程
  - [ ] 切换线程上下文不丢失

#### P2-5c · 附件上传(3–5d)
- **文件锚点**:新增 `desktop/ui/src/components/chat/AttachmentChip.tsx`;改 `desktop/ui/src/components/chat/ChatInput.tsx`、`desktop/ui/src/lib/tauri-api.ts`;改 `desktop/src/commands_files.rs`、`desktop/src/events.rs`
- **实施步骤**:
  1. 后端 `commands_files.rs` 加 `read_attachment(path) -> AttachmentPayload { mime, base64?, text?, name, size }`。
  2. 图片(png/jpg/webp/gif)→ `ContentBlock::Image`;文本/代码 → `ContentBlock::Text`;PDF → 文本提取(先简单 pdftotext 回退,后接 P1-1 的 /pdf)。
  3. 限制:单文件 25MB、类型 allowlist、总附件数 ≤10(配置化)。
  4. 前端:assistant-ui attachment primitives + Composer;拖放区 + chip 预览(图片缩略图/文件图标/大小)+ 删除。
  5. 发送时把附件映射为 `ContentBlock[]` 拼到消息。
- **验收**:
  - [ ] 图片/文本/PDF 三类可上传并发给 LLM(vision 模型能看图)
  - [ ] 拖放 + 点选 + 多文件 + 删除可用
  - [ ] 超限/超类型的拒绝提示清晰

#### P2-5d · 整体美化升级(5–7d)
- **文件锚点**:重构 `desktop/ui/src/components/chat/MessageBubble.tsx`、`Markdown.tsx`、`StreamingResponse.tsx`、`pages/Chat.tsx`;`desktop/ui/tailwind.config`/设计 token 文件
- **实施步骤**:
  1. **设计 token**:统一 Tailwind theme(色板/间距/圆角/字号/阴影)+ 完整暗色模式 + Shannon 品牌色;建 `desktop/ui/src/styles/tokens.css`。
  2. **消息气泡**:assistant-ui Message 组件替换自研;markdown 优化(代码块复制/语言标签/行号、引用块、表格、链接预览)。
  3. **流式体验**:打字动画、滚动锚定(assistant-ui)、tool-call/thinking 折叠块、artifact 卡片(接 P3-1)。
  4. **输入栏**:assistant-ui Composer;多行、快捷键(Ctrl+Enter 发送)、斜杠命令提示、@提及、上下文预览。
  5. **微交互**:加载骨架、错误/空态、通知(sonner)、过渡(motion)。
  6. **a11y**:键盘导航、ARIA、对比度 AA、focus 管理。
- **验收**:
  - [ ] insta 视觉回归基线通过(明/暗模式)
  - [ ] Lighthouse a11y ≥90
  - [ ] 主观对比 Claude/Codex Desktop 不显落后(人工评审)

#### P2-5e · 语音消息(5–7d)
- **文件锚点**:改 `desktop/ui/src/lib/voice/`(已存在!);新增 `desktop/src/commands_voice.rs`(whisper-rs);改 `ChatInput.tsx`(mic UI)
- **实施步骤**:
  1. 后端:加 `whisper-rs` 依赖;新命令 `transcribe_audio(path, model, lang) -> { text, confidence }`;模型按需下载到 `~/.shannon/models/whisper/`。
  2. 设置:模型档位选择(tiny/base/small/medium)、语言、是否本地优先。
  3. 前端:Web Audio API 采集 mic → 编码 wav → 写临时文件 → 调 `transcribe_audio`。
  4. UI:输入栏麦克风按钮、录音指示、波形动画(motion)、停止 → 转写 → 注入 composer(可编辑)。
  5. 错误处理:无麦克风权限、模型未下载、转写低置信度提示。
- **验收**:
  - [ ] 中英文录音可转写为文本填入输入栏
  - [ ] 模型按需下载,不打包进安装包
  - [ ] 无网络环境下(本地模型)可用
- **风险**:whisper-rs 模型体积 → 按需下载缓解

### P2-2 · ADR-0005 Phase 2 桌面 re-platforming(2–3w)
- **文件锚点**:`crates/shannon-core/src/provider.rs`(ProviderProfile);`desktop/src/commands_config.rs`、`commands_providers.rs`;`desktop/src/main.rs`
- **实施步骤**:
  1. 桌面 provider/credential 路径全迁到共享 `ProviderProfile` + 统一 credential store(keyring)。
  2. 删除桌面私有 provider 配置副本,改读引擎统一配置。
  3. 走 STABILITY deprecation 周期(旧 API 标 deprecated,`cargo-semver-checks` 把关)。
  4. 验证 CLI 与桌面 provider/credential 行为完全一致。
- **验收**:CLI 与桌面 connect/model/tier/refresh 行为逐项对齐;无双路径。
- **依赖**:P0-1(ADR-0008 已统一 tier 判定)· **风险**:中(跨 crate 签名)

### P2-6 · auto-commit + Undo/快照(2w)
- **文件锚点**:`crates/shannon-tools/src/`(git 工具);`crates/shannon-core/src/`(编辑管线);先梳理现有 auto-commit hook(见 [project-review](./project-review-2026-08.md) §3.4)
- **实施步骤**:
  1. 梳理现有 PostToolUse auto-commit hook 的拆分行为,决定保留/改造/移除。
  2. auto-commit:每次 AI 编辑后,带上下文消息提交(可选开关)。
  3. Undo/快照:编辑前 git stash/snapshot,`/rewind` 扩到文件级。
- **依赖**:无 · **风险**:中(与现有 hook 冲突)

### P2-7 · `shannon serve` HTTP API(2–3w)
- **文件锚点**:新增 `crates/shannon-server/`(axum);`crates/shannon-cli/src/`(serve 子命令)
- **实施步骤**:axum HTTP 层 + auth(token/OAuth)+ session 管理 + SSE 流式;复用 QueryEngine;OpenAPI 文档。
- **依赖**:无 · **风险**:中(安全面)

### P2-8 · VS Code 扩展完善(2–3w)
- **文件锚点**:`legacy-archives/` 下的 VS Code 扩展源(需迁移到活跃目录);通信改用 P2-7 的 HTTP API(比 NDJSON 子进程更稳)
- **依赖**:P2-7 · **风险**:中(发布流程)

---

## 4. P3 — 长期(记录,不排期)

| 编号 | 任务 | 估时 | 来源 |
|---|---|---|---|
| P3-1 | artifact 显性化(交付成品>聊天) | 2w | openworker §9.3;依赖 P2-5d |
| P3-2 | 无人值守 inbox(长跑 agent 安全闭环) | 1.5w | openworker §9.4 |
| P3-3 | Computer Use 完善(跨平台,不依赖 macOS AX) | 3w | desktop-product-analysis |
| P3-4 | 桌面状态层迁移 SQLite | 2w | project-review §2.4 |
| P3-5 | Deep LSP 集成(对标 OpenCode 25+) | 3–4w | matrix §2.2 |
| P3-6 | 移动端派发(对标 WorkBuddy) | 8–12w | WorkBuddy |
| P3-7 | 沙盒执行(Landlock/seccomp/Seatbelt) | 4–6w | matrix §2.2 安全基础 |
| P3-8 | Cross-surface / Cloud / Agent SDK / Skills marketplace | 各 4–12w | ROADMAP-FUTURE P3 |

---

## 5. 实施路线图(3 Wave)

### Wave 1:收敛对齐(本月 ~1.5w)
P0-1(QA 合并)+ P0-2(文档)+ P0-3(metrics)+ P1-2(hook events)+ P1-1(dead code)
**出口**:文档/度量可信、分支合并、hook 做实、dead code 清。

### Wave 2:补短板(~6–8w)
| 任务 | 估时 | 并行? |
|---|---|---|
| P1-3 SaaS MCP ⭐ | 2–3w | ✅ |
| P1-4 repo map | 2–3w | ✅ |
| P1-5 auto-test loop | 1–2w | ✅ |
| P2-1 Compact 多策略 | 1w | 依赖 P1-4 |
| P2-3 insta + 不变量 | 2w | ✅ |
| P2-4 CI Rust 门禁 | 1–2w | ✅ |
**出口**:双赛道硬短板补齐。

### Wave 3:扩张差异化(~6–8w)
| 任务 | 估时 | 并行? |
|---|---|---|
| P2-5 桌面 chat 升级 ⭐(5 子任务) | 4–6w | ✅(子任务内部有序) |
| P2-2 桌面 re-platforming | 2–3w | ✅ |
| P2-6 auto-commit+Undo | 2w | ✅ |
| P2-7 shannon serve | 2–3w | ✅ |
| P2-8 VS Code 扩展 | 2–3w | 依赖 P2-7 |
| P3-1 artifact 显性化 | 2w | 依赖 P2-5d |
**出口**:chat 体验跃升、桌面/CLI parity、程序化入口打开。

---

## 6. 依赖关系图(关键路径)

```
P0-1(ADR-0008 QA)─┬─→ P2-2(桌面 re-platforming)
P0-3(metrics)─────┴─→ P2-3(snapshot 测试)─→ P2-5d(美化,需视觉基线)
P0-2(文档)──────────→ (独立)

P1-1(dead code)─────→ (独立)
P1-2(hook events)────→ (独立)

P1-3(SaaS MCP)──┬─→ P3-2(无人值守 inbox)
P1-4(repo map)──┴─→ P2-1(Compact 多策略)

P2-5a(runtime adapter)─→ P2-5b(多线程)─→ P2-5c(附件)+ P2-5d(美化)─→ P3-1(artifact)
P2-5e(语音)───────────→ (独立子任务)

P2-7(serve)─→ P2-8(VS Code)
```

**关键路径**:
- 编码线:P1-4 → P2-1(repo map → 压缩质量)
- 办公线:P2-5a → P2-5b → P2-5d → P3-1(chat 升级 → artifact)

---

## 7. 风险登记册

| 风险 | 概率 | 影响 | 缓解 |
|---|---|---|---|
| P1-3 SaaS OAuth/速率限制踩坑 | 中 | 中 | API key 优先,OAuth 后置;复用 gateway 雏形 |
| P1-4 repo map token 预算难调 | 中 | 中 | 参照 Aider pack 算法,自适应裁剪 |
| P2-2 跨 crate 签名引发 semver 违规 | 中 | 高 | 走 STABILITY deprecation;semver-checks 把关 |
| P2-5a runtime 接口 beta 漂移 | 中 | 高 | 锁版本 + snapshot + fork 准备 |
| P2-5b 多线程并发资源竞争 | 中 | 中 | adapter 线程隔离;QueryCoordinator 并发验证 |
| P2-5e whisper-rs 模型体积 | 中 | 中 | 按需下载,不打包 |
| P2-4 CI runner 依赖运维 | 高 | 中 | 临时:PR 模板要求贴本地 clippy/test 输出 |
| 范围蔓延 | 高 | 中 | 严格按本计划;新需求进 backlog |
| auto-commit hook 拆分提交 | 高 | 低 | P2-6 先梳理现有 hook 行为 |

---

## 8. 评审请求(v2)

请 ericdong 复核 v2 增量(其余 v1 已授权):

1. **双赛道战略视图**(§0):编码线/办公线分线补短 —— 是否认可?
2. **桌面 chat 升级五子任务**(P2-5a–e):runtime adapter → 多线程 → 附件 → 美化 → 语音 的顺序与拆分是否合理?是否调整子任务优先级?
3. **chat 升级以 assistant-ui 为骨架**(P2-5):是否同意(替代 v1 的"渐进选择性采纳")?
4. **语音走 whisper-rs 本地**(P2-5e):是否同意(对比云 Whisper API)?
5. **多线程并发**(P2-5b):QueryCoordinator 是否已支持多 query 并发,需我先验证?
6. **任务细化粒度**(§1–§3 文件锚点 + 步骤 + 验收):是否够细?哪些任务要再展开?
7. **P3 排期**(§4):P3-1~P3-8 哪些应提前到 Wave 3?

**评审通过后,我会**:
- 把确认的任务(尤其 P2-5 五子任务)拆成独立 `docs/plans/<task>.md` 可执行方案(仿 `provider-model-command-remediation.md`);
- 验证 P2-5b 的 QueryCoordinator 并发能力;
- 建立 Wave 1 的任务跟踪;
- 按授权开始执行(**评审通过前不开工**)。

---

*v2 由双赛道分析 + chat 升级需求驱动,所有判断有据可查。请评审。*
