# 后续规划：无法实现 / 未实施项清单（2026-09-08 落库）

> **来源**: 计算机操控 / 浏览器控制 / 文件上传三个 PR（#73 / #74 / #76，均已合并）实施过程中明确**无法在当前环境实现**或**按决策延后**的事项。本文档是唯一权威跟踪清单——此前散落在各 PR 描述与 QA 文档中的"无法实现"条目以本文为准。
>
> **维护约定**: 每季度评审一次；条目满足触发条件后移入对应季度的实施计划，并从本清单删除。

---

## A. 硬件 / 环境受限（当前无法验证或实现）

| # | 项 | 阻塞原因 | 解锁条件 | 关联 |
|---|---|---|---|---|
| A1 | **T13-T2 macOS AX 适配器完整实现** | ~~无 Mac 开发机~~（**2026-09-10 起已有 Mac**，见 A2）；剩余门槛是 telemetry：AXUIElement 行为（TCC 授权流、AXObserver、Electron 树差异）需真机投入验证，盲写风险仍高 | ① ~~一台 Mac~~ ✅；② telemetry 显示 macOS 用户占比可观 | `platform_adapter.rs` 的 `MacosAxAdapter` 骨架已合并，实现即插即用 |
| A2 | **applescript 工具 macOS 真机 QA**（T13-T1 的 TCC 授权流 + 真实 osascript 执行） | 剩余 4 步需真人操作：TCC 弹窗点击（#2）、权限开关切换（#3）、快捷指令名（#5）、provider + REPL 审批流（#6） | 步骤与复跑命令见 [QA 清单 QA-1](../qa/2026-09-07-computer-use-browser-qa-checklist.md) 与 [2026-09-10 结果文档](../qa/2026-09-10-macos-real-machine-qa-results.md)；#1/#4/#7 已 ✅（harness `tests/macos_real_machine.rs`） | T13-T1 代码已合并 |
| A3 | ~~chromiumoxide / computer-use 在 Windows 与 macOS 的编译与运行验证~~ | **macOS 半边已完成（2026-09-10）**：编译修复（xcap 0.0.13→0.9.8，见 F1）、screenshot/browser E2E 真机通过。**剩余：Windows 真机验证** | Windows 真机 | T14 / T10；macOS 证据见结果文档 |
| A4 | **computer-use libei 后端的 Wayland 真机会话验证**（Portal 授权流 + 原生 Wayland 点击） | 本机无原生 Wayland 会话可自动化；Portal 授权需人工点击 | 带 GNOME-Wayland 的测试机或自托管 runner；步骤见 QA 清单 QA-2 | T10-Phase1 已合并（编译门在 CI） |

## B. 工程量大，按排期延后（有就绪的底座）

> **2026-09-08 更新**：B1-B / B2 / B3 / B4-inbound 已在本轮实施（`feat/followups-b1-b4`，提交 d4f1a183 / c8d7c7f5 / fedf2659）。B1 的自动编排尾巴与 B4 的出站尾巴保留在清单中，触发条件不变。

### 已实施（2026-09-08，待合并）

| # | 项 | 实施内容 | 提交 |
|---|---|---|---|
| B1-B | **`SHANNON_BROWSER_CDP` CDP 附加模式**（评审选定方案 B） | 设置 env 即 connect 远端 CDP 端点（chromiumoxide 自动解析 /json/version），ssh -L 隧道即可用远端浏览器；`/browser doctor` 显示 attach 优先级，CDP 配置时本地浏览器降级为信息项 | d4f1a183 |
| B2 | **浏览器会话层下沉** | 新叶子 crate `shannon-browser`：detect + providers（自 shannon-remote）与 chromiumoxide 会话（自 shannon-tools）合一，消除三份重复；remote/tools 消费同一实现，chromiumoxide 归属唯一化。注：未按原计划做 tools→remote 边翻转（`ToolProviders`/`DenialClassifier` 深植 tools 无法搬移），改提取共享叶 crate，效果等同 | d4f1a183 |
| B3 | **T10-Phase2 Wayland 截图栈** | `computer-use-wayland-capture` feature：wlr-screencopy（wlroots 系，SHM 零拷贝直读）→ xdg-desktop-portal（GNOME/KDE，ashpd）→ xcap（XWayland 回退）；buffer 转换单测覆盖，真机 compositor 矩阵见 QA-5 | c8d7c7f5 |
| B4-in | **gateway 入站媒体管线** | `MessageAttachment` 上收 api-protocol；`QueryRequest`/WS query 帧带 `attachments`，校验逻辑三路合一（core `attachments_to_blocks`）；gateway `router/media.ts` 镜像同规则（image-only、≤8、≤10MiB）；telegram（照片+图片文档，getFile 下载）/ discord（CDN url）/ slack（token 下载）入站媒体 → 引擎附件。出站（引擎图片→IM）未做 | fedf2659 |

### 仍然延后

| # | 项 | 已就位的底座 | 剩余工作 | 触发条件 |
|---|---|---|---|---|
| B1-tail | **远端浏览器自动编排**（B1 方案 A 残余：远端自动启动 Chrome + `ssh -L` 转发生命周期 + 断线重连） | CDP attach 通道已通（B1-B）；`SshRuntime` exec/piped/控制套接字齐备；手动 `ssh -L` + `SHANNON_BROWSER_CDP` 已可用 | openssh 会话编排、远端环境探测、转发进程生命周期、Degraded 联动重连 | 2027 H1；用户提出远端浏览器需求且手动转发嫌麻烦 |
| B4-out | **引擎出站图片 → IM 渠道**（B4 残余） | `SendOpts.attachments` 类型已存在；入站管线已建 | 引擎事件携带图片块 → adapter send 附件（sendPhoto/上传/Block Kit）；各渠道 mediaOut 能力表 | 有 IM 渠道用户需求时 |
| B5 | **Windows 平台的 ssh 远程世界**（openssh 栈 unix-only；2026-09-08 起类型/trait 全平台编译、运行时报 `Unsupported`） | 类型/trait 表面全平台编译（`SshRuntime`/`SshFs`/`SshProcess` stub + provider trait）；Windows 内置 OpenSSH 的 per-command 模式 + 非 mux sftp 子进程的路线在 `ssh/session.rs` 注释已有雏形 | 非 mux 传输实现（逐命令 ssh 子进程）、SFTP 子进程等价物、真机验证 | Windows 桌面用户提出 `/remote use` 需求 |

## C. 明确不做（留档防重提）

| # | 项 | 原因 |
|---|---|---|
| C1 | **捆绑 Chromium 二进制到安装包** | 团队决策（2026-09-06）：复用系统浏览器，安装包零增重；`/browser doctor` 提供安装指引 |
| C2 | **T13-Tier3 SkyLight private SPI**（macOS 背景虚拟光标） | 依赖 Apple 私有 API，随时可能失效；仅当用户强烈反馈焦点抢占问题时再评估 |
| C3 | **原生 Rust CDP 替代 MCP Playwright 的"二选一"** | 两者共存是设计（MCP=跨 provider 集成层，内嵌=Shannon 自有世界），不互斥 |

## D. 修复建议转交（非本系列范围）

| # | 项 | 现状 |
|---|---|---|
| D1 | **dev 前端 overlay lint 红**（`MigrationWizard.tsx:193` 白名单外 `fixed inset-0`，dev CI "Desktop Unit Tests" job） | **已修复（2026-09-08）**：MigrationWizard 迁移至 Modal 原语（与 CancelTaskModal 的 T1.2 路径一致），`ui/modal.tsx` 增加可选 `testId` prop；未动白名单 |
| D2 | **goal.rs 的 clippy 告警**（unused imports ×3、unused_mut、never_loop）与 `shannon-ui` 两处 private-interface 告警 | **已修复（2026-09-08）**：全 workspace 25 处 clippy 告警清零（含 `GuardCounters` 提为 pub、applescript 后端按 `cfg(target_os)` 门控）；Clippy job 转绿 |

## E. macOS 真机验证发现（2026-09-10，见 [结果文档](../qa/2026-09-10-macos-real-machine-qa-results.md)）

| # | 项 | 说明 | 状态 |
|---|---|---|---|
| E1 | **`screen_size()` 静默兜底 (1024,768)** | `Monitor::all()` 失败（如显示器休眠，`CGGetActiveDisplayList` 返回 0）时坐标缩放退化为恒等映射；若 AX 已授权，参考系坐标会被原样当作屏幕坐标点击（错位） | **已修复（2026-09-10）**：改为向上传播错误 + `tracing::warn` |
| E2 | **CI 的 macOS 腿不覆盖 `computer-use` feature** | "Build with computer-use feature" step 在 Linux-only job；`cross-platform` macOS 腿只跑默认 feature `cargo check` → F1 那类 macOS 专属编译损坏在 CI 不可见 | **已修复（2026-09-10）**：`cross-platform` macOS 腿新增 `cargo check -p shannon-tools --features computer-use` |
| E3 | **权限预检缺口的实证** | AX 未授权时 enigo 动作报成功但 CGEvent 静默丢弃（此前仅为注释级认知，现有点击/输入双向证据） | **已修复（2026-09-10，输入侧）**：`platform_adapter::accessibility_granted()`（AXIsProcessTrusted）+ 6 个输入动作预检，未授权时返回可行动错误；真机 harness 已验证。剩余：desktop 端 Info.plist/entitlements 声明 |
| E4 | **存量 clippy 告警在 computer-use 形态下** | `computer_use.rs` format! 风格 ×3、Key clone ×3、landlock unused imports、platform_adapter unneeded return、glob/sandbox 测试散点 | **已清理（2026-09-10）**：双形态 clippy 归零 |
| E5 | **30s 超时与首次 TCC 提示竞争** | 实测：Notes 首次授权若用户未在 30s 内点击，osascript 连同未决提示被超时杀死，报 `timed out after 30s` | **已缓解（2026-09-10）**：超时错误信息附带 TCC 提示指引；根治（放宽/暂停计时）需 TCC 状态内省（无公开 API），随权限预检立项评估 |
| E6 | **macOS /private 路径别名破坏沙箱显示与策略匹配** | `std::fs::canonicalize` 把 /etc、/tmp、/var 解析为 /private/…：denied pattern（/etc/**）失配降级为 outside-roots 错误；bind-alias 显示失配导致错误信息泄漏宿主真实路径；temp 拼写失配使 tmp 排除失效，临时目录下的路径被错误重写为 /workspace。4 个存量单测在 macOS 上失败即源于此 | **已修复（2026-09-10）**：denied pattern 按可见拼写别名匹配；alias 展示匹配 canonical+raw 双拼写；temp 根统一渲染为沙箱可见的 /tmp 拼写；4 个测试修正为拼写无关断言，`file::sandbox::` 53/53 通过 |
| E7 | **残留 9 个存量 macOS 失败（E6 同族，散布在工具回显构造点）** | `file::sandbox_adapter::tests` validate 三件套 + `file::tests` 6 个 alias-echo/snapshot 断言（glob/read/write/multiedit/edit）；基线对比确认与本批改动无关。另：`git::tests` 负载敏感（单测过、并行全量随机失败子集，CI nextest 重试已覆盖） | 逐工具回显点应用与 E6 相同的双拼写/别名展示处理；git 测试考虑 repo 隔离 |

---

## 已完成项索引（供对照，勿重复立项）

- T1 直接单测 / T2 REST 错误体 / T3 CI feature 测试 / T4 部分（3 项预存失败修复）—— #73
- T5-B MIME 提示 / T6 桌面端 PDF / T7 附件栏删除 / T8 `--attach` / T9 附件计数 / T11 `/browser uninstall` —— #74
- T10-Phase1 enigo backend / T12 OptionC toolset / T13-T1 AppleScript / T13-T2 地基 / T14 地基 —— #74
- T14 Phase 1+2（chromiumoxide 会话 / 8 工具 / TUI open / press_key / console / full_page / E2E）/ Phase 3（providers + DynamicWorld + dispatch QA）—— #76
- T15 架构基线 —— #73（已验证干净）
- **macOS 真机验证第一批**（QA-1 #1/#4/#7、screenshot、browser E2E、CI 对齐单测；xcap 0.0.13→0.9.8 编译修复、browser_e2e 探测修复、landlock 非 Linux 测试编译修复、真机 harness `tests/macos_real_machine.rs`）—— 2026-09-10，证据见 [结果文档](../qa/2026-09-10-macos-real-machine-qa-results.md)
