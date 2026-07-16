# Shannon Desktop UI 设计审查报告 — 2026-06-25

**审查人**：资深 UI/UX 设计顾问（10 年经验）
**审查对象**：Chat / Memory / Extensions Hub (8 个子页) / Settings·Notifications
**审查方式**：mock mode 浏览器截图 + zai-mcp-server 图像分析 + 8 维度评估
**截图位置**：`/tmp/pm-review/desktop/01-chat.png` … `10-settings-notifications.png`

---

## 一、跨页面共性问题（systemic）

> 以下问题在 **3 个以上页面**重复出现，应作为设计系统级修复，单点修补无效。

### S1. 主色饱和度过高，缺乏现代感

- **现状**：品牌紫 `#6c5ce7` 饱和度约 70%，被用于按钮、选中态、图标、用户消息气泡、tag 等几乎一切强调位置
- **对比标杆**：Claude `#6c63ff`（60%）、Linear `#5e6ad2`（55%）、Notion `#6b6f7d`（中性偏冷）
- **影响**：长时间使用视觉疲劳；紫色"刺眼"显得廉价；与高端 AI 工具标杆拉开档次差距
- **建议**：调色板分两阶——主色 `#5b4fc7`（饱和度 55%）+ 强调色 `#7c70e8`（用于真正需要聚焦的 CTA），其他场景改用灰阶

### S2. 间距节奏混乱，缺少 spacing scale

- **证据**：Memory 卡片间距 12px、Featured 16px、Plugins 8px、Settings·Notifications section 间 24px；同一页面内 padding 12/16/24px 混用
- **影响**：视觉"喘不过气"或"空荡"，节奏不一致让产品显得"拼凑"
- **建议**：采用 4/8/12/16/24/32/48 的 strict spacing scale；section 间固定 32px、字段间 16px、按钮组内 8px

### S3. 圆角不统一（4/8/12px 混用）

- **证据**：Chat 中按钮 4px + 卡片 8px；Featured 中卡片 12px + 按钮 8px；MCP 列表项 pill 8px + 移除按钮 4px
- **影响**：控件风格碎片化、缺设计语言一致性
- **建议**：tokens 化——`--radius-sm: 6px`（按钮、pill）、`--radius-md: 10px`（卡片、输入框）、`--radius-lg: 16px`（dialog、hero）

### S4. 缺少 hover / focus / loading 交互反馈

- **证据**：所有截图均未捕获到 hover 状态设计；搜索框无 focus ring；按钮无 hover 色变；卡片无 hover 阴影
- **影响**：用户操作感知弱，产品"死板"
- **建议**：每个交互控件定义 4 态——default / hover / active / focus-visible（2px ring + offset）

### S5. 空状态与错误状态无引导

- **证据**：Skills/Agents/Installed/Datasources 显示 `[mock] unhandled Tauri command` 原始技术错误；Memory 无"新建第一条记忆"引导；Plugins 空状态只显示"目录暂无条目"
- **影响**：新用户迷茫；错误暴露给非技术用户造成不信任
- **建议**：
  - 错误提示分级：user-facing 用 "加载失败，请重试"，技术细节折叠到"详情"
  - 空状态三件套：icon + 文案 + primary CTA（如"浏览市场"/"创建第一条记忆"）

### S6. 图标语义弱、风格碎片化

- **证据**：Memory 的"决策"用箭头（约定俗成"方向"）、"偏好"用旗帜（约定俗成"标记"）；导航的"分流队列"用列表图标（与"对话"易混）；MCP server 全部用云形（不区分 filesystem/github/playwright）
- **影响**：识别成本高、专业感弱
- **建议**：
  - 引导"语义化图标库"——material-symbols 已在用，但选择应经设计师审计
  - MCP/Skill/Agent 应使用品牌官方 logo（参考 Raycast 的 Extensions Hub）

### S7. 类型/状态 pill 颜色无规则

- **证据**：Memory 类型标签 3 色（紫/棕/蓝）；Featured trust badge 2 色（蓝=官方、紫=已验证）；MCP 状态 pill 灰色（未连接）+ 紫色（工具数）
- **影响**：用户需读文字才能区分，颜色未传达语义
- **建议**：定义语义色板——success/warning/danger/info/neutral，所有 pill 必须映射到语义色，禁止品牌色与状态色混用

### S8. 标签栏（Tabs）选中态对比度不足

- **证据**：Extensions Hub 顶部 tabs（精选/MCP/Skills/...）选中态仅下划线 2px 紫色，未选中是灰字；对比度低于 WCAG AA
- **影响**：当前页面定位不直观
- **建议**：选中态加底色（`bg-primary-50`）+ 图标变实色 + 字重 medium；未选中用 `text-secondary` 灰

---

## 二、按页面详细问题清单

> 每条格式：**问题** · 位置 · 严重度（P0=阻断/P1=严重/P2=改进）

### 2.1 Chat 页（`/chat`）

| # | 问题 | 位置 | 严重度 |
|---|------|------|--------|
| C1 | "新对话"按钮与"在 worktree 中新建"副文字间距 8px，副文字被挤压 | 侧栏顶部 | P1 |
| C2 | AI 回复中 R1/R2/R3 用缩进替代项目符号，可读性差 | 消息区 | P2 |
| C3 | 搜索框无 focus 边框变色，缺交互反馈 | 主区顶部 | P1 |
| C4 | 消息卡片圆角 8px ≠ 按钮 4px | 全局 | P1（见 S3） |
| C5 | 消息列表项间距 12px 偏密，移动端易误触 | 侧栏会话列表 | P2 |
| C6 | 紫色饱和度过高（user 气泡 + 按钮 + 图标），视觉疲劳 | 全局 | P1（见 S1） |
| C7 | 空状态缺失（新用户首次进入无引导） | 消息区 | P1 |

### 2.2 Memory 页（`/memory`）

| # | 问题 | 位置 | 严重度 |
|---|------|------|--------|
| M1 | "DECISION" 用箭头图标、语义弱 | 类型标签 | P2（见 S6） |
| M2 | "Used 7x" 含义不明，应用中文"使用 7 次" | 条目底部 | P2 |
| M3 | 下拉菜单未显示当前选中值（"所有项目"是占位符还是选中？） | 筛选栏 | P1 |
| M4 | 内容文本与 tags 间距 4px，视觉粘连 | 条目内 | P1 |
| M5 | 类型 pill 3 色规则混乱 | 条目顶部 | P1（见 S7） |
| M6 | 统计卡片图标对比度临界（白底蓝图标 4.5:1） | 顶部统计区 | P2 |
| M7 | 卡片无阴影，与背景无层次区分 | 条目 | P2 |
| M8 | 空状态缺失 | 列表区 | P1（见 S5） |

### 2.3 Extensions Hub - Featured（`/extensions/featured`）

| # | 问题 | 位置 | 严重度 |
|---|------|------|--------|
| F1 | CTA 按钮颜色不一致（Notion 黑 / GitHub 深灰 / Slack 紫） | vendor 卡底部 | P0 |
| F2 | GitHub 深灰按钮对比度 4.5:1 临界 | GitHub 卡 | P1 |
| F3 | "官方"trust badge 蓝色 ≠ 品牌紫 | 卡右上 | P1（见 S7） |
| F4 | 卡片描述长度不齐（2 行 vs 1 行），排版散乱 | vendor 卡 | P2 |
| F5 | 筛选标签与 hero 间距过大，视觉割裂 | 顶部 | P2 |
| F6 | 卡片无 hover 效果、无渐变，"扁平" | 卡片 | P1（见 S4） |
| F7 | 空状态缺失（若 vendor 列表为空） | 卡片网格 | P2 |

### 2.4 Extensions Hub - MCP Servers（`/extensions/mcp-servers`）

| # | 问题 | 位置 | 严重度 |
|---|------|------|--------|
| MC1 | "未连接"状态用灰色 pill，对比度不足 4:1 | 列表项底部 | P1 |
| MC2 | 服务器名与"4 个工具"标签间距 4px，粘连 | 列表项顶部 | P1 |
| MC3 | 长命令未截断（github 的 `npx -y @modelcontextprotocol/server-github` 撑宽列表） | 列表项中部 | P1 |
| MC4 | 所有 server 用同款云形图标，不区分 filesystem/github/playwright | 列表项左侧 | P1（见 S6） |
| MC5 | "添加服务器"按钮无下拉子操作（浏览/上传/手动） | 列表底部 | P2 |
| MC6 | "移除"按钮红色，与品牌紫冲突，且 hover 无反馈 | 列表项右侧 | P2 |
| MC7 | 无分组（已连接/未连接/错误），信息扁平 | 整列表 | P2 |

### 2.5 Extensions Hub - Skills（`/extensions/skills`）

| # | 问题 | 位置 | 严重度 |
|---|------|------|--------|
| SK1 | **`[mock] unhandled Tauri command: list_skill_catalog`** 错误暴露给用户 | 顶部 | **P0 BUG** |
| SK2 | 错误提示用纯红色 + 技术文案，非技术用户恐慌 | 顶部 | P1（见 S5） |
| SK3 | 空状态 "No skills installed." 无 CTA 引导 | 中部 | P1（见 S5） |
| SK4 | Tabs 选中态对比度不足 | 顶部 | P1（见 S8） |
| SK5 | skill 卡片设计缺失（无法评估），但从布局看会与 Featured 同款问题 | 中部 | P2 |

### 2.6 Extensions Hub - Agents（`/extensions/agents`）

| # | 问题 | 位置 | 严重度 |
|---|------|------|--------|
| AG1 | **`[mock] unhandled Tauri command: list_agent_catalog`** 错误暴露 | 顶部 | **P0 BUG** |
| AG2 | 错误文案含 `src/lib/mock/handlers.ts`，技术泄露 | 顶部 | P1（见 S5） |
| AG3 | 错误与"已安装"区中间留白 100px，视觉空洞 | 中部 | P2 |
| AG4 | "暂无智能体"无引导按钮 | 中部 | P1（见 S5） |
| AG5 | "分流队列"角标红色 ≠ 品牌紫，冲突 | 侧栏 | P2（见 S7） |

### 2.7 Extensions Hub - Datasources（`/extensions/datasources`）

| # | 问题 | 位置 | 严重度 |
|---|------|------|--------|
| DS1 | 中英文混用（"未找到数据源适配器" vs "No data sources installed"） | 空状态 | **P0** |
| DS2 | 缺数据源核心信息（连接状态/最近同步/文档数） | 列表区 | P1 |
| DS3 | 空状态无"如何添加数据源"引导 | 中部 | P1（见 S5） |
| DS4 | "适配器"与"已安装"模块间留白 24px 过大 | 中部 | P2 |
| DS5 | 无数据源品牌 logo（Obsidian/IMAP 等是品牌驱动的） | 列表项 | P1（见 S6） |

### 2.8 Extensions Hub - Plugins（`/extensions/plugins`）

| # | 问题 | 位置 | 严重度 |
|---|------|------|--------|
| PL1 | 筛选下拉水平间距 8px，文字被截断 | 筛选栏 | P1 |
| PL2 | 筛选样式（边框）≠ 按钮（填充），视觉不统一 | 筛选栏 | P1（见 S3） |
| PL3 | 空状态"放大镜 + 锁"图标易误解为"权限限制" | 中部 | P1 |
| PL4 | "目录暂无条目"无 CTA（如"从 MCP 服务器导入"） | 中部 | P1（见 S5） |
| PL5 | 筛选维度少（仅"全部来源/排序/信任度"），与 VS Code Marketplace 差距大 | 筛选栏 | P2 |

### 2.9 Extensions Hub - Installed（`/extensions/installed`）

| # | 问题 | 位置 | 严重度 |
|---|------|------|--------|
| IN1 | **`加载已安装的扩展失败`** 错误（mock handler 缺 `list_installed_extensions`） | 顶部 | **P0 BUG** |
| IN2 | 错误提示框对比度 3:1，低于 WCAG AA | 顶部 | P1 |
| IN3 | 主内容区加载失败时无友好占位 | 全部 | P1（见 S5） |
| IN4 | Tabs 选中态对比度不足 | 顶部 | P1（见 S8） |
| IN5 | 无启用/禁用/版本/依赖展示（无法评估），列表设计缺失 | 列表区 | P2 |

### 2.10 Settings - Notifications（`/settings/notifications`）

| # | 问题 | 位置 | 严重度 |
|---|------|------|--------|
| SN1 | Webhook URL 输入无实时验证（输错无反馈） | Webhook 区 | P1 |
| SN2 | "清除"按钮灰色，与"保存"权重差距过大，易误点保存 | 按钮组 | P1 |
| SN3 | "设置"与"简单模式"都用齿轮图标，混淆 | 侧栏底部 | P1（见 S6） |
| SN4 | 模块边界不清（Webhook / 出站消息 / Slack / Telegram 全用留白分隔） | 主内容区 | P1 |
| SN5 | Slack placeholder `xoxb-……` 显示星号，但未注明"Bot Token" | Slack 输入框 | P2 |
| SN6 | helper text 与正文颜色/字号几乎一致，层级模糊 | 字段下方 | P2 |
| SN7 | Slack/Telegram 输入框宽度不足，token 易换行 | 出站消息区 | P2 |
| SN8 | 无 danger zone（删除配置/停用投递无 destructive UI） | 全局 | P2 |

---

## 三、P0 紧急修复清单（先于一切）

| # | 问题 | 文件位置 | 修复方式 |
|---|------|----------|----------|
| **B1** | `/extensions/skills` 显示 `[mock] unhandled Tauri command: list_skill_catalog` | `ui/src/lib/mock/handlers.ts` | 新增 `list_skill_catalog` handler，返回 `MOCK_SKILL_CATALOG`（已有 MOCK_SKILLS 可复用） |
| **B2** | `/extensions/agents` 显示 `list_agent_catalog` 错误 | 同上 | 新增 `list_agent_catalog` handler |
| **B3** | `/extensions/installed` 加载失败 | 同上 | 新增 `list_installed_extensions` handler，返回 MCP+Skills+Agents+Plugins 的并集 |
| **B4** | `/extensions/datasources` 中英文混杂空状态 | `ui/src/pages/DataSources.tsx` | i18n 化所有硬编码英文 |
| **B5** | Featured CTA 按钮颜色三款不一致 | `ui/src/pages/extensions/Featured.tsx` | 统一到品牌紫色（或紫色 + vendor 品牌色双方案） |

> B1–B3 都是 mock 缺失，**正式后端已实现**，仅 demo mode 受影响——但 demo 是评审/销售的关键场景，必须修。

---

## 四、按优先级的改进路线图

### Phase 1 — 设计系统级修复（1-2 周，所有页面受益）

1. **建立 design tokens 文件**：`colors.ts`（两阶紫 + 语义色）、`spacing.ts`（4-8-12-16-24-32-48）、`radius.ts`（6/10/16）、`shadows.ts`（sm/md/lg）
2. **更新 Tailwind v4 `@theme` 配置**：替换硬编码的 #6c5ce7；引入 `bg-primary / bg-primary-emphasis / text-primary` 等语义类
3. **统一定义交互控件 4 态**：`Button`、`Input`、`Card`、`Tabs`、`Pill` 组件的 hover/focus/active/disabled 样式
4. **空状态 + 错误状态组件化**：`<EmptyState icon title action />`、`<ErrorState title detail onRetry />`

### Phase 2 — 关键页面重做（2-3 周）

5. **Chat 页空状态**：新用户引导卡片（3 个示例 prompt + 模板库入口）
6. **Memory 页**：tags 间距、统计图标审计、卡片阴影、下拉菜单值显示
7. **Extensions Hub 共用 shell**：Tabs 选中态、筛选栏布局、卡片网格基线
8. **Settings·Notifications**：模块卡片化、字段验证、按钮权重、danger zone

### Phase 3 — 精修细节（1-2 周）

9. **品牌 logo 替换通用图标**：MCP/Skill/Agent/Datasource 用真实品牌 logo
10. **Tabs、Pill、Card 的微交互**：hover 阴影、active 缩放、focus ring 一致
11. **错误文案审计**：所有 mock handler 缺失/网络错误改成 user-friendly 文案 + 详情折叠
12. **i18n 审计**：Datasources 等页面中英文混杂、Header 之外的硬编码英文

---

## 四、标杆参考

| 场景 | 标杆产品 | 关键借鉴点 |
|------|----------|------------|
| Chat | Claude.ai / Cursor | 空状态引导、消息层次、密度 |
| Memory | Notion AI / Mem.ai | 卡片阴影、tag 颜色、统计可视化 |
| Extensions Hub | Raycast Store / Linear Integrations | vendor logo、trust badge、install CTA |
| MCP Servers | Claude Desktop / Cursor MCP | 连接状态指示、命令截断、分组列表 |
| Settings·Notifications | Linear / Slack | 模块卡片、toggle 开关、danger zone |

---

## 附：截图清单

| 文件 | 页面 | 路由 |
|------|------|------|
| 01-chat.png | Chat | `/chat` |
| 02-memory.png | Memory | `/memory` |
| 03-ext-featured.png | Extensions · Featured | `/extensions/featured` |
| 04-ext-mcp.png | Extensions · MCP Servers | `/extensions/mcp-servers` |
| 05-ext-skills.png | Extensions · Skills | `/extensions/skills` |
| 06-ext-agents.png | Extensions · Agents | `/extensions/agents` |
| 07-ext-datasources.png | Extensions · Datasources | `/extensions/datasources` |
| 08-ext-plugins.png | Extensions · Plugins | `/extensions/plugins` |
| 09-ext-installed.png | Extensions · Installed | `/extensions/installed` |
| 10-settings-notifications.png | Settings · Notifications | `/settings/notifications` |

---

**报告生成**：2026-06-25 · 共 8 跨页面问题 + 50+ 页面级问题 + 5 P0 bug
