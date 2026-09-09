# 计算机操控（use-computer）、浏览器控制（use-browser）与文件上传设计方案

- 日期：2026-09-06
- 分支：`feat/use-browser-computer-upload`（基于 `dev`）
- 状态：实施中
- 依据：本仓库现状审计 + 竞品调研（Claude Code / OpenAI Codex / Cursor / Manus / OpenCode / Gemini CLI）

---

## 1. 背景与问题

用户提出三个能力诉求：**use-browser（浏览器操控）**、**use-computer（电脑操控）**、**文件上传**。经全面代码审计，三项能力均未达到"可用"标准——其中 use-computer 与文件上传已有相当代码存量，但存在**关键断链**，用户实际无法使用。

## 2. 现状盘点（Gap 分析）

| 能力 | Shannon 现状 | 结论 |
|---|---|---|
| **use-computer（桌面操控）** | `ComputerUseTool`（`crates/shannon-tools/src/computer_use.rs`，1166 行）代码完整：8 个动作（screenshot/click/type/scroll/key_press/wait/mouse_move/left_click_drag）、1024×768 参考坐标系缩放、键组合解析。但：(1) feature gate `computer-use = ["xcap","enigo","image"]` **未在任何构建中启用**（workspace 根 Cargo.toml 无 features 透传），所有发布二进制中该工具只返回错误桩；(2) **截图→模型回路断裂**：工具把 base64 放在 `metadata["data"]`，而引擎 `to_tool_result_content`（`engine.rs:256-307`）只解析 `content` 中的 JSON `data` 字段（Read/AnalyzeImage 约定），解析失败回退纯文本——**模型永远看不到屏幕**；(3) `max_screenshot_width/height` 配置字段从未生效（无降采样）；(4) 权限系统无 `computer` 策略（`register_default_policies` 只有 Bash/FileEdit/FileWrite/Read/WebFetch）；(5) 缺 right/double/triple-click 变体（Anthropic schema 有） | **半成品，闭环断裂** |
| **use-browser（浏览器操控）** | 无原生浏览器工具（工具注册表无 browser_* 工具）。仅有 `browser_control_prompt`（`crates/shannon-core/src/query_engine/browser_control_prompt.rs`）：检测到 Playwright/Chrome DevTools MCP 工具名时注入浏览器操作指导提示。MCP 基础设施完备（`.mcp.json` 加载、`mcp__<server>__<tool>` 适配），但 Playwright MCP **需用户手动配置**（`configs/mcp-browser.json` 模板无任何代码引用）。`WebFetch` 是纯 HTTP GET（无 JS/DOM/截图） | **缺失（仅提示注入）** |
| **文件上传** | (1) TUI：`/image` 命令 + Ctrl+V 粘贴已端到端可用（base64 → `ContentBlock::Image` → Anthropic/OpenAI 双适配器序列化，有测试）✅；(2) `@` 文件选择器：纯文本注入，图片（二进制）只插入路径 ❌；(3) REST API：`MessageRequest { content: String }` 仅纯文本，无 multipart/附件字段 ❌；(4) 桌面端（Tauri）：附件选取/拖拽 UI 完整（25MB/10 个上限、路径沙箱校验、base64 编码），但 **base64 只存入 ChatMessage 用于显示，从未送入模型**（`QueryContext.user_message` 纯文本）❌；(5) `shannon-api-protocol` 无附件字段 ❌ | **部分可用，API/桌面端断链** |

## 3. 竞品模式（调研结论，2026-09）

### 3.1 浏览器控制：CLI 层 MCP 是行业共识

- **Claude Code**：CLI 不内嵌浏览器，官方路径是 MCP server——Playwright MCP 与 Google 官方 `chrome-devtools-mcp`（CDP 连真实 Chrome、网络检查、性能 trace、`--autoConnect` 附着已开 Chrome）。API 层有 `browser_toolset_20260302` 客户端工具集，但 CLI 仍走 MCP（来源：github.com/ChromeDevTools/chrome-devtools-mcp；platform.claude.com/docs/en/agents-and-tools/tool-use/browser-use-tool）。
- **Cursor**：唯一原生实现——内置 Browser 工具（受管安全 webview，navigate/click/type/screenshot/console/network），但官方文档明确其内部形态是"以扩展形式运行的 MCP server"；权限默认逐次审批 + Auto-Run 模式 + 域名白名单（来源：cursor.com/docs/agent/tools/browser）。
- **OpenAI Codex**：CLI 无浏览器工具（仅 config 门控的 `web_search`）；桌面端靠 ChatGPT Chrome 扩展驱动真实浏览器（来源：learn.chatgpt.com/docs/computer-use）。
- **Gemini CLI**：v0.37.0 实验性 Browser Agent，网页任务在隔离副会话中运行，避免污染主会话上下文（来源：github.com/google-gemini/gemini-cli/discussions/25064）。
- **Manus**：无自研 GUI 模型，逆向证实为 Claude Sonnet + ~29 个普通工具 + 开源 Browser Use，跑在沙箱云 VM 里（来源：the-decoder.com/manus-ai-analysis）。

**结论**：三级形态——模型 API 层原生工具集、CLI 层 MCP/扩展、桌面层原生但内部 MCP 化。Shannon 的 CLI + MCP（Playwright）路线与 Claude Code/Codex/Gemini CLI 完全同构。

### 3.2 电脑操控：截图-动作环 + 动作级审批门控

- **Anthropic**：`computer-use` API 工具族（`computer_20241022` → `computer_20251124` → `computer_toolset_20260801` 客户端工具集，17 个成员工具 + `batch` 批量动作），截图≤2000px/边、推荐 1024×768/1280×720（约 1000–1800 token/图）；官方参考实现在 Linux 容器（Xvfb+Mutter）；文档要求为高风险动作构建人工确认层（来源：platform.claude.com/docs/en/agents-and-tools/tool-use/computer-use-tool）。**Claude Code CLI 本身不内置 computer use**。
- **Claude Cowork**（桌面端操控真实桌面）：设置一次性启用开关 + 逐应用审批（金融/加密应用默认禁止）+ 会话级全屏授权 + 用户黑名单，并明示"应用间无沙箱"（来源：support.claude.com/en/articles/14128542）。
- **OpenAI**：Responses API `computer_use` 工具（`computer-use-preview` 模型驱动同一截图-动作环），内置 `pending_safety_checks` / `acknowledge_safety_checks` API 级安全确认（来源：developers.openai.com/api/docs/guides/tools-computer-use）。
- **Google**：Gemini API 亦有 Computer Use 工具 + 容器化浏览器 agent 沙箱（来源：ai.google.dev/gemini-api/docs/computer-use）。

**结论**：截图-动作环是唯一共识架构；Shannon 已按 `computer_20251124` schema 实现、方向正确。**审批不是可选项**——Anthropic/Cursor/OpenAI 全部默认逐动作确认，本次为 `computer` 注册 High risk 策略即是对齐。降采样（A2）同时命中 Anthropic 的 token 预算建议与坐标参考系契约。

### 3.3 文件上传：base64-in-JSON + 客户端降采样

- **入口四件套**：Claude Code 支持剪贴板粘贴 / 终端拖拽 / 提示中写文件路径 / `claude --attach <file>`（无头模式）；Codex CLI 有 `--image` 标志；Cursor 有粘贴/拖拽/附件按钮（来源：code.claude.com/docs/en/cli-reference；inventivehq.com Codex image input）。
- **API 层**：Anthropic Messages API 用 base64 JSON content block（png/jpeg/gif/webp，单图 5MB / 长边 8000px，建议客户端预缩至 ~1568px 长边）；OpenAI 用 data-URL image_url 或文件 ID（来源：platform.claude.com/docs/en/build-with-claude/vision）。
- **截图回流**：竞品的 computer/browser 截图都以普通图片 tool-result 回流给模型（同一 image block 管线）。

**结论**：Shannon 消息管线已支持 image block，本设计把四个入口（`/image` 已有、`@` 引用、REST API、桌面端）接到同一管线即可对齐竞品；`--attach` CLI 标志留作后续（§5）。

## 4. 设计方案

### A. use-computer 闭环修复（P0）

**A1. 修复截图→模型回路**（`crates/shannon-core/src/query_engine/engine.rs`）
- `ToolResultEntry::to_tool_result_content`：图片结果 base64 优先取 `metadata["data"]`（computer use 约定），取不到再回退解析 `content` JSON 的 `data` 字段（Read/AnalyzeImage 约定）。文本描述块同步带上尺寸信息。

**A2. 截图降采样**（`crates/shannon-tools/src/computer_use.rs`）
- `execute_screenshot`：捕获后按 `max_screenshot_width/height`（默认 1024×768，即参考分辨率）等比缩放再编码 PNG——与坐标缩放契约一致，显著降低 token 消耗。高 DPI（Retina 2x）屏幕原生分辨率截图可达 4× token，此修复同时是成本修复。

**A3. 权限策略注册**（`crates/shannon-engine/src/permissions.rs`）
- `register_default_policies` 增加 `computer` 策略：`RiskLevel::High`，描述注明"控制鼠标键盘与截屏，需逐次确认"。
- `tool_execution.rs::extract_attachments`：`image_tools` 增加 `"computer"`，截图进入 UI 附件通道。

**A4. 补齐点击变体**（对齐 Anthropic schema）
- 新增 `right_click` / `double_click` / `triple_click` / `middle_click` 动作（enigo `Button::Right/Middle` + 重复点击），schema/枚举/测试同步。

**A5. 构建启用通道**
- `shannon-cli` 与 `desktop` 增加 passthrough feature `computer-use = ["shannon-tools/computer-use"]`（默认关，避免贡献者环境强依赖 libxdo/X11 头文件）；CI 增加 feature 构建矩阵项。运行时门控交给权限系统（A3）+ `--features` 构建选择，并在工具 stub 错误信息中给出启用指引（已存在）。

### B. use-browser：托管 Playwright MCP 一键装配（P1）

**B1. `/browser` REPL 命令**（`crates/shannon-ui/src/repl/commands/` 新增 `browser.rs`）
- `/browser setup`：检测 `npx` 可用性 → 写项目 `.mcp.json`（合并已有 servers，幂等添加 `playwright` 条目：`npx @playwright/mcp@latest`）→ 提示重启/重载生效。
- `/browser status`：报告 Playwright MCP 配置状态与已注册的 `mcp__playwright__*` 工具。
- `/browser open <url>`（可选增强）：无 MCP 时提示 setup；有 MCP 时引导模型执行（由模型调用工具，命令本身只做提示引导，保持 REPL 命令薄）。
- 命令实现复用 `configs/mcp-browser.json` 的 server 定义与 `SettingsManager`/mcp_advanced 的 `.mcp.json` 合并写入路径。

**B2. 提示注入已存在**：`browser_control_prompt` 检测到 `playwright` 工具后自动注入操作指导（navigate → snapshot → interact → verify、UID 用法），B1 装配完成后即刻生效，无需改动；仅在无工具时引导用户执行 `/browser setup`（追加强化：系统提示追加一行指引，见 B3）。

**B3. 兜底引导**：当用户消息包含"浏览器/打开网页/网页截图"类意图且无任何 browser 工具时，在系统提示追加一行"当前未配置浏览器工具，可运行 /browser setup 启用"。实现为 `browser_control_prompt` 的姊妹函数 `browser_setup_hint(tool_names)`，仅提示、不注册工具，避免误导航。

### C. 文件上传：三入口接线（P1）

**C1. 引擎附件通道**（`crates/shannon-core/src/query_engine/types.rs` + `engine.rs`）
- `QueryContext` 增加 `attachments: Vec<ContentBlock>`（`#[serde(default)]` 等价物——非 Serialize，直接 `Default`；新增 `QueryContext::new` 保持现有构造点兼容， attachments 缺省为空）。
- `process_query` 组装用户消息：attachments 为空 → 现状 `MessageContent::Text`；非空 → `MessageContent::Blocks([Text(用户文本), ...attachments])`。

**C2. REST API 附件**（`crates/shannon-server/src/routes/mod.rs`）
- `MessageRequest` 增加 `attachments: Option<Vec<AttachmentIn>>`，`AttachmentIn { name: Option<String>, media_type: String, data: String(base64) }`。
- 校验：media_type 白名单（png/jpeg/gif/webp）、base64 解码上限 10 MB/附件、总数 ≤ 8；非法返回 400。图片 → `ContentBlock::Image`；其余 media_type 拒绝（后续版本再开放 PDF）。
- OpenAPI schema 注解同步。

**C3. 桌面端最后一公里**（`desktop/src/commands.rs`）
- `send_message` 已构建 `FileAttachment.base64_data`：图片类（png/jpeg/gif/webp）转为 `ContentBlock::Image` 注入 `QueryContext.attachments`；非图片保持现状（文本文件可选拼接摘要——本版仅图片，文本文件继续走显示层）。

**C4. TUI `@` 图片路由**（`crates/shannon-ui/src/repl/at_reference.rs`）
- `extract_file_content` 前增加扩展名判定：图片扩展名（png/jpg/jpeg/gif/webp/bmp）→ 不再走文本注入，改为复用 `media.rs` 的 base64 + `add_user_message_blocks` 路径（与 `/image` 一致），并更新状态栏提示。

## 5. 非目标（本版不做）

- 原生 Rust CDP/Playwright 客户端（浏览器控制不走 MCP）——工程量大，且与竞品 MCP 共识相悖。
- macOS Accessibility API 精准控件操控（已在 CLAUDE.md Future Considerations，P3）。
- REST API 文本/PDF 附件（先图片，协议字段已预留 media_type）。
- Wayland 原生输入支持（enigo xdo 后端限制，X11/macOS/Windows 优先）。
- gateway `MediaAttachment` 实现（IM 渠道媒体，另立计划）。

## 6. 测试与验收

- 单测：A1（两类 metadata 约定均产出 Image block）、A2（降采样尺寸断言，feature-gated）、A4（新动作反序列化/schema）、B1（`.mcp.json` 合并写入幂等）、B3（有/无 browser 工具的提示开关）、C1（Blocks 组装）、C2（附件校验：白名单/超限/坏 base64 → 400）。
- 回归：`just test`（nextest）全绿；`cargo check --workspace` 与 `cargo check -p shannon-tools --features computer-use` 均通过。
- 手动验收路径：`/image`（已有）→ `@screenshot.png`（新）→ 桌面附件对话（新）→ `curl -X POST /v1/sessions/:id/messages -d '{"content":"看这张图","attachments":[...]}'`（新）→ `/browser setup` 后 `mcp__playwright__browser_navigate` 可用 → （编译启用 feature 后）`computer screenshot` 模型可见屏幕。

## 7. 风险与权衡

| 风险 | 缓解 |
|---|---|
| computer-use feature 启用引入 X11 系统依赖 | 保持默认关；CI 矩阵覆盖；stub 报错信息指引 |
| 截图 base64 进入会话日志体积膨胀 | A2 降采样至参考分辨率；后续可在 tee 层截断（不阻塞本版） |
| 附件滥用（内存/日志 DoS） | C2 白名单 + 大小/数量上限；桌面端已有 25MB/10 个限制 |
| `@` 图片路由改变现有行为 | 仅扩展名命中图片时改道，文本路径回归测试保护 |
