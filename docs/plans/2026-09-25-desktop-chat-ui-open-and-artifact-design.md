# 桌面端 AI 对话页审查与改进方案：链接/文件引用「高亮 + 打开」与右侧产物面板

- 日期：2026-09-25
- 状态：**已评审定稿（2026-09-25）**——决策点 1–5 按建议执行；决策点 6 改判为「链接默认在右侧面板打开，右键菜单两项且面板优先」。未开始任何实现，下一步出实施计划。
- 范围：`desktop/ui` 聊天页（`pages/Chat.tsx` 及 `components/chat/*`）、右侧 Dock（`pages/chat/RightDock.tsx`、`components/artifact/*`）、Tauri 打开外部资源的能力面（`desktop/src`、`desktop/capabilities`）
- 关联文档：`desktop/COMPETITIVE-ANALYSIS.md`（2026-06-13 产品级竞品基线）；本报告是其 §3.4「可视化反馈」在**对话页交互粒度**上的细化与更新

---

## 0. TL;DR

对话页的消息流渲染、diff 审查、dev server 预览已经做得扎实（`FileChangesCard`、`DiffReviewBody`、`LivePreview`、文档 TOC/表格导出等都是亮点）。但围绕「**内容引用 → 打开**」这条最基本的桌面交互链路，存在三个结构性缺口：

1. **P0 · 链接点不开**：聊天与文档里的外部链接只是裸 `<a target="_blank">`，全应用**没有** shell/opener 权限、没有点击拦截、没有 newWindow 处理——点击行为完全交给 WebView 默认（各平台不一致，常见为无反应）。桌面应用「点链接 → 系统浏览器」这一默认期望未成立。（评审补充需求：链接还要支持「在右侧面板打开」——已纳入 P0-A 目标选择与 P1-E 面板网页 tab。）
2. **P0 · 文件路径不可点**：消息正文（含内联代码）中的文件路径**无任何识别/高亮**；工具卡里只有 5 个「写文件」类工具有 Diff 按钮，Read/Bash/Grep 等提到的路径是纯文本。
3. **P1 · 磁盘产物不入面板**：右侧 Dock 的 artifact 只从聊天 markdown 围栏检测（html/svg/mermaid/长 markdown）；而本产品 agent 的主要产出方式是 **write_file 写磁盘文件**——写出的 `report.md` / `index.html` 根本不会出现在 Dock 里。渲染格式不支持（PDF/图片等）时也没有「用系统默认程序打开」的逃生门。

按 **P0-A 打开管线 → P0-B 路径高亮 → P1-C 磁盘产物入 Dock → P1-D 格式扩展与逃生门 → P1-E 面板网页 tab → P2 细节** 六步走，总工作量约 4.5~7.5 人日。§5 的 6 个决策点已全部拍板：1–5 按建议执行；6 改判为「**链接默认在右侧面板打开**」，右键菜单两项且面板优先（XFO 风险已配套降级方案，见 P1-E）。

---

## 1. 现状盘点（证据为准）

### 1.1 聊天页结构

`pages/Chat.tsx:203-263`：左 Sidebar → 中间消息区（ApiKeyBanner → BudgetBanner → `MessageArea` → `ComposerPanel` → TerminalPanel）→ 右 `RightDock`（Ctrl+\ 开关，宽度 280–720 可拖拽，支持全屏阅读态，tab/宽度/全屏均持久化）。消息 >30 条走 `useVirtualizer` 虚拟化（`Chat.tsx:102-110`、`MessageArea.tsx:18`）。

气泡类型（`MessageBubble.tsx`）：
- **user**：纯文本 `<p whitespace-pre-wrap>`（:243-258），**不渲染 Markdown**——用户粘贴的链接同样不可点。
- **assistant**：`FootnoteMarkdown` → `Markdown`（react-markdown + GFM + rehype-highlight + sanitize），正文后附 `ArtifactChipList`（:334）。
- **tool**：`ToolCallDisplay` 卡片（:600-680），状态图标 + 工具名 + 耗时 + tokens + 展开。

### 1.2 「链接 / 文件引用」现状

| # | 场景 | 现状 | 证据 |
|---|------|------|------|
| L1 | 正文外部链接 | 渲染为带 `open_in_new` 图标的 `<a target="_blank">`，**样式上已高亮** | `Markdown.tsx:193-227` |
| L2 | 点击外部链接 | **无任何接管**：capabilities 无 `shell:*`/`opener:*`；无全局 click/auxclick 拦截；Rust 无 `on_navigation`/newWindow 处理；`withGlobalTauri=false`。唯一 Rust 打开命令 `open_release_page` 白名单仅 GitHub 官方 repo URL | `capabilities/*.json`、`commands_surface.rs:315-340`、`main.rs:383` |
| L3 | 相对路径/锚点链接 | `isExternal=false` 不加 target，点击会在 WebView 内导航（有把应用窗口导航走的风险；脚注 `#fn-` 锚点是合法内跳） | `Markdown.tsx:195-209`、`FootnoteMarkdown.tsx:92-100` |
| F1 | 正文中的文件路径（含内联代码） | **无识别、无高亮、不可点**。`InlineCode` 只有样式；全 src 无 path/linkify 检测逻辑 | `Markdown.tsx:178-185` |
| F2 | 写文件类工具（write_file/edit_file/apply_patch/str_replace_editor/replace） | 完成后卡片有 **Diff 按钮** → `onViewDiff(path)` → RightDock diff tab；同消息多文件聚合为 `FileChangesCard`（+x−y 行数、单文件/全部审查、rewind 撤销）——**这是现有最接近「文件引用可交互」的部分** | `MessageBubble.tsx:463,538-547,648-659,470-536` |
| F3 | Read/Bash/Grep 等工具 | 路径在展开的输入摘要里是**纯文本**，不可点 | `MessageBubble.tsx:692-708` |
| F4 | 用户附件 | 发送前 `AttachmentChip` 无点击行为；历史消息 `AttachmentPreview` 图片可开全屏，非图片的「在外部打开」实为 `window.open(asset://…)` 在 WebView 内打开，**并非系统默认程序** | `AttachmentChip.tsx`、`MessageBubble.tsx:95-172,151-155` |
| F5 | 正文图片 | `file://`/绝对路径经 `convertFileSrc` 转 asset 协议渲染（asset scope `$HOME/**`、`$TEMP/**`） | `Markdown.tsx:231-257`、`tauri.conf.json:70-76` |
| F6 | 拖拽 | 仅 ChatInput 的 HTML5 DnD（取 `file.path`），未接 Tauri 拖放事件 | `ChatInput.tsx:172-213` |

### 1.3 右侧 Dock（产物面板）现状

`RightDock.tsx` 是 2026-09 重构后的统一右侧栈：工具 tab（上下文/计划/预览/Diff）+ **每个 artifact 一个可关闭 tab**；新 artifact / 进入计划模式 / Diff 点击自动停靠并激活。

**产物来源只有一处**：`detectArtifacts()` 扫描**聊天消息 markdown 的代码围栏**（`detectArtifact.ts:51-80`）：
- `html`（≥5 行）→ HtmlRenderer（iframe sandbox + 严格 CSP）
- `svg` → SvgRenderer；`mermaid` → MermaidRenderer
- `markdown`/`md`（≥200 词，medium 置信度）→ DocumentRenderer

**渲染能力矩阵**：

| 格式 | 内嵌渲染 | 辅助能力 | 缺口 |
|------|---------|---------|------|
| Markdown 文档 | ✅ DocumentRenderer：GFM、代码高亮、标题锚点 + TOC rail、表格悬浮工具栏（TSV 复制/CSV 下载） | 代码视图切换、复制、导出 `.md` | 链接仍是裸 `target=_blank`（`DocumentRenderer.tsx:152-154`）；图片经主窗口 CSP 可渲染 |
| HTML 网页 | ✅ iframe `sandbox="allow-scripts"` + 严格 CSP（script/inline-style 允许） | 代码视图、复制、导出 `.html` | **CSP `img-src data:` → 外部图片/字体全部不显示**；无「在系统浏览器打开」逃生门（`HtmlRenderer.tsx:8-18`） |
| SVG | ✅（23 行实现） | 同上 | — |
| Mermaid | ✅ | 同上 | — |
| 代码文件 | —（仅作为 document 的源码视图） | — | 无语法级文件查看 tab |
| PDF / 图片 / CSV / Office | ❌ 不支持 | ❌ **无「系统打开」fallback** | 不支持即没有出路 |
| 磁盘真实文件 | ❌ **检测不到**（只看聊天围栏） | 「+」按钮可手动开本地文件，但实现是 `getFileDiff(path).old_content` 的 hack（`RightDock.tsx:209-219`）——diff 不存在/二进制/超大文件场景未定义 | — |

其他：`LivePreview`（dev server 探测 + iframe 沙箱 + 地址栏 + 日志，`LivePreview.tsx:16-19`）已对齐竞品「预览窗」的主要形态；`DiffReviewBody` 支持逐 hunk accept/reject。

### 1.4 Tauri 能力面（打开外部资源的底层）

- 插件：只有 `plugin-dialog`（open/save）授权给前端；`tauri-plugin-shell` 仅 Rust 侧使用（且为 deprecated API，`extensions_commands.rs:246-253` 有 `TODO: migrate to tauri-plugin-opener`）；**`tauri-plugin-opener` 完全未引入**。
- Rust 命令：无 `open_url` / `open_path` / `reveal_in_folder` / 「用编辑器打开」类命令。
- 前端依赖：`package.json` 仅 `@tauri-apps/api` + `plugin-dialog`。
- **CSP 缺 `frame-src`（连带发现）**：csp/devCsp 均为 `default-src 'self'` 且未声明 `frame-src`（`tauri.conf.json:68-69`），按 CSP 回退规则 `frame-src` 取 `default-src 'self'`——理论上**生产构建会拦掉 `<iframe src="http://localhost:…">`**，即现有 LivePreview 的 dev server 预览与新方案的「面板内嵌网页」都依赖补一条 `frame-src`（P1-E）。实施时先实测 LivePreview 生产表现：若实锤被拦，修复属于顺带救活既有功能。
- 已知工程约束（来自 `task_plan.md` 踩坑记录）：**新增 Tauri 命令必须同步 `desktop/acl/app-permissions.json` + capabilities，ACL 覆盖测试会把门**。

---

## 2. 竞品对比（对话页粒度）

> 产品级竞品分析见 `desktop/COMPETITIVE-ANALYSIS.md`（2026-06-13，信息源附录在文内）。本节聚焦「对话页如何呈现引用与产出」。标注 ⛳ 的为该文档已记载的实测/调研事实；其余为公开产品行为的常识性总结，落地前建议再实测一轮（尤其 2026-06 之后的版本变化）。

| 维度 | Claude（claude.ai / Desktop）Artifacts | ChatGPT（Desktop）Canvas | Cursor | **Shannon Desktop 现状** |
|------|------|------|------|------|
| 产物面板形态 | 对话右侧独立面板，可全屏/分享/发布 | 右侧 Canvas，可编辑、AI 局部改写 | 无独立面板（IDE 即画布） | ✅ RightDock 多 tab + 全屏，形态不落后 |
| 支持格式 | markdown、代码、**HTML（可跑外部 CDN）**、React、SVG、Mermaid | markdown、代码（含运行）；**不渲染任意 HTML**（安全取向） | — | html/svg/mermaid/markdown 四类（CSP 严格断外链） |
| 文件引用点击 | 文件/引用以 chip 呈现可点开预览 | 上传文件 chip 可点开/下载 | 路径/符号点击 → **编辑器打开**（核心交互） | ❌ 除 5 个写文件工具的 Diff 按钮外全不可点 |
| 链接打开 | Web 天然；Desktop app → 系统浏览器 | 系统浏览器 | 系统浏览器 | ❌ 未接管 |
| 磁盘产出 → 面板 | ⛳ Claude Code Artifacts（2026-06）：**编码会话产出 → live HTML 页面**，即产物来自工作过程而非聊天围栏 | Codex 产出经 Triage 呈现 | 产出即磁盘文件，天然在编辑器 | ❌ 只认聊天围栏 |
| 不支持格式 | 提供下载/导出 | 提供下载 | 系统编辑器打开 | ❌ 无出路 |

**结论**：
1. RightDock 的**内嵌渲染层**（多 tab、TOC、表格导出、LivePreview、Diff 审查）已达到或超过竞品均值——这部分是资产，方案不动它的骨架。
2. 差距集中在**引用的「可去性」**（点了能去哪儿）与**产出的「可达性」**（agent 真正写出的文件能不能被看到）。竞品桌面 app 把「链接→浏览器、文件→编辑器/预览/reveal」当作不需要讨论的默认能力；Shannon 恰好缺这一层管线，而它又是本次需求（高亮 + 打开）的地基。
3. HTML 外部资源策略竞品分化：Claude 放开 CDN，ChatGPT 干脆不渲染。Shannon 现在夹在中间（渲染但断外链且无逃生门），需要选一个明确立场（见 §5 决策点 3）。

---

## 3. 问题清单（按严重度）

### P0（基本期望不成立）

| # | 问题 | 影响 | 证据 |
|---|------|------|------|
| P0-1 | 点击外部链接行为未接管，依赖 WebView 默认（常见为无动作，跨平台不一致） | 用户核心诉求「链接高亮可打开」当前**事实上不成立**；所有裸 `<a target=_blank>` 场景（聊天、DocumentRenderer、Extensions 页、报告 Modal 引文）全部波及 | §1.2 L2 |
| P0-2 | 相对路径链接会在 WebView 内导航，可能把整个应用窗口导航离开应用 | 页面白屏/路由错乱风险 | §1.2 L3 |
| P0-3 | 正文/内联代码中的文件路径零识别、零交互 | 「AI 对话内容关联文件高亮支持打开」的另一半诉求不成立 | §1.2 F1 |
| P0-4 | Read/Bash/Grep 等工具卡路径不可点；附件「在外部打开」是 WebView 内打开而非系统程序 | 路径交互覆盖面窄且行为误导（按钮叫「外部打开」） | §1.2 F3/F4 |

### P1（产出可见性与能力缺口）

| # | 问题 | 影响 | 证据 |
|---|------|------|------|
| P1-1 | artifact 只检测聊天围栏，`write_file` 写出的 md/html/svg/mermaid 文件不入 Dock | 产出主体（磁盘文件）与展示面板脱节；与 Claude Code Artifacts 2026-06 方向相悖 | §1.3 |
| P1-2 | 不支持格式（PDF/图片/Office 等）无「系统默认程序打开」fallback | 用户只能手动找文件 | §1.3 矩阵 |
| P1-3 | HtmlRenderer CSP 断绝外部图片/字体，且无「系统浏览器打开」逃生门 | 含 CDN 资源的页面显示残缺且无解 | `HtmlRenderer.tsx:8` |
| P1-4 | Dock「+」打开本地文件走 `getFileDiff().old_content` hack | diff 不存在（新文件）/二进制/超大文件行为未定义；语义错位 | `RightDock.tsx:209-219` |
| P1-5 | `research_report` 前端有完整消费端（报告 Modal），仓库内无生产者 | 死代码/假功能风险 | `MessageBubble.tsx:240,440-457`，全仓库 grep |

### P2（打磨）

| # | 问题 | 说明 |
|---|------|------|
| P2-1 | 用户消息不渲染任何 Markdown | 用户贴的链接不可点、代码无高亮（与竞品不一致） |
| P2-2 | 无 reveal in file manager / 在编辑器中打开 | 桌面工具链常规动作缺失 |
| P2-3 | 拖拽仅覆盖输入框 | 可顺带接 Tauri drag-drop（非本次必须） |
| P2-4 | `ExternalLink` 的 hover 取 title 是显式 no-op 占位 | 要么实现（需 CORS 代理）要么删除死代码 |

---

## 4. 改进方案

> 原则：**先修管线，再修展示**。P0-A 是其余一切的地基；P0-B/P1-C 相互独立可并行；不动 RightDock 骨架，全部增量叠加。

### P0-A 统一「打开」管线（Rust 能力 + 全局链接接管）

**Rust 侧**（新命令放在 `commands_surface.rs`，与 `open_release_page` 同域）：
1. `Cargo.toml` 引入 `tauri-plugin-opener`（官方插件，同时清掉 shell deprecated TODO）。
2. 新命令：
   - `open_external(url: String)`——仅接受 `http`/`https`（策略常量集中在 `commands_surface.rs`，复用 `is_official_release_url` 的防攻破思路但放宽为 scheme 校验；拒绝 `file:`、自定义 scheme，杜绝渲染进程被攻破后变任意 opener）；
   - `open_with_default_app(path: String)`——仅接受规范化后的绝对路径，且限制在 asset protocol 同款 scope（`$HOME/**`、`$TEMP/**`）内；
   - `reveal_in_folder(path: String)`——同 scope 限制，Windows 走 `explorer /select`、macOS `open -R`、Linux `xdg-open <dir>`。
3. `desktop/acl/app-permissions.json` + `capabilities/app-commands.json` 同步三个 `allow-*` 权限（**ACL 覆盖测试会把门**，`task_plan.md` 踩坑记录）。

**UI 侧**：
4. `lib/` 新增 `openExternal.ts`：统一入口（mock 模式下 no-op），`invoke('open_external', …)`。
5. `main.tsx` 挂**全局 capture 阶段 click/auxclick 拦截**：`a[href^="http"]` → `preventDefault()` + `openExternal(href)`；`a[href^="mailto:"]` → 交给系统（走 opener 的 mailto 支持或直接忽略）；**纯锚点 `#…` 与应用内路由放行**（脚注回跳不能被误杀）。这一层让 Extensions/报告 Modal 等所有既有裸链接一次性修复。
6. `Markdown.tsx:ExternalLink` 与 `DocumentRenderer` 的 `a` 改为 onClick 走统一入口（保留 `rel="noopener noreferrer"` 与图标）；同时修 P0-2（相对路径一律 preventDefault，仅允许应用内锚点语义）。
7. **链接打开目标可选（已拍板：默认面板）**：统一入口升级为 `openLink(url, target)`，target 解析优先级 = **修饰键/上下文菜单覆盖 > 全局设置** `shannon.link.target`（`panel`（**默认**）｜`browser`）。交互三通道：普通点击 → **右侧面板网页 tab**；**右键上下文菜单**两项且面板优先——第一项「在右侧面板打开」、第二项「在浏览器打开」；**Alt+点击**临时反向进浏览器（tooltip 与快捷键帮助面板注明）。`panel` 路由由 P1-E 的网页 tab 承接——P0-A 先落地路由钩子与设置项，P1-E 交付前 `panel` 行为降级为系统浏览器打开（P1-E 上线前点击行为可预期，无功能损失）。

**验收**：默认设置下，聊天/文档/扩展页内 https 链接路由到面板打开钩子；右键菜单两项且「在右侧面板打开」居首；Alt+点击走浏览器；`href="foo.md"` 点击不再引发 WebView 导航；脚注锚点正常滚动；mock 模式（`pnpm demo`）不崩。
**测试**：Rust——scheme/路径 scope 白名单单测（`commands_surface.rs` 内 `#[cfg(test)]`，ACL 覆盖测试自动把关）；UI——拦截器 jsdom 单测（http/锚点/相对路径/中键 auxclick 四类）。

### P0-B 消息内文件路径高亮 + 点击

1. 新 util `lib/fileRefs.ts`：`detectFileRefs(text) -> { path, start, end }[]`。
   - 识别对象先限**内联代码**（反引号内以 `/`、`./`、`~/` 开头，或含已知源码/文档扩展名的 token）——低误报；
   - 相对路径基于会话 `working_dir` 解析为绝对路径；渲染前用轻量存在性校验（可复用 `get_working_dir_info`/file tree 缓存，或新增 `path_exists` 只读命令，scope 同 P0-A）。
2. `Markdown.tsx:InlineCode`：命中路径则渲染为 `FileRefChip`（虚线下划线 + 文件图标，hover 显示绝对路径 tooltip），点击默认行为 = 「应用内最合适的打开方式」（见下），`Shift+点击` 或右键菜单给完整动作组：应用内打开 / Reveal in folder / 系统默认程序打开。
   - 应用内路由策略：`.md/.html/.svg/.mermaid` → `useArtifact().open()` 进 RightDock（标记 `confidence: 'high'`）；代码文件 → 聊天内嵌 EditorPanel（`mod+5`/`/editor` 已有目的地，`App.tsx:100-103`）；可 diff 文件 → diff tab。
3. 工具卡扩展：`extractFilePath` 的工具集放宽（Read/Grep/Glob 从 `tool_input` 的 `path`/`pattern` 字段提取），路径渲染为同一 `FileRefChip`——写文件类工具保留 Diff 按钮，其余给「打开/Reveal」。
4. 纯文本路径的 linkify 明确放到 P2（sanitize 与误报风险，见决策点 4）。

**验收**：assistant 消息内 `` `src/foo.rs` ``（存在）高亮可点，点击在 EditorPanel 打开；`` `docs/plan.md` `` 点击在 RightDock 出现文档 tab；不存在的路径不高亮（防幻觉路径误导）。
**测试**：`fileRefs.ts` 全量单测（扩展名矩阵/相对路径/排除 URL 与 JSON 键）；MessageBubble 快照更新。

### P1-C 磁盘产物自动入 Dock

1. 捕获点放在**前端工具流**（不动 Rust）：`ToolCall` 完成事件中 `FILE_MUTATING_TOOLS` 已有 path（`MessageBubble.tsx:538-547` 同源逻辑抽到 `lib/`），新增扩展名判断（md/markdown/html/svg/mermaid）。
2. 内容读取：新增只读命令 `read_text_file(path, max_bytes)`（上限 512KB，超限/二进制返回结构化错误），或在 `commands_files.rs` 内复用 `read_attachment` 的校验骨架（PR #116 刚统一过附件校验+PDF 逻辑，直接沿用其模式）。
3. 生成 artifact 入 `ArtifactContext`（`kind` 按扩展名、`title`=文件名、新增 `origin: 'disk' | 'chat'` 字段），auto-open 沿用现有 `shannon.artifact.autoOpen` 开关（默认关，出现 chip 即可）。
4. `ArtifactDocBody` 头部为 `origin: 'disk'` 的产物加「来源文件」徽章 + Reveal/外部打开按钮（复用 P0-A 命令）。

**验收**：让 agent 写 `report.md`，Dock 自动出现文档 tab 且内容正确；写 1MB 大 md 时得到超限提示而非卡死。
**测试**：hook 层单测（扩展名→kind 映射、去重——同一路径多轮编辑更新而非新开 tab，复用 `ArtifactItem.id` 按 `path` 稳定生成）；mock 数据补 `coreMock` 用例。

### P1-D 渲染格式扩展 + 逃生门

1. **HtmlRenderer 外开按钮**：`ArtifactDocBody` 工具条加「在系统浏览器打开」——source 写入 `$TEMP/shannon-artifact-<id>.html`（`saveTextFile` 已有）→ `open_with_default_app`。不放宽 iframe CSP 本身（见决策点 3）。
2. **不支持格式 fallback**：`ArtifactDocBody` 按扩展名分流——图片（png/jpg/webp/gif/bmp/svg 文件）用 `convertFileSrc` 内嵌 `<img>`（低成本高收益）；PDF/Office/其余 → 「此格式暂不支持内嵌预览」空态卡 + 「系统默认程序打开」+「Reveal」两个按钮（即用户建议的：复杂格式改为利用操作系统能力打开）。
3. **Dock「+」去 hack**：改为 `read_text_file`（文本类）+ 上述 fallback 分流（二进制类），删除 `getFileDiff().old_content` 依赖。
4. SVG 文件与 mermaid 源文件（`.mmd`）同理入列可打开扩展名。

**验收**：导出的 html 一键系统浏览器打开；「+」打开 png 显示图片、打开 pdf 显示 fallback 卡且按钮可用。
**测试**：分流逻辑单测；临时文件写失败（无 $TEMP）降级路径。

### P1-E 链接在右侧面板打开：「网页」tab（内嵌浏览视图）

承接 P0-A 的**默认 `panel` 路由**，把外部 http(s) 页面嵌进 RightDock（对齐 Codex Desktop 的 in-app browser 形态，但用更低成本的 iframe 实现）：

1. **载体**：新增 artifact kind `web`（作为 dock 里的网页 tab，多开、可关闭，与文档 tab 同构）：`sandbox="allow-scripts allow-forms allow-popups"`（**不给** `allow-same-origin` / `allow-top-navigation`），地址栏仅展示 + 重载（复用 `LivePreview.tsx:194-198` 的 iframe 模式），工具条提供「在系统浏览器打开」「关闭」。
2. **CSP 前置变更**：csp/devCsp 补 `frame-src 'self' https: http://localhost:*`——否则面板内嵌与 LivePreview 一样被 `default-src 'self'` 回退拦截（见 §1.4 连带发现）。`http://localhost:*` 单列是为 LivePreview 保住最小授权；任意站点只放 `https:` scheme。
3. **XFO 优雅降级**：大量站点带 `X-Frame-Options: DENY/SAMEORIGIN` 或 `frame-ancestors`，iframe 会白屏/报错。加载超时 + `onLoad` 检测兜底 → 显示「该站点禁止内嵌展示」空态卡 + 「在系统浏览器打开」按钮。这是 iframe 方案的固有边界（Codex 用真 webview 子窗口绕开，成本高），第一期接受该边界；二期可加「在独立窗口打开」（`WebviewWindow` 外部 URL）作为第三个目的地。
4. **安全边界**：`frame-src` 只放 scheme 不放具体域名白名单；iframe 沙箱不授予 same-origin/top-navigation；网页内的进一步点击留在 iframe 内部导航，不逃逸主窗口；地址栏不可编辑（避免面板沦为通用浏览器后的导航滥用）。
5. **默认面板策略的风险提示（拍板时已知并接受）**：默认目的地为面板时，XFO 拒绝站点的占比直接决定默认体验——因此降级卡按一级体验打磨：一键「在浏览器打开」、明确文案；可选增强：按域名记住用户选择（同域名再次点击直达浏览器）。实施第一步先抽测常用站点（GitHub、各大文档站、搜索引擎、Notion 等）的内嵌可用率并记录数据；**若可用率 <50%，将二期「在独立窗口打开」（WebviewWindow 外部 URL）提前为面板兜底**，避免默认路径频繁撞墙。

**验收**：普通点击（默认设置）聊天内 https 链接 → dock 出现网页 tab 且页面可交互；Alt+点击 → 系统浏览器；打开带 XFO 的站点（如 github.com）显示降级卡且「浏览器打开」可用；关闭 tab 无残留；LivePreview 在生产构建恢复可用（若实测确认此前被拦）。
**测试**：CSP 变更后全量跑现有 e2e 确认未放宽过度；网页 tab 状态机（loading/loaded/xfo-fallback/timeout）单测。

### P2 细节打磨（可拆散随行）

- **P2-1**：用户消息走「受限 Markdown」——仅 `linkify`（URL 自动链接）+ 行内代码，仍过 `rehypeSanitize`，不开放标题/图片等块级语法（避免破坏用户原意）。
- **P2-2**：随 P0-A 已顺带完成（Reveal/系统打开按钮）。
- **P2-5**：附件「外部打开」从 `window.open(asset://)` 改为 `open_with_default_app`（P0-A 地基上是一行改动）。
- **P2-6**：`research_report` 死功能处置：若后端短期不接线，在 MessageBubble 挂 feature flag 下线按钮，或标注 experimental。
- **P2-7**：`ExternalLink` 删除 no-op hover 占位。

### 工作量与顺序

| 阶段 | 内容 | 估算 | 依赖 |
|------|------|------|------|
| P0-A | opener 插件 + 3 命令 + ACL + 全局拦截 | 0.5–1 人日 | 无 |
| P0-B | fileRefs + FileRefChip + 工具卡扩展 | 1–2 人日 | P0-A |
| P1-C | 磁盘产物入 Dock + read_text_file | 1–1.5 人日 | P0-A |
| P1-D | fallback 分流 + html 外开 + 「+」去 hack | 1 人日 | P0-A、P1-C |
| P1-E | 面板网页 tab + frame-src CSP + 链接目标设置 | 0.5–1 人日 | P0-A |
| P2 | 用户消息 linkify 等细项 | 0.5–1 人日 | P0-A |

---

## 5. 决策点（已全部拍板，2026-09-25 评审）

> 决策 1–5 按建议执行；决策 6 由产品改判。表中「结论」为最终决策。

| # | 决策点 | 选项 | 结论（2026-09-25） |
|---|--------|------|------|
| 1 | `open_external` 的 URL 策略 | A. 放行全部 http/https＋拒绝其它 scheme；B. 域名白名单 | **A（已确认）**。通用助手场景下白名单摩擦不可接受；最危险的向量（渲染进程被攻破后变任意 opener）主要来自 `file:`/自定义 scheme，scheme 级校验已封住；与竞品默认一致 |
| 2 | 磁盘产物入 Dock 默认开还是关 | A. chip 必显示 + auto-open 默认关；B. 默认全开（自动弹 tab） | **A（已确认）**。不打断对话流，可发现性由 chip 保证；沿用现有 `shannon.artifact.autoOpen` 开关并在设置页暴露，想要 B 的用户一键可得 |
| 3 | HTML 外部资源策略 | A. 严格 CSP + 系统浏览器逃生门；B. per-artifact「加载外部资源」开关 | **第一期 A，二期视反馈升级 B（已确认）**。P1-E 的 frame-src 变更已为 B 铺平地基，届时升级边际成本低；ChatGPT（不渲染）与 Claude（放开 CDN）之间，A 是风险可控的第一步 |
| 4 | 路径识别范围 | A. 仅内联代码 + 工具输入；B. 纯文本也 linkify | **P0 做 A（已确认）**。反引号内联代码误报率最低；B 放 P2 且必须带存在性校验兜底（不存在的路径不高亮，防幻觉误导） |
| 5 | Read/Bash 等工具路径 chip | 随 P0-B 一起做 / 最小切片跳过 | **随 P0-B 做（已确认）**。同一个 FileRefChip 组件与提取函数，边际成本 <0.5 人日；不做会让「路径可点」体验残缺一半 |
| 6 | 链接默认打开位置 | A. 默认浏览器 + 修饰键/右键菜单进面板；B. 默认面板 + 右键菜单两项且面板优先 | **B（产品改判）**：`shannon.link.target` 默认 `panel`；普通点击 → 右侧面板网页 tab；右键菜单第一项「在右侧面板打开」、第二项「在浏览器打开」；Alt+点击临时走浏览器。配套要求见 P1-E 第 5 条：降级卡按一级体验打磨 + 常用站点内嵌可用率实测，可用率 <50% 则把「独立窗口打开」提前 |

---

## 6. 竞品信息源与校准说明

- 仓库内基线：`desktop/COMPETITIVE-ANALYSIS.md`（2026-06-13，含 Claude Code Desktop / Codex Desktop / Hermes / OpenClaw / Cursor 3 / Windsurf 的产品级对照与 URL 附录）。其中 §3.4 标注的「预览窗 P0 差距」已由本仓 2026-09 的 RightDock/LivePreview/Diff UI 部分兑现；本次方案补齐的是该节未覆盖的「引用打开」与「磁盘产物」两块。
- 2026-09-25 网络检索确认：Claude artifacts 支持格式（markdown/HTML/React/SVG/代码/Mermaid、分享与发布）见 [Claude Help Center](https://support.claude.com) 与 [Codecademy 指南](https://www.codecademy.com)、[VentureBeat 2026-06 报道](https://venturebeat.com)、[Nimbalyst 2026-07](https://nimbalyst.com)；ChatGPT Canvas 侧栏形态见 [help.openai.com](https://help.openai.com) 及 2026 年多篇评测。
- 表格中未标 ⛳ 的竞品行为属公开产品常识性总结，实施前建议对 Windows/macOS 桌面客户端做一轮 30 分钟人工实测校准（本机为 Linux，无法代做）。
