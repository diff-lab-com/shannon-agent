# 竞品画像:Z.ai ZCode(桌面端)

> 调研于 2026-10-07,由联网调研代理核实,每条事实附来源 URL;[inferred]/[unverified] 为未能直接核实的项。
> 本文是 [R3 主报告](2026-10-07-desktop-ui-competitive-review-r3.md) 的附件。

## 0. 候选判定

"Zcode" 搜索结果高度一致地指向 **Z.ai(智谱 Zhipu AI)的 ZCode** —— AI 编码 "Agentic Development Environment (ADE)" 桌面应用:官方站点 zcode.z.ai、GitHub `zai-org/ZCode` 官方组织、多家第三方评测均指向同一产品([zcode.z.ai](https://zcode.z.ai/en)、[GitHub](https://github.com/zai-org/ZCode)、[eesel 评测](https://www.eesel.ai/blog/zcode-review))。

## 1. 产品基本面

- **厂商**:Z.ai(Zhipu AI 智谱),GLM 系列模型厂商([flaviocopes](https://flaviocopes.com/zcode)、[GitHub](https://github.com/zai-org/ZCode))。
- **平台**:macOS(Apple Silicon/Intel)、Windows(x64/ARM64)、Linux(x64/ARM64,AppImage/DEB/RPM,标 beta)([install docs](https://zcode.z.ai/en/docs/install))。
- **时间线**:2026-07 初随 GLM-5.2 发布([windowsforum](https://windowsforum.com/news/z-ai-zcode-launch-free-agentic-ai-coding-desktop-on-windows-macos-linux.433572/));GLM-5.3 于 8 月 14 日发布后转向 "ZCode for GLM-5.3";**2026-09-20 以 Apache 2.0 开源**([cyber-ivy](https://cyber-ivy.com/en/articles/zcode-open-source-coding-agent-tool-check-2026)、[Reddit](https://www.reddit.com/r/LocalLLaMA/comments/1wlzvcx/zcode_is_now_open_source));当前 v3.14.x。
- **形态**:Electron 桌面应用 + 浏览器界面(`zcode --web`)+ 终端 TUI,同一 `zcode` 命令分流;**非 VS Code fork,无传统代码编辑器**;开源 monorepo 含 React+Zustand 共享 UI。([GitHub](https://github.com/zai-org/ZCode)、[flaviocopes](https://flaviocopes.com/zcode))
- **定价**:应用免费;模型走 GLM Coding Plan(Lite $18 / Pro $80 / Max $168,常打折;credit 周额度制)或自带 API Key;新用户 5 天试用;低谷时段半价;应用内可购套餐。([welcome docs](https://zcode.z.ai/en/docs/welcome)、[configuration](https://zcode.z.ai/en/docs/configuration))

## 2. 信息架构

左侧边栏 = 任务列表(三视图:**Grouped 分组 / Workspace 按项目 / Timeline 时间线**)+ 搜索(Quick Actions 命令面板)+ automations + Plugin Marketplace;中央 = 单一 prompt box(对话即工作区);右侧面板可切换 = 终端 / 内置浏览器画面 / Side Conversation / Goal 摘要面板;顶部 workspace 卡片(分支切换、file tree、Wiki 入口)。([task-management](https://zcode.z.ai/en/docs/task-management)、[flaviocopes](https://flaviocopes.com/zcode))

## 3. 关键界面深挖

- **Onboarding/登录**:首启 setup 页 → **Connect Z.ai(国际)/ Connect BigModel(中国大陆)/ Use API Key** 三条路;**Data Migration Wizard 仅支持导入 Claude Code 与旧版 ZCode 会话**;随后选工作目录、发测试指令;OAuth 经浏览器 + `zcode://` deep link 回跳(Linux 常见回跳失败)。([install](https://zcode.z.ai/en/docs/install)、[qa](https://zcode.z.ai/en/docs/qa))
- **任务/会话管理**:分组可命名 + **7 色**(灰红橙黄绿蓝紫)、拖拽排序;归档保留期 3/7/14/30 天;可 **Work outside a project** 无项目闲聊。([task-management](https://zcode.z.ai/en/docs/task-management))
- **Diff/改动审查**:file tree 一键 **Show changed files only** + Git 状态标记;**每条回复下有文件变更摘要 + Undo/Reapply 按钮**(全有或全无,终端命令改动不可 undo);**Git Graph** 只读提交图;无传统行内 diff 编辑器 [inferred,来自"无代码编辑器"定位]。([task-management](https://zcode.z.ai/en/docs/task-management)、[edit-history](https://zcode.z.ai/en/docs/edit-history))
- **权限/审批**:**4 执行模式 Ask before changes(默认)/ Edit automatically / Plan / Full access,输入框聚焦按 Shift+Tab 循环**;审批请求**暂停任务并锁住 composer**;决策四档 **Allow / Always Allow / Reject / Always Reject**;普通提问默认 **5 分钟倒计时**超时自动继续(标 "Unanswered, auto-continued"),权限与 Plan 审批永远等待。([safety-confirm](https://zcode.z.ai/en/docs/safety-confirm))
- **设置**:General(代理/终端 shell/Memory/关窗最小化到托盘/自动继续问题)、Model providers(每模型上下文窗口可改)、Skills、Browser;版本号只在 About 对话框。

## 4. 组件清单

- **Composer**:左下 "+" = 附件 / @ 文件与文件夹 / `#` 引用历史会话 / `/` 命令 / `$` 技能;超长粘贴自动转附件;划选回复文本弹小工具栏(Add to current task / Ask in side conversation)。([agents docs](https://zcode.z.ai/en/docs/agents)、[bitdoze 评测](https://www.bitdoze.com/zcode-ai-review))
- **选择器**:模型(Ctrl+M)、Thought Level 三档 Low/High/Max 默认 Max(Ctrl+T)、执行模式(Shift+Tab)、Git 分支。([keyboard-shortcuts](https://zcode.z.ai/en/docs/keyboard-shortcuts))
- **Agent 进度**:Goal 模式右侧摘要面板逐轮卡片 + 按迭代分组的 checklist + 耗时;消息级 Edit/Fork/Reset chat+files。([goal](https://zcode.z.ai/en/docs/goal))

## 5. User Stories(依据已核实功能推导 [inferred])

1. 扫码用手机接管桌面正在跑的 agent 并批准请求(Remote Control);2. 在微信/飞书/Telegram @bot 查进度、切模型、换执行模式(Bot Channel);3. 用 `/goal` 下达"修完所有 TS 报错"后离开,系统逐轮自验证至完成;4. 把本周改动文件一键喂给 agent 生成 commit;5. 误发指令后原地编辑最后一条消息并连带回滚文件重发;6. 从某条高质量回复 fork 出平行方案;7. 主任务卡在审批时,在右侧 Side Conversation 问旁支问题;8. 用 `/goal` 设"Lighthouse>90"让 agent 自驾浏览器验证;9. 把晨报/巡检设为定时 Automation,低谷时段半价跑;10. 把 Claude Code 历史会话一键迁移进 ZCode;11. 用 7 色分组按项目/优先级整理几十个并行任务;12. 新接手陌生仓库时生成 Repo Wiki(每条论断挂源码链接);13. 按风险分级的 4 档放权;14. 一键 Undo agent 本轮文件改动。

## 6. 视觉设计语言

深色工作台;"agent 对话为中心",文件管理器/终端/Git 面板/浏览器预览环绕;Quick Actions 内置切换主题。([developersdigest](https://www.developersdigest.tech/blog/zcode-developer-guide-2026))**社区批评其界面是 Codex 桌面版近复刻**:"侧边栏风格 1:1 identical to Codex"。([eesel](https://www.eesel.ai/blog/zcode-review)、[HN](https://news.ycombinator.com/item?id=48753715))强调色/字体/密度无可靠来源 [unverified]。

## 7. 独有交互

- **Goal Mode**(/goal 自动迭代 + 证据化验证:只认文件改动/命令输出,不认"听起来完成")、**Idle-time Tasks**(空闲算力跑任务不耗套餐)、**Bot Channel**(微信/飞书/Telegram)、**手机扫码镜像桌面**。([goal](https://zcode.z.ai/en/docs/goal)、[welcome](https://zcode.z.ai/en/docs/welcome))
- **智谱生态绑定**:Z.ai/BigModel 双端点(国际/中国大陆)、Coding Plan 端点自动路由、应用内购套餐、5 分钟提问倒计时自动续跑。
- 项目指令用 **AGENTS.md**(CLAUDE.md 仅 onboarding 一次性迁移);Project Memory 默认关、不可浏览/清理。([agents](https://zcode.z.ai/en/docs/agents))

## 8. 已知弱点/批评

- 界面被指 Codex 克隆(见上)。
- 可靠性:TUI 频繁崩溃、API 不稳需重试、Max 档 token 消耗被称 5x([eesel](https://www.eesel.ai/blog/zcode-review));WSL2 下生成 300GB tmp 文件([Reddit](https://www.reddit.com/r/ZaiGLM/new))。
- 定价档位是"未公开基数的倍数",被诟病不透明([eesel](https://www.eesel.ai/blog/zcode-review))。
- 数据隐私:发往中国公司基础设施的顾虑([flaviocopes](https://flaviocopes.com/zcode)、[HN](https://news.ycombinator.com/item?id=48753715))。
- 官方 feedback 库活跃 bug:plan 卡片连不上、agent 扫描 node_modules 误报、Goal 验证器把 invalid JSON 判成功、**跨设备不同步会话历史**(#930/#919/#918/#917/#932,[[feedback issues](https://github.com/zai-org/feedback/issues)])。
- 上下文压缩无用户可调阈值;7 月评测指无跨会话记忆(现已有 Project Memory,默认关)。
