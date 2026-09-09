# TUI/Desktop AI 产品多模态能力调研与 Shannon 差距分析（2026-09）

**调研日期**: 2026-09-08
**调研主题**: 阅读理解（图像/PDF/OCR）+ 生成类（图像/视频/音频）的产品支持、UI 模式与原理
**调研对象**: 17 款 TUI/Desktop 类 AI 编码/办公产品 + Shannon 自身（dev @ c6-8-ce6）
**性质**: 内部 Gap 分析。基于 Shannon 代码盘点 + 竞品官方文档、CHANGELOG、用户实测。截至 2026-09-08。

---

## 0. 一句话结论

**TUI/Desktop 类 AI 产品在「理解类」能力（图像/PDF/音频→文本）上已收敛为基线**，但**「生成类」能力（图像/视频/音频输出）仍呈现"分层渗透"格局**：图像生成刚进入头部编码 Agent（Cursor 集成 Gemini 3 Pro Image Preview、GitHub Copilot 集成 DALL·E 3、ZCode 集成 CogView），视频生成仅 ChatGPT（Sora）一家内嵌、Comfy 等独立平台承担；音频生成以"语音输入听写"为标配、TTS 与音乐经 MCP/上层 Chat 集成。

**对 Shannon 的核心判断（P0）**：
1. **"理解类" Shannon 已 90% 闭环**——Vision（图像输入）、PDF 文本提取、STT（云+本地+CLI 三路径）全部实现且 UI 完整（`/image` 命令、`@`-picker 自动路由、Tauri 桌面端云/本地 STT）；**仅 OCR 与 Office 文档解析两条缝隙**。
2. **"生成类" Shannon 是 100% 空白象限**——无图像生成（无工具/无 API 端点/无 UI）、无视频生成、无 TTS 云服务、无音乐生成。这是 Shannon 面对消费向产品（ChatGPT、Manus、Comet）时最显眼的差异化缺口；面对编码向竞品（Cursor、Copilot、Cline）时则是"避免被甩开"的必要补足。
3. **战略机会**：Shannon 的 Tauri+Rust 本地优先架构、BYOK 多 provider 中立性、完整的 Voice/STT pipeline 是承接"生成类"能力的天然平台——按 BYOK 模式对接 OpenAI gpt-image、DALL·E、智谱 CogView、即梦/Seedream、Gemini Image、Sora、ElevenLabs、Suno/UDIO 等多 provider，是性价比最高的差异化路径（不需要自研模型，复用现有 provider + 统一 UI/CLI 抽象）。

**P0 优先级建议**：补 5 项即可闭环生成类——
- **G1**：图像生成（工具 + Provider 适配 + 终端 inline 预览 + 桌面 Gallery）
- **G2**：TTS 云服务（替代浏览器 Web Speech API）
- **G3**：视频生成（最小可用：链接 + 元数据存盘）
- **G4**：OCR（PDF 扫描件回退）
- **G5**：Office 文档解析（docx/xlsx/pptx，至少 docx/xlsx）

---

## 1. 调研范围与方法

### 1.1 调研对象（17 款产品）

| 类别 | 产品 |
|---|---|
| **海外旗舰** | Claude Code + Claude Code Desktop（Anthropic）、Codex CLI/Desktop（OpenAI）、GitHub Copilot、Cursor、Windsurf/Devin Desktop |
| **海外开源/工具型** | Hermes Desktop（Nous Research）、Aider、Cline、Roo Code、Kilo Code、Continue.dev、Cody（Sourcegraph） |
| **海外消费/通用** | Grok（xAI）、Perplexity Comet、Manus、Comfy（ComfyUI Desktop） |
| **中国厂商** | 腾讯 CodeBuddy/WorkBuddy、智谱 ZCode、阿里 Tongyi Lingma、月之暗面 Kimi |
| **Shannon 自身** | dev 分支代码盘点 |

### 1.2 调研方法

- **官方文档/CHANGELOG 一手**（Claude Code 6362 行 CHANGELOG、Cursor Docs、Copilot Docs、Z.ai Docs、Comfy Docs）
- **GitHub 源码与 README**（Aider、Cline、Roo Code、Kilo Code、Continue.dev、Codex CLI）
- **Shannon 自身代码盘点**：深入扫描 crates/shannon-tools、crates/shannon-engine、crates/shannon-ui、crates/shannon-server、crates/shannon-commands、crates/shannon-core、desktop/src、desktop/ui 全部多模态相关路径
- **Shannon 既有竞品文档**：docs/competitive-research-2026-09.md、docs/research/grok-bots-research-2026-09.md 作为基线

### 1.3 能力维度定义

- **理解类（Input Multimodality）**：
  - **图像理解（Vision）**：将图像作为输入，模型返回文本/代码
  - **PDF 阅读**：将 PDF 作为输入，提取/解析文本
  - **OCR**：从扫描图像/PDF 中提取文字（独立工具，非模型视觉）
  - **文档解析**：从 Office 文档（docx/xlsx/pptx）提取结构化内容
  - **语音输入（STT）**：将语音转文本
- **生成类（Output Multimodality）**：
  - **图像生成（Image Gen）**：从文本/参考图生成新图像
  - **视频生成（Video Gen）**：从文本/图像生成视频
  - **语音输出（TTS）**：从文本生成语音
  - **音乐生成（Music Gen）**：从文本生成音乐

### 1.4 图例

- ✅ 完整支持 · 🟡 部分支持 / 有缺口 · ❌ 无 / 缺失
- 输入侧「🔵蓝」、输出侧「🟠橙」用于后续表格中区分

---

## 2. 横向能力矩阵（17 款产品 + Shannon）

### 2.1 输入侧（理解类）

| 产品 | 图像理解 | PDF 阅读 | OCR | Office 解析 | 语音输入 |
|---|---|---|---|---|---|
| **Claude Code + Desktop** | ✅ 剪贴板/拖放/Read | ✅ Read 工具 | 🟡 模型 OCR（无独立工具） | ❌ | ✅ 听写（WSL 修复） |
| **Codex CLI/Desktop** | ✅ 拖放 | 🟡 经 ChatGPT 模型 | ❌ | ❌ | ❌ |
| **GitHub Copilot** | ✅ JPEG/PNG/GIF/WEBP/PDF/HEIC/HEIF | ✅ 拖放 | 🟡 模型 OCR | ❌ | 🟡 部分 IDE |
| **Cursor** | ✅ Read/拖放 | ✅ Read | 🟡 模型 OCR | ❌ | ❌ |
| **Windsurf/Devin Desktop** | ✅ @-mentions | 🟡 Cascade 间接 | ❌ | ❌ | ✅ Voice Mode |
| **Hermes Desktop** | ✅ Vision（OpenRouter 路由） | 🟡 | 🟡 模型 OCR | ❌ | 🟡 |
| **Aider** | 🟡 文件包含（无独立 UI） | 🟡 | ❌ | ❌ | ✅ `/voice` |
| **Cline** | ✅ 拖放/Ctrl+V | 🟡 | 🟡 模型 OCR | ❌ | ❌ |
| **Roo Code** | 🟡 MCP 集成 | 🟡 MCP | ❌ | ❌ | ❌ |
| **Kilo Code** | ✅ 多模态模型路由 | 🟡 | 🟡 | ❌ | ❌ |
| **Continue.dev** | ✅ 拖放/粘贴 | 🟡 | 🟡 | ❌ | ❌ |
| **Cody (Sourcegraph)** | 🟡 模型层 | 🟡 | ❌ | ❌ | ❌ |
| **Grok (xAI)** | ✅ 图像理解 | ✅ URL/PDF | 🟡 | ❌ | ✅ Voice Mode |
| **Perplexity Comet** | ✅ 拖放 | ✅ | 🟡 | ❌ | ✅ Voice |
| **Manus** | ✅ 多模态 | ✅ | 🟡 | 🟡 间接 | 🟡 |
| **Comfy (ComfyUI)** | ✅ 节点工作流 | ❌ | ❌ | ❌ | ❌ |
| **腾讯 CodeBuddy/WorkBuddy** | ✅ 多模态 | 🟡 | 🟡 | 🟡 | 🟡 |
| **智谱 ZCode** | ✅ **GLM-5V-Turbo**（原生多模态） | ✅ | 🟡 | ❌ | ❌ |
| **阿里 Tongyi Lingma** | ✅ Qwen-VL | 🟡 | 🟡 | ❌ | ❌ |
| **Shannon (dev)** | ✅ 完整（@picker/`/image`/HTTP 附件/Computer Use 截图） | 🟡 仅文本层（`pdftotext`） | ❌ 无独立 OCR（扫描 PDF 报错） | ❌ 无原生 crate | ✅ 三路径（CLI whisper / 桌面云 STT / 桌面本地 whisper-rs） |

### 2.2 输出侧（生成类）

| 产品 | 图像生成 | 视频生成 | TTS（云） | 音乐生成 |
|---|---|---|---|---|
| **Claude Code + Desktop** | ❌（MCP 可扩展） | ❌ | ❌ | ❌ |
| **Codex CLI/Desktop** | 🟡 经 ChatGPT gpt-image | 🟡 经 ChatGPT Sora | 🟡 经 ChatGPT 4o-audio | ❌ |
| **GitHub Copilot** | ✅ DALL·E 3 / GPT-image | ❌ | ❌ | ❌ |
| **Cursor** | ✅ **Gemini 3 Pro Image Preview**（$0.134/1K 图） | ❌ | ❌ | ❌ |
| **Windsurf/Devin Desktop** | ❌ | ❌ | ❌ | ❌ |
| **Hermes Desktop** | ✅ OpenRouter 集成 | 🟡 OpenRouter 间接 | ✅ TTS | ❌ |
| **Aider** | ❌ | ❌ | ❌ | ❌ |
| **Cline** | ❌（MCP/SDK 注册） | ❌ | ❌ | ❌ |
| **Roo Code** | ❌ | ❌ | ❌ | ❌ |
| **Kilo Code** | 🟡 Kilo Gateway 路由 | ❌ | ❌ | ❌ |
| **Continue.dev** | 🟡 自定义 provider | ❌ | ❌ | ❌ |
| **Cody (Sourcegraph)** | ❌ | ❌ | ❌ | ❌ |
| **Grok (xAI)** | ✅ **Aurora** | 🟡 实验性 | ✅ Voice Mode | ❌ |
| **Perplexity Comet** | 🟡 集成 model | ❌ | 🟡 | ❌ |
| **Manus** | ✅ 集成 | 🟡 有限 | 🟡 | 🟡 |
| **Comfy (ComfyUI)** | ✅ **MiniMax H3** / Seedance / LTX 2.5 / Wan Animate 2 | ✅（同节点生态） | ✅（H3 原生立体声） | ✅（同节点生态） |
| **腾讯 CodeBuddy/WorkBuddy** | ✅ 混元 Hunyuan-Image | ✅ 混元 HunyuanVideo | ✅ 混元 TTS | 🟡 |
| **智谱 ZCode** | ✅ **CogView-3/4** | ✅ **CogVideoX** | ✅ **GLM-TTS** | 🟡 |
| **阿里 Tongyi Lingma** | 🟡 Qwen-Image | 🟡 Qwen-Video | 🟡 Qwen-TTS | ❌ |
| **Shannon (dev)** | ❌ **完全缺失** | ❌ **完全缺失** | 🟡 仅浏览器 Web Speech API（无云 TTS） | ❌ **完全缺失** |

### 2.3 关键观察

1. **输入侧已收敛为基线**：17 款中 16 款支持图像理解，14 款支持 PDF 阅读，OCR 普遍依赖"模型视觉"而非独立工具。
2. **输出侧是分层竞争**——图像生成在头部编码 Agent 中普及（Cursor、Copilot），视频生成仍主要在 ChatGPT/Comfy 平台，音乐生成几乎只有 Comfy 一家。
3. **ZCode（智谱）是国内唯一"输入+输出全模态"自研闭环产品**：GLM-5V-Turbo（图像理解）+ CogView（图像生成）+ CogVideoX（视频生成）+ GLM-TTS（语音合成）全套自研，且通过 CLI/Web IDE 集成到编码工作流。
4. **Comfy 是唯一"全生成"专业平台**：MiniMax H3 / Seedance / LTX 2.5 / Wan Animate 2 等多模型即插即用，但定位是"视觉 AI 工程师"而非"编码 Agent"，与编码工作流隔离。
5. **Shannon 的"理解类"已 90% 闭环，差距在 OCR 与 Office 解析；"生成类"100% 空白，是最大缺口**。

---

## 3. 竞品逐项深析（按产品）

### 3.1 Claude Code + Claude Code Desktop（Anthropic）

**输入侧**：
- **图像粘贴**：TUI 内联 `[Image #N]` 占位符；CHANGELOG 2.0.122 "Pasted and clipboard images are read without blocking the event loop"；macOS PowerShell 备用通道。
- **拖放**：WSL2 在 Windows 11 上支持从 Windows Explorer 拖放；Linux 用 xclip/wl-paste，失败回退 PowerShell；多图只插入最后一张、Windows Alt+V 不识别剪贴板截图等已修复。
- **Read 工具**：自动降采样到 2000px（CHANGELOG 3007 修复最大尺寸、2966 修复大图断会话、3615 改进粘贴图像压缩到与 Read 相同预算）；零字节/损坏图像 → 文本占位符（不崩溃）；MCP 返回 SVG 等不支持 MIME → 保存到磁盘 + 工具结果引用；超 2000px 历史剥离 + 重试。
- **Remote Control 移动端**：自动包含文件路径，Claude 直接读；App 拍照直接发（不再走 Read）。
- **OCR**：模型层（Claude Sonnet/Opus）原生 OCR，但 CLI 无独立 `/ocr` slash。

**输出侧**：
- ❌ 无内置图像/视频/音频生成；通过 MCP 扩展。

**UI/原理**：
- 触发：剪贴板 Ctrl+V/Alt+V、拖放、Read 工具自动调用、`@-mention`、slash commands（如 `/add-dir`、`/review`、`/goal`、`/rewind`、`/schedule`、`/teleport`）。
- Remote Control 跨设备同步：Web/Mobile/Desktop 共享会话。

**价格**：Pro $20/月起；Max $100–$200/月高用量；API 按 token（Sonnet 5 输入 $3 / 输出 $15 per M tokens 级别，Opus 更贵）；第三方 provider 经 Bedrock/Foundry。

**对 Shannon**：Claude 的 Read 工具降采样 + 错误容忍 + 跨设备附件持久化是 Shannon 可直接借鉴的工程模式；Claude 主动"不内置生成"是刻意产品定位——Anthropic 把编码 Agent 定位为"读+写"，把"生成"留给 MCP 生态。Shannon 在 BYOK 哲学下可选不同路线（自接 provider 或留白）。

### 3.2 OpenAI Codex CLI / Desktop / ChatGPT Desktop

**输入侧**：Codex CLI 经本地文件系统读图（GPT-5/Codex 多模态）；Codex Desktop 取代 ChatGPT Desktop 直接集成 ChatGPT 账户；VS Code/Cursor/Windsurf 扩展支持 `@<file>`/拖放。

**输出侧**：
- 图像生成：底层 GPT-5/gpt-image-1 可生成（ChatGPT 包含额度）。
- 视频生成：**Sora 集成在 ChatGPT Desktop**，Plus/Pro 层级有月度额度，Codex Desktop 与 ChatGPT 共享账户间接可用。
- 音频：ChatGPT 内置 TTS（GPT-4o 音频模型 6 种语音）+ Realtime Voice Mode。

**UI/原理**：`codex --prompt`/`codex app`/`codex --bg`；`codex mcp` 管理 MCP；`Sign in with ChatGPT` 或 `OPENAI_API_KEY`。

**价格**：Plus $20/月（含 Codex、图像、有限 Sora）；Pro $200/月（无限 Codex + 高 Sora）；Business $25/月/席。

**对 Shannon**：Sora 集成是 ChatGPT 独占优势；Shannon 可经 OpenAI API key 集成（Sora API 如已开放）作为 P3 远期。TTS 集成（GPT-4o audio）是 Shannon G2 P0 候选方案。

### 3.3 GitHub Copilot

**输入侧**：最完整的 IDE 端多模态文件支持——JPEG/PNG/GIF/WEBP/PDF/HEIC/HEIF；VS Code/Visual Studio/JetBrains/Xcode/Eclipse 全部支持；具体触发：复制粘贴、拖放、右键 Copilot → Add File to Chat、Visual Studio paperclip 图标 → Upload Image（多张）；PDF 直接拖放；所有计划默认启用无需策略开关。

**输出侧**：
- 图像生成：✅ DALL·E 3 / GPT-image（slash 命令或提示触发）。
- 视频：❌ 不支持。
- TTS/音乐：❌。

**UI/原理**：VS Code Chat 侧边栏；上下文菜单；剪贴板；拖放；paperclip 图标；@-mentions；slash commands（`/explain`、`/fix`、`/tests`、`/new`、`/help`）。

**价格**：Free $0；Pro $10/月；Business $19/月/席；Enterprise $39/月/席。

**对 Shannon**：Copilot 的"全 IDE 全文件类型"是输入侧的黄金标准；图像生成通过 DALL·E 3 是 Shannon G1 P0 候选方案的 reference 体验。

### 3.4 Cursor

**输入侧**：Read files（PNG/JPG/GIF/WEBP/SVG）；@-mentions（file/directory/function/web/git/doc/codebase）；Browser tool（截图、视觉验证、DOM 检查）；Web tool（搜索）；Search files/folders。

**输出侧**：
- 图像生成：✅ **Image generation – create images from text/reference images; saved to `assets/`**；Provider 模型 **Gemini 3 Pro Image Preview**（图像输出 $120/M tokens，约 $0.134 per 1K/2K image，约 $0.24 per 4K）。
- 视频：❌。
- TTS/音乐：❌。

**UI/原理**：Cmd+I 侧边栏 Agent；@-mention；/goal（持久目标）；/loop；Checkpoints（自动快照还原只回滚文件不影响消息）；Queued Messages（Enter 排队、Cmd+Enter 中途重定向）；Steering（下一个工具调用边界投递消息不中断）；Custom Mode（playbook）；Max Mode（+20% 上下文加价）。

**价格（2026）**：Pro $20/月、Pro Plus $60/月、Ultra $200/月、Start（India）₹649/月；包含无限 Tab + Agent 用量 + Bugbot + Cloud Agents；Cursor Token Rate $0.25/M（Teams/Enterprise 第三方模型）；Cursor 自家模型 Grok 4.6/Grok 4.5/Composer 2.5。

**对 Shannon**：Cursor 的图像生成（G3 Pro Image Preview）是"编码 Agent 内生成"的最佳范例——价格、产物存盘路径、`assets/` 目录约定值得 Shannon 直接借鉴。

### 3.5 Windsurf / Devin Desktop

**输入侧**：@-mentions（`@diff` 取 git diff）；Persistent Context（Custom Chat Instructions/Pinned Contexts/Active Document/Local Indexes）；Voice input（Voice Mode 听写 + "Continue" 恢复）；Linter 自动修复；Send to Cascade；@mention previous conversations。

**输出侧**：❌ 无生成工具。

**UI/原理**：Cmd/Ctrl+L 打开 Cascade；多 IDE（VS Code/JetBrains/Eclipse/Xcode/Visual Studio）；Cascade Code/Chat 模式；Planning + Todo lists；Auto-Continue；Simultaneous Cascades（worktree 隔离）。

**价格**：Free；Pro $15/月；Teams $30/月/席。

**对 Shannon**：Windsurf 把语音听写做成"Voice Mode"产品概念而非工具，值得 Shannon 借鉴产品包装。

### 3.6 Hermes Desktop（Nous Research）

**输入侧**：Vision（OpenRouter 路由 GPT-4o/Claude 3.5/Gemini）；multi-model reasoning；Web search + 浏览器自动化 + vision + 图像生成 + TTS + 多模型推理（"全栈"）。

**输出侧**：
- 图像生成：✅ OpenRouter/Replicate/Stability 集成。
- 视频：🟡 间接（OpenRouter 路由）。
- TTS：✅。
- 音乐：❌。

**UI/原理**：桌面（macOS/Win/Linux）+ CLI（`npm i -g hermes-agent`）；多渠道（Telegram/Discord/Slack/WhatsApp/Signal/Email/CLI）；沙箱后端（local/Docker/SSH/Singularity/Modal/Daytona/Vercel Sandbox）；子代理隔离；Skills 自生成；Cron 自然语言调度。

**价格**：Free/Plus/Super/Ultra（Nous Portal）；300+ 模型访问 + 10% 积分奖励。

**对 Shannon**：Hermes 的"OpenRouter 路由多模态"是 Shannon G1/G2 P0 候选方案——经 OpenRouter/Anthropic/OpenAI 兼容端点接入多 provider，按 BYOK 设计中立。

### 3.7 腾讯 CodeBuddy / WorkBuddy

**输入侧**：图像视觉/PDF/多模态（关键词覆盖；具体细节官方未公开完整）。
**输出侧**：图像生成（混元 Hunyuan-Image）/ 视频（混元 HunyuanVideo）/ TTS（混元 TTS）/ 音频（关键词覆盖）。

**UI/原理**：完整 IDE（VS Code + JetBrains 插件 + 腾讯 Lingma-like IDE）+ CLI；企业版 + 个人版；MCP 接入。

**价格**：个人基础免费/Pro 付费；企业版按席位订阅；具体未公开完整。

**对 Shannon**：腾讯系的"全栈多模态"是国产对标——Shannon 在中国市场可经腾讯云 API 接入作为替代 provider 选项。

### 3.8 智谱 ZCode（Z.ai）

**输入侧**：
- **GLM-5.2**：Coding SOTA、1M 无损上下文、长程任务执行更稳定。
- **GLM-5V-Turbo**：原生融合视觉与文本，针对视觉编程与龙虾类 Agent 任务专项优化。
- **CogAgent-9B**：基于 GLM-4V-9B，屏幕截图作为输入即可操作 GUI。
- **CodeGeeX**：与 Intel 合作的设备端智能编程助手。

**输出侧**：
- 图像生成：✅ CogView-3/4（BigModel 平台 API）。
- 视频生成：✅ CogVideoX（文本/图像到视频/视频续写）。
- TTS：✅ GLM-TTS / GLM-ASR（语音合成/识别/声音克隆）。

**UI/原理**：Z.ai 控制台（https://bigmodel.cn/console/zcode）Web IDE + CLI；模型 API HTTP/Python/Java/OpenAI 兼容 SDK/LangChain；IDE 插件 VS Code/JetBrains。

**价格**：按 token 计费；GLM-4.5/4.6 输入 ~¥0.8/M tokens；CogView/CogVideoX 按图像/秒数。

**对 Shannon**：ZCode 是国内唯一"输入+输出全模态自研闭环"产品，且与 Shannon 哲学兼容（GLM Coding Plan + Shannon 多 provider 中立）——Shannon 用户经 Anthropic/OpenAI 兼容端点可用同一份 Coding Plan。Shannon G1/G2/G3 的"国产替代 provider"候选。

### 3.9 Aider

**输入侧**：支持多模态模型（Claude 3.5 Sonnet、GPT-4o、Gemini 2.5 Pro）；repo map 自动构建代码库上下文。

**输出侧**：❌ 无生成工具。

**UI/原理**：TUI `aider <files>` → `aider >` 提示符；slash commands（`/model`、`/undo`、`/add`、`/paste`、`/voice`、`/web` 等 20+）；watch-files 自动 commit；`/voice` 听写。

**价格**：开源 Apache 2.0；按底层 LLM token 计费。

**对 Shannon**：Aider 是 CLI 类编码 Agent 的极简范本——`/paste` 图像粘贴、`/voice` 听写是 Shannon 既有模式的同行印证。

### 3.10 Cline / Roo Code / Kilo Code

**Cline 输入侧**：拖放图像、Ctrl+V 粘贴、@-mentions；多模态模型（Claude/GPT/Gemini/OpenRouter/Vercel AI Gateway/AWS Bedrock/Azure/GCP Vertex/Cerebras/Groq/Ollama/LM Studio/任何 OpenAI 兼容 API）；Subagents（只读并发研究）。

**Cline 输出侧**：❌ 不原生支持；通过 MCP/SDK 注册自定义工具（deployTool、imageGenTool）。

**Cline UI/原理**：Plan/Act 模式切换；VS Code/JetBrains/CLI/Kanban board/SDK；Auto-approve；Checkpoints；`.clinerules`；Skills；Multi-Agent Teams（`cline --team-name`）；Scheduled Agents。

**Cline 价格**：开源 + 按 token。

**Roo Code**：VS Code 扩展（marketplace 安装 574.1k；5月15日归档只读）；核心特性：Concurrent File Reads、Code Actions、Diagnostics Integration、Codebase Indexing、Enhance Prompt、Suggested Responses、Custom Modes、API Profiles、Skills、`.rooignore`、MCP、Shell Integration、Marketplace、Auto-Approving Actions、Intelligent Context Condensing、Custom Tools、Concurrent File Edits（experimental）。

**Kilo Code**：重写于 Kilo CLI，500+ 模型经 Kilo Gateway；VS Code 扩展 + CLI（`npm install -g @kilocode/cli`）；集成 KiloClaw（云端自动化）。

**对 Shannon**：三家共同模式是"**多 provider 路由 + MCP/SDK 扩展生成能力**"——Shannon 哲学一致。

### 3.11 Perplexity Comet / Manus / Comfy（消费向）

**Perplexity Comet**：浏览器助理，多模态输入（图像/PDF 拖放）；搜索 + 浏览代理；Pro Search 深度多步搜索；Comet Voice Mode；图像生成经集成 model；Free/Pro $20/Max $200+/月。

**Manus**：通用 AI Agent，多模态输入（图像/PDF/文档/网页）；Manus 1.5+ 加入扩展媒体能力；Web 优先 + Computer Use；Free/Pro $39/Team/Enterprise。

**Comfy (ComfyUI)**：**专业视觉生成平台**，不是编码 Agent：
- 模型生态：MiniMax H3（全多模态 I/O，原生立体声，5-15s，2K，条件在输入音频而非覆盖）、Seedance 2.5（字节，电影级多镜头 + 原生音频）、LTX 2.5（开源带音频 Diffusion）、Wan Animate 2（参考视频驱动角色）。
- 60,000+ 节点与数千社区工作流。
- App Mode + 节点画布双形态。
- 商业授权：MiniMax 商业许可独家经销商。

**对 Shannon**：Comfy 是"编码 Agent + 视觉生成"集成模式的 reference architecture——Shannon 可借鉴"节点工作流 + 多模型路由"思路到 MCP/Plugins 框架。

### 3.12 其他（Groq、Continue.dev、Cody、Tongyi Lingma、Kimi 等）

简表见 §2 矩阵，详情略。

---

## 4. 深度思考：UI 模式与原理分类

### 4.1 输入类 UI 模式分类

| 模式 | 代表产品 | 适用场景 |
|---|---|---|
| **剪贴板粘贴**（Ctrl+V/Alt+V） | Claude Code、Aider、Cline、Continue.dev、Copilot | 截图、IDE 截图、跨应用拷贝 |
| **拖放**（OS 文件管理器 → chat） | Claude Code、Cursor、Copilot、Windsurf、Cline | 桌面产品标配 |
| **`@-mention` 引用**（@file/@dir/@function） | Claude Code、Cursor、Cody、Windsurf、Continue.dev | IDE/桌面产品标配 |
| **Read 工具自动调用** | Claude Code、Cursor | CLI/TUI 主流；模型主动调 |
| **Slash command**（`/image`、`/paste`、`/voice`） | Claude Code、Aider、Cline | CLI 标配 |
| **粘贴自动检测**（基于 MIME） | Shannon（`/image paste`）、Claude Code | TUI 自动路由 |
| **附件 API**（HTTP multipart） | Shannon（`MessageAttachment`）、Copilot（paperclip）、ChatGPT Desktop | 桌面/Web 通用 |
| **远程/移动附件持久化** | Claude Code（Remote Control）、Codex（`--bg`）、Manus（Web） | 跨设备同步 |

### 4.2 输出类 UI 模式分类

| 模式 | 代表产品 | 适用场景 |
|---|---|---|
| **终端 inline 预览**（Kitty Graphics/Sixel/iTerm2） | Shannon（`terminal_image.rs`）、Claude Code | TUI 内联 |
| **Gallery/侧边栏** | Cursor（`assets/` 目录）、Codex、Manus | 桌面/Web |
| **产物落盘 + 会话引用** | Cursor、WorkBuddy（落盘交付物）、Hermes（Artifacts 画廊） | 跨会话复用 |
| **自动 commit / git 版本化** | Aider（每次修改自动 commit） | CLI 工程化 |
| **审批/接管卡**（once/always/deny） | Grok Bot、Codex（`/permissions`） | 高成本/敏感操作 |
| **队列/并发**（multiple jobs） | Codex（review queue）、Grok Bot（50 bot roster） | 多任务 |

### 4.3 原理层：多模态的底层流程

**图像理解（Vision）通用流程**：
1. **输入采集**：UI 层（剪贴板/拖放/@-mention）→ 拿到文件路径或 URL 或 base64
2. **预处理**：MIME 嗅探（PNG/JPEG/GIF/WEBP/HEIC/HEIF...）、尺寸检查（Claude 自动降采样到 2000px；Anthropic/OpenAI 限制 5MB）、格式转换（BMP/SVG 警告）
3. **编码**：base64 编码（如本地）或保留 URL（如远程）
4. **模型序列化**：按 provider 格式包装——Anthropic `Image { source: ImageSource::base64 }`；OpenAI `image_url`；Google `inline_data`
5. **Token 预算**：图像按 token 计（Anthropic 约 1000-2000 tokens/张，OpenAI 按 tile 计算）；Shannon 按 100 tokens/block 估算（`streaming.rs:77`）
6. **持久化**：保存到 message history；超 32MB 会话压缩剥离；超 2000px 历史自动剥离
7. **重试/容错**：损坏图像文本占位符、MIME 不支持 → 落盘引用

**图像生成（Image Gen）通用流程**：
1. **触发**：UI 层（slash `/image-gen <prompt>`、菜单 "Insert image"、@-mention 引用 prompt）
2. **Provider 选择**：BYOK 选择（OpenAI gpt-image-1、Anthropic 不支持、Stability/Replicate/Fal.ai/智谱 CogView、混元 Hunyuan-Image、即梦/Seedream、Gemini Image、Cursor 自家、Grok Aurora）
3. **API 调用**：multipart/form-data 或 JSON；携带 prompt + 可选参考图 + size + n
4. **响应处理**：URL（OpenAI 旧）/ base64（gpt-image-1）/ saved asset path（Cursor `assets/`）
5. **本地化**：下载到本地缓存（`~/.shannon/image-gen/` 或工程 `.shannon-assets/`）
6. **UI 渲染**：TUI 用 Kitty/Sixel/half-block；Web 用 `<img src="data:...">`；IDE 侧边栏 Gallery
7. **会话注入**：作为 `ContentBlock::GeneratedImage` 或 message attachment 注入历史
8. **归档/版本化**：可选 git LFS、自动 commit（Aider 模式）

**视频生成（Video Gen）通用流程**：
1. **触发**：slash `/video <prompt>`
2. **Provider**：Sora（OpenAI 闭源）、Runway/Pika（第三方）、Veo（Google）、CogVideoX（智谱）、Seedance（字节）、HunyuanVideo（腾讯）、Wan Animate（参考视频驱动）
3. **异步**：视频生成耗时数分钟，UI 需 polling/webhook/SSE
4. **下载/落盘**：MP4 保存到本地缓存
5. **UI 渲染**：Web `<video>`；TUI 仅显示元数据 + 路径（终端无法播放视频）

**TTS 通用流程**：
1. **触发**：自动（agent 响应后转语音）或手动（`/speak <text>`）
2. **Provider**：OpenAI TTS、ElevenLabs、Google TTS、Azure Speech、AWS Polly、智谱 GLM-TTS、混元 TTS、浏览器 Web Speech（Shannon 现状）
3. **流式/批量**：流式（首字节立即播放）或批量（完整生成后播放）
4. **UI 集成**：播放/暂停/进度条；与 chat message 关联

### 4.4 跨产品的统一抽象（Shannon 视角）

```rust
// 引擎层多模态统一抽象（推荐设计）
trait MultimodalTool {
    fn input_modality() -> Modality; // Vision/PDF/Audio
    fn output_modality() -> Modality; // Text/Image/Video/Audio
    fn execute(&self, input: ToolInput) -> Result<ToolOutput>;
    fn cost_estimate(&self, input: &ToolInput) -> Cost;
    fn preview(&self, output: &ToolOutput) -> PreviewHandle; // TUI/Web/Mobile 适配
}

// Provider 层多模态路由
trait MultimodalProvider {
    async fn generate_image(&self, req: ImageGenRequest) -> Result<ImageOutput>;
    async fn generate_video(&self, req: VideoGenRequest) -> Result<VideoOutput>;
    async fn text_to_speech(&self, req: TtsRequest) -> Result<AudioOutput>;
    async fn speech_to_text(&self, req: SttRequest) -> Result<TextOutput>;
    async fn ocr(&self, req: OcrRequest) -> Result<TextOutput>;
    async fn parse_document(&self, req: DocParseRequest) -> Result<StructuredDoc>;
}
```

---

## 5. Shannon 当前实现快照

### 5.1 理解类（Input Multimodality）

| 能力 | 状态 | 实现位置 |
|---|---|---|
| **图像理解（Vision）** | ✅ 完整 | `crates/shannon-tools/src/image_analysis.rs`（731 行，`AnalyzeImageTool`）、`crates/shannon-ui/src/repl/at_reference.rs`、`crates/shannon-commands/src/builtin/image.rs`（`/image` + aliases `/img`/`/screenshot`）、`crates/shannon-ui/src/repl/commands/media.rs`（Ctrl+V/url/path）、`crates/shannon-server/src/routes/mod.rs:25-135`（HTTP API `MessageAttachment`）、`crates/shannon-tools/src/computer_use.rs`（Computer Use 截图）、`crates/shannon-ui/src/terminal_image.rs`（Kitty/Sixel/iTerm2/half-block） |
| **PDF 阅读** | 🟡 仅文本层 | `crates/shannon-ui/src/repl/at_reference.rs:209-315`（`pdftotext -layout` + `pdfinfo`），截断 50 KiB；扫描件报错无 OCR 回退 |
| **OCR** | ❌ 缺失 | 仅 LLM 视觉替代；无 `tesseract`/`tesseract-rs` 依赖；PDF 报错 `crates/shannon-ui/src/repl/at_reference.rs:246` |
| **Office 文档解析**（docx/xlsx/pptx） | ❌ 缺失 | 无 `docx`/`xlsx`/`pptx`/`calamine`/`umya-spreadsheet` crate；二进制文件 fallback 文本模式（乱码） |
| **语音输入（STT）** | ✅ 三路径 | CLI：`crates/shannon-ui/src/voice.rs`（`whisper` CLI shell-out + `MockVoiceInput`）<br>桌面云：`desktop/src/commands_voice.rs:69-161`（Groq whisper-large-v3 / OpenAI whisper-1 / custom）<br>桌面本地：`desktop/src/commands_voice.rs:360-770`（whisper-rs 1.5.x，hound WAV，Greedy）<br>UI：`desktop/ui/src/lib/voice/{types,factory,remoteProvider,localProvider,stubProvider}.ts`、`hooks/useVoice.ts`、`VoiceSttSettings.tsx` |
| **语音模式服务**（orchestration） | ✅ 完整 | `crates/shannon-core/src/voice_mode.rs`（1124 行；`VoiceModeService`/`VoiceConfig`/`VoiceStatus`/`VoiceSession`/`KeywordSpotter`；wake words: "hey shannon"、"shannon"） |

### 5.2 生成类（Output Multimodality）

| 能力 | 状态 | 实现位置 |
|---|---|---|
| **图像生成** | ❌ **完全缺失** | 无 `GenerateImage`/`CreateImage`/`dall-e`/`imagen`/`comfyui`/`gpt-image`/`flux`/`firefly` 工具；`crates/shannon-tools/src/lib.rs:38-78` 无图像生成工具；`crates/shannon-commands/src/builtin/` 无 `image-gen`；唯一 `dall-e` 字串在 `token_estimation.rs:77`（定价估算） |
| **视频生成** | ❌ **完全缺失** | 无 `GenerateVideo`/`Sora`/`Runway`/`Pika`/`Veo`/`Kling`/`Luma`；`ContentBlock` 无视频类型 |
| **TTS（云）** | 🟡 仅浏览器 Web Speech | `desktop/ui/src/lib/voice/tts.ts`（139 行，包装 `window.speechSynthesis`）；无云 TTS、无 ElevenLabs/Polly/Azure/OpenAI Audio 集成；CLI/TUI 无 TTS |
| **音乐生成** | ❌ 完全缺失 | 无 `suno`/`udio`/`musicgen` |

### 5.3 自我认知（已有竞品文档已识别）：
- `desktop/docs/product-review/05c-competitive-analysis-2026-06-26.md:15` — "Shannon has no voice mode, no artifact builder, no image generation. This is the single biggest feature gap versus the consumer-facing leaders."（2026-06 已识别，本调研确认仍未补）

### 5.4 Shannon 既有优势（与生成类集成相关）

- **BYOK 多 provider 中立**：`shannon-core` 已支持 Anthropic、OpenAI、Ollama、DeepSeek、Z.ai（GLM Coding Plan）、任何 OpenAI 兼容端点——可直接对接 DALL·E 3、gpt-image-1、CogView、Hunyuan-Image、即梦 Seedream、Gemini Image、Aurora。
- **完整 STT pipeline**：CLI/云/本地三路径——TTS 可参照对称设计（Provider 端点 + 本地 sherpa-onnx + 浏览器 Web Speech 三路径）。
- **Computer Use 截图**：`crates/shannon-tools/src/computer_use.rs` 已支持截图，可扩展为"截图→视觉验证→自动 commit"闭环。
- **Inline 预览基础设施**：`crates/shannon-ui/src/terminal_image.rs` 已支持 Kitty/Sixel/iTerm2/half-block——可直接复用做生成图像的终端预览。
- **附件 API**：`crates/shannon-server/src/routes/mod.rs:25-135` `MessageAttachment` 已支持 image/png+jpeg+webp+gif——可扩展为 video/audio 类型。

---

## 6. User Journey Map（基于现有能力 + 目标生成能力）

> 重构自公开功能事实 + Shannon 自身代码。Persona 设为「**中型团队的全栈工程师 + 知识工作者**」（同时有编码、文档、设计、演示场景），符合 Shannon 的多 provider 中立 BYOK 哲学。

### 6.1 Journey 阶段表

| 阶段 | 触点/界面 | 用户行为 | 心理 | 痛点（基于竞品分析） | Shannon 当前/设计机会 |
|---|---|---|---|---|---|
| **1 认知** | README、竞品对比、产品发布 | 评估"我的 Claude/Cursor 之外是否需要一个开源 BYOK 多模态 Agent？" | 期待成本透明、本地优先 | 编码 Agent 普遍"理解有、生成无"——只能传图不能生图 | 当前 Shannon 已有"理解"但"生成"缺位；需在 README/官网明示"多模态路线图" |
| **2 安装/配置** | CLI 安装 / 桌面下载 | 装 CLI、装桌面 App、配 provider | 期待快速开始 | 配置项复杂（STT/TTS/Provider 多层） | 当前 `VoiceSttSettings.tsx` 已设了 stt 配置模式；G2 可扩展为 `MediaProviderSettings`（image-gen/tts/music） |
| **3 创建项目/会话** | 桌面 welcome / CLI `shannon` | 创建会话、选模型、配 MCP | 掌控感 | 无"项目级生成资产目录"约定（Cursor `assets/`） | G1 设计：每个项目 `.shannon-assets/{images,videos,audio}/` 自动创建 |
| **4 派活（编码/理解）** | chat composer、slash、`@-mention` | 输入任务、引用文件、贴图 | 期待模型快准 | OCR 缺失（扫描 PDF 报错）、Office 文档二进制乱码 | G4（OCR）：PDF 扫描件自动 tesseract 回退；G5（Office 解析）：`.docx/.xlsx/.pptx` 经对应 crate 抽取 |
| **5 派活（生成需求）** | chat composer、slash `/image`、`/video`、`/tts`、`/music` | 让 agent 生成/转换/合成 | "AI 应该懂我" | 当前 100% 空白——所有生成需求只能 MCP 间接或转外部工具 | **P0 G1-G3**：slash 命令 + provider 适配 + inline 预览 + 资产 Gallery |
| **6 审批/接管** | 审批卡、takeover、permission | 高成本操作（生成长视频、付费 TTS）确认 | 安全焦虑 | 成本不可控（视频生成一次可能 $1-5） | 复用既有审批网关：生成操作按 cost_tier 拦（low/medium/high） |
| **7 产物集成** | 自动 commit、Gallery、引用、Artifacts | 产物落到代码、文档、PPT、README | 复利感 | 产物难找、难复用 | G1-G3 设计：产物自动 commit（Aider 模式）+ 项目级 `.shannon-assets/` Gallery + Artifacts 视图（对标 Hermes） |
| **8 跨设备/异步** | mobile pairing、CLI 远程、IM 渠道 | 在路上让 agent 跑任务、收结果 | 自动化收益 | 移动端无生成预览；TTS 在手机无 server-side | 复用 mobile pairing 命令 7 条；G2 TTS 服务端可生成音频文件→手机播放 |
| **9 复盘/成本** | usage 页、/cost、goal budget | 看生成消耗、上下文构成 | 失控感 vs 掌控感 | 视频/图像生成成本高、易失控 | 复用 Hermes 模式：状态栏按类别拆解（image-gen tokens / video-gen $/TTS chars）+ 预算上限 |

### 6.2 阶段-能力-竞品对应

| 阶段 | 必选能力 | 当前/目标 Shannon | 竞品对标 |
|---|---|---|---|
| 2 | 多 provider 配置（image-gen/tts） | ❌ → G2 | Hermes（OpenRouter）、Cursor（自有多模型） |
| 3 | 项目级资产目录 | ❌ → G1 | Cursor `assets/` |
| 4 | OCR / Office 解析 | ❌ → G4/G5 | Copilot PDF/Office |
| 5 | 图像/视频/TTS/音乐生成 | ❌ → G1/G2/G3 | Cursor/GitHub Copilot/ZCode/Hermes/ChatGPT |
| 6 | 审批（按 cost tier） | ✅（复用既有） | Codex `/permissions` |
| 7 | 自动 commit、Gallery、Artifacts | ❌ → G7 | Aider/Hermes/Codex |
| 8 | 移动端预览、TTS 服务端 | 🟡 部分 → G2/G8 | WorkBuddy/Claude Dispatch |
| 9 | 成本可观测（含生成） | 🟡 部分 → G9 | Hermes |

---

## 7. User Stories（按 persona + 验收要点）

### 7.1 P1 编码工程师（含设计/文档混合场景）

**US-1 图像理解（已有，验证）**：
- 我要粘贴截图让 agent 分析错误。
- 验收：`/image` 或 Ctrl+V；base64 编码；按 provider 格式序列化；token 预算计入（100 tokens/block）；终端 inline 预览；损坏图像 → 文本占位符。

**US-2 PDF 阅读（已有，验证）**：
- 我要让 agent 读产品需求 PDF 并生成代码。
- 验收：`@report.pdf` 触发 `pdftotext -layout`；注入代码块；50 KiB 截断；多页码标注；扫描件提示需要 OCR（**G4 补全**）。

**US-3 图像生成（P0 G1）**：
- 我要让 agent 为我的 README 生成 logo 占位图。
- 验收：slash `/image-gen <prompt>`；BYOK 选择 provider（OpenAI gpt-image-1 / 智谱 CogView / 即梦 Seedream / Gemini Image）；产物下载到 `.shannon-assets/images/`；可选参考图上传；终端 inline 预览（Kitty/Sixel/half-block）；桌面 Gallery 显示；自动 commit；下次会话可@-mention 引用。

**US-4 设计稿转换（P0 G1）**：
- 我要让 agent 把手绘草图（PNG）转成 HTML/CSS 代码。
- 验收：图像理解（G1 输入）+ 现有 Write 工具；产物可自动 commit 到 repo。

### 7.2 P2 知识工作者

**US-5 Office 文档读（P0 G5）**：
- 我要让 agent 读客户发来的 docx 报价单并提取关键条款到 CRM。
- 验收：`@quote.docx` 触发 `docx-rs` 解析；段落/表格/列表结构化输出；中文/英文/表格内容可读。

**US-6 xlsx 数据洞察（P0 G5）**：
- 我要让 agent 读 sales.xlsx 并生成图表脚本。
- 验收：`@sales.xlsx` 触发 `calamine`/`umya-spreadsheet`；sheet 名/列/数据预览；agent 调 Python/JS 生成 matplotlib/chart。

**US-7 PPT 生成（P2）**：
- 我要让 agent 把研究纪要生成 PPT。
- 验收：slash `/ppt <topic>`；调 LLM 生大纲；`pptx-rs` 生成 .pptx；产物 `.shannon-assets/ppt/` 落盘；可后续 @-mention 编辑。

**US-8 视频脚本 + 视频生成（P0 G2）**：
- 我要 agent 写脚本 → 用视频模型生成 15s demo 视频。
- 验收：脚本 → `/video <script>`；provider 选 Sora/CogVideoX/Seedance；异步 polling（webhook 或 SSE）；产物 `.shannon-assets/videos/` 落盘；Web `<video>` 预览；TUI 显示路径 + 元数据。

### 7.3 P3 移动/远程用户

**US-9 TTS 听答复（P0 G2）**：
- 我在通勤路上想听 agent 给我读代码变更摘要。
- 验收：移动端 pairing → 桌面 agent 生成 → server-side TTS（OpenAI/ElevenLabs/GLM-TTS）→ mp3 流式回传 → 手机播放；非浏览器 Web Speech。

**US-10 音乐生成（P1 G3）**：
- 我要让 agent 为我的短视频生成背景音乐。
- 验收：slash `/music <prompt>`；provider（Suno/UDIO/魔音 Morlyn）；产物 mp3 + cover；落到 `.shannon-assets/audio/`；Artifaces 画廊可播。

### 7.4 P4 企业管理员

**US-11 多模态预算控制（P0 G9）**：
- 我要限制团队每会话图像/视频/TTS 生成的花费。
- 验收：复用 goal budget cap；按 modality 加预算；超限暂停 + 询问；usage 页按 modality 拆解（图像 tokens / 视频 $/TTS chars/音乐 requests）。

**US-12 多模态 provider 管控（P1 G7）**：
- 我要允许/禁止特定 provider（如禁用 OpenAI 仅留 GLM）。
- 验收：复用 Settings → Providers；image-gen/tts/music 子页勾选；MCP server 配置同款治理。

### 7.5 P5 透明/隐私敏感用户

**US-13 本地化生成（P1 G8）**：
- 我要让图像生成在本地（Stable Diffusion / FLUX.1-schnell 本地）。
- 验收：provider 选 `local:sd`；调 Ollama/Stable Diffusion WebUI/ComfyUI HTTP API；产物本地落盘；不出网。

**US-14 音频文件附件（P2）**：
- 我要把 mp3 拖到 chat 让 agent 转录。
- 验收：`MessageAttachment` 扩展支持 `audio/mpeg`、`audio/wav`、`audio/m4a`、`audio/ogg`；经 STT pipeline；附件元数据显示。

---

## 8. 列表对比分析（深度）

### 8.1 视觉理解能力细分对比

| 产品 | 触发 UI | 格式支持 | 预处理 | 持久化 | 容错 |
|---|---|---|---|---|---|
| Claude Code | Ctrl+V/Alt+V/拖放/Read | PNG/JPEG/GIF/WEBP/PDF | 自动降采样 2000px | 会话历史（超尺寸剥离） | 损坏→占位符、MIME 不支持→落盘引用 |
| Cursor | Cmd+I 侧栏/Read/@-mention | PNG/JPG/GIF/WEBP/SVG | 浏览器截图 + Read | `assets/` 目录 | 浏览器集成错误提示 |
| Copilot | 拖放/剪贴板/右键/paperclip | JPEG/PNG/GIF/WEBP/PDF/HEIC/HEIF | IDE 集成 | 会话历史 | 文件类型错误提示 |
| Hermes | CLI/OpenRouter | 任意（取决于 provider） | provider 路由 | 持久记忆 | provider 回退 |
| Shannon | `/image <path>`/`paste`/`url`/`@-mention`/HTTP `MessageAttachment` | PNG/JPG/JPEG/GIF/WEBP/BMP/ICO/TIFF/SVG | MIME 嗅探，20 MB 限制 | 块级 token 预算（100 tokens/block）；compaction 保留 | MCP SVG 警告、损坏图像→占位符（待验） |

**Shannon 优势**：触发 UI 最完整（CLI/桌面/HTTP/TUI inline 预览）；**短板**：缺 HEIC/HEIF（iOS 用户截图主流）、自动降采样、3 个文件大小限制文档化。

### 8.2 PDF 阅读能力细分对比

| 产品 | 提取方式 | OCR 回退 | 多页 | 表格/结构 | 大文档 |
|---|---|---|---|---|---|
| Claude Code | Read 工具（Anthropic 原生 PDF） | 模型视觉（原生） | ✅ | ✅ | 自动降采样 |
| Cursor | Read 工具（Anthropic 原生） | 模型视觉 | ✅ | ✅ | 自动降采样 |
| Copilot | 拖放（IDE 模型层） | 模型视觉 | ✅ | 🟡 | 🟡 |
| Shannon | `pdftotext -layout` shell-out | ❌ 缺失（显式报错） | ✅（page count from `pdfinfo`） | ❌（仅文本） | 50 KiB 截断 |

**Shannon 优势**：CLI shell-out 透明、本地无需上传；**短板**：依赖外部 `poppler-utils`、扫描 PDF 完全失能、无表格识别。

### 8.3 语音输入能力细分对比

| 产品 | 触发 | 引擎 | 本地/云 | UI |
|---|---|---|---|---|
| Claude Code | 麦克风图标 | 平台内置 | 云 | TUI/IDE |
| Aider | `/voice` | 平台 | 云 | TUI |
| Windsurf | Voice Mode | 平台 | 云 | IDE |
| Shannon (CLI) | TUI 麦克风 | `whisper` CLI shell-out | 本地（外部 Python） | TUI |
| Shannon (桌面云) | MicButton/VoiceOrb | Groq whisper-large-v3 / OpenAI whisper-1 | 云 | 桌面 |
| Shannon (桌面本地) | 同上 | whisper-rs 1.5.x（tiny.en/base/small） | 本地 | 桌面 |

**Shannon 优势**：三路径最完整（CLI/云/本地）、模型管理（`commands_voice_models`）、Settings 卡片统一治理；**短板**：无 wake word 持续监听（CLI）、voice command 词表需扩展。

### 8.4 图像生成能力细分对比

| 产品 | 触发 | Provider | 价格 | 产物 |
|---|---|---|---|---|
| Cursor | 内置命令（具体 slash 未公开） | **Gemini 3 Pro Image Preview** | $0.134/1K 图（图像输出 $120/M tokens） | `assets/` 目录 |
| GitHub Copilot | slash/提示 | **DALL·E 3** / GPT-image | Pro 包含额度 | 附件 |
| Hermes | MCP/OpenRouter 路由 | 多种（OpenRouter/Replicate/Stability） | provider 定价 | 落盘 |
| 智谱 ZCode | Web IDE/CLI | **CogView-3/4** | 按图像计费 | 落盘 |
| 腾讯 WorkBuddy | IDE | 混元 Hunyuan-Image | 套餐含 | 落盘 |
| Shannon | — | — | — | — |

**Shannon 空白**：100% 缺失；最佳差异化机会——BYOK 接入多 provider（OpenAI gpt-image-1、智谱 CogView、即梦 Seedream、Gemini Image、Grok Aurora），统一 UI/CLI 抽象。

### 8.5 视频生成能力细分对比

| 产品 | 触发 | Provider | 异步 | 产物 |
|---|---|---|---|---|
| ChatGPT Desktop | 内置 | **Sora** | 异步（分钟级） | 嵌入 ChatGPT |
| Comfy | 节点工作流 | MiniMax H3 / Seedance / LTX 2.5 / Wan Animate 2 | 异步 | 落盘 |
| 智谱 ZCode | Web IDE/CLI | **CogVideoX** | 异步 | 落盘 |
| 腾讯 WorkBuddy | IDE | 混元 HunyuanVideo | 异步 | 落盘 |
| Shannon | — | — | — | — |

**Shannon 空白**：100% 缺失；**最低成本方案**：仅做"产物落盘 + 元数据存 + Gallery 显示"，不内置视频生成（经 MCP 路由到 provider 即可）。

### 8.6 TTS 能力细分对比

| 产品 | 触发 | Provider | 流式 |
|---|---|---|---|
| ChatGPT Desktop | 内置 | OpenAI GPT-4o audio 6 语音 | ✅ |
| Grok | Voice Mode | xAI | ✅ |
| 智谱 ZCode | Web IDE/CLI | **GLM-TTS** | 🟡 |
| 腾讯 WorkBuddy | IDE | 混元 TTS | 🟡 |
| Comfy | 节点 | MiniMax H3 | 🟡 |
| Hermes | MCP | TTS provider | ✅ |
| Shannon (桌面) | TTS button | **仅浏览器 `window.speechSynthesis`**（OS 语音） | 🟡 |
| Shannon (CLI) | — | ❌ 无 TTS | — |

**Shannon 短板**：仅浏览器内置语音（依赖 OS TTS 引擎，跨平台不一致）、CLI 无 TTS；**机会**：对接 OpenAI TTS、ElevenLabs、Azure Speech、GLM-TTS，按 STT 对称设计。

### 8.7 触发 UI 模式 Shannon 适配性分析

| 模式 | Shannon CLI 适配 | Shannon 桌面适配 | Shannon TUI 适配 |
|---|---|---|---|
| 剪贴板粘贴 | ✅ `pngpaste`/`xclip`/`wl-paste`（已实现 `/image paste`） | ✅ Electron/Tauri `navigator.clipboard` | ✅ 同 CLI |
| 拖放 | ❌（终端无） | ✅ Tauri 文件 drop event | ❌ |
| @-mention | ✅ `at_reference.rs`（已实现） | ✅ 复用 | ✅ |
| Read 工具 | ✅ 模型调 | ✅ | ✅ |
| Slash | ✅ 44+70 命令（已实现 `/image`） | ✅ 12 条（仅部分） | ✅ |
| 附件 API | ✅ `MessageAttachment`（已实现） | ✅ | n/a |
| 远程附件 | 🟡 pairing 命令（已实现 7 条） | 🟡 | n/a |

**核心差距**：桌面 slash 12 条 vs CLI 44+70 条——桌面用户无法发现/使用 `/image`、`/image-gen`（G1 后）等生成类能力。

---

## 9. Gap 分析（按 P0/P1/P2 排序）

### G1 图像生成（P0，最优先）

**竞品证据**：Cursor（Gemini 3 Pro Image Preview $0.134/1K）、GitHub Copilot（DALL·E 3）、ZCode（CogView）、Hermes（OpenRouter）、WorkBuddy（混元）；17 款中 6 款已内置，4 款经 MCP/扩展。
**Shannon 现状**：100% 空白。
**设计**：
- 工具层：`crates/shannon-tools/src/image_gen.rs`（参考 `image_analysis.rs` 结构；provider trait `ImageGenProvider`；BYOK 路由 OpenAI gpt-image-1 / 智谱 CogView / 即梦 Seedream / Gemini Image / Grok Aurora / 本地 Stable Diffusion / ComfyUI）
- CLI 命令：`/image-gen <prompt>`，aliases `/img-gen`、`/gen`、`/create-image`；支持参考图（`-r <path>`）；尺寸（`-s 1k|2k|4k`）；数量（`-n 1-4`）。
- TUI 预览：复用 `terminal_image.rs`（Kitty/Sixel/half-block）。
- 桌面 Gallery：Tasks>artifacts 或新页 Artifacts（参考 Hermes Artifacts 画廊）；侧栏 + 网格 + 缩略图 + 元数据（provider/prompt/size/cost/timestamp）。
- HTTP API：`POST /v1/images/generations`（OpenAI 兼容）。
- 工程指标：CLI slash 增至 13+；测试覆盖生成/错误/取消/预算。
- 差异化机会：产物 `.shannon-assets/images/` 自动创建；自动 commit（Aider 模式）；BYOK 选 provider；成本透明（usage 页）。

### G2 TTS 云服务（P0）

**竞品证据**：ChatGPT（GPT-4o audio）、ZCode（GLM-TTS）、WorkBuddy（混元）、Hermes、Comfy；17 款中 8 款已内置；Shannon 仅浏览器 Web Speech。
**Shannon 现状**：浏览器 Web Speech（`window.speechSynthesis`），CLI 无。
**设计**：
- 工具层：`crates/shannon-tools/src/tts.rs`（参考 `commands_voice.rs` 对称设计；provider trait `TtsProvider`；BYOK 路由 OpenAI TTS / ElevenLabs / Azure Speech / GLM-TTS / 混元 TTS / 本地 sherpa-onnx / piper）
- CLI 命令：`/speak <text>`，`/tts <text> <voice>`；自动 follow-up 触发（TTS auto 开关）。
- 桌面集成：voice hook 加 TTS speaker 选项；消息朗读按钮；流式播放。
- HTTP API：`POST /v1/audio/speech`（OpenAI 兼容）。
- 差异化机会：CLI TTS（终端服务器模式 + 本地音频输出）；多语音克隆；中文友好（GLM-TTS 表现优于浏览器 TTS）。

### G3 视频生成（P0，最小可用）

**竞品证据**：ChatGPT（Sora）、Comfy（MiniMax/Seedance/LTX/Wan Animate）、ZCode（CogVideoX）、WorkBuddy（HunyuanVideo）。
**Shannon 现状**：100% 空白。
**设计（最小可用）**：
- 工具层：`crates/shannon-tools/src/video_gen.rs`（provider trait；BYOK 路由 Sora API / CogVideoX / Seedance / HunyuanVideo / 可灵 Kling）
- CLI 命令：`/video <prompt>`；异步 polling（webhook + 桌面 SSE）；产物 `.shannon-assets/videos/` 落盘。
- 桌面预览：Web `<video controls>`；TUI 显示路径 + 元数据 + 缩略图（首帧 PNG）。
- 差异化机会：与 Computer Use 截图/录屏集成——agent 看完自己生成的视频后自我评估。

### G4 OCR（P0，PDF 补完）

**竞品证据**：所有竞品都依赖"模型视觉"作为 OCR；Shannon 缺失导致扫描 PDF 失能。
**设计**：
- 工具层：`crates/shannon-tools/src/ocr.rs`（`OcrTool`；`tesseract-rs` 或 `leptess`；多语言 tesseract 数据；本地推理）
- 集成点：`crates/shannon-ui/src/repl/at_reference.rs:246` 的"appears to contain no extractable text" 错误改为自动 fallback OCR。
- CLI 命令：`/ocr <path>`（可选独立工具）。
- 桌面：拖放扫描 PDF 自动触发 OCR。
- 成本：纯本地、零 token。

### G5 Office 文档解析（P0）

**竞品证据**：Cursor/Claude Code Read 工具支持 PDF（部分支持 docx via 模型）；WorkBuddy 关键词覆盖。
**Shannon 现状**：无 docx/xlsx/pptx crate。
**设计**：
- 工具层：`crates/shannon-tools/src/document.rs`（`docx-rs` 读 docx、`calamine`/`umya-spreadsheet` 读 xlsx、`pptx-rs` 读 pptx）
- `@`-picker：扩展 `AtReferenceKind` 加 Docx/Xlsx/Pptx；结构化输出（段落/表格/幻灯片）。
- 桌面：拖放 `.docx/.xlsx/.pptx` 自动识别。
- 差异化机会：表格语义识别 → 让 agent 直接生成 Python/JS 数据处理代码。

### G6 Music 生成（P1）

**竞品证据**：Comfy MiniMax H3；WorkBuddy 关键词。
**Shannon 现状**：100% 空白。
**设计**：
- 工具层：`crates/shannon-tools/src/music_gen.rs`（provider trait；BYOK 路由 Suno / UDIO / 魔音 Morlyn / Stable Audio）
- CLI 命令：`/music <prompt>`；产物 `.shannon-assets/audio/music/` 落盘 + cover。
- 桌面：播放器组件 + Gallery。
- 优先级：P1（编码场景非必需，但知识工作者场景需要）。

### G7 资产 Gallery 与 Artifacts 视图（P1）

**竞品证据**：Cursor `assets/`、Hermes Artifacts 画廊、Codex review queue、Aider 自动 commit。
**Shannon 现状**：仅 tasks/timeline 路由，缺统一资产视图。
**设计**：新路由 `artifacts`（桌面 + TUI）；按类型（images/videos/audio/docs/code）分组；元数据 + 跳转回会话；Aider 模式自动 commit。

### G8 本地化生成（P1）

**竞品证据**：Stable Diffusion WebUI / ComfyUI / Ollama 支持本地。
**设计**：provider 加 `local:sd` / `local:comfyui` / `local:ollama` 端点；图像生成走本地 HTTP；零 token。

### G9 成本可观测（含生成）（P0）

**竞品证据**：Hermes 按类别拆解上下文 + 缓存命中率（最佳范本）。
**Shannon 现状**：usage 页、/cost、goal budget cap；无生成类成本。
**设计**：
- 状态栏扩展：上下文构成（系统提示/工具定义/技能/记忆/MCP/对话）+ 缓存命中率 + tokens/s + 生成成本（image tokens / video $/TTS chars/music requests）。
- session 预算上限：复用 goal budget cap 机制，扩展到所有生成类。
- 换模型缓存击穿警告：参考 Hermes。

### G10 多 provider 配置 UI（P1）

**竞品证据**：Hermes OpenRouter 路由、Cursor 多模型。
**Shannon 现状**：Settings → Providers 已有。
**设计**：扩展为 Media 子页（image-gen / tts / video / music），按 provider 勾选 + API key + 默认模型。

### G11 Audio 附件（P2）

**Shannon 现状**：`MessageAttachment` 仅支持图片 MIME。
**设计**：扩展支持 `audio/mpeg`、`audio/wav`、`audio/m4a`、`audio/ogg`；经 STT pipeline 自动转录。

### G12 iOS 截图主流格式支持（P2）

**Shannon 现状**：HEIC/HEIF 不在支持列表。
**设计**：扩展 `SUPPORTED_MEDIA_TYPES`；客户端解码为 JPEG（用 `image` crate）。

---

## 10. Shannon 的多模态路线图建议

### 10.1 短-中-长期（按性价比）

| 阶段 | 目标 | 工作量估计 | 商业价值 |
|---|---|---|---|
| **Phase 1 (P0, 4 周)** | G1 图像生成 + G2 TTS 云 + G4 OCR + G5 Office 解析 + G9 成本可观测 | 大 | 高（补最大缺口） |
| **Phase 2 (P1, 3 周)** | G3 视频生成 + G6 音乐生成 + G7 Gallery + G10 多 provider UI | 中 | 中-高（完成生成类全景） |
| **Phase 3 (P2, 2 周)** | G8 本地化 + G11 Audio 附件 + G12 HEIC | 小-中 | 中（差异化补完） |

### 10.2 优先级矩阵

```
        高价值 低成本
            │
            │  ★ G4 OCR（本地、零 token、补 PDF 失能）
   ─────────┼─────────────
            │  ★ G5 Office 解析（crate 成熟）
            │  ★ G9 成本可观测（复用既有 usage）
            │
  低价值    │                  高价值 高成本
            │  ★ G1 图像生成（5+ provider 适配）
            │  ★ G2 TTS 云（对称 STT 设计）
            │
            │  ★ G3 视频生成（最小可用）
            │  ★ G7 Gallery（新路由）
            │
            │
```

### 10.3 Shannon 的多模态定位（差异化）

- **开源 BYOK 多 provider**：与 Cursor（闭源、多模型）、ChatGPT（闭源、单模型）、Copilot（绑定 GitHub）形成对比——Shannon 用户可混搭 OpenAI gpt-image + 智谱 CogView + 本地 Stable Diffusion，按成本/隐私/质量自由组合。
- **本地优先 + 云可扩展**：与 Hermes（云+本地）、Comfy（专业本地）、Manus（云）形成对比——Shannon 的 Tauri+Rust 本地执行 + BYOK 灵活扩展是独有的"工程师友好"定位。
- **CLI+桌面+HTTP 三形态**：与 Cursor（仅 IDE）、ChatGPT Desktop（仅桌面）形成对比——Shannon 既有 TUI 内联预览（Kitty/Sixel/half-block）、又有桌面 Gallery、又有 HTTP API 兼容 OpenAI。
- **可审计 + 可扩展**：所有多模态能力走 `crates/shannon-tools/` 模块 + Provider trait + BYOK 配置——用户可审计、可替换、可扩展（vs Hermes Skills Hub 零审核被安全点名）。

---

## 11. 待核实清单（单一来源或多口径冲突）

| 事项 | 现有口径 |
|---|---|
| Cursor 图像生成具体 UI 触发（slash 名称） | 文档写 "Image generation – create images from text/reference images; saved to `assets/`" 未给 slash 名 |
| Hermes 视频/音乐生成具体 provider | 仅提到"OpenRouter 集成"，未列具体模型 |
| CodeBuddy/WorkBuddy 多模态具体能力 | 官方页面仅关键词覆盖，模型细节未公开 |
| ChatGPT Sora API 开放程度 | Codex CLI 可经 Sora 但 API 直接可用性未核实 |
| 各 provider 2026-09 最新定价 | 多数定价来自 agent 报告；需对照官方页面二次核实 |

---

## 12. 引用来源

### Claude Code
- https://code.claude.com/docs/en/overview
- https://code.claude.com/docs/en/cli-reference
- https://code.claude.com/docs/en/commands
- https://github.com/anthropics/claude-code/blob/main/CHANGELOG.md
- https://claude.com/pricing

### OpenAI Codex / ChatGPT
- https://github.com/openai/codex
- https://chatgpt.com/codex
- https://help.openai.com/en/articles/11369540-codex-in-chatgpt
- https://openai.com/index/introducing-codex/

### Hermes (Nous Research)
- https://hermes-agent.nousresearch.com/
- https://github.com/NousResearch/hermes-agent
- https://nousresearch.com/

### CodeBuddy / WorkBuddy (腾讯)
- https://www.codebuddy.cn/
- https://copilot.tencent.com/

### Z.ai / ZCode / 智谱
- https://zhipuai.cn/zcode
- https://bigmodel.cn
- https://bigmodel.cn/console/zcode
- https://docs.bigmodel.cn/

### Cursor
- https://cursor.com/docs/agent
- https://cursor.com/docs/models

### GitHub Copilot
- https://github.com/features/copilot
- https://github.com/features/copilot/plans
- https://docs.github.com/en/copilot/using-github-copilot/copilot-chat/asking-github-copilot-questions-in-your-ide

### Cody (Sourcegraph)
- https://sourcegraph.com/docs/cody
- https://docs.sourcegraph.com/cody

### Continue.dev
- https://docs.continue.dev/

### Aider
- https://aider.chat/docs/llms.html
- https://aider.chat/docs/usage.html

### Cline
- https://docs.cline.bot/
- https://github.com/cline/cline/blob/main/README.md

### Roo Code
- https://docs.roocode.com/
- https://roocodeinc.github.io/Roo-Code/

### Kilo Code
- https://kilo.ai/docs
- https://kilocode.ai/

### Windsurf / Devin Desktop
- https://docs.windsurf.com/
- https://docs.devin.ai/desktop/chat
- https://docs.devin.ai/desktop/cascade
- https://codeium.com/windsurf

### Grok (xAI)
- https://x.ai/
- https://x.ai/blog/grok-4

### Perplexity
- https://www.perplexity.ai/
- https://www.perplexity.ai/computer

### Manus
- https://manus.im/

### Comfy (ComfyUI)
- https://comfy.org/
- https://docs.comfy.org/

### Tongyi Lingma (通义灵码)
- https://lingma.aliyun.com/

### Kimi (Moonshot)
- https://www.kimi.com/

### Shannon 自身（内部文档与代码）
- docs/competitive-research-2026-09.md
- docs/research/grok-bots-research-2026-09.md
- docs/research/competitive-research-2026-09.md
- desktop/docs/product-review/05c-competitive-analysis-2026-06-26.md

### Shannon 代码路径
- crates/shannon-tools/src/image_analysis.rs
- crates/shannon-tools/src/computer_use.rs
- crates/shannon-tools/src/preview.rs
- crates/shannon-tools/src/lib.rs
- crates/shannon-ui/src/repl/at_reference.rs
- crates/shannon-ui/src/repl/commands/media.rs
- crates/shannon-ui/src/terminal_image.rs
- crates/shannon-ui/src/voice.rs
- crates/shannon-commands/src/builtin/image.rs
- crates/shannon-server/src/routes/mod.rs
- crates/shannon-engine/src/api/types.rs
- crates/shannon-core/src/voice_mode.rs
- desktop/src/commands_voice.rs
- desktop/src/commands_voice_models.rs
- desktop/ui/src/lib/voice/{types,factory,remoteProvider,localProvider,stubProvider,tts}.ts
- desktop/ui/src/hooks/useVoice.ts
- desktop/ui/src/components/settings/VoiceSttSettings.tsx

---

> 调研方法：3 路并行调研（竞品多模态能力矩阵 / Shannon 自身代码盘点 / 已有竞品文档对照），按 17 款产品逐项深析；UI 模式按"剪贴板/拖放/@-mention/Read/Slash/附件 API/远程附件"6 类抽象；原理层按"输入侧流程 + 输出侧流程 + Provider trait"3 层建模；User Journey Map 与 User Stories 基于公开功能事实重构（标注验收要点与可测量标准）。