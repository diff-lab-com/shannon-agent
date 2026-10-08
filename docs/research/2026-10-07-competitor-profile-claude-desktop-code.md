# 竞品画像:Claude Desktop 的 Code tab(Anthropic)

> 调研于 2026-10-07,由联网调研代理核实,每条事实附来源 URL;[inferred]/[unverified] 为未能直接核实的项。
> 本文是 [R3 主报告](2026-10-07-desktop-ui-competitive-review-r3.md) 的附件。

## 0. 产品形态判定

**不存在独立的 Claude Code 安装包**。官方所谓 "Claude Code desktop app" 即 **Claude Desktop 应用(claude.com/download)内的 "Code" tab** —— 同一安装器、同一窗口,三 tab 结构为 Chat / Cowork / Code。判定依据:官方文档 "The Claude Desktop app has three tabs: Chat…Cowork…Code. This page is the reference for the Code tab"([code.claude.com/docs/en/desktop](https://code.claude.com/docs/en/desktop));官方公告让用户 "Download or update the Claude desktop app to get started"([x.com/claudeai](https://x.com/claudeai/status/2044131493966909862));VentureBeat 实测确认 "It's not a separate new application"([VentureBeat](https://venturebeat.com/orchestration/we-tested-anthropics-redesigned-claude-code-desktop-app-and-routines-heres-what-enterprises-should-know))。

## 1. 产品基本面

- **平台**:macOS + Windows(含 ARM64);Linux 走 apt,2026-06-30 起 beta。([docs](https://code.claude.com/docs/en/desktop))
- **时间线**:Code tab 2026 年初已存在;2026-02-20 官方博客 "Bringing automated preview, review, and merge to Claude Code on desktop";03-23/24 computer use 进 Code tab(research preview,macOS,Pro/Max)([kingy.ai](https://kingy.ai));**2026-04-14 大重构**:多会话侧栏 + 拖放布局 + 集成终端/编辑器 + Routines;2026-09-16 Cowork 并入主聊天;09-23 云会话 GA。([claude.com](https://claude.com))
- **与 CLI 关系**:桌面与 CLI 同引擎、共享 CLAUDE.md/MCP/hooks/settings;CLI 内 `/desktop` 把会话搬进桌面,桌面内 `/resume` 接续 CLI 会话,`claude --desktop` 直开桌面;云会话可在 claude.ai/code 与手机 App 监控。([docs](https://code.claude.com/docs/en/desktop))
- **定价**:Pro($20/月)/Max($100/200)/Team/Enterprise 订阅,或 API key。

## 2. 信息架构

- 左侧栏 = 会话列表(+ New session、按状态/项目/环境过滤、按项目分组、归档、活动统计看板);侧栏还有 Customize(connectors/skills/plugins 统一管理)与 Projects(云会话项目)。([miraflow 指南](https://miraflow.ai/blog/claude-code-desktop-redesign-parallel-sessions-routines-workspace-guide))
- **窗格清单**:chat、diff、browser、terminal、file editor、plan、tasks(subagent)、iOS Simulator、side chat —— **全部可拖放成任意网格、可弹出到独立窗口**。([docs](https://code.claude.com/docs/en/desktop))

## 3. 关键界面深挖

- **Onboarding**:登录 → Code tab;首条消息前在 prompt 区配置四件事:**Environment(Local/Cloud/SSH/WSL)、Project folder、Model、Permission mode**。桌面端要求选择具体子文件夹,而非 CLI 式用户目录起会话。([docs](https://code.claude.com/docs/en/desktop)、[VentureBeat](https://venturebeat.com/orchestration/we-tested-anthropics-redesigned-claude-code-desktop-app-and-routines-heres-what-enterprises-should-know))
- **并行会话**:Cmd+N 新建;Ctrl+Tab 循环;Git repo 可选 worktree 隔离(存于 `.claude/worktrees/`);**Cmd+点击侧栏会话可双窗格同看两会话**;PR merge/close 自动归档;**会话间可互发消息、Claude 可代管其他会话**。([docs](https://code.claude.com/docs/en/desktop))
- **Diff 审查**:会话内出现 `+12 -1` 指示器 → 点开 diff viewer(左文件列表/右变更),**点任意行加行内评论,Cmd+Enter 批量提交评论让 Claude 修改**;仅 unified 视图。([docs](https://code.claude.com/docs/en/desktop)、[wmedia](https://wmedia.es/en/tips/claude-code-desktop-5-features-over-cli))
- **权限/审批**:发送按钮旁 mode selector(Cmd+Shift+M):**Manual(逐项 diff 批准)/ Accept edits / Plan(只读出方案)/ Auto(后台分类器把关)/ Bypass(需 Settings 开关)**;外部网站操作弹 "Allow once/Always allow/Deny" 权限卡;computer use 按应用分 View only/Click only/Full control 三级固定档位。([docs](https://code.claude.com/docs/en/desktop))
- **设置组织**:Settings → Claude Code(worktree 位置、auto-archive、bypass 开关、Browser tools)、Connectors、This computer → System(computer use 拒绝名单);读同一套 CLI settings 文件,支持 managed settings/MDM 企业管控。

## 4. 组件清单

Composer:@文件补全、附件(图片/PDF)、`+` 按钮(skills/斜杠命令/connectors/plugins)、模型下拉(可中途换,Cmd+Shift+I)、effort 档(Cmd+Shift+E)、权限模式选择器、usage ring(悬停看 context 与套餐用量)、CI 状态栏(auto-fix/auto-merge 开关)、view modes(Normal/Thinking/Verbose,Ctrl+O)、OS 通知(会话完成且不在看时)、orange asterisk 实时显示耗时与 token。**checkpoints//rewind 是 CLI 能力(Esc Esc),桌面端文档未见 rewind UI**(基于官方文档 absence)[inferred]。([docs](https://code.claude.com/docs/en/desktop))

## 5. User Stories(示例 12 条)

作为开发者,我可以:1) 在一个窗口并排跑 4 个跨 repo 会话并轮流推进;2) 让每个会话自动获得 worktree 隔离分支;3) 在 diff 上点行留评论批量让 Claude 修;4) 开 PR 后让 Claude auto-fix CI 并 squash merge;5) 在 Browser 窗格看 Claude 自动截图/DOM 验证自己的改动;6) Cmd+; 侧聊提问不污染主线程;7) 让 Claude 转告另一会话 schema 变了;8) 点任务 chip 把顺手发现的活儿开成新 worktree 会话;9) 手机 Dispatch 下指令生成带徽章的 Code 会话;10) 把 CLI 会话 /desktop 搬进窗口继续;11) 把长任务抛给 Cloud 关机后继续;12) nightly Routines 自动跑代码评审。

## 6. 视觉设计语言

窗格化 IDE 式布局;当前会话高亮、其余变暗;蓝色文本气泡、小图标按钮;顶栏 Chat|Cowork|Code 三 tab。([prosperinai](https://prosperinai.substack.com/p/claude-code-terminal-vs-desktop-app)、[VentureBeat](https://venturebeat.com/orchestration/we-tested-anthropics-redesigned-claude-code-desktop-app-and-routines-heres-what-enterprises-should-know))具体色值/字体未经截图取证 [unverified]。

## 7. 独有交互

- **侧聊**(`Cmd+;` / `/btw`):只读上下文提问,不回写主线程。
- **跨会话消息卡**:带来源署名与归档前确认;任务 chip 派生新 worktree 会话。
- **Dispatch 手机派发** + 30 分钟复批;**Ultraplan** 云端计划评审(行内评论+emoji 反应+"teleport back to terminal");iOS Simulator 专窗格;per-app computer use 三级权限。([docs](https://code.claude.com/docs/en/desktop)、[miraflow](https://miraflow.ai/blog/claude-code-desktop-redesign-parallel-sessions-routines-workspace-guide))

## 8. 已知弱点/批评

- Windows 自动更新后会话内容丢失([issue #53717](https://github.com/anthropics/claude-code/issues/53717));集成终端输入延迟、多项目(3+)并行卡顿、单项目锁定的可发现性差、第三方插件不显示、"walled garden" 模型锁定、Code 入口藏在 Chat 图标 hover 后([VentureBeat](https://venturebeat.com/orchestration/we-tested-anthropics-redesigned-claude-code-desktop-app-and-routines-heres-what-enterprises-should-know)、[Reddit](https://www.reddit.com/r/ClaudeCode/comments/1v4fwjv/));无多窗口、diff 仅 unified、布局更新后不保存、早期不稳定([miraflow](https://miraflow.ai/blog/claude-code-desktop-redesign-parallel-sessions-routines-workspace-guide));社区吐槽 "toy-like" 美学、蓝色气泡刺眼、图标小且不直观([Reddit](https://www.reddit.com/r/ClaudeAI/comments/1slictc/));并行会话 2-3x token 消耗压力。
