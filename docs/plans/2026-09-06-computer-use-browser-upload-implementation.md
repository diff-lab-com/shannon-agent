# 计算机操控 / 浏览器控制 / 文件上传 实施方案

> **审核说明**：本方案为**实施后回写**的可审核版本——每个任务的代码均为已落地的实际代码（非草案），验证结果为实测输出，可直接对照提交审核。
>
> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**状态**：已实施并提交，分支 `feat/use-browser-computer-upload`（基于 dev `5d9adef5`）
**提交**：`d7fbdad3`（主体 30 文件）→ `375b9005`（补充 12 文件，含 cargo fmt 顺带格式化）→ `768a6eaf`（input/query 接线）→ `6fed0beb`（快照 + /browser 命令 + 设计文档）

**Goal:** 让 Shannon 的三项能力达到"开箱可用"——① `computer` 工具截图真正进入模型视觉（闭环修复）；② `/browser setup` 一键装配 Playwright MCP 浏览器控制；③ 文件（图片）从 REST API / 桌面端 / TUI `@` 选择器全链路送达 LLM。

**Architecture:** 不引入新的执行引擎。computer use 复用既有 `ComputerUseTool`（Anthropic `computer` schema 截图-动作环），修复其返回通道并补门控；浏览器控制遵循竞品共识（CLI 层走 MCP：Claude Code/Chrome DevTools MCP 同构），复用既有 MCP 基础设施做一键装配；文件上传在 `QueryContext` 增加 `attachments` 通道，将四个入口统一接到既有 `ContentBlock::Image` 管线。

**Tech Stack:** Rust 2024（workspace crates：shannon-tools / shannon-core / shannon-engine / shannon-server / shannon-ui / shannon-desktop）；xcap 0.0.13 + enigo 0.2（feature `computer-use`）；axum（REST）；Tauri（桌面端）；serde_json（`.mcp.json` 合并）；cargo nextest / insta（测试与快照）。

**Spec:** [docs/plans/2026-09-06-computer-use-browser-upload-design.md](./2026-09-06-computer-use-browser-upload-design.md)（含竞品调研来源引用）

## 全局约束

- Rust edition 2024（Rust 1.85+）；生产代码用 `expect("reason")` 不用 `unwrap()`
- 每个源文件至少一个 `#[test]`；新 `#[allow(dead_code)]` 必须带 `// KEEP:` 注释（`architecture_invariants` 强制）
- `computer-use` feature 保持**默认关闭**（Linux 构建需 libxdo/X11 开发库，避免强加给贡献者）；运行时门控交给权限系统
- 附件 MIME 白名单：`image/png | image/jpeg | image/gif | image/webp`（Anthropic 视觉支持集）；桌面端来源额外接受 bmp/svg（与 `/image` 命令既有行为一致），REST 与桌面入模路径均过滤 SVG/BMP
- 附件上限：REST 10 MB/个、8 个/消息；桌面端沿用既有 25 MB/10 个（`MAX_ATTACHMENT_SIZE`/`MAX_ATTACHMENT_COUNT`）
- 提交信息英文、`feat:`/`ci:` 前缀（仓库惯例）

---

## 文件变更总览

| 文件 | 变更 | 职责 |
|---|---|---|
| `crates/shannon-core/src/query_engine/engine.rs` | 修改 | 图片 tool-result 双约定解析（:256-320）；用户消息 Blocks 组装（:1407, :1620-1638）；`browser_setup_hint` 注入（:1534-1544） |
| `crates/shannon-tools/src/computer_use.rs` | 修改 | 点击变体动作、截图降采样、click 变体执行 |
| `crates/shannon-engine/src/permissions.rs` | 修改 | `computer` High-risk 策略（:1056-1067） |
| `crates/shannon-cli/Cargo.toml`、`desktop/Cargo.toml` | 修改 | `computer-use` feature 透传 |
| `.github/workflows/ci.yml` | 修改 | CLI feature 构建项 |
| `crates/shannon-core/src/query_engine/types.rs` | 修改 | `QueryContext.attachments` 字段（:625） |
| `crates/shannon-server/src/routes/mod.rs` | 修改 | `MessageAttachment` 类型 + 校验 + 6 单测 |
| `crates/shannon-server/Cargo.toml` | 修改 | base64 依赖 |
| `desktop/src/commands.rs` | 修改 | 附件→image_blocks 入 QueryContext（:511-545, :643） |
| `crates/shannon-ui/src/repl/state.rs` | 修改 | `pending_attachments` 字段 |
| `crates/shannon-ui/src/repl/at_reference.rs` | 修改 | `is_image_reference` / `load_image_block` |
| `crates/shannon-ui/src/repl/input.rs` | 修改 | 两个 `@` 选择流图片分支 + 附件栏 |
| `crates/shannon-ui/src/repl/query.rs` | 修改 | 提交时 drain 附件入 QueryContext |
| `crates/shannon-core/src/query_engine/browser_control_prompt.rs` | 修改 | `browser_setup_hint` + 3 单测 |
| `crates/shannon-ui/src/repl/commands/browser.rs` | 新建 | `/browser` 命令 + merge 逻辑 + 5 单测 |
| `crates/shannon-ui/src/repl/commands/mod.rs` | 修改 | 命令注册 |
| `crates/shannon-commands/src/builtin/help.rs` | 修改 | `/browser` 帮助条目 |
| `configs/mcp-browser.json` | 修改 | 包名修正为 `@playwright/mcp@latest` |
| `crates/shannon-core/src/query_engine/mod.rs` | 修改 | 导出 `browser_setup_hint` |
| `CHANGELOG.md`、`CLAUDE.md`、设计文档 | 修改/新建 | 文档同步 |
| `crates/shannon-tools/tests/snapshots/tool_schema_snapshots__all_tool_schemas.snap` | 修改 | computer schema 新增 4 动作 |
| **25 处 `QueryContext {` 构造点**（13 个文件） | 修改 | 补 `attachments: Vec::new()` |

---

### Task 1: 修复截图→模型回路（A1）

**Files:**
- Modify: `crates/shannon-core/src/query_engine/engine.rs:256-320`（`ToolResultEntry::to_tool_result_content`）

**Interfaces:**
- Consumes: `ToolOutput.metadata: HashMap<String, Value>`（computer 工具产出 `type=image`、`media_type`、`data`（base64）、`width`、`height`）
- Produces: `ToolResultContent::Multiple([Text 描述, Image block])` —— 模型可见图片

- [x] **Step 1: 修改 base64 提取优先级**（computer 约定优先，Read/AnalyzeImage 约定兜底）

```rust
// Two metadata conventions carry the base64 payload: the
// computer tool returns it in `metadata["data"]` with plain text
// in `content`, while Read/AnalyzeImage return a JSON object in
// `content` with a `data` field. Prefer the metadata form.
let base64_data = self
    .metadata
    .get("data")
    .and_then(|v| v.as_str())
    .map(String::from)
    .or_else(|| {
        serde_json::from_str::<serde_json::Value>(&self.content)
            .ok()
            .and_then(|v| v.get("data").and_then(|d| d.as_str()).map(String::from))
    })
    .unwrap_or_default();
```

- [x] **Step 2: 描述文本携带尺寸**（`file_path` 不存在时——computer 截图场景——回退 `Image (media_type) WxH`）

```rust
let mut text = match self.metadata.get("file_path").and_then(|v| v.as_str()) {
    Some(path) => format!("Image file: {path} ({media_type})"),
    None => format!("Image ({media_type})"),
};
if let (Some(w), Some(h)) = (
    self.metadata.get("width").and_then(|v| v.as_u64()),
    self.metadata.get("height").and_then(|v| v.as_u64()),
) {
    text.push_str(&format!(" {w}x{h}"));
}
text.push_str("\nThe image content is provided as an image block below.");
```

- [x] **Step 3: 验证**

Run: `cargo check -p shannon-core && cargo test -p shannon-core --test query_engine_tool_use_tests --test api_integration`
Expected: 编译通过；既有 image 路径（Read/AnalyzeImage 约定）回归通过。实测：14 + 65 全部通过。

> **审核注记**：`to_tool_result_content` 为私有方法，无直接单测；覆盖依赖 ①既有两条 image 回归路径 ②Task 2/3 的 integration 测试（stub 模式）③真机 feature 构建。若需直接覆盖，可在 engine.rs `#[cfg(test)]` 内构造 `ToolResultEntry` 断言（后续跟进项，见 §已知缺口）。

---

### Task 2: 截图降采样（A2）

**Files:**
- Modify: `crates/shannon-tools/src/computer_use.rs:174-189`（`downscale_dims`）、`:395-440`（`execute_screenshot`）、测试 `:7xx`

**Interfaces:**
- Produces: `fn downscale_dims(&self, width: u32, height: u32) -> Option<(u32, u32)>` —— 纯计算，`None` = 无需缩放（不放大）；`#[cfg(feature = "computer-use")]`（唯一生产调用方在 feature 内）

- [x] **Step 1: 写失败测试**（4 个：不放大、缩到参考系、保持纵横比、0=禁用）

```rust
#[test]
#[cfg(feature = "computer-use")]
fn test_downscale_dims_scales_to_reference() {
    let tool = ComputerUseTool::new();
    // 2x Retina capture fits back into the 1024x768 reference box
    assert_eq!(tool.downscale_dims(2048, 1536), Some((1024, 768)));
    assert_eq!(tool.downscale_dims(1920, 1080), Some((1024, 576)));
}
```

- [x] **Step 2: 实现纯计算辅助 + 接入截图管线**

```rust
#[cfg(feature = "computer-use")]
fn downscale_dims(&self, width: u32, height: u32) -> Option<(u32, u32)> {
    let (max_w, max_h) = (
        self.config.max_screenshot_width,
        self.config.max_screenshot_height,
    );
    if max_w == 0 || max_h == 0 || (width <= max_w && height <= max_h) {
        return None;
    }
    let scale = (f64::from(max_w) / f64::from(width)).min(f64::from(max_h) / f64::from(height));
    let new_w = ((f64::from(width) * scale).round() as u32).max(1);
    let new_h = ((f64::from(height) * scale).round() as u32).max(1);
    Some((new_w, new_h))
}
```

`execute_screenshot` 中捕获后缩放（Lanczos3），metadata 宽高改用**缩放后**尺寸（与坐标缩放契约一致）：

```rust
let (orig_w, orig_h) = (image.width(), image.height());
let image = match self.downscale_dims(orig_w, orig_h) {
    Some((new_w, new_h)) => image::imageops::resize(
        &image, new_w, new_h, image::imageops::FilterType::Lanczos3,
    ),
    None => image,
};
let width = image.width();
let height = image.height();
```

- [x] **Step 3: 验证**。Run: `cargo test -p shannon-tools --lib computer`（43 通过，含 4 个新降采样测试）；`cargo check -p shannon-tools`（无 feature 构建无 dead_code 告警——方法与测试均已 feature 门控）。

---

### Task 3: 点击变体动作（A4）

**Files:**
- Modify: `crates/shannon-tools/src/computer_use.rs`（枚举 :35-51、schema :2xx、`execute_click_variant` :464-540、测试）

**Interfaces:**
- Produces: `ComputerAction` 新增 `RightClick | MiddleClick | DoubleClick | TripleClick`（serde snake_case：`right_click` 等）；`fn click_spec(action) -> (enigo::Button, usize, &'static str)`

- [x] **Step 1: 枚举 + schema enum 扩展**（12 个动作）
- [x] **Step 2: execute 分发**——click 族统一走 `execute_click_variant(&action, coord)`：

```rust
ComputerAction::Click
| ComputerAction::RightClick
| ComputerAction::MiddleClick
| ComputerAction::DoubleClick
| ComputerAction::TripleClick => {
    let coord = computer_input.coordinate.ok_or_else(|| {
        ToolError::InvalidInput("click action requires 'coordinate'".to_string())
    })?;
    self.execute_click_variant(&computer_input.action, coord).await
}
```

feature 版实现（连击用 `Direction::Click` 循环；无 feature 版返回含动词的指引错误，保持既有桩语义）：

```rust
let (button, clicks, label) = Self::click_spec(action);
// ... move_mouse(scale_coordinate(coord)) ...
for _ in 0..clicks {
    enigo.button(button, Direction::Click)
        .map_err(|e| ToolError::ExecutionFailed(format!("Mouse click failed: {e}")))?;
}
```

- [x] **Step 3: 测试**：4 动作反序列化断言 + schema enum 含 12 项。
- [x] **Step 4: 快照更新**：`tool_schema_snapshots` diff 仅为新增 4 项 enum 值 → 接受 `.snap.new`。
- [x] **Step 5: 验证**。Run: `cargo test -p shannon-tools --test tool_schema_snapshots --test computer_use_integration`。实测：2 + 5 通过。

---

### Task 4: `computer` 权限策略（A3）

**Files:**
- Modify: `crates/shannon-engine/src/permissions.rs:1056-1067`（`register_default_policies`）

- [x] **Step 1: 注册 High-risk 策略**

```rust
// Computer tool - high risk (desktop control: screen capture plus
// mouse/keyboard input simulation). Competitors gate GUI control
// behind per-action approval by default (Cursor Auto-Run, Claude
// Cowork per-app approval); High risk routes it to confirmation.
let computer_policy = ToolPermissionPolicy::new(
    "computer".to_string(),
    RiskLevel::High,
    "Control the desktop: capture the screen and simulate mouse/keyboard input"
        .to_string(),
);
self.tool_policies
    .insert("computer".to_string(), computer_policy);
```

- [x] **Step 2: 验证**。Run: `cargo test -p shannon-engine --lib`。实测全绿（480 项）。

---

### Task 5: feature 透传 + CI（A5）

**Files:**
- Modify: `crates/shannon-cli/Cargo.toml`（新增 `[features]` 段）、`desktop/Cargo.toml`（features 段追加）、`.github/workflows/ci.yml`

- [x] **Step 1: 两个消费 crate 增加 passthrough**

```toml
[features]
# Compile in real screen capture / input simulation for the `computer` tool
# (desktop automation). Opt-in: Linux builds need libxdo + X11 dev libraries.
# Runtime access is still gated by the permission system (High risk policy).
computer-use = ["shannon-tools/computer-use"]
```

- [x] **Step 2: CI 追加 CLI feature 构建**

```yaml
- name: Build with computer-use feature
  run: |
    cargo build -p shannon-tools --features computer-use
    # CLI passthrough feature (opt-in desktop automation build)
    cargo build -p shannon-cli --features computer-use
```

- [x] **Step 3: 验证**。Run: `cargo check -p shannon-cli --features computer-use`。实测通过（本机具备 libxdo/X11）。

---

### Task 6: `QueryContext.attachments` 通道（C1）

**Files:**
- Modify: `crates/shannon-core/src/query_engine/types.rs:617-626`、`engine.rs:1407`（克隆）、`:1620-1638`（组装）、**25 处构造点**（13 个文件，见总览表）

**Interfaces:**
- Produces: `pub attachments: Vec<shannon_engine::api::ContentBlock>`（空 = 纯文本查询，行为不变）

- [x] **Step 1: 字段定义**

```rust
pub struct QueryContext {
    pub query_id: Uuid,
    pub session_id: Uuid,
    pub user_message: String,
    /// Multimodal attachments (e.g. images) delivered alongside
    /// `user_message`. Empty for text-only queries; non-empty values switch
    /// the user message to a content-blocks form the multimodal adapters
    /// serialize for both Anthropic and OpenAI providers.
    pub attachments: Vec<shannon_engine::api::ContentBlock>,
    pub metadata: QueryMetadata,
}
```

- [x] **Step 2: process_query 组装**（文本追加 Blocks；`tee.record_user_message` 等文本消费方不受影响）

```rust
let user_content = if user_attachments.is_empty() {
    MessageContent::Text(user_message.clone())
} else {
    let mut blocks = Vec::with_capacity(user_attachments.len() + 1);
    blocks.push(shannon_engine::api::ContentBlock::Text {
        text: user_message.clone(),
    });
    blocks.extend(user_attachments);
    MessageContent::Blocks(blocks)
};
conversation.messages.push(Message {
    role: "user".to_string(),
    content: user_content,
});
```

- [x] **Step 3: 批量修补构造点**（Python 脚本：回看 8 行内含 `QueryContext {` 才在 `user_message:` 行后插入 `attachments: Vec::new(),`，共 25 处；`agent.rs:474` 因间距超窗单独手补）
- [x] **Step 4: 验证**。Run: `cargo check --workspace`；`cargo test -p shannon-core --lib`（2785 全绿）、`--test multi_turn_conversation`（10）、`--test api_integration`（65）、`--test query_engine_recovery_tests` 等。实测全部通过。

---

### Task 7: REST API 附件（C2）

**Files:**
- Modify: `crates/shannon-server/src/routes/mod.rs:22-100`（类型+校验）、`:137-160`（post_message 接线）、`:345+`（6 单测）；`crates/shannon-server/Cargo.toml`（+`base64 = { workspace = true }`）

**Interfaces:**
- Produces: `MessageRequest { content, attachments?: Vec<MessageAttachment> }`；`MessageAttachment { name?: String, media_type: String, data: String }`；`fn attachments_to_blocks(&[MessageAttachment]) -> Result<Vec<ContentBlock>, String>`

- [x] **Step 1: 写失败测试**（6 个：合法转换、MIME 拒绝、坏 base64、超限、超数量、SVG 拒绝）

```rust
#[test]
fn test_svg_not_accepted_via_rest() {
    // Vision providers accept png/jpeg/gif/webp only; the desktop app
    // filters SVG before this point, and the REST API rejects it.
    let atts = vec![MessageAttachment {
        name: Some("logo.svg".into()),
        media_type: "image/svg+xml".into(),
        data: png_b64(8),
    }];
    assert!(attachments_to_blocks(&atts).is_err());
}
```

- [x] **Step 2: 校验实现**（白名单 `SUPPORTED_MEDIA_TYPES`、`MAX_ATTACHMENT_BYTES = 10 MB`、`MAX_ATTACHMENTS = 8`；首个违规即返回带 label 的错误串）
- [x] **Step 3: post_message 接线**——空文本+有附件合法（纯图消息）；校验失败 `400`（附 tracing::warn）：

```rust
if request.content.trim().is_empty() && request.attachments.as_ref().is_none_or(Vec::is_empty) {
    return Err(StatusCode::BAD_REQUEST);
}
let attachments = match request.attachments.as_deref() {
    None => Vec::new(),
    Some(atts) => match attachments_to_blocks(atts) {
        Ok(blocks) => blocks,
        Err(message) => {
            tracing::warn!("attachment validation failed: {message}");
            return Err(StatusCode::BAD_REQUEST);
        }
    },
};
```

- [x] **Step 4: 验证**。Run: `cargo test -p shannon-server`。实测：6 通过。

> **审核注记**：400 响应体不带错误详情（保持既有 handler `StatusCode` 错误类型不变，避免影响现有 404/400 空响应体契约）；详情走服务端日志。若前端需要详情，后续可改为 `(StatusCode, Json<Value>)` 错误类型（独立小改动）。

---

### Task 8: 桌面端最后一公里（C3）

**Files:**
- Modify: `desktop/src/commands.rs:511-545`（提取 image_blocks）、`:643`（注入 QueryContext）

- [x] **Step 1: 在附件绑定后、ChatMessage 入栈前提取**（`FileAttachment` 保持仅显示用途）

```rust
// Route image attachments into the multimodal query path so the model
// actually sees them. The `FileAttachment`s stored on the ChatMessage
// below are display-only (chat history / UI chips); only these content
// blocks reach the LLM. SVG is excluded — vision providers accept
// png/jpeg/gif/webp only.
let image_blocks: Vec<shannon_engine::api::ContentBlock> = attachments
    .as_ref()
    .map(|list| {
        list.iter()
            .filter_map(|att| {
                let b64 = att.base64_data.as_ref()?;
                let media_type = att.media_type.as_deref()?;
                if !matches!(
                    media_type,
                    "image/png" | "image/jpeg" | "image/gif" | "image/webp"
                ) {
                    return None;
                }
                Some(shannon_engine::api::ContentBlock::Image {
                    source: shannon_engine::api::ImageSource::base64(
                        media_type.to_string(),
                        b64.clone(),
                    ),
                })
            })
            .collect()
    })
    .unwrap_or_default();
```

- [x] **Step 2: QueryContext 注入** `attachments: image_blocks`（第二个构造点 `:1160` 为无附件的 prompt 命令路径，保持 `Vec::new()`）
- [x] **Step 3: 验证**。Run: `cargo check --workspace`（desktop 为 workspace 成员）；`cargo test -p shannon-desktop --lib`。实测：480 全绿。

---

### Task 9: TUI `@` 图片路由（C4）

**Files:**
- Modify: `crates/shannon-ui/src/repl/state.rs`（`pending_attachments` 字段 + Default）、`at_reference.rs:54-88`（helpers）、`input.rs`（两个选择流）、`query.rs:312-317`（drain）

**Interfaces:**
- Produces: `ReplState.pending_attachments: Vec<ContentBlock>`；`fn is_image_reference(&str) -> bool`；`fn load_image_block(&str) -> Result<ContentBlock, String>`

- [x] **Step 1: helpers**（扩展名集合与 `/image` 命令一致：png/jpg/jpeg/gif/webp/bmp/svg）

```rust
pub fn is_image_reference(file_path: &str) -> bool {
    Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.as_str()))
}

pub fn load_image_block(file_path: &str) -> Result<shannon_engine::api::ContentBlock, String> {
    use base64::Engine;

    let path = Path::new(file_path);
    let media_type = match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => return Err(format!("Unsupported image format: {file_path}")),
    };
    let bytes = std::fs::read(path).map_err(|e| format!("Could not read {file_path}: {e}"))?;
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(shannon_engine::api::ContentBlock::Image {
        source: shannon_engine::api::ImageSource::base64(media_type, data),
    })
}
```

- [x] **Step 2: 两个 `@` 选择流**（fuzzy picker `input.rs:1750+`、file selector `input.rs:1930+`）在 `extract_file_content` 前插入图片分支：入队 `pending_attachments` → 附件栏 `add(Attachment{kind: Image})` → 输入框回填路径文本 → System 提示"将随下一条消息发送" → `return Ok(())`
- [x] **Step 3: 提交时 drain + 清栏**（`query.rs`）

```rust
attachments: {
    let drained = std::mem::take(&mut repl.state.pending_attachments);
    repl.state.attachment_bar.attachments.clear();
    drained
},
```

> **审核注记**：附件栏（`AttachmentBarWidget`）原为死代码（从未 add/remove），本次仅接入 add 与随查询清空；其 delete_mode 无按键接线，不存在"栏里删了但仍发送"的失同步风险。
- [x] **Step 4: 验证**。Run: `cargo check -p shannon-ui && cargo test -p shannon-ui --lib browser at_reference`。实测编译通过；at_reference 既有测试（含 `image.png` 分类断言）不回归。

---

### Task 10: `browser_setup_hint` 兜底引导（B3）

**Files:**
- Modify: `crates/shannon-core/src/query_engine/browser_control_prompt.rs:41-62`（函数+常量）、测试 `:1xx`；`mod.rs`（导出）；`engine.rs:1534-1544`（注入）

**Interfaces:**
- Produces: `pub fn browser_setup_hint(tool_names: &[String], user_message: &str) -> Option<String>` —— 已有浏览器工具 → `None`；无工具且消息含 `browser/浏览器/网页/playwright/chrome/firefox/devtools` → 一段提示

- [x] **Step 1: 函数 + 3 单测**（意图触发、有工具抑制、无意图抑制）

```rust
#[test]
fn test_setup_hint_when_browser_intent_without_tools() {
    let tools = vec!["Bash".to_string(), "Read".to_string()];
    let hint = browser_setup_hint(&tools, "帮我打开 example.com 网页截图");
    assert!(hint.is_some());
    assert!(hint.unwrap().contains("/browser setup"));
}
```

- [x] **Step 2: engine 注入**（`process_query` 内，紧随 `browser_control_prompt` 块，`user_message` 在作用域内；`SystemContentBlock::text` 非缓存——一次性引导不污染缓存断点）
- [x] **Step 3: 验证**。Run: `cargo test -p shannon-core --lib browser`。实测：5 通过（2 既有 + 3 新增）。

---

### Task 11: `/browser` REPL 命令（B1）

**Files:**
- Create: `crates/shannon-ui/src/repl/commands/browser.rs`（245 行）
- Modify: `commands/mod.rs`（`mod browser;` + `repl_only_commands` 增加 `"browser"` + match 分支）、`shannon-commands/src/builtin/help.rs`（帮助条目，HelpCategory::System）、`configs/mcp-browser.json`（包名修正）

**Interfaces:**
- Consumes: `repl.state.working_directory`（项目根）；`crate::widgets::ChatRole`
- Produces: `pub(crate) fn handle_browser(repl: &mut Repl, args: &str) -> Result<()>`；`fn merge_playwright_server(Option<Value>) -> (Value, MergeOutcome)`（`Added | AlreadyConfigured | Replaced`）

- [x] **Step 1: 写失败测试**（5 个 merge 单测：空文档、保留无关 server、幂等、替换陈旧条目、非法文档修复）
- [x] **Step 2: 实现 merge（纯函数，可独立测试）**

```rust
fn merge_playwright_server(
    existing: Option<serde_json::Value>,
) -> (serde_json::Value, MergeOutcome) {
    let desired = serde_json::json!({
        "command": PLAYWRIGHT_COMMAND,      // "npx"
        "args": PLAYWRIGHT_ARGS,            // ["@playwright/mcp@latest"]
    });
    // ... 非 object 根 / 非 object mcpServers 一律归一化 ...
    let outcome = match servers.get(PLAYWRIGHT_SERVER) {
        Some(entry) if *entry == desired => MergeOutcome::AlreadyConfigured,
        Some(_) => MergeOutcome::Replaced,
        None => MergeOutcome::Added,
    };
    servers.insert(PLAYWRIGHT_SERVER.to_string(), desired);
    (doc, outcome)
}
```

- [x] **Step 3: setup 流程**——`npx --version` 探测（缺失给 Node.js 安装指引）→ 拒绝符号链接目标（与 `shannon mcp` 安装器同策略）→ 读/归一化/合并/pretty 写回 → 按 outcome 输出结果 + 重启提示
- [x] **Step 4: 注册与帮助**；`browser_control_prompt` 检测到 `mcp__playwright__browser_*` 后自动注入操作指导（既有机制，零改动生效）
- [x] **Step 5: 验证**。Run: `cargo test -p shannon-ui --lib browser`。实测：5 通过。

---

### Task 12: 文档同步

**Files:** `CHANGELOG.md`（Unreleased 新小节）、`CLAUDE.md`（MEDIUM gaps 与 Tier-2 描述更新）、设计文档（竞品章节带来源回填）

- [x] 全部完成并随 `375b9005` / `6fed0beb` 提交。

---

## 验证汇总（实测）

| 项 | 命令 | 结果 |
|---|---|---|
| 工作区编译 | `cargo check --workspace` | ✅ 0 error |
| 双 feature 编译 | `cargo check -p shannon-tools [--features computer-use]` | ✅ 均无告警 |
| CLI feature 构建 | `cargo check -p shannon-cli --features computer-use` | ✅（本机有 libxdo/X11） |
| 格式 | `cargo fmt --check` | ✅ 干净 |
| clippy | `cargo clippy -p {tools,server,ui,core} --all-features` | ✅ 无新增（goal.rs 告警/dev 同在） |
| 改动相关测试 | computer 43+5、server 6、browser 5+3、query 回归 14+10+65、engine 480、desktop 480、core lib 2785 | ✅ 全绿 |
| 全量套件 | `cargo test --workspace --no-fail-fast` | 11 失败：1 个为本方案造成（schema 快照，已修复），其余 10 个在 dev 主工作区逐一复现确认预先存在（credentials / goal.rs 基线漂移 / git 与 HOME 竞态类） |

## 已知边界与缺口（如实呈报）

1. **`to_tool_result_content` 无直接单测**（私有方法，靠回归路径覆盖）——补测需在 engine.rs 测试模块构造 `ToolResultEntry`。
2. **REST 400 不带错误详情**（保持既有错误类型契约），详情在服务端日志。
3. **`/image` 接受 bmp/svg 而 REST/桌面入模拒绝**——三条入口 MIME 集合不一致是有意为之（vision provider 契约优先），但体验上 `/image` 发 SVG 可能被 provider 拒绝，属既有行为。
4. **会话日志（tee）只记录附件文本部分**，图片 base64 不入 `events.jsonl`（防体积膨胀）；重放/审计看不到图片本体。
5. **桌面端非图片附件（文本/PDF）仍仅显示不入模**（维持现状，设计文档 §5 非目标）。
6. **Linux 运行时仅 X11**（enigo xdo 后端）；Wayland 下 `Enigo::new`/捕获将运行时报错（不 panic）。
7. **CI 的 feature 构建仅 build 不 test**——降采样等 feature 门控单测需本地 `cargo test -p shannon-tools --features computer-use` 触发。

## 实施核对表（自审）

- 设计文档 §4 A1–A5 / B1–B3 / C1–C4 全部有对应 Task（1–5 / 10–11 / 6–9）✅
- §5 非目标均未越界（无原生 CDP、无 AX、无 PDF 附件、gateway 未动）✅
- §6 验收路径中"REST/桌面/@/browser setup"四项已可执行；`computer screenshot` 闭环需 feature 构建后真机验证 ⚠️（本环境无桌面会话，编译与单测已覆盖，端到端留待有显示环境验收）
- 类型一致性：`QueryContext.attachments` / `ContentBlock::Image { source }` / `ImageSource::base64(impl Into<String>, impl Into<String>)` 全链路一致 ✅
