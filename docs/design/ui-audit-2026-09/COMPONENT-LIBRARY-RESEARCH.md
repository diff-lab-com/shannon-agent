# 组件库调研与选型（UI 改进方案 v2 增补，2026-09-11）

> 本文是 [UI-IMPROVEMENT-PLAN-2026-09.md](./UI-IMPROVEMENT-PLAN-2026-09.md) 的组件库专题增补。
> 截图对比、user journey、玻璃设计系统、术语统一等主体内容见主文档；
> 截至本文撰写，主文档 Wave 1/2 的绝大多数条目已实施并推送（commits b9e3829c…08d952b9）。

---

## 1. 竞品组件库取证

### 1.1 ZCode Desktop v3.11.2（本机取证，可信度 A）

直接解包本机安装的 `/opt/ZCode/resources/app.asar`，`node_modules` 依赖清单显示：

| 依赖 | 版本 | 角色 |
|---|---|---|
| react / react-dom | ^19.2.4 | 视图层 |
| **radix-ui**（全套 `@radix-ui/react-*`，dialog/dropdown-menu/context-menu/select/tooltip/accordion…40+ 包） | — | 无头原语 |
| **class-variance-authority** + clsx + tailwind-merge | — | **shadcn/ui 三件套** |
| cmdk | — | ⌘K 命令面板 |
| lucide-react | — | 图标 |
| framer-motion / motion | — | 动效 |
| @floating-ui/react-dom | — | 浮层定位 |
| ansi-to-react / anser | — | 终端输出渲染 |

**结论：ZCode 是标准 shadcn/ui 技术栈**（Radix 无头原语 + CVA 变体 + Tailwind 原子类，copy-in 组件模式），深色自绘主题。

### 1.2 OpenAI Codex Desktop（社区逆向，可信度 B）

[yuanjiwei.com 架构分析](https://yuanjiwei.com/20250215-architecture-behind-codex/) 与 [LinkedIn 技术帖](https://www.linkedin.com/posts/yangshun_tech-stack-openai-used-to-build-codex-desktop-activity-7424676759347822593-UiFy)：

- Electron 40（主进程 Node.js）+ TypeScript + React Router
- **UI：Radix UI + Tailwind CSS**；renderer 6.5MB JS / 433 个懒加载 chunk
- 与 VS Code 扩展共享 App Server 代码；CLI 仍为 Rust
- 70 方法 IPC API 面

### 1.3 ChatGPT Web（OpenAI 同源体系，可信度 B）

[Reverse Engineering ChatGPT Web](https://performance.dev/chatgpt)：chatgpt.com DOM 中 `data-radix` 属性遍布（菜单/选择器/toast/滚动区/浮层），**Radix UI + Tailwind**（React/Next.js 系）。即 Codex 桌面端与 ChatGPT web 共享同一前端设计体系。

### 1.4 Claude Desktop / Claude Code（可信度 C）

Anthropic 未官方公开内部栈。公开可查：claude.ai 为 React + Tailwind 系；**Claude Code 官方默认生成 shadcn/ui 脚手架**（[shadcn/ui Skills 文档](https://ui.shadcn.com/docs/skills)），社区有「shadcn 复刻 Claude Code UI」的[HN 讨论](https://news.ycombinator.com/item?id=48926085)。生态与 shadcn 模式强绑定。

### 1.5 横向结论

| 产品 | 桌面框架 | 组件体系 | 图标 | 动效 |
|---|---|---|---|---|
| ZCode | Electron | Radix + CVA + Tailwind（shadcn 系） | lucide | framer-motion |
| Codex app | Electron 40 | Radix + Tailwind | — | — |
| ChatGPT web | Next.js | Radix + Tailwind | — | — |
| Claude 系 | Electron | React + Tailwind（shadcn 生态强关联） | — | — |

**行业收敛点：无头原语（Radix）+ Tailwind 原子类 + CVA 变体——没有任何一家用传统"带样式组件库"（AntD/MUI/Mantine/Chakra 均未出现）。** 理由显而易见：AI 桌面产品的界面高度定制（对话流、diff、面板布局、玻璃材质），带样式库的默认外观反而是负担。

---

## 2. Shannon 现状盘点

`desktop/ui/package.json`：

| 依赖 | 角色 | 与竞品对位 |
|---|---|---|
| **@base-ui/react ^1.5.0** | 无头原语 | ≈ Radix 的精神续作（Radix 原班团队在 WorkOS 的重写线，API 范式相同：无头、可组合、可访问性内建） |
| shadcn CLI ^4.19 + CVA + clsx + tailwind-merge | copy-in 组件模式 | **与 ZCode/Codex 完全同构** |
| cmdk / sonner / @tanstack/react-virtual | ⌘K / toast / 虚拟滚动 | 与竞品同选型 |
| @uiw/react-codemirror + @xterm/xterm | 编辑器 / 终端 | 竞品同级（Monaco 更重，Codex 用 xterm 多终端面板） |
| @assistant-ui/react | 对话 UI 原语 | 竞品自研对话流；Shannon 保留此 spike |
| tailwindcss ^4 + tw-animate-css | 样式与动效 | Tailwind v4 与竞品同代 |
| react-intl / react-router 7 | i18n / 路由 | — |

**关键判断：Shannon 的组件底座与三家竞品属同一范式、同一世代，无需迁移。** Base UI 与 Radix 的差异是「同一流派的两个版本」，不是「两种流派」；本轮已落地的玻璃材质、语义 token、门禁脚本都建立在 Tailwind v4 + CVA 之上，与竞品路线兼容且已验证。

---

## 3. 候选库对比与选型结论

对「是否换组件库」的深度比较：

| 候选 | 优势 | 否决/采纳理由 |
|---|---|---|
| **维持 Base UI + shadcn copy-in（现状）** | 与竞品范式同构；玻璃材质/MD3 token/12 主题已验证；无迁移成本 | **✅ 采纳** |
| 迁移到 Radix（对齐 ZCode/Codex 字面一致） | 字面上与竞品相同 | ❌ Radix 维护已放缓、社区重心转向 Base UI；为「名字一致」付整体迁移成本不值 |
| Ant Design | 企业级全家桶、中文生态 | ❌ 与 Tailwind v4 原子类体系冲突；体积大；视觉强锁定，玻璃材质需对抗默认样式；MD3 token 全废 |
| Mantine | 组件全 | ❌ 运行时 CSS 变量体系与 Tailwind v4 双轨；同样视觉锁定 |
| HeroUI（原 NextUI，React Aria 底座） | 现代观感、Tailwind 友好 | ❌ 可访问性底座（React Aria）优于 Base UI，但迁移收益 < 成本；玻璃材质仍需自建 |
| MUI | 生态最大 | ❌ Material 历史包袱与 MD3 token 重复；JSS/emotion 与 Tailwind 冲突 |

**选型结论：维持 Base UI + shadcn copy-in + Tailwind v4 + CVA + 自有 Liquid Glass token 层。** 组件库负责行为与可访问性，玻璃质感由 token 层负责——竞品用 Radix 也一样要在 token 层自建质感，这条路没有捷径也不需要第三方玻璃库（不存在成熟的 glassmorphism 组件库；有也只是玩具）。

与竞品的**真正差距不在组件库，而在**：(a) primitive 覆盖度（数据表/表单/日期选择等尚未 copy-in）；(b) 动效体系（ZCode 有 framer-motion，Shannon 用 tw-animate-css + CSS spring token，够用但少编排能力）；(c) connector/插件目录规模。这三点进入实施规划。

---

## 4. 实施规划 v2（组件库维度）

> Phase A（术语统一、composer 恒见、深色优先、两步引导、目标入口、用量面板等）**已完成并推送**，见主文档实施状态表。

**Phase B — 组件库深化（1–2 周）**
1. shadcn CLI 按需补齐缺 primitive：DataTable（TanStack Table）、Form（react-hook-form + zod，Base UI 表单原语）、DatePicker、Combobox——先盘点 Settings/连接目录/任务创建三处高价值表单场景。
2. Base UI 升级到最新 minor（关注 Dialog/Menu 的 focus 管理 API 变更）。
3. 完成剩余手搓 modal → Modal primitive 收敛（本轮已收敛 Dialog 遮罩与两处主按钮，存量约 20 处）。
4. Button 迁移清尾（7 种手搓配方，本轮已清 2 处代表案例）。

**Phase C — 视觉与动效对齐（2–4 周）**
5. 玻璃材质逐页审查（contrast-audit CI 已保证可读性下限）。
6. 动效决策：评估 `motion`（framer-motion 的新包名，ZCode 同款）引入成本 vs 纯 CSS spring token——建议仅对「面板进出场/布局动画」引 motion，微交互保持 CSS。
7. 12 主题 × 玻璃参数微调（`--material-*` 每主题 base 色覆盖）。

**Phase D — 能力补齐（与后端协同）**
8. `thought_level` 引擎字段（ZCode 推理档对位；需 ProviderModelConfig 扩展，见审计 §9 结论）。
9. G5 消息渠道入站（Telegram/Discord/Slack/飞书 → NewGoalDialog 派活）。
10. 连接目录从内置注册表迁移到远端策展（对抗 MCP 生态增速）。

**Phase B/C/D 实施记录（2026-09-11，当日完成）**：
- B：Base UI 1.5→1.8（滚动锁行为级断言更新）；4 个新 primitive（DataTable/Form/DatePicker/ComboboxSelect）+ Usage 会话表迁移；overlay 白名单清零
- C：Header/Sidebar 玻璃统一到 --glass-blur-surface token；面板入场 CSS spring 动画（animate-panel-in，reduced-motion 降级）；玻璃 tint 改 --glass-tint-alpha 模式级变量（决策：不引 motion 运行时，CSS 足够且零 bundle 成本）
- D：推理档落地（引擎已有 effort_level/CLI /effort，composer 新增「推理力度」Select 直写 config）；D9 入站触发端点已在 loopback_api.rs（HMAC POST /api/routines/:id/trigger），NotificationsSettings 补端点展示；D10 远端策展已有（McpRegistryClient 24h 缓存 + SearchTab）
- turn-timeline e2e 为既有 flaky（force-click 菜单竞态，retry 即过），非本轮回归

**验收基线（已就位）**：`check:tokens` 术语/裸色值门禁、contrast-audit AA、Playwright 视觉快照（chat/tasks/settings-models）、e2e 63 例、vitest 1663 例。

---

## 5. 参考来源

- ZCode asar 解包（本机 v3.11.2，第一手证据）
- [The Architecture Behind OpenAI's Codex Desktop App](https://yuanjiwei.com/20250215-architecture-behind-codex/) · [Codex desktop 技术栈（LinkedIn/Yangshun Tay）](https://www.linkedin.com/posts/yangshun_tech-stack-openai-used-to-build-codex-desktop-activity-7424676759347822593-UiFy) · [codenote.net Electron 名应用研究](https://codenote.net/en/posts/famous-electron-apps-2026-research/)
- [Reverse Engineering ChatGPT Web](https://performance.dev/chatgpt)（data-radix 实证）
- [shadcn/ui Skills（Claude Code 官方集成）](https://ui.shadcn.com/docs/skills) · [shadcn vs Radix（Vercel）](https://vercel.com/i/shadcn-vs-radix) · [HN：shadcn 复刻 Claude Code/Codex UI](https://news.ycombinator.com/item?id=48926085) · [2026 React UI 默认栈](https://www.shadcndeck.com/blog/rise-of-shadcn-ui-2026)
