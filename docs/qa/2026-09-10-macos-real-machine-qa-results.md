# macOS 真机验证结果（A2/A3，2026-09-10）

> 执行环境：Mac（darwin 24.5.0 arm64，macOS 15.x），仓库钉住工具链 rustc 1.88.0。
> 关联：[roadmap A1/A2/A3](../plans/2026-09-08-followups-roadmap.md)、
> [QA 清单 QA-1](2026-09-07-computer-use-browser-qa-checklist.md)。
> 本次为首批 macOS 真机验证——此前 CI 的 `computer-use` feature 编译门**只在 Linux 腿运行**
> （`ci.yml` 中该 step 属 Linux job；`cross-platform` 的 macOS/Windows 腿只跑默认 feature 的
> `cargo check`），因此 xcap/enigo 路径从未在 macOS 上编译或运行过。

## 一、结论总览

| 项 | 状态 |
|---|---|
| computer-use / local-browser / preview-capture 编译（tools + cli + desktop） | ✅ 修复后全绿 |
| QA-1 #1 纯 AppleScript（`return 1+1` → `2`） | ✅ |
| QA-1 #4 JXA（`1+1` → `2`） | ✅ |
| QA-1 #7 30s 超时 | ✅（语义修正见 F4） |
| `computer screenshot` 真机截图 + 降采样契约 | ✅（显示器唤醒时） |
| browser 会话层真机 E2E（launch→navigate→快照→截图→click/type→console→key→tabs） | ✅ 真实 Chrome 2.3s |
| CI 对齐单测（`--lib computer`，45 个） | ✅（修复 F3 后） |
| QA-1 #2/#3/#5/#6（TCC 弹窗类 + REPL 审批流） | ⏸ 阻塞：需人点击/授权 + provider |
| `computer type` 输入落屏验证 | ⏸ 阻塞：Accessibility 未授权（无权限失败模式已取证） |

## 二、发现与修复（本次改动）

- **F1（已修复，阻塞级）**：`xcap 0.0.13` 在 rustc 1.88 上 macOS 后端编译失败
  （E0282，`macos/boxed.rs:22` 的 `to_void()` 推断失效）。升级至 **xcap 0.9.8**
  （image 0.25 兼容，`Monitor::width/height` 变为 `XCapResult<u32>`，仍为
  CGDisplayBounds 逻辑点，与 enigo CGEvent 绝对坐标语义一致；新增 `scale_factor()`）。
  适配点：`computer_use.rs::screen_size()` 处理 Result 包装。
- **F2（已修复）**：`tests/browser_e2e.rs` 的跳过探测是手写 Linux-only 路径表，
  导致 macOS 上永远 skip（会话层 `detect_system_browser()` 本身支持
  /Applications）。改为复用会话层探测后，e2e 用本机 Chrome 原生跑通。
- **F3（已修复）**：`landlock_backend.rs::non_linux_probe_fails_closed`
  （`#[cfg(not(target_os = "linux"))]`）在任何 CI 腿都未编译过，macOS 首编即炸
  （E0277，`expect_err` 需要 Ok 侧 Debug）。改用 let-else 断言。
- **F4（语义澄清）**：QA-1 #7 预期 "is_error 含 timed out"，实际超时以
  `ToolError::ExecutionFailed("osascript: timed out after 30s")` 返回（非零退出路径
  才走 `is_error` ToolOutput）。对模型两者都呈现为工具失败，语义等价；测试按实际
  契约断言。
- **F5（新缺口，未修，建议立项）**：`screen_size()` 在 `Monitor::all()` 失败时
  **静默兜底 (1024,768)**，坐标缩放退化为恒等映射——若此时 Accessibility 已授权，
  参考系坐标会被原样当作屏幕坐标点击（错位点）。建议改为向上传播错误（或至少
  tracing::warn）。本次未修，见 roadmap 新条目。
- **F6（取证）**：Accessibility 未授权时 enigo 动作**报告成功但 CGEvent 被静默丢弃**
  （click 返回 ok；type 由 TextEdit 读回验证未落屏）。坐实了 "无权限预检" 缺口的
  实际表现，为权限预检/引导功能提供了确切证据。
- **F7（取证）**：显示器休眠时 `CGGetActiveDisplayList` 返回 0 → 截图以干净错误
  `ExecutionFailed("No monitors found")` 失败（screencapture 此时出黑帧，二者行为
  分叉）。无 panic、无挂起，可接受。
- **F8（新缺口，未修，建议立项）**：工具的 30s `SCRIPT_TIMEOUT` 与**首次 TCC 提示的
  人工响应时间竞争**——实测 Notes 首次授权时用户未在 30s 内点击，osascript 连同
  未决的授权提示被一并超时杀死，工具报 `timed out after 30s`。首次调用场景的
  超时应当放宽（或检测到 TCC 提示挂起时暂停计时），否则"第一次用就报超时"会成为
  标准体验。
- **F9（取证）**：`shortcuts list` 正常工作，本机有 3 个个人快捷指令
  （抖音全类型 / 快速记账（合计版本）/ 新建快捷指令）——均含真实副作用，QA-1 #5
  不宜自动执行，保持 `SHANNON_QA_SHORTCUT` env 门控由用户指定。
- **F10（取证）**：全环境无 provider 配置（无 `~/.shannon/`、无 `~/.config/shannon/`、
  shell rc 无 key 导出），REPL 启动即回落 ollama `127.0.0.1:11434` 连接拒绝退出。
  QA-1 #6（REPL High-risk 审批流）与 QA-4 的 `/browser doctor` REPL 行在配置
  provider 前无法执行。

## 三、改动清单

- `crates/shannon-tools/Cargo.toml`：xcap 0.0.13 → 0.9；dev-deps + base64
- `desktop/Cargo.toml`：xcap 同步升 0.9（`preview-capture` feature 编译通过）
- `crates/shannon-tools/src/computer_use.rs`：`screen_size()` 适配 xcap 0.9 Result API
- `crates/shannon-tools/src/sandbox/landlock_backend.rs`：F3 测试修复
- `crates/shannon-tools/tests/browser_e2e.rs`：F2 探测修复
- `crates/shannon-tools/tests/macos_real_machine.rs`：**新增** macOS 真机 QA harness
  （`#![cfg(all(target_os = "macos", feature = "computer-use"))]` + 全部 `#[ignore]`，
  默认构建不受影响）

## 四、复跑方式

```sh
# 全量真机 harness（需要 macOS + computer-use；部分项需要 TCC/AX 授权，见下）
cargo test -p shannon-tools --features computer-use \
  --test macos_real_machine -- --ignored --nocapture

# 浏览器 E2E（自动探测 /Applications 下的 Chrome/Chromium/Edge）
cargo test -p shannon-tools --features local-browser --test browser_e2e -- --nocapture

# 截图取证
SHANNON_QA_DUMP=/tmp/shot.png cargo test -p shannon-tools --features computer-use \
  --test macos_real_machine -- --ignored computer_screenshot

# env 门控项
SHANNON_QA_DENIED_APP=Notes  cargo test ... applescript_denied_app_reports_not_authorized
SHANNON_QA_SHORTCUT=<名称>   cargo test ... applescript_shortcuts_run_named
```

## 五、遗留（需真机人工操作，按 QA-1 步骤）

1. **QA-1 #2 Notes Automation TCC**：运行 `applescript_notes_automation_tcc`，首次弹
   "'<宿主app>'想要控制'Notes'" 时点允许。
2. **QA-1 #3 拒绝场景**：系统设置 → 隐私与安全性 → 自动化 → 关闭 Notes 开关后，
   以 `SHANNON_QA_DENIED_APP=Notes` 重跑（预期 -1743 语义错误）。
3. **QA-1 #5 Shortcuts**：`SHANNON_QA_SHORTCUT=<已有快捷指令名>` 重跑。
4. **QA-1 #6 REPL High-risk 逐次确认**：需先配置模型 provider（本机无
   `~/.shannon/`，默认回落 ollama 127.0.0.1:11434 连接拒绝）。
5. **type 落屏重验**：系统设置 → 隐私与安全性 → 辅助功能 → 给宿主 app（ZCode/终端）
   开关打开后重跑 `computer_type_lands_in_textedit`。
6. **/browser doctor macOS 行（QA-4）**：同需可用 provider 的 REPL。
