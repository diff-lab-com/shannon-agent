# 后续规划：无法实现 / 未实施项清单（2026-09-08 落库）

> **来源**: 计算机操控 / 浏览器控制 / 文件上传三个 PR（#73 / #74 / #76，均已合并）实施过程中明确**无法在当前环境实现**或**按决策延后**的事项。本文档是唯一权威跟踪清单——此前散落在各 PR 描述与 QA 文档中的"无法实现"条目以本文为准。
>
> **维护约定**: 每季度评审一次；条目满足触发条件后移入对应季度的实施计划，并从本清单删除。

---

## A. 硬件 / 环境受限（当前无法验证或实现）

| # | 项 | 阻塞原因 | 解锁条件 | 关联 |
|---|---|---|---|---|
| A1 | **T13-T2 macOS AX 适配器完整实现** | 无 Mac 开发机；AXUIElement 行为（TCC 授权流、AXObserver、Electron 树差异）无法在 Linux 验证，盲写风险高 | ① 一台 Mac；② telemetry 显示 macOS 用户占比可观 | `platform_adapter.rs` 的 `MacosAxAdapter` 骨架已合并，实现即插即用 |
| A2 | **applescript 工具 macOS 真机 QA**（T13-T1 的 TCC 授权流 + 真实 osascript 执行） | 同上（无 Mac） | 同上；步骤已备于 [docs/qa/2026-09-07-computer-use-browser-qa-checklist.md](./2026-09-07-computer-use-browser-qa-checklist.md) QA-1 | T13-T1 代码已合并 |
| A3 | **chromiumoxide / computer-use 在 Windows 与 macOS 的编译与运行验证** | 无对应环境 | CI windows/macos job 已覆盖编译（Cross-platform Check windows 项为 dev 基线红、与上游 openssh 依赖相关）；运行验证需真机 | T14 / T10 |
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

## C. 明确不做（留档防重提）

| # | 项 | 原因 |
|---|---|---|
| C1 | **捆绑 Chromium 二进制到安装包** | 团队决策（2026-09-06）：复用系统浏览器，安装包零增重；`/browser doctor` 提供安装指引 |
| C2 | **T13-Tier3 SkyLight private SPI**（macOS 背景虚拟光标） | 依赖 Apple 私有 API，随时可能失效；仅当用户强烈反馈焦点抢占问题时再评估 |
| C3 | **原生 Rust CDP 替代 MCP Playwright 的"二选一"** | 两者共存是设计（MCP=跨 provider 集成层，内嵌=Shannon 自有世界），不互斥 |

## D. 修复建议转交（非本系列范围）

| # | 项 | 现状 |
|---|---|---|
| D1 | **dev 前端 overlay lint 红**（`MigrationWizard.tsx:193` 白名单外 `fixed inset-0`，dev CI "Desktop Unit Tests" job） | dev 上游提交引入；建议转前端 owner（加白名单或改用规范组件） |
| D2 | **goal.rs 的 clippy 告警**（unused imports ×3、unused_mut、never_loop）与 `shannon-ui` 两处 private-interface 告警 | dev 基线预存；15 分钟清理即可 |

---

## 已完成项索引（供对照，勿重复立项）

- T1 直接单测 / T2 REST 错误体 / T3 CI feature 测试 / T4 部分（3 项预存失败修复）—— #73
- T5-B MIME 提示 / T6 桌面端 PDF / T7 附件栏删除 / T8 `--attach` / T9 附件计数 / T11 `/browser uninstall` —— #74
- T10-Phase1 enigo backend / T12 OptionC toolset / T13-T1 AppleScript / T13-T2 地基 / T14 地基 —— #74
- T14 Phase 1+2（chromiumoxide 会话 / 8 工具 / TUI open / press_key / console / full_page / E2E）/ Phase 3（providers + DynamicWorld + dispatch QA）—— #76
- T15 架构基线 —— #73（已验证干净）
