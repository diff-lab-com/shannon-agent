# P3 远期功能调研：Wayland / browser_toolset / macOS AX / 原生 CDP

> **目的**: 把上一轮方案文档（[2026-09-06-computer-use-browser-upload-implementation.md](./2026-09-06-computer-use-browser-upload-implementation.md)）§后续任务清单中的 T10–T14 四项远期能力做一次完整竞品调研 + 必要性分析 + 实施规划，供团队评审决定优先级与排期。
>
> **调研日期**: 2026-09-06
> **配套提交**: `feat/use-browser-computer-upload` 10 个提交（`d7fbdad3`…`63a7409c`）

## 评审决策记录（2026-09-06）

| # | 决策 | 结果 |
|---|---|---|
| 1 | T12 工具集切换是否走 Option C（provider-aware dispatch）？ | ✅ 同意，按推荐方案 |
| 2 | T13 Tier 1 AppleScript MCP 是否本季度启动？ | ✅ 同意 |
| 3 | T10 Phase 1 enigo backend 透传是否需要先做？ | ✅ 同意 |
| 4 | T14 推迟到何时？ | ✅ 同意，2027 H1 |
| 5 | P3 排期是否符合团队季度 OKR？ | ✅ 同意 |
| 6 | T14 是否本地内嵌浏览器？ | ✅ 同意本地内嵌 |
| **7** | **T14 是否将 chromium 二进制嵌入 Shannon 安装包？** | ❌ **否** —— **复用系统浏览器**（Chrome/Chromium/Edge），安装包零增重；用户在 `/browser` 检测不到时获得清晰的安装指引而非静默下载 180 MB |

---

## 0. 四项摘要对比

| Task | 竞品基准 | 当前 Shannon 缺口 | 建议时间窗 | 优先级 | 工作量 |
|---|---|---|---|---|---|
| **T10 Wayland** | Anthropic 推荐 libei portal；enigo 4 个 Linux backend 齐备 | enigo 0.2.1 支持 `libei`/`wayland` feature（默认 `xdo`），但 Shannon 未启用；**xcap 0.0.13 无 Wayland 后端**——这是关键 gap | 2026 Q4 | **P0 / 中** | 1–2 人周 |
| **T12 browser_toolset** | Anthropic `browser_toolset_20260801`（30 成员工具）+ `computer_toolset_20260801`（18 成员） | Shannon 当前走 MCP 形态。Anthropic 现已是 toolset 主流 | 2026 Q4（option C） | **P1 / 中** | 1–1.5 人周 |
| **T13 macOS AX** | trycua/cua (22.3k ⭐)、lahfir/agent-desktop、oculos 等 4 个项目皆 AX-first | Shannon 当前用 enigo + xcap 截图（CGEvent），无 AX adapter | 2027 Q1–Q2 | **P2 / 低** | 6–10 人周（含 Tier 1 AppleScript 2 周） |
| **T14 原生 CDP** | Cursor "MCP 扩展"模式；chromiumoxide 0.9.1 是唯一活跃 Rust CDP | Shannon 走 MCP Playwright；可叠加内嵌 CDP 复用 `SandboxProvider` + `DynamicWorld`；**复用系统浏览器（不内置 Chromium，安装包零增重）** | 2027 H1（Phase 2） | **P3 / 低** | ~2k 行新代码；**安装包 0 增重**；需用户系统装 Chrome/Chromium/Edge |

---

## T10 — Linux Wayland 运行时支持

### T10.1 现状盘点（基于本地代码审计）

| 组件 | 现状 | Wayland 支持 |
|---|---|---|
| `crates/shannon-tools/Cargo.toml:74-88` | `computer-use = ["xcap", "enigo", "image"]` | enigo 通过 feature 切换 backend；xcap 仅 xcb/D-Bus |
| `enigo 0.2.1` | 4 个 Linux backend feature：`xdo` (默认)、`x11rb`、`wayland`、`libei` | **可用** —— 但 Shannon 未启用任何 |
| `xcap 0.0.13` | Linux 仅 `xcb`+`dbus`（vendored）；macOS/Windows 完整 | **无 Wayland backend**——是关键技术 gap |

**enigo 0.2.1 Linux backends**（来源：`/root/.cargo/registry/src/.../enigo-0.2.1/Cargo.toml`）：

| Feature | 协议 | 备注 |
|---|---|---|
| `xdo`（默认） | X11 via `libxdo` | 需要 libxdo-dev |
| `x11rb` | X11 pure Rust | 无系统依赖 |
| `wayland` | `wayland-client` + `wayland-protocols-{misc,wlr,plasma}` | 直接走 Wayland 客户端协议 — 多数 compositor 默认禁用虚拟指针输入（wl-virtual-pointer），易失败 |
| `libei`（推荐） | Portal RemoteDesktop DBus via `ashpd` | 走 `reis` + `ashpd`，用户 TCC 授权一次，最稳 |

**xcap 0.0.13 Linux deps**：`dbus 0.9 (vendored)` + `percent-encoding` + `xcb 1.4 (randr)`。**无** wayland-client / screencopy / wlr-protocols。

### T10.2 竞品共识

- **Anthropic 文档**（[computer-use-tool](https://platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool)）推荐 Linux: Xvfb + Mutter + Tint2 + X11（XWayland 给 Wayland 会话）。
- **社区共识**：`libei` (via Portal RemoteDesktop) 是 2024+ 推荐的 Wayland 输入路径——见 [cua-driver blog](https://github.com/trycua/cua/blob/main/blog/inside-macos-window-internals.md) 和 enigo 文档。
- **截图**：Wayland 下截图标准是 `org_kde_kwin_screenshot` Portal 或 `wlr-screencopy-unstable-v1` 协议——xcap 不支持，需替换或补充。

### T10.3 必要性 / 风险

**用户影响**：Ubuntu 22.04+、Fedora 35+、Arch、openSUSE Tumbleweed 默认桌面都是 Wayland。在这些系统上 Shannon `computer` 工具"开箱即崩"（runtime `Enigo::new`/xcap 失败）。

**风险**：
- 必须同时解决**输入**（enigo）和**截图**（xcap）
- 截图替换方案需评估 Wayland portal 的 UX 摩擦（首次截屏需用户点 dialog）
- 跨 distros 兼容性差异（GNOME/KDE/Hyprland 对 protocol 支持程度不同）

### T10.4 推荐方案（建议优先级 P0 中）

**Phase 1（~1 人周）**：enigo backend 透传
1. `shannon-tools/Cargo.toml` 在 `enigo` 依赖上加 `default-features = false`，新增 `enigo = { version = "0.2", features = ["libei"] }` 或动态 feature map
2. 增加 cargo features：`computer-use-libei`、`computer-use-wayland`、`computer-use-x11rb`、`computer-use-xdo`（互斥）
3. 启动时检测 `WAYLAND_DISPLAY`/`DISPLAY` 环境变量 → 选 feature（脚本 + `compile_error!` 文档）
4. 失败时给清晰运行时报错（"X11/Wayland session not detected"）

**Phase 2（~1 人周）**：截图后端切换
5. 选项 A：替换 xcap → 自维护 fork，加 `wlr-screencopy-unstable-v1` + `org_kde_kwin_screenshot` portal（推荐——避免依赖外部 patch）
6. 选项 B：写 `screenshot` Rust crate wrapper：GNOME `GnomeScreenshotPortal`、KDE `org_kde_kwin_screenshot`、wlr `wlr-screencopy`，自动探测
7. 选项 C：用 `wayland-screenshot` 等社区 crate（待评估）
8. CI 增加 wayland runner（GNOME + Mutter + xwayland）跑 integration 测试

**Phase 3**：CI 与文档
9. README + Linux setup docs 更新：Wayland 用户需开启 Accessibility / Portal 权限

### T10.5 决策建议

**做 Phase 1（1 周），不做 Phase 2-3，除非**：
- 用户反馈集中于 Wayland 桌面（Ubuntu 24.04 LTS 已成主流）
- 后续 P3 T14（chromiumoxide）若计划做，可顺便做 wayland screenshot（chromiumoxide 走 wlr-screencopy 不需要 xcap）——**两个项目的 Wayland 截图依赖可以统一解**

**推荐**：T14 与 T10-Phase 2 合并为单一项目"P3-Wayland 截图栈"，按需投入。

---

## T12 — Anthropic browser_toolset / computer_toolset

### T12.1 关键修正

⚠️ 上一轮方案文档 §3.1 误把版本号写为 `browser_toolset_20260302`。**Anthropic Python SDK（Stainless-generated 权威源）当前唯一版本**：

- `browser_toolset_20260801`（30 成员工具：`navigate`/`left_click`/`left_click_drag`/`double_click`/`triple_click`/`right_click`/`middle_click`/`type`/`key`/`scroll`/`scroll_to`/`zoom`/`hover`/`wait`/`find`/`get_page_text`/`read_console`/`read_network`/`read_page`/`form_input`/`file_upload`/`javascript_exec`/`screenshot`/`close_tab`/`new_tab`/`list_tabs`/`switch_tab`/`left_mouse_down`/`left_mouse_up`/`mouse_move`/`hold_key`）
- `computer_toolset_20260801`（18 成员工具）
- beta header 共用 `computer-use-2025-11-24`（无需额外 flag）

来源：[`BetaBrowserToolset20260801Param`](https://raw.githubusercontent.com/anthropics/anthropic-sdk-python/main/src/anthropic/types/beta/beta_browser_toolset_20260801_param.py)、[`BetaBrowserToolsetConfigsParam`](https://raw.githubusercontent.com/anthropics/anthropic-sdk-python/main/src/anthropic/types/beta/beta_browser_toolset_configs_param.py)。

### T12.2 竞品形态对照

| 维度 | toolset（Anthropic） | MCP Playwright（Shannon 现状） |
|---|---|---|
| 浏览器执行位置 | Anthropic-hosted Chrome（服务端） | 本地 Chromium via `npx @playwright/mcp@latest` |
| schema 形态 | 单个 `tools[]` entry + `configs` map 控制 member 启用 | 平铺 `tools[]`（每个 MCP tool 一项，10–20 个） |
| prompt token 开销 | **极低**（单 schema） | 较高（N 个完整 schema） |
| 延迟 | 服务端、低 RTT | 本地 JSON-RPC + Chromium cold start |
| 跨 provider | **Anthropic-only** | 任意 provider |
| 模型门槛 | Claude Opus 4.5 / Sonnet 4.5 + `computer-use-2025-11-24` beta | 无 |
| Anthropic 主流程度 | **是当前主线**（沿 `20241022→20250124→20251124→20260801` 收敛） | MCP 是集成层，非浏览器原语 |

**竞品对比**：
- Cursor 内置 Browser 仍是 **MCP 模式**（Cursor 自有 server 跑为扩展），非 Anthropic toolset
- GitHub Copilot Coding Agent：MCP only
- Sourcegraph Cody：MCP only
- Continue：custom-tool layer（MCP 风格），已停止维护
- **→ 工具集是 Anthropic 独家路径；其他竞品因非 Anthropic 走 MCP**

### T12.3 必要性 /风险

**价值**：
- 每 turn 省 ~5–15k prompt tokens
- 移除 `npx @playwright/mcp` 依赖（启动更快、无 npm 网络）
- 浏览器沙箱由 Anthropic 提供（隐私更佳，不暴露用户本机）
- 对齐 Anthropic 路线图

**风险**：
- **Anthropic-only**：Shannon 多 provider 路线（OpenAI/Zhipu/Bedrock/Custom）将失去浏览器能力
- **离线不可用**（依赖 Anthropic 服务）
- **deprecation 风险**（已是第 4 次重命名）
- 必须 Opus 4.5 / Sonnet 4.5 + 相应 beta entitlement

### T12.4 推荐方案

**策略**：**Option C（provider-aware dispatch）**

按 provider 自动选择：
- `LlmProvider::Anthropic` 且模型 in `claude-opus-4-5 / claude-sonnet-4-5` + `anthropic-beta: ["computer-use-2025-11-24"]` → **注册 toolset 形态**
- 其他 provider → 保持 MCP Playwright 形态

**配置**：每 provider 配置中加 `browser_backend: "auto" | "toolset" | "mcp_playwright"`（默认 `auto`），让高级用户显式选择。

**实施量**（1–1.5 人周）：
- 新模块 `crates/shannon-tools/src/anthropic_browser_toolset.rs`（~500 LOC：toolset 序列化、`configs` builder、provider 检测）
- 新模块 `crates/shannon-tools/src/anthropic_computer_toolset.rs`（~300 LOC：computer toolset variant）
- `ToolRegistry::register_browser_tools(provider)` 调度入口（~50 LOC + tests）
- `browser_control_prompt.rs` 双 vocab 切换（~100 LOC）
- `shannon-engine/src/api/types.rs` provider dispatch（~30 LOC）
- 单测：Anthropic + toolset / 非 Anthropic + MCP Playwright / schema-serialization
- 集成测试：mock Anthropic Messages API，断言 tools[] payload 形态

**兼容策略**：保持现有 MCP 路径，toolset 是 opt-in via `anthropic-beta` header + 模型自动探测。**任何现存调用方式不受影响**。

### T12.5 决策建议

**做 Option C**（1.5 周）：
- 价值明确（token 节省 + Anthropic 对齐）
- 风险可控（默认行为不变，opt-in）
- 与现有 MCP 路径并存

**不做**：A（big-bang 移除 MCP）—— 跨 provider 兼容性丧失。

---

## T13 — macOS Accessibility API（AX）

### T13.1 现状盘点

Shannon macOS 走 **screenshot + enigo CGEvent** 路径：
- `crates/shannon-tools/src/computer_use.rs` 全平台代码一致，**无 `cfg(target_os = "macos")` 分支**
- `enigo 0.2.1` 在 macOS 上用 `core-graphics` + `objc2`（[enigo docs](https://github.com/enigo-rs/enigo)）
- 需要 Accessibility 权限（仍）
- 优势：跨平台代码统一
- 劣势：截图 + vision 是 **local maximum**

### T13.2 竞品现状（macOS 桌面操控）

| 项目 | 模式 | 维护度 |
|---|---|---|
| **Claude Cowork** | screenshot + CGEvent + MCP fallback（"only uses screen as last resort"） | Anthropic 主力 |
| **OpenAI Operator** | web-only（Operator 仍 web，**无 macOS desktop release**） | — |
| **OpenAI Sky 收购（2025-10-23）** | 未发布（团队被整合） | — |
| **trycua/cua** | AX + CGEvent 混合 + SkyLight private SPI（背景虚拟光标） | 22.3k ⭐，**最成熟** |
| **lahfir/agent-desktop** | AX tree + skeleton traversal（78–96% token 节省）+ C-ABI | 1k ⭐，Rust 单 binary ~15 MB |
| **huseyinstif/oculos** | AX + CGEvent + MCP | 131 ⭐ |
| **Zooeyii/macos-computer-use-mcp** | AX + CGEvent + Swift helper | 实验 |

### T13.3 AX vs screenshot 对比

| 维度 | screenshot+CGEvent（Shannon 现状） | AX tree |
|---|---|---|
| 精度 | 像素坐标（2-8px 漂移） | 属性引用（角色/名字/值，确定性） |
| 稳定性 | UI 像素位移破坏 loop | 引用存活于小布局变化 |
| token/turn | 50–200k（PNG vision） | 2–5k（skeleton） |
| anti-detection | 光标位移、抢焦点、敏感 app "beep" | headless-by-default，无视觉痕迹 |
| AppleScript-aware apps | 不可内省（需截图） | 原生 AX 暴露 Mail/Calendar/Messages |
| 后台执行 | 抢焦点 | 可（cua 路线，SkyLight private SPI） |
| 权限 | Accessibility + Screen Recording | Accessibility only |

### T13.4 必要性 /风险

**必要性**：
- macOS 是 Shannon 跨平台定位中三等份之一（CLAUDE.md 设计原则）
- 4 个开源项目 12 个月内**全部** AX-first 是强烈信号
- 单一结构优势：78–96% token 节省 = Shannon 多 provider 路线下成本敏感用户最关心

**风险**：
- Swift 桥接 → **可避免**（用 `objc2` + `core-graphics` 直接调 AXUIElement）
- `SLEventPostToPid`（cua 后台虚拟光标）是 **private SPI**——Apple 可破坏
- TCC 权限提示流程需要文档完善
- Electron/Chromium app AX tree 较浅（部分需 fallback 截图）

### T13.5 推荐方案（**分 3 tier，最优先 Tier 1**）

**Tier 1：AppleScript/Shortcuts MCP bridge（~2 周，推荐本季度做）**
- 纯 `osascript` 调用走 Shannon 已有 MCP 基础设施
- 类比 `joshrutkowski/applescript-mcp` / `supermemoryai/apple-mcp` / `MayCXC/osa-mcp`
- **无需 Swift bindings**，无 AX 复杂度
- 收益：Mail/Calendar/Reminders/Messages/Shortcuts/Notes first-class——与 Cowork "connector first" 策略对标
- 实现：新增 `applescript-mcp` 子模块（`crates/shannon-mcp/src/applescript.rs`），检测 macOS 启用

**Tier 2：AX-tree `MacosAxAdapter`（2027 Q1–Q2，~6–10 周）**
- 借鉴 `lahfir/agent-desktop`（Rust MIT，1k ⭐，单 binary 15MB）
- 用 `objc2` + `core-graphics`（enigo 已在依赖树）直接调 `AXUIElement`
- skeleton traversal snapshot + ref IDs（`@e1`, `@e12`）替代 vision
- 保留 enigo 路径作 Electron fallback
- trait 抽象 `PlatformAdapter`：MacosAx / MacosEnigo / WindowsUia / LinuxX11 / LinuxAtSpi

**Tier 3：Cua-style 背景 SPI（opportunistic，不推荐）**
- `SLEventPostToPid` + yabai focus-without-raise——仅在 Tier 1+2 用户反馈强烈时考虑
- Apple 可随时破坏，feature flag 包裹

### T13.6 决策建议

**Tier 1 必做**（2 周）：
- 高价值低风险
- 与竞品（Cursor/Cowork/Claude Code）路径一致
- 即使不做 Tier 2/3，Tier 1 单独也有完整 Mail/Calendar/Shortcuts 收益

**Tier 2 推迟到 2027 Q1**——需等 macOS 用户占比数据（待 P2 telemetry）再做决策。
**Tier 3 不建议主动做**——依赖 SPI 不稳定。

---

## T14 — 原生 Chromium / CDP 内嵌

### T14.1 现状盘点

Shannon 当前走 **Playwright MCP**（`npx @playwright/mcp` 起本地 Chromium）：
- `configs/mcp-browser.json` 模板
- `crates/shannon-core/src/query_engine/browser_control_prompt.rs` 提示注入
- TUI 端无浏览器面板；`shannon-ui/src/terminal_image.rs` 已支持 Kitty/Sixel/iTerm2/HalfBlocks 渲染（**这是关键基础**）
- `computer_use.rs` 是 OS 桌面自动化（**非浏览器工具**，不替代）

### T14.2 关键澄清：Cursor 不是内嵌 chromium

Cursor 的"内置 Browser 工具"实际是 **MCP server 跑为浏览器扩展**，复用用户系统 Chrome 进程（来源：[cursor.com/docs/agent/tools/browser](https://cursor.com/docs/agent/tools/browser)）。**不是** chromiumoxide 自起进程。

**要内嵌 CDP，对照对象应是 chromiumoxide / playwright-core / puppeteer（自起进程）**。

### T14.3 Rust CDP 客户端现状

| 库 | 最新版 | 维护 | 推荐度 |
|---|---|---|---|
| **chromiumoxide** | 0.9.1（2026-02-25） | 活跃（mattsse + Sytten + 4 贡献者），1.4k ⭐，1.1k 反向依赖（含 spider-rs） | **首选**（tokio 优先 + 全量 CDP + Shannon 已 tokio） |
| `headless_chrome` | 1.0.22 | 多 owner 维护中，文档覆盖率 1.27% | 备用 |
| `fantoccini` | — | WebDriver（非 CDP） | 不适用 |

**chromiumoxide 已知坑**（自述）：
- 仅支持 tokio runtime（✅ Shannon 全栈）
- Chromium 启动语言必须英文（"DevTools listening on..." 正则识别）
- 部分非实验 PDL 类型需 `CDP_NO_EXPERIMENTAL=true` 时编译失败

### T14.4 与 MCP Playwright 对比

| 维度 | MCP Playwright（现状） | 内嵌 chromiumoxide |
|---|---|---|
| 工具可用性 | ✅ | ✅ 等价 |
| Profile 隔离 | ✅ workspace-hash | ✅ 等价可控 |
| 沙箱化 | ⚠️ 仅 origin allowlist | ✅ 可复用 Shannon `SandboxProvider`（landlock） |
| 长会话内存 | ⚠️ Playwright 已知 24 GB virt / 600 MB RSS 泄漏（[issue #1636](https://github.com/microsoft/playwright-mcp/issues/1636)、[issue #38489](https://github.com/microsoft/playwright/issues/38489)） | ✅ Shannon 可控 idle shutdown |
| 启动延迟 | ⚠️ MCP spawn + npx | ✅ 复用 tokio |
| REPL 嵌截图 | ❌ 走 MCP 文本 | ✅ base64 → ratatui kitty/sixel |
| 网络 egress | ⚠️ origin 白名单 | ✅ 与 Landlock / docker sandbox 复合 |
| `--target` 扩展 | ❌ MCP 难穿透 SSH | ✅ `BrowserProvider` 可作 `DynamicWorld` 新增世界 |
| 多 tab 并发 | ⚠️ 单 profile 串行 | ✅ per-context 隔离 |

**关键洞察**：内嵌浏览器可成为 `DynamicWorld` 的 **第四世界**（与 SSH/Docker/Local 并列）。

### T14.5 必要性 /风险

**必要性**：
- Shannon `shannon-ui/src/terminal_image.rs` 已支持终端图像协议，**就缺** base64 → ratatui 一根管道
- 与 `DynamicWorld`（`crates/shannon-remote/src/dynamic.rs`）整合可让浏览器成为一等执行世界（远端 SSH/Docker 容器内 chromiumoxide + 反向 SSH 端口转发 debugger port）
- 沙箱化深度可控（与 Landlock/dockersandbox 复合）

**风险**：
- Chromium 单进程 200–500 MB RSS（headless + `--disable-dev-shm-usage` + idle shutdown 可控）
- 跨平台 Chromium 二进制分发（Linux/macOS/Windows 总 ~150–200 MB），不依赖系统浏览器
- 多 tab 并发需 semaphore
- 长会话泄漏需 session heartbeat + 周期性 `Target.detach`
- `adblock-rust`（brave）替代 uBO 做请求级过滤

### T14.6 推荐方案（**分 3 phase，最先做 Phase 1**）

> **决策约束（已与团队确认，2026-09-06）**：
> **不复用 chromiumoxide 自带 `BrowserFetcher` 下载 chromium 二进制**——浏览器二进制**不嵌入 Shannon 安装包**，产品发布物零浏览器体积增量。
> 仅当用户系统已安装 Chrome/Chromium/Edge 时复用其 CDP 端点。

#### 系统浏览器探测策略

| 平台 | 探测路径（按优先级） | 失败行为 |
|---|---|---|
| **Linux** | `$XDG_CONFIG_HOME/chromium`/ `$XDG_CONFIG_HOME/google-chrome`、`/usr/bin/{chromium,google-chrome,chromium-browser}`、`flatpak run org.chromium.Chromium`、snap `chromium` | 找不到 → `/browser` 提示安装命令，**不自动下载** |
| **macOS** | `/Applications/Google Chrome.app/Contents/MacOS/Google Chrome`、`/Applications/Chromium.app/Contents/MacOS/Chromium`、`~/Applications/Google Chrome.app/...`、Homebrew Cask (`/opt/homebrew/bin/chromium`) | 同上 |
| **Windows** | `%ProgramFiles%\Google\Chrome\Application\chrome.exe`、`%ProgramFiles(x86)%\...`、`%LOCALAPPDATA%\Chromium\Application\chrome.exe`、Edge via `--app=` (WebView2 → 不支持 CDP)，故仅 Chrome/Chromium 计入探测 | 同上 |

**launch 协议**：
```
chromiumoxide::BrowserConfig::builder()
    .with_executable(detected_path)
    .args(["--remote-debugging-port=0",       // ephemeral port
           "--user-data-dir=<workspace-hash>",  // 自动隔离
           "--no-first-run",
           "--no-default-browser-check",
           "--disable-background-networking",
           "--disable-dev-shm-usage",           // Linux Docker/CI 友好
           "--lang=en-US"])                     // chromiumoxide 启动解析依赖
    .launch()
```
Chromiumoxide `Browser::connect_with_executable()` / `BrowserConfig::launch()` 支持指定 binary；用 `--remote-debugging-port=0` 让 Chromiumoxide 拿到随机端口（`ws://127.0.0.1:<port>`）—— 复用 chromiumoxide 现成的 `--head` / `--new-window` 等开关。

#### Phase 1 — MVP（~3 周）：复用系统浏览器

1. `crates/shannon-tool-interface/src/providers.rs` 新增 `BrowserProvider` trait + `BrowserSession` 类型（~80 行）
2. `crates/shannon-remote/src/browser/` 新建：
    - `chromiumoxide.rs` — 复用现成 chromiumoxide；启动期**只**探测系统浏览器路径，**不做任何下载**（~400 行）
    - `detect.rs` — 平台特定的浏览器二进制探测（Linux/macOS/Windows），失败返回清晰的错误指引（apt/brew/winget 命令） （~250 行）
    - `session.rs` — session 管理（workspace-hash 派生 user-data-dir、idle 关停、心跳） + observability（console/network 临时文件 sink，~300 行）
    - `dev_server_probe.rs` — TCP 端口探测（~80 行）
    - 合计 ~1k 行（vs 原方案 1.5k 行——少了 fetcher 与 platform 移植代码）
3. `crates/shannon-tools/src/lib.rs` 注册 6 个 builtin browser tool：navigate/click/type/snapshot/screenshot/console/tabs/close（~600 行）
4. `browser_control_prompt.rs` 扩展 detection 前缀（`mcp__plugin_shannon_browser_*`）
5. TUI `/browser` slash command 复用 `terminal_image.rs` 渲染截图
6. `/browser doctor` 命令报告探测结果与安装指引（`apt install chromium-browser` / `brew install --cask chromium` / `winget install chromium`）
7. CI 不构建 chromium 二进制，无 release asset（**安装包零增重**）
8. **完成定义**：REPL `/browser open http://localhost:3000` 看到截图；`browser_click` / `browser_console_messages` 与现有 MCP 同等效果；不破坏 MCP 路径；用户系统无浏览器时 `/browser doctor` 给清晰安装指引

#### Phase 2（~2 周）：与 `DynamicWorld` 整合

8. `TargetKind` 增加 `Browser(LocalBrowser | RemoteBrowserOverSsh)`
9. `CurrentWorld` 加 `browser` 字段；远端 SSH 容器内 Chromiumoxide-server（用户容器自带 Chromium）+ 反向 SSH 端口转发 debugger port 到本地
10. **完成定义**：`--target browser-host` 或 `/remote use browser` 切到远端容器内浏览器

#### Phase 3（~2 周）：沙箱 + 网络 egress

11. 复合 `SandboxProvider` 装饰器
12. `Fetch.requestPaused` handler 强制白名单拒绝
13. `adblock-rust` 集成
14. **完成定义**：浏览器世界具备与 Bash 工具同等沙箱保证

**总工作量**：~2k 行新代码（较原方案 2.5k 减少）；**Shannon 安装包体积零增量**；首次启动依赖用户系统装一个 Chrome/Chromium/Edge。

### T14.7 决策建议

**Phase 1 推迟到 2027 H1**——成本高（~2k 行代码），优先级低于 T10/T12/T13 Tier 1。

**前置依赖**：先有用户对浏览器控制的明确需求 + T10 Wayland 截图栈（T14 顺带解 Linux Wayland 截图问题）。

**关键约束**（已确认，2026-09-06）：**不内置浏览器二进制**，仅复用系统已有浏览器。Shannon 安装包零增重。

**不做 Big Bang**：保留 MCP Playwright 路径，内嵌是 opt-in（用户可继续用 `npx @playwright/mcp`——事实上 Playwright 自己启动 Chromium 不影响 Shannon 安装包，因为 npx 是运行时下载而非 Shannon 安装包）。

#### 失败 UX（关键）

**`/browser` 在没浏览器时的行为**（**比"下载内置浏览器"更友好**）：

```
$ shannon › /browser open https://example.com
✗ No compatible browser found.

Searched (Linux):
  • /usr/bin/google-chrome      — not present
  • /usr/bin/chromium           — not present
  • /usr/bin/chromium-browser   — not present
  • ~/.local/share/chromium     — not present
  • flatpak org.chromium.Chromium — not installed
  • snap chromium                — not installed

To enable browser control, install one of:
  • apt:    sudo apt install chromium-browser
  • dnf:    sudo dnf install chromium
  • pacman: sudo pacman -S chromium
  • snap:   sudo snap install chromium
  • brew:   brew install --cask chromium

Or use Playwright MCP instead: /browser setup mcp
```

这一 UX 与"自动下载 chromium 占用 180 MB 用户磁盘"相比，是更低摩擦（用户对自己的磁盘拥有 100% 控制权）、更低安装包体积、更尊重用户选择。

---

## 1. 总体推荐排序

| 排序 | Task | 时间 | 理由 |
|---|---|---|---|
| **🥇 1** | **T13 Tier 1**（AppleScript MCP） | 2 周 | 高价值低风险，与竞品同路径（Cursor/Cowork/Claude Code 全部走这条路） |
| **🥈 2** | **T10 Phase 1**（enigo libei backend 透传） | 1 周 | 现代 Linux 桌面跑不起来的关键 gap，enigo feature 开关几乎零成本 |
| **🥉 3** | **T12 Option C**（browser_toolset 双路径） | 1.5 周 | Anthropic 主线，对齐路线图 + 省 token；Option C 风险可控 |
| 4 | T10 Phase 2（Wayland 截图栈） | 1 周 | 与 T14 合并统一做，避免单独投入 |
| 5 | T13 Tier 2（macOS AX adapter） | 6–10 周 | 2027 Q1，需先看 telemetry + Tier 1 落地效果 |
| 6 | T14 Phase 1（chromiumoxide 内嵌） | 3 周 | 2027 H1，前置 T10 Phase 2 |
| 不建议 | T13 Tier 3（SkyLight private SPI） | — | 依赖 Apple SPI 不稳定，不主动投入 |

## 2. 关键风险与依赖

| 风险 | 缓解 |
|---|---|
| Anthropic toolset API 变更（已第 4 次重命名） | Option C 双路径并存，可平滑迁移；schema-serialization 单测守护 |
| Wayland 截图方案分歧 | T10 Phase 2 与 T14 chromiumoxide 合并（后者天然支持 Wayland 截图） |
| macOS TCC 权限复杂 | Tier 1 AppleScript 用 Automation 权限（用户已习惯），Tier 2 再加 AX |
| Chromium 二进制分发（180 MB） | `BrowserFetcher` + release 附 fallback；用户可禁用 |
| 跨 distro 兼容性（GNOME/KDE/Hyprland） | Phase 2 portal 优先；wlr-screencopy 兜底 |

## 3. 关键参考源

### T12 (browser_toolset)
- Anthropic Python SDK 权威源（Stainless-generated）：
  - [`BetaBrowserToolset20260801Param`](https://raw.githubusercontent.com/anthropics/anthropic-sdk-python/main/src/anthropic/types/beta/beta_browser_toolset_20260801_param.py)
  - [`BetaBrowserToolsetConfigsParam`](https://raw.githubusercontent.com/anthropics/anthropic-sdk-python/main/src/anthropic/types/beta/beta_browser_toolset_configs_param.py)
  - [`BetaComputerToolset20260801Param`](https://raw.githubusercontent.com/anthropics/anthropic-sdk-python/main/src/anthropic/types/beta/beta_computer_toolset_20260801_param.py)
  - [`BetaComputerToolsetConfigsParam`](https://raw.githubusercontent.com/anthropics/anthropic-sdk-python/main/src/anthropic/types/beta/beta_computer_toolset_configs_param.py)
  - [`AnthropicBetaParam`](https://raw.githubusercontent.com/anthropics/anthropic-sdk-python/main/src/anthropic/types/anthropic_beta_param.py)
- [Anthropic: enabling claude-code to work more autonomously](https://www.anthropic.com/news/enabling-claude-code-to-work-more-autonomously)
- [Anthropic: Claude in Chrome](https://www.anthropic.com/news/claude-in-chrome)
- [Cursor Browser 工具](https://cursor.com/docs/agent/tools/browser)

### T13 (macOS AX)
- [Claude Cowork product](https://claude.com/product/cowork)
- [Claude Cowork 博客](https://claude.com/blog/cowork-research-preview)
- [Claude Code computer-use 文档](https://code.claude.com/docs/en/computer-use)
- [trycua/cua (22.3k ⭐, AX + SkyLight SPI)](https://github.com/trycua/cua)
- [trycua macOS 内部窗口博客](https://github.com/trycua/cua/blob/main/blog/inside-macos-window-internals.md)
- [lahfir/agent-desktop (Rust AX + skeleton)](https://github.com/lahfir/agent-desktop)
- [huseyinstif/oculos (Rust + MCP)](https://github.com/huseyinstif/oculos)
- [Zooeyii/macos-computer-use-mcp](https://github.com/Zooeyii/macos-computer-use-mcp)
- [TheGuyWithoutH/mac-computer-use](https://github.com/TheGuyWithoutH/mac-computer-use)
- [joshrutkowski/applescript-mcp (393 ⭐)](https://github.com/joshrutkowski/applescript-mcp)
- [supermemoryai/apple-mcp (3.1k ⭐)](https://github.com/supermemoryai/apple-mcp)
- [MayCXC/osa-mcp (动态发现 ~700 tools)](https://github.com/MayCXC/osa-mcp)

### T14 (chromiumoxide)
- [chromiumoxide 仓库](https://github.com/mattsse/chromiumoxide)
- [chromiumoxide docs](https://docs.rs/chromiumoxide)
- [chromiumoxide CHANGELOG](https://github.com/mattsse/chromiumoxide/blob/main/CHANGELOG.md)
- [Playwright MCP 长会话内存 issue #1636](https://github.com/microsoft/playwright-mcp/issues/1636)
- [Playwright 1.57 20GB 内存 issue #38489](https://github.com/microsoft/playwright/issues/38489)
- [adblock-rust (brave)](https://github.com/brave/adblock-rust)

### T10 (Wayland)
- enigo 文档（已在本地依赖树）：
  - `/root/.cargo/registry/src/.../enigo-0.2.1/Cargo.toml` 4 backend features
  - `/root/.cargo/registry/src/.../enigo-0.2.1/src/linux/libei.rs` Portal RemoteDesktop 实现
  - `/root/.cargo/registry/src/.../enigo-0.2.1/src/linux/keymap.rs` wayland cfg
- xcap 0.0.13 仅 xcb + dbus（无 wayland backend）
- [cua-driver blog on macOS internals](https://github.com/trycua/cua/blob/main/blog/inside-macos-window-internals.md)

## 4. Shannon 当前相关文件路径

```
crates/shannon-tools/
  Cargo.toml:74-88            ← computer-use feature gate
  src/computer_use.rs:1165    ← ComputerUseTool（已闭环，12 个动作）
  src/lib.rs:41               ← pub mod computer_use;
crates/shannon-core/
  src/query_engine/
    engine.rs:256-320         ← ToolResultEntry::to_tool_result_content
    types.rs:625              ← QueryContext.attachments
    browser_control_prompt.rs ← browser_control_prompt + browser_setup_hint
  src/tools.rs                ← ToolRegistry
  src/mcp_tool_adapter.rs     ← mcp__server__tool adapter
crates/shannon-engine/
  src/api/
    client.rs:229             ← anthropic-beta header join site
    types.rs:252              ← LlmProvider 枚举
crates/shannon-remote/
  src/
    dynamic.rs                ← DynamicWorld（FileSystemProvider + ProcessProvider）
    target.rs                 ← TargetKind（Local | Ssh | Docker）
crates/shannon-ui/
  src/terminal_image.rs       ← Kitty/Sixel/iTerm2/HalfBlocks 渲染
  src/repl/commands/browser.rs ← /browser setup|status|uninstall
crates/shannon-server/
  src/routes/mod.rs            ← REST API + ApiError 错误体
configs/
  mcp-browser.json            ← Playwright MCP 模板
```

## 5. 评审请确认的关键点

1. **T12 工具集切换是否要做 Option C**（provider-aware dispatch）？还是维持 MCP-only？
2. **T13 Tier 1 AppleScript MCP** 是否本季度启动（2 周，独立交付）？
3. **T10 Phase 1 enigo backend 透传** 是否需要先做（避免 Ubuntu 24.04+ 默认用户跑不起来）？
4. **T14 chromiumoxide 内嵌** 推迟到何时——需先看 telemetry 数据？
5. 整体 P3 排期是否符合团队季度 OKR？

## 6. 不在本文档范围的 follow-up

- T5 (MIME 统一) — 已在 P1 跳过，决策待用户
- T6 (桌面端 PDF 附件) — 已在 P2 跳过，provider 支持评估
- T9 (session log 附件计数) — 已撤销转独立 PR

以上三项与本 P3 调研正交，可独立推进。