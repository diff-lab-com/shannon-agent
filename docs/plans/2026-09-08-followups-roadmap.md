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

| # | 项 | 已就位的底座 | 剩余工作 | 触发条件 |
|---|---|---|---|---|
| B1 | **T14-Phase3 收尾：SSH 远端浏览器完整链路**（远端自动启动 Chrome + `ssh -L` 端口转发编排 + 断线重连） | `RemoteBrowserProvider`（CDP 端点 + reachable 探测）+ `chromiumoxide::Browser::connect` 均已合并 | openssh 会话编排、远端环境探测、转发生命周期管理 | 2027 H1；用户提出远端浏览器需求 |
| B2 | **chrome_session 会话层下沉 shannon-remote**（消除 tools↔remote 的 detect 实现重复） | 双侧实现已 1:1 对齐（provider.rs 文档注明） | chromiumoxide 依赖归属重构 + shannon-tools 改为消费 remote 的会话类型 | 与 B1 同批做（B1 必然触及） |
| B3 | **T10-Phase2 Wayland 截图栈**（wlr-screencopy / KDE portal 自实现） | 与 B1 合并执行可共享 Wayland 依赖 | 截图后端替换 + compositor 兼容矩阵测试 | 2027 H1 |
| B4 | **gateway `MediaAttachment`**（IM 渠道收发媒体 → 会话附件） | wire 类型 stub 已存在 | adapter 填充 + 附件管线复用 | 有 IM 渠道用户需求时 |

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
