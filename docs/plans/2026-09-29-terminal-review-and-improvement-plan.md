# Shannon Terminal 功能深度审查与改进方案（2026-09-29）

> 审查范围：产品中所有"terminal 相关"界面与能力，共三块——① 桌面端集成终端（Tauri PTY 后端 `desktop/src/terminal_commands.rs` + xterm.js 前端 `desktop/ui/src/components/terminal/`）；② CLI 的 ratatui TUI（`crates/shannon-ui/`，终端用户的默认界面）；③ 产品层（user stories、竞品对标、文档承诺 vs 实际交付）。
> 方法：三路并行深度代码审查（桌面集成面 / TUI 质量 / 产品文档考古）+ 关键发现逐条人工复核（本文所有 P0/P2 结论均已在当前 `dev` 分支代码上二次验证）。
> 本文回答四个问题：① terminal 功能现状如何；② 有哪些问题（按严重度）；③ 对照产品定位与竞品缺了什么；④ 分批改进方案与验收标准。

---

## 0. TL;DR

1. **工程质量高于预期，但存在两个会"冻死整个 CLI 界面"的 P0**：TUI 的 `!` 内联 shell 在 UI 线程同步执行、无超时无取消（`!sleep infinity` = 界面永久假死）；statusline 脚本先 `wait()` 后读 stdout，输出 >64KB 管道缓冲即经典双端死锁。两者均在 2026-09-22 综合审查中报告过、至今未修，且都在主事件循环路径上。
2. **桌面集成终端后端是教科书级的**（进程组纪律、背压截断、分块 emit、Drop 兜底、全链路测试），前端字节流保真与多行粘贴防护也很扎实。它的问题不在代码质量，而在**产品半成品状态**：与 AI 完全零集成（原始 brief 写明 "sharing environment with the agent"）、零设置项、工作区网格退役后遗留一整套死代码与孤儿持久化。
3. **最大的产品缺口是"终端与 agent 零联系"**：竞品分析把集成终端定位为 agent 的工作台（Claude Code Desktop 七面板、Hermes 子代理终端），而当前 `terminalWrite` 只有 TerminalPanel 一个调用方——agent 既不能向终端发命令，也读不到终端输出。干净隔离是安全优势，但与既定产品方向相悖，需要显式决策而不是保持现状。
4. 改进方案分 4 批：**止血（P0，1-2 天）→ 正确性与死代码清理（2-3 天）→ 体验补全（约 1 周）→ 产品差异化（2-3 周，可独立取舍）**，每批附验收标准与守卫，见 §5。

---

## 1. 现状综述：一个产品，两个 terminal 形态

### 1.1 桌面集成终端（P1-5 D，2026-09-06 合入）

- **后端**（`desktop/src/terminal_commands.rs`，1329 行）：portable-pty 长会话管理。冻结契约 `terminal_spawn/write/resize/kill/list` + `terminal:output` 事件（base64 字节保真）。≤4 并发（`MAX_TERMINALS`，:62）、16ms 合帧（:66）、2MiB 背压上限+最旧丢弃+在流截断声明（:72, 193-196）、256KiB 单事件分块（:77）、`setsid` 进程组整树 kill（:312-325）、pump 线程持 `Weak` 不钉住销毁、`Drop` 兜底。**单测覆盖是本仓库的标杆水平**（DTO 冻结形状、真实 PTY 生命周期、洪泛、退出的 15+ 用例）。
- **前端**（`TerminalPanel.tsx`，569 行 + `xtermTheme.ts`）：聊天页底部抽屉（320px，全高切换），Ctrl+` 开合（capture phase 注册，绕开全局快捷键 hook 的 textarea 守卫），动态 import xterm 保持首屏 bundle 干净，主题跟随 12 主题注册表（live CSS 变量 + 明暗 ANSI 兜底），多行粘贴二次确认，a11y 有 region/tablist 语义。
- **集成点**：唯一挂载在 `Chat.tsx:561`（抽屉变体）；会话窗口（`session-*`）复用 `/chat` 页面因此各自带一份面板。
- **历史**：原始四面板工作区网格（chat/diff/preview/terminal 可拖拽布局，brief C-2）已在 `e786ec25` 退役，只留下终端的 drawer 形态。

### 1.2 CLI TUI（`crates/shannon-ui/`，~68k 行，终端用户的主界面）

- Codex-CLI 式 inline viewport 架构：底部状态栏+多行编辑器+活动内容，已完成消息经 `insert_before()` 提交到原生终端 scrollback（`src/tui/mod.rs:175-193`）——**这是正确且少有人做好的模型**，且 resize 时对已提交 scrollback 做宽度感知 reflow（`repl/mod.rs:2015-2026`）。
- 输入侧成熟：vim 模式（Normal/Insert/Visual/Command、marks、text objects、in-buffer 搜索）、Ctrl+R 增量历史搜索、Ctrl+P 模糊命令面板、`@` 引用、Ctrl+E `$EDITOR`、大粘贴占位符化、括号粘贴退出时先于 raw mode 关闭防 escape 泄漏。
- 主题/兼容性纪律好：10 主题含色盲变体、`COLORFGBG` 明暗自检、`NO_COLOR`/`TERM=dumb` 单色、truecolor 检测、OSC 52 按 tmux/zellij 门控、unicode-width 全 widget 覆盖、char-indexed 输入缓冲（CJK/emoji 无字节切割 panic）、panic 时 `TerminalGuard` 恢复终端 + 崩溃文件 hook。
- 流式渲染有专门设计：Smooth/CatchUp 双模式背压、换行门控、syntect `(lang, hash)` 增量高亮缓存、streaming diff 最小重绘。

### 1.3 产品层定位

- 产品叙事（README:86）：Advanced 模式卖点 = "multi-panel workspace with **integrated terminal**"；目标用户包括"被终端吓住的非技术用户"（05c 竞品分析：304）。
- 竞品基线（`desktop/docs/competitive-ui-analysis.md:19,46,49`、`docs/competitive-research-2026-09.md:42`）：集成终端是 Claude Code Desktop / Cursor / Hermes 的桌面标配；Claude Code Desktop 七面板可拖拽 + 每仓库布局持久化。
- 原始 brief（`docs/improvement-plan-2026-09.md:133-141`）：集成终端应"**sharing environment with the agent**"——这是当前实现与既定方向偏差最大的一条（见 §3）。

---

## 2. 问题清单

### 2.1 P0 — 会冻结界面的阻塞点（CLI TUI）

| # | 问题 | 证据 | 影响 |
|---|------|------|------|
| P0-1 | **`!` 内联 shell 同步阻塞 UI 线程**：`std::process::Command::new("sh").arg("-c")....output()` 直接在 REPL 事件循环里执行，raw mode 下无超时、无取消键 | `crates/shannon-ui/src/repl/commands/mod.rs:174-210`（已亲验） | `!sleep infinity`、`!ssh host`、任何交互式/长命令 → 整个 TUI 永久假死，只能杀进程。2026-09-22 审查 P0 #9，至今未修 |
| P0-2 | **statusline 死锁**：先 `child.wait()` 再读 stdout——脚本输出超过管道缓冲（~64KB）时子进程写阻塞、父进程 wait 阻塞，双向死锁；且对脚本本身无超时，脚本 hang = UI 线程永久 hang。该函数由 tick 循环的 `refresh_statusline` 周期调用 | `crates/shannon-ui/src/repl/helpers.rs:330-355`（已亲验） | 用户自定义 statusline 脚本输出较大即触发；无任何自愈路径 |

### 2.2 P1 — 显著的体验/架构缺陷

| # | 问题 | 证据 | 影响 |
|---|------|------|------|
| P1-1 | 每帧 `block_on` 阻塞 UI 线程：团队协调器 + agent 面板存在时，`self.runtime.block_on(task_board.summary())` 在**每次循环迭代**（50ms tick，最高 20 次/秒）同步执行 | `crates/shannon-ui/src/repl/mod.rs:~1706-1712` | 输入延迟被 async runtime 抖动放大；流式输出高峰时掉帧感 |
| P1-2 | TUI 无 IME 预编辑支持、无 kitty keyboard protocol、无 `KeyEventKind` 过滤 | `events.rs` / `input.rs` 全文无相关处理 | CJK（本产品核心市场之一）输入完全依赖终端侧合成，无法拦截组合态；修饰键语义依赖终端默认 |

### 2.3 P2 — 正确性 / 死代码 / 可访问性

| # | 问题 | 证据 | 影响 |
|---|------|------|------|
| P2-1 | **退役网格的死代码群**：`TerminalPanel` 的 `'panel'` 变体 + "手动 DOM reparenting（React 19 portal 会重挂丢 scrollback）" hack 及其注释、embedded-only 启动对账 effect、Ctrl+` 的 embedded 豁免——全部 **0 调用方**（`e786ec25` 退役后无人传 `variant="panel"`） | `TerminalPanel.tsx:74-91, 100-101, 333-347, 350-354, 441-447`（已亲验：全仓 grep 无调用） | 维护噪音 + 误导性文档注释（描述一个不存在的 Chat 页行为）；下次重构极易误改 |
| P2-2 | **孤儿后端持久化**：`workspace_get_layout`/`workspace_set_layout` 仍是注册中的 Tauri 命令，`PANEL_KINDS` 含 `"terminal"`、默认布局嵌终端面板、`~/.shannon/desktop/workspace-layouts.json` 持续读写、测试仍在维护——而前端无任何调用方 | `desktop/src/main.rs:475-477`、`workspace_commands.rs:40, 367, 344-359`（已亲验） | 死接口继续吃攻击面与测试维护成本；`workspace.add.terminal` / `workspace.panel.terminal` 两键 ×10 locale 为死键 |
| P2-3 | 终端 tablist 不可键盘导航：无 roving tabindex/方向键切换；close 按钮在 tab 外部成为独立 tab stop、tab 与面板无 `aria-controls` 关联；xterm 未开 `screenReaderMode`；`term.focus()` 无公告抢焦点；重连提示是普通 `<p>` 非 live region；达到上限仅 `title` 反馈 | `TerminalPanel.tsx:451-484, 151-162, 236, 522-536, 490` | 键盘/读屏用户在 tab 序列中听到的是无差别按钮串，终端表面内容对读屏完全不可见 |
| P2-4 | 前端测试缺口：resize→fit→`terminal_resize` IPC 链路 0 前端测试（jsdom 无 ResizeObserver，effect 直接 no-op）；unmount/close 的 listener 退订与 dispose 路径 0 断言 | `TerminalPanel.tsx:240-255, 257-265, 397-414`；`__tests__/components/TerminalPanel.test.tsx` | 尺寸同步回归只能靠手工发现；监听器泄漏回归无守卫 |
| P2-5 | TUI 无条件 20 FPS 重绘：完全空闲也每 50ms tick 全量 `draw_frame` | `repl/mod.rs:1852-1860` | 基线 CPU/功耗常驻开销（ratatui diff 缓解但不免除）；笔记本电池场景敏感 |
| P2-6 | `NO_COLOR` 被捆绑为 reduced_motion 默认值：色彩偏好 ≠ 动效偏好，且无独立 `SHANNON_REDUCED_MOTION` | `repl/state.rs:580` | 想要无色的用户仍被强制动效，语义错位 |

### 2.4 P3 — 打磨与一致性（汇总表）

| # | 问题 | 证据 |
|---|------|------|
| P3-1 | **桌面终端零设置项**：默认 shell（后端只认 `$SHELL`→`/bin/sh`）、字号 12、scrollback 5000、抽屉 320px 全部硬编码；`shell?` 参数在冻结契约里但无任何 UI 暴露 | `terminal_commands.rs:120-128`、`TerminalPanel.tsx:49, 155-156`；`config.rs` 无 terminal 键；AdvancedSettings 无相关卡片 |
| P3-2 | **会话窗口泄漏 PTY**：`session-*` 窗口渲染 `/chat` 自带终端面板，但销毁钩子只做注册表清理；用户关掉会话窗口后其 shell 进程继续跑，只能从主窗口抽屉或退出应用时回收 | `session_window_commands.rs:203-206`、`main.rs:493-530`（已亲验） |
| P3-3 | 全局事件扇出 + 跨窗口无界控制：`AppHandle.emit` 把每个终端的原始输出广播给**所有** webview（O(窗口×字节) 拷贝）；任意窗口可 list/write/kill 任意窗口的终端 | `terminal_commands.rs:161-168` |
| P3-4 | Ctrl+` 不在快捷键帮助浮层中（该快捷键刻意绕开全局注册表，结果帮助里不可发现） | `desktop/ui/src/components/KeyboardShortcutsHelp.tsx`（grep 无 terminal/`\`` 命中，已亲验） |
| P3-5 | `decodeTerminalOutput` 的 `atob` 在 listen 回调内无 catch，一个坏 base64 载荷炸掉该次处理且无日志 | `lib/runtime/terminalEvents.ts:46-51` → `TerminalPanel.tsx:185` |
| P3-6 | 退出检测按 ASCII 子串匹配 `[shannon: process exited`——任何程序 print 这串即可伪造 "ended" 标记（低危，但标记应走独立事件而非数据流内联） | `TerminalPanel.tsx:187-191`、`terminal_commands.rs:646-654` |
| P3-7 | TUI 硬编码英文串破坏 locale 对等：`[Pasted Text #n ...]`、`[Diagnostics: ✓ ...]`、pipe 模式消息、退出会话摘要 | `repl/mod.rs:1927, ~1717` 等 |
| P3-8 | 命名碰撞误导文档检索：CHANGELOG "### Terminal UI" 章节讲的是 CLI REPL，桌面集成终端（含粘贴防护、重连、网格）**全无 CHANGELOG 条目**；README 承诺了 website 文档没承诺 | `CHANGELOG.md:1177`；`website/src/content/docs/`（无桌面终端内容） |
| P3-9 | ROADMAP 状态失真：PTY 终端仍标"genuinely-pending"，而 `crates/shannon-tools/src/pty.rs` 的 `execute_in_pty` 已部分交付（交互命令 PTY 执行，无 Gemini 式快照+内联渲染） | `ROADMAP.md:11, 433-439` |
| P3-10 | 前端每 tab 一个全局 `listen` 订阅（N 终端 = N 个全量接收+过滤的监听器）；`terminalList` 刷新合并、history-hint 关闭按钮、Ctrl+` 在 xterm 焦点内的行为均无测试 | `TerminalPanel.tsx:179` |
| P3-11 | TUI 鼠标仅滚轮滚动（无点击定位），F8 释放捕获可选中文本——能力存在但未在帮助/文档中说明 | `repl/input.rs:46-66, 230-233` |
| P3-12 | 终端 tab 是应用全局的：抽屉首次打开后不再对账，其他窗口/后续新开的终端不会出现；`list.slice(0, MAX_TERMINALS)` 在后端上限变化时会静默隐藏溢出 | `TerminalPanel.tsx:303-321, 309` |

---

## 3. User Stories 与产品层评估

按产品定位与 brief 推导核心 user stories，逐条对照实现现状：

| # | User Story | 现状 | 判定 |
|---|-----------|------|------|
| US1 | 开发者不离开应用即可运行命令、看实时输出 | 抽屉 + PTY + 16ms 合帧 + 背压截断 | ✅ 覆盖，质量高 |
| US2 | 同时管理多个终端会话（≤4） | tab 化、上限双端一致、exit 后 tab 驻留提示 | ✅ 覆盖 |
| US3 | 新手不怕终端（粘贴不误执行） | 多行粘贴确认；退出/截断均有显式在流声明 | ✅ 覆盖 |
| US4 | **agent 使用终端 / 读取终端输出**（brief: "sharing environment with the agent"） | **零集成**：`terminalWrite` 唯一调用方是面板本身；无"在终端运行"、无输出发给 agent、agent 工具不感知终端 | ❌ **未实现，与既定方向相悖** |
| US5 | 自定义 shell / 字号 / 回滚行数 / 抽屉高度 | 无任何设置面 | ❌ 缺失 |
| US6 | 重连/重开面板后历史不丢 | 显式声明 v1 无 replay buffer，仅 re-list + 警示条 | ⚠️ 有意降级，竞品（VS Code replay、Claude Code Desktop）均有 |
| US7 | 多窗口/多项目工作流互不干扰 | tab 应用全局非按项目；会话窗口关了 PTY 还在（P3-2） | ⚠️ 部分 |
| US8 | 键盘/读屏可用 | region/tablist 标签、焦点回返有；roving tabindex、screenReaderMode、live region 无 | ⚠️ 部分（P2-3） |
| US9 | CLI 内运行交互式程序（vim/htop）不冻结 | BashTool 有 `execute_in_pty` 部分交付；TUI `!` 内联 shell 反而冻结（P0-1）；ROADMAP 状态与代码互相矛盾 | ⚠️ 部分且文档失真（P3-9） |

**关键产品判断**：US4 是分水岭。当前"完全隔离"客观上是安全优点（agent 无法未经确认向用户 shell 注入），但它意味着集成终端在产品叙事里只是一个"终端模拟器"，而不是竞品所定义的"agent 工作台"。这不需要立刻建完整管道，但需要显式决策（做/不做/怎么做安全边界），而不是像现在这样由"brief 写了、代码没做"的沉默状态维持。

---

## 4. 值得保持的强项（改进时不得回退）

1. 后端 PTY 进程纪律与测试文化（`terminal_commands.rs` 测试块 ~520 行：真实 PTY 生命周期、洪泛、进程组整树、pump 退役）。
2. 字节保真传输链：base64 → 按事件不解码、直接以 `Uint8Array` 喂 xterm，让 xterm 写缓冲自行拼接跨 pump tick 切开的多字节序列（`TerminalPanel.tsx:181-186` + 专项测试 216-235）。
3. 退出/截断的"在流显式声明"模式——用户永远看得到丢失了什么。
4. TUI 的 teardown 纪律（括号粘贴先于 raw mode 关闭、`ScrollUp` 清屏、panic guard + 崩溃快照）与 resize reflow。
5. TUI 主题体系（色盲变体、NO_COLOR/TERM=dumb、truecolor 检测、COLORFGBG）与 CJK 安全（char-indexed buffer + unicode-width 全覆盖）。

---

## 5. 改进方案

> 分四批，每批独立可交付、可独立取舍。验收标准均可用现有测试设施（`just test` / vitest）+ 手工脚本验证。

### Batch 0 — 止血：消除两个界面冻结点（P0，预计 1-2 天）

**Task 0.1：`!` 内联 shell 异步化 + 超时 + 可取消**
- 文件：`crates/shannon-ui/src/repl/commands/mod.rs:174-210`。
- 方案：`std::thread::spawn` + `mpsc` 把 `Command::output()` 移出 UI 线程；REPL 状态加 `InlineShellJob { child_pid, started_at, rx }`；渲染层在 chat 中显示运行中占位（`$ cmd … running`），Esc 触发 `killpg(SIGKILL)`（子进程用 `processGroup()`/`setsid` 成为组长）；默认超时 30s（可配 `SHANNON_INLINE_SHELL_TIMEOUT`），到期同样 killpg 并在 chat 中标注超时；输出在完成后整体入 chat（不做流式，控制改动面）。
- 测试：`!echo ok` 正常；`!sleep 5` 期间 TUI 可继续输入、Esc 取消后 `sleep` 进程消失（`/proc/<pid>` 检查）；超时路径注入 1s 超时验证。
- 验收：`!sleep infinity` 不再冻结 TUI，Esc 可取消，`cargo nextest` 相关用例通过。

**Task 0.2：statusline 管道死锁修复 + 超时 + 输出上限**
- 文件：`crates/shannon-ui/src/repl/helpers.rs:330-355`。
- 方案：改用 `child.wait_with_output()` 语义（并发读尽 stdout 再 wait）；整体超时（默认 2s，statusline 是每 tick 级刷新的装饰品，超时直接跳过本轮并缓存上次结果）；stdout 读取上限 32KiB，超限截断；写 stdin 前置 payload 尺寸断言（当前 payload 为小 JSON，保持不变但加注释约束）。
- 测试：输出 200KB 的 statusline 脚本 → 正常返回截断结果不阻塞；`sleep 30` 脚本 → 2s 后放弃、UI 不卡。
- 验收：两个 P0 冻结场景在真机 tmux + 裸终端下各验证一遍。

### Batch 1 — 正确性与死代码清理（P2-1/P2-2 + P3-5/P3-6，预计 2-3 天）

**Task 1.1：删除退役网格遗留**
- `TerminalPanel.tsx`：删 `'panel'` 变体、reparenting 注释与逻辑（:74-91, 100-101, 333-347, 350-354, 441-447）、`data-terminal-variant`；简化为纯 drawer 组件。
- 后端：`workspace_get_layout`/`workspace_set_layout` 从 `main.rs:475-477` 注销，`workspace_commands.rs` 及其测试、`~/.shannon/desktop/workspace-layouts.json` 读写一并删除（git 历史可回溯；若未来重建多面板再按新 brief 重写）。
- i18n：删 `workspace.add.terminal` / `workspace.panel.terminal`（10 locale）。
- 验收：grep 无 `variant="panel"`/`workspace_get_layout` 残留；`desktop` crate 编译测试通过；UI 测试通过。

**Task 1.2：前端健壮性小修**
- `terminalEvents.ts:46`：`decodeTerminalOutput` 包 try/catch，坏载荷记 `console.warn` 并返回空字节，不让单事件炸回调。
- 退出标记改约定：`terminal:output` 增加伴随事件（或在 payload 加 `exited: true` 旁路字段——注意冻结契约，推荐**新增** `terminal:exit` 事件，保持 output 纯字节流），`TerminalPanel` 改听新事件，废弃 ASCII 子串匹配（保留一版兼容读旧流，两个版本双写一周期后删）。
- 验收：vitest 用例——坏 base64 不抛；伪造 exit 字符串不再置灰 tab；真实 exit 事件置灰。

**Task 1.3：会话窗口终端回收（P3-2）**
- `main.rs` 的 `session-*` 销毁分支（:519-527）增加：该窗口 `terminal_list` 对账 → 无其他存活窗口引用则 `terminal_kill`。简单做法：会话窗口销毁时直接 kill 该窗口面板 spawn 的终端（面板无按窗口归属，先按"窗口关闭时若主窗口已不存在则 kill_all；否则保留"实现，并在 `terminal_info` 增加 `spawned_by_window` 字段以便精确归属——字段是**新增**非破坏，符合冻结契约惯例）。
- 验收：打开会话窗口 → 开终端 → 关窗口 → `ps` 确认 shell 进程树消失。

### Batch 2 — 体验补全（P2-3/P2-4/P2-5 + P3-1/P3-4/P3-7，预计约 1 周）

**Task 2.1：终端设置面**
- 后端：`terminal_spawn` 读取统一配置（沿用 `ProviderConfigService` 同代的 config 读取路径），新增 `~/.shannon/config.toml` `[terminal]` 段：`shell`（覆盖 `$SHELL`）、`font_size`、`scrollback`、`drawer_height`；均带默认值，契约不变（shell 参数仍可显式传入覆盖配置）。
- 前端：Settings 新增 Terminal 卡片（shell/字号/回滚/抽屉高度），`TerminalPanel` 消费设置替代硬编码常量（:49, :155-156）。
- 验收：改字号立即对新 tab 生效；shell 设置为 `fish` 后新 tab `$SHELL` 正确；设置缺失时回退现默认。

**Task 2.2：可访问性补齐（P2-3）**
- tablist roving tabindex + Left/Right 切换、tab↔panel `aria-controls`/`id` 关联；`showHistoryHint` 容器加 `role="status"`；达到上限时给 aria-live 提示（替换纯 `title`，:490）；xterm 实例按设置暴露 `screenReaderMode` 开关（默认关，读屏用户开启）。
- `KeyboardShortcutsHelp.tsx` 补 Ctrl+` 条目（P3-4）。
- 验收：axe 扫描 Terminal 抽屉无新违例；纯键盘可完成 开→切换 tab→关 全流程；帮助浮层可见 Ctrl+`。

**Task 2.3：前端测试补缺口（P2-4）**
- mock `ResizeObserver` 断言 fit→`terminalResize` IPC 链路；unmount/close 时 `unsubscribed === true` 且 `term.dispose` 被调用；`terminalList` 刷新合并；Ctrl+` 在 xterm textarea 焦点内仍生效（capture phase 验证）。
- 验收：`pnpm vitest` 全绿，上述 4 组断言落地。

**Task 2.4：TUI 渲染与偏好修正（P2-5/P2-6/P3-7）**
- `repl/mod.rs`：draw_frame 前 check 脏标记（事件驱动重绘 + 低频保底刷，如 500ms），空闲零重绘。
- `repl/state.rs:580`：reduced_motion 与 NO_COLOR 解耦，新增 `SHANNON_REDUCED_MOTION` env 与 `/config appearance` 项，默认 false。
- 硬编码英文串过 `t!()`（`[Pasted Text #n]`、诊断横幅、pipe 模式、退出摘要），10 locale 补键。
- 顺带（P1-1 同文件）：`task_board.summary()` 移出每帧路径——改为每 N tick 或事件触发刷新 + `try_recv` 风格非阻塞消费。
- 验收：空闲时 `strace`/CPU profile 无周期绘制；`NO_COLOR=1` 下 spinner 仍可动、`SHANNON_REDUCED_MOTION=1` 下静止；`just test` 通过。

### Batch 3 — 产品差异化（可独立取舍，预计 2-3 周）

**Task 3.1：US4 决策与最小集成（agent ↔ 终端）**
- 先决策（本文建议：**做，带显式权限边界**）：
  - 方向 A（最小）：chat 消息的代码块/命令加"在终端运行"按钮 → 前端确认后 `terminalWrite` 到活动终端；选中文本"发给 agent"注入为附件上下文。纯前端 + 现有契约，无新攻击面。
  - 方向 B（完整）：冻结契约新增 `terminal_attach`（agent 工具可列终端、征得 High-risk 权限确认后写入、读取增量输出）。复用 `TerminalManager` 的 sink 广播为 per-terminal 订阅即可，后端改动集中在 `terminal_commands.rs`。
- 无论 A/B：权限策略对齐现有 `ApprovalMode`（写入用户 shell = High risk，逐次确认），UI 上 agent 写入的内容在终端以可辨识前缀呈现。
- 验收：A——chat 中一条 `npm test` 建议 3 次点击内在真实终端执行；B——agent 工具调用终端需权限弹窗，拒绝后 agent 收到明确错误。

**Task 3.2：按项目归属 + replay buffer（US6/US7）**
- `TerminalInfo` 增加 `project_dir` 维度的前端过滤（tab 按当前工作目录分组/过滤，替代应用全局平铺，修复 P3-12 的静默隐藏）。
- 后端 ring buffer：`TerminalManager` 每 session 保留最近 N MiB（建议 1MiB）原始字节，`terminal_list` 返回 `hasHistory`，前端 re-list/reconnect 时拉取回放进 xterm——删除"历史不可恢复"警示条（`terminal.historyWarning` i18n 键随之退役）。
- 验收：重开面板后 scrollback 恢复；4MiB 输出后重连只回放最近 1MiB 且带截断头。

**Task 3.3：文档与叙事对齐（P3-8/P3-9，0.5 天）**
- CHANGELOG 补桌面集成终端完整条目（含 09-26 修复批）；"Terminal UI" 章节改名 "CLI REPL (TUI)" 消除碰撞。
- `ROADMAP.md:11, 433-439` 更新 PTY 状态为"部分交付（交互命令 PTY 执行）；剩余：Gemini 式快照+内联渲染"。
- website 文档补桌面终端一节，对齐 README 承诺。

### 显式暂缓项（本期不做，需记录决策理由）

- **P1-2 TUI IME 预编辑 / kitty keyboard protocol**：crossterm 对 kitty 协议有 API 但各终端支持参差，IME 组合态渲染需要自绘 preedit 表面，工程量 2 周+ 且收益依赖终端侧实现质量。当前 CJK 输入经终端合成可用（char-indexed 缓冲无 panic 风险），暂缓不阻塞核心市场；触发条件：出现 CJK 输入且回热门 issue。
- **P3-3 全局事件扇出 / 跨窗口控制**：单用户桌面应用内属低危；Batch 3 的 per-terminal 订阅改造（Task 3.1 B / 3.2）落地时顺带收敛为定向 emit，不单独立项。
- **P3-11 TUI 鼠标点击定位**：滚动+选文（F8）已覆盖主要诉求；在 `/help` 补一行 F8 说明即可，其余维持现状。

### 批次依赖与排序

- Batch 0 无依赖，立即做（两个冻结点是现网风险）。
- Batch 1 依赖 Batch 0 的分支合并节奏但无代码依赖；Task 1.3 建议与 3.2 的字段新增一起设计（`spawned_by_window` 与 `project_dir` 一次加齐，避免两次碰冻结契约）。
- Batch 2 与 3.1 方向 A 可并行；3.1 方向 B 与 3.2 共享 per-terminal 订阅改造，宜连续实施。
- 全程守卫：`just dev`（check + clippy + nextest）+ `pnpm vitest`；Batch 2 后为 TerminalPanel 补 axe e2e 断言进现有全路由扫描。

---

## 6. 审查方法附注

- 三路并行探索代理（桌面集成面 45 次工具调用 / TUI 48 次 / 产品考古 61 次）+ 本文档所有 P0 与 P2-1/P2-2/P3-2/P3-4 结论由主审在 `dev`（`02214380`）上逐条人工复核。
- 复核中修正代理报告 1 处：`KeyboardShortcutsHelp.tsx` 实际路径为 `desktop/ui/src/components/`（代理误报 `shared/`），结论不变。
- 已知审查边界：gateway/web 端的 terminal 字符串命中（`engineBridge.ts` 等）为移动端桥接命名，非终端功能，未纳入；`shannon-remote` 的远程 PTY 限制（CLAUDE.md 已记录）未重复展开。
