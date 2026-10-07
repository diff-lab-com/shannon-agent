# 竞品 browser-use / computer-use 框架层对比与 Shannon 改进清单（2026-10-07）

对比对象是**框架/工具实现**（不是终端产品）：各家 coding-agent/框架如何实现"computer use"（截图+鼠标键盘的桌面控制）与"browser use"（浏览器自动化）工具。移动端产品场景见姊妹篇 `2026-10-07-mobile-remote-desktop-control-use-cases.md`。

## 1. 各实现一览

| 实现 | 实际形态 |
|---|---|
| Claude Agent SDK / Claude Code | Claude Code 本体**无内置桌面控制**；computer use 是 Messages API 工具集（`computer_20250124`/`computer_20251124`，后者加 `zoom`），API 返回动作、由宿主 harness 实现截图/输入 |
| OpenAI Codex | CLI **无** computer use（社区还在要）；Codex 桌面 App 有"后台 computer use"（读 macOS AX 树 + ScreenCaptureKit，自有光标）；API CUA（`computer_use_preview`）暴露 click/double_click/drag/key/move/screenshot/scroll/type/wait，**1024×768 参考坐标系由 harness 缩放**，带 safety-check 回调协议 |
| Gemini / Antigravity | Gemini API computer use 返回 browser/mobile/desktop 三种环境的动作，**0–999 归一化坐标 + intent 字段**；模型卡含安全策略与提示注入检测；Antigravity IDE 有原生浏览器工具（Chrome 扩展）；Gemini CLI 的浏览器代理基于 chrome-devtools-mcp |
| Open Interpreter | `--os` 模式：pyautogui + 截图喂 GPT-4V，自述"高度实验性"；现已转向 vercel-labs/agent-browser 与 trycua/cua |
| browser-use（Python 库） | Playwright/CDP + **DOM 结构化抽取 + 可选视觉**；多标签、上传下载、持久 profile、CDP attach；云版才有 stealth/CAPTCHA 解题；亦把 31 个 Claude 浏览器动作暴露为 Anthropic 工具集 |
| Playwright MCP | ~70 工具，**无障碍树快照 + ref，非像素输入**为核心；快照/输入/标签页/PDF/上传/console/网络路由/storage-state/verify-* 等；默认持久 profile，`--cdp-endpoint`/`--extension` 可附着真实 Chrome |
| chrome-devtools-mcp | ~57 工具（Puppeteer/CDP）：uid 点击/填写、性能 trace+insights、lighthouse、14 个堆快照工具、网络检查、screencast、CPU/网络/视口/移动模拟；仅 Chrome/Chrome-for-Testing |
| Microsoft OmniParser / UFO / WAA | OmniParser V2：纯视觉截图→可点元素+标注（grounding 模块）；UFO²：Windows **UIA+Win32+WinCOM 混合** GUI+API 动作、推测性多动作（LLM 调用 −51%）；WAA 基准人类 74.5% vs 最佳 agent ~25% |

## 2. 功能矩阵

图例：✅ 一等支持；⚠️ 部分/实验/需开关；❌ 无。

| 特性 | Claude (CU tools) | OpenAI Codex (app/API CUA) | Gemini/Antigravity | Open Interpreter | browser-use | Playwright MCP | chrome-devtools-mcp | OmniParser/UFO | **Shannon (本 PR 后)** |
|---|---|---|---|---|---|---|---|---|---|
| 截图 | ✅1 | ✅ ScreenCaptureKit/自供 | ⚠️2 | ✅ | ✅ | ✅ | ✅ + screencast | ✅ 唯一输入 | ✅ xcap/Wayland 双后端 |
| 多显示器 | ⚠️ display_number | ⚠️ | ⚠️ harness | ❌ | n/a | n/a | n/a | ⚠️ 活动窗口 | ✅ monitor 索引+虚拟桌面原点换算 |
| 坐标契约/HiDPI | ⚠️ 需自报 display 尺寸，文档承认 ~4% 误差 | ✅ 1024×768→真实缩放公式 | ✅ 0–999 归一化 | ❌ | n/a(CSS px) | ✅ CSS px | ✅ CSS px | ⚠️ 缩放到模型分辨率 | ✅ **最近截图实际尺寸为源空间**（4:3 假设已修），Windows per-monitor-v2 DPI |
| 点击/双击/右键/拖拽 | ✅ | ✅ | ⚠️2 | ✅ | ✅ 语义动作 | ✅ button+modifiers | ✅ | ✅ UIA | ✅ 含**拖拽插值**（HTML5 dnd 需要） |
| 滚轮 | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ mouse_wheel | ⚠️ | ✅ | ✅（browser 侧真 wheel 事件；computer 侧 enigo scroll） |
| Unicode 输入 | ✅ | ✅ | ✅ | ⚠️ 布局依赖 | ✅ | ✅ | ✅ | ✅ UIA SetText | ✅ enigo text（Linux xdo 非 ASCII 有限） |
| 组合键 | ✅ 序列+hold_key | ✅ | ⚠️2 | ✅ | ✅ | ✅ "Control+A" | ✅ | ✅ | ✅ 双侧：computer `ctrl++` 语法+严格键名；browser modifier 位 |
| 等待/稳定 | ✅ wait | ✅ | ✅ wait_5s | ⚠️ | ✅ 内建 auto-wait | ✅ wait_for(text/time) | ✅ | ⚠️ | ✅ browser_wait_for + computer wait（60s 上限防挂死） |
| 无障碍树读取 | ❌ | ✅ app 读 macOS AX | ⚠️ DOM | ❌ | ✅ web | ✅ 核心 | ✅ uid 树 | ✅ UIA | ⚠️ Windows `ui_tree`/`ui_click`（UIA）；macOS AX 为 Tier-2 骨架 |
| 语义点击（按元素） | ❌ | ⚠️ | ⚠️ | ❌ | ✅ | ✅ ref | ✅ uid | ✅ | ✅ browser ref 点击（真实鼠标事件）+ Windows ui_click |
| 剪贴板 | ❌（用 bash） | ⚠️ | ⚠️ | ⚠️ | ⚠️ | ⚠️ 权限开关 | ⚠️ | ⚠️ | ✅ Windows clipboard_read/write 工具 |
| 窗口枚举/聚焦 | ❌ | ✅ app | ⚠️ | ⚠️ | n/a | n/a | n/a | ✅ | ✅ Windows window_list/window_focus |
| CDP attach 已有浏览器 | ❌ | ❌ | ⚠️ 扩展 | ⚠️ | ✅ | ✅ 含扩展模式 | ✅ | ❌ | ✅ SHANNON_BROWSER_CDP（http/ws） |
| 持久 profile | n/a | ✅ 真实用户应用 | ⚠️ | ⚠️ | ✅ | ✅ 默认持久 | ✅ | n/a | ⚠️ 一次性 temp + `SHANNON_BROWSER_USER_DATA_DIR` 显式持久 |
| headless+headed | ✅ Xvfb | ⚠️ app 天然 headed | ⚠️ | ✅ | ✅ | ✅ | ✅ | ❌ | ✅（默认 headed，CDP attach 可接 headless） |
| 多标签 | n/a | ✅ | ✅ | ⚠️ | ✅ | ✅ | ✅ pageId | n/a | ✅ browser_tabs（open/close/list） |
| 元素快照 | ❌ | ✅ | ⚠️ | ❌ | ✅ | ✅ a11y 快照 | ✅ | ✅ | ✅ 交互元素索引（150 上限）+ `browser_text` |
| 文件上传 | ⚠️ 键盘对话框 | ✅ 原生对话框 | ⚠️ | ⚠️ | ✅ | ✅ 多文件 | ✅（须在浏览器主机本地） | ⚠️ | ✅ 新增 `browser_upload`（DataTransfer，含 hidden input，≤20MB） |
| 文件下载 | ⚠️ bash | ✅ | ⚠️ | ⚠️ | ✅ 落盘追踪 | ⚠️ 落到输出目录 | ⚠️ | n/a | ❌（后续：setDownloadBehavior） |
| Console 日志 | ❌ | n/a | n/a | ❌ | ✅ | ✅ 按级别/导航过滤 | ✅ 源映射栈 | n/a | ✅ 每标签 500 条环形缓冲 |
| 网络拦截 | ❌ | n/a | n/a | ❌ | ✅ | ✅ route/unroute | ✅ 请求体检查 | n/a | ❌（CDP 域可用，未封装） |
| 移动模拟 | n/a | n/a | ✅ mobile env | ❌ | ⚠️ | ✅ --device | ✅ | ❌ | ❌ |
| 权限/TCC 处理 | ⚠️ harness 负责 | ✅ app 引导 | ⚠️ | ❌ | n/a | n/a | n/a | ⚠️ | ✅ macOS Accessibility 硬门+失败大声报错；截图失败附屏幕录制提示 |
| Wayland 原生输入 | ❌ | ❌ | ❌ | ❌ | n/a | n/a | n/a | ❌ | ✅ libei(portal RemoteDesktop)/wayland-client 特性（编译期三选一） |
| Wayland 原生截图 | ❌ | ❌ | ❌ | ❌ | n/a | n/a | n/a | ❌ | ✅ wlr-screencopy + xdg-portal 回退 |
| Linux X11 | ✅ | ❌ app macOS 优先 | ⚠️ | ✅ | n/a | n/a | n/a | ❌ | ✅ xdo |
| Windows UIA | ❌ | ⚠️ | ⚠️ | ⚠️ | n/a | n/a | n/a | ✅ 参考实现 | ✅ ui_tree/ui_click/剪贴板/窗口 |
| 保存 PDF | ❌ | ✅ | ⚠️ | ⚠️ | ⚠️ | ✅ | ❌ | n/a | ✅ 新增 `browser_pdf` |
| JPEG 轻量截图 | n/a | n/a | n/a | n/a | ⚠️ | ⚠️ jpeg | ✅ jpeg | n/a | ✅ 新增 format=jpeg+quality |
| zoom 区域放大 | ✅（20251124） | ✅ | ⚠️ | ❌ | n/a | n/a | ❌ | ⚠️ | ✅ 新增 `zoom` 动作（原生分辨率裁剪） |
| 光标位置回报 | ✅ cursor_position | ✅ move 返回位置 | ⚠️ | ❌ | n/a | n/a | ❌ | ❌ | ✅ 新增 `cursor_position`（全局/局部/截图空间三份坐标） |

## 3. Rust 栈（enigo+xcap+chromiumoxide）普遍缺、本 PR 已补 vs 仍缺

**已补齐（本 PR）**：语义化下拉/hover/等待、组合键、真 wheel、文件上传、PDF 导出、JPEG 轻量截图、zoom、cursor_position、拖拽插值、截图空间坐标契约、严格键名（防"按错键"静默失败）、点击的 hover 布局位移防护、macOS 屏幕录制权限失败提示、wait 上限。

**仍缺（按价值排序，列为后续）**：
1. **Playwright 式 actionability auto-wait**（visible/enabled/stable/receives-events + 动作后导航稳定等待）——chromiumoxide 是薄 CDP 客户端，全靠手动；wait_for_text 是第一步。
2. **网络拦截/路由封装**（browser_route、请求体检查、storage-state 存取）。
3. **性能/观测工具**（trace、lighthouse、heap、screencast——chrome-devtools-mcp 的差异化）。
4. macOS AX Tier-2 实现（objc2；选择器已就位）与 Linux AT-SPI 读取。
5. OCR/grounding 模块（OmniParser 类）插件位。
6. 移动模拟（CDP Emulation 域，含 touch/DPR）。
7. 下载文件落盘追踪 + 回传。

## 4. 已知坑位对照（调研到的失效模式 → Shannon 现状）
- **坐标缩放错配**：Anthropic 文档承认 ~4% 误差；OpenAI 要求 harness 缩放 1024×768。→ Shannon 原固定 4:3 参考系在 16:9 屏有 33% 纵向系统偏差，已改为"最近截图实际尺寸"契约。
- **enigo Wayland/xdo**：xdotool 仅 XWayland；enigo 官方标注 Wayland 后端实验性。→ Shannon 用编译期三选一（libei/wayland-client/x11rb/xdo）+ 会话不匹配提示。
- **xcap Wayland**：portal/PipeWire 会弹用户对话框（不适合无人值守）。→ Shannon wlr-screencopy 优先，portal 兜底。
- **macOS 15 屏幕录制权限周期性重授权**、TCC 挂在宿主 App。→ 截图失败信息附提示（capture 报错路径）。
- **CDP 点击 vs a11y 点击**：CDP 点击打不过跨域 iframe/OS 对话框；uid 树 DOM 变化后失效。→ ref 快照后置提示"DOM 变了请重跑 snapshot"；点击重测防 hover 位移。
- **UIA 会挂死在无响应应用**；纯 UIA 看不见自绘/web 内容 → UFO 用视觉混合。→ Shannon ui_tree 带窗口参数+超时路径，视觉回路兜底。
- **Playwright MCP 持久 profile 只允许一个实例**（并发客户端冲突）。→ Shannon 每进程独立 temp profile 天然规避。

## 5. 来源
[Anthropic computer use](https://docs.anthropic.com/en/docs/agents-and-tools/tool-use/computer-use-tool) · [claude-agent-sdk #215](https://github.com/anthropics/claude-agent-sdk-python/issues/215) · [OpenAI CUA guide](https://platform.openai.com/docs/guides/tools-computer-use) · [mjtsai: Codex app](https://mjtsai.com/blog/2026/04/17/codex-for-almost-everything/) · [Gemini computer use](https://ai.google.dev/gemini-api/docs/computer-use) · [Antigravity](https://antigravity.google) · [Open Interpreter](https://docs.openinterpreter.com/guides/os-mode) · [browser-use](https://github.com/browser-use/browser-use) · [Playwright MCP](https://github.com/microsoft/playwright-mcp) · [chrome-devtools-mcp](https://github.com/ChromeDevTools/chrome-devtools-mcp/blob/main/docs/tool-reference.md) · [UFO](https://github.com/microsoft/UFO) · [OmniParser-v2.0](https://huggingface.co/microsoft/OmniParser-v2.0) · [enigo](https://github.com/enigo-rs/enigo) · [xcap](https://github.com/nashaofu/xcap) · [chromiumoxide](https://github.com/mattsse/chromiumoxide) · [trycua/cua](https://www.trycua.com) · [Apple 15 屏幕录制重授权](https://www.macrumors.com/2024/10/07/macos-sequoia-screen-recording-prompts/)
