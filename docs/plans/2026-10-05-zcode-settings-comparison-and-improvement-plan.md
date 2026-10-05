# ZCode 配置页 vs Shannon 设置页 — 对比分析与改进方案

- 日期: 2026-10-05
- 状态: 待审查(决策点见 §8,拍板前不动代码)
- 输入: ZCode 桌面版配置页全文(用户提供)+ shannon-mono 代码调研(前端 `desktop/ui/`,后端 `desktop/src/`、`crates/`)
- 结论速览: **IA 与写入健壮性 Shannon 不落后,但存在 4 类实质性缺口** —— ① 系统集成项(代理/证书/更新/休眠/硬件加速)基本缺失;② 4 个常用设置(含"检查更新")被 dev-gate 藏进高级区,普通用户不可达;③ 会话行为(思考过程/分组/自动归档/提问超时/压缩开关)无用户可控项;④ 通知缺"需确认"事件与声音控制。

---

## 1. ZCode 配置页解析

### 1.1 信息架构

单一「常规」长页 + 「交互行为」分组(用户提供的文本仅见这两个分组),每项统一为「标题 + 1~2 行说明 + 控件」三段式。值得学习的**页面级设计模式**:

| 模式 | ZCode 的做法 | 例子 |
|---|---|---|
| 生效语义逐项内联标注 | 「修改后需重启应用生效」「对新会话生效,当前会话保持现有设置」 | 代理、证书、硬件加速、增强 Find/Grep |
| 跨项依赖显式声明 | 「仅在办公模式下支持」「通知开启后,可单独关闭提示音」 | 主动任务推荐、通知声音 |
| 作用域声明 | 说明流量覆盖范围(模型/MCP/命令工具/渲染层)、是否读系统环境变量 | HTTP 代理 |
| 安全护栏 | 下载完成后如有任务运行,重启更新前仍要求确认 | 自动更新 |
| 数据安全 | 改存储路径自动复制现有数据,路径后缀锁定不可改 | 数据存储路径 |
| 默认值策略 | 大量「留空 = 自动/跟随系统」 | 终端字体(探测系统终端配置)、语言(系统默认) |

### 1.2 设置项清单(27 项)

**常规区:**
① 界面语言(下拉,默认「系统默认」) ② 界面模式(办公=操作摘要与结果 / 编程=命令、输出、代码变更详情) ③ 主动任务推荐(开关,仅办公模式) ④ 继承系统终端 Profile(登录 shell 环境、代理、Kube 变量、本机终端字体) ⑤ 终端字体(留空自动探测) ⑥ 增强 Find/Grep(新会话生效) ⑦ HTTP 代理(模型/MCP/命令工具/渲染层出口流量;不读系统 env;留空直连) ⑧ 不使用代理的地址(逗号分隔规则) ⑨ 自定义证书(PEM 路径 → NODE_EXTRA_CA_CERTS 注入模型/MCP/命令工具 + 渲染层校验) ⑩ Chrome 硬件加速(关闭规避白屏/闪退) ⑪ 接受预览版更新(beta 频道) ⑫ 自动下载并安装更新(任务运行时重启前需确认) ⑬ 任务通知(完成/失败/**需确认**) ⑭ 通知声音(独立开关) ⑮ 保持电脑运行(阻止空闲休眠,全局)

**交互行为区:**
⑯ 队列(运行中加入队列 / 引导至下一轮工具调用后运行) ⑰ 提问自动继续(提问 5 分钟未回答自动继续) ⑱ 完整保留模型 I/O(不自动压缩、限制大小或删除旧记录) ⑲ 显示思考过程(关闭时每轮仍展示第一次思考) ⑳ 显示待办(Todo 卡片) ㉑ 分组探索工具(连续读取/搜索聚合) ㉒ 分组终端命令(连续非只读 Shell 聚合) ㉓ 分组文件更改(连续 Write/Edit/ApplyPatch 聚合) ㉔ 自动归档旧任务(已完成、无未读、未置顶且超保留期) ㉕ 归档保留时长(默认 7 天) ㉖ 数据存储路径(自动迁移,后缀锁定) ㉗ 引导(重选职业/界面模式/偏好,数据迁移另行入口)

---

## 2. Shannon 设置现状(调研摘要)

- 入口 `desktop/ui/src/pages/Settings.tsx`:8 分区左侧栏(General / Theme / Models / Permissions / Advanced / Notifications / Connections / Remotes),Advanced 仅 `mode === 'dev'` 时出现在导航(`Settings.tsx:32`),**但路由无守卫,直接输 URL 可达**。
- **Terminal / VoiceStt / VoiceLocal 挂在 dev-gated 的 Advanced 内**(`AdvancedSettings.tsx:14-16,543-549`);「检查更新」也在其中(`AdvancedSettings.tsx:588-644`)。
- 持久化分四层:localStorage(locale/theme/density/链接目标/artifact/远程图片)、desktop config(`~/.shannon/desktop/config.json`,configure() 约 25 个 key)、`~/.shannon/config.toml`(terminal 表、webhook)、gateway config(IM 平台)。
- 写入健壮性好:失败回读不让 UI 说谎(P1-10)、滑杆去重提交(P1-11)、模态脏状态确认、webhook 测试门禁(R2-P1-7)。
- i18n:react-intl,10 语言,localStorage 持久化 + 首次浏览器语言探测,**无「跟随系统」显式选项**(`i18n/index.tsx:160-176`)。
- 设置页无搜索。窄屏仅重排不裁剪。

---

## 3. 逐项对比总表

判定:✅ 有且可用 / 🟡 部分(缺开关或缺场景) / ❌ 无。「优先级」为 §7 建议方案的优先级。

| # | ZCode 设置项 | Shannon 现状 | 关联功能/UI(Shannon 侧) | 缺口 | 优先级 |
|---|---|---|---|---|---|
| ① | 界面语言(系统默认) | ✅ 10 语言按钮组,首启自动探测 | `GeneralSettings.tsx:193-218` | 无「跟随系统」显式选项;控件占地大 | P3 |
| ② | 界面模式(办公/编程) | 🟡 Simple/Advanced 侧栏模式仅影响导航可见性 + density 字号间距,**不改变消息流详略** | `Sidebar.tsx:40-53`、`lib/density.ts` | 办公=摘要、编程=详情的语义缺失 | P3(实验) |
| ③ | 主动任务推荐 | ❌(最近的是技能候选提醒、Dream 夜间蒸馏,语义不同) | — | 无 | P3 backlog |
| ④ | 继承系统终端 Profile | ❌ 非 login shell,无代理/Kube env 注入;继承的是 GUI 会话 env | `terminal_commands.rs:215-230,822-827` | 无 | P2 |
| ⑤ | 终端字体 | 🟡 仅字号 8–32;**字体族硬编码** | `TerminalPanel.tsx:99`、`TerminalSettings.tsx` | 字体族不可配、无自动探测 | P2 |
| ⑥ | 增强 Find/Grep | ✅ 自研 ripgrep-like 工具,常开 | `crates/shannon-tools/src/grep.rs` | 无实质缺口(无需开关) | — |
| ⑦ | HTTP 代理 | ❌ 全仓无 reqwest::Proxy;仅隐式读 env(GUI 启动时通常拿不到) | `shannon-engine/src/api/client.rs:136-145` | 企业内网/自托管网关场景不可用 | **P0** |
| ⑧ | 不使用代理的地址 | ❌ | 同上 | 无 | P0(随⑦) |
| ⑨ | 自定义 CA 证书 | ❌ 无 NODE_EXTRA_CA_CERTS / add_root_certificate | rustls-tls(`desktop/Cargo.toml:79`) | TLS 拦截型企业网络不可用 | **P0**(随⑦) |
| ⑩ | Chrome 硬件加速 | ❌ | tauri.conf.json 无 GPU 项 | Linux WebKitGTK 白屏无自救手段 | P2 |
| ⑪ | 预览版更新频道 | ❌ updater 插件已移除(B1-15,签名 key 未就绪) | `main.rs:893-897` | 无频道、无 in-place 更新 | P2(依赖基建) |
| ⑫ | 自动下载并安装更新 | ❌ 仅手动检查 + 跳转发布页 | `commands_surface.rs:206-345` | 无自动下载;**且检查入口被 dev-gate 藏匿** | **P0**(入口)/ P2(基建) |
| ⑬ | 任务通知(完成/失败/需确认) | 🟡 完成✅ 失败✅ 预算告警✅;**「需确认/审批」不发 OS 通知**(只进 inbox) | `notifications.rs`、`commands_notifications.rs:304-326` | 无人值守时 agent 卡在审批,用户无感知 | P1 |
| ⑭ | 通知声音 | ❌ 无声音、无开关(需平台能力 spike) | `notifications.rs` | 无 | P2 |
| ⑮ | 保持电脑运行 | 🟡 macOS 自动 caffeinate(任务运行期),**非设置项**;Win/Linux no-op | `shannon-core/src/prevent_sleep.rs` | 无开关、跨平台缺失 | P2 |
| ⑯ | 队列行为 | ✅ 队列 chips + steer 插话都已实现(常开,内存态) | `Chat.tsx:548-559,642-669`、`useSteerSend.ts` | 两种行为无用户可选项 | P3 |
| ⑰ | 提问自动继续 | ❌ `ask_user_question` 无超时/自动继续(终端 stdin handler) | `shannon-tools/src/ask_user.rs:303` | 无人值守跑批会永久挂起 | P1 |
| ⑱ | 完整保留模型 I/O | ❌ 反向:自动压缩常开不可关(60%/80% 预警、micro-compaction、0.75 阈值);仅 env 可调工具输出 cap | `agent_loop.rs:1279-1452`、`compact_messages.rs:99-115` | 调试/审计场景无法关闭压缩 | P1 |
| ⑲ | 显示思考过程 | 🟡 仅流式期间折叠可展开;**历史消息完全不渲染 thinking**;无开关 | `StreamingResponse.tsx:73-76`、`MessageBubble.tsx`(不渲染) | 无 | P1 |
| ⑳ | 显示待办 | ❌ 无开关;TodoWrite 渲染为通用工具卡(无 todo 专属卡) | `MessageBubble.tsx:486-496` | 无 | P2 |
| ㉑ | 分组探索工具 | ❌ 每个工具调用独立卡片 | `MessageBubble.tsx:461-534` | 长会话消息流噪音大 | P1 |
| ㉒ | 分组终端命令 | ❌ 同上(仅失败链横幅/agent_spawn 块/FileChangesCard 三个相邻聚合) | 同上 | 同上 | P1(随㉑) |
| ㉓ | 分组文件更改 | 🟡 每消息 FileChangesCard(Review/Undo)已有,但无跨调用聚合、无开关 | `MessageBubble.tsx:463-478` | 部分 | P1(随㉑) |
| ㉔ | 自动归档旧任务 | 🟡 仅「已归档会话」的 GC(opt-in,**dev-gated**);无「自动转为归档」的扫描 | `commands_sessions.rs:552-666` | 无 | P1 |
| ㉕ | 归档保留时长 | 🟡 GC 保留期 永不/30/90 天(dev-gated) | `AdvancedSettings.tsx:370-408` | 同上 | P1(随㉔) |
| ㉖ | 数据存储路径 | ❌ 仅 `$SHANNON_HOME` env;UI 不可改、无迁移;有降级告警栅栏 | `data_meta.rs:74-81`、`main.rs:117-128` | 无 | P2(先只读展示) |
| ㉗ | 引导(重跑) | ✅ Welcome 重跑 + 迁移向导(支持从 **Claude Code 和 ZCode** 导入 mcp/skill/command/memory/settings 五类,比 ZCode 反向迁移更全) | `Welcome.tsx`、`MigrationWizard.tsx`、`migration_commands.rs:43-77` | 无实质缺口 | — |

---

## 4. UI / UX 模式对比

| 维度 | ZCode | Shannon | 评价 |
|---|---|---|---|
| 信息架构 | 单页长滚动 + 2 分组 | 8 分区左栏 | Shannon 可扩展性更好;**但把终端/语音/CLI/更新藏进 dev-gate 是反模式**——普通用户永远找不到「检查更新」 |
| 项级说明文案 | 每项 1–2 行 | 每项有 help 文案 | 平手 |
| 生效语义标注 | 逐项内联统一措辞(重启/新会话/新终端/作用域) | 有标注但不统一:沙箱=文案、网关=横幅+一键重启、终端=文案、其余即时 | Shannon 需统一为 badge 组件 |
| 跨项联动 | 显式声明(仅办公模式/依赖通知) | master 开关禁用子项、webhook dirty 门禁 | 平手 |
| 危险操作 | 数据迁移自动复制+后缀锁定 | factory reset/删除/脏状态均有确认 | Shannon 更细 |
| 控件选型 | 高频下拉(语言/保留时长),省空间 | 偏好按钮组(语言 10 按钮并排) | 大选项集建议改下拉 |
| 默认值策略 | 大量「留空=自动/跟随系统」 | 终端 shell 留空=系统默认(有),语言无「跟随系统」 | 补齐 |
| 设置搜索 | 提供的文本中未见 | 无 | 共同空白,列为自主改进项 |

---

## 5. 发现的问题(按严重度)

- **G1 (P0)** 普通用户无法更新应用:「检查更新」在 dev-gated Advanced 内(`Settings.tsx:32` 过滤),非 dev 模式不可见;且无 in-place 更新,只能跳 GitHub 页。
- **G2 (P0)** 企业网络不可用:无代理/no_proxy/自定义 CA。macOS 从 Finder 启动时 GUI 会话无 shell env,reqwest 隐式读 env 的兜底也失效——模型 API 直连失败且无任何自救配置。
- **G3 (P1)** 无人值守断点:审批请求不发 OS 通知(只进 inbox)、`ask_user_question` 无超时、自动压缩不可关。三者叠加:跑批任务会静默挂死在提问/审批上。
- **G4 (P1)** 消息流可读性:思考过程历史不可见、工具调用逐卡不分组、Todo 无专属卡。长会话(尤其编码模式大量工具调用)信息噪音大——这正是 ZCode 三个分组开关 + 待办卡要解决的问题。
- **G5 (P1)** 会话生命周期管理薄弱:无自动归档(仅已归档 GC),且该功能 dev-gated;普通用户的会话列表会无限增长。
- **G6 (P2)** 桌面系统集成缺口:终端环境继承/字体、keep-awake(仅 macOS 且不可关/开)、硬件加速开关、更新频道、数据路径 UI。
- **G7 (P2)** 通知无声音控制(需先 spike tauri-plugin-notification 各平台声音能力)。
- **G8 (P3)** 语言无「跟随系统」;语言控件占地大;设置页无搜索。
- **G9 (P3)** Advanced 路由无守卫(导航隐藏但 URL 直达),dev-gate 契约不完整。
- **G10 (P3)** 界面模式语义缺失:simple/dev 只影响导航与密度,不影响消息详略;队列/steer 行为无用户可选入口。

---

## 6. Shannon 已有、ZCode 该页未见的能力(保持优势,勿在改进中回退)

权限档 + 自定义规则 + 沙箱三档 UI、webhook 多通道通知(8 预设 + HMAC + SSRF 防护)、IM 平台连接(8 平台 + keyring + 配对质询)、Remotes(SSH/Docker)、迁移向导(从 ZCode/Claude Code 五类导入)、13 主题 + 字号 + 密度、人格包一键导出/导入、预算告警通知、反馈聚合、诊断导出。

---

## 7. 改进方案(四批,供审查)

工作量:S ≤2 天 / M 2–5 天 / L >1 周。每批可独立成 PR 序列;批内按序号排列即实施顺序。

### 批次 A — 设置页 IA 与入口(合计约 1 周,全是小改动)

| # | 内容 | 涉及 | 量 |
|---|---|---|---|
| A1 | 「关于与更新」提为独立分区(或 General 顶部卡):版本、检查更新、发布页链接、更新日志;**移出 dev-gate**(G1) | `Settings.tsx`、`AdvancedSettings.tsx:588-644` 拆出 | S |
| A2 | Advanced 路由守卫:非 dev 访问 `/settings/advanced` → redirect general + toast(G9) | `App.tsx` 路由层 | S |
| A3 | 语言改「下拉 + 系统默认」:新增 follow-system 选项(持久化 null = 跟随 OS,首启现状即其子集)(G8) | `GeneralSettings.tsx:193-218`、`i18n/index.tsx` | S |
| A4 | 生效语义统一 badge:`即时生效 / 新会话生效 / 需重启应用 / 需重启网关` 四种 tag 组件,替换现有散落文案;terminal/网关/sandbox 及后续 B/C 批新增项一律使用 | 新组件 + 各分区替换 | S |
| A5 | 设置搜索(自主改进,非 ZCode 对齐):i18n 文案建索引,Cmd+K/顶栏入口,命中跳转对应分区并高亮;可后置到批次 C 后 | `Settings.tsx` | M |

### 批次 B — 网络与企业可用性(合计 2–3 周)

| # | 内容 | 涉及 | 量 |
|---|---|---|---|
| B1 | **网络设置区**(新分区,放 General 之后):HTTP 代理 URL、不使用代理的地址、自定义 PEM 证书路径;语义对齐 ZCode——显式配置优先、**留空才回退 env 直连**;注入点:① engine `build_client()` 加 `reqwest::Proxy` + rustls `add_root_certificate`(`shannon-engine/src/api/client.rs:136-145`)② MCP server env ③ gateway sidecar env(`gateway_supervisor.rs:129-135`)④ 命令工具 env(NODE_EXTRA_CA_CERTS/SSL_CERT_FILE)。标注「需重启应用生效」。渲染层代理暂跟随系统(与 ZCode 的差异在文档注明,WebView 代理 Tauri 支持有限)(G2) | 新 `settings/NetworkSettings.tsx` + desktop config keys + engine/supervisor | L |
| B2 | keep-awake:改用 `keepawake-rs` 跨平台;**默认语义 = 任务运行时阻止休眠**(即现有 macOS 行为全平台化),另提供「保持电脑运行」常开开关(对齐 ZCode)。见决策点 ② | `prevent_sleep.rs`、Notifications 或 General 区新开关 | M |
| B3 | 硬件加速开关(关闭规避白屏):Linux `WEBKIT_DISABLE_COMPOSITING_MODE=1`、Windows WebView2 additional browser args `--disable-gpu`、macOS 隐藏该项;「需重启应用生效」(G6) | `main.rs` 启动参数、desktop config | M |
| B4 | 通知补强:① 审批/提问等待时发 OS 通知(新增 `needs_attention` 事件类型,复用 Notifier;点击通知聚焦窗口)② 通知声音开关(先 spike 各平台 `sound` 能力,不支持的平台隐藏该项)(G3/G7) | `commands_notifications.rs`、`NotificationsSettings.tsx` | M |

### 批次 C — 会话与消息流行为(合计 2–3 周)

| # | 内容 | 涉及 | 量 |
|---|---|---|---|
| C1 | 新「会话」分区(从 dev-gate 收编 GC + 新增项,普通用户可见):自动压缩开关(`完整保留模型 I/O` 反向开关,写 `CompactionConfig.enabled`,保留 /compact 手动)、会话历史/归档组 | 新 `settings/SessionSettings.tsx` + engine config 接线 | M |
| C2 | 显示思考过程:三档(全部显示(默认,折叠)/ 仅每轮第一次 / 关闭);历史消息渲染 `ChatMessage.thinking`(`MessageBubble.tsx` 补 Reasoning 块);开关持久化 desktop config | `MessageBubble.tsx`、engine 事件已带 thinking | M |
| C3 | 提问自动继续:`ask_user_question` 桌面 handler 加 5 分钟超时 + 自动继续(注入「用户未回答,按你的最佳判断继续」提示),开关默认关;超时前 1 分钟在消息流内倒计时提醒 | `ask_user.rs` handler 注入、desktop loopback、设置开关 | M |
| C4 | 发送行为设置:运行中发送 = `插话打断(steer)` / `加入队列(queue)` 两档,把现有两条路径暴露为配置,默认维持现状 | `Chat.tsx`、`useSteerSend.ts`、composer | S |
| C5 | 自动归档:定时(复用 GC 调度)扫描最近打开工作区,将「已完成 + 无未读 + 未置顶 + 最后更新早于保留期」的会话自动置为归档(区别于现有 GC 的删除);保留时长下拉 默认 7 天(1/7/30/90);归档动作记入会话事件流可撤销(G5) | `commands_sessions.rs`、`SidebarSessions.tsx`、C1 新分区 | M |
| C6 | 工具调用分组:连续只读工具(读/搜)聚合为 Explore 组、连续非只读 Shell 聚合为 Terminal 组、连续 Write/Edit/Patch 聚合为 Changes 组;分组卡可展开,默认折叠;三个独立开关(G4) | `MessageBubble.tsx`、`StreamingResponse.tsx` | L |

> 批次 C 各开关与 ZCode 一样放 UI 层即可(分组纯前端;C1–C3 需 engine 配置接线)。

### 批次 D — 系统基建(需单独排期/外部依赖,先立项后实施)

| # | 内容 | 前置依赖 | 量 |
|---|---|---|---|
| D1 | in-place 更新:tauri-plugin-updater + 签名密钥体系 + 预览版频道 + 自动下载(下载完成后若有任务运行,重启前确认) | **发布流水线签名决策(决策点 ⑤)** | L |
| D2 | 终端:①「继承登录 shell 环境」开关(以 login shell 启动或 `SHELL -l -i -c 'env; exec ...'` 快照注入代理/Kube 变量)② 终端字体族输入框(留空 = 现有等宽栈;自动探测系统终端配置后置 P3) | 无 | M |
| D3 | 数据存储路径:第一步只做「当前路径只读展示 + `$SHANNON_HOME` 说明 + 目标路径校验」;复制迁移(含 sessions/providers/credentials/desktop config/gateway 数据)单独立项 | 决策点 ⑥ | S(第一步)/ L(迁移) |
| D4 | 界面模式语义扩展(实验):simple 模式下消息流默认折叠命令原始输出、只展示摘要与结果,dev 模式全量——作为 flag 灰度,验证后再决定是否对齐 ZCode 的办公/编程双模式 | 决策点 ⑦ | L |
| D5 | 主动任务推荐:先调研(基于 OpenPeak/空闲队列 + 会话历史),不进本排期 | — | — |

---

## 8. 决策点(审查时请拍板)

1. **批次顺序**:建议 A → B → C → D;若近期主打企业客户,可 B 提到 A 后立即执行(C 挪后)。
2. **keep-awake 语义**:推荐「任务运行时阻止休眠(默认开)+ 全局常开开关(默认关)」双层;还是仅对齐 ZCode 的单一全局开关?
3. **新分区方案**:建议新增「网络」(B1)与「会话」(C1)两个分区 + 「关于」(A1),设置栏从 8 → 10~11;是否接受?或网络项并入 General?
4. **自动压缩开关的默认值**:保持现状(开)仅暴露关闭能力,还是默认关?推荐前者。
5. **更新基建(D1)**:是否启动签名证书/发布流水线决策?不启动则 A1 的「关于」区维持手动检查形态。
6. **数据路径(D3)**:是否只需要第一步(只读展示 + 文档)?
7. **界面模式(D4)**:是否值得做办公/编程双模式实验,还是保持 simple/dev 现状?
8. **显示思考过程默认值**:推荐「全部显示(折叠)」。
9. **自动归档默认保留期**:ZCode 为 7 天;Shannon 现有 GC 档位是 30/90,推荐归档默认 7 天、GC 档位补 7 并保持永不默认。

---

## 附录 A:关键文件索引

| 领域 | 文件 |
|---|---|
| 设置页框架 | `desktop/ui/src/pages/Settings.tsx`(分区导航、dev-gate) |
| 各分区 | `desktop/ui/src/components/settings/*.tsx`(General/Theme/Models/Permissions/Advanced/Notifications/Connections/Remotes) |
| configure 后端 | `desktop/src/commands_config.rs:445-1025`(key 穷举)、`desktop/src/config.rs:82-107,705-708`(desktop config.json) |
| 引擎 HTTP 客户端 | `crates/shannon-engine/src/api/client.rs:136-145`(B1 注入点) |
| 通知 | `desktop/src/notifications.rs`、`desktop/src/commands_notifications.rs` |
| 防休眠 | `crates/shannon-core/src/prevent_sleep.rs` |
| 终端 | `desktop/src/terminal_commands.rs`、`desktop/ui/src/components/terminal/TerminalPanel.tsx:99`、`settings/TerminalSettings.tsx` |
| 消息流 | `desktop/ui/src/components/chat/MessageBubble.tsx:461-534`、`StreamingResponse.tsx:73-92` |
| 压缩 | `crates/shannon-core/src/query_engine/engine/agent_loop.rs:1279-1452`、`crates/shannon-engine/src/compact/compact_messages.rs:99-115` |
| ask_user | `crates/shannon-tools/src/ask_user.rs`、`desktop/src/loopback_api.rs:320-331` |
| 会话 GC/归档 | `desktop/src/commands_sessions.rs:552-666`、`crates/shannon-core/src/housekeeping.rs` |
| 队列/插话 | `desktop/ui/src/context/AppContext.tsx:692-748`、`desktop/ui/src/hooks/useSteerSend.ts` |
| 更新检查 | `desktop/src/commands_surface.rs:206-345`、`AdvancedSettings.tsx:588-644` |

## 附录 B:调研方法

三路并行代码调研(设置 UI 穷举 / Rust 系统能力 / 交互行为映射)+ 人工抽查复核(更新入口 dev-gate、终端字体硬编码、SUPPORTED_LOCALES 无跟随系统、Advanced 路由无守卫,均已亲自验证)。行号以 2026-10-05 工作区状态为准。
