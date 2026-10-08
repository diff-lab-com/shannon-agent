# 竞品画像:OpenAI Codex app(桌面端)

> 调研于 2026-10-07,由联网调研代理核实,每条事实附来源 URL;[inferred] 为从已核实事实推导的 UX 推论。
> 本文是 [R3 主报告](2026-10-07-desktop-ui-competitive-review-r3.md) 的附件。

## 1. 产品基本面

- **名称/定位**:官方名 "Codex app",定位 "command center for agents"。([OpenAI 发布文](https://openai.com/index/introducing-the-codex-app/)、[官方文档](https://developers.openai.com/codex/app))
- **平台时间线**:2026-02-02 macOS(Apple Silicon 起步,后补 Intel);2026-03-04 Windows(原生 PowerShell + 原生沙箱,不需 WSL);无 Linux(官方仅 "Get notified for Linux")。([OpenAI](https://openai.com/index/introducing-the-codex-app/)、[docs](https://developers.openai.com/codex/app)、[PCWorld](https://www.pcworld.com/article/3078509/openais-ai-coding-app-comes-to-windows-at-last.html))
- **重大变化**:2026-07-09 并入新 ChatGPT 桌面应用(Chat / Work / Codex 三模式),可设 Codex 为默认视图;旧 ChatGPT 桌面端改名 "ChatGPT Classic";独立 Codex 工作区仍在。([OpenAI](https://openai.com/index/chatgpt-for-your-most-ambitious-work))
- **与 CLI/web 关系**:自动继承 CLI/IDE 扩展的项目、session 历史与配置;每线程可选 **Local / Worktree / Cloud** 三种执行环境;与 IDE 双向同步活动线程。([发布文](https://openai.com/index/introducing-the-codex-app/)、[features](https://developers.openai.com/codex/app/features))
- **定价**:ChatGPT 订阅内含(Plus/Pro/Business/Enterprise/Edu),可购额外 credits;API key 登录可用但部分功能受限。

## 2. 信息架构

- **三栏布局**:项目侧栏 + 活动线程(对话)+ review 面板。([docs](https://developers.openai.com/codex/app))
- **侧栏分区**:Projects(线程按项目组织,可 pin/archive)、Automations(深链 `codex://automations`)、Skills(内置技能商店)、**Triage/审查队列**(自动化产出收件箱)、Chats(无项目自由聊天)。([features](https://developers.openai.com/codex/app/features)、[第三方知识库](https://codex.danielvaughan.com/2026/04/08/codex-desktop-automations))
- **全局件**:Cmd+K 命令面板、Cmd+J 内置终端(按项目/worktree 作用域,agent 可读终端输出)、线程可弹窗为独立窗口并置顶。

## 3. 关键界面深挖

- **Onboarding**:登录(ChatGPT 账号或 API key)→ 选项目文件夹 → 确认 "Local" 模式后发首条消息;附 3 个建议提示词;老用户自动显示历史项目。([docs](https://developers.openai.com/codex/app))
- **Diff 审查**(核心环,[docs/review](https://developers.openai.com/codex/app/review)):
  - 范围可切「未提交 / 整分支 / 最后一轮 turn」,Unstaged/Staged 切换;
  - 整 diff / 文件 / **hunk 三级** stage 与 revert;
  - **行级悬停 "+" 按钮加内联评论,回消息即让 agent 处理**;
  - `/review` 结果内联显示;PR 审查:侧栏显示 GitHub reviewer 评论与 diff 并排(需 `gh` 认证)。
- **权限/审批**:沙箱 + Approvals 双层;弹窗提供 "approve once / approve for this session" 粒度;默认只许改工作目录内文件,联网等提权需批准。([features](https://developers.openai.com/codex/app/features))
- **环境配置**:云环境 = 仓库/依赖/secrets/出网策略/install script,由 Codex 自动准备试跑后人工 publish;本地环境可定义 "action" 快捷按钮置于窗口顶部。([help.openai.com](https://help.openai.com/en/articles/20001545-using-codex-cloud))
- **设置组织**:MCP(三端共享配置)、通知(后台默认/永不/始终)、"Prevent sleep while running"、默认编辑器、浏览器插件允许/屏蔽站点列表。

## 4. 组件清单

- **Composer**:拖放图片(按住 Shift 入上下文)、`$skill-name` 显式技能调用、Ctrl+M 按住说话语音输入、IDE context 开关、Cmd+Enter 提交长提示词、可配置追问行为(排队 vs 打断)。([features](https://developers.openai.com/codex/app/features))
- **进度呈现**:右侧任务侧栏实时展示 agent 的 plan / sources / artifacts / task summary;artifact viewer 预览 PDF/表格/幻灯片。([OpenAI](https://openai.com/index/codex-for-almost-everything/))
- **其它**:多终端 tab、内嵌浏览器(基于 Atlas)页面圈注评论、Appshots(把最前台 Mac 窗口截图+文本发给 Codex)。

## 5. User Stories(官方文档归纳)

作为开发者,我可以:1) 一键并行开多个 agent 线程并互不冲突(worktree 隔离);2) 在 diff 行内写评论让 agent 定点修改;3) 按 hunk 挑选接受/丢弃改动后直接 commit/push/开 PR;4) 在侧栏读完 GitHub PR 评审意见并让 agent 逐条修复;5) 设置 cron/手动/webhook 自动化,结果进 Triage 队列审批;6) 让已结束线程定时"心跳式"唤醒继续长任务;7) 从手机(ChatGPT mobile)接管/批准本机 agent 任务;8) 用语音对着 composer 口述需求;9) 用内嵌浏览器在页面元素上圈注反馈驱动前端修改;10) 让 Codex 后台操作其它 macOS App(computer use);11) 用 `$imagegen` 在线程内生成 UI 素材;12) 把线程弹窗置顶放在浏览器旁做前端迭代;13) 发布可复用云环境让任务在合盖后继续跑;14) 切换 agent 人格(terse / conversational)。

## 6. 视觉设计语言

官方提供 light/dark 双主题;发布宣传以深色为主。评测:"clean UI";亦有 "minimalist dark UI、面板间距偏紧、代码与系统消息排版区分度不足、快捷键可发现性差" 的批评。([every.to](https://every.to/vibe-check/codex-vibe-check)、[wetheflywheel](https://wetheflywheel.com/en/guides/codex-desktop-app-review))

## 7. 独有交互

- **Computer use 虚拟光标**:思考时光标摆动、路径 "playful"、颜色取自系统壁纸;权限授予流程被评为三方 Mac 应用最佳(源自 Sky 收购团队)。([MacStories](https://www.macstories.net/notes/openais-new-codex-app-has-the-best-computer-use-feature-ive-ever-tested))
- **Diff 即反馈通道**:在 diff 行内评论驱动 agent 返工,浏览器页面同理。([every.to](https://every.to/vibe-check/codex-vibe-check))
- **Automations + Triage 收件箱**:定时任务产出进审批队列(diff/批准/修改/拒绝四选)。([danielvaughan](https://codex.danielvaughan.com/2026/04/08/codex-desktop-automations))

## 8. 已知弱点/批评

- 界面 "functional, not refined";仅 OpenAI 模型(锁定);云端执行对合规团队不可接受。([wetheflywheel](https://wetheflywheel.com/en/guides/codex-desktop-app-review))
- 社区实测:卡死/转圈、交互式 shell 与长命令摩擦、worktree UX 生硬、环境变量管理不清晰。([dev.to 汇总](https://dev.to/sivarampg/openais-new-codex-mac-app-is-an-agent-command-center-and-its-lighting-up-the-devtool-wars-3aek))
- GitHub issues:审批弹窗卡在 "Awaiting approval" 阻塞后续;Windows 应用内更新后不自动重启;Windows 版静默退出。([community.openai.com](https://community.openai.com))
- 侧栏搜索只覆盖任务标题,缺项目/文件内容搜索。([community](https://community.openai.com))
- 2026-07 合并惹议:"OpenAI gutted ChatGPT Desktop"(["OpenAI gutted ChatGPT Desktop"](https://www.zdnet.com/article/openai-gutted-chatgpt-desktop-app-for-codex-work/));重度用户认为 GUI 相对 CLI 是倒退。([every.to](https://every.to/vibe-check/codex-vibe-check))
