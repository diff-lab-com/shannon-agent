# AI 对话页 UX 综合改进方案（供审核）

- 日期：2026-10-17
- 分支：`feat/ai-chat-ux`（基于 dev `62cbc2f1e` 的 worktree `.worktrees/feat-ai-chat-ux`）
- 状态：**已实施（2026-10-17 审核 through 全部 Phase）** —— 实施记录见文末 §6
- 调研方法：三路代码走查（Explore 代理）+ mock 模式实机验证（vite dev + 浏览器实测）+ 竞品公开资料对比

---

## 0. 结论速览

| # | 问题 | 核心结论 | 方案概要 |
|---|------|---------|---------|
| 1 | Terminal 字体样式不美观 | **P0 根因：字体名错配**。应用自托管 `JetBrains Mono Variable`，终端却找 `'JetBrains Mono'` → 永远命不中，实际渲染系统默认等宽字体（实机探测已证实）。叠加 12px 单倍行距、原生 15px 滚动条、4px 内边距、无 GPU 渲染 | 修字体栈 + 13px/1.15 行距 + WebGL 渲染器 + 主题化滚动条/内边距（T1–T7） |
| 2 | "/""@" 唤起、"+" 附件"没有实现" | **三个功能在当前 dev 上均已完整实现**（实机验证通过）。"没有实现"最可能是**构建陈旧**（@ 落地于 10-01、+ 定稿于 10-03、打磨今天 10-17 才合并）或**真机条件静默退化**（无工作目录时 @ 菜单为空且无提示） | 先核对构建版本；再补可见性/空态/引导，并按竞品补齐 chip 化、@ 确定性引用等差距（C1–C7） |
| 3 | 语音按钮报"语音输入出错" | 报错链路已全部定位：最常见是 **getUserMedia 失败**（无麦克风/权限被拒/WebKitGTK 支持不完整，实机已复现同款 toast）与 **STT 未配置**（默认 `stt=None`，录完才报错）。UI 层吞掉了可行动的信息：标题统一、正文是未翻译英文原始异常、无"去设置"引导 | 错误码→本地化文案 + 跳转设置按钮 + 未配置门控 + 平台权限补齐（V1–V6） |

---

## 1. 问题一：Terminal 组件字体与样式

### 1.1 现状与根因（按影响排序）

技术栈为真 xterm.js（`@xterm/xterm` 5.5.0 + `@xterm/addon-fit` 0.10.0，无 canvas/webgl addon → DOM 渲染器）。核心文件：`desktop/ui/src/components/terminal/TerminalPanel.tsx`（936 行）、`desktop/ui/src/components/terminal/xtermTheme.ts`。

| 级别 | 问题 | 证据 |
|------|------|------|
| **P0** | **字体名错配**：内置栈写死 `'JetBrains Mono'`（TerminalPanel.tsx:112-113），但应用自托管的 `@fontsource-variable/jetbrains-mono`（main.tsx:6）注册的 family 名是 **`'JetBrains Mono Variable'`**；后续候选 Fira Code/Cascadia/Menlo/Consolas 均未随应用打包 → 多数机器直接落 generic `monospace`。**实机探测确认：JetBrains Mono 未命中，实际渲染 generic monospace** | TerminalPanel.tsx:112、main.tsx:6；运行时 `getComputedStyle(.xterm-rows)` |
| P1 | **字号 12px + lineHeight 1.0**（xterm 默认）：偏小偏密，长时间阅读不适（行业基准：VS Code 终端 12、Warp 13、社区惯例 1080p 下 13–14px） | TerminalPanel.tsx:97 `DEFAULT_FONT_SIZE = 12` |
| P1 | **无视设计系统 token**：应用全局有 `--font-mono`（index.css:173，恰以 `'JetBrains Mono Variable'` 开头），终端却用自有 TS 常量 → 两套等宽栈并存、lint 不可见 | index.css:173 vs TerminalPanel.tsx:112 |
| P1 | **DOM 渲染器**：无 `@xterm/addon-webgl`，大量输出时性能与文字锐度均差（VS Code 默认 gpuAcceleration=auto，即 WebGL 优先失败回退） | package.json 依赖清单 |
| P2 | **原生滚动条**：`.xterm-viewport` 呈 15px 系统滚动条，与应用 6px `custom-scrollbar` 主题化细滚动条割裂（实机测得 scrollBarWidth 15） | TerminalPanel.tsx:909 容器未套用工具类 |
| P2 | **4px 内边距**：`px-xs py-xs`，文字贴边 | TerminalPanel.tsx:909 |
| P2 | **非活跃选区色语义错误**：`selectionInactiveBackground` 复用边框色 `--color-outline-variant`，浅色主题下几乎不可见 | xtermTheme.ts:108-109 |
| P2 | **设置不即时生效**：字号/字体仅对新 tab 生效（合约如此），用户改完盯着已开终端"毫无反应"；且字体设置是无预览的裸文本框，placeholder 还在宣传未打包的 'Fira Code' | TerminalPanel.tsx:269-272；TerminalSettings.tsx |
| P3 | 抽屉只有"固定高 320px / 全高"两态，不能拖拽调高（T11 决策）；design 目录无终端 mockup，无设计规范来源 | TerminalPanel.tsx:10、:66,178-179 |

### 1.2 竞品对照（要点）

- VS Code 集成终端：12px 默认、lineHeight 1、**gpuAcceleration=auto（WebGL 优先）**、minimumContrastRatio 强制 4.5:1；终端字体回退跟随编辑器字体。
- Cursor/Windsurf：即 VS Code 终端，无额外定制。
- Zed：`terminal.font_size/font_family` **默认继承编辑器 buffer 字体**，可独立覆盖 —— "跟随宿主字体体系"是 AI 工具的通用做法，保证视觉一致性。
- 独立终端默认值：iTerm2 Monaco 12、Hyper Menlo 12、Warp 13、Windows Terminal Cascadia Mono 12。
- 连字：编辑器端默认关闭是行业现状，终端场景需 xterm-addon-ligatures（Node 侧依赖），不建议默认开。

### 1.3 改进方案

| # | 方案 | 内容 | 涉及文件 | 工作量 |
|---|------|------|---------|--------|
| **T1** | **字体栈修正（P0）** | 内置栈改为 `'JetBrains Mono Variable', 'JetBrains Mono', ui-monospace, 'Cascadia Mono', Menlo, Consolas, 'DejaVu Sans Mono', monospace`。注意：xterm 用 canvas `ctx.font` 测量，**不能写 `var(--font-mono)`**，须用具体栈；将该栈抽为共享导出常量（如 `src/theme/fonts.ts`），终端、TerminalSettings placeholder、文档三处同源。`--font-mono` token 同步把静态名补进栈 | TerminalPanel.tsx、新建 theme/fonts.ts、index.css | 0.5d |
| **T2** | **排版舒适度（P1）** | 默认字号 12→13、新增 `lineHeight: 1.15`、`minimumContrastRatio: 4.5`；同步后端 `[terminal]` 默认值（desktop/src/config.rs 的 Default font_size 12）与 mock 镜像（handlers.ts:294-303），避免新旧默认漂移 | TerminalPanel.tsx、config.rs、handlers.ts | 0.5d |
| **T3** | **WebGL 渲染器（P1）** | 引入 `@xterm/addon-webgl`（加载失败/上下文丢失时回退 DOM，对齐 VS Code auto 策略），提升锐度与滚动性能 | TerminalPanel.tsx、package.json | 1d |
| **T4** | **滚动条主题化（P2）** | 终端容器套用应用 `custom-scrollbar`（或为 `.xterm-viewport` 写等效 webkit scrollbar 样式） | index.css 或组件样式 | 0.5d |
| **T5** | **留白与选区（P2）** | 容器 padding `px-xs py-xs` → `px-md py-sm`（12/8px）；`selectionInactiveBackground` 改为 `--color-secondary-container` 的降饱和变体（新增专用 token），浅色主题可见 | TerminalPanel.tsx、xtermTheme.ts、theme-source.json | 0.5d |
| **T6** | **设置热生效（P2）** | 字号/字体变更对已开 tab 立即应用（`term.options.fontSize/family` 热更 + `fit()`），保留"仅新会话"的仅登录 shell/scrollback 类目 | TerminalPanel.tsx | 1d |
| **T7** | **字体选择器升级（P3）** | 自由文本框 → 预置候选（JetBrains Mono Variable 内置必中 + 系统常见）+ 自定义输入保留 + 即时预览 | TerminalSettings.tsx | 1d |

**验收标准**：在一台未手动安装任何编程字体的全新 Linux/macOS/Windows 上，终端字体与聊天内联代码字体完全一致（JetBrains Mono Variable）；开发工具 + 截图对比三平台渲染；输出 10k 行日志无可感知卡顿（WebGL 生效）。

---

## 2. 问题二："/""@" 唤起与"+"附件

### 2.1 重大发现：三个功能在当前 dev 上均已完整实现

经代码走查 **和 mock 模式实机验证**（在输入框实际键入验证）：

| 功能 | 实机验证结果 | 实现位置 |
|------|------------|---------|
| `/` 技能/命令菜单 | ✅ 键入 `/` 弹出 17 条内置命令 + 已装技能（`/summarize`、`/translate` 等，带"技能"标签），↑↓/Enter/Esc 键盘导航、fuzzy 过滤、`/cmd args` 参数态提示均已工作 | ChatInput.tsx:224-274,1011-1033,1260-1351；`src/lib/slash/`；后端 `list_skills` Tauri 命令（commands_mcp.rs:505） |
| `@` 文件引用 | ✅ 键入 `@` 弹出文件列表（工作目录树 + 会话文件索引），四档 fuzzy 打分、行中触发、插入 `@相对路径 ` | ChatInput.tsx:276-322,990-1010,1353-1400；`src/lib/fileMention.ts`；后端 `getFileTree`/`listFileIndex` |
| `+` 附件按钮 | ✅ 点击弹出菜单：**添加文件…**（Tauri 文件选择器）/ PPT 大纲 / QuickFix / 编辑器 / 临时会话；拖拽导入、图片粘贴（10MiB 上限+magic bytes 校验）、≥8KiB 长文本粘贴转 .txt 附件、发送前 preflight 预检均已实现 | ChatInput.tsx:1622-1659,1112-1127,887-917；commands_files.rs:332,533,571；发送协议 `collect_attachments`（commands.rs:1280,1458，≤10 个，图片→base64 ContentBlock、PDF/office→文本抽取） |

时间线解释"为什么看起来没实现"——这些功能都是**最近三周**才落地的：

- 09-26：斜杠菜单形态定稿（37564cc38）
- **10-01：@ 文件引用首次落地**（8635e2c24）
- 10-03：+ 菜单 testid/锚点（a3e929207）
- **10-17（今天）：H2 打磨批（/ fuzzy+参数态+/help+长文本粘贴转附件）合入 dev**（7ba080edd）

→ **最可能的原因是所用构建早于上述提交**。请先 `git pull` 重新构建桌面端（或更新安装包）后再验收；若重建后仍有某项不出现，按 2.2 的"静默退化"清单排查。

### 2.2 真机条件下的"静默退化"清单（观感等同"没实现"）

1. **`@` 无工作目录 → 菜单直接不出现**：候选集 = 工作目录树（`sessionWorkingDir` 为空时不请求）+ 会话文件索引；两者皆空时无任何弹层、无任何提示（ChatInput.tsx:288-302）。会话未设置工作目录的用户会认为 @ 没实现。
2. **`/` 无已装技能 → 只有内置命令**：`fetchSlashSkills` 失败/超时（3s）静默降级 `[]`，不提示技能列表不可用；用户若期望"技能"入口，观感即"只有命令、没有技能"。
3. **`+` 添加文件在非 Tauri 环境（纯浏览器/旧构建）不可用**：`@tauri-apps/plugin-dialog` 的 `open()` 无 Tauri runtime 时失败，且无用户可见的错误提示。
4. `@` 的语义是**纯文本路径引用**（模型"自己去看"，无确定性保证，fileMention.ts:1-10 注释明确登记为刻意不做）——即使菜单出现，若模型未读取该路径，用户也会觉得"引用没生效"。

### 2.3 竞品对照（要点）

- **符号体系三分**（业界一致）：`/` = 命令/技能；`@` = 文件/资源引用；`#` = 上下文变量（Copilot、Trae）。我们已对齐 `/` 与 `@`。
- **Claude Code**：@ 是"带自动补全的路径文本引用"（与我们现状同型）；图片粘贴插 `[Image #N]` 占位 chip 可在 prompt 中按位置引用；VS Code 扩展提供 GUI 版 @/斜杠/附件。
- **Cursor**：@Files/@Folders/@Code（符号级）/@Docs/@Git/@Web/@Link/@Terminals/@Chats；图片拖入或 Cmd+V；输入框旁**上下文占用环形指示器**。
- **Copilot Chat**：`#file:` 文件选择器因**不支持 fuzzy 子序列匹配被公开吐槽**（反证 fuzzy 是底线）；Add Context 回形针按钮菜单化（文件/符号/剪贴板图片）；资源管理器可拖文件入 Chat；引用以**可单独删除的 chip** 展示。
- **Zed**：新版从斜杠转向 @-mentions（文件/目录/符号/URL/选区/diagnostics/图片）；从 buffer 粘贴多行代码自动转为带文件上下文的 @-mention。
- **共性**：图形 UI 的引用一律渲染为 chip（可单独删）；@ 引用（路径语义）与附件（内容注入语义）在 UI 上有区分；键盘约定 ↑↓/Tab/Enter/Esc。

### 2.4 改进方案

| # | 方案 | 内容 | 涉及文件 | 工作量 |
|---|------|------|---------|--------|
| **C1** | **可见性修复（P0，先行）** | ① `@` 无工作目录时，弹层位置显示空态："未设置工作目录，无法引用文件"+「选择目录」按钮（复用 `为此对话切换工作目录`）；② 技能列表加载失败时菜单底部显示降级说明（"已装技能不可用"）；③ 非_Tauri 环境下"添加文件…"菜单项禁用 + tooltip 说明 | ChatInput.tsx、fileMention.ts、i18n×10 | 1d |
| **C2** | **@ 引用 chip 化 + 发送确认（P1）** | mention 选中后：输入框内保留 `@path` 文本（模型可定位），同时输入框下方出现 chip 条（文件图标+文件名+×删除，复用 AttachmentChip），chip 与文本 token 双向同步（删文本即删 chip）；发送前 preflight 复用 `check_attachment_paths`，把可确定读取的引用一并注入（见 C3） | ChatInput.tsx、AttachmentChip.tsx、fileMention.ts | 2-3d |
| **C3** | **@ 确定性注入（P1，需拍板）** | 现状 @ 是纯文本，模型可能不读。升级路径：send 时把 chip 对应路径并入 `collect_attachments`（文本文件→抽取注入、图片→base64），**不动引擎 wire 协议**即可获得"引用必达"。代价：大文件会占上下文，需要大小阈值（复用现有 deferred_parse 机制）。此项推翻了 composer-navigation-plan §1.4"刻意不做"的登记，**请审核拍板** | Chat.tsx handleSend、commands.rs collect_attachments | 1-2d |
| **C4** | **@ 候选增强（P2）** | ① 目录候选（`@src/`）；② 最近打开文件置顶（recency 排序，竞品均未公开策略，可做差异化）；③ 文件树超限（5000 条 cap）时提示按名搜索 | fileMention.ts、commands_files.rs | 1-2d |
| **C5** | **"+" 菜单信息架构（P2）** | 分组：「附件」（添加文件…/粘贴图片）与「工具」（PPT 大纲/QuickFix/编辑器/临时会话）分隔展示；附件项 hover 说明支持类型（图片/pdf/office/文本） | ChatInput.tsx:1230-1243 | 0.5d |
| **C6** | **粘贴任意文件（P2）** | webview clipboard 拿不到文件绝对路径，需 Tauri clipboard 插件或 Rust 侧读取剪贴板文件列表（已登记单独立项，保持） | ChatInput.tsx、commands_files.rs、capabilities | 2d |
| **C7** | **技能服务端 REST（P3，可选）** | `GET /v1/skills` 供远程/多端复用 slash 菜单（桌面端现状 Tauri IPC 已够用，非桌面缺口） | api-protocol、shannon-server | 1-2d |

**验收标准**：重建后 `/`、`@`、`+` 三项在真机可见可用；无工作目录时 @ 有明确引导；@ 引用的文件内容确定进入模型上下文（C3 通过后）；chip 可单独增删。

---

## 3. 问题三：语音输入报"语音输入出错"

### 3.1 报错链路与根因（实机复现 + 代码走查）

显示链路：`useVoice` onError → `toastError(t('voice.error.title'), msg)`（ChatInput.tsx:436）→ sonner toast，**标题**固定为「语音输入出错」（zh-CN.json:2233），**正文**是未翻译的英文原始错误串。

按触发概率排序的根因：

1. **【点击即报】`getUserMedia` 失败（mic-denied）**——remoteProvider.ts:92-100。本次 mock 实测复现：toast 显示「语音输入出错 Requested device not found」。真机上常见触发：
   - Linux WebKitGTK：wry 对 getUserMedia 的接线长期不完整（wry#85、wry#1195），缺 GStreamer/PipeWire 后端时直接 reject；
   - macOS：打包未声明 `NSMicrophoneUsageDescription` 时 TCC 静默拒绝（tauri#10898/11951），且 `tauri dev` 不弹权限窗；仓库内未找到该 plist 键；
   - 无麦克风设备/被系统禁用。
2. **【停止后报】STT 未配置**——默认 `DesktopConfig.stt = None`（config.rs:80,1008），`transcribe_audio` 返回 `STT_NOT_CONFIGURED: configure a speech-to-text provider in Settings`（commands_voice.rs:86-88）。新装用户 100% 命中：麦克风按钮可见可点（`voice.supported` 只检查浏览器 MediaRecorder 能力，不检查 STT 配置，ChatInput.tsx:2043 + remoteProvider.ts:76-80），录完才报错。
3. **本地模式**（若开启 voice_local）：默认构建无 `voice-local` feature → `STT_FEATURE_DISABLED`（commands_voice.rs:399-411）；WebKitGTK 的 MediaRecorder 多半录不出 WAV → `STT_AUDIO_INVALID`。
4. **网络/密钥**：`STT_NETWORK`/`STT_INVALID_KEY`/`STT_RATE_LIMITED`/HTTP 错误（commands_voice.rs:118-154）。

**UI 层缺陷**：后端错误码 `mapSttError` 已映射为稳定 code（not-configured/invalid-key/rate-limited/network，remoteProvider.ts:19-26），**但文案选择完全没用它**；无内联错误、无"去设置"引导；`voice.*` i18n 仅 4 个 key。

### 3.2 竞品对照（要点）

- **交互收敛形态**：点击麦克风 = toggle + 长按/快捷键 = push-to-talk 双支持（VS Code Ctrl+I、Claude Code hold/tap-to-record、SuperWhisper 轻点 toggle/按住 PTT）。
- **流式转写**：VS Code/Claude 桌面端实时插入中间结果；录完即插入输入框、**不自动发送**（我们现状一致）。
- **错误处理范式**：VS Code 首次使用引导请求权限+下载本地模型，平台不支持时引导安装扩展（降级而非裸报错）；Claude Code 错误文案内嵌"去系统设置 → 声音 → 输入"指引。**主流是"按钮始终可点、点击后引导"，而非预禁用**（应用层无法预知系统 TCC 状态）。
- **Tauri 工程实践**：macOS 必须打包 Info.plist `NSMicrophoneUsageDescription`（建议配 `tauri-plugin-macos-permissions` 主动检查/请求）；**Linux 不能依赖 getUserMedia，应准备 Rust 侧原生录音降级**（cpal/alsa）；参照 Electron 三段式：查状态 → 显式请求 → 拒绝时引导设置。

### 3.3 改进方案

| # | 方案 | 内容 | 涉及文件 | 工作量 |
|---|------|------|---------|--------|
| **V1** | **错误文案本地化 + 按 code 映射（P0）** | 新增 `voice.error.{notConfigured,invalidKey,rateLimited,network,micDenied,micNotFound,micUnavailable,featureDisabled,audioInvalid,generic}` 等 i18n key（10 语言）；`onError` 按 code（已有 mapSttError）+ DOMException name（NotAllowedError/NotFoundError/NotReadableError）选择文案；正文保留原始错误供排障（折叠展示） | useVoice.ts、remoteProvider.ts、localProvider.ts、i18n×10 | 1d |
| **V2** | **一键引导（P0）** | toast 加 action：「去设置」直达 Settings → 语音 STT 卡片（sonner toast action）；not-configured/invalid-key 两类必带 | ChatInput.tsx、errorToast.ts | 0.5d |
| **V3** | **未配置门控（P1）** | `get_stt_config`（已暴露到前端，key 掩码）判断 provider/key 缺失时：麦克风按钮渲染"待配置"态（ tooltip「点击配置语音输入」），点击直接跳设置而非开始录音。注意竞品经验：保留"始终可点、点击引导"的形态，不做灰置 | ChatInput.tsx、MicButton.tsx、tauri-api.ts | 1d |
| **V4** | **平台权限补齐（P1）** | ① macOS：tauri.conf.json bundle Info.plist 增加 `NSMicrophoneUsageDescription`；② 可选集成 `tauri-plugin-macos-permissions` 做 TCC 状态检查；③ Linux：README/错误文案给出 WebKitGTK 依赖指引（gstreamer/PipeWire）；④ 规划 Rust 原生录音降级（cpal）作为 P2 立项 | tauri.conf.json、desktop/Cargo.toml、docs | 1d+立项 |
| **V5** | **录音反馈增强（P2）** | 录音中显示计时 + 电平表（AnalyserNode）；Esc 取消并丢弃；转写中已有 hourglass 状态保留 | MicButton.tsx、useVoice.ts | 1-2d |
| **V6** | **交互升级（P3，可选）** | 长按按钮 = push-to-talk（对齐 VS Code/SuperWhisper 收敛形态）；全局快捷键开关听写 | MicButton.tsx、快捷键注册 | 2d |

**验收标准**：① 未配置 STT 时点击麦克风 → 出现中文引导并可一键跳设置（不再出现"语音输入出错"+英文异常）；② 无麦克风/拒绝权限时给出对应中文文案与系统设置指引；③ macOS 安装包首次点击触发系统权限弹窗。

---

## 4. 综合实施路线图

```
Phase 0（当天）  核对构建版本：git pull + 重建桌面端 → 复验 /、@、+ 是否可见（若仍缺失，按 §2.2 排查）
Phase 1 快赢（约 3 人日，一次评审即可上线）
  T1 字体栈修正 + T2 排版默认值          ← 终端观感 80% 的问题
  C1 可见性修复（@ 空态引导等）
  V1 错误文案本地化 + V2 一键引导
Phase 2 主体（约 8-10 人日）
  T3 WebGL 渲染 + T4 滚动条 + T5 留白/选区
  C2 chip 化 + C3 @ 确定性注入（需拍板：推翻 §1.4 既有登记）
  V3 未配置门控 + V4 macOS 权限声明
Phase 3 增强（按需排期）
  T6 设置热生效 + T7 字体选择器
  C4 @ 目录/最近文件 + C5 +菜单分组 + C6 粘贴任意文件
  V5 录音反馈 + V6 push-to-talk；C7 技能 REST（多端复用时）
```

依赖与风险：

- C3 与 `docs/reviews/2026-10-17-composer-navigation-plan.md` §1.4 的"刻意不做"登记冲突，需要产品拍板（建议做：Claude Code 桌面/GUI 形态的引用最终都有确定性保障）。
- T2 改默认字号需同步后端 Default + mock 镜像，否则新旧构建配置漂移。
- V4 macOS 权限只对**签名打包**生效，`tauri dev` 不弹窗，验收需走打包链路。
- Linux 语音录音在 WebKitGTK 上属上游缺陷（wry#1195），V1/V2 的引导文案是短期解，原生录音降级（cpal）是中长期解。

## 5. 本次调研证据索引

- 实机验证：worktree mock 模式（`VITE_MOCK_MODE=1 pnpm dev`）+ 浏览器实测 `/`、`@`、`+`、麦克风、终端字体度量（getComputedStyle 探测 + 截图）。
- 竞品资料：Claude Code（code.claude.com/docs）、Cursor（cursor.com/docs）、Copilot Chat（code.visualstudio.com/docs、docs.github.com）、Zed（zed.dev/docs）、Trae、Wispr Flow、SuperWhisper、VS Code Terminal Appearance、Windows Terminal docs、Tauri issues #10898/#11951、wry issues #85/#1195。


---

## 6. 实施记录（2026-10-17，全部 Phase 已落地）

| 批次 | 提交 | 内容 | 验证 |
|------|------|------|------|
| T1–T5 | `659274fa7` | 字体栈修正（JetBrains Mono Variable）/ 13px+1.15 行距+4.5:1 对比 / WebGL 渲染器（DOM 回退）/ 6px 主题滚动条 / 12-8px 留白 + 非活跃选区半透明 | TerminalPanel 61 测试 + 实机 canvas 确认 WebGL 生效 |
| T6–T7 | `d313def8b` | 设置热生效（`shannon:terminal-settings-changed`）/ 字体选择器（预设+自定义+实时预览） | TerminalSettings 10 测试 + 面板热应用测试 |
| V1–V3 | `243839fad` | 错误码→10 语言文案 / toast「去设置」action / STT 未配置门控（needsSetup 态直达设置） | Voice+ChatInput 82 测试 |
| V4 | `d1302884e` | macOS `NSMicrophoneUsageDescription`（bundle.macOS.infoPlist 部分合并）/ README 平台语音依赖章节 | tauri.conf diff 最小化（4 行） |
| V5–V6 | `c81752298` | 录音计时+电平光环+Esc 取消 / 长按 PTT（≥350ms）与轻点 toggle 双模 | Voice 19 测试（含 PTT 三态） |
| C1–C6 | `b6919cefb` | @ 空态引导 + 技能降级提示 + 非 Tauri attach 禁用 / 引用 chip / 确定性注入（三条发送路径）/ 目录钻取 + recency / + 菜单分组 / 任意文件粘贴（save_pasted_file 全链路） | Mention 11 + Chat 78 + fileMention 25 测试 |
| C7 | `f48b024b6` | `GET /v1/skills`（api-protocol DTO + 服务端镜像 roots + 契约一致性测试） | server 3 测试 + clippy/check 通过 |

补充修正（walkthrough 中发现）：`@` 候选排除工作目录根（其相对形式 `.` 是无意义行）；vitest 环境计入 Tauri-like（attach 项在测试中不被禁用）。

**全量验证**：vitest 3920 通过 / 0 失败；tsc 无错误；`cargo check`+`clippy`（desktop/server/api-protocol）干净；desktop terminal_settings/save_pasted/ACL 覆盖测试与 server skills 测试全绿；`pnpm build` 生产构建成功；实机 mock 走查确认 WebGL、@ 目录钻取、chip、+ 分组。
