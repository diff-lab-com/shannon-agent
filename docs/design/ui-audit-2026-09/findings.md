# Findings: Shannon Desktop UI 审计（2026-09-10）

## Shannon Desktop 页面清单（App.tsx 实测）
/welcome · /chat · /tasks · /triage · /usage · /extensions(+featured|mcp-servers|skills|agents|datasources|plugins|installed) · /opc(+task/:id) · /editor · /memory · /timeline/:id · /settings(+general|theme|models|permissions|advanced|notifications|connections|remotes)
- Mock 演示模式：`pnpm demo`（VITE_MOCK_MODE=1 vite）→ http://localhost:1420
- 技术栈：Tauri 2 + React 19 + Tailwind v4 + MD3 token，react-router-dom 7
- 主题：多套生成主题（generate:themes 脚本）

## 2026-06 历史审计确认的问题（来源 05b/05c/COMPETITIVE-ANALYSIS，经子代理提炼）
1. **术语黑话**：OPC/Triage/Hooks 等开发者术语 vs 竞品平实英文 Tasks/Inbox/Automations
2. **固定三栏布局**：不可拖拽重组（Claude Code Desktop 已是可拖拽面板按 repo 保存）
3. **品牌感弱**：material-blue 开发者外壳 vs Claude terracotta/奶油暖色消费级质感
4. **组件债务**：Button 绕过率 ~50%、25 处手搓 modal、4 种卡片配方、focus ring 缺失（WCAG 违规）
5. **侧栏状态失明**：无 HIL 待审批/失败 routine/未读 triage badge
6. **导航状态不持久**：Tasks tab、Triage 筛选、Chat 面板开合均 local state
7. **Onboarding 4 步** vs Claude/ChatGPT 2 步
8. **无 Project/工作区概念**（Codex/Cursor 有）
9. Skills 子系统 token 失守（裸色值 26 处）；font-headline-sm 未定义静默回退
10. 独有资产应保留：Simple/Advanced 双模式侧栏、OPC 运营仪表盘、agent 消息可视化、双语 i18n

## 2026-09 竞品研究要点（docs/competitive-research-2026-09.md，官方信息源）
- **Claude Code Desktop**（Electron，Mac/Win/Linux beta，2026-04-14 重设计）：可拖拽多面板工作区（chat/diff/browser/terminal/editor/plan/tasks/subagent，按 repo 保存布局）、预览浏览器（自动起 dev server + DOM 自检）、集成终端（Ctrl+`）、PR 监控（gh 轮询 CI 自动修复/合并）、checkpoints/rewind、Routines（cron+API endpoint+GitHub 触发）
- **Codex app**（Electron+Rust，2026-02 macOS / 2026-03 Windows，2026-07 并入 ChatGPT 桌面 super app）：按项目分组 threads 并行、每 agent 独立 worktree、Automations→review queue→原 thread 续跑、best-of-N（--attempts 1-4）、in-app browser 划词评论、多终端面板、90+ 插件、多窗口仍非一等公民（#33205）
- **ZCode**（Electron 闭源 v3.11.2，zcode.z.ai，无 CLI）：4 执行模式 Shift+Tab（逐项确认/自动编辑/Plan/Full access）、Goal Mode 核心卖点、Thought Level 三档、Side Conversation /btw、会话 Fork、Idle-time Task、Usage Stats、Wiki、Edit History；**本机已装 /usr/bin/zcode（Electron）可截图**
- **Hermes**（开源 MIT，Electron+Python）：Apple 式精致 UI、成本按类别拆解+缓存命中率状态栏、Comment Mode、Artifacts 画廊、Bot Mode
- 竞品评论区三大痛点全与成本/限额相关 → Shannon BYOK 透明成本是 UI 应放大的故事

## 本机环境
- DISPLAY=:1 (X11, GNOME shell)，可启动 ZCode Electron 截图
- claude CLI 2.1.250、codex-cli 0.118.0、zcode 已装
- Claude Desktop 消费版无 Linux 版本；Codex app 无 Linux 版 → 用官方文档/发布页官方截图替代，文档中注明

## 竞品官方截图源（候选）
- Claude Code Desktop: https://code.claude.com/docs/en/desktop
- Codex app: https://openai.com/index/introducing-the-codex-app/ 、https://developers.openai.com/codex/app
- ZCode: https://zcode.z.ai （本机应用优先）
- Claude 消费版: https://claude.ai/product 或 anthropic.com/claude

## 竞品证据采集结果（2026-09-10 实测）

### 本机真机截图
- **ZCode Desktop v3.11.2**（zcode-desktop-main.png，真实运行窗口）：
  三栏深色布局。左侧窄栏：新任务/搜索/自动化/并行任务/项目分组/会话列表/底部账号+设置；
  中央：会话流（工具调用折叠块）+ composer（输入框 + 权限模式下拉「完全访问」+ 模型 GLM-5.3-Flash + 推理档「最高」+ 发送）；
  右侧：内置浏览器面板（URL 栏 + 视口 1440×900）。自定义标题栏、近黑背景 (#0d0d0d)、细灰分隔线、无重装饰。

### 官方文档全文（code.claude.com/docs/en/desktop.md，100KB 已存 /tmp/cc-desktop.md）
- **Claude Desktop 应用三 tab：Chat（对话）/ Cowork（Dispatch 长任务）/ Code（开发）**；Code tab 内会话= sidebar 并行列表，会话内 pane 自由拼装（chat/diff/browser/terminal/file editor/iOS simulator），diff 评论、PR 监控、side chats、浏览器自检（截图/DOM/点击/表单）、checkpoints、connectors。

### 官方/媒体截图（受网络封锁限制，已采集）
- anthropic.com/claude 产品页 5 张（含 Cowork/使用场景 tabs）
- zcode.z.ai 官网 5 张（定价卡、Goal 面板、模型选择器 GLM-4.2→GLM-5.3、闲时任务）
- Codex app：intuitionlabs hero、kingy guide hero、proflead CLI dashboard png
- 注：openai.com / developers.openai.com / web.archive.org / github.com 均被本机网络封锁（403/000），Codex Desktop 真机截图以官方文字资料 + 媒体描述补充，已在文档中标注证据类型。

## Shannon 逐页截图问题速记（对照 screenshots/shannon/*.png）
| 页 | 截图 | 问题 |
|---|---|---|
| Welcome | 01 | 4 步向导 vs 竞品 2 步；大片留白利用率低；无产品价值展示 |
| Chat | 02 | composer 不可见（须选中会话?）；空会话无引导；视图 tab「聚焦聊天/评审/构建」术语自造 |
| Tasks | 03 | 侧栏「已排程」vs 页头「定时任务」双术语；7 个并排按钮无主次；best-of-N 英文混排；best-of-N 又在 Triage 叫「并行方案」 |
| Triage | 04 | 侧栏「分流队列」vs 页头「收件箱」双术语；操作图标无文字说明 |
| Usage | 05 | 侧栏无入口；错误 toast 中英混排；「总览」选中态蓝色与主题紫不一致 |
| Extensions | 06-11 | 精选页仅 3 卡内容单薄；品牌渐变按钮风格离群 |
| OPC | 12 | 顶栏「单人公司」但侧栏无入口（简单模式隐藏）；状态枚举 raw 英文 IN_PROGRESS/PENDING…；棕色图表配色沉闷 |
| Editor | 13 | 仅一个路径输入框，工具级原型感 |
| Memory | 14 | 结构最完善（统计/筛选/列表图谱）；记忆内容英文 mock 混排 |
| Settings | 15-18 | 审批模式 slider 5 档标签挤成一行不可读；「系统设置」vs 顶栏「设置」 |
| 全局 | — | 扁平白卡+细灰线无层次；紫罗兰单一强调色+杂色（蓝 tab、棕图表、红 badge）；侧栏状态仅有 Triage badge；无玻璃材质/阴影层级/暗色优化证据 |
